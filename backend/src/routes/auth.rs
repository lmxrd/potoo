use crate::auth::{
    COOKIE, DATE_FORMATS, TIME_FORMATS, User, create_session, current_user, destroy_session,
    hash_password, verify_password,
};
use crate::error::HttpError;
use crate::state::AppState;
use crate::visitor::client_ip;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::OnceLock;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/session", get(session))
        .route("/setup", post(setup))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/email", post(change_email))
        .route("/settings", post(change_settings))
        .route("/password", post(change_password))
        .route("/users", get(list_users).post(create_user))
        .route(
            "/users/{id}",
            axum::routing::patch(change_role).delete(delete_user),
        )
        .route("/users/{id}/password", post(reset_password))
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
        let conn = state.db.lock().await;
        let user = current_user(&conn, token).ok_or_else(HttpError::unauthorized)?;
        Ok(AuthUser(user))
    }
}

fn require_admin(user: &AuthUser) -> Result<(), HttpError> {
    if user.0.is_admin == 1 {
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

fn parse_body<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, HttpError> {
    serde_json::from_str(body).map_err(|_| HttpError::bad_request("Invalid request body"))
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

async fn has_users(state: &AppState) -> Result<bool, HttpError> {
    let conn = state.db.lock().await;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    Ok(count > 0)
}

fn admin_count(conn: &rusqlite::Connection) -> Result<i64, HttpError> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM users WHERE is_admin = 1", [], |r| {
        r.get(0)
    })?;
    Ok(count)
}

// Session + first-run state. The frontend uses this to route every page.
async fn session(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, HttpError> {
    let needs_setup = !has_users(&state).await?;
    let user = if needs_setup {
        None
    } else {
        let conn = state.db.lock().await;
        current_user(&conn, jar.get(COOKIE).map(|c| c.value()))
    };
    Ok(Json(json!({ "needsSetup": needs_setup, "user": user })))
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

// Creates the admin. Only possible while there are no users at all.
async fn setup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    body: String,
) -> Result<Response, HttpError> {
    let creds = parse_body::<Credentials>(&body)?;
    let email = validate_email(&creds.email)?;
    let password = validate_password(&creds.password)?;
    let conn = state.db.lock().await;
    let inserted = conn.execute(
        "INSERT INTO users (email, password_hash, is_admin)
         SELECT ?1, ?2, 1 WHERE NOT EXISTS (SELECT 1 FROM users)",
        params![email, hash_password(&password)],
    )?;
    if inserted == 0 {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Already set up".into(),
        });
    }
    let id = conn.last_insert_rowid();
    let jar = create_session(&conn, jar, id, secure(&headers))?;
    Ok((StatusCode::CREATED, jar).into_response())
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, HttpError> {
    // A malformed body fails the same way as a wrong password.
    let Ok(creds) = serde_json::from_str::<Credentials>(&body) else {
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

    let conn = state.db.lock().await;
    let stored: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, password_hash FROM users WHERE email = ?1",
            params![&email],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    // Same work and same response whether or not the user exists.
    let (id, stored) = match stored {
        Some(found) => (found.0, found.1),
        None => (0, dummy_hash().to_string()),
    };
    if !verify_password(&creds.password, &stored) || id == 0 {
        drop(conn);
        state.throttle.record_failure(&key);
        return Err(HttpError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid email or password".into(),
        });
    }
    state.throttle.clear(&key);
    let jar = create_session(&conn, jar, id, secure(&headers))?;
    Ok((StatusCode::NO_CONTENT, jar).into_response())
}

async fn logout(State(state): State<AppState>, jar: CookieJar) -> Result<Response, HttpError> {
    let conn = state.db.lock().await;
    let jar = destroy_session(&conn, jar)?;
    Ok((StatusCode::NO_CONTENT, jar).into_response())
}

// Change the signed-in user's own email.
async fn change_email(
    State(state): State<AppState>,
    user: AuthUser,
    body: String,
) -> Result<Response, HttpError> {
    #[derive(Deserialize)]
    struct Body {
        email: String,
    }
    let body = parse_body::<Body>(&body)?;
    let email = validate_email(&body.email)?;
    let conn = state.db.lock().await;
    let taken: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM users WHERE email = ?1 AND id != ?2",
            params![&email, user.0.id],
            |r| r.get(0),
        )
        .optional()?;
    if taken.is_some() {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Email already in use".into(),
        });
    }
    conn.execute(
        "UPDATE users SET email = ?1 WHERE id = ?2",
        params![&email, user.0.id],
    )?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Change the signed-in user's own preferences.
async fn change_settings(
    State(state): State<AppState>,
    user: AuthUser,
    body: String,
) -> Result<Response, HttpError> {
    #[derive(Deserialize)]
    struct Body {
        date_format: Option<String>,
        time_format: Option<String>,
        theme: Option<String>,
    }
    let body = parse_body::<Body>(&body)?;
    if body.date_format.is_none() && body.time_format.is_none() && body.theme.is_none() {
        return Err(HttpError::bad_request("A setting to change is required"));
    }
    if let Some(format) = &body.date_format
        && !DATE_FORMATS.contains(&format.as_str())
    {
        return Err(HttpError::bad_request("Invalid setting"));
    }
    if let Some(format) = &body.time_format
        && !TIME_FORMATS.contains(&format.as_str())
    {
        return Err(HttpError::bad_request("Invalid setting"));
    }
    let theme = body.theme.as_deref().map(validate_theme).transpose()?;
    let conn = state.db.lock().await;
    if let Some(format) = &body.date_format {
        conn.execute(
            "UPDATE users SET date_format = ?1 WHERE id = ?2",
            params![format, user.0.id],
        )?;
    }
    if let Some(format) = &body.time_format {
        conn.execute(
            "UPDATE users SET time_format = ?1 WHERE id = ?2",
            params![format, user.0.id],
        )?;
    }
    if let Some(theme) = theme {
        conn.execute(
            "UPDATE users SET theme = ?1 WHERE id = ?2",
            params![theme, user.0.id],
        )?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Change the signed-in user's own password.
async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    body: String,
) -> Result<Response, HttpError> {
    #[derive(Deserialize)]
    struct Body {
        current_password: String,
        new_password: String,
    }
    let body = parse_body::<Body>(&body)?;
    if body.current_password.len() > 200 {
        return Err(HttpError::bad_request("Invalid password"));
    }
    let new_password = validate_password(&body.new_password)?;
    let conn = state.db.lock().await;
    let stored: Option<String> = conn
        .query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            params![user.0.id],
            |r| r.get(0),
        )
        .optional()?;
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
    conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2",
        params![hash_password(&new_password), user.0.id],
    )?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Users: admin only. Everyone created here can be an admin or not.
async fn list_users(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, HttpError> {
    require_admin(&user)?;
    let conn = state.db.lock().await;
    let mut stmt = conn.prepare("SELECT id, email, is_admin, created_at FROM users ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "email": row.get::<_, String>(1)?,
            "is_admin": row.get::<_, i64>(2)?,
            "created_at": row.get::<_, String>(3)?,
        }))
    })?;
    Ok(Json(json!(rows.collect::<Result<Vec<_>, _>>()?)))
}

async fn create_user(
    State(state): State<AppState>,
    user: AuthUser,
    body: String,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    #[derive(Deserialize)]
    struct Body {
        email: String,
        password: String,
        #[serde(default)]
        is_admin: bool,
    }
    let body = parse_body::<Body>(&body)?;
    let email = validate_email(&body.email)?;
    let password = validate_password(&body.password)?;
    let conn = state.db.lock().await;
    let taken: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM users WHERE email = ?1",
            params![&email],
            |r| r.get(0),
        )
        .optional()?;
    if taken.is_some() {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Email already in use".into(),
        });
    }
    conn.execute(
        "INSERT INTO users (email, password_hash, is_admin) VALUES (?1, ?2, ?3)",
        params![&email, hash_password(&password), body.is_admin as i64],
    )?;
    Ok(StatusCode::CREATED.into_response())
}

// Change the role of a user. The last admin cannot be demoted, so the
// app always has at least one admin left.
async fn change_role(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    body: String,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    #[derive(Deserialize)]
    struct Body {
        is_admin: bool,
    }
    let body = parse_body::<Body>(&body)?;
    let conn = state.db.lock().await;
    let target: Option<i64> = conn
        .query_row(
            "SELECT is_admin FROM users WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(target) = target else {
        return Err(HttpError {
            status: StatusCode::NOT_FOUND,
            message: "Not found".into(),
        });
    };
    if target == 1 && !body.is_admin && admin_count(&conn)? == 1 {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Cannot demote the last admin".into(),
        });
    }
    conn.execute(
        "UPDATE users SET is_admin = ?1 WHERE id = ?2",
        params![body.is_admin as i64, id],
    )?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn delete_user(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    let conn = state.db.lock().await;
    let target: Option<i64> = conn
        .query_row(
            "SELECT is_admin FROM users WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(target) = target else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    if target == 1 && admin_count(&conn)? == 1 {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: "Cannot delete the last admin".into(),
        });
    }
    conn.execute("DELETE FROM users WHERE id = ?1", params![id])?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// Admin sets a new password for a regular user. The admin account is
// excluded here, like deletion: admins change their own via /password.
async fn reset_password(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    body: String,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    #[derive(Deserialize)]
    struct Body {
        new_password: String,
    }
    let body = parse_body::<Body>(&body)?;
    let password = validate_password(&body.new_password)?;
    let conn = state.db.lock().await;
    let updated = conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2",
        params![hash_password(&password), id],
    )?;
    if updated == 0 {
        return Err(HttpError {
            status: StatusCode::NOT_FOUND,
            message: "Not found".into(),
        });
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
