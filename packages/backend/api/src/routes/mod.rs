use crate::error::ErrorBody;
use crate::openapi::ApiDoc;
use crate::state::AppState;
use axum::Json;
use axum::http::StatusCode;
use axum::middleware;
use utoipa::OpenApi;
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

/// Every route of the API with its OpenAPI document, and the layers every
/// request goes through: the server adds the frontend and swagger-ui on top.
pub fn api(state: AppState) -> OpenApiRouter<AppState> {
    // Sessions (tower-sessions, our store) and who is signed in on them
    // (axum-login).
    routes()
        .layer(crate::sessions::layer(&state))
        .layer(middleware::map_response_with_state(
            state.secure_cookies,
            crate::sessions::secure_removals,
        ))
        // Outside the sessions: a forged request is refused before any
        // session is loaded or saved.
        .layer(middleware::from_fn_with_state(state, crate::csrf::protect))
        // Every error answer in the API's own shape, whatever produced it.
        .layer(middleware::map_response(crate::extract::error_bodies))
}

/// The OpenAPI document of the same routes, built with no state (the
/// `specs` binary writes it to files): the layers [`api`] adds change no
/// operation.
pub fn api_doc() -> utoipa::openapi::OpenApi {
    routes().split_for_parts().1
}

/// The tracker script at the root, and everything else under /api.
fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(collect::tracker_router())
        .nest("/api", router())
}

/// Every /api route, with the JSON 404 for unknown paths. The routes are
/// registered together with their `#[utoipa::path]` annotations, so the
/// OpenAPI document is made of exactly what is routed: the paths in the
/// annotations are relative to their module's nest, and the nests below
/// are the namespaces under /api.
fn router() -> OpenApiRouter<AppState> {
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
