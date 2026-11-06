pub mod auth;
pub mod config;
pub mod csrf;
pub mod csv;
pub mod db;
pub mod error;
pub mod extract;
pub mod filter;
pub mod frontend;
pub mod geo;
pub mod ingest;
pub mod jobs;
pub mod limits;
pub mod oidc;
pub mod openapi;
pub mod profile;
pub mod routes;
pub mod sessions;
pub mod state;
pub mod stats;
pub mod time;
pub mod totp;
pub mod tracker;
pub mod util;
pub mod visitor;

use axum::Router;
use state::AppState;

/// Every route of the API with its OpenAPI document: the tracker script at
/// the root, and everything else under /api. The app serves the routes; the
/// `specs` binary writes the document (`openapi::spec`).
pub fn api() -> utoipa_axum::router::OpenApiRouter<AppState> {
    utoipa_axum::router::OpenApiRouter::with_openapi(openapi::base())
        .merge(routes::collect::tracker_router())
        .nest("/api", routes::router())
}

/// The whole app: the API routes, then the embedded frontend.
pub fn app(state: AppState) -> Router {
    let (mut router, spec) = api().split_for_parts();
    // The API's description (swagger-ui) is served only when asked for (`SWAGGER_UI`).
    if state.swagger_ui {
        router = router.merge(
            utoipa_swagger_ui::SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", spec),
        );
    }
    // Sessions (tower-sessions, our store) and who is signed in on them
    // (axum-login).
    frontend::mount(router)
        .layer(sessions::layer(&state))
        .layer(axum::middleware::map_response_with_state(
            state.secure_cookies,
            sessions::secure_removals,
        ))
        // Outside the sessions: a forged request is refused before any
        // session is loaded or saved.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf::protect,
        ))
        // Every error answer in the API's own shape, whatever produced it.
        .layer(axum::middleware::map_response(extract::error_bodies))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

/// Install ring as the process TLS provider. reqwest is built with
/// `rustls-no-provider` (aws-lc is painful on musl, which the Docker
/// image builds for), so the provider must be installed before the
/// first client is built. Safe to call more than once.
pub fn install_tls_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
    });
}
