use crate::state::AppState;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

pub mod auth;
pub mod collect;
pub mod export;
pub mod sites;
pub mod stats;

/// Every /api route, with the JSON 404 for unknown paths.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .merge(collect::router())
        .merge(export::router())
        .merge(auth::router())
        .nest("/sites", sites::router())
        .nest("/stats", stats::router())
        .fallback(not_found)
}

async fn not_found() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "Not found" })))
}

/// Liveness: 200 OK, for the Docker healthcheck and load balancers.
async fn health() -> &'static str {
    "OK"
}
