//! Two-factor authentication: setting up an authenticator app, the second
//! step of a sign-in, recovery codes, and an admin's reset.
//!
//! A sign-in (password or single sign-on) of a user with two-factor
//! authentication on does not start a session: it sets a short-lived signed
//! cookie that names the user (`challenge_cookie`), and `/login/totp` starts
//! the session once a code from the app, or a recovery code, is given. With
//! `TOTP_REQUIRED`, a user without it gets that cookie in its enrolment mode
//! instead: `/login/totp/setup` and `/login/totp/enable` set the app up, and
//! the latter starts the session.

use crate::auth::User;
use crate::auth::{CurrentUser, SessionOnlyUser, require_admin};
use crate::error::{ApiError, ErrorBody};
use crate::extract::ApiJson;
use crate::limits::UserAction;
use crate::sessions::{self, AuthSession};
use crate::state::AppState;
use crate::totp;
use crate::visitor::client_ip;
use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;
use axum_login::AuthnBackend as _;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(login_totp))
        .routes(routes!(login_totp_setup))
        .routes(routes!(login_totp_enable))
        .routes(routes!(totp_setup))
        .routes(routes!(totp_enable))
        .routes(routes!(totp_disable))
        .routes(routes!(totp_recovery_codes))
        .routes(routes!(totp_replace))
        .routes(routes!(totp_replace_confirm))
}

/// Turning off a user's two-factor authentication, nested under /admin.
pub fn admin_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(admin_reset_totp))
}

/// How long the second step may wait after the password.
/// Sign in the user of a sign-in that took its last step: the session,
/// and the challenge cookie used up (the jar to answer with).
async fn finish_sign_in(
    state: &AppState,
    auth: &mut AuthSession,
    user_id: Uuid,
) -> Result<CookieJar, ApiError> {
    let user = auth
        .backend
        .get_user(&user_id)
        .await?
        .ok_or_else(ApiError::unauthorized)?
        .user;
    let jar = sessions::sign_in(auth, state, &user).await?;
    let mut removal = totp::challenge(String::new(), state.secure_cookies);
    removal.make_removal();
    Ok(jar.add(removal))
}

fn invalid_code() -> ApiError {
    ApiError::bad_request("Invalid code")
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

/// Sign in: the second step.
///
/// After `/login` answered `totp_required` (or single sign-on sent the browser to `/login?step=totp`):
/// a code from the authenticator app, or a recovery code (used up), sets the session cookie. Needs the
/// cookie the first step set, good for five minutes. After five wrong codes for a user within 15
/// minutes, their codes are refused for a while (429, with a Retry-After header).
#[utoipa::path(
    post,
    path = "/login/totp",
    tag = "Auth",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 204, description = "Signed in"),
        (status = 400, description = "Invalid code", body = ErrorBody),
        (status = 401, description = "No first step, or it expired: sign in again", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
)]
async fn login_totp(
    State(state): State<AppState>,
    mut auth: AuthSession,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    let user_id = totp::challenged_user(&state, &headers, totp::Step::Code)?;
    state.limits.address(Some(&client_ip(
        &headers,
        Some(addr.ip()),
        state.trust_proxy,
    )))?;
    state.limits.user(UserAction::Code, user_id)?;
    if !totp::use_code(&state, user_id, &body.code).await? {
        return Err(invalid_code());
    }
    let jar = finish_sign_in(&state, &mut auth, user_id).await?;
    Ok((StatusCode::NO_CONTENT, jar).into_response())
}

/// Sign in: set up the authenticator app.
///
/// With `TOTP_REQUIRED`, after `/login` answered `totp_setup_required` (or single sign-on sent the
/// browser to `/login?step=totp-setup`): a new secret for the app, as `/totp/setup` gives. Starting
/// again replaces it.
#[utoipa::path(
    post,
    path = "/login/totp/setup",
    tag = "Auth",
    responses(
        (status = 200, description = "The secret", body = TotpSetup),
        (status = 401, description = "No first step, or it expired: sign in again", body = ErrorBody),
        (status = 409, description = "Two-factor authentication is already on", body = ErrorBody),
    ),
)]
async fn login_totp_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<TotpSetup>, ApiError> {
    let user_id = totp::challenged_user(&state, &headers, totp::Step::Enroll)?;
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?1")
        .bind(user_id)
        .fetch_optional(state.db.reader())
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    Ok(Json(start_setup(&state, user_id, &email).await?))
}

/// Sign in: turn two-factor authentication on.
///
/// Confirms the secret of `/login/totp/setup` with a code from the app, sets the session cookie, and
/// answers the recovery codes, shown this once.
#[utoipa::path(
    post,
    path = "/login/totp/enable",
    tag = "Auth",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 200, description = "On, and signed in", body = RecoveryCodes),
        (status = 400, description = "Invalid code, or no setup started", body = ErrorBody),
        (status = 401, description = "No first step, or it expired: sign in again", body = ErrorBody),
        (status = 409, description = "Two-factor authentication is already on", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
)]
async fn login_totp_enable(
    State(state): State<AppState>,
    mut auth: AuthSession,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    let user_id = totp::challenged_user(&state, &headers, totp::Step::Enroll)?;
    state.limits.address(Some(&client_ip(
        &headers,
        Some(addr.ip()),
        state.trust_proxy,
    )))?;
    state.limits.user(UserAction::Code, user_id)?;
    let codes = confirm_setup(&state, user_id, &body.code).await?;
    let jar = finish_sign_in(&state, &mut auth, user_id).await?;
    Ok((jar, Json(RecoveryCodes { codes })).into_response())
}

/// Start setting up two-factor authentication.
///
/// A new secret for an authenticator app. Nothing changes until `/totp/enable` confirms it with a code;
/// starting again replaces it.
#[utoipa::path(
    post,
    path = "/totp/setup",
    tag = "Account",
    responses(
        (status = 200, description = "The secret", body = TotpSetup),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 409, description = "Two-factor authentication is already on", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn totp_setup(
    State(state): State<AppState>,
    SessionOnlyUser(user): SessionOnlyUser,
) -> Result<Json<TotpSetup>, ApiError> {
    Ok(Json(start_setup(&state, user.id, &user.email).await?))
}

/// A new pending secret for a user who has two-factor authentication off.
async fn start_setup(state: &AppState, user_id: Uuid, email: &str) -> Result<TotpSetup, ApiError> {
    let secret = totp::new_secret();
    let updated =
        sqlx::query("UPDATE users SET totp_pending = ?1 WHERE id = ?2 AND totp_secret IS NULL")
            .bind(&secret)
            .bind(user_id)
            .execute(state.db.writer())
            .await?
            .rows_affected();
    if updated == 0 {
        return Err(already_on());
    }
    let uri = totp::uri(&secret, email)
        .ok_or_else(|| ApiError::internal("Cannot make the authenticator URI"))?;
    Ok(TotpSetup { uri, secret })
}

fn already_on() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "Two-factor authentication is already on",
    )
}

/// Turn two-factor authentication on.
///
/// Confirms the secret of `/totp/setup` with a code from the app. Answers the recovery codes, shown
/// this once.
#[utoipa::path(
    post,
    path = "/totp/enable",
    tag = "Account",
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
async fn totp_enable(
    State(state): State<AppState>,
    SessionOnlyUser(user): SessionOnlyUser,
    mut auth: AuthSession,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    state.limits.user(UserAction::Code, user.id)?;
    let codes = confirm_setup(&state, user.id, &body.code).await?;
    // The secret is part of what sessions are checked against: the user's
    // other sessions are signed out, this one stays.
    sessions::refresh(&mut auth, user.id).await?;
    Ok(Json(RecoveryCodes { codes }).into_response())
}

/// Turn two-factor authentication on with the pending secret, confirmed by
/// a code of it: the new recovery codes. The caller counted the attempt
/// (`state.limits`).
async fn confirm_setup(
    state: &AppState,
    user_id: Uuid,
    code: &str,
) -> Result<Vec<String>, ApiError> {
    let mut tx = state.db.writer().begin().await?;
    let (secret, pending): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT totp_secret, totp_pending FROM users WHERE id = ?1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
    if secret.is_some() {
        return Err(already_on());
    }
    let Some(pending) = pending else {
        return Err(ApiError::bad_request("Start the setup first"));
    };
    let Some(step) = totp::verify(&pending, code, totp::current_step(), 0) else {
        return Err(invalid_code());
    };
    sqlx::query(
        "UPDATE users SET totp_secret = totp_pending, totp_pending = NULL, totp_last_step = ?1
         WHERE id = ?2",
    )
    .bind(step as i64)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    let codes = totp::replace_recovery_codes(&mut tx, user_id).await?;
    tx.commit().await?;
    Ok(codes)
}

/// Turn two-factor authentication off.
///
/// Needs a code from the app, or a recovery code. The recovery codes are deleted. Refused while
/// `TOTP_REQUIRED` is set.
#[utoipa::path(
    post,
    path = "/totp/disable",
    tag = "Account",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 204, description = "Off"),
        (status = 400, description = "Invalid code, or two-factor authentication is off", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Two-factor authentication is required (`TOTP_REQUIRED`)", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn totp_disable(
    State(state): State<AppState>,
    SessionOnlyUser(user): SessionOnlyUser,
    mut auth: AuthSession,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    if state.totp_required {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Two-factor authentication is required here",
        ));
    }
    state.limits.user(UserAction::Code, user.id)?;
    if !totp::use_code(&state, user.id, &body.code).await? {
        return Err(invalid_code());
    }
    totp::remove(&state, user.id).await?;
    sessions::refresh(&mut auth, user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Start replacing the authenticator app.
///
/// For a new phone or app: a code from the current app, or a recovery code, gives a new secret. Two-
/// factor authentication stays on, with the current app, until `/totp/replace/confirm` confirms the new
/// one; so this works while it is required (`TOTP_REQUIRED`) too. Starting again replaces the new
/// secret.
#[utoipa::path(
    post,
    path = "/totp/replace",
    tag = "Account",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 200, description = "The new secret", body = TotpSetup),
        (status = 400, description = "Invalid code, or two-factor authentication is off", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not with an API key", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn totp_replace(
    State(state): State<AppState>,
    SessionOnlyUser(user): SessionOnlyUser,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    state.limits.user(UserAction::Code, user.id)?;
    if !totp::use_code(&state, user.id, &body.code).await? {
        return Err(invalid_code());
    }
    let secret = totp::new_secret();
    let updated =
        sqlx::query("UPDATE users SET totp_pending = ?1 WHERE id = ?2 AND totp_secret IS NOT NULL")
            .bind(&secret)
            .bind(user.id)
            .execute(state.db.writer())
            .await?
            .rows_affected();
    if updated == 0 {
        return Err(ApiError::bad_request("Two-factor authentication is off"));
    }
    let uri = totp::uri(&secret, &user.email)
        .ok_or_else(|| ApiError::internal("Cannot make the authenticator URI"))?;
    Ok(Json(TotpSetup { uri, secret }).into_response())
}

/// Finish replacing the authenticator app.
///
/// A code from the new app: its secret replaces the old one, which stops working. The recovery codes
/// are kept. A wrong code changes nothing.
#[utoipa::path(
    post,
    path = "/totp/replace/confirm",
    tag = "Account",
    request_body(content = TotpCode, content_type = "application/json"),
    responses(
        (status = 204, description = "Replaced"),
        (status = 400, description = "Invalid code, or no replacement started", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not with an API key", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn totp_replace_confirm(
    State(state): State<AppState>,
    SessionOnlyUser(user): SessionOnlyUser,
    mut auth: AuthSession,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    state.limits.user(UserAction::Code, user.id)?;
    let mut tx = state.db.writer().begin().await?;
    let (secret, pending): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT totp_secret, totp_pending FROM users WHERE id = ?1")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    let (Some(_), Some(pending)) = (secret, pending) else {
        return Err(ApiError::bad_request("Start the replacement first"));
    };
    let Some(step) = totp::verify(&pending, &body.code, totp::current_step(), 0) else {
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
    tx.commit().await?;
    sessions::refresh(&mut auth, user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// New recovery codes.
///
/// Needs a code from the app, or a recovery code. The old recovery codes stop working.
#[utoipa::path(
    post,
    path = "/totp/recovery-codes",
    tag = "Account",
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
async fn totp_recovery_codes(
    State(state): State<AppState>,
    SessionOnlyUser(user): SessionOnlyUser,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, ApiError> {
    state.limits.user(UserAction::Code, user.id)?;
    if !totp::use_code(&state, user.id, &body.code).await? {
        return Err(invalid_code());
    }
    let mut tx = state.db.writer().begin().await?;
    let codes = totp::replace_recovery_codes(&mut tx, user.id).await?;
    tx.commit().await?;
    Ok(Json(RecoveryCodes { codes }).into_response())
}

// For a user who lost their authenticator and their recovery codes. Not for
// the admin's own account: they turn it off in their settings, with a code.
/// Turn off two-factor authentication for a user.
///
/// For a user who lost their authenticator app and their recovery codes; they can set it up again. Not
/// for your own account.
#[utoipa::path(
    delete,
    path = "/users/{id}/totp",
    tag = "Users",
    params(("id" = Uuid, Path, description = "The user id")),
    responses(
        (status = 204, description = "Off"),
        (status = 400, description = "Your own account", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
    ),
    security(("session" = []), ("api_key" = [])),
)]
async fn admin_reset_totp(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    require_admin(&user)?;
    let User { id: own, .. } = user.user;
    if id == own {
        return Err(ApiError::bad_request(
            "Turn off your own two-factor authentication in your settings",
        ));
    }
    if !totp::remove(&state, id).await? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "Not found"));
    }
    tracing::info!("admin {own} turned off two-factor authentication for user {id}");
    Ok(StatusCode::NO_CONTENT.into_response())
}
