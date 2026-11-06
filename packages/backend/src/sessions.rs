//! Sessions: tower-sessions keeps them, axum-login signs users in and out.
//!
//! The store is our own: the community's sqlx store needs sqlx 0.8 (we use
//! 0.9, and two bundled SQLites cannot be linked into one binary). It keeps
//! each session's record in the `sessions` table, keyed by the SHA-256 of
//! the session id, so a copy of the database (a backup, say) holds no
//! session anyone could use. Records are written only when they change
//! (sign-in, sign-out), not on every request.
//!
//! axum-login loads the signed-in user on every request ([`Backend`]), so a
//! deleted user is signed out everywhere at once. Each session also holds a
//! hash of the user's credentials (`session_auth_hash`): changing the
//! password or the two-factor secret signs the user's other sessions out
//! (the one that made the change signs in again: [`refresh`]).

use crate::auth::User;
use crate::db::Db;
use crate::error::ApiError;
use crate::state::AppState;
use async_trait::async_trait;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use axum_login::AuthnBackend as _;
use sha2::{Digest, Sha256};
use std::fmt;
use time::OffsetDateTime;
use tower_sessions::cookie::SameSite;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, ExpiredDeletion, SessionStore};
use tower_sessions::{Expiry, SessionManagerLayer};
use uuid::Uuid;

/// How long a sign-in lasts, however active the session is.
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Where a session keeps when it ends (Unix seconds), so signing in again
/// after a password change keeps it: tower-sessions does not remember a
/// session's expiry between requests.
const EXPIRES_KEY: &str = "potoo.expires";

/// The session cookie's name over plain http (development); see [`cookie_name`].
pub const SESSION_COOKIE: &str = "potoo_session";
/// Its name over https: browsers accept a `__Host-` cookie only with
/// `Secure`, `Path=/` and no `Domain`, so a sibling subdomain can neither
/// set nor read one (login CSRF, cookie tossing).
pub const SECURE_SESSION_COOKIE: &str = "__Host-potoo_session";

/// The session cookie's name: prefixed when the site is https (`PUBLIC_URL`
/// starting with `https://`).
pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_SESSION_COOKIE
    } else {
        SESSION_COOKIE
    }
}

/// The signed-in session, as handlers take it.
pub type AuthSession = axum_login::AuthSession<Backend>;

/// The layer that gives every request its session and, through it, its
/// user. Its cookie: the name above, `HttpOnly`, `SameSite=Strict`,
/// `Path=/`, `Secure` over https; its lifetime is set at sign-in. A session
/// is only loaded when a handler asks for it.
pub fn layer(state: &AppState) -> axum_login::AuthManagerLayer<Backend, Store> {
    let sessions = SessionManagerLayer::new(Store::new(state.db.clone()))
        .with_name(cookie_name(state.secure_cookies))
        .with_http_only(true)
        .with_same_site(SameSite::Strict)
        .with_secure(state.secure_cookies)
        .with_path("/");
    axum_login::AuthManagerLayerBuilder::new(Backend::new(state.db.clone()), sessions).build()
}

/// tower-sessions removes the cookie (sign-out, or a session that has
/// ended) with a cookie built from the request's, which has no attributes;
/// a browser ignores a `__Host-` removal without `Secure`. Over https, this
/// adds it (wrapped around [`layer`] in `app`).
pub async fn secure_removals(
    axum::extract::State(secure): axum::extract::State<bool>,
    mut response: axum::response::Response,
) -> axum::response::Response {
    if !secure {
        return response;
    }
    let prefix = format!("{SECURE_SESSION_COOKIE}=");
    let headers = response.headers_mut();
    let fixed: Vec<_> = headers
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .map(|value| match value.to_str() {
            Ok(text) if text.starts_with(&prefix) && !text.contains("; Secure") => {
                axum::http::HeaderValue::from_str(&format!(
                    "{text}; Secure; HttpOnly; SameSite=Strict"
                ))
                .unwrap_or_else(|_| value.clone())
            }
            _ => value.clone(),
        })
        .collect();
    headers.remove(axum::http::header::SET_COOKIE);
    for value in fixed {
        headers.append(axum::http::header::SET_COOKIE, value);
    }
    response
}

/// The sessions table as a tower-sessions store.
#[derive(Clone)]
pub struct Store {
    db: Db,
}

impl Store {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Store")
    }
}

/// What is stored for a session id: never the id itself.
fn id_hash(id: &Id) -> String {
    crate::auth::sha256_hex(&id.to_string())
}

fn backend_error(e: impl fmt::Display) -> session_store::Error {
    session_store::Error::Backend(e.to_string())
}

fn encode(record: &Record) -> session_store::Result<String> {
    serde_json::to_string(&record.data).map_err(|e| session_store::Error::Encode(e.to_string()))
}

#[async_trait]
impl SessionStore for Store {
    /// A new session: a new id if the one drawn is taken (128 random bits,
    /// so in practice never).
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let data = encode(record)?;
        loop {
            let inserted = sqlx::query(
                "INSERT INTO sessions (id_hash, data, expires_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (id_hash) DO NOTHING",
            )
            .bind(id_hash(&record.id))
            .bind(&data)
            .bind(record.expiry_date.unix_timestamp())
            .execute(self.db.writer())
            .await
            .map_err(backend_error)?
            .rows_affected();
            if inserted == 1 {
                return Ok(());
            }
            record.id = Id::default();
        }
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id_hash, data, expires_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (id_hash) DO UPDATE SET data = ?2, expires_at = ?3",
        )
        .bind(id_hash(&record.id))
        .bind(encode(record)?)
        .bind(record.expiry_date.unix_timestamp())
        .execute(self.db.writer())
        .await
        .map_err(backend_error)?;
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT data, expires_at FROM sessions WHERE id_hash = ?1 AND expires_at > ?2",
        )
        .bind(id_hash(id))
        .bind(OffsetDateTime::now_utc().unix_timestamp())
        .fetch_optional(self.db.reader())
        .await
        .map_err(backend_error)?;
        let Some((data, expires_at)) = row else {
            return Ok(None);
        };
        Ok(Some(Record {
            id: *id,
            data: serde_json::from_str(&data)
                .map_err(|e| session_store::Error::Decode(e.to_string()))?,
            expiry_date: OffsetDateTime::from_unix_timestamp(expires_at).map_err(backend_error)?,
        }))
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id_hash = ?1")
            .bind(id_hash(id))
            .execute(self.db.writer())
            .await
            .map_err(backend_error)?;
        Ok(())
    }
}

#[async_trait]
impl ExpiredDeletion for Store {
    async fn delete_expired(&self) -> session_store::Result<()> {
        sqlx::query("DELETE FROM sessions WHERE expires_at <= ?1")
            .bind(OffsetDateTime::now_utc().unix_timestamp())
            .execute(self.db.writer())
            .await
            .map_err(backend_error)?;
        Ok(())
    }
}

/// axum-login's view of the users table.
#[derive(Clone)]
pub struct Backend {
    db: Db,
}

impl Backend {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

/// The signed-in user, with the hash that ties their sessions to their
/// password and two-factor secret.
#[derive(Clone)]
pub struct SessionOnlyUser {
    pub user: User,
    auth_hash: Vec<u8>,
}

impl fmt::Debug for SessionOnlyUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionOnlyUser")
            .field("id", &self.user.id)
            .finish_non_exhaustive()
    }
}

impl axum_login::AuthUser for SessionOnlyUser {
    type Id = Uuid;

    fn id(&self) -> Uuid {
        self.user.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.auth_hash
    }
}

/// SHA-256 of the password hash and the two-factor secret (each may be
/// empty), separated by a NUL, which neither contains.
fn auth_hash(password_hash: Option<&str>, totp_secret: Option<&str>) -> Vec<u8> {
    Sha256::new()
        .chain_update(password_hash.unwrap_or("").as_bytes())
        .chain_update([0])
        .chain_update(totp_secret.unwrap_or("").as_bytes())
        .finalize()
        .to_vec()
}

#[derive(sqlx::FromRow)]
struct UserRow {
    #[sqlx(flatten)]
    user: User,
    password_hash: Option<String>,
    totp_secret: Option<String>,
}

/// The user as a session holds them; `None` for a missing account.
pub async fn session_user(db: &Db, user_id: Uuid) -> Result<Option<SessionOnlyUser>, ApiError> {
    let row: Option<UserRow> = sqlx::query_as(
        "SELECT id, email, is_admin, date_format, use_24, theme,
            password_hash IS NOT NULL AS has_password, totp_secret IS NOT NULL AS totp_enabled,
            password_hash, totp_secret
         FROM users WHERE id = ?1",
    )
    .bind(user_id)
    .fetch_optional(db.reader())
    .await?;
    Ok(row.map(|row| SessionOnlyUser {
        auth_hash: auth_hash(row.password_hash.as_deref(), row.totp_secret.as_deref()),
        user: row.user,
    }))
}

/// axum-login's view of our users. Sign-in itself is ours (`routes::auth`,
/// `routes::oidc`, `routes::totp`): they check the password, the code or
/// the provider, then call [`sign_in`].
impl axum_login::AuthnBackend for Backend {
    type User = SessionOnlyUser;
    type Credentials = std::convert::Infallible;
    type Error = ApiError;

    async fn authenticate(
        &self,
        credentials: std::convert::Infallible,
    ) -> Result<Option<SessionOnlyUser>, ApiError> {
        match credentials {}
    }

    async fn get_user(&self, id: &Uuid) -> Result<Option<SessionOnlyUser>, ApiError> {
        session_user(&self.db, *id).await
    }
}

fn login_error(e: axum_login::Error<Backend>) -> ApiError {
    match e {
        axum_login::Error::Backend(e) => e,
        axum_login::Error::Session(e) => anyhow::Error::new(e).context("session").into(),
    }
}

/// Start a session for `user` (after their password, code or provider was
/// checked), lasting [`SESSION_TTL_SECS`]. Over https, the answer also clears
/// a plain-named session cookie from before the site used the `__Host-`
/// prefix (the jar to answer with).
pub async fn sign_in(
    auth: &mut AuthSession,
    state: &AppState,
    user: &User,
) -> Result<CookieJar, ApiError> {
    let session_user = session_user(&state.db, user.id)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    auth.login(&session_user).await.map_err(login_error)?;
    keep_until(auth, jiff::Timestamp::now().as_second() + SESSION_TTL_SECS).await?;
    let mut jar = CookieJar::new();
    if state.secure_cookies {
        let mut old = Cookie::new(SESSION_COOKIE, "");
        old.set_path("/");
        old.set_secure(true);
        old.make_removal();
        jar = jar.add(old);
    }
    Ok(jar)
}

async fn keep_until(auth: &AuthSession, unix_seconds: i64) -> Result<(), ApiError> {
    let at = OffsetDateTime::from_unix_timestamp(unix_seconds)
        .map_err(|e| anyhow::Error::new(e).context("session expiry"))?;
    auth.session.set_expiry(Some(Expiry::AtDateTime(at)));
    auth.session
        .insert(EXPIRES_KEY, unix_seconds)
        .await
        .map_err(|e| anyhow::Error::new(e).context("session"))?;
    Ok(())
}

/// After the user changed their own password or two-factor secret: their
/// other sessions are signed out (the hash changed), this one is signed in
/// again with the new hash, keeping when it expires.
pub async fn refresh(auth: &mut AuthSession, user_id: Uuid) -> Result<(), ApiError> {
    if auth.user.is_none() {
        return Ok(());
    }
    let user = auth
        .backend
        .get_user(&user_id)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    let expires = auth
        .session
        .get::<i64>(EXPIRES_KEY)
        .await
        .map_err(|e| anyhow::Error::new(e).context("session"))?
        .unwrap_or_else(|| jiff::Timestamp::now().as_second() + SESSION_TTL_SECS);
    auth.login(&user).await.map_err(login_error)?;
    keep_until(auth, expires).await
}

/// End this session (sign out).
pub async fn sign_out(auth: &mut AuthSession) -> Result<(), ApiError> {
    auth.logout().await.map_err(login_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use time::Duration;

    /// A record expiring at the given time, to the second (as stored).
    fn record(expiry_date: OffsetDateTime) -> Record {
        Record {
            id: Id::default(),
            data: HashMap::from([("k".to_string(), serde_json::json!("v"))]),
            expiry_date: OffsetDateTime::from_unix_timestamp(expiry_date.unix_timestamp()).unwrap(),
        }
    }

    #[test]
    fn the_hash_changes_with_the_password_and_the_secret() {
        let base = auth_hash(Some("a"), None);
        assert_ne!(base, auth_hash(Some("b"), None));
        assert_ne!(base, auth_hash(Some("a"), Some("S")));
        assert_eq!(base, auth_hash(Some("a"), Some("")));
        // The separator keeps "ab" + "" apart from "a" + "b".
        assert_ne!(auth_hash(Some("ab"), None), auth_hash(Some("a"), Some("b")));
    }

    #[tokio::test]
    async fn the_store_keeps_no_session_id_and_drops_expired_ones() {
        let db = crate::db::open(":memory:").await;
        let store = Store::new(db.clone());
        let later = OffsetDateTime::now_utc() + Duration::hours(1);

        let mut live = record(later);
        store.create(&mut live).await.unwrap();
        let stored: Vec<String> = sqlx::query_scalar("SELECT id_hash FROM sessions")
            .fetch_all(db.reader())
            .await
            .unwrap();
        assert_eq!(stored, [id_hash(&live.id)]);
        assert_ne!(stored[0], live.id.to_string());
        assert_eq!(store.load(&live.id).await.unwrap(), Some(live.clone()));

        // Saved again: updated, not added.
        let mut changed = live.clone();
        changed.data.insert("k".into(), serde_json::json!("w"));
        store.save(&changed).await.unwrap();
        assert_eq!(store.load(&live.id).await.unwrap(), Some(changed));

        // An expired one is not loaded, and the cleanup deletes it.
        let mut old = record(OffsetDateTime::now_utc() - Duration::seconds(1));
        store.create(&mut old).await.unwrap();
        assert_eq!(store.load(&old.id).await.unwrap(), None);
        store.delete_expired().await.unwrap();
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(db.reader())
            .await
            .unwrap();
        assert_eq!(left, 1);

        store.delete(&live.id).await.unwrap();
        assert_eq!(store.load(&live.id).await.unwrap(), None);
    }
}
