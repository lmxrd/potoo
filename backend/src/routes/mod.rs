use crate::error::ErrorBody;
use crate::state::AppState;
use axum::Json;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub mod auth;
pub mod collect;
pub mod export;
pub mod sites;
pub mod stats;

/// Every /api route, with the JSON 404 for unknown paths. The routes are
/// registered together with their `#[utoipa::path]` annotations, so the
/// OpenAPI document is made of exactly what is routed: the paths in the
/// annotations are relative to their module's nest, and the nests below
/// are the namespaces under /api.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(health))
        .merge(collect::router())
        .nest("/auth", auth::router())
        .nest("/export", export::router())
        .nest("/sites", sites::router())
        .nest("/stats", stats::router())
        .fallback(not_found)
}

async fn not_found() -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: "Not found".into(),
        }),
    )
}

/// Liveness: 200 OK, for the Docker healthcheck and load balancers.
#[utoipa::path(
    get,
    path = "/health",
    tag = "System",
    operation_id = "getHealth",
    summary = "Liveness",
    description = "200 OK, for the Docker healthcheck and load balancers.",
    responses(
        (status = 200, description = "The server is up", body = String, content_type = "text/plain"),
    ),
)]
async fn health() -> &'static str {
    "OK"
}
