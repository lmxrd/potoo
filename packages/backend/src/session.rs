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
//! password or the two-factor secret signs the user's other sessions out.

use crate::auth::{SESSION_DAYS, User, verify_password};
use crate::db::Db;
use crate::error::HttpError;
use async_trait::async_trait;
use axum_login::AuthnBackend as _;
use sha2::{Digest, Sha256};
use std::fmt;
use time::{Duration, OffsetDateTime};
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, ExpiredDeletion, SessionStore};
use tower_sessions::{Expiry, Session};

/// The session cookie's name.
pub const COOKIE: &str = "potoo_session";

/// The extractor of the signed-in session.
pub type AuthSession = axum_login::AuthSession<Backend>;

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
    crate::util::sha256_hex(&id.to_string())
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

/// An email and a password, as the login form sends them.
#[derive(Clone)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

impl axum_login::AuthUser for User {
    type Id = i64;

    fn id(&self) -> i64 {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.auth_hash
    }
}

/// A valid hash to verify against when the user does not exist, so the
/// answer takes the same time either way.
fn dummy_hash() -> &'static str {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| crate::auth::hash_password("potoo-dummy"))
}

/// The columns of a [`User`], plus what its auth hash is made of.
const USER_COLUMNS: &str = "id, email, is_admin, date_format, use_24, theme,
    password_hash IS NOT NULL AS has_password, totp_secret IS NOT NULL AS totp_enabled,
    password_hash, totp_secret";

#[derive(sqlx::FromRow)]
struct UserRow {
    #[sqlx(flatten)]
    user: User,
    password_hash: Option<String>,
    totp_secret: Option<String>,
}

impl UserRow {
    /// The user, with the hash of the credentials their sessions are
    /// checked against: the password hash and the two-factor secret.
    fn into_user(self) -> User {
        let mut hasher = Sha256::new();
        hasher.update(self.password_hash.as_deref().unwrap_or("").as_bytes());
        hasher.update([0]);
        hasher.update(self.totp_secret.as_deref().unwrap_or("").as_bytes());
        User {
            auth_hash: hasher.finalize().to_vec(),
            ..self.user
        }
    }
}

impl axum_login::AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = sqlx::Error;

    /// The user of an email and a password. The same work whether or not
    /// the user exists, or has a password.
    async fn authenticate(&self, creds: Credentials) -> Result<Option<User>, sqlx::Error> {
        let row: Option<UserRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {USER_COLUMNS} FROM users WHERE email = ?1"
        )))
        .bind(&creds.email)
        .fetch_optional(self.db.reader())
        .await?;
        let hash = row
            .as_ref()
            .and_then(|r| r.password_hash.clone())
            .unwrap_or_else(|| dummy_hash().to_string());
        let valid = verify_password(&creds.password, &hash);
        Ok(row
            .filter(|r| valid && r.password_hash.is_some())
            .map(UserRow::into_user))
    }

    async fn get_user(&self, id: &i64) -> Result<Option<User>, sqlx::Error> {
        let row: Option<UserRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = ?1"
        )))
        .bind(id)
        .fetch_optional(self.db.reader())
        .await?;
        Ok(row.map(UserRow::into_user))
    }
}

/// Sign the user in on this session (a new session id), for
/// [`SESSION_DAYS`] from now.
pub async fn sign_in(auth: &mut AuthSession, user: &User) -> Result<(), HttpError> {
    auth.login(user).await.map_err(anyhow::Error::new)?;
    set_expiry(&auth.session);
    Ok(())
}

/// After the user changed their own password or two-factor secret: their
/// other sessions are signed out (the hash changed), this one stays signed
/// in with the new hash.
pub async fn refresh(auth: &mut AuthSession, user_id: i64) -> Result<(), HttpError> {
    let user = auth
        .backend
        .get_user(&user_id)
        .await?
        .ok_or_else(HttpError::unauthorized)?;
    auth.login(&user).await.map_err(anyhow::Error::new)?;
    Ok(())
}

/// The session ends this many days after the sign-in, however active it
/// is: as before, and it saves a write per request.
fn set_expiry(session: &Session) {
    let at = OffsetDateTime::now_utc() + Duration::days(SESSION_DAYS as i64);
    session.set_expiry(Some(Expiry::AtDateTime(at)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A record expiring at the given time, to the second (as stored).
    fn record(expiry_date: OffsetDateTime) -> Record {
        Record {
            id: Id::default(),
            data: HashMap::from([("k".to_string(), serde_json::json!("v"))]),
            expiry_date: OffsetDateTime::from_unix_timestamp(expiry_date.unix_timestamp()).unwrap(),
        }
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
