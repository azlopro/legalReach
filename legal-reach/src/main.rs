// Enhanced src/main.rs with conflict detection and validation routes
mod config;
mod database;
mod errors;
mod handlers;
mod services;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post, delete},
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
        leads::{
            get_leads, get_lead, get_lead_with_details, get_leads_with_details,
            create_lead, update_lead, bulk_update_leads, 
            import_leads_csv, export_leads_csv, get_lead_stats
        },
        email::{send_emails, get_email_logs, test_email_config, send_leads_to_zapier_bulk},
        health::{health_check, detailed_health_check, readiness_check, liveness_check},
        disputes::{
            get_disputed_leads, get_dispute_stats, resolve_dispute, bulk_resolve_disputes,
            analyze_conflicts, validate_all_emails, get_lead_dispute_details,
            validate_lead_email, delete_disputed_lead
        },
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

    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build the application router
    let app = Router::new()
        // Health check routes (no auth required)
        .route("/health", get(health_check))
        .route("/health/detailed", get(detailed_health_check))
        .route("/health/readiness", get(readiness_check))
        .route("/health/liveness", get(liveness_check))
        
        // Enhanced Lead API routes (auth required)
        .route("/api/leads", get(get_leads).post(create_lead))
        .route("/api/leads/with-details", get(get_leads_with_details))
        .route("/api/leads/stats", get(get_lead_stats))
        .route("/api/leads/import", post(import_leads_csv))
        .route("/api/leads/export", get(export_leads_csv))
        .route("/api/leads/bulk-update", post(bulk_update_leads))
        .route("/api/leads/:id", get(get_lead))
        .route("/api/leads/:id/details", get(get_lead_with_details))
        .route("/api/leads/:id/update", post(update_lead))
        
        // Dispute Management routes (auth required)
        .route("/api/disputes", get(get_disputed_leads))
        .route("/api/disputes/stats", get(get_dispute_stats))
        .route("/api/disputes/analyze", post(analyze_conflicts))
        .route("/api/disputes/validate-emails", post(validate_all_emails))
        .route("/api/disputes/:id/resolve", post(resolve_dispute))
        .route("/api/disputes/:id", delete(delete_disputed_lead))
        .route("/api/disputes/bulk-resolve", post(bulk_resolve_disputes))
        .route("/api/disputes/leads/:id/details", get(get_lead_dispute_details))
        .route("/api/disputes/leads/:id/validate", post(validate_lead_email))
        
        // Email routes (auth required)
        .route("/api/email/send", post(send_emails))
        .route("/api/email/test", post(test_email_config))
        .route("/api/email/send-to-zapier", post(send_leads_to_zapier_bulk))
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
    tracing::info!("  Enhanced Leads API:");
    tracing::info!("    GET/POST /api/leads - Basic lead operations");
    tracing::info!("    GET /api/leads/with-details - Leads with conflict/validation details");
    tracing::info!("    GET /api/leads/:id/details - Single lead with full details");
    tracing::info!("    POST /api/leads/import - Enhanced CSV import with conflict detection");
    tracing::info!("    GET /api/leads/export - Enhanced CSV export");
    tracing::info!("  Dispute Management API:");
    tracing::info!("    GET /api/disputes - Get disputed leads");
    tracing::info!("    GET /api/disputes/stats - Dispute and validation statistics");
    tracing::info!("    POST /api/disputes/analyze - Analyze existing leads for conflicts");
    tracing::info!("    POST /api/disputes/validate-emails - Validate all emails");
    tracing::info!("    POST /api/disputes/:id/resolve - Resolve specific dispute");
    tracing::info!("    POST /api/disputes/bulk-resolve - Bulk resolve disputes");
    tracing::info!("    GET /api/disputes/leads/:id/details - Get dispute details");
    tracing::info!("    POST /api/disputes/leads/:id/validate - Validate specific email");
    tracing::info!("  Email API:");
    tracing::info!("    POST /api/email/send - Send emails to leads");
    tracing::info!("    POST /api/email/send-to-zapier - Bulk send to Zapier");
    tracing::info!("    POST /api/email/test - Test email configuration");

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
        assert_eq!(config.server.port, 3000);
    }

    #[test]
    fn test_conflict_detection_settings() {
        let settings = crate::database::models::ConflictDetectionSettings::default();
        assert!(settings.enable_duplicate_email_check);
        assert!(settings.enable_same_domain_check);
        assert_eq!(settings.same_domain_threshold, 3);
    }

    #[test]
    fn test_validation_settings() {
        let settings = crate::services::validation_service::ValidationSettings::default();
        assert!(settings.enable_syntax_check);
        assert!(settings.enable_domain_check);
        assert_eq!(settings.timeout_seconds, 10);
    }
}