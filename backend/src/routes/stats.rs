use crate::error::HttpError;
use crate::routes::auth::AuthUser;
use crate::routes::sites::{StatsQuery, range_and_filter, site_ids};
use crate::state::AppState;
use crate::stats::stats_for;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(stats))
}

// The aggregated numbers of one dashboard view, across sites.
async fn stats(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (range, filters) = range_and_filter(&query)?;
    let ids = site_ids(&state, query.sites.as_deref()).await?;
    let stats = stats_for(&state.db, &ids, &range, &filters, Utc::now()).await?;
    Ok(Json(json!(serde_json::to_value(&stats).unwrap())))
}
