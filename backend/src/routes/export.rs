use crate::csv;
use crate::error::HttpError;
use crate::routes::auth::AuthUser;
use crate::routes::sites::{StatsQuery, csv_response, range_and_filter, site_ids};
use crate::state::AppState;
use crate::stats::stats_for;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;
use chrono::Utc;
use rusqlite::params_from_iter;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/export/stats", get(stats_csv))
        .route("/export", get(export))
}

#[derive(Deserialize)]
struct ExportQuery {
    sites: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

// The aggregated numbers of one dashboard view: the same endpoint as
// /api/stats, but as a CSV document.
async fn stats_csv(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<StatsQuery>,
) -> Result<Response, HttpError> {
    let (range, filters) = range_and_filter(&query)?;
    let ids = site_ids(&state, query.sites.as_deref()).await?;
    let stats = stats_for(&state.db, &ids, &range, &filters, Utc::now()).await?;
    let body = csv::stats_csv(&stats, &csv::range_line(&range), query.q.as_deref());
    Ok(csv_response(body, "potoo-stats.csv"))
}

/// A strict inclusive day, as YYYY-MM-DD.
fn parse_day(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
        && raw[5..7].parse::<u8>().is_ok_and(|m| (1..=12).contains(&m))
        && raw[8..10]
            .parse::<u8>()
            .is_ok_and(|d| (1..=31).contains(&d))
    {
        Some(raw)
    } else {
        None
    }
}

// Every recorded hit of a range, one row per hit, oldest first.
async fn export(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<ExportQuery>,
) -> Result<Response, HttpError> {
    for day in [&query.from, &query.to].into_iter().flatten() {
        if parse_day(day).is_none() {
            return Err(HttpError::bad_request(
                "from and to must be days, as YYYY-MM-DD",
            ));
        }
    }
    let ids = site_ids(&state, query.sites.as_deref()).await?;

    // The days are inclusive: from its first to the last millisecond.
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    if !ids.is_empty() {
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        where_clauses.push(format!("h.site_id IN ({placeholders})"));
        params.extend(ids.iter().map(|id| id.to_string()));
    } else {
        where_clauses.push("1 = 0".into()); // no sites: an empty export
    }
    if let Some(from) = &query.from {
        where_clauses.push("h.created_at >= ?".into());
        params.push(format!("{from}T00:00:00.000"));
    }
    if let Some(to) = &query.to {
        where_clauses.push("h.created_at <= ?".into());
        params.push(format!("{to}T23:59:59.999"));
    }

    let conn = state.db.lock().await;
    let sql = format!(
        "SELECT s.hostname AS site, h.created_at, h.kind, h.path, h.name,
                h.referrer, h.country, h.device, h.browser, h.os
         FROM hits h JOIN sites s ON s.id = h.site_id
         WHERE {}
         ORDER BY h.created_at, h.id",
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
    Ok(csv_response(csv::write_csv(&rows), "potoo-export.csv"))
}
