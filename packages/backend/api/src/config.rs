//! Settings read from the environment.

/// The value with surrounding whitespace removed; `None` while it is empty.
pub fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// An environment variable; `None` while it is unset or blank.
pub fn var(name: &str) -> Option<String> {
    non_empty(std::env::var(name).ok())
}

/// Where the app is reached from (`PUBLIC_URL`), without a trailing slash.
pub fn public_url() -> Option<String> {
    var("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string())
}

/// The database's file, from `DATABASE_URL` (default `sqlite:data/potoo.db`).
/// A URL rather than a path, so another database could take its place
/// later; for now it must be SQLite (`sqlite:path`, `sqlite://path`, with
/// sqlx's options such as `?mode=rwc` allowed).
pub fn database_path() -> Result<String, String> {
    parse_database_url(&var("DATABASE_URL").unwrap_or_else(|| "sqlite:data/potoo.db".into()))
}

/// [`database_path`] for a value already read.
pub fn parse_database_url(url: &str) -> Result<String, String> {
    use std::str::FromStr;
    if !url.starts_with("sqlite:") {
        return Err(format!(
            "DATABASE_URL must be a SQLite URL (sqlite:path), not `{url}`"
        ));
    }
    let options = sqlx::sqlite::SqliteConnectOptions::from_str(url)
        .map_err(|e| format!("DATABASE_URL is not a valid SQLite URL: {e}"))?;
    Ok(options.get_filename().to_string_lossy().into_owned())
}

/// A yes/no setting: `1`/`0`, `true`/`false`, `yes`/`no` or `on`/`off` (any case); `None` while unset or
/// blank. Anything else is an error, so a typo stops the server instead of silently meaning "off".
pub fn flag(name: &str) -> Result<Option<bool>, String> {
    parse_flag(name, &std::env::var(name).unwrap_or_default())
}

/// [`flag`] for a value already read.
pub fn parse_flag(name: &str, value: &str) -> Result<Option<bool>, String> {
    let value = value.trim();
    match value.to_lowercase().as_str() {
        "" => Ok(None),
        "1" | "true" | "yes" | "on" => Ok(Some(true)),
        "0" | "false" | "no" | "off" => Ok(Some(false)),
        _ => Err(format!(
            "{name} must be true or false (1/0, yes/no, on/off), not `{value}`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_url_names_a_sqlite_file() {
        assert_eq!(
            parse_database_url("sqlite:data/potoo.db").unwrap(),
            "data/potoo.db"
        );
        assert_eq!(
            parse_database_url("sqlite:///data/potoo.db").unwrap(),
            "/data/potoo.db"
        );
        assert_eq!(
            parse_database_url("sqlite:/data/potoo.db?mode=rwc").unwrap(),
            "/data/potoo.db"
        );
        assert!(parse_database_url("postgres://db/potoo").is_err());
        assert!(parse_database_url("data/potoo.db").is_err());
    }

    #[test]
    fn flags_take_the_usual_spellings_and_refuse_the_rest() {
        for yes in ["1", "true", "TRUE", "yes", "on", " on "] {
            assert_eq!(parse_flag("X", yes), Ok(Some(true)), "{yes}");
        }
        for no in ["0", "false", "no", "off"] {
            assert_eq!(parse_flag("X", no), Ok(Some(false)), "{no}");
        }
        assert_eq!(parse_flag("X", ""), Ok(None));
        assert!(parse_flag("X", "maybe").unwrap_err().contains("X must be"));
    }
}
