use crate::error::ErrorBody;
use crate::state::AppState;
use axum::Json;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;

pub mod api_keys;
pub mod auth;
pub mod collect;
pub mod export;
pub mod health;
pub mod import;
pub mod oidc;
pub mod sites;
pub mod stats;
pub mod totp;

/// Every /api route, with the JSON 404 for unknown paths. The routes are
/// registered together with their `#[utoipa::path]` annotations, so the
/// OpenAPI document is made of exactly what is routed: the paths in the
/// annotations are relative to their module's nest, and the nests below
/// are the namespaces under /api.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(health::router())
        .merge(collect::router())
        .nest(
            "/admin",
            oidc::admin_router()
                .merge(auth::admin_router())
                .merge(totp::admin_router()),
        )
        .nest(
            "/auth",
            auth::router()
                .merge(oidc::router())
                .merge(totp::router())
                .merge(api_keys::router()),
        )
        .nest("/export", export::router())
        .nest("/import", import::router())
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
