//! The OpenAPI description of the API, written to a file by the `openapi`
//! binary (`mise run gen:spec`) and never served: the frontend's client is
//! generated from it. The paths come from the router itself: every handler
//! is registered with `routes!` next to its `#[utoipa::path]` (see
//! `routes/`), so a routed endpoint is documented and a documented one is
//! routed. This file holds the rest: the title, the tags and the cookie
//! the dashboard endpoints are secured with.

use crate::auth::COOKIE;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Potoo API",
        description = "The API of Potoo, a small, privacy-friendly, self-hosted web analytics tool. \
            The dashboard endpoints need the session cookie from /api/login (or /api/setup); the \
            tracking endpoints are public.",
    ),
    tags(
        (name = "Auth", description = "Signing in and out, and the first-run setup"),
        (name = "Account", description = "The signed-in user's own email, password and preferences"),
        (name = "Users", description = "Managing users (admin only)"),
        (name = "Sites", description = "The sites that are tracked"),
        (name = "Stats", description = "The numbers of the dashboard, and the export and import of hits"),
        (name = "Tracking", description = "What a visitor's browser calls; public"),
        (name = "System", description = "Operations"),
    ),
    modifiers(&SessionCookie),
)]
pub struct ApiDoc;

/// Declares the cookie the dashboard endpoints are secured with.
struct SessionCookie;

impl Modify for SessionCookie {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi
            .components
            .get_or_insert_with(Default::default)
            .add_security_scheme(
                "session",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(COOKIE))),
            );
    }
}

/// The document without paths: what the router starts from.
pub fn base() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// The complete document: the base plus every routed endpoint.
pub fn spec() -> utoipa::openapi::OpenApi {
    crate::api().split_for_parts().1
}

/// The spec as pretty printed JSON.
pub fn spec_json() -> String {
    spec().to_pretty_json().expect("the spec serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn operations() -> Vec<(String, String, serde_json::Value)> {
        let spec: serde_json::Value = serde_json::from_str(&spec_json()).unwrap();
        let mut found = Vec::new();
        for (path, item) in spec["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                found.push((method.to_uppercase(), path.clone(), operation.clone()));
            }
        }
        found
    }

    #[test]
    fn operations_have_unique_ids_tags_and_summaries() {
        let operations = operations();
        let ids: BTreeSet<_> = operations
            .iter()
            .map(|(_, _, op)| op["operationId"].as_str().expect("an operationId"))
            .collect();
        assert_eq!(ids.len(), operations.len(), "operation ids must be unique");
        for (method, path, op) in &operations {
            assert!(op["tags"][0].is_string(), "{method} {path} has no tag");
            assert!(op["summary"].is_string(), "{method} {path} has no summary");
        }
    }

    #[test]
    fn the_dashboard_endpoints_need_the_session_cookie() {
        for (method, path, op) in operations() {
            let public = ["Auth", "Tracking", "System"].contains(&op["tags"][0].as_str().unwrap());
            let secured = op["security"].is_array();
            assert_eq!(!public, secured, "{method} {path}");
        }
    }
}
