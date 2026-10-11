use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

/// The body of every error response: `{"error": "..."}`.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    /// What went wrong, fit to show to a person.
    pub error: String,
}

/// An error answered as an [`ErrorBody`] with its status, the shape every
/// frontend call understands.
#[derive(Debug)]
pub struct HttpError {
    pub status: StatusCode,
    pub message: String,
}

impl HttpError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn unauthorized() -> Self {
        Self::from_status(StatusCode::UNAUTHORIZED)
    }

    pub fn forbidden() -> Self {
        Self::from_status(StatusCode::FORBIDDEN)
    }

    /// An error that needs no more words than its status: the message is
    /// the status's canonical reason.
    fn from_status(status: StatusCode) -> Self {
        Self {
            status,
            message: status.canonical_reason().unwrap_or("Error").to_string(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for HttpError {
    fn from(e: sqlx::Error) -> Self {
        HttpError::internal(format!("database error: {e}"))
    }
}
