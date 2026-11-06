use crate::db::Db;
use crate::profile::{Phase, timed};
use crate::util::{random_hex, sha256_hex};
use axum::http::HeaderMap;
use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};

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

/// The browser family, as a lowercase key (`chrome`, `safari`, ...): what
/// is stored. Display names are the frontend's business. The order matters:
/// Edge, Opera and iOS Chrome all contain "Chrome" and "Safari".
///
/// A user agent that is none of the known ones is not given a generic
/// value: the name of the product it announces is stored as it is (see
/// [`product_name`]), and reported once in the log, so that a migration can
/// name it later. `other` is only for a user agent that announces nothing.
pub fn parse_browser(ua: &str) -> String {
    let lower = ua.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("edg/") {
        return "edge".into();
    }
    if has("opr/") || has("opera") {
        return "opera".into();
    }
    if has("crios") || has("chrome/") {
        return "chrome".into();
    }
    if has("firefox/") || has("fxios") {
        return "firefox".into();
    }
    if has("safari/") {
        return "safari".into();
    }
    match product_name(ua) {
        Some(name) => {
            report_unmapped("browser", &name);
            name
        }
        None => "other".into(),
    }
}

/// The name of the last product a user agent announces (`PaleMoon/32.5`
/// gives `palemoon`), lowercase and without the version, or None. Only a
/// short plain name qualifies: letters, digits, `.`, `_` and `-`, at most 32
/// of them. Anything else (an address, an identifier, free text) is not
/// stored: the user agent itself is never kept, only a name that says what
/// kind of client it is.
fn product_name(ua: &str) -> Option<String> {
    // The parenthesized parts are the platform and the comments.
    let mut outside = String::new();
    let mut depth = 0;
    for c in ua.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => outside.push(c),
            _ => {}
        }
    }
    outside
        .split_whitespace()
        .filter_map(|token| token.split_once('/'))
        .filter_map(|(name, _version)| plain_name(name))
        .rfind(|name| !NOT_A_PRODUCT.contains(&name.as_str()))
}

/// Names that are part of every user agent, or of the engine, not of the
/// client that sends it.
const NOT_A_PRODUCT: [&str; 14] = [
    "mozilla",
    "applewebkit",
    "khtml",
    "gecko",
    "webkit",
    "trident",
    "presto",
    "version",
    "mobile",
    "safari",
    "chrome",
    "cfnetwork",
    "darwin",
    "build",
];

/// A lowercase name of 1 to 32 plain characters, or None.
fn plain_name(raw: &str) -> Option<String> {
    let name = raw.trim().to_lowercase();
    let plain = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    (plain && (1..=32).contains(&name.len())).then_some(name)
}

/// Report a value that has no known name, once per value: it is stored as
/// it is, and this is how a person finds out which ones there are. The list
/// is bounded, so a client sending endless different names cannot flood the
/// log or the memory.
fn report_unmapped(facet: &str, value: &str) {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut seen = SEEN.get_or_init(Default::default).lock().unwrap();
    if seen.len() < 500 && seen.insert(format!("{facet}:{value}")) {
        tracing::warn!(
            facet,
            value,
            "no known name for this value: stored as it is, to be named by a migration"
        );
    }
}

/// The coarse operating system.
pub fn parse_os(ua: &str) -> String {
    let lower = ua.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("windows") {
        return "windows".into();
    }
    if has("iphone") || has("ipod") {
        return "ios".into();
    }
    if has("ipad") {
        return "ipados".into();
    }
    if has("android") {
        return "android".into();
    }
    if has("macintosh") || has("mac os x") {
        return "macos".into();
    }
    if has("linux") || has("x11") {
        return "linux".into();
    }
    // Not a known one: the platform the user agent names, stored as it is.
    match platform_name(ua) {
        Some(name) => {
            report_unmapped("os", &name);
            name
        }
        None => "other".into(),
    }
}

/// The first plain name in the platform part of a user agent, the first
/// parenthesized group: `(CrOS x86_64 14541.0.0)` gives `cros`.
fn platform_name(ua: &str) -> Option<String> {
    const NOT_A_PLATFORM: [&str; 12] = [
        "compatible",
        "u",
        "i",
        "n",
        "mozilla",
        "mobile",
        "tablet",
        "wow64",
        "win64",
        "x64",
        "x86_64",
        "arm64",
    ];
    let group = ua.split_once('(')?.1.split_once(')')?.0;
    group
        .split(';')
        .filter_map(|part| plain_name(part.split_whitespace().next()?))
        .find(|name| {
            !NOT_A_PLATFORM.contains(&name.as_str()) && name.chars().any(|c| c.is_ascii_lowercase())
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

    #[test]
    fn the_known_families_are_lowercase_keys() {
        assert_eq!(parse_browser(CHROME_MAC), "chrome");
        assert_eq!(parse_os(CHROME_MAC), "macos");
        assert_eq!(
            parse_os("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)"),
            "ios"
        );
        assert_eq!(
            parse_os("Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X)"),
            "ipados"
        );
    }

    #[test]
    fn an_unknown_browser_is_stored_under_the_name_it_announces() {
        assert_eq!(
            parse_browser("MyApp/1.0 CFNetwork/1494 Darwin/23.0"),
            "myapp"
        );
        assert_eq!(
            parse_browser("Mozilla/5.0 (X11; Linux x86_64) Gecko/20100101 PaleMoon/32.5.0"),
            "palemoon"
        );
        assert_eq!(
            parse_browser("Dalvik/2.1.0 (Linux; U; Android 13; Pixel 8 Build/TQ3A)"),
            "dalvik"
        );
    }

    #[test]
    fn an_unknown_os_is_stored_under_the_name_it_announces() {
        assert_eq!(
            parse_os("Mozilla/5.0 (CrOS x86_64 14541.0.0) AppleWebKit/537.36"),
            "cros"
        );
        assert_eq!(
            parse_os("Mozilla/5.0 (FreeBSD amd64) Gecko/20100101"),
            "freebsd"
        );
        assert_eq!(
            parse_os("Mozilla/5.0 (SunOS; U; sun4u) Gecko/20100101"),
            "sunos"
        );
    }

    #[test]
    fn only_a_short_plain_name_is_stored_never_the_user_agent() {
        // Nothing announced, or nothing that is a plain name: the generic key.
        assert_eq!(parse_browser("Mozilla/5.0 (compatible)"), "other");
        assert_eq!(parse_os("Mozilla/5.0"), "other");
        // An address, a name that is too long, or free text is not kept.
        assert_eq!(parse_browser("user@example.com/1.0"), "other");
        assert_eq!(parse_browser(&format!("{}/1.0", "a".repeat(33))), "other");
        assert_eq!(parse_browser("Name With Spaces/1.0"), "spaces");
        assert_eq!(parse_os("Mozilla/5.0 (jane.doe@example.com; x)"), "x");
        // The version never comes along.
        assert!(!parse_browser("Quux/9.9.9").contains('9'));
    }
}
