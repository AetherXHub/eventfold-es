//! Application-level error type for Tauri command results.

use serde::Serialize;

/// Errors returned by Tauri command handlers.
///
/// Each variant maps to a distinct error category that the frontend can
/// inspect via the `kind` field in the JSON representation.
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    /// An error originating from the event store (I/O, concurrency, etc.).
    #[error("store error: {0}")]
    Store(String),

    /// A domain validation failure (empty name, invalid state transition, etc.).
    #[error("validation error: {0}")]
    Validation(String),

    /// The requested entity does not exist.
    #[error("not found: {0}")]
    NotFound(String),
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Store(err.to_string())
    }
}

impl From<eventfold_es::DispatchError> for AppError {
    fn from(err: eventfold_es::DispatchError) -> Self {
        match err {
            // Domain validation errors (command rejected by aggregate logic).
            eventfold_es::DispatchError::Execution(inner) => Self::Validation(inner.to_string()),
            other => Self::Store(other.to_string()),
        }
    }
}
