use crate::error::{ErrorBody, HttpError};
use crate::routes::auth::AuthUser;
use crate::routes::sites::{StatsQuery, range_and_filter, site_ids};
use crate::state::AppState;
use crate::stats::{SiteStats, stats_for};
use axum::Json;
use axum::extract::{Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(stats))
}

// The aggregated numbers of one dashboard view, across sites.
#[utoipa::path(
    get,
    path = "/",
    tag = "Stats",
    operation_id = "getStats",
    summary = "The numbers of a dashboard view",
    description = "Page views and visitors over a range, for any subset of the sites, filtered by facet, with the breakdowns the dashboard shows. \"Visitors\" counts distinct daily hashes, which include the site id, so the same person on two sites counts as two visitors.",
    params(StatsQuery),
    responses(
        (status = 200, description = "The aggregated numbers", body = SiteStats),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn stats(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<StatsQuery>,
) -> Result<Json<SiteStats>, HttpError> {
    let (range, filters) = range_and_filter(&query)?;
    let ids = site_ids(&state, query.sites.as_deref()).await?;
    state.ingest.flush().await;
    let stats = stats_for(&state.db, &ids, &range, &filters).await?;
    Ok(Json(stats))
}
