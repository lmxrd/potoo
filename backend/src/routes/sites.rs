use crate::error::{ErrorBody, HttpError};
use crate::filter;
use crate::routes::auth::AuthUser;
use crate::state::AppState;
use crate::stats::parse_range;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, create))
        .routes(routes!(get_site, update, delete))
}

/// What the stats and the export are computed over. All parameters are
/// optional: without any, every site over the last 7 days.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatsQuery {
    /// Comma separated hostnames, e.g. `blog.example.com,shop.example.com`.
    /// Every site when absent. An unknown hostname is a 400.
    pub sites: Option<String>,
    /// A rolling window ending now, as an ISO 8601 duration:
    /// `PT12H`, `P7D`, `P30D`, `P90D`, `P1Y`, ... (at most 365 days).
    /// Default `P7D`. Ignored when `from` and `to` are given.
    pub range: Option<String>,
    /// The start of an exact window, `YYYY-MM-DDTHH:MM` in UTC. Needs `to`.
    pub from: Option<String>,
    /// The end of an exact window (that minute included), `YYYY-MM-DDTHH:MM`
    /// in UTC. Needs `from`; the window spans at most 730 days.
    pub to: Option<String>,
    /// A filter over the facets (path, country, device, browser, os,
    /// referrer, name): equality, OR (`,`) within a facet and AND (`;`)
    /// across facets, e.g. `country==NO;path==/pricing`.
    pub q: Option<String>,
}

/// The ids of the sites a stats or export request selects: the comma
/// separated hostnames it names (case does not matter; an unknown one is a
/// 400 that says which), or every site's when there are none.
pub async fn site_ids(state: &AppState, sites: Option<&str>) -> Result<Vec<i64>, HttpError> {
    let hostnames: BTreeSet<String> = sites
        .unwrap_or_default()
        .split(',')
        .map(|host| host.trim().to_lowercase())
        .filter(|host| !host.is_empty())
        .collect();
    if !hostnames.is_empty() {
        let mut ids = Vec::new();
        for host in hostnames {
            match state.db.site(&host).await? {
                Some(site) => ids.push(site.id),
                None => return Err(HttpError::bad_request(format!("Unknown site: {host}"))),
            }
        }
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

/// A registered site.
#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct SiteRow {
    pub id: i64,
    pub hostname: String,
    /// Whether hits from visitors who send Do Not Track (or Global Privacy
    /// Control) are ignored.
    pub honor_dnt: bool,
    pub created_at: String,
}

/// The site that was just created.
#[derive(Serialize, ToSchema)]
pub struct CreatedSite {
    pub id: i64,
    pub hostname: String,
    pub honor_dnt: bool,
}

#[utoipa::path(
    get,
    path = "/",
    tag = "Sites",
    operation_id = "listSites",
    summary = "List the sites",
    responses(
        (status = 200, description = "All sites, by hostname", body = Vec<SiteRow>),
        (status = 401, description = "Not signed in", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<Json<Vec<SiteRow>>, HttpError> {
    let rows: Vec<SiteRow> =
        sqlx::query_as("SELECT id, hostname, honor_dnt, created_at FROM sites ORDER BY hostname")
            .fetch_all(state.db.reader())
            .await?;
    Ok(Json(rows))
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

/// A site to register.
#[derive(Deserialize, ToSchema)]
pub struct SiteInput {
    /// A hostname such as `example.com` or `blog.example.com`, with an
    /// optional port. Every subdomain is its own site.
    pub hostname: String,
    /// Ignore visitors who ask not to be tracked. Default true.
    #[serde(default = "default_true")]
    pub honor_dnt: bool,
}

fn default_true() -> bool {
    true
}

#[utoipa::path(
    post,
    path = "/",
    tag = "Sites",
    operation_id = "createSite",
    summary = "Register a site",
    request_body(content = SiteInput, content_type = "application/json"),
    responses(
        (status = 201, description = "The site was registered", body = CreatedSite),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 409, description = "Conflict", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    body: String,
) -> Result<Response, HttpError> {
    if !user.0.is_admin {
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
        Json(CreatedSite {
            id,
            hostname,
            honor_dnt: input.honor_dnt,
        }),
    )
        .into_response())
}

#[derive(Deserialize, ToSchema)]
pub struct DntInput {
    pub honor_dnt: bool,
}

#[utoipa::path(
    patch,
    path = "/{id}",
    tag = "Sites",
    operation_id = "updateSite",
    summary = "Change a site's settings",
    description = "Whether Do Not Track is honored. Changing a site that does not exist is not an error.",
    params(("id" = i64, Path, description = "The site id")),
    request_body(content = DntInput, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    body: String,
) -> Result<Response, HttpError> {
    if !user.0.is_admin {
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
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "Sites",
    operation_id = "deleteSite",
    summary = "Delete a site and all of its hits",
    params(("id" = i64, Path, description = "The site id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Response, HttpError> {
    if !user.0.is_admin {
        return Err(HttpError::forbidden());
    }
    sqlx::query("DELETE FROM sites WHERE id = ?1")
        .bind(id)
        .execute(state.db.writer())
        .await?;
    state.db.forget_sites();
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    get,
    path = "/{id}",
    tag = "Sites",
    operation_id = "getSite",
    summary = "One site",
    params(("id" = i64, Path, description = "The site id")),
    responses(
        (status = 200, description = "The site", body = SiteRow),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn get_site(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<SiteRow>, HttpError> {
    let site: Option<SiteRow> =
        sqlx::query_as("SELECT id, hostname, honor_dnt, created_at FROM sites WHERE id = ?1")
            .bind(id)
            .fetch_optional(state.db.reader())
            .await?;
    site.map(Json).ok_or(HttpError {
        status: StatusCode::NOT_FOUND,
        message: "Not found".into(),
    })
}
