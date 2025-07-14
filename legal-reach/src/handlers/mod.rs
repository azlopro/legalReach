pub mod leads;
pub mod email;
pub mod health;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use crate::{errors::AppError, AppState};

// Authentication middleware
pub async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    // Extract API key from Authorization header
    let auth_header = headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("Missing Authorization header"))?;

    if !auth_header.starts_with("Bearer ") {
        return Err(AppError::unauthorized("Invalid Authorization header format"));
    }

    let token = &auth_header[7..]; // Remove "Bearer " prefix
    
    if token != state.config.auth.api_key {
        return Err(AppError::unauthorized("Invalid API key"));
    }

    Ok(next.run(request).await)
}

// Rate limiting middleware (simple implementation)
pub async fn rate_limit_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // In a production environment, you would implement proper rate limiting
    // using Redis or an in-memory store. For now, we'll just pass through.
    Ok(next.run(request).await)
}