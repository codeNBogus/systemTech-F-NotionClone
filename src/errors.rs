use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

use crate::models::ErrorResponse;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Board not found: {0}")]
    BoardNotFound(String),

    #[error("Column not found: {0}")]
    ColumnNotFound(String),

    #[error("Card not found: {0}")]
    CardNotFound(String),

    #[error("Version conflict: expected {expected}, got {actual}")]
    VersionConflict { expected: u64, actual: u64 },

    #[error("Invalid position: {0}")]
    InvalidPosition(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::BoardNotFound(_) => (StatusCode::NOT_FOUND, "BOARD_NOT_FOUND"),
            AppError::ColumnNotFound(_) => (StatusCode::NOT_FOUND, "COLUMN_NOT_FOUND"),
            AppError::CardNotFound(_) => (StatusCode::NOT_FOUND, "CARD_NOT_FOUND"),
            AppError::VersionConflict { .. } => (StatusCode::CONFLICT, "VERSION_CONFLICT"),
            AppError::InvalidPosition(_) => (StatusCode::BAD_REQUEST, "INVALID_POSITION"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = Json(ErrorResponse {
            error: self.to_string(),
            code: code.to_string(),
        });

        (status, body).into_response()
    }
}
