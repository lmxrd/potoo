use crate::error::ApiError;
use crate::sessions::AuthSession;
use crate::state::AppState;
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, generate_salt};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteExecutor;
use utoipa::ToSchema;
use uuid::Uuid;

/// The hex-encoded SHA-256 of a string: how API key tokens, recovery codes
/// and session ids are stored (never the secret itself).
pub fn sha256_hex(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

/// The date formats a user can pick from, for displaying dates.
pub const DATE_FORMATS: [&str; 4] = ["YYYY-MM-DD", "DD/MM/YYYY", "MM/DD/YYYY", "DD.MM.YYYY"];

/// The signed-in user, as the session endpoint returns them.
#[derive(Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub is_admin: bool,
    /// One of the date formats of the settings, e.g. `YYYY-MM-DD`.
    pub date_format: String,
    /// Whether times are shown in 24-hour (`14:30`) or 12-hour (`2:30 PM`) format.
    pub use_24: bool,
    /// A JSON object as text: `{"base":"light"|"dark","accent":"#rrggbb"}`.
    pub theme: String,
    /// False for a user who signs in through single sign-on only: they can
    /// set a password without giving a current one.
    pub has_password: bool,
    /// Whether a code from an authenticator app is asked for at sign-in.
    pub totp_enabled: bool,
}

// Not derived: the auth hash stays out of logs.
impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// Hash a password with argon2id; the PHC string is what gets stored.
pub fn hash_password(password: &str) -> String {
    Argon2::default()
        .hash_password_with_salt(password.as_bytes(), &generate_salt())
        .expect("password hashing cannot fail")
        .to_string()
}

/// A hash of nothing anyone knows, checked when there is no real one (an
/// unknown email, or an account without a password), so that answer takes as
/// long as a wrong password.
static DUMMY_HASH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| hash_password(&crate::random::hex::<16>()));

/// Make the dummy hash now (at start), so the first login it serves takes no
/// longer than the rest.
pub fn warm_up() {
    std::sync::LazyLock::force(&DUMMY_HASH);
}

/// Verify a password against a user's stored hash. No hash never matches,
/// after the same work as a wrong password.
pub fn verify_user_password(password: &str, phc: Option<&str>) -> bool {
    match phc {
        Some(phc) => verify_password(password, phc),
        None => {
            verify_password(password, &DUMMY_HASH);
            false
        }
    }
}

/// Verify a password against a stored PHC hash.
pub fn verify_password(password: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .ok()
        .and_then(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .ok()
        })
        .is_some()
}

/// What an API key may do. Never more than its user may: an admin key of a
/// user who is no longer an admin reaches no admin endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Reads only (GET).
    Read,
    /// Reads and changes, but no admin endpoints.
    Write,
    /// Everything the user may do, admin endpoints included.
    Admin,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Write => "write",
            Scope::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Scope> {
        match value {
            "read" => Some(Scope::Read),
            "write" => Some(Scope::Write),
            "admin" => Some(Scope::Admin),
            _ => None,
        }
    }
}

/// The start of every API key, so that one is recognised in a leak.
pub const API_KEY_PREFIX: &str = "potoo_";

/// An API key in use: its id, scope and when it was last used.
pub struct KeyUse {
    pub id: Uuid,
    pub scope: Scope,
    pub last_used_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct KeyRow {
    #[sqlx(flatten)]
    user: User,
    key_id: Uuid,
    scope: String,
    last_used_at: Option<String>,
}

/// The user of a valid, unexpired API key, if any.
pub async fn key_user<'e>(db: impl SqliteExecutor<'e>, token: &str) -> Option<(User, KeyUse)> {
    if !token.starts_with(API_KEY_PREFIX) || token.len() > 200 {
        return None;
    }
    let row = sqlx::query_as::<_, KeyRow>(
        "SELECT u.id, u.email, u.is_admin, u.date_format, u.use_24, u.theme,
                u.password_hash IS NOT NULL AS has_password,
                u.totp_secret IS NOT NULL AS totp_enabled,
                k.id AS key_id, k.scope, k.last_used_at
         FROM api_keys k JOIN users u ON u.id = k.user_id
         WHERE k.token_hash = ?1 AND (k.expires_at IS NULL OR k.expires_at > ?2)",
    )
    .bind(sha256_hex(token))
    .bind(crate::time::now_iso())
    .fetch_optional(db)
    .await
    .ok()
    .flatten()?;
    let scope = Scope::parse(&row.scope)?;
    Some((
        row.user,
        KeyUse {
            id: row.key_id,
            scope,
            last_used_at: row.last_used_at,
        },
    ))
}

/// The signed-in user, as a request extractor: a valid session cookie, or
/// else an API key (`Authorization: Bearer`), or a 401. A read key is
/// refused anything but GET and HEAD (403); admin endpoints check the rest
/// in [`require_admin`]. The account's own settings (email, password,
/// two-factor authentication, API keys) take a [`SessionOnlyUser`] instead: no
/// key reaches them.
pub struct CurrentUser {
    pub user: User,
    /// The API key the request was made with, if not a session.
    pub api_key: Option<KeyUse>,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer(&parts.headers) {
            let (user, key) = key_user(state.db.reader(), token)
                .await
                .ok_or_else(ApiError::unauthorized)?;
            if key.scope == Scope::Read && !parts.method.is_safe() {
                return Err(ApiError::forbidden("This API key can only read"));
            }
            note_key_use(state, &key).await;
            return Ok(CurrentUser {
                user,
                api_key: Some(key),
            });
        }
        let session = AuthSession::from_request_parts(parts, state)
            .await
            .map_err(|(_, e)| ApiError::internal(format!("no session layer: {e}")))?;
        let user = session.user.ok_or_else(ApiError::unauthorized)?.user;
        Ok(CurrentUser {
            user,
            api_key: None,
        })
    }
}

/// The signed-in user of a session cookie only: an API key is refused
/// (403), so a leaked key cannot change the account or make more keys.
pub struct SessionOnlyUser(pub User);

impl FromRequestParts<AppState> for SessionOnlyUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match CurrentUser::from_request_parts(parts, state).await? {
            CurrentUser {
                user,
                api_key: None,
            } => Ok(SessionOnlyUser(user)),
            CurrentUser {
                api_key: Some(_), ..
            } => Err(ApiError::forbidden("Not with an API key: sign in")),
        }
    }
}

/// Admin only: an admin's session, or an admin's admin-scoped key.
pub fn require_admin(current: &CurrentUser) -> Result<(), ApiError> {
    let scope_allows = current
        .api_key
        .as_ref()
        .is_none_or(|key| key.scope == Scope::Admin);
    if current.user.is_admin && scope_allows {
        Ok(())
    } else {
        Err(ApiError::forbidden("Admin only"))
    }
}

/// The token of an `Authorization: Bearer` header.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

/// Records when a key was used, at most once a minute, so that a busy
/// script does not take the one writer for every request. A failure is
/// only logged: the request goes on.
async fn note_key_use(state: &AppState, key: &KeyUse) {
    let now = crate::time::now_iso();
    let stale = crate::time::seconds_from_now_iso(-60);
    if key
        .last_used_at
        .as_deref()
        .is_some_and(|last| last > stale.as_str())
    {
        return;
    }
    if let Err(e) = sqlx::query("UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(key.id)
        .execute(state.db.writer())
        .await
    {
        tracing::warn!("cannot record the use of API key {}: {e}", key.id);
    }
}
