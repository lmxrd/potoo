use crate::error::{ErrorBody, HttpError};
use crate::ingest::NewHit;
use crate::profile::{Phase, timed};
use crate::state::AppState;
use crate::tracker::{GIF, TRACKER_JS};
use crate::visitor::{client_ip, parse_browser, parse_device, parse_os, visitor_hash};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub fn router() -> OpenApiRouter<AppState> {
    // The tracker sends text/plain via sendBeacon, so no CORS preflight
    // is needed; the wildcard keeps the pixel usable anywhere.
    OpenApiRouter::new()
        .routes(routes!(collect_post))
        .routes(routes!(collect_pixel))
        .layer(CorsLayer::permissive())
}

/// The tracker script, a static file served at /tracker.js with its
/// ending. It belongs at the root: it is the one thing other people's
/// pages load, not part of the dashboard's API.
pub fn tracker_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(tracker))
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
    let site = timed(Phase::Site, state.db.site(&host)).await?;
    let Some(site) = site else {
        return Ok(());
    };
    if site.honor_dnt
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
    let country = timed(Phase::Geo, state.geo.lookup(&ip)).await;
    let visitor = visitor_hash(&state.db, site.id, &ip, ua).await;

    state.ingest.submit(NewHit {
        created_at: crate::auth::now_iso(),
        site_id: site.id,
        kind: hit.kind.as_str(),
        path: clean_path(hit.path.as_deref()),
        name,
        referrer,
        country,
        device: device.as_str().into(),
        browser: parse_browser(ua),
        os: parse_os(ua),
        visitor,
    })
}

/// One hit, as the tracker script sends it.
#[derive(Deserialize, ToSchema)]
pub struct CollectBody {
    /// `pageview` or `click`; a hit of any other type is ignored.
    #[serde(rename = "type")]
    #[schema(example = "pageview")]
    pub kind: String,
    /// The site's hostname; the `Origin` header is used when it is absent.
    pub host: Option<String>,
    /// The page's path. Its query string and fragment are dropped.
    pub path: Option<String>,
    /// The label of a click.
    pub name: Option<String>,
    /// Where the visitor came from. Only the hostname is kept.
    pub referrer: Option<String>,
}

// The beacon endpoint of /api/collect, hit by the tracker script.
#[utoipa::path(
    post,
    path = "/collect",
    tag = "Tracking",
    operation_id = "collectHit",
    summary = "Record a hit (the tracker script)",
    description = "Sent by /tracker.js with sendBeacon: the body is a JSON object sent as text/plain, so that no CORS preflight is needed (application/json is accepted too: this is the only endpoint that takes anything but JSON). Answers 204 for every well-formed request, also for hits that are ignored (bots, Do Not Track, unknown sites). The hit is queued and written within milliseconds; with 20,000 hits waiting the answer is 503.",
    request_body(content((CollectBody = "text/plain"), (CollectBody = "application/json"))),
    responses(
        (status = 204, description = "The hit was accepted, or ignored"),
        (status = 503, description = "Too many hits queued, try again", body = ErrorBody),
    ),
)]
async fn collect_post(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, HttpError> {
    if let Ok(hit) = serde_json::from_str::<CollectBody>(&body) {
        let kind = match hit.kind.as_str() {
            "pageview" => Kind::Pageview,
            "click" => Kind::Click,
            _ => return Ok(StatusCode::NO_CONTENT.into_response()),
        };
        timed(
            Phase::Total,
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
            ),
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

// No-JS fallback: <img src=".../api/pixel?h=example.com&p=/page">.
// Without `p`/`h`, the Referer header is used when the browser sends it.
#[utoipa::path(
    get,
    path = "/pixel",
    tag = "Tracking",
    operation_id = "getPixel",
    summary = "Record a pageview (the no-JS pixel)",
    description = "A transparent 1x1 GIF; requesting it is the visit. Without h and p the site and the path come from the Referer header. Counts pageviews only.",
    params(("h" = Option<String>, Query, description = "The site's hostname"), ("p" = Option<String>, Query, description = "The page's path"), ("r" = Option<String>, Query, description = "The referrer")),
    responses(
        (status = 200, description = "The GIF", body = String, content_type = "image/gif"),
        (status = 503, description = "Too many hits queued, try again", body = ErrorBody),
    ),
)]
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

#[utoipa::path(
    get,
    path = "/tracker.js",
    tag = "Tracking",
    operation_id = "getTrackerScript",
    summary = "The tracker script",
    description = "The cookie-less script a site includes with a single script tag. Options are data attributes on the tag: data-clicks, data-outbound, data-host.",
    responses(
        (status = 200, description = "The script", body = String, content_type = "text/javascript"),
    ),
)]
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
