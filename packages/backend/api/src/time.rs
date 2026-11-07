//! Dates and times, with jiff. Every stored time is UTC text in one shape,
//! `YYYY-MM-DDTHH:MM:SS.mmmZ` ([`iso`]), so stored times sort and compare
//! as text too.

use jiff::{SignedDuration, Timestamp};

/// The current moment.
pub fn now() -> Timestamp {
    Timestamp::now()
}

/// Seconds since the Unix epoch, now.
pub fn unix_now() -> u64 {
    now().as_second().max(0) as u64
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ`, the form times are stored in.
pub fn iso(t: Timestamp) -> String {
    format!("{t:.3}")
}

/// The current moment, as stored.
pub fn now_iso() -> String {
    iso(now())
}

/// The moment `days` days from now (before now when negative), as stored.
pub fn days_from_now_iso(days: i64) -> String {
    iso(now() + SignedDuration::from_hours(days * 24))
}

/// The moment `seconds` seconds from now (before now when negative), as stored.
pub fn seconds_from_now_iso(seconds: i64) -> String {
    iso(now() + SignedDuration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_times_have_one_shape() {
        let t: Timestamp = "2026-11-06T08:30:00Z".parse().unwrap();
        assert_eq!(iso(t), "2026-11-06T08:30:00.000Z");
        assert_eq!(now_iso().len(), 24);
        assert!(days_from_now_iso(-1) < now_iso());
        assert!(seconds_from_now_iso(60) > now_iso());
    }
}
