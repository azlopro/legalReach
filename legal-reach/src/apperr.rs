use std::fmt;
use tokio_postgres::{Error as PostgresError};

// --- Custom Application Error ---
// This enum represents all possible errors in our application.
#[derive(Debug)]
pub enum AppError {
    /// For errors originating from the database.
    DbError(PostgresError),
    /// For permission-related failures.
    PermissionDenied(String),
}

// Implement the Display trait for nice error messages.
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AppError::DbError(e) => write!(f, "Database Error: {}", e),
            AppError::PermissionDenied(msg) => write!(f, "Permission Denied: {}", msg),
        }
    }
}

// Implement the Error trait.
impl std::error::Error for AppError {}

// Allow converting from a PostgresError into our AppError.
// This lets us use the `?` operator on database calls.
impl From<PostgresError> for AppError {
    fn from(err: PostgresError) -> Self {
        AppError::DbError(err)
    }
}
