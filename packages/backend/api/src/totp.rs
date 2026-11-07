//! Two-factor authentication with time-based one-time passwords (RFC 6238),
//! through `totp-rs`: HMAC-SHA1, 6 digits, a 30-second step, the
//! authenticator apps' defaults. The crate does not stop a code from being
//! used twice; `verify` takes the last step used for that.

use crate::auth::sha256_hex;
use crate::error::ApiError;
use crate::state::AppState;
use crate::time::unix_now;
use axum::http::{HeaderMap, StatusCode};
use axum_extra::extract::SignedCookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use rand::{Rng, RngExt};
use sqlx::SqliteExecutor;
use std::time::Duration;
use totp_rs::{Algorithm, Builder, Secret, Totp};
use uuid::Uuid;

const STEP_SECONDS: u64 = 30;
/// A code from the step before or after the current one is accepted too, for
/// a phone clock that is a little off.
const SKEW: u16 = 1;
/// How many recovery codes a user gets at a time.
pub const RECOVERY_CODES: usize = 10;

/// The TOTP of a base32 secret, labelled with the account for the app.
fn totp(secret: &str, account: &str) -> Option<Totp> {
    let bytes = Secret::try_from_base32(secret).ok()?;
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(SKEW)
        .with_step_duration(STEP_SECONDS)
        .with_secret(bytes)
        .with_account_name(account)
        .with_issuer(Some("Potoo"))
        .build()
        .ok()
}

/// A new secret: 20 random bytes (160 bits, RFC 4226's recommendation),
/// base32, as the authenticator apps take it.
pub fn new_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::rng().fill_bytes(&mut bytes);
    Secret::from(bytes.to_vec()).to_base32()
}

/// The `otpauth://` URI an authenticator app scans as a QR code.
pub fn uri(secret: &str, email: &str) -> Option<String> {
    totp(secret, email)?.to_url().ok()
}

/// The current time step.
pub fn current_step() -> u64 {
    crate::time::unix_now() / STEP_SECONDS
}

/// The step `code` belongs to, if it is the code of the secret for a step
/// within the allowed skew of the step `now` and later than `after` (the
/// last step used, so a code works once). The caller stores the step it
/// returns.
pub fn verify(secret: &str, code: &str, now: u64, after: u64) -> Option<u64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // The account name does not count for a code.
    totp(secret, "user")?
        .check(&code, now * STEP_SECONDS)
        .filter(|&step| step > after)
}

/// The code of a secret for a time step (the tests use it as an app would).
pub fn code_at(secret: &str, step: u64) -> Option<String> {
    Some(
        totp(secret, "user")?
            .generate(step * STEP_SECONDS)
            .to_string(),
    )
}

/// New recovery codes, as shown to the user once: ten groups of
/// `xxxxx-xxxxx` from an alphabet without look-alikes (50 bits each).
pub fn new_recovery_codes() -> Vec<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::rng();
    (0..RECOVERY_CODES)
        .map(|_| {
            let mut code = String::with_capacity(11);
            for i in 0..10 {
                if i == 5 {
                    code.push('-');
                }
                code.push(ALPHABET[rng.random_range(0..ALPHABET.len())] as char);
            }
            code
        })
        .collect()
}

/// What is stored for a recovery code. The codes are random and long, so a
/// plain hash is enough (no password hashing); spaces, dashes and case do
/// not count, so a code typed differently still matches.
pub fn recovery_code_hash(code: &str) -> String {
    let normalized: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    sha256_hex(&normalized)
}

// ---------------------------------------------------------------------------
// The stored state: the codes in use, recovery codes, and the sign-in's
// second step (the challenge cookie).
// ---------------------------------------------------------------------------

pub const CHALLENGE_TTL: Duration = Duration::from_secs(5 * 60);
const CHALLENGE_COOKIE: &str = "potoo_2fa";
const CHALLENGE_PATH: &str = "/api/auth/login";

/// The challenge cookie's name and path: like the single sign-on cookie, a
/// `__Host-` cookie over https (which needs the path `/`), so a sibling
/// subdomain cannot plant one; over plain http it goes to the login paths
/// only.
pub fn challenge_name(secure: bool) -> (&'static str, &'static str) {
    if secure {
        ("__Host-potoo_2fa", "/")
    } else {
        (CHALLENGE_COOKIE, CHALLENGE_PATH)
    }
}

pub fn challenge(value: String, secure: bool) -> Cookie<'static> {
    let (name, path) = challenge_name(secure);
    Cookie::build((name, value))
        .path(path)
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .max_age(time::Duration::seconds(CHALLENGE_TTL.as_secs() as i64))
        .build()
}

/// What a sign-in still needs before its session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// A code from the app (two-factor authentication is on).
    Code,
    /// Setting the app up (it is off, and `TOTP_REQUIRED` is set).
    Enroll,
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Code => "code",
            Step::Enroll => "enroll",
        }
    }

    /// Where single sign-on sends the browser to take this step.
    pub fn page(self) -> &'static str {
        match self {
            Step::Code => "/login?step=totp",
            Step::Enroll => "/login?step=totp-setup",
        }
    }
}

/// The step a user who just proved who they are must take next, if any.
pub async fn next_step<'e>(
    db: impl SqliteExecutor<'e>,
    state: &AppState,
    user_id: Uuid,
) -> sqlx::Result<Option<Step>> {
    let on: bool = sqlx::query_scalar("SELECT totp_secret IS NOT NULL FROM users WHERE id = ?1")
        .bind(user_id)
        .fetch_one(db)
        .await?;
    Ok(if on {
        Some(Step::Code)
    } else if state.totp_required {
        Some(Step::Enroll)
    } else {
        None
    })
}

/// The signed cookie that lets this browser finish the sign-in of a user
/// who proved who they are: `<user id>.<expiry, unix seconds>.<step>`. It is
/// signed with its own key, made at start: a restart makes the user sign in
/// again.
pub fn challenge_cookie(state: &AppState, user_id: Uuid, step: Step) -> SignedCookieJar {
    let expires = unix_now() + CHALLENGE_TTL.as_secs();
    let value = format!("{user_id}.{expires}.{}", step.as_str());
    SignedCookieJar::new(state.challenge_key.clone()).add(challenge(value, state.secure_cookies))
}

/// The user of a valid, unexpired challenge cookie for this step, or a 401.
pub fn challenged_user(
    state: &AppState,
    headers: &HeaderMap,
    step: Step,
) -> Result<Uuid, ApiError> {
    let expired = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "The sign-in expired, sign in again",
        )
    };
    let (name, _) = challenge_name(state.secure_cookies);
    let value = SignedCookieJar::from_headers(headers, state.challenge_key.clone())
        .get(name)
        .ok_or_else(expired)?
        .value()
        .to_string();
    let mut parts = value.split('.');
    let (Some(user), Some(expires), Some(kind), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(expired());
    };
    let expires: u64 = expires.parse().map_err(|_| expired())?;
    if expires <= unix_now() || kind != step.as_str() {
        return Err(expired());
    }
    user.parse().map_err(|_| expired())
}

/// Whether `code` is a valid code of the user's authenticator app or one of
/// their recovery codes; either is used up. False when two-factor
/// authentication is off. The caller counted the attempt (`state.limits`).
pub async fn use_code(state: &AppState, user_id: Uuid, code: &str) -> Result<bool, ApiError> {
    if code.len() > 64 {
        return Ok(false);
    }
    let row: Option<(Option<String>, i64)> =
        sqlx::query_as("SELECT totp_secret, totp_last_step FROM users WHERE id = ?1")
            .bind(user_id)
            .fetch_optional(state.db.reader())
            .await?;
    let Some((Some(secret), last_step)) = row else {
        return Ok(false);
    };
    let used = if let Some(step) = verify(&secret, code, current_step(), last_step.max(0) as u64) {
        // Only if no other request used this step (or a later one) since,
        // and the secret is still the one checked.
        sqlx::query(
            "UPDATE users SET totp_last_step = ?1
             WHERE id = ?2 AND totp_secret = ?3 AND totp_last_step < ?1",
        )
        .bind(step as i64)
        .bind(user_id)
        .bind(&secret)
        .execute(state.db.writer())
        .await?
        .rows_affected()
            == 1
    } else {
        let used = sqlx::query(
            "DELETE FROM recovery_codes WHERE user_id = ?1 AND code_hash = ?2
             AND EXISTS (SELECT 1 FROM users WHERE id = ?1 AND totp_secret IS NOT NULL)",
        )
        .bind(user_id)
        .bind(recovery_code_hash(code))
        .execute(state.db.writer())
        .await?
        .rows_affected()
            == 1;
        if used {
            tracing::info!("user {user_id} used a recovery code");
        }
        used
    };
    Ok(used)
}

/// Replace a user's recovery codes with new ones, returning them.
pub async fn replace_recovery_codes(
    tx: &mut sqlx::SqliteConnection,
    user_id: Uuid,
) -> Result<Vec<String>, ApiError> {
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let codes = new_recovery_codes();
    for code in &codes {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?1, ?2)")
            .bind(user_id)
            .bind(recovery_code_hash(code))
            .execute(&mut *tx)
            .await?;
    }
    Ok(codes)
}

/// Turn two-factor authentication off for a user: no secret, no recovery codes.
/// False when there is no such user.
pub async fn remove(state: &AppState, user_id: Uuid) -> Result<bool, ApiError> {
    let mut tx = state.db.writer().begin().await?;
    let updated = sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_pending = NULL, totp_last_step = 0
         WHERE id = ?1",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(updated == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238, appendix B: the SHA-1 key is "12345678901234567890" and the
    // test values are 8 digits; their last 6 are what a 6-digit code is.
    #[test]
    fn matches_the_rfc_6238_test_vectors() {
        let secret = Secret::from(b"12345678901234567890".to_vec()).to_base32();
        for (time, expected) in [
            (59u64, "94287082"),
            (1111111109, "07081804"),
            (1111111111, "14050471"),
            (1234567890, "89005924"),
            (2000000000, "69279037"),
            (20000000000, "65353130"),
        ] {
            let step = time / STEP_SECONDS;
            assert_eq!(code_at(&secret, step).unwrap(), &expected[2..], "at {time}");
        }
    }

    #[test]
    fn a_code_works_within_the_skew_and_only_once() {
        let secret = new_secret();
        assert_eq!(secret.len(), 32);
        let now = 1_000_000;
        let code = code_at(&secret, now).unwrap();
        assert_eq!(verify(&secret, &code, now, 0), Some(now));
        let spaced = format!(" {} {} ", &code[..3], &code[3..]);
        assert_eq!(verify(&secret, &spaced, now, 0), Some(now));
        // Used: the same step is refused, an earlier one too.
        assert_eq!(verify(&secret, &code, now, now), None);
        let previous = code_at(&secret, now - 1).unwrap();
        assert_eq!(verify(&secret, &previous, now, 0), Some(now - 1));
        assert_eq!(verify(&secret, &previous, now, now), None);
        // Too old, too new, or not a code.
        let old = code_at(&secret, now - 2).unwrap();
        let new = code_at(&secret, now + 2).unwrap();
        // (A code may repeat within the window by chance: skip that case.)
        if ![code.as_str(), previous.as_str()].contains(&old.as_str()) {
            assert_eq!(verify(&secret, &old, now, 0), None);
        }
        if new != code && new != previous && Some(new.clone()) != code_at(&secret, now + 1) {
            assert_eq!(verify(&secret, &new, now, 0), None);
        }
        assert_eq!(verify(&secret, "12345", now, 0), None);
        assert_eq!(verify(&secret, "abcdef", now, 0), None);
        assert_eq!(verify("not base32!", "123456", now, 0), None);
    }

    #[test]
    fn recovery_codes_are_distinct_and_typed_loosely() {
        let codes = new_recovery_codes();
        assert_eq!(codes.len(), RECOVERY_CODES);
        assert!(
            codes
                .iter()
                .all(|c| c.len() == 11 && c.as_bytes()[5] == b'-')
        );
        let mut unique = codes.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), RECOVERY_CODES);
        assert_eq!(
            recovery_code_hash("abcde-fghjk"),
            recovery_code_hash(" ABCDE FGHJK ")
        );
    }

    #[test]
    fn the_uri_names_the_issuer_and_the_account() {
        let secret = new_secret();
        let uri = uri(&secret, "a+b@example.com").unwrap();
        assert!(
            uri.starts_with("otpauth://totp/Potoo:a%2Bb%40example.com?"),
            "{uri}"
        );
        assert!(uri.contains(&format!("secret={secret}")), "{uri}");
        assert!(uri.contains("issuer=Potoo"), "{uri}");
    }
}
