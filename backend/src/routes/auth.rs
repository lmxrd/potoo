use crate::auth::{
    COOKIE, DATE_FORMATS, User, create_session, current_user, destroy_session, hash_password,
    verify_password,
};
use crate::error::{ErrorBody, HttpError};
use crate::extract::ApiJson;
use crate::state::AppState;
use crate::visitor::client_ip;
use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteExecutor;
use std::net::SocketAddr;
use std::sync::OnceLock;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(session))
        .routes(routes!(setup))
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(change_email))
        .routes(routes!(change_settings))
        .routes(routes!(change_password))
        .routes(routes!(list_users, create_user))
        .routes(routes!(change_role, delete_user))
        .routes(routes!(reset_password))
}

/// The signed-in user, as a request extractor: a valid session cookie or
/// a 401.
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| HttpError::unauthorized())?;
        let token = jar.get(COOKIE).map(|c| c.value());
        let user = current_user(state.db.reader(), token)
            .await
            .ok_or_else(HttpError::unauthorized)?;
        Ok(AuthUser(user))
    }
}

fn require_admin(user: &AuthUser) -> Result<(), HttpError> {
    if user.0.is_admin {
        Ok(())
    } else {
        Err(HttpError::forbidden())
    }
}

/// A theme is one small JSON object — {"base": "light" | "dark",
/// "accent": "#rrggbb"} — the same form the presets send. Anything else
/// is a 400; it is cosmetic, but it is still stored.
fn validate_theme(raw: &str) -> Result<String, HttpError> {
    let invalid = || HttpError::bad_request("Invalid theme");
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| invalid())?;
    let base = value
        .get("base")
        .and_then(|v| v.as_str())
        .ok_or_else(invalid)?;
    if !matches!(base, "light" | "dark") {
        return Err(invalid());
    }
    let accent = value
        .get("accent")
        .and_then(|v| v.as_str())
        .ok_or_else(invalid)?
        .to_ascii_lowercase();
    let bytes = accent.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    Ok(format!("{{\"base\":\"{base}\",\"accent\":\"{accent}\"}}"))
}

/// A valid hash to verify against when the user does not exist, so the
/// response takes the same time either way.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| hash_password("potoo-dummy"))
}

/// The cookie is secure behind an https proxy that says so.
fn secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

fn validate_email(raw: &str) -> Result<String, HttpError> {
    let email = raw.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.contains(char::is_whitespace)
        && email.split('@').collect::<Vec<_>>().len() == 2
        && email
            .split_once('@')
            .map(|(local, domain)| {
                !local.is_empty()
                    && domain.contains('.')
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
            })
            .unwrap_or(false);
    if valid {
        Ok(email)
    } else {
        Err(HttpError::bad_request("Expected an email address"))
    }
}

fn validate_password(raw: &str) -> Result<String, HttpError> {
    if raw.len() >= 10 && raw.len() <= 200 {
        Ok(raw.to_string())
    } else {
        Err(HttpError::bad_request(
            "Expected a password of 10 to 200 characters",
        ))
    }
}

/// The session and first-run state; the frontend routes every page by it.
#[derive(Serialize, ToSchema)]
pub struct Session {
    /// True while there are no users at all: the next step is `/setup`.
    #[serde(rename = "needsSetup")]
    pub needs_setup: bool,
    /// The signed-in user: null when nobody is signed in. Always present.
    #[schema(required = true)]
    pub user: Option<User>,
}

/// A new email address for the signed-in user.
#[derive(Deserialize, ToSchema)]
pub struct ChangeEmail {
    pub email: String,
}

/// The preferences to change; at least one is required.
#[derive(Deserialize, ToSchema)]
pub struct ChangeSettings {
    /// One of `YYYY-MM-DD`, `DD/MM/YYYY`, `MM/DD/YYYY`, `DD.MM.YYYY`.
    pub date_format: Option<String>,
    /// Whether times are shown in 24-hour format.
    pub use_24: Option<bool>,
    /// A JSON object as text: `{"base":"light"|"dark","accent":"#rrggbb"}`.
    pub theme: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePassword {
    pub current_password: String,
    /// 10 to 200 characters.
    pub new_password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateUser {
    pub email: String,
    /// 10 to 200 characters.
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct ChangeRole {
    pub is_admin: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct ResetPassword {
    /// 10 to 200 characters.
    pub new_password: String,
}

/// One user in the list of users.
#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub is_admin: bool,
    pub created_at: String,
}

async fn has_users(state: &AppState) -> Result<bool, HttpError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(state.db.reader())
        .await?;
    Ok(count > 0)
}

async fn admin_count<'e>(db: impl SqliteExecutor<'e>) -> Result<i64, HttpError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_admin = 1")
        .fetch_one(db)
        .await?;
    Ok(count)
}

// Session + first-run state. The frontend uses this to route every page.
#[utoipa::path(
    get,
    path = "/session",
    tag = "Auth",
    operation_id = "getSession",
    summary = "The session and the first-run state",
    description = "Whether the first admin still has to be created, and who is signed in (null when nobody is).",
    responses(
        (status = 200, description = "The session", body = Session),
    ),
)]
async fn session(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Session>, HttpError> {
    let needs_setup = !has_users(&state).await?;
    let user = if needs_setup {
        None
    } else {
        current_user(state.db.reader(), jar.get(COOKIE).map(|c| c.value())).await
    };
    Ok(Json(Session { needs_setup, user }))
}

/// An email and a password, to sign in or to create the first admin.
#[derive(Deserialize, ToSchema)]
pub struct Credentials {
    pub email: String,
    /// 10 to 200 characters when creating an account.
    pub password: String,
}

// Creates the admin. Only possible while there are no users at all.
#[utoipa::path(
    post,
    path = "/setup",
    tag = "Auth",
    operation_id = "setup",
    summary = "Create the first admin",
    description = "Only possible while there are no users. Signs the new admin in: the session cookie is set.",
    request_body(content = Credentials, content_type = "application/json"),
    responses(
        (status = 201, description = "The admin was created and signed in"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 409, description = "Conflict", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
)]
async fn setup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    ApiJson(creds): ApiJson<Credentials>,
) -> Result<Response, HttpError> {
    let email = validate_email(&creds.email)?;
    let password = validate_password(&creds.password)?;
    let inserted = sqlx::query(
        "INSERT INTO users (email, password_hash, is_admin)
         SELECT ?1, ?2, 1 WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(email)
    .bind(hash_password(&password))
    .execute(state.db.writer())
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Already set up".into(),
        });
    }
    let jar = create_session(
        state.db.writer(),
        jar,
        inserted.last_insert_rowid(),
        secure(&headers),
    )
    .await?;
    Ok((StatusCode::CREATED, jar).into_response())
}

#[utoipa::path(
    post,
    path = "/login",
    tag = "Auth",
    operation_id = "login",
    summary = "Sign in",
    description = "Sets the session cookie. After five failed attempts for one IP and email within 15 minutes the pair is locked out (429, with a Retry-After header).",
    request_body(content = Credentials, content_type = "application/json"),
    responses(
        (status = 204, description = "Signed in"),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
)]
async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<ApiJson<Credentials>, HttpError>,
) -> Result<Response, HttpError> {
    // A malformed body fails the same way as a wrong password.
    let Ok(ApiJson(creds)) = body else {
        return Err(HttpError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid email or password".into(),
        });
    };
    let email = validate_email(&creds.email).map_err(|_| HttpError {
        status: StatusCode::UNAUTHORIZED,
        message: "Invalid email or password".into(),
    })?;

    let key = format!(
        "{}|{email}",
        client_ip(&headers, Some(addr.ip()), state.trust_proxy)
    );
    if let Some(retry_after) = state.throttle.locked_out(&key) {
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", retry_after.to_string())],
            Json(json!({ "error": "Too many attempts, try again later" })),
        )
            .into_response());
    }

    let stored: Option<(i64, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?1")
            .bind(&email)
            .fetch_optional(state.db.reader())
            .await?;
    // Same work and same response whether or not the user exists.
    let (id, stored) = match stored {
        Some(found) => (found.0, found.1),
        None => (0, dummy_hash().to_string()),
    };
    if !verify_password(&creds.password, &stored) || id == 0 {
        state.throttle.record_failure(&key);
        return Err(HttpError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid email or password".into(),
        });
    }
    state.throttle.clear(&key);
    let jar = create_session(state.db.writer(), jar, id, secure(&headers)).await?;
    Ok((StatusCode::NO_CONTENT, jar).into_response())
}

#[utoipa::path(
    post,
    path = "/logout",
    tag = "Auth",
    operation_id = "logout",
    summary = "Sign out",
    responses(
        (status = 204, description = "Signed out; the session cookie is cleared"),
    ),
)]
async fn logout(State(state): State<AppState>, jar: CookieJar) -> Result<Response, HttpError> {
    let jar = destroy_session(state.db.writer(), jar).await?;
    Ok((StatusCode::NO_CONTENT, jar).into_response())
}

// Change the signed-in user's own email.
#[utoipa::path(
    post,
    path = "/email",
    tag = "Account",
    operation_id = "changeEmail",
    summary = "Change your own email",
    request_body(content = ChangeEmail, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 409, description = "Conflict", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn change_email(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(body): ApiJson<ChangeEmail>,
) -> Result<Response, HttpError> {
    let email = validate_email(&body.email)?;
    let mut tx = state.db.writer().begin().await?;
    let taken: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM users WHERE email = ?1 AND id != ?2")
            .bind(&email)
            .bind(user.0.id)
            .fetch_optional(&mut *tx)
            .await?;
    if taken.is_some() {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Email already in use".into(),
        });
    }
    sqlx::query("UPDATE users SET email = ?1 WHERE id = ?2")
        .bind(&email)
        .bind(user.0.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Change the signed-in user's own preferences.
#[utoipa::path(
    post,
    path = "/settings",
    tag = "Account",
    operation_id = "changeSettings",
    summary = "Change your own preferences",
    description = "The date format, the time format and the theme. Each one that is sent changes, or none does.",
    request_body(content = ChangeSettings, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn change_settings(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(body): ApiJson<ChangeSettings>,
) -> Result<Response, HttpError> {
    if body.date_format.is_none() && body.use_24.is_none() && body.theme.is_none() {
        return Err(HttpError::bad_request("A setting to change is required"));
    }
    if let Some(format) = &body.date_format
        && !DATE_FORMATS.contains(&format.as_str())
    {
        return Err(HttpError::bad_request("Invalid setting"));
    }
    let theme = body.theme.as_deref().map(validate_theme).transpose()?;
    // Each setting that was sent changes, or none does.
    let mut tx = state.db.writer().begin().await?;
    if let Some(format) = &body.date_format {
        sqlx::query("UPDATE users SET date_format = ?1 WHERE id = ?2")
            .bind(format)
            .bind(user.0.id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(use_24) = body.use_24 {
        sqlx::query("UPDATE users SET use_24 = ?1 WHERE id = ?2")
            .bind(use_24)
            .bind(user.0.id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(theme) = theme {
        sqlx::query("UPDATE users SET theme = ?1 WHERE id = ?2")
            .bind(theme)
            .bind(user.0.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Change the signed-in user's own password.
#[utoipa::path(
    post,
    path = "/password",
    tag = "Account",
    operation_id = "changePassword",
    summary = "Change your own password",
    request_body(content = ChangePassword, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(body): ApiJson<ChangePassword>,
) -> Result<Response, HttpError> {
    if body.current_password.len() > 200 {
        return Err(HttpError::bad_request("Invalid password"));
    }
    let new_password = validate_password(&body.new_password)?;
    // The hashing below takes a while: no connection is held meanwhile.
    let stored: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?1")
            .bind(user.0.id)
            .fetch_optional(state.db.reader())
            .await?;
    let Some(stored) = stored else {
        return Err(HttpError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid password".into(),
        });
    };
    if !verify_password(&body.current_password, &stored) {
        return Err(HttpError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid password".into(),
        });
    }
    sqlx::query("UPDATE users SET password_hash = ?1 WHERE id = ?2")
        .bind(hash_password(&new_password))
        .bind(user.0.id)
        .execute(state.db.writer())
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Users: admin only. Everyone created here can be an admin or not.
#[utoipa::path(
    get,
    path = "/users",
    tag = "Users",
    operation_id = "listUsers",
    summary = "List the users",
    responses(
        (status = 200, description = "All users, by id", body = Vec<UserRow>),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn list_users(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<UserRow>>, HttpError> {
    require_admin(&user)?;
    let rows: Vec<UserRow> =
        sqlx::query_as("SELECT id, email, is_admin, created_at FROM users ORDER BY id")
            .fetch_all(state.db.reader())
            .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    post,
    path = "/users",
    tag = "Users",
    operation_id = "createUser",
    summary = "Create a user",
    request_body(content = CreateUser, content_type = "application/json"),
    responses(
        (status = 201, description = "Created"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 409, description = "Conflict", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn create_user(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(body): ApiJson<CreateUser>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    let email = validate_email(&body.email)?;
    let password = validate_password(&body.password)?;
    let password_hash = hash_password(&password);
    let mut tx = state.db.writer().begin().await?;
    let taken: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE email = ?1")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
    if taken.is_some() {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Email already in use".into(),
        });
    }
    sqlx::query("INSERT INTO users (email, password_hash, is_admin) VALUES (?1, ?2, ?3)")
        .bind(&email)
        .bind(password_hash)
        .bind(body.is_admin as i64)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::CREATED.into_response())
}

// Change the role of a user. The last admin cannot be demoted, so the
// app always has at least one admin left.
#[utoipa::path(
    patch,
    path = "/users/{id}",
    tag = "Users",
    operation_id = "changeUserRole",
    summary = "Change the role of a user",
    description = "The last admin cannot be demoted.",
    params(("id" = i64, Path, description = "The user id")),
    request_body(content = ChangeRole, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 409, description = "Conflict", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn change_role(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    ApiJson(body): ApiJson<ChangeRole>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    let mut tx = state.db.writer().begin().await?;
    let target: Option<i64> = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(target) = target else {
        return Err(HttpError {
            status: StatusCode::NOT_FOUND,
            message: "Not found".into(),
        });
    };
    if target == 1 && !body.is_admin && admin_count(&mut *tx).await? == 1 {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Cannot demote the last admin".into(),
        });
    }
    sqlx::query("UPDATE users SET is_admin = ?1 WHERE id = ?2")
        .bind(body.is_admin as i64)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    delete,
    path = "/users/{id}",
    tag = "Users",
    operation_id = "deleteUser",
    summary = "Delete a user",
    description = "The last admin cannot be deleted. Deleting a user that does not exist is not an error.",
    params(("id" = i64, Path, description = "The user id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 409, description = "Conflict", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn delete_user(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    let mut tx = state.db.writer().begin().await?;
    let target: Option<i64> = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(target) = target else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    if target == 1 && admin_count(&mut *tx).await? == 1 {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Cannot delete the last admin".into(),
        });
    }
    sqlx::query("DELETE FROM users WHERE id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Admin sets a new password for a regular user. The admin account is
// excluded here, like deletion: admins change their own via /password.
#[utoipa::path(
    post,
    path = "/users/{id}/password",
    tag = "Users",
    operation_id = "resetUserPassword",
    summary = "Set a new password for a user",
    params(("id" = i64, Path, description = "The user id")),
    request_body(content = ResetPassword, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn reset_password(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    ApiJson(body): ApiJson<ResetPassword>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    let password = validate_password(&body.new_password)?;
    let updated = sqlx::query("UPDATE users SET password_hash = ?1 WHERE id = ?2")
        .bind(hash_password(&password))
        .bind(id)
        .execute(state.db.writer())
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(HttpError {
            status: StatusCode::NOT_FOUND,
            message: "Not found".into(),
        });
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
