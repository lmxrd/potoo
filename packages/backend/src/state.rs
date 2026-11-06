use crate::db::Db;
use crate::geo::Geo;
use crate::ingest::Ingest;
use crate::oidc::Oidc;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub ingest: Ingest,
    pub geo: Arc<Geo>,
    /// `TRUST_PROXY`: the client's address is the proxy's `X-Forwarded-For`.
    pub trust_proxy: bool,
    /// How often sign-ins and codes may be tried (`limits.rs`).
    pub limits: Arc<crate::limits::Limits>,
    /// `PUBLIC_URL`, without a trailing slash: where Potoo is reached from.
    pub public_url: Option<String>,
    /// Signs the cookie of a sign-in's two-factor step. Made at start: a
    /// restart only means signing in again from the password.
    pub challenge_key: axum_extra::extract::cookie::Key,
    pub oidc: Arc<Oidc>,
    /// `TOTP_REQUIRED`: every user must have two-factor authentication on;
    /// one without it sets it up at their next sign-in.
    pub totp_required: bool,
    /// `PUBLIC_URL` is https: every cookie is `Secure` and `__Host-`.
    pub secure_cookies: bool,
}
