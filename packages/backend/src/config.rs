//! Settings read from the environment (the same helpers as woodmouse's).

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

/// Where Potoo is reached from (`PUBLIC_URL`), without a trailing slash.
pub fn public_url() -> Option<String> {
    var("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string())
}

/// A yes/no setting: `1`/`0`, `true`/`false`, `yes`/`no` or `on`/`off` (any
/// case); `None` while unset or blank. Anything else is an error, so a typo
/// stops the server instead of silently meaning "off".
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
