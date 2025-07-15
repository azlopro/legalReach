mod config;
mod database;
mod errors;
mod handlers;
mod services;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Router,
};
use deadpool_postgres::Pool;
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer}, // <-- Import Any
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    config::Settings,
    database::{repository::LeadRepository, create_pool},
    handlers::{
        auth_middleware,
        leads::{
            get_leads, get_lead, create_lead, update_lead, bulk_update_leads, 
            import_leads_csv, export_leads_csv, get_lead_stats
        },
        email::{send_emails, get_email_logs, test_email_config, send_leads_to_zapier_bulk},
        health::{health_check, detailed_health_check, readiness_check, liveness_check},
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

    // **CORRECTED CORS LAYER**
    let cors = CorsLayer::new()
        .allow_origin(Any) // Allow any origin
        .allow_methods(Any) // Allow any method
        .allow_headers(Any); // Allow any header

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
        .route("/api/leads/{id}", get(get_lead))
        .route("/api/leads/{id}/update", post(update_lead))
        
        // Email routes (auth required)
        .route("/api/email/send", post(send_emails))
        .route("/api/email/test", post(test_email_config))
        .route("/api/email/send-to-zapier", post(send_leads_to_zapier_bulk)) // NEW ROUTE
        .route("/api/email/logs/{lead_id}", get(get_email_logs))
        
        // Middleware stack
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors) // Apply the corrected CORS layer
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
    tracing::info!("  Zapier Bulk Send: POST /api/email/send-to-zapier");
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

    #[tokio::test]
    async fn test_app_creation() {
        let config = Settings::default();
        // Simple test to verify the app can be created
        assert_eq!(config.server.port, 3000);
    }
}