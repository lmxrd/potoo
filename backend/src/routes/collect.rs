use crate::error::HttpError;
use crate::state::AppState;
use crate::tracker::{GIF, TRACKER_JS};
use crate::visitor::{client_ip, parse_browser, parse_device, parse_os, visitor_hash};
use axum::Router;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

pub fn router() -> Router<AppState> {
    // The tracker sends text/plain via sendBeacon, so no CORS preflight
    // is needed; the wildcard keeps the pixel usable anywhere.
    Router::new()
        .route("/collect", post(collect_post))
        .route("/pixel", get(collect_pixel))
        .layer(CorsLayer::permissive())
}

/// The tracker script, a static file served at /tracker.js with its
/// ending. It belongs at the root: it is the one thing other people's
/// pages load, not part of the dashboard's API.
pub fn tracker_router() -> Router<AppState> {
    Router::new().route("/tracker.js", get(tracker))
}

/// The host part of a URL or bare hostname, lowercased.
fn host_of(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let rest = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.trim().to_lowercase();
    if host.is_empty() { None } else { Some(host) }
}

/// Paths may contain personal data in their query/fragment, so those are
/// dropped.
fn clean_path(raw: Option<&str>) -> String {
    let raw = raw.unwrap_or("/");
    let path = raw.split(['?', '#']).next().unwrap_or("/");
    let path = &path[..path.len().min(500)];
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

/// Only the referring hostname is kept, and never the site itself.
fn clean_referrer(raw: Option<&str>, host: &str) -> Option<String> {
    let referrer = host_of(raw)?;
    if referrer == host {
        return None;
    }
    Some(referrer[..referrer.len().min(253)].to_string())
}

enum Kind {
    Pageview,
    Click,
}

impl Kind {
    fn as_str(&self) -> &'static str {
        match self {
            Kind::Pageview => "pageview",
            Kind::Click => "click",
        }
    }
}

struct Hit {
    host: Option<String>,
    kind: Kind,
    path: Option<String>,
    name: Option<String>,
    referrer: Option<String>,
}

async fn record(
    state: &AppState,
    headers: &HeaderMap,
    addr: Option<std::net::IpAddr>,
    hit: Hit,
) -> Result<(), HttpError> {
    let host = hit
        .host
        .clone()
        .or_else(|| host_of(headers.get("origin").and_then(|v| v.to_str().ok())));
    let Some(host) = host else {
        return Ok(());
    };
    let conn = state.db.lock().await;
    let site: Option<(i64, i64)> = conn
        .query_row(
            "SELECT id, honor_dnt FROM sites WHERE hostname = ?1",
            params![&host],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    drop(conn);
    let Some((site_id, honor_dnt)) = site else {
        return Ok(());
    };
    if honor_dnt == 1
        && (headers.get("dnt").map(|v| v == "1").unwrap_or(false)
            || headers.get("sec-gpc").map(|v| v == "1").unwrap_or(false))
    {
        return Ok(());
    }

    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Some(device) = parse_device(ua) else {
        return Ok(()); // a bot
    };
    let ip = client_ip(headers, addr, state.trust_proxy);

    let (name, referrer) = match hit.kind {
        Kind::Click => (
            hit.name.as_deref().map(|n| {
                let name = n.split(['?', '#']).next().unwrap_or("");
                name[..name.len().min(200)].to_string()
            }),
            None,
        ),
        Kind::Pageview => (None, clean_referrer(hit.referrer.as_deref(), &host)),
    };
    let country = state.geo.lookup(&ip).await;
    let visitor = visitor_hash(&state.db, site_id, &ip, ua).await;

    let conn = state.db.lock().await;
    conn.execute(
        "INSERT INTO hits
           (site_id, kind, path, name, referrer, country, device, browser, os, visitor)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            site_id,
            hit.kind.as_str(),
            clean_path(hit.path.as_deref()),
            name,
            referrer,
            country,
            device.as_str(),
            parse_browser(ua),
            parse_os(ua),
            visitor,
        ],
    )?;
    Ok(())
}

// The beacon endpoint of /api/collect, hit by the tracker script.
async fn collect_post(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, HttpError> {
    #[derive(Deserialize)]
    struct Body {
        #[serde(rename = "type")]
        kind: String,
        host: Option<String>,
        path: Option<String>,
        name: Option<String>,
        referrer: Option<String>,
    }
    if let Ok(hit) = serde_json::from_str::<Body>(&body) {
        let kind = match hit.kind.as_str() {
            "pageview" => Kind::Pageview,
            "click" => Kind::Click,
            _ => return Ok(StatusCode::NO_CONTENT.into_response()),
        };
        record(
            &state,
            &headers,
            Some(addr.ip()),
            Hit {
                host: host_of(hit.host.as_deref()),
                kind,
                path: hit.path,
                name: hit.name,
                referrer: hit.referrer,
            },
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

// No-JS fallback: <img src=".../api/pixel?h=example.com&p=/page">.
// Without `p`/`h`, the Referer header is used when the browser sends it.
async fn collect_pixel(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, HttpError> {
    let referer = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .map(|r| r.to_string());
    let ref_path = referer.as_deref().and_then(|r| {
        let rest = r.split_once("://").map(|(_, rest)| rest).unwrap_or(r);
        rest.find('/').map(|i| rest[i..].to_string())
    });
    record(
        &state,
        &headers,
        Some(addr.ip()),
        Hit {
            host: host_of(query.get("h").map(|s| s.as_str()))
                .or_else(|| host_of(referer.as_deref())),
            kind: Kind::Pageview,
            path: query.get("p").map(|p| p.to_string()).or(ref_path),
            referrer: query.get("r").map(|r| r.to_string()),
            name: None,
        },
    )
    .await?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/gif"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        GIF,
    )
        .into_response())
}

async fn tracker() -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        TRACKER_JS,
    )
        .into_response()
}
