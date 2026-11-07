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
//! - without it (an older browser), an `Origin` must be `PUBLIC_URL`'s, or,
//!   without `PUBLIC_URL`, the request's `Host`;
//! - without either, it is not a browser, so not a forgery: it passes.
//!
//! `/api/collect` is cross-origin by design (the tracker on every site)
//! and uses no cookie; a request with `Authorization: Bearer` (an API key)
//! cannot be forged by a page either. No crate does this (`axum_csrf` needs
//! tokens in the frontend), hence our own.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The middleware on every route.
pub async fn protect(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if allowed(
        request.method(),
        path,
        request.headers(),
        state.public_url.as_deref(),
    ) {
        next.run(request).await
    } else {
        ApiError::forbidden("Cross-site request refused").into_response()
    }
}

/// Whether a request may go on (see the module docs).
pub fn allowed(method: &Method, path: &str, headers: &HeaderMap, public_url: Option<&str>) -> bool {
    if method.is_safe()
        || path == "/api/collect"
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
        Some(origin) => own_origin(origin, headers, public_url),
        None => true,
    }
}

/// Whether an `Origin` (`scheme://host[:port]`) is Potoo's own: the one of
/// `PUBLIC_URL` (scheme included), or without it the host the request was
/// sent to. Forwarded headers do not count: behind a proxy that rewrites
/// `Host`, set `PUBLIC_URL`.
fn own_origin(origin: &str, headers: &HeaderMap, public_url: Option<&str>) -> bool {
    if let Some(public_url) = public_url {
        return origin.eq_ignore_ascii_case(public_url);
    }
    let Some((_, origin_host)) = origin.split_once("://") else {
        return false; // "null", or not a URL
    };
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|host| host.eq_ignore_ascii_case(origin_host))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request to Potoo's host, as the arguments of [`allowed`].
    struct Request(Method, &'static str, HeaderMap);

    fn request(method: &str, path: &'static str, headers: &[(&str, &str)]) -> Request {
        let mut map = HeaderMap::new();
        map.insert("host", "potoo.example.com".parse().unwrap());
        for (name, value) in headers {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        Request(method.parse().unwrap(), path, map)
    }

    fn allowed(request: &Request, public_url: Option<&str>) -> bool {
        super::allowed(&request.0, request.1, &request.2, public_url)
    }

    #[test]
    fn fetch_metadata_decides_for_a_browser() {
        let post =
            |headers: &[(&str, &str)]| allowed(&request("POST", "/api/sites", headers), None);
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
        let post = |headers: &[(&str, &str)], public_url| {
            allowed(&request("DELETE", "/api/sites/1", headers), public_url)
        };
        assert!(post(&[("origin", "https://potoo.example.com")], None));
        assert!(post(&[("origin", "https://POTOO.example.com")], None));
        assert!(!post(&[("origin", "https://blog.example.com")], None));
        assert!(!post(&[("origin", "null")], None));
        // Not a browser: nothing to forge.
        assert!(post(&[], None));
        // With PUBLIC_URL, its origin, scheme included, whatever the Host
        // a proxy sends; forwarded headers do not count.
        let public = Some("https://stats.example.org");
        assert!(post(&[("origin", "https://stats.example.org")], public));
        assert!(!post(&[("origin", "http://stats.example.org")], public));
        assert!(!post(&[("origin", "https://potoo.example.com")], public));
        let forwarded = [
            ("origin", "https://elsewhere.example.org"),
            ("x-forwarded-host", "elsewhere.example.org"),
        ];
        assert!(!post(&forwarded, None));
    }

    #[test]
    fn reads_the_tracker_and_api_keys_pass() {
        let cross = [("sec-fetch-site", "cross-site")];
        assert!(allowed(&request("GET", "/api/stats", &cross), None));
        assert!(allowed(&request("POST", "/api/collect", &cross), None));
        let key = [
            ("sec-fetch-site", "cross-site"),
            ("authorization", "Bearer potoo_abc"),
        ];
        assert!(allowed(&request("POST", "/api/sites", &key), None));
    }
}
