use crate::csv;
use crate::error::HttpError;
use crate::filter::filter_sql;
use crate::routes::auth::AuthUser;
use crate::routes::sites::{StatsQuery, range_and_filter, site_ids};
use crate::state::AppState;
use crate::stats::range_bounds;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;
use chrono::Utc;
use rusqlite::params_from_iter;

/// The export: every recorded hit of the dashboard's current view, as raw
/// CSV rows — the same range, sites and filter the stats endpoints take,
/// one row per hit, oldest first.
pub fn router() -> Router<AppState> {
    Router::new().route("/export", get(export))
}

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

    let conn = state.db.lock().await;
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
    let mut stmt = conn.prepare(&sql)?;
    let hits = stmt
        .query_map(params_from_iter(params.iter()), |row| {
            Ok([
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                row.get::<_, Option<String>>(9)?.unwrap_or_default(),
            ]
            .to_vec())
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    drop(conn);

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
