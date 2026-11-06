use crate::util::sha256_hex;
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, generate_salt};
use serde::{Deserialize, Serialize};
use sqlx::SqliteExecutor;
use std::time::SystemTime;
use utoipa::ToSchema;
use uuid::Uuid;

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
    /// The hash of the credentials the user's sessions are checked against
    /// (see `session.rs`); never sent.
    #[serde(skip)]
    #[sqlx(default)]
    pub auth_hash: Vec<u8>,
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
    .bind(now_iso())
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

pub fn now_iso() -> String {
    iso(SystemTime::now())
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ`, the form times are stored in.
pub fn iso(t: SystemTime) -> String {
    let t = jiff::Timestamp::try_from(t).expect("a time within jiff's range");
    format!("{t:.3}")
}
