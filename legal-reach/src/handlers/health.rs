// Fixed handlers/health.rs
use axum::{
    extract::State, 
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use crate::{errors::AppError, AppState};

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub version: String,
    pub database: DatabaseHealth,
    pub features: FeatureHealth,
}

#[derive(Debug, Serialize)]
pub struct DatabaseHealth {
    pub status: String,
    pub connection_pool: ConnectionPoolHealth,
}

#[derive(Debug, Serialize)]
pub struct ConnectionPoolHealth {
    pub active_connections: usize,
    pub idle_connections: usize,
    pub max_connections: u32,
}

#[derive(Debug, Serialize)]
pub struct FeatureHealth {
    pub email_sending: bool,
    pub csv_import: bool,
    pub csv_export: bool,
}

// GET /health - Basic health check
pub async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "timestamp": chrono::Utc::now()
    })))
}

// GET /health/detailed - Detailed health check
pub async fn detailed_health_check(
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Test database connection
    let database_status = match test_database_connection(&state).await {
        Ok(_) => "healthy",
        Err(_) => "unhealthy",
    };

    let pool_status = state.pool.status();

    let health = HealthResponse {
        status: if database_status == "healthy" { "healthy" } else { "unhealthy" }.to_string(),
        timestamp: chrono::Utc::now(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        database: DatabaseHealth {
            status: database_status.to_string(),
            connection_pool: ConnectionPoolHealth {
                active_connections: pool_status.size,
                idle_connections: pool_status.available,
                max_connections: pool_status.max_size as u32,
            },
        },
        features: FeatureHealth {
            email_sending: state.config.features.enable_email_sending,
            csv_import: true,
            csv_export: true,
        },
    };

    (StatusCode::OK, Json(health))
}

// GET /health/readiness - Kubernetes readiness probe
pub async fn readiness_check(
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Test if the application is ready to serve requests
    match test_database_connection(&state).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({
            "status": "ready",
            "timestamp": chrono::Utc::now()
        }))).into_response(),
        Err(e) => e.into_response(),
    }
}

// GET /health/liveness - Kubernetes liveness probe
pub async fn liveness_check() -> impl IntoResponse {
    // Basic liveness check - just return success if the server is running
    (StatusCode::OK, Json(serde_json::json!({
        "status": "alive",
        "timestamp": chrono::Utc::now()
    })))
}

async fn test_database_connection(state: &AppState) -> Result<(), AppError> {
    let client = state.pool.get().await?;
    client.query_one("SELECT 1", &[]).await?;
    Ok(())
}