use crate::util::{random_hex, sha256_hex};
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, generate_salt};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const COOKIE: &str = "potoo_session";
const SESSION_DAYS: u64 = 30;

/// The date formats a user can pick from, for displaying dates.
pub const DATE_FORMATS: [&str; 5] = [
    "YYYY-MM-DD",
    "DD/MM/YYYY",
    "MM/DD/YYYY",
    "DD.MM.YYYY",
    "D MMM YYYY",
];

/// The time formats a user can pick from, for displaying times.
pub const TIME_FORMATS: [&str; 2] = ["24h", "12h"];

/// The signed-in user, as the session endpoint returns them.
#[derive(Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub is_admin: i64,
    pub date_format: String,
    pub time_format: String,
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
pub fn current_user(conn: &Connection, token: Option<&str>) -> Option<User> {
    let token = token?;
    let now = now_iso();
    conn.query_row(
        "SELECT u.id, u.email, u.is_admin, u.date_format, u.time_format
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ?1 AND s.expires_at > ?2",
        params![sha256_hex(token), now],
        |row| {
            Ok(User {
                id: row.get(0)?,
                email: row.get(1)?,
                is_admin: row.get(2)?,
                date_format: row.get(3)?,
                time_format: row.get(4)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

/// Create a session and set its cookie, returning the updated jar.
pub fn create_session(
    conn: &Connection,
    jar: CookieJar,
    user_id: i64,
    secure: bool,
) -> rusqlite::Result<CookieJar> {
    let token = random_hex(32);
    let expires = SystemTime::now() + Duration::from_secs(SESSION_DAYS * 86_400);
    conn.execute(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
        params![sha256_hex(&token), user_id, iso(expires)],
    )?;
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
pub fn destroy_session(conn: &Connection, jar: CookieJar) -> rusqlite::Result<CookieJar> {
    if let Some(cookie) = jar.get(COOKIE) {
        let token = cookie.value().to_string();
        conn.execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            params![sha256_hex(&token)],
        )?;
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

fn iso(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
