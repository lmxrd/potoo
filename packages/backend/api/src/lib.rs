pub mod auth;
pub mod config;
pub mod csrf;
pub mod csv;
pub mod db;
pub mod error;
pub mod extract;
pub mod filter;
pub mod geo;
pub mod ingest;
pub mod jobs;
pub mod limits;
pub mod oidc;
pub mod openapi;
pub mod profile;
pub mod random;
pub mod routes;
pub mod sessions;
pub mod state;
pub mod stats;
pub mod time;
pub mod totp;
pub mod tracker;
pub mod visitor;

/// Install ring as the process TLS provider. reqwest is built with
/// `rustls-no-provider` (aws-lc is painful on musl, which the Docker
/// image builds for), so the provider must be installed before the
/// first client is built. Safe to call more than once.
pub fn install_tls_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
    });
}
