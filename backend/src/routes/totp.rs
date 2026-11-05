//! Two-factor authentication: setting up an authenticator app, the second
//! step of a password sign-in, recovery codes, and an admin's reset.
//!
//! A password sign-in of a user with two-factor authentication on does not
//! start a session: it sets a short-lived signed cookie that names the user
//! (`challenge_cookie`), and `/login/totp` starts the session once a code
//! from the app, or a recovery code, is given. Single sign-on asks for no
//! code: the provider has its own second factor.

use crate::auth::{User, create_session};
use crate::error::{ErrorBody, HttpError};
use crate::extract::ApiJson;
use crate::routes::auth::{AuthUser, require_admin, secure};
use crate::state::AppState;
use crate::totp;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::{CookieJar, SignedCookieJar};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(login_totp))
        .routes(routes!(setup))
        .routes(routes!(enable))
        .routes(routes!(disable))
        .routes(routes!(regenerate_recovery_codes))
        .routes(routes!(reset))
}

/// How long the second step may wait after the password.
const CHALLENGE_TTL: Duration = Duration::from_secs(5 * 60);
const CHALLENGE_COOKIE: &str = "potoo_2fa";
const CHALLENGE_PATH: &str = "/api/auth/login";

/// The challenge cookie's name and path: like the single sign-on cookie, a
/// `__Host-` cookie over https (which needs the path `/`), so a sibling
/// subdomain cannot plant one; over plain http it goes to the login paths
/// only.
fn challenge_name(secure: bool) -> (&'static str, &'static str) {
    if secure {
        ("__Host-potoo_2fa", "/")
    } else {
        (CHALLENGE_COOKIE, CHALLENGE_PATH)
    }
}

fn challenge(value: String, secure: bool) -> Cookie<'static> {
    let (name, path) = challenge_name(secure);
    Cookie::build((name, value))
        .path(path)
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .max_age(time::Duration::seconds(CHALLENGE_TTL.as_secs() as i64))
        .build()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The signed cookie that lets this browser finish the sign-in of a user
/// whose password it gave: `<user id>.<expiry, unix seconds>`. It is signed
/// with the instance's key (made at start, like the single sign-on cookie's):
/// a restart makes the user sign in again.
pub(crate) fn challenge_cookie(state: &AppState, user_id: i64, secure: bool) -> SignedCookieJar {
    let expires = unix_now() + CHALLENGE_TTL.as_secs();
    SignedCookieJar::new(state.oidc.key().clone())
        .add(challenge(format!("{user_id}.{expires}"), secure))
}

/// The user of a valid, unexpired challenge cookie.
fn challenged_user(state: &AppState, headers: &HeaderMap) -> Option<i64> {
    let (name, _) = challenge_name(secure(headers));
    let value = SignedCookieJar::from_headers(headers, state.oidc.key().clone())
        .get(name)?
        .value()
        .to_string();
    let (user, expires) = value.split_once('.')?;
    let expires: u64 = expires.parse().ok()?;
    (expires > unix_now()).then_some(())?;
    user.parse().ok()
}

/// The throttle key of a user's codes. Per user, not per address: whoever
/// gets this far has the password, and may have many addresses.
fn throttle_key(user_id: i64) -> String {
    format!("totp|{user_id}")
}

fn too_many(retry_after: i64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", retry_after.to_string())],
        Json(json!({ "error": "Too many attempts, try again later" })),
    )
        .into_response()
}

fn invalid_code() -> HttpError {
    HttpError::bad_request("Invalid code")
}

/// Whether `code` is a valid code of the user's authenticator app or one of
/// their recovery codes; either is used up. False when two-factor
/// authentication is off. Failures count towards the user's throttle.
async fn use_code(state: &AppState, user_id: i64, code: &str) -> Result<bool, HttpError> {
    if code.len() > 64 {
        return Ok(false);
    }
    let row: Option<(Option<String>, i64)> =
        sqlx::query_as("SELECT totp_secret, totp_last_step FROM users WHERE id = ?1")
            .bind(user_id)
            .fetch_optional(state.db.reader())
            .await?;
    let Some((Some(secret), last_step)) = row else {
        return Ok(false);
    };
    let used = if let Some(step) =
        totp::verify(&secret, code, totp::current_step(), last_step.max(0) as u64)
    {
        // Only if no other request used this step (or a later one) since,
        // and the secret is still the one checked.
        sqlx::query(
            "UPDATE users SET totp_last_step = ?1
             WHERE id = ?2 AND totp_secret = ?3 AND totp_last_step < ?1",
        )
        .bind(step as i64)
        .bind(user_id)
        .bind(&secret)
        .execute(state.db.writer())
        .await?
        .rows_affected()
            == 1
    } else {
        let used = sqlx::query(
            "DELETE FROM recovery_codes WHERE user_id = ?1 AND code_hash = ?2
             AND EXISTS (SELECT 1 FROM users WHERE id = ?1 AND totp_secret IS NOT NULL)",
        )
        .bind(user_id)
        .bind(totp::recovery_code_hash(code))
        .execute(state.db.writer())
        .await?
        .rows_affected()
            == 1;
        if used {
            tracing::info!("user {user_id} used a recovery code");
        }
        used
    };
    if used {
        state.throttle.clear(&throttle_key(user_id));
    } else {
        state.throttle.record_failure(&throttle_key(user_id));
    }
    Ok(used)
}

/// Replace a user's recovery codes with new ones, returning them.
async fn new_recovery_codes(
    tx: &mut sqlx::SqliteConnection,
    user_id: i64,
) -> Result<Vec<String>, HttpError> {
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let codes = totp::new_recovery_codes();
    for code in &codes {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?1, ?2)")
            .bind(user_id)
            .bind(totp::recovery_code_hash(code))
            .execute(&mut *tx)
            .await?;
    }
    Ok(codes)
}

/// A code from the authenticator app (six digits), or a recovery code.
#[derive(Deserialize, ToSchema)]
pub struct TotpCode {
    pub code: String,
}

/// A new secret for an authenticator app, not in use until it is confirmed.
#[derive(Serialize, ToSchema)]
pub struct TotpSetup {
    /// The secret, base32, for typing into the app.
    pub secret: String,
    /// The `otpauth://` URI, for a QR code.
    pub uri: String,
}

/// Single-use recovery codes, shown this once.
#[derive(Serialize, ToSchema)]
pub struct RecoveryCodes {
    pub codes: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/login/totp",
    tag = "Auth",
    operation_id = "loginTotp",
    summary = "Sign in: the second step",
    description = "After `/login` answered `totp_required`: a code from the authenticator app, or a recovery code (used up), sets the session cookie. Needs the cookie that `/login` set, good for five minutes. After five wrong codes for a user within 15 minutes, their codes are refused for a while (429, with a Retry-After header).",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 204, description = "Signed in"),
        (status = 400, description = "Invalid code", body = ErrorBody),
        (status = 401, description = "No password step, or it expired: sign in again", body = ErrorBody),
        (status = 403, description = "Password sign-in is turned off (`OIDC_ONLY`)", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
)]
async fn login_totp(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, HttpError> {
    if !state.oidc.passwords_allowed() {
        return Err(HttpError::passwords_off());
    }
    let Some(user_id) = challenged_user(&state, &headers) else {
        return Err(HttpError {
            status: StatusCode::UNAUTHORIZED,
            message: "The sign-in expired, sign in again".into(),
        });
    };
    if let Some(retry_after) = state.throttle.locked_out(&throttle_key(user_id)) {
        return Ok(too_many(retry_after));
    }
    if !use_code(&state, user_id, &body.code).await? {
        return Err(invalid_code());
    }
    let secure = secure(&headers);
    let jar = create_session(state.db.writer(), jar, user_id, secure).await?;
    // The challenge is used up.
    let mut removal = challenge(String::new(), secure);
    removal.make_removal();
    Ok((StatusCode::NO_CONTENT, jar.add(removal)).into_response())
}

#[utoipa::path(
    post,
    path = "/totp/setup",
    tag = "Account",
    operation_id = "setupTotp",
    summary = "Start setting up two-factor authentication",
    description = "A new secret for an authenticator app. Nothing changes until `/totp/enable` confirms it with a code; starting again replaces it.",
    responses(
        (status = 200, description = "The secret", body = TotpSetup),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 409, description = "Two-factor authentication is already on", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn setup(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<TotpSetup>, HttpError> {
    let secret = totp::new_secret();
    let updated =
        sqlx::query("UPDATE users SET totp_pending = ?1 WHERE id = ?2 AND totp_secret IS NULL")
            .bind(&secret)
            .bind(user.id)
            .execute(state.db.writer())
            .await?
            .rows_affected();
    if updated == 0 {
        return Err(already_on());
    }
    let uri = totp::uri(&secret, &user.email)
        .ok_or_else(|| HttpError::internal("Cannot make the authenticator URI"))?;
    Ok(Json(TotpSetup { uri, secret }))
}

fn already_on() -> HttpError {
    HttpError {
        status: StatusCode::CONFLICT,
        message: "Two-factor authentication is already on".into(),
    }
}

#[utoipa::path(
    post,
    path = "/totp/enable",
    tag = "Account",
    operation_id = "enableTotp",
    summary = "Turn two-factor authentication on",
    description = "Confirms the secret of `/totp/setup` with a code from the app. Answers the recovery codes, shown this once.",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 200, description = "On", body = RecoveryCodes),
        (status = 400, description = "Invalid code, or no setup started", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 409, description = "Two-factor authentication is already on", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn enable(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, HttpError> {
    let key = throttle_key(user.id);
    if let Some(retry_after) = state.throttle.locked_out(&key) {
        return Ok(too_many(retry_after));
    }
    let mut tx = state.db.writer().begin().await?;
    let (secret, pending): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT totp_secret, totp_pending FROM users WHERE id = ?1")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    if secret.is_some() {
        return Err(already_on());
    }
    let Some(pending) = pending else {
        return Err(HttpError::bad_request("Start the setup first"));
    };
    let Some(step) = totp::verify(&pending, &body.code, totp::current_step(), 0) else {
        state.throttle.record_failure(&key);
        return Err(invalid_code());
    };
    sqlx::query(
        "UPDATE users SET totp_secret = totp_pending, totp_pending = NULL, totp_last_step = ?1
         WHERE id = ?2",
    )
    .bind(step as i64)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    let codes = new_recovery_codes(&mut tx, user.id).await?;
    tx.commit().await?;
    state.throttle.clear(&key);
    Ok(Json(RecoveryCodes { codes }).into_response())
}

#[utoipa::path(
    post,
    path = "/totp/disable",
    tag = "Account",
    operation_id = "disableTotp",
    summary = "Turn two-factor authentication off",
    description = "Needs a code from the app, or a recovery code. The recovery codes are deleted.",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 204, description = "Off"),
        (status = 400, description = "Invalid code, or two-factor authentication is off", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn disable(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, HttpError> {
    if let Some(retry_after) = state.throttle.locked_out(&throttle_key(user.id)) {
        return Ok(too_many(retry_after));
    }
    if !use_code(&state, user.id, &body.code).await? {
        return Err(invalid_code());
    }
    turn_off(&state, user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Two-factor authentication off for a user: no secret, no recovery codes.
/// False when there is no such user.
async fn turn_off(state: &AppState, user_id: i64) -> Result<bool, HttpError> {
    let mut tx = state.db.writer().begin().await?;
    let updated = sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_pending = NULL, totp_last_step = 0
         WHERE id = ?1",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(updated == 1)
}

#[utoipa::path(
    post,
    path = "/totp/recovery-codes",
    tag = "Account",
    operation_id = "regenerateRecoveryCodes",
    summary = "New recovery codes",
    description = "Needs a code from the app, or a recovery code. The old recovery codes stop working.",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 200, description = "The new codes, shown this once", body = RecoveryCodes),
        (status = 400, description = "Invalid code, or two-factor authentication is off", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn regenerate_recovery_codes(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, HttpError> {
    if let Some(retry_after) = state.throttle.locked_out(&throttle_key(user.id)) {
        return Ok(too_many(retry_after));
    }
    if !use_code(&state, user.id, &body.code).await? {
        return Err(invalid_code());
    }
    let mut tx = state.db.writer().begin().await?;
    let codes = new_recovery_codes(&mut tx, user.id).await?;
    tx.commit().await?;
    Ok(Json(RecoveryCodes { codes }).into_response())
}

// For a user who lost their authenticator and their recovery codes. Not for
// the admin's own account: they turn it off in their settings, with a code.
#[utoipa::path(
    delete,
    path = "/users/{id}/totp",
    tag = "Users",
    operation_id = "resetUserTotp",
    summary = "Turn off two-factor authentication for a user",
    description = "For a user who lost their authenticator app and their recovery codes; they can set it up again. Not for your own account.",
    params(("id" = i64, Path, description = "The user id")),
    responses(
        (status = 204, description = "Off"),
        (status = 400, description = "Your own account", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn reset(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    let User { id: own, .. } = user.0;
    if id == own {
        return Err(HttpError::bad_request(
            "Turn off your own two-factor authentication in your settings",
        ));
    }
    if !turn_off(&state, id).await? {
        return Err(HttpError {
            status: StatusCode::NOT_FOUND,
            message: "Not found".into(),
        });
    }
    tracing::info!("admin {own} turned off two-factor authentication for user {id}");
    Ok(StatusCode::NO_CONTENT.into_response())
}
