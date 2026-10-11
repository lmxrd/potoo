//! The OpenAPI description of the API, written to a file by the `openapi`
//! binary (`mise run gen:spec`) and never served: the frontend's client is
//! generated from it. Every handler carries its own `#[utoipa::path]`;
//! this file only lists them. A new endpoint is added to `paths(...)`
//! below, or it is missing from the spec (the test at the bottom fails).

use crate::auth::COOKIE;
use crate::routes::{auth, collect, export, sites, stats};
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
    paths(
        auth::session,
        auth::setup,
        auth::login,
        auth::logout,
        auth::change_email,
        auth::change_settings,
        auth::change_password,
        auth::list_users,
        auth::create_user,
        auth::change_role,
        auth::delete_user,
        auth::reset_password,
        sites::list,
        sites::get_site,
        sites::create,
        sites::update,
        sites::delete,
        stats::stats,
        export::export,
        collect::collect_post,
        collect::collect_pixel,
        collect::tracker,
        crate::routes::health,
    ),
    tags(
        (name = "Auth", description = "Signing in and out, and the first-run setup"),
        (name = "Account", description = "The signed-in user's own email, password and preferences"),
        (name = "Users", description = "Managing users (admin only)"),
        (name = "Sites", description = "The sites that are tracked"),
        (name = "Stats", description = "The numbers of the dashboard, and their export"),
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

/// The spec as pretty printed JSON.
pub fn spec_json() -> String {
    ApiDoc::openapi()
        .to_pretty_json()
        .expect("the spec serializes")
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
    fn every_endpoint_is_documented() {
        // The routes of the app. Adding or removing an endpoint means
        // changing this list and `paths(...)` above.
        let expected: BTreeSet<(&str, &str)> = [
            ("GET", "/api/health"),
            ("POST", "/api/collect"),
            ("GET", "/api/pixel"),
            ("GET", "/tracker.js"),
            ("GET", "/api/export"),
            ("GET", "/api/stats"),
            ("GET", "/api/session"),
            ("POST", "/api/setup"),
            ("POST", "/api/login"),
            ("POST", "/api/logout"),
            ("POST", "/api/email"),
            ("POST", "/api/settings"),
            ("POST", "/api/password"),
            ("GET", "/api/users"),
            ("POST", "/api/users"),
            ("PATCH", "/api/users/{id}"),
            ("DELETE", "/api/users/{id}"),
            ("POST", "/api/users/{id}/password"),
            ("GET", "/api/sites"),
            ("POST", "/api/sites"),
            ("GET", "/api/sites/{id}"),
            ("PATCH", "/api/sites/{id}"),
            ("DELETE", "/api/sites/{id}"),
        ]
        .into_iter()
        .collect();
        let documented = operations();
        let documented: BTreeSet<(&str, &str)> = documented
            .iter()
            .map(|(method, path, _)| (method.as_str(), path.as_str()))
            .collect();
        assert_eq!(documented, expected);
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
            // Auth is public as a tag, except nothing in it needs a session.
            assert_eq!(!public, secured, "{method} {path}");
        }
    }
}
