use crate::auth::LoginThrottle;
use crate::db::Db;
use crate::geo::Geo;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub geo: Arc<Geo>,
    pub trust_proxy: bool,
    pub throttle: Arc<LoginThrottle>,
}
