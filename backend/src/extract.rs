//! Request extractors that answer in the API's own error shape.

use crate::error::HttpError;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Json, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;

/// A JSON request body: `Content-Type: application/json` and a body that
/// fits `T`. Every endpoint that takes a body uses this, except the
/// tracker's `/api/collect`, which takes `text/plain` as well because
/// `sendBeacon` sends it (a CORS preflight is avoided that way).
///
/// axum's own `Json` is what reads the body, so its limits apply; only its
/// rejections are turned into the `{"error": "..."}` answers the frontend
/// understands: a body of another type is a 415, a body that is not valid
/// JSON for `T` is a 400, one that cannot be read at all (too large) keeps
/// the status axum gives it.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = HttpError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(body)| ApiJson(body))
            .map_err(rejection)
    }
}

fn rejection(rejection: JsonRejection) -> HttpError {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => HttpError {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            message: "Expected a JSON body (Content-Type: application/json)".into(),
        },
        JsonRejection::JsonSyntaxError(_) | JsonRejection::JsonDataError(_) => {
            HttpError::bad_request("Invalid request body")
        }
        other => HttpError {
            status: other.status(),
            message: "The request body could not be read".into(),
        },
    }
}
