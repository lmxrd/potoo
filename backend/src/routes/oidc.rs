//! Single sign-on: the login and the provider's callback, and the admin
//! settings of the provider. The protocol is in `oidc.rs`; who may sign in
//! is decided here: a user linked to the provider account, else the user
//! with the provider's verified email (who is linked from then on, unless
//! they are linked to another account at the provider already), else, when
//! the settings say so, a new regular user without a password. Nobody signs
//! in this way before the first admin exists.

use crate::auth::create_session;
use crate::error::{ErrorBody, HttpError};
use crate::extract::ApiJson;
use crate::oidc::{CALLBACK_PATH, Failure, Identity, LOGIN_COOKIE, LOGIN_TTL, Settings};
use crate::routes::auth::{AuthUser, require_admin, secure, validate_email};
use crate::routes::totp::{Step, challenge_cookie, next_step};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::{CookieJar, SignedCookieJar};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

/// The login and the callback, nested under /auth.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(callback))
}

/// The provider settings, nested under /admin.
pub fn admin_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_settings, update_settings))
}

/// The login cookie's name and path. Over https it is a `__Host-` cookie,
/// which a sibling subdomain cannot set: one that could would plant its own
/// login and sign the browser in as someone else. That prefix needs the
/// path `/`; over plain http the cookie goes to the callback only.
fn login_cookie_name(secure: bool) -> (&'static str, &'static str) {
    if secure {
        ("__Host-potoo_oidc", "/")
    } else {
        (LOGIN_COOKIE, CALLBACK_PATH)
    }
}

/// The login cookie. Lax, not Strict: the callback is a navigation that
/// comes from the provider's site.
fn login_cookie(value: String, secure: bool) -> Cookie<'static> {
    let (name, path) = login_cookie_name(secure);
    Cookie::build((name, value))
        .path(path)
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(time::Duration::seconds(LOGIN_TTL.as_secs() as i64))
        .build()
}

/// Back to the login page, which says what went wrong.
fn failed(failure: Failure) -> Redirect {
    Redirect::to(&format!("/login?error={}", failure.code()))
}

#[utoipa::path(
    get,
    path = "/oidc/login",
    tag = "Auth",
    operation_id = "oidcLogin",
    summary = "Sign in with single sign-on",
    description = "Redirects the browser to the provider, or to `/login?error=unavailable` when single sign-on is off or the provider cannot be reached (it is not asked again for 30 seconds), or to `/login?error=misconfigured` when the callback URL is not on the scheme (http or https) Potoo is reached over. Sets a short-lived signed cookie that holds the login, so that only this browser can finish it.",
    responses(
        (status = 303, description = "To the provider, or back to the login page"),
    ),
)]
async fn login(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let secure = secure(&headers);
    let redirect_url = state.oidc.callback_url(&headers, state.trust_proxy);
    // The login cookie is named for this request's scheme and the callback
    // reads the one of its own: a callback on the other scheme could never
    // finish the login. Accepting the plain cookie over https would let a
    // sibling subdomain plant one, so this is refused before the provider,
    // with a reason that points at the setup.
    if redirect_url.starts_with("https://") != secure {
        tracing::warn!(
            "single sign-on: Potoo is reached over {} but its callback URL is {redirect_url}; \
             check OIDC_REDIRECT_URL, TRUST_PROXY and the proxy's X-Forwarded-Proto",
            if secure { "https" } else { "http" }
        );
        return failed(Failure::Misconfigured).into_response();
    }
    match state.oidc.start(redirect_url).await {
        Ok((url, login)) => (
            SignedCookieJar::new(state.oidc.key().clone()).add(login_cookie(login, secure)),
            Redirect::to(&url),
        )
            .into_response(),
        Err(failure) => failed(failure).into_response(),
    }
}

/// What the provider sends back.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CallbackParams {
    /// The authorization code.
    code: Option<String>,
    /// The state of the login.
    state: Option<String>,
    /// Set when the provider refused, e.g. `access_denied`.
    error: Option<String>,
}

#[utoipa::path(
    get,
    path = "/oidc/callback",
    tag = "Auth",
    operation_id = "oidcCallback",
    summary = "The provider's callback",
    description = "Where the provider sends the browser back. Signs the user in (the session cookie is set) and redirects to `/`; or, for two-factor authentication first, sets the short-lived challenge cookie and redirects to `/login?step=totp` (a code) or `/login?step=totp-setup` (setting the app up, with `TOTP_REQUIRED`); or redirects to `/login?error=<code>`: `unavailable` (also before the first admin is set up), `state`, `provider`, `token`, `email`, `unknown_user` or `linked` (the user with that email is linked to another account at the provider).",
    params(CallbackParams),
    responses(
        (status = 303, description = "Signed in, to `/`; or back to the login page"),
        (status = 500, description = "The database failed", body = ErrorBody),
    ),
)]
async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Response {
    // The login cookie, if this server signed it. Any cookie of that name
    // is used up whatever happens next, errors included, and one this
    // server did not sign (from before a restart, say) too.
    let secure = secure(&headers);
    let (name, _) = login_cookie_name(secure);
    let login = SignedCookieJar::from_headers(&headers, state.oidc.key().clone())
        .get(name)
        .map(|c| c.value().to_string());
    let logins = if jar.get(name).is_some() {
        // With the attributes it was set with: a browser ignores a
        // `__Host-` cookie without Secure, the removal included.
        let mut removal = login_cookie(String::new(), secure);
        removal.make_removal();
        CookieJar::new().add(removal)
    } else {
        CookieJar::new()
    };
    match sign_in(&state, jar, &headers, params, login).await {
        Ok(Ok(SignedIn::Session(jar))) => (logins, jar, Redirect::to("/")).into_response(),
        Ok(Ok(SignedIn::Step(challenge, step))) => {
            (logins, challenge, Redirect::to(step.page())).into_response()
        }
        Ok(Err(failure)) => (logins, failed(failure)).into_response(),
        Err(error) => (logins, error).into_response(),
    }
}

/// The callback's work: the session cookie of the user signed in, or why
/// nobody is.
async fn sign_in(
    state: &AppState,
    jar: CookieJar,
    headers: &HeaderMap,
    params: CallbackParams,
    login: Option<String>,
) -> Result<Result<SignedIn, Failure>, HttpError> {
    let Some(login_state) = params.state else {
        return Ok(Err(Failure::State));
    };
    if params.error.is_some() {
        return Ok(Err(Failure::Provider));
    }
    let identity = match state
        .oidc
        .finish(&login_state, login.as_deref(), params.code)
        .await
    {
        Ok(identity) => identity,
        Err(failure) => return Ok(Err(failure)),
    };

    let mut tx = state.db.writer().begin().await?;
    let user_id = match user_for(&mut tx, &identity, state.oidc.settings().auto_create).await? {
        Ok(id) => id,
        Err(failure) => return Ok(Err(failure)),
    };
    // Two-factor authentication is asked for after single sign-on too: the
    // login page takes the step, with the challenge cookie.
    if let Some(step) = next_step(&mut *tx, state, user_id).await? {
        tx.commit().await?;
        return Ok(Ok(SignedIn::Step(
            challenge_cookie(state, user_id, step, secure(headers)),
            step,
        )));
    }
    let jar = create_session(&mut *tx, jar, user_id, secure(headers)).await?;
    tx.commit().await?;
    Ok(Ok(SignedIn::Session(jar)))
}

/// What a callback that proved who the user is leads to.
enum SignedIn {
    Session(CookieJar),
    /// Two-factor authentication first, on the login page.
    Step(SignedCookieJar, Step),
}

/// The user a provider account signs in as, linking or creating them.
async fn user_for(
    tx: &mut sqlx::SqliteConnection,
    identity: &Identity,
    auto_create: bool,
) -> sqlx::Result<Result<i64, Failure>> {
    // Nobody before the first admin: a user made now would take /setup
    // away and leave the instance without one.
    let set_up: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
        .fetch_one(&mut *tx)
        .await?;
    if !set_up {
        return Ok(Err(Failure::Unavailable));
    }
    let linked: Option<i64> = sqlx::query_scalar(
        "SELECT user_id FROM user_identities WHERE issuer = ?1 AND subject = ?2",
    )
    .bind(&identity.issuer)
    .bind(&identity.subject)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(id) = linked {
        return Ok(Ok(id));
    }

    // Not linked yet: only a verified email may find (or make) the user.
    let Some(email) = identity
        .verified_email
        .as_deref()
        .and_then(|email| validate_email(email).ok())
    else {
        return Ok(Err(Failure::Email));
    };
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?1")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
    let id = match existing {
        // Linked to another account at this provider: a second account
        // with the same email (a recycled address, say) is not the same
        // person, so it does not get in.
        Some(id) if linked_to(tx, id, &identity.issuer).await? => {
            tracing::warn!(
                "single sign-on: user {id} is linked to another account at the provider"
            );
            return Ok(Err(Failure::Linked));
        }
        Some(id) => id,
        None if auto_create => {
            sqlx::query("INSERT INTO users (email, password_hash, is_admin) VALUES (?1, NULL, 0)")
                .bind(&email)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid()
        }
        None => return Ok(Err(Failure::UnknownUser)),
    };
    sqlx::query("INSERT INTO user_identities (issuer, subject, user_id) VALUES (?1, ?2, ?3)")
        .bind(&identity.issuer)
        .bind(&identity.subject)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    Ok(Ok(id))
}

/// Whether a user is linked to an account at this provider.
async fn linked_to(tx: &mut sqlx::SqliteConnection, user: i64, issuer: &str) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM user_identities WHERE user_id = ?1 AND issuer = ?2)",
    )
    .bind(user)
    .bind(issuer)
    .fetch_one(&mut *tx)
    .await
}

/// The single sign-on settings in effect. The client secret is never
/// sent back.
#[derive(Serialize, ToSchema)]
pub struct OidcSettings {
    /// Whether single sign-on is on: an issuer and a client id are set.
    pub enabled: bool,
    /// The provider's issuer URL, e.g. `https://id.example.com/realms/main`.
    pub issuer: String,
    pub client_id: String,
    /// Whether a client secret is set.
    pub client_secret_set: bool,
    /// Space separated; empty is `openid email`.
    pub scopes: String,
    /// What the login button says; empty is "Single sign-on".
    pub label: String,
    /// The callback URL, when one is set; empty is the one the request
    /// came to.
    pub redirect_url: String,
    /// Whether a verified email that matches no user creates a regular one.
    pub auto_create: bool,
    /// The callback URL to register with the provider.
    pub callback_url: String,
    /// The fields set by an environment variable (`issuer` by `OIDC_ISSUER`,
    /// and so on), which cannot be changed here.
    pub locked: Vec<String>,
}

/// The settings to change; a field that is left out stays as it is.
#[derive(Deserialize, ToSchema)]
pub struct UpdateOidcSettings {
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    /// An empty string removes the secret. A new issuer removes it too,
    /// unless one is sent with it: the secret is for one provider only.
    pub client_secret: Option<String>,
    pub scopes: Option<String>,
    pub label: Option<String>,
    pub redirect_url: Option<String>,
    pub auto_create: Option<bool>,
}

fn view(state: &AppState, headers: &HeaderMap) -> OidcSettings {
    let settings = state.oidc.settings();
    OidcSettings {
        enabled: settings.enabled(),
        client_secret_set: !settings.client_secret.is_empty(),
        callback_url: state.oidc.callback_url(headers, state.trust_proxy),
        locked: state
            .oidc
            .env()
            .locked()
            .into_iter()
            .map(String::from)
            .collect(),
        issuer: settings.issuer,
        client_id: settings.client_id,
        scopes: settings.scopes,
        label: settings.label,
        redirect_url: settings.redirect_url,
        auto_create: settings.auto_create,
    }
}

#[utoipa::path(
    get,
    path = "/oidc",
    tag = "Admin",
    operation_id = "getOidcSettings",
    summary = "The single sign-on settings",
    responses(
        (status = 200, description = "The settings in effect", body = OidcSettings),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
    ),
    security(("session" = []), ("api_key" = [])),
)]
async fn get_settings(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
) -> Result<Json<OidcSettings>, HttpError> {
    require_admin(&user)?;
    Ok(Json(view(&state, &headers)))
}

#[utoipa::path(
    put,
    path = "/oidc",
    tag = "Admin",
    operation_id = "updateOidcSettings",
    summary = "Change the single sign-on settings",
    description = "Takes effect at once. With an issuer and a client id set, the provider must answer its discovery document, or nothing changes (400). A field set by an environment variable cannot be changed (400). A new issuer removes the client secret unless one is sent with it, and is refused (400) while the secret comes from `OIDC_CLIENT_SECRET` and the issuer does not.",
    request_body(content = UpdateOidcSettings, content_type = "application/json"),
    responses(
        (status = 200, description = "The settings now in effect", body = OidcSettings),
        (status = 400, description = "The request is invalid, or the provider cannot be used", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Admin only", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody),
    ),
    security(("session" = []), ("api_key" = [])),
)]
async fn update_settings(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    ApiJson(body): ApiJson<UpdateOidcSettings>,
) -> Result<Json<OidcSettings>, HttpError> {
    require_admin(&user)?;
    // A task of its own, which runs to the end even if this request is
    // dropped half way (the client went away): the row is never stored
    // without being put in effect.
    tokio::spawn(save(state.clone(), body))
        .await
        .map_err(|e| HttpError::internal(format!("saving failed: {e}")))??;
    Ok(Json(view(&state, &headers)))
}

/// Store new settings and put them in effect, one save at a time.
async fn save(state: AppState, body: UpdateOidcSettings) -> Result<(), HttpError> {
    let _saving = state.oidc.lock_saves().await;
    let env = state.oidc.env();
    let locked = env.locked();
    let mut stored = crate::oidc::stored(&state.db).await?;
    let issuer_before = env.apply(&stored).issuer;
    let new_secret = body.client_secret.is_some();
    let fields = [
        ("issuer", body.issuer, &mut stored.issuer),
        ("client_id", body.client_id, &mut stored.client_id),
        (
            "client_secret",
            body.client_secret,
            &mut stored.client_secret,
        ),
        ("scopes", body.scopes, &mut stored.scopes),
        ("label", body.label, &mut stored.label),
        ("redirect_url", body.redirect_url, &mut stored.redirect_url),
    ];
    for (field, value, stored) in fields {
        let Some(value) = value else { continue };
        if locked.contains(&field) {
            return Err(locked_field(field));
        }
        *stored = value.trim().to_string();
    }
    if let Some(auto_create) = body.auto_create {
        if locked.contains(&"auto_create") {
            return Err(locked_field("auto_create"));
        }
        stored.auto_create = auto_create;
    }

    // The secret goes to the provider's token endpoint: a new issuer, which
    // names its own, must not receive the old provider's secret.
    if env.apply(&stored).issuer != issuer_before {
        if env.client_secret.is_some() {
            return Err(HttpError::bad_request(
                "The client secret is set by OIDC_CLIENT_SECRET: set the issuer with OIDC_ISSUER too",
            ));
        }
        if !new_secret {
            stored.client_secret.clear();
        }
    }

    // The provider is asked before anything is stored, and no connection
    // is held while it answers.
    let settings: Settings = env.apply(&stored);
    state
        .oidc
        .check(&settings)
        .await
        .map_err(HttpError::bad_request)?;
    sqlx::query(
        "UPDATE oidc_settings SET issuer = ?1, client_id = ?2, client_secret = ?3, scopes = ?4,
           label = ?5, redirect_url = ?6, auto_create = ?7
         WHERE id = 1",
    )
    .bind(&stored.issuer)
    .bind(&stored.client_id)
    .bind(&stored.client_secret)
    .bind(&stored.scopes)
    .bind(&stored.label)
    .bind(&stored.redirect_url)
    .bind(stored.auto_create)
    .execute(state.db.writer())
    .await?;
    state.oidc.replace(settings);
    Ok(())
}

fn locked_field(field: &str) -> HttpError {
    HttpError::bad_request(format!(
        "{field} is set by the OIDC_{} environment variable",
        field.to_uppercase()
    ))
}
