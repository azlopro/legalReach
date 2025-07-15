use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    
    #[error("Database pool error: {0}")]
    DatabasePool(#[from] deadpool_postgres::PoolError),
    
    #[error("Database config error: {0}")]
    DatabaseConfig(#[from] deadpool::managed::CreatePoolError<deadpool_postgres::ConfigError>),
    
    #[error("Validation error: {message}")]
    Validation { message: String },
    
    #[error("Not found: {resource}")]
    NotFound { resource: String },
    
    #[error("Unauthorized: {message}")]
    Unauthorized { message: String },
    
    #[error("Forbidden: {message}")]
    Forbidden { message: String },
    
    #[error("Conflict: {message}")]
    Conflict { message: String },
    
    #[error("Internal server error: {message}")]
    Internal { message: String },
    
    #[error("CSV processing error: {0}")]
    Csv(#[from] csv::Error),
    
    #[error("Email error: {0}")]
    Email(#[from] lettre::error::Error),
    
    #[error("SMTP transport error: {0}")]
    SmtpTransport(#[from] lettre::transport::smtp::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),
    
    #[error("Multipart form error: {0}")]
    Multipart(#[from] axum::extract::multipart::MultipartError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            AppError::Validation { message } => (StatusCode::BAD_REQUEST, message.clone()),
            AppError::NotFound { resource } => (StatusCode::NOT_FOUND, format!("{} not found", resource)),
            AppError::Unauthorized { message } => (StatusCode::UNAUTHORIZED, message.clone()),
            AppError::Forbidden { message } => (StatusCode::FORBIDDEN, message.clone()),
            AppError::Conflict { message } => (StatusCode::CONFLICT, message.clone()),
            AppError::Database(_) | AppError::DatabasePool(_) | AppError::DatabaseConfig(_) => {
                tracing::error!("Database error: {}", self);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
            AppError::Internal { message } => {
                tracing::error!("Internal error: {}", message);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
            _ => {
                tracing::error!("Unexpected error: {}", self);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
        };

        let body = Json(json!({
            "error": {
                "message": error_message,
                "code": status.as_u16()
            }
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

// Helper functions for creating specific errors
impl AppError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation { message: message.into() }
    }
    
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound { resource: resource.into() }
    }
    
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized { message: message.into() }
    }
    
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden { message: message.into() }
    }
    
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict { message: message.into() }
    }
    
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal { message: message.into() }
    }
}