pub mod leads;
pub mod email;
pub mod health;
pub mod disputes;

use axum::{
    extract::{Request, State},
    http::{HeaderMap},
    middleware::Next,
    response::{IntoResponse},
};
use crate::{errors::AppError, AppState};

// Authentication middleware
pub async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    // Extract API key from Authorization header
    let auth_header = match headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
    {
        Some(header) => header,
        None => return AppError::unauthorized("Missing Authorization header").into_response(),
    };

    if !auth_header.starts_with("Bearer ") {
        return AppError::unauthorized("Invalid Authorization header format").into_response();
    }

    let token = &auth_header[7..]; // Remove "Bearer " prefix
    
    if token != state.config.auth.api_key {
        return AppError::unauthorized("Invalid API key").into_response();
    }

    next.run(request).await
}

// Rate limiting middleware (simple implementation)