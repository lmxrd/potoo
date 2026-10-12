use crate::error::{ErrorBody, HttpError};
use crate::routes::auth::AuthUser;
use crate::routes::collect::{clean_path, clean_referrer};
use crate::routes::sites::validate_hostname;
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequest, Multipart, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

/// The import of hits from the CSV the export writes. All or nothing: the
/// whole file is checked first, and the rows go in one transaction.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(import))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

/// A million rows are about 120 MB.
const MAX_BODY: usize = 256 * 1024 * 1024;
/// More problems than this are counted, not listed.
/// The columns a file must have; `visitor` may follow.
const REQUIRED: usize = COLUMNS.len() - 1;
const MAX_PROBLEMS: usize = 100;
/// The columns of the export; the last is optional in a file to import.
const COLUMNS: [&str; 11] = [
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
    "visitor",
];

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportQuery {
    /// Create the sites of the file that do not exist, instead of refusing
    /// the file. Default false.
    #[serde(default)]
    pub create_sites: bool,
}

#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    /// How many hits were added.
    pub imported: i64,
    /// How many sites were created.
    pub sites_created: i64,
}

/// One thing wrong with the file.
#[derive(Serialize, ToSchema)]
pub struct ImportProblem {
    /// The line of the file, counting the header as line 1; 0 when the
    /// problem is not on a line.
    pub line: u64,
    /// The column concerned, if there is one.
    pub column: Option<String>,
    pub message: String,
}

/// The answer to a file that was refused: nothing was imported.
#[derive(Serialize, ToSchema)]
pub struct ImportRejected {
    /// A summary, fit to show to a person.
    pub error: String,
    /// The first problems, in file order (unknown sites once each).
    pub problems: Vec<ImportProblem>,
    /// All the problems found, which may be more than are listed.
    pub total_problems: usize,
}

/// A failed import: an ordinary error, or the list of what is wrong with
/// the file.
enum Failure {
    Http(HttpError),
    Rejected(Vec<ImportProblem>, usize),
}

impl From<HttpError> for Failure {
    fn from(e: HttpError) -> Self {
        Failure::Http(e)
    }
}

impl From<sqlx::Error> for Failure {
    fn from(e: sqlx::Error) -> Self {
        Failure::Http(e.into())
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Failure::Http(e) => e.into_response(),
            Failure::Rejected(problems, total) => {
                let first = &problems[0];
                let at = if first.line > 0 {
                    format!("line {}: ", first.line)
                } else {
                    String::new()
                };
                let more = if total > 1 {
                    format!(" (and {} more)", total - 1)
                } else {
                    String::new()
                };
                let body = ImportRejected {
                    error: format!("Nothing was imported. {at}{}{more}", first.message),
                    problems,
                    total_problems: total,
                };
                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/",
    tag = "Stats",
    operation_id = "importHits",
    summary = "Import hits from CSV",
    description = "Adds the rows of a CSV in the export's format (site, created_at, kind, path, name, referrer, country, device, browser, os and optionally visitor; a header row names the columns). Sent as multipart/form-data with the CSV in a `file` part, or as the body itself with Content-Type text/csv or text/plain. Admin only. All or nothing: if a row cannot be read or names a site that does not exist, nothing is added and the answer lists what is wrong. With `create_sites=true` the sites of the file that do not exist are created (with the defaults of a new site) in the same transaction, in place of being refused. Referrer, country, device, browser and os may be empty (unknown). The header may end with an optional `visitor` column, the hash the export writes; hits without one add pageviews but no unique visitors. Paths and referrers are reduced as they are for tracked hits; the rows are not checked for duplicates. At most 256 MB.",
    params(ImportQuery),
    request_body(content(
        (ImportForm = "multipart/form-data"),
        (String = "text/csv"),
        (String = "text/plain"),
    )),
    responses(
        (status = 200, description = "Every row was added", body = ImportResult),
        (status = 400, description = "The file is invalid; nothing was imported", body = ImportRejected),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not an admin", body = ErrorBody),
        (status = 413, description = "The file is larger than 256 MB", body = ErrorBody),
        (status = 415, description = "Send multipart/form-data, text/csv or text/plain", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn import(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ImportQuery>,
    request: Request,
) -> Result<Json<ImportResult>, Failure> {
    if !user.0.is_admin {
        return Err(HttpError::forbidden().into());
    }
    let csv = read_csv(&state, request).await?;

    let mut tx = state.db.writer().begin().await?;
    let sites: HashMap<String, i64> = sqlx::query_as("SELECT hostname, id FROM sites")
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();

    // The check reads every row; the transaction is open meanwhile, but the
    // ingest queue only waits for the writer, which the import holds anyway.
    let sites = std::sync::Arc::new(sites);
    let problems = {
        let (csv, sites) = (csv.clone(), sites.clone());
        tokio::task::spawn_blocking(move || check(&csv, &sites, query.create_sites))
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?
    };
    if problems.total > 0 {
        return Err(Failure::Rejected(problems.listed, problems.total));
    }

    // The sites the file asked for, then the same rows again, now known to
    // be good.
    let mut sites = (*sites).clone();
    for hostname in &problems.new_sites {
        let id = sqlx::query("INSERT INTO sites (hostname) VALUES (?1)")
            .bind(hostname)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
        sites.insert(hostname.clone(), id);
    }
    let mut reader = reader(without_bom(&csv));
    let width = reader.headers().map_or(REQUIRED, |h| h.len());
    let mut imported = 0;
    for record in reader.records() {
        let record = record.map_err(|e| HttpError::internal(e.to_string()))?;
        let row = parse_row(&record, &sites, width, false).expect("the check passed");
        sqlx::query(
            "INSERT INTO hits
               (created_at, site_id, kind, path, name, referrer, country, device, browser, os, visitor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(row.created_at)
        .bind(row.site_id)
        .bind(row.kind)
        .bind(row.path)
        .bind(row.name)
        .bind(row.referrer)
        .bind(row.country)
        .bind(row.device)
        .bind(row.browser)
        .bind(row.os)
        .bind(row.visitor)
        .execute(&mut *tx)
        .await?;
        imported += 1;
    }
    tx.commit().await?;
    if !problems.new_sites.is_empty() {
        state.db.forget_sites();
    }
    Ok(Json(ImportResult {
        imported,
        sites_created: problems.new_sites.len() as i64,
    }))
}

/// The CSV of the request: the `file` part of a form, or the body.
async fn read_csv(state: &AppState, request: Request) -> Result<Bytes, HttpError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let unreadable = |e: &dyn std::fmt::Display| HttpError {
        status: StatusCode::BAD_REQUEST,
        message: format!("The upload could not be read: {e}"),
    };
    if content_type.starts_with("multipart/form-data") {
        let mut form = Multipart::from_request(request, state)
            .await
            .map_err(|e| unreadable(&e))?;
        while let Some(field) = form.next_field().await.map_err(|e| unreadable(&e))? {
            if field.name() == Some("file") {
                return field.bytes().await.map_err(|e| unreadable(&e));
            }
        }
        return Err(HttpError::bad_request(
            "The form has no `file` part with the CSV",
        ));
    }
    if content_type.starts_with("text/csv") || content_type.starts_with("text/plain") {
        return axum::body::to_bytes(request.into_body(), MAX_BODY)
            .await
            .map_err(|e| unreadable(&e));
    }
    Err(HttpError {
        status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
        message: "Send multipart/form-data, text/csv or text/plain".into(),
    })
}

/// A byte order mark (Excel adds one) is not part of the first header.
fn without_bom(csv: &[u8]) -> &[u8] {
    csv.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(csv)
}

fn reader(csv: &[u8]) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(csv)
}

/// The line of a byte offset. The csv reader's own positions lag by one
/// after a CRLF, which the export writes; a row may also span
/// lines (a quoted newline), so lines are counted from the bytes, moving
/// forward only.
struct Lines<'a> {
    csv: &'a [u8],
    byte: usize,
    line: u64,
}

impl<'a> Lines<'a> {
    fn new(csv: &'a [u8]) -> Self {
        Lines {
            csv,
            byte: 0,
            line: 1,
        }
    }

    fn at(&mut self, byte: u64) -> u64 {
        // Inclusive: after a CRLF the reader's offset is that of the `\n`,
        // one byte before the row. A row never starts with a newline.
        let byte = (byte as usize + 1).min(self.csv.len());
        if byte > self.byte {
            self.line += self.csv[self.byte..byte]
                .iter()
                .filter(|&&b| b == b'\n')
                .count() as u64;
            self.byte = byte;
        }
        self.line
    }
}

/// The problems found so far: the first few, and how many in all.
#[derive(Default)]
struct Problems {
    listed: Vec<ImportProblem>,
    total: usize,
    /// The sites to create, once each, in file order.
    new_sites: Vec<String>,
}

impl Problems {
    fn add(&mut self, problem: ImportProblem) {
        self.total += 1;
        if self.listed.len() < MAX_PROBLEMS {
            self.listed.push(problem);
        }
    }

    fn file(&mut self, message: String) {
        self.add(ImportProblem {
            line: 0,
            column: None,
            message,
        });
    }
}

/// Every problem of the file, up to a limit, and how many there were.
fn check(csv: &[u8], sites: &HashMap<String, i64>, create_sites: bool) -> Problems {
    let mut problems = Problems::default();
    let csv = without_bom(csv);
    let mut lines = Lines::new(csv);
    let mut reader = reader(csv);
    match reader.headers() {
        Err(e) => problems.file(format!("The header row cannot be read: {e}")),
        Ok(header) => {
            let found: Vec<&str> = header.iter().map(str::trim).collect();
            if found != COLUMNS[..REQUIRED] && found != COLUMNS {
                problems.add(ImportProblem {
                    line: 1,
                    column: None,
                    message: format!(
                        "The header must be {}, optionally followed by {}. Found: {}",
                        COLUMNS[..REQUIRED].join(","),
                        COLUMNS[REQUIRED],
                        found.join(",")
                    ),
                });
            }
        }
    }
    if problems.total > 0 {
        return problems;
    }
    let width = reader.headers().map_or(REQUIRED, |h| h.len());

    let mut unknown_sites: Vec<String> = Vec::new();
    let mut rows = 0;
    for record in reader.records() {
        rows += 1;
        let record = match record {
            Ok(record) => record,
            Err(e) => {
                problems.add(ImportProblem {
                    line: e.position().map_or(0, |p| lines.at(p.byte())),
                    column: None,
                    message: format!("The row cannot be read: {e}"),
                });
                continue;
            }
        };
        let row = parse_row(&record, sites, width, create_sites);
        if let Ok(Row {
            new_site: Some(hostname),
            ..
        }) = &row
            && !problems.new_sites.contains(hostname)
        {
            problems.new_sites.push(hostname.clone());
        }
        if let Err(problem) = row {
            // An unknown site is one problem however many rows it has.
            if problem.unknown_site {
                let site = record.get(0).unwrap_or("").trim().to_lowercase();
                if unknown_sites.contains(&site) {
                    continue;
                }
                unknown_sites.push(site);
            }
            problems.add(ImportProblem {
                line: record.position().map_or(0, |p| lines.at(p.byte())),
                column: problem.column.map(String::from),
                message: problem.message,
            });
        }
    }
    if rows == 0 {
        problems.file("The file has no rows below the header".into());
    }
    problems
}

struct Row {
    site_id: i64,
    /// The hostname, when the site does not exist yet.
    new_site: Option<String>,
    created_at: String,
    kind: &'static str,
    path: String,
    name: Option<String>,
    referrer: Option<String>,
    country: Option<String>,
    device: Option<String>,
    browser: Option<String>,
    os: Option<String>,
    visitor: Option<String>,
}

#[derive(Debug)]
struct RowProblem {
    column: Option<&'static str>,
    message: String,
    unknown_site: bool,
}

fn bad(column: &'static str, message: impl Into<String>) -> RowProblem {
    RowProblem {
        column: Some(column),
        message: message.into(),
        unknown_site: false,
    }
}

/// One row, cleaned the way a tracked hit is: facets lowercase, paths
/// without a query string or fragment, referrers reduced to the hostname.
fn parse_row(
    record: &csv::StringRecord,
    sites: &HashMap<String, i64>,
    width: usize,
    create_sites: bool,
) -> Result<Row, RowProblem> {
    if record.len() != width {
        return Err(RowProblem {
            unknown_site: false,
            column: None,
            message: format!("expected {width} columns, found {}", record.len()),
        });
    }
    let field = |i: usize| record.get(i).unwrap_or("").trim();

    let host = field(0).to_lowercase();
    let (site_id, new_site) = match sites.get(&host) {
        Some(&id) => (id, None),
        // A site to create: its id is known once it is inserted.
        None if create_sites => match validate_hostname(&host) {
            Ok(hostname) => (0, Some(hostname)),
            Err(_) => {
                return Err(RowProblem {
                    unknown_site: true,
                    ..bad("site", format!("\"{host}\" is not a valid hostname"))
                });
            }
        },
        None => {
            return Err(RowProblem {
                unknown_site: true,
                ..bad(
                    "site",
                    format!("unknown site \"{host}\"; create it first, or allow creating sites"),
                )
            });
        }
    };
    let created_at = DateTime::parse_from_rfc3339(field(1))
        .map_err(|_| {
            bad(
                "created_at",
                format!("\"{}\" is not an ISO 8601 date and time", field(1)),
            )
        })?
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let kind = match field(2) {
        "pageview" => "pageview",
        "click" => "click",
        other => {
            return Err(bad("kind", format!("\"{other}\" is not pageview or click")));
        }
    };
    let path = field(3);
    if path.is_empty() {
        return Err(bad("path", "is empty"));
    }
    let name = Some(field(4)).filter(|n| !n.is_empty());
    if name.is_some_and(|n| n.chars().count() > 500) {
        return Err(bad("name", "is longer than 500 characters"));
    }
    let country = Some(field(6).to_lowercase()).filter(|c| !c.is_empty());
    if country
        .as_deref()
        .is_some_and(|c| c.len() != 2 || !c.bytes().all(|b| b.is_ascii_lowercase()))
    {
        return Err(bad(
            "country",
            format!("\"{}\" is not a two-letter country code", field(6)),
        ));
    }
    // Empty is unknown, as for the country.
    let facet = |i: usize, column: &'static str| {
        let value = field(i).to_lowercase();
        if value.chars().count() > 64 {
            Err(bad(column, "is longer than 64 characters"))
        } else {
            Ok(Some(value).filter(|v| !v.is_empty()))
        }
    };
    Ok(Row {
        site_id,
        new_site,
        created_at,
        kind,
        path: clean_path(Some(path)),
        name: name.map(String::from),
        referrer: clean_referrer(Some(field(5)), &host),
        country,
        device: facet(7, "device")?,
        browser: facet(8, "browser")?,
        os: facet(9, "os")?,
        visitor: visitor(field(10))?,
    })
}

/// The visitor hash of a hit, if the file has one: opaque, as the export
/// wrote it (32 hex characters).
fn visitor(value: &str) -> Result<Option<String>, RowProblem> {
    if value.chars().count() > 64 {
        return Err(bad("visitor", "is longer than 64 characters"));
    }
    Ok(Some(value.to_string()).filter(|v| !v.is_empty()))
}

/// The multipart form of the request, as the spec describes it.
#[derive(ToSchema)]
#[allow(dead_code)]
struct ImportForm {
    /// The CSV file.
    #[schema(format = Binary, value_type = String)]
    file: String,
}
