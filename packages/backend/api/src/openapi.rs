//! The OpenAPI description of the API, written to files by the `specs`
//! binary (`mise run gen:spec`; the frontend's client is generated from
//! it) and served at `/swagger-ui` only with `SWAGGER_UI` set. The paths
//! come from the router itself: every handler is registered with `routes!`
//! next to its `#[utoipa::path]` (see `routes/`), so a routed endpoint is
//! documented and a documented one is routed. This file holds the rest: the title, the tags and the cookie
//! the dashboard endpoints are secured with.

use crate::sessions::SESSION_COOKIE;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Potoo API",
        description = "The API of Potoo, a small, privacy-friendly, self-hosted web analytics tool. \
            The dashboard endpoints need the session cookie from /api/login (or /api/setup), or \
            an API key as `Authorization: Bearer`; the tracking endpoints are public.",
    ),
    tags(
        (name = "Auth", description = "Signing in and out, and the first-run setup"),
        (name = "Account", description = "The signed-in user's own email, password and preferences"),
        (name = "Users", description = "Managing users (admin only)"),
        (name = "Admin", description = "Settings of the whole instance, such as single sign-on (admin only)"),
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
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(SESSION_COOKIE))),
            );
        // API keys (`potoo_...`), made in the settings: on the endpoints that
        // list it, capped by the key's scope.
        openapi
            .components
            .get_or_insert_with(Default::default)
            .add_security_scheme(
                "api_key",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some(
                            "An API key: `read` keys may only GET, `write` keys reach no admin endpoint.",
                        ))
                        .build(),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    fn operations() -> Vec<(String, String, serde_json::Value)> {
        let spec: serde_json::Value =
            serde_json::from_str(&crate::routes::api_doc().to_pretty_json().unwrap()).unwrap();
        let mut found = Vec::new();
        for (path, item) in spec["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                found.push((method.to_uppercase(), path.clone(), operation.clone()));
            }
        }
        found
    }

    #[test]
    fn every_tag_is_declared_and_used() {
        let spec: serde_json::Value =
            serde_json::from_str(&crate::routes::api_doc().to_pretty_json().unwrap()).unwrap();
        let declared: BTreeSet<_> = spec["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| tag["name"].as_str().unwrap().to_string())
            .collect();
        let used: BTreeSet<_> = operations()
            .iter()
            .map(|(_, _, op)| op["tags"][0].as_str().unwrap().to_string())
            .collect();
        assert_eq!(declared, used);
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
