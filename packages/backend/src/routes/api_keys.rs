//! The signed-in user's API keys: tokens for scripts, sent as
//! `Authorization: Bearer <token>`. Managed with the session only (a key
//! cannot list, make or revoke keys); the token is shown once, on creation.

use crate::auth::SessionUser;
use crate::auth::{API_KEY_PREFIX, Scope};
use crate::error::{ApiError, ErrorBody};
use crate::extract::ApiJson;
use crate::state::AppState;
use crate::util::{random_hex, sha256_hex};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_keys, create_key))
        .routes(routes!(revoke_key))
}

/// The most keys one user may have, so a script gone wrong stops somewhere.
const MAX_KEYS: i64 = 50;

/// An API key, without its token.
#[derive(Serialize, ToSchema)]
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    /// The start of the token, to tell keys apart, e.g. `potoo_3f9a1c`.
    pub prefix: String,
    pub scope: Scope,
    pub created_at: String,
    /// Null until the key is used; updated at most once a minute.
    #[schema(required = true)]
    pub last_used_at: Option<String>,
    /// Null for a key that does not expire.
    #[schema(required = true)]
    pub expires_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    name: String,
    prefix: String,
    scope: String,
    created_at: String,
    last_used_at: Option<String>,
    expires_at: Option<String>,
}

impl ApiKeyRow {
    fn into_key(self) -> ApiKey {
        ApiKey {
            id: self.id,
            name: self.name,
            prefix: self.prefix,
            // The table's CHECK allows nothing else.
            scope: Scope::parse(&self.scope).unwrap_or(Scope::Read),
            created_at: self.created_at,
            last_used_at: self.last_used_at,
            expires_at: self.expires_at,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct CreateApiKey {
    /// What the key is for, 1 to 100 characters.
    pub name: String,
    /// `admin` for admins only.
    pub scope: Scope,
    /// 1 to 3650; left out for a key that does not expire.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

/// A new key, with its token: shown this once.
#[derive(Serialize, ToSchema)]
pub struct CreatedApiKey {
    pub key: ApiKey,
    /// `potoo_` and 64 hex characters.
    pub token: String,
}

const COLUMNS: &str = "id, name, prefix, scope, created_at, last_used_at, expires_at";

#[utoipa::path(
    get,
    path = "/api-keys",
    tag = "Account",
    operation_id = "listApiKeys",
    summary = "List your API keys",
    description = "Your own keys, newest first, expired ones included. Not with an API key.",
    responses(
        (status = 200, description = "The keys", body = Vec<ApiKey>),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not with an API key", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn list_keys(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
) -> Result<Json<Vec<ApiKey>>, ApiError> {
    let rows: Vec<ApiKeyRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM api_keys WHERE user_id = ?1 ORDER BY id DESC"
    )))
    .bind(user.id)
    .fetch_all(state.db.reader())
    .await?;
    Ok(Json(rows.into_iter().map(ApiKeyRow::into_key).collect()))
}

#[utoipa::path(
    post,
    path = "/api-keys",
    tag = "Account",
    operation_id = "createApiKey",
    summary = "Create an API key",
    description = "Answers the token, shown this once: send it as `Authorization: Bearer <token>`. A `read` key may only GET; a `write` key may also change things but reaches no admin endpoint; an `admin` key (admins only) may do what its user may. No key reaches your account's settings or keys. At most 50 keys per user.",
    request_body(content = CreateApiKey, content_type = "application/json"),
    responses(
        (status = 201, description = "Created", body = CreatedApiKey),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not with an API key, or an admin key for a user who is not an admin", body = ErrorBody),
        (status = 409, description = "Too many keys", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn create_key(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    ApiJson(body): ApiJson<CreateApiKey>,
) -> Result<Response, ApiError> {
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::bad_request(
            "Expected a name of 1 to 100 characters",
        ));
    }
    if body.scope == Scope::Admin && !user.is_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Only admins can create admin keys",
        ));
    }
    let expires_at = match body.expires_in_days {
        None => None,
        Some(days @ 1..=3650) => Some(crate::time::days_from_now_iso(days as i64)),
        Some(_) => {
            return Err(ApiError::bad_request(
                "Expected an expiry of 1 to 3650 days",
            ));
        }
    };
    let token = format!("{API_KEY_PREFIX}{}", random_hex(32));
    let prefix = token[..API_KEY_PREFIX.len() + 6].to_string();

    let mut tx = state.db.writer().begin().await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE user_id = ?1")
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_KEYS {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "Too many API keys: revoke one first",
        ));
    }
    let row: ApiKeyRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO api_keys (id, user_id, name, prefix, token_hash, scope, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING {COLUMNS}"
    )))
    .bind(Uuid::now_v7())
    .bind(user.id)
    .bind(name)
    .bind(&prefix)
    .bind(sha256_hex(&token))
    .bind(body.scope.as_str())
    .bind(expires_at)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiKey {
            key: row.into_key(),
            token,
        }),
    )
        .into_response())
}

#[utoipa::path(
    delete,
    path = "/api-keys/{id}",
    tag = "Account",
    operation_id = "revokeApiKey",
    summary = "Revoke an API key",
    description = "It stops working at once. Not with an API key.",
    params(("id" = Uuid, Path, description = "The key id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not with an API key", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn revoke_key(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let deleted = sqlx::query("DELETE FROM api_keys WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(state.db.writer())
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "Not found"));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
