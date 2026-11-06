//! Cross-site request forgery: a request that changes something must come
//! from Potoo's own pages. The session cookie is `SameSite=Strict`, but a
//! sibling subdomain counts as the same *site* (`blog.example.com` next to
//! `potoo.example.com`), and a form there can send a multipart upload
//! (the import) without a CORS preflight. So the browser's Fetch Metadata
//! decides (OWASP's "Fetch Metadata" defence, the rule of Go's
//! `http.CrossOriginProtection`):
//!
//! - GET, HEAD and OPTIONS change nothing and always pass;
//! - `Sec-Fetch-Site: same-origin` (or `none`, typed by the user) passes,
//!   anything else (`same-site`, `cross-site`) is refused;
//! - without it (an older browser), an `Origin` must match the host;
//! - without either, it is not a browser, so not a forgery: it passes.
//!
//! `/api/collect` is cross-origin by design (the tracker on every site)
//! and uses no cookie; a request with `Authorization: Bearer` (an API key)
//! cannot be forged by a page either. No crate does this (`axum_csrf` needs
//! tokens in the frontend), hence our own.

use crate::error::HttpError;
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn check(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if allowed(&request, state.trust_proxy) {
        next.run(request).await
    } else {
        HttpError::new(StatusCode::FORBIDDEN, "Cross-origin request refused").into_response()
    }
}

fn allowed(request: &Request, trust_proxy: bool) -> bool {
    let headers = request.headers();
    if request.method().is_safe()
        || request.uri().path() == "/api/collect"
        || headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.len() > 7 && v[..7].eq_ignore_ascii_case("bearer "))
    {
        return true;
    }
    if let Some(site) = headers.get("sec-fetch-site") {
        return matches!(site.to_str(), Ok("same-origin" | "none"));
    }
    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(origin) => same_host(origin, headers, trust_proxy),
        None => true,
    }
}

/// Whether an `Origin` (`scheme://host[:port]`) names the host the request
/// was sent to: the `Host` header, or the proxy's `X-Forwarded-Host` when
/// it is trusted.
fn same_host(origin: &str, headers: &HeaderMap, trust_proxy: bool) -> bool {
    let Some((_, origin_host)) = origin.split_once("://") else {
        return false; // "null", or not a URL
    };
    let forwarded = trust_proxy
        .then(|| headers.get("x-forwarded-host"))
        .flatten();
    forwarded
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .is_some_and(|host| host.eq_ignore_ascii_case(origin_host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn request(method: &str, path: &str, headers: &[(&str, &str)]) -> Request {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "potoo.example.com");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn fetch_metadata_decides_for_a_browser() {
        let post =
            |headers: &[(&str, &str)]| allowed(&request("POST", "/api/sites", headers), false);
        assert!(post(&[("sec-fetch-site", "same-origin")]));
        assert!(post(&[("sec-fetch-site", "none")]));
        assert!(!post(&[("sec-fetch-site", "same-site")]));
        assert!(!post(&[("sec-fetch-site", "cross-site")]));
        // Fetch Metadata wins over a matching Origin.
        assert!(!post(&[
            ("sec-fetch-site", "same-site"),
            ("origin", "https://potoo.example.com")
        ]));
    }

    #[test]
    fn without_fetch_metadata_the_origin_must_match() {
        let post = |headers: &[(&str, &str)], proxy| {
            allowed(&request("DELETE", "/api/sites/1", headers), proxy)
        };
        assert!(post(&[("origin", "https://potoo.example.com")], false));
        assert!(post(&[("origin", "https://POTOO.example.com")], false));
        assert!(!post(&[("origin", "https://blog.example.com")], false));
        assert!(!post(&[("origin", "null")], false));
        // Not a browser: nothing to forge.
        assert!(post(&[], false));
        // Behind a trusted proxy, the host it forwarded.
        let proxied = [
            ("origin", "https://stats.example.org"),
            ("x-forwarded-host", "stats.example.org"),
        ];
        assert!(post(&proxied, true));
        assert!(!post(&proxied, false));
    }

    #[test]
    fn reads_the_tracker_and_api_keys_pass() {
        let cross = [("sec-fetch-site", "cross-site")];
        assert!(allowed(&request("GET", "/api/stats", &cross), false));
        assert!(allowed(&request("POST", "/api/collect", &cross), false));
        let key = [
            ("sec-fetch-site", "cross-site"),
            ("authorization", "Bearer potoo_abc"),
        ];
        assert!(allowed(&request("POST", "/api/sites", &key), false));
    }
}
