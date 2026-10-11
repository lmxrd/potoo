use crate::error::HttpError;
use crate::filter;
use crate::routes::auth::AuthUser;
use crate::state::AppState;
use crate::stats::{parse_range, stats_for};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", patch(update).delete(delete))
        .route("/{id}/stats", get(site_stats))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    pub sites: Option<String>,
    pub range: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub q: Option<String>,
}

/// The comma separated site ids a stats or export request selects, or
/// every site's when there are none.
pub async fn site_ids(state: &AppState, sites: Option<&str>) -> Result<Vec<i64>, HttpError> {
    if let Some(sites) = sites {
        let ids = sites
            .split(',')
            .map(|id| id.parse::<i64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| HttpError::bad_request("sites must be comma separated site ids"))?;
        return Ok(ids);
    }
    let ids = sqlx::query_scalar("SELECT id FROM sites")
        .fetch_all(state.db.reader())
        .await?;
    Ok(ids)
}

/// Parse the range and filter of a stats or export request, or answer
/// with the 400. Every endpoint that filters by time takes the same
/// params, so they all go through here.
pub fn range_and_filter(
    query: &StatsQuery,
) -> Result<(crate::stats::StatsRange, filter::FacetFilters), HttpError> {
    let range = parse_range(
        query.range.as_deref(),
        query.from.as_deref(),
        query.to.as_deref(),
    )
    .map_err(HttpError::bad_request)?;
    let filters = filter::parse_filter_param(query.q.as_deref()).map_err(HttpError::bad_request)?;
    Ok((range, filters))
}

async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, HttpError> {
    #[derive(Serialize, sqlx::FromRow)]
    struct SiteRow {
        id: i64,
        hostname: String,
        honor_dnt: i64,
        created_at: String,
    }
    let rows: Vec<SiteRow> =
        sqlx::query_as("SELECT id, hostname, honor_dnt, created_at FROM sites ORDER BY hostname")
            .fetch_all(state.db.reader())
            .await?;
    Ok(Json(json!(rows)))
}

/// Expected a hostname such as example.com or blog.example.com.
fn validate_hostname(raw: &str) -> Result<String, HttpError> {
    let host = raw.trim().to_lowercase();
    let invalid =
        || HttpError::bad_request("Expected a hostname such as example.com or blog.example.com");
    if host.len() > 253 {
        return Err(invalid());
    }
    // An optional port.
    let (host, port) = match host.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), Some(port.to_string())),
        None => (host.clone(), None),
    };
    if let Some(port) = port
        && (port.is_empty() || port.len() > 5 || !port.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(invalid());
    }
    if host.is_empty()
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
    {
        return Err(invalid());
    }
    Ok(host)
}

#[derive(Deserialize)]
struct SiteInput {
    hostname: String,
    #[serde(default = "default_true")]
    honor_dnt: bool,
}

fn default_true() -> bool {
    true
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    body: String,
) -> Result<Response, HttpError> {
    if user.0.is_admin != 1 {
        return Err(HttpError::forbidden());
    }
    let input: SiteInput =
        serde_json::from_str(&body).map_err(|_| HttpError::bad_request("Invalid request body"))?;
    let hostname = validate_hostname(&input.hostname)?;
    let mut tx = state.db.writer().begin().await?;
    let taken: Option<i64> = sqlx::query_scalar("SELECT 1 FROM sites WHERE hostname = ?1")
        .bind(&hostname)
        .fetch_optional(&mut *tx)
        .await?;
    if taken.is_some() {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Site already exists".into(),
        });
    }
    let id = sqlx::query("INSERT INTO sites (hostname, honor_dnt) VALUES (?1, ?2)")
        .bind(&hostname)
        .bind(input.honor_dnt as i64)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "hostname": hostname,
            "honor_dnt": input.honor_dnt as i64,
        })),
    )
        .into_response())
}

#[derive(Deserialize)]
struct DntInput {
    honor_dnt: bool,
}

async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    body: String,
) -> Result<Response, HttpError> {
    if user.0.is_admin != 1 {
        return Err(HttpError::forbidden());
    }
    let input: DntInput =
        serde_json::from_str(&body).map_err(|_| HttpError::bad_request("Invalid request body"))?;
    sqlx::query("UPDATE sites SET honor_dnt = ?1 WHERE id = ?2")
        .bind(input.honor_dnt as i64)
        .bind(id)
        .execute(state.db.writer())
        .await?;
    state.db.forget_sites();
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Deleting a site deletes all of its hits.
async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Response, HttpError> {
    if user.0.is_admin != 1 {
        return Err(HttpError::forbidden());
    }
    sqlx::query("DELETE FROM sites WHERE id = ?1")
        .bind(id)
        .execute(state.db.writer())
        .await?;
    state.db.forget_sites();
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn site_stats(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<i64>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (range, filters) = range_and_filter(&query)?;
    let site: Option<(i64, String, i64)> =
        sqlx::query_as("SELECT id, hostname, honor_dnt FROM sites WHERE id = ?1")
            .bind(id)
            .fetch_optional(state.db.reader())
            .await?;
    let Some((id, hostname, honor_dnt)) = site else {
        return Err(HttpError {
            status: StatusCode::NOT_FOUND,
            message: "Not found".into(),
        });
    };
    state.ingest.flush().await;
    let stats = stats_for(&state.db, &[id], &range, &filters, Utc::now()).await?;
    let mut value = serde_json::to_value(&stats).unwrap();
    value["site"] = json!({ "id": id, "hostname": hostname, "honor_dnt": honor_dnt });
    Ok(Json(value))
}
