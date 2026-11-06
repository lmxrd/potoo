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
/// frontend call understands. What the client may know is a `Status` with
/// its message; a failure on our side (the database, or anything else) is
/// logged with its causes and answered with a plain 500, so no internal
/// detail reaches the client.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// An answer for the client: the status, and a message fit to show.
    #[error("{status}: {message}")]
    Status { status: StatusCode, message: String },
    /// Too many attempts (see `limits.rs`): try again in this many seconds.
    #[error("too many attempts, retry after {retry_after} s")]
    TooManyRequests { retry_after: u64 },
    /// The database failed.
    #[error("database error")]
    Database(#[from] sqlx::Error),
    /// Something else failed on our side.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl HttpError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self::Status {
            status,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized() -> Self {
        Self::from_status(StatusCode::UNAUTHORIZED)
    }

    pub fn forbidden() -> Self {
        Self::from_status(StatusCode::FORBIDDEN)
    }

    /// Password sign-in is turned off (`OIDC_ONLY`).
    pub fn passwords_off() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "Password sign-in is turned off: use single sign-on",
        )
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "Not found")
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    /// An error that needs no more words than its status: the message is
    /// the status's canonical reason.
    fn from_status(status: StatusCode) -> Self {
        Self::new(status, status.canonical_reason().unwrap_or("Error"))
    }

    /// A failure on our side, with what failed for the log; the client
    /// gets a plain 500.
    pub fn internal(message: impl std::fmt::Display + Send + Sync + 'static) -> Self {
        Self::Internal(anyhow::anyhow!("{message}"))
    }

    /// The status this error is answered with.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Status { status, .. } => *status,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = self.status();
        let error = match self {
            Self::Status { message, .. } => message,
            Self::TooManyRequests { retry_after } => {
                let body = Json(ErrorBody {
                    error: "Too many attempts, try again later".into(),
                });
                return (status, [("retry-after", retry_after.to_string())], body).into_response();
            }
            // `{:#}` is the error with its causes, on one line.
            Self::Database(e) => {
                tracing::error!("database error: {e:#}");
                "Internal server error".to_string()
            }
            Self::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                "Internal server error".to_string()
            }
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body(error: HttpError) -> (StatusCode, String) {
        let response = error.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_failure_on_our_side_says_nothing_of_it() {
        let database = HttpError::from(sqlx::Error::Protocol("secret detail".into()));
        let internal = HttpError::internal("saving failed: secret detail");
        for error in [database, internal] {
            let (status, body) = body(error).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(body, r#"{"error":"Internal server error"}"#);
        }
    }

    #[tokio::test]
    async fn a_client_error_keeps_its_status_and_message() {
        let (status, body) = body(HttpError::conflict("Email already in use")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, r#"{"error":"Email already in use"}"#);
    }
}
