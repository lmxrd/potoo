use crate::csv;
use crate::error::{ErrorBody, HttpError};
use crate::filter::filter_sql;
use crate::routes::auth::AuthUser;
use crate::routes::sites::{StatsQuery, range_and_filter, site_ids};
use crate::state::AppState;
use crate::stats::range_bounds;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use sqlx::Row;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

/// The export: every recorded hit of the dashboard's current view, as raw
/// CSV rows — the same range, sites and filter the stats endpoints take,
/// one row per hit, oldest first.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(export))
}

#[utoipa::path(
    get,
    path = "/",
    tag = "Stats",
    operation_id = "exportHits",
    summary = "Export the hits of a dashboard view as CSV",
    description = "Every recorded hit of the selected view, oldest first, one CSV row each: site, created_at, kind, path, name, referrer, country, device, browser, os. Takes the same parameters as the stats, so what you see is what you get. Sent as an attachment named potoo-export.csv.",
    params(StatsQuery),
    responses(
        (status = 200, description = "The hits as CSV", body = String, content_type = "text/csv", headers(("content-disposition" = String, description = "attachment; filename=\"potoo-export.csv\""))),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn export(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<StatsQuery>,
) -> Result<Response, HttpError> {
    let (range, filters) = range_and_filter(&query)?;
    let ids = site_ids(&state, query.sites.as_deref()).await?;
    let (since, until) = range_bounds(&range, Utc::now());

    let mut where_clauses: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    if !ids.is_empty() {
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        where_clauses.push(format!("hits.site_id IN ({placeholders})"));
        params.extend(ids.iter().map(|id| id.to_string()));
    } else {
        where_clauses.push("1 = 0".into()); // no sites: an empty export
    }
    where_clauses.push("hits.created_at >= ?".into());
    params.push(since);
    if let Some(until) = until {
        where_clauses.push("hits.created_at < ?".into());
        params.push(until);
    }
    let (filter_clause, filter_params) = filter_sql(&filters);
    if !filter_clause.is_empty() {
        where_clauses.push(filter_clause);
        params.extend(filter_params);
    }

    state.ingest.flush().await;
    // No table alias: the filter clause names its columns unqualified,
    // and every column it can name lives in hits alone.
    let sql = format!(
        "SELECT s.hostname AS site, hits.created_at, hits.kind, hits.path, hits.name,
                hits.referrer, hits.country, hits.device, hits.browser, hits.os
         FROM hits JOIN sites s ON s.id = hits.site_id
         WHERE {}
         ORDER BY hits.created_at, hits.id",
        where_clauses.join(" AND ")
    );
    // Fixed fragments and `?` placeholders only; the values are bound.
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
    for param in &params {
        query = query.bind(param.as_str());
    }
    let hits = query
        .fetch_all(state.db.reader())
        .await?
        .iter()
        .map(|row| {
            (0..10)
                .map(|i| Ok(row.try_get::<Option<String>, _>(i)?.unwrap_or_default()))
                .collect::<sqlx::Result<Vec<String>>>()
        })
        .collect::<sqlx::Result<Vec<_>>>()?;

    let mut rows = vec![
        [
            "site",
            "created_at",
            "kind",
            "path",
            "name",
            "referrer",
            "country",
            "device",
            "browser",
            "os",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>(),
    ];
    rows.extend(hits);
    Ok(csv::download(csv::write_csv(&rows), "potoo-export.csv"))
}
