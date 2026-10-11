use crate::db::Db;
use crate::profile::{Phase, timed};
use crate::util::{random_hex, sha256_hex};
use axum::http::HeaderMap;
use std::net::IpAddr;

/// The device class of a hit.
pub enum Device {
    Desktop,
    Mobile,
    Tablet,
}

impl Device {
    pub fn as_str(&self) -> &'static str {
        match self {
            Device::Desktop => "desktop",
            Device::Mobile => "mobile",
            Device::Tablet => "tablet",
        }
    }
}

const BOT: [&str; 10] = [
    "bot",
    "crawl",
    "spider",
    "slurp",
    "headless",
    "lighthouse",
    "curl",
    "wget",
    "python-requests",
    "preview",
];

/// The device class from a user agent, or None for a bot.
pub fn parse_device(ua: &str) -> Option<Device> {
    let lower = ua.to_lowercase();
    if ua.is_empty() || BOT.iter().any(|b| lower.contains(b)) {
        return None;
    }
    let has = |needle: &str| lower.contains(needle);
    if has("ipad") || has("tablet") || has("playbook") || has("silk") {
        return Some(Device::Tablet);
    }
    if has("android") && !has("mobile") {
        return Some(Device::Tablet);
    }
    if has("mobi") || has("iphone") || has("ipod") || has("android") || has("windows phone") {
        return Some(Device::Mobile);
    }
    Some(Device::Desktop)
}

/// The coarse browser family. The order matters: Edge, Opera and iOS
/// Chrome all contain "Chrome" and "Safari".
pub fn parse_browser(ua: &str) -> &'static str {
    let lower = ua.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("edg/") {
        return "Edge";
    }
    if has("opr/") || has("opera") {
        return "Opera";
    }
    if has("crios") || has("chrome/") {
        return "Chrome";
    }
    if has("firefox/") || has("fxios") {
        return "Firefox";
    }
    if has("safari/") {
        return "Safari";
    }
    "Other"
}

/// The coarse operating system.
pub fn parse_os(ua: &str) -> &'static str {
    let lower = ua.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("windows") {
        return "Windows";
    }
    if has("iphone") || has("ipod") {
        return "iOS";
    }
    if has("ipad") {
        return "iPadOS";
    }
    if has("android") {
        return "Android";
    }
    if has("macintosh") || has("mac os x") {
        return "macOS";
    }
    if has("linux") || has("x11") {
        return "Linux";
    }
    "Other"
}

/// The client IP: the first X-Forwarded-For entry when the proxy is
/// trusted, otherwise the socket's address.
pub fn client_ip(headers: &HeaderMap, addr: Option<IpAddr>, trust_proxy: bool) -> String {
    if trust_proxy
        && let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = forwarded.split(',').next()
    {
        let first = first.trim();
        if !first.is_empty() {
            return first.to_string();
        }
    }
    addr.map(|a| a.to_string()).unwrap_or_default()
}

/// The salt of today; each day gets a fresh one, and older days are
/// deleted, so hashes cannot be linked across days.
async fn daily_salt(db: &Db) -> String {
    let day = crate::auth::now_iso()[..10].to_string();
    if let Some(salt) = db.cached_salt(&day) {
        return salt;
    }
    let existing: Option<String> = sqlx::query_scalar("SELECT salt FROM salts WHERE day = ?1")
        .bind(&day)
        .fetch_optional(db.reader())
        .await
        .unwrap_or(None);
    if let Some(salt) = existing {
        db.remember_salt(&day, &salt);
        return salt;
    }
    // The first hit of a day creates its salt, and removes the older ones.
    let created = async {
        let mut tx = db.writer().begin().await?;
        sqlx::query("INSERT OR IGNORE INTO salts (day, salt) VALUES (?1, ?2)")
            .bind(&day)
            .bind(random_hex(32))
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM salts WHERE day < ?1")
            .bind(&day)
            .execute(&mut *tx)
            .await?;
        let salt: String = sqlx::query_scalar("SELECT salt FROM salts WHERE day = ?1")
            .bind(&day)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok::<String, sqlx::Error>(salt)
    }
    .await
    .unwrap_or_default();
    // A failed write leaves the salt empty: do not keep that for the day.
    if !created.is_empty() {
        db.remember_salt(&day, &created);
    }
    created
}

/// Pseudonymous, per-day, per-site visitor id. The IP and UA are never
/// stored: only this hash of them, truncated to 32 hex characters.
pub async fn visitor_hash(db: &Db, site_id: i64, ip: &str, ua: &str) -> String {
    let salt = timed(Phase::Salt, daily_salt(db)).await;
    sha256_hex(&format!("{salt}|{site_id}|{ip}|{ua}"))[..32].to_string()
}
