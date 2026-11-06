pub mod auth;
pub mod csrf;
pub mod csv;
pub mod db;
pub mod error;
pub mod extract;
pub mod filter;
pub mod frontend;
pub mod geo;
pub mod ingest;
pub mod oidc;
pub mod openapi;
pub mod profile;
pub mod routes;
pub mod session;
pub mod state;
pub mod stats;
pub mod totp;
pub mod tracker;
pub mod util;
pub mod visitor;

use axum::Router;
use state::AppState;

/// Every route of the API with its OpenAPI document: the tracker script at
/// the root, and everything else under /api. The app serves the routes; the
/// `openapi` binary writes the document (`openapi::spec`).
pub fn api() -> utoipa_axum::router::OpenApiRouter<AppState> {
    utoipa_axum::router::OpenApiRouter::with_openapi(openapi::base())
        .merge(routes::collect::tracker_router())
        .nest("/api", routes::router())
}

/// The whole app: the API routes, then the embedded frontend.
pub fn app(state: AppState) -> Router {
    let (router, _spec) = api().split_for_parts();
    // Sessions (tower-sessions, our store) and who is signed in on them
    // (axum-login). A session is only loaded when a handler asks for it.
    let sessions = tower_sessions::SessionManagerLayer::new(session::Store::new(state.db.clone()))
        .with_name(session::COOKIE)
        .with_path("/")
        .with_http_only(true)
        .with_same_site(tower_sessions::cookie::SameSite::Strict)
        .with_secure(state.secure_cookies);
    let auth =
        axum_login::AuthManagerLayerBuilder::new(session::Backend::new(state.db.clone()), sessions)
            .build();
    frontend::mount(router)
        .layer(auth)
        // Outside the sessions: a forged request is refused before any
        // session is loaded or saved.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf::check,
        ))
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
