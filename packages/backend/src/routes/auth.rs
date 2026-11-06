use crate::auth::{
    DATE_FORMATS, KeyUse, Scope, User, hash_password, iso, key_user, verify_password,
};
use crate::error::{ErrorBody, HttpError};
use crate::extract::ApiJson;
use crate::routes::totp::{Step, challenge_cookie, next_step};
use crate::session::{self, AuthSession};
use crate::state::AppState;
use crate::visitor::client_ip;
use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum_login::AuthnBackend as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteExecutor;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};
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

/// How a request is signed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// The session cookie: everything the user may do.
    Session,
    /// An API key (`Authorization: Bearer`), capped by its scope.
    Key(Scope),
}

/// The signed-in user, as a request extractor: a valid session cookie, or
/// an API key, or a 401. A read key is refused anything but GET and HEAD
/// here (403); admin endpoints check the rest in `require_admin`. The
/// account's own settings (email, password, two-factor authentication, API
/// keys) take a `SessionUser` instead: no key reaches them.
pub struct AuthUser(pub User, pub Access);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer(&parts.headers) {
            let (user, key) = key_user(state.db.reader(), token)
                .await
                .ok_or_else(HttpError::unauthorized)?;
            if key.scope == Scope::Read && !parts.method.is_safe() {
                return Err(HttpError::new(
                    StatusCode::FORBIDDEN,
                    "This API key can only read",
                ));
            }
            note_key_use(state, &key).await;
            return Ok(AuthUser(user, Access::Key(key.scope)));
        }
        let user = auth_session(parts, state)
            .await?
            .user
            .ok_or_else(HttpError::unauthorized)?;
        Ok(AuthUser(user, Access::Session))
    }
}

/// The request's session (axum-login loads the signed-in user, if any).
async fn auth_session(
    parts: &mut axum::http::request::Parts,
    state: &AppState,
) -> Result<AuthSession, HttpError> {
    AuthSession::from_request_parts(parts, state)
        .await
        .map_err(|(_, e)| HttpError::internal(format!("no session layer: {e}")))
}

/// The signed-in user of a session cookie only: an API key is refused
/// (403), so a leaked key cannot change the account or make more keys.
pub struct SessionUser(pub User);

impl FromRequestParts<AppState> for SessionUser {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await? {
            AuthUser(user, Access::Session) => Ok(SessionUser(user)),
            AuthUser(_, Access::Key(_)) => Err(HttpError::new(
                StatusCode::FORBIDDEN,
                "Not with an API key: sign in",
            )),
        }
    }
}

/// The token of an `Authorization: Bearer` header.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("authorization")?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

/// Records when a key was used, at most once a minute, so that a busy
/// script does not take the one writer for every request. A failure is
/// only logged: the request goes on.
async fn note_key_use(state: &AppState, key: &KeyUse) {
    let now = SystemTime::now();
    let stale = iso(now - Duration::from_secs(60));
    if key
        .last_used_at
        .as_deref()
        .is_some_and(|last| last > stale.as_str())
    {
        return;
    }
    if let Err(e) = sqlx::query("UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2")
        .bind(iso(now))
        .bind(key.id)
        .execute(state.db.writer())
        .await
    {
        tracing::warn!("cannot record the use of API key {}: {e}", key.id);
    }
}

/// Admin only: an admin's session, or an admin's admin-scoped key.
pub(crate) fn require_admin(user: &AuthUser) -> Result<(), HttpError> {
    match user {
        AuthUser(u, Access::Session | Access::Key(Scope::Admin)) if u.is_admin => Ok(()),
        _ => Err(HttpError::forbidden()),
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

/// The cookie is secure behind an https proxy that says so.
pub(crate) fn secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

pub(crate) fn validate_email(raw: &str) -> Result<String, HttpError> {
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
    /// Single sign-on, when it is on: null when it is off. Always present.
    #[schema(required = true)]
    pub oidc: Option<OidcLogin>,
    /// False when single sign-on is the only way in (`OIDC_ONLY`): the
    /// login page offers no password, and passwords cannot be set.
    pub password_login: bool,
    /// Whether every user must have two-factor authentication on
    /// (`TOTP_REQUIRED`): it cannot be turned off.
    pub totp_required: bool,
}

/// How the login page offers single sign-on.
#[derive(Serialize, ToSchema)]
pub struct OidcLogin {
    /// What the button says, e.g. "Single sign-on".
    pub label: String,
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
    /// Required when the user has a password; left out to set a first one.
    #[serde(default)]
    pub current_password: Option<String>,
    /// 10 to 200 characters.
    pub new_password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateUser {
    pub email: String,
    /// 10 to 200 characters. May be left out while single sign-on is on:
    /// the user then signs in through the provider.
    #[serde(default)]
    pub password: Option<String>,
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
    pub has_password: bool,
    /// Whether the user is linked to a single sign-on account.
    pub single_sign_on: bool,
    /// Whether the user has two-factor authentication on.
    pub totp_enabled: bool,
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
    auth: AuthSession,
) -> Result<Json<Session>, HttpError> {
    let needs_setup = !has_users(&state).await?;
    let user = if needs_setup { None } else { auth.user };
    let settings = state.oidc.settings();
    let oidc = settings.enabled().then(|| OidcLogin {
        label: settings.label().to_string(),
    });
    Ok(Json(Session {
        needs_setup,
        user,
        oidc,
        password_login: state.oidc.passwords_allowed(),
        totp_required: state.totp_required,
    }))
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
    mut auth: AuthSession,
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
        return Err(HttpError::new(StatusCode::CONFLICT, "Already set up"));
    }
    let user = auth
        .backend
        .get_user(&inserted.last_insert_rowid())
        .await?
        .ok_or_else(|| HttpError::internal("the new admin is gone"))?;
    session::sign_in(&mut auth, &user).await?;
    Ok(StatusCode::CREATED.into_response())
}

#[utoipa::path(
    post,
    path = "/login",
    tag = "Auth",
    operation_id = "login",
    summary = "Sign in",
    description = "Sets the session cookie. For a user with two-factor authentication on it answers `totp_required` instead, with a cookie good for five minutes: `/login/totp` with a code finishes the sign-in. With `TOTP_REQUIRED`, a user without it gets `totp_setup_required`: `/login/totp/setup` and `/login/totp/enable` set the app up and finish the sign-in. After five failed attempts for one IP and email within 15 minutes the pair is locked out (429, with a Retry-After header).",
    request_body(content = Credentials, content_type = "application/json"),
    responses(
        (status = 200, description = "The password is right; two-factor authentication is next", body = LoginStep),
        (status = 204, description = "Signed in"),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Password sign-in is turned off (`OIDC_ONLY`)", body = ErrorBody),
        (status = 429, description = "Too many attempts, try again later", body = ErrorBody),
    ),
)]
async fn login(
    State(state): State<AppState>,
    mut auth: AuthSession,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<ApiJson<Credentials>, HttpError>,
) -> Result<Response, HttpError> {
    if !state.oidc.passwords_allowed() {
        return Err(HttpError::passwords_off());
    }
    // A malformed body fails the same way as a wrong password.
    let Ok(ApiJson(creds)) = body else {
        return Err(HttpError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid email or password",
        ));
    };
    let email = validate_email(&creds.email)
        .map_err(|_| HttpError::new(StatusCode::UNAUTHORIZED, "Invalid email or password"))?;

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

    // The same work and the same answer whether or not the user exists,
    // or has a password (see `session::Backend::authenticate`).
    let user = auth
        .authenticate(session::Credentials {
            email,
            password: creds.password,
        })
        .await
        .map_err(anyhow::Error::new)?;
    let Some(user) = user else {
        state.throttle.record_failure(&key);
        return Err(HttpError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid email or password",
        ));
    };
    state.throttle.clear(&key);
    if let Some(step) = next_step(state.db.reader(), &state, user.id).await? {
        return Ok((
            challenge_cookie(&state, user.id, step, secure(&headers)),
            Json(LoginStep {
                totp_required: step == Step::Code,
                totp_setup_required: step == Step::Enroll,
            }),
        )
            .into_response());
    }
    session::sign_in(&mut auth, &user).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The password was right, and two-factor authentication is the next step.
/// Exactly one of the two is true.
#[derive(Serialize, ToSchema)]
pub struct LoginStep {
    /// A code from the authenticator app is next (`/login/totp`).
    pub totp_required: bool,
    /// Setting the app up is next (`TOTP_REQUIRED`): `/login/totp/setup`,
    /// then `/login/totp/enable`.
    pub totp_setup_required: bool,
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
async fn logout(mut auth: AuthSession) -> Result<Response, HttpError> {
    auth.logout().await.map_err(anyhow::Error::new)?;
    Ok(StatusCode::NO_CONTENT.into_response())
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
    user: SessionUser,
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
        return Err(HttpError::new(StatusCode::CONFLICT, "Email already in use"));
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
    user: SessionUser,
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

// Change the signed-in user's own password, or set a first one.
#[utoipa::path(
    post,
    path = "/password",
    tag = "Account",
    operation_id = "changePassword",
    summary = "Change your own password",
    description = "The current password is required, except for a user who has none (single sign-on): they set a first one. The user's other sessions are signed out. Refused while password sign-in is turned off (`OIDC_ONLY`).",
    request_body(content = ChangePassword, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Password sign-in is turned off (`OIDC_ONLY`)", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = [])),
)]
async fn change_password(
    State(state): State<AppState>,
    user: SessionUser,
    mut auth: AuthSession,
    ApiJson(body): ApiJson<ChangePassword>,
) -> Result<Response, HttpError> {
    if !state.oidc.passwords_allowed() {
        return Err(HttpError::passwords_off());
    }
    let invalid = || HttpError::new(StatusCode::UNAUTHORIZED, "Invalid password");
    let current = body.current_password.unwrap_or_default();
    if current.len() > 200 {
        return Err(HttpError::bad_request("Invalid password"));
    }
    let new_password = validate_password(&body.new_password)?;
    // The hashing below takes a while: no connection is held meanwhile.
    let stored: Option<Option<String>> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?1")
            .bind(user.0.id)
            .fetch_optional(state.db.reader())
            .await?;
    let Some(stored) = stored else {
        return Err(invalid());
    };
    if let Some(stored) = &stored
        && !verify_password(&current, stored)
    {
        return Err(invalid());
    }
    // Only if the password is still the one checked (or still none): a
    // change in between fails like a wrong password.
    let updated =
        sqlx::query("UPDATE users SET password_hash = ?1 WHERE id = ?2 AND password_hash IS ?3")
            .bind(hash_password(&new_password))
            .bind(user.0.id)
            .bind(stored)
            .execute(state.db.writer())
            .await?
            .rows_affected();
    if updated == 0 {
        return Err(invalid());
    }
    // The user's other sessions are signed out; this one stays.
    session::refresh(&mut auth, user.0.id).await?;
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
    security(("session" = []), ("api_key" = [])),
)]
async fn list_users(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<UserRow>>, HttpError> {
    require_admin(&user)?;
    let rows: Vec<UserRow> = sqlx::query_as(
        "SELECT id, email, is_admin, created_at, password_hash IS NOT NULL AS has_password,
                    EXISTS (SELECT 1 FROM user_identities i WHERE i.user_id = users.id)
                      AS single_sign_on,
                    totp_secret IS NOT NULL AS totp_enabled
             FROM users ORDER BY id",
    )
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
    description = "The password may be left out while single sign-on is on. With `OIDC_ONLY` no password is taken: the user signs in through the provider.",
    request_body(content = CreateUser, content_type = "application/json"),
    responses(
        (status = 201, description = "Created"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 409, description = "Conflict", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = []), ("api_key" = [])),
)]
async fn create_user(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(body): ApiJson<CreateUser>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    let email = validate_email(&body.email)?;
    // Without a password the user can only sign in through the provider.
    let password = body.password.as_deref().filter(|p| !p.is_empty());
    if password.is_some() && !state.oidc.passwords_allowed() {
        return Err(HttpError::passwords_off());
    }
    let password_hash = match password {
        Some(password) => Some(hash_password(&validate_password(password)?)),
        None if !state.oidc.passwords_allowed() => None,
        None if state.oidc.settings().enabled() => None,
        None => {
            return Err(HttpError::bad_request(
                "Expected a password of 10 to 200 characters",
            ));
        }
    };
    let mut tx = state.db.writer().begin().await?;
    let taken: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE email = ?1")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
    if taken.is_some() {
        return Err(HttpError::new(StatusCode::CONFLICT, "Email already in use"));
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
    security(("session" = []), ("api_key" = [])),
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
        return Err(HttpError::new(StatusCode::NOT_FOUND, "Not found"));
    };
    if target == 1 && !body.is_admin && admin_count(&mut *tx).await? == 1 {
        return Err(HttpError::new(
            StatusCode::CONFLICT,
            "Cannot demote the last admin",
        ));
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
    security(("session" = []), ("api_key" = [])),
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
        return Err(HttpError::new(
            StatusCode::CONFLICT,
            "Cannot delete the last admin",
        ));
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
    description = "Refused while password sign-in is turned off (`OIDC_ONLY`).",
    params(("id" = i64, Path, description = "The user id")),
    request_body(content = ResetPassword, content_type = "application/json"),
    responses(
        (status = 204, description = "Changed"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only, or password sign-in is turned off (`OIDC_ONLY`)", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = []), ("api_key" = [])),
)]
async fn reset_password(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    ApiJson(body): ApiJson<ResetPassword>,
) -> Result<Response, HttpError> {
    require_admin(&user)?;
    if !state.oidc.passwords_allowed() {
        return Err(HttpError::passwords_off());
    }
    let password = validate_password(&body.new_password)?;
    let updated = sqlx::query("UPDATE users SET password_hash = ?1 WHERE id = ?2")
        .bind(hash_password(&password))
        .bind(id)
        .execute(state.db.writer())
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(HttpError::new(StatusCode::NOT_FOUND, "Not found"));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
