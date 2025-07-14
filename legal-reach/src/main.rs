mod config;
mod database;
mod errors;
mod handlers;
mod services;

use axum::{
    extract::DefaultBodyLimit,
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, Method,
    },
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use deadpool_postgres::Pool;
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    config::Settings,
    database::{repository::LeadRepository, create_pool},
    handlers::{
        auth_middleware,
        leads::*,
        email::*,
        health::*,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub config: Settings,
    pub pool: Pool,
    pub lead_repository: LeadRepository,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "legal_reach=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = Settings::new().map_err(|e| {
        tracing::error!("Failed to load configuration: {}", e);
        e
    })?;

    tracing::info!("Starting Legal Reach API server...");
    tracing::info!("Configuration loaded successfully");

    // Create database pool
    let pool = create_pool(&config).await.map_err(|e| {
        tracing::error!("Failed to create database pool: {}", e);
        e
    })?;

    tracing::info!("Database connection pool created successfully");

    // Test email configuration if enabled
    if config.features.enable_email_sending && !config.email.mock_mode {
        if let Err(e) = services::email_service::test_email_connection(&config).await {
            tracing::warn!("Email connection test failed: {}. Continuing with email disabled.", e);
        }
    }

    // Create repositories
    let lead_repository = LeadRepository::new(pool.clone());

    // Create application state
    let state = AppState {
        config: config.clone(),
        pool,
        lead_repository,
    };

    // Build CORS layer
    let cors = CorsLayer::new()
        .allow_origin(
            config
                .server
                .cors_origins
                .iter()
                .map(|origin| origin.parse::<HeaderValue>())
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|_| vec![Any::default()])
        )
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE]);

    // Build the application router
    let app = Router::new()
        // Health check routes (no auth required)
        .route("/health", get(health_check))
        .route("/health/detailed", get(detailed_health_check))
        .route("/health/readiness", get(readiness_check))
        .route("/health/liveness", get(liveness_check))
        
        // API routes (auth required)
        .route("/api/leads", get(get_leads).post(create_lead))
        .route("/api/leads/stats", get(get_lead_stats))
        .route("/api/leads/import", post(import_leads_csv))
        .route("/api/leads/export", get(export_leads_csv))
        .route("/api/leads/bulk-update", post(bulk_update_leads))
        .route("/api/leads/:id", get(get_lead).put(update_lead).delete(delete_lead))
        
        // Email routes (auth required)
        .route("/api/email/send", post(send_emails))
        .route("/api/email/test", post(test_email_config))
        .route("/api/email/logs/:lead_id", get(get_email_logs))
        
        // Middleware stack
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors)
                .layer(DefaultBodyLimit::max(config.server.max_request_size))
                .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        )
        .with_state(state);

    // Create server address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    
    tracing::info!("Server starting on {}", addr);
    tracing::info!("API Documentation:");
    tracing::info!("  Health Check: GET /health");
    tracing::info!("  Leads API: GET/POST /api/leads");
    tracing::info!("  Email API: POST /api/email/send");
    tracing::info!("  CSV Import: POST /api/leads/import");
    tracing::info!("  CSV Export: GET /api/leads/export");

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(|e| {
            tracing::error!("Server error: {}", e);
            e
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use serde_json::json;

    async fn create_test_app() -> TestServer {
        let config = Settings::default();
        let pool = create_pool(&config).await.unwrap();
        let lead_repository = LeadRepository::new(pool.clone());
        
        let state = AppState {
            config,
            pool,
            lead_repository,
        };

        let app = Router::new()
            .route("/health", get(health_check))
            .with_state(state);

        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_health_check() {
        let server = create_test_app().await;
        
        let response = server.get("/health").await;
        
        assert_eq!(response.status_code(), 200);
        
        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "ok");
    }
}