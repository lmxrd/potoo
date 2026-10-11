pub mod auth;
pub mod csv;
pub mod db;
pub mod error;
pub mod filter;
pub mod geo;
pub mod ingest;
pub mod profile;
pub mod routes;
pub mod state;
pub mod stats;
pub mod tracker;
pub mod util;
pub mod visitor;

use axum::Router;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use state::AppState;

/// The built frontend, embedded in the binary (and read from disk in
/// debug builds, so a dev server needs no rebuild).
#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Dist;

/// The whole app: the /api routes, then the embedded frontend with the
/// SPA fallback.
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::collect::tracker_router())
        .nest("/api", routes::router())
        .fallback(static_handler)
        .layer(middleware::from_fn(no_cache))
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

/// Everything without its own cache policy answers with no-cache, like
/// the API responses and the shell; the hashed files under /assets are
/// immutable.
async fn no_cache(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response.headers().get(header::CACHE_CONTROL).is_none() {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    }
    response
}

/// The embedded CSR frontend: static files, falling back to index.html
/// for client routes.
async fn static_handler(uri: Uri, _headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (file, cache) = if path.is_empty() {
        ("index.html", "no-cache")
    } else {
        match Dist::get(path) {
            Some(_) if path.starts_with("assets/") => (path, "public, max-age=31536000, immutable"),
            Some(_) => (path, "no-cache"),
            None => ("index.html", "no-cache"),
        }
    };
    match Dist::get(file) {
        Some(content) => {
            let mime = mime_guess::from_path(file).first_or_octet_stream();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, cache),
                ],
                content.data,
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "frontend not built; run: mise run build:frontend",
        )
            .into_response(),
    }
}
