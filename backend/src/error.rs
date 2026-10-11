use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// An error answered as `{"error": "..."}` with its status, the shape every
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
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "Unauthorized".into(),
        }
    }

    pub fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: "Forbidden".into(),
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
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<sqlx::Error> for HttpError {
    fn from(e: sqlx::Error) -> Self {
        HttpError::internal(format!("database error: {e}"))
    }
}
