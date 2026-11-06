use crate::auth::LoginThrottle;
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
    pub trust_proxy: bool,
    pub throttle: Arc<LoginThrottle>,
    pub oidc: Arc<Oidc>,
    /// `TOTP_REQUIRED`: every user must have two-factor authentication on;
    /// one without it sets it up at their next sign-in.
    pub totp_required: bool,
    /// `SECURE_COOKIES`: the session cookie is `Secure` (Potoo is served
    /// over https).
    pub secure_cookies: bool,
}
