use crate::util::{random_hex, sha256_hex};
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, generate_salt};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};
use sqlx::SqliteExecutor;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use utoipa::ToSchema;

pub const COOKIE: &str = "potoo_session";
const SESSION_DAYS: u64 = 30;

/// The date formats a user can pick from, for displaying dates.
pub const DATE_FORMATS: [&str; 4] = ["YYYY-MM-DD", "DD/MM/YYYY", "MM/DD/YYYY", "DD.MM.YYYY"];

/// The signed-in user, as the session endpoint returns them.
#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct User {
    pub id: i64,
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

/// The user of a valid session cookie, if any.
pub async fn current_user<'e>(db: impl SqliteExecutor<'e>, token: Option<&str>) -> Option<User> {
    let token = token?;
    sqlx::query_as::<_, User>(
        "SELECT u.id, u.email, u.is_admin, u.date_format, u.use_24, u.theme,
                u.password_hash IS NOT NULL AS has_password,
                u.totp_secret IS NOT NULL AS totp_enabled
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ?1 AND s.expires_at > ?2",
    )
    .bind(sha256_hex(token))
    .bind(now_iso())
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
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
    pub id: i64,
    pub scope: Scope,
    pub last_used_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct KeyRow {
    #[sqlx(flatten)]
    user: User,
    key_id: i64,
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

/// Create a session and set its cookie, returning the updated jar.
pub async fn create_session<'e>(
    db: impl SqliteExecutor<'e>,
    jar: CookieJar,
    user_id: i64,
    secure: bool,
) -> sqlx::Result<CookieJar> {
    let token = random_hex(32);
    let expires = SystemTime::now() + Duration::from_secs(SESSION_DAYS * 86_400);
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)")
        .bind(sha256_hex(&token))
        .bind(user_id)
        .bind(iso(expires))
        .execute(db)
        .await?;
    Ok(jar.add(
        Cookie::build((COOKIE.to_string(), token))
            .path("/")
            .http_only(true)
            .same_site(SameSite::Strict)
            .secure(secure)
            .expires(time::OffsetDateTime::from(expires)),
    ))
}

/// Delete the session of the request's cookie, and clear the cookie.
pub async fn destroy_session<'e>(
    db: impl SqliteExecutor<'e>,
    jar: CookieJar,
) -> sqlx::Result<CookieJar> {
    if let Some(cookie) = jar.get(COOKIE) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?1")
            .bind(sha256_hex(cookie.value()))
            .execute(db)
            .await?;
    }
    Ok(jar.remove(Cookie::from(COOKIE)))
}

/// In-memory throttle for failed logins: after MAX_FAILURES within the
/// window the IP + email pair is locked out until the oldest failure
/// expires. It resets when the server restarts, which keeps the
/// no-storage promise.
const WINDOW_MS: i64 = 15 * 60_000;
const MAX_FAILURES: usize = 5;

#[derive(Default)]
pub struct LoginThrottle {
    failures: Mutex<HashMap<String, VecDeque<i64>>>,
}

impl LoginThrottle {
    /// The failures of a key that are still inside the window.
    fn recent(&self, key: &str) -> VecDeque<i64> {
        let now = now_ms();
        let mut map = self.failures.lock().unwrap();
        let recent: VecDeque<i64> = map
            .get(key)
            .map(|times| {
                times
                    .iter()
                    .copied()
                    .filter(|t| now - t < WINDOW_MS)
                    .collect()
            })
            .unwrap_or_default();
        if recent.is_empty() {
            map.remove(key);
        } else {
            map.insert(key.to_string(), recent.clone());
        }
        recent
    }

    /// Whether the key is locked out, and how long to wait if it is.
    pub fn locked_out(&self, key: &str) -> Option<i64> {
        let recent = self.recent(key);
        if recent.len() < MAX_FAILURES {
            return None;
        }
        let retry_ms = recent[0] + WINDOW_MS - now_ms();
        Some((retry_ms + 999) / 1000)
    }

    pub fn record_failure(&self, key: &str) {
        let now = now_ms();
        let mut map = self.failures.lock().unwrap();
        map.entry(key.to_string()).or_default().push_back(now);
        if map.len() > 10_000 {
            map.clear();
        }
    }

    pub fn clear(&self, key: &str) {
        self.failures.lock().unwrap().remove(key);
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn now_iso() -> String {
    iso(SystemTime::now())
}

pub fn iso(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
