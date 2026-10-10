mod auth;
mod csv;
mod db;
mod error;
mod filter;
mod geo;
mod routes;
mod state;
mod stats;
mod tracker;
mod util;
mod visitor;

use axum::Router;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use rust_embed::RustEmbed;
use state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// The built frontend, embedded in the binary (and read from disk in
/// debug builds, so a dev server needs no rebuild).
#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Dist;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "data/potoo.db".into());
    let geo_path = std::env::var("GEOIP_DB_PATH").unwrap_or_else(|_| "data/country.mmdb".into());
    // Set GEOIP_URL to another URL, or to an empty string to never
    // download anything.
    let geo_url = match std::env::var("GEOIP_URL") {
        Ok(url) if url.is_empty() => None,
        Ok(url) => Some(url),
        Err(_) => Some(geo::Geo::default_url()),
    };

    let db = db::open(&db_path);
    let geo = Arc::new(geo::Geo::open(&geo_path, geo_url).await);
    let state = AppState {
        db: db.clone(),
        geo: geo.clone(),
        trust_proxy: std::env::var("TRUST_PROXY")
            .map(|v| v == "1")
            .unwrap_or(false),
        throttle: Arc::new(auth::LoginThrottle::default()),
    };

    // Optional data retention: delete hits older than RETENTION_DAYS.
    let retention = std::env::var("RETENTION_DAYS")
        .ok()
        .and_then(|days| days.parse::<i64>().ok())
        .filter(|&days| days > 0);
    if let Some(retention) = retention {
        let db = db.clone();
        purge(&db, retention).await;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(6 * 3_600)).await;
                purge(&db, retention).await;
            }
        });
    }

    // While running, refresh the GeoIP database once a week.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(geo::REFRESH_PERIOD).await;
            geo.refresh().await;
        }
    });

    let app = Router::new()
        .nest("/api", routes::router())
        .fallback(static_handler)
        .route_layer(middleware::from_fn(no_cache))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| panic!("cannot listen on {port}: {e}"));
    println!("potoo listening on http://0.0.0.0:{port}");
    axum::serve(listener, app).await.unwrap();
}

async fn purge(db: &db::Db, retention_days: i64) {
    let cutoff = (Utc::now() - chrono::Duration::days(retention_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let conn = db.lock().await;
    if let Err(e) = conn.execute(
        "DELETE FROM hits WHERE created_at < ?1",
        rusqlite::params![cutoff],
    ) {
        eprintln!("retention purge failed: {e}");
    }
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
