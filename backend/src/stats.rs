use crate::db::Db;
use crate::filter::{FacetFilters, filter_sql};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::Serialize;
use utoipa::ToSchema;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
const MINUTE_MS: i64 = 60_000;

/// The bucket a custom range's chart is aggregated over.
#[derive(Serialize, ToSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Hour,
    Day,
    Week,
    Month,
}

/// A rolling window ending now, or an explicit UTC range with minute
/// precision — the two forms every time-filtered endpoint accepts.
pub enum StatsRange {
    /// The window's length in milliseconds; it ends now.
    Rolling { millis: i64 },
    /// An explicit range; the `to` minute is included.
    FromTo {
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
}

/// Parse the range of a stats or export request — the one shape every
/// endpoint that filters by time accepts:
///
/// - `range`: an ISO 8601 duration like `P7D`, `PT12H` or `P1Y` — a
///   rolling window of exactly that length, ending now;
/// - or `from`/`to`: an explicit pair of ISO timestamps, YYYY-MM-DDTHH:MM
///   in UTC, with the `to` minute included. The dashboard's "Today" view
///   is this form: UTC midnight to now.
pub fn parse_range(
    range: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<StatsRange, String> {
    if from.is_some() || to.is_some() {
        let (Some(from), Some(to)) = (from, to) else {
            return Err("from and to are both required, as YYYY-MM-DDTHH:MM in UTC".into());
        };
        let (Some(start), Some(end)) = (parse_minute(from), parse_minute(to)) else {
            return Err("from and to must be YYYY-MM-DDTHH:MM in UTC".into());
        };
        if start >= end {
            return Err("from must be before to".into());
        }
        if end - start > 730 * DAY_MS {
            return Err("the range can span at most 730 days".into());
        }
        return Ok(StatsRange::FromTo {
            from: Utc.timestamp_millis_opt(start).unwrap(),
            to: Utc.timestamp_millis_opt(end).unwrap(),
        });
    }
    match range {
        None => Ok(StatsRange::Rolling { millis: 7 * DAY_MS }),
        Some(range) => {
            let Some(millis) = parse_duration(range) else {
                return Err("range must be a duration like P7D or PT24H".into());
            };
            if millis > 365 * DAY_MS {
                return Err("range can span at most P1Y (365 days)".into());
            }
            Ok(StatsRange::Rolling { millis })
        }
    }
}

/// An ISO 8601 duration like P1Y, P3M, P2W, P7D or PT24H, as
/// milliseconds. Years count as 365 and months as 30 days, which is
/// exactly how the day presets behave.
fn parse_duration(raw: &str) -> Option<i64> {
    let body = raw.strip_prefix('P')?;
    let (date, time) = body
        .split_once('T')
        .map(|(date, time)| (date, Some(time)))
        .unwrap_or((body, None));
    let mut total = 0i64;
    let mut scan = |part: &str, date_units: bool| -> Option<()> {
        let mut number: i64 = 0;
        let mut digits = 0;
        for c in part.chars() {
            match c.to_digit(10) {
                Some(d) => {
                    number = number * 10 + i64::from(d);
                    digits += 1;
                }
                None => {
                    if digits == 0 {
                        return None;
                    }
                    total += match (c, date_units) {
                        ('Y', true) => number * 365 * DAY_MS,
                        ('M', true) => number * 30 * DAY_MS,
                        ('W', true) => number * 7 * DAY_MS,
                        ('D', true) => number * DAY_MS,
                        ('H', false) => number * HOUR_MS,
                        ('M', false) => number * MINUTE_MS,
                        ('S', false) => number * 1_000,
                        _ => return None,
                    };
                    number = 0;
                    digits = 0;
                }
            }
        }
        (digits == 0).then_some(())
    };
    scan(date, true)?;
    if let Some(time) = time {
        scan(time, false)?;
    }
    (total > 0).then_some(total)
}

/// A strict YYYY-MM-DDTHH:MM, as milliseconds since the epoch.
fn parse_minute(raw: &str) -> Option<i64> {
    let bytes = raw.as_bytes();
    if bytes.len() != 16
        || bytes[10] != b'T'
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[13] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || i == 10 || i == 13 || b.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = raw[0..4].parse().ok()?;
    let month: u32 = raw[5..7].parse().ok()?;
    let day: u32 = raw[8..10].parse().ok()?;
    let hour: u32 = raw[11..13].parse().ok()?;
    let minute: u32 = raw[14..16].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .map(|dt| dt.timestamp_millis())
}

/// The range as the `days` field of the stats response.
fn days_field(range: &StatsRange) -> i64 {
    match range {
        StatsRange::Rolling { millis } => (*millis + DAY_MS - 1) / DAY_MS,
        StatsRange::FromTo { from, to } => ((*to - *from).num_milliseconds() + DAY_MS - 1) / DAY_MS,
    }
}

/// The buckets a range's chart is aggregated over.
struct Buckets {
    keys: Vec<String>,
    since: String,
    until: Option<String>,
    sql: &'static str,
    bucket: Option<Bucket>,
}

fn buckets(range: &StatsRange, now: DateTime<Utc>) -> Buckets {
    match range {
        StatsRange::Rolling { millis } => {
            let end = now.timestamp_millis();
            let mut buckets = span_buckets(end - millis, end);
            // The window ends now, so it has no fixed upper bound.
            buckets.until = None;
            buckets
        }
        StatsRange::FromTo { from, to } => span_buckets(
            from.timestamp_millis(),
            to.timestamp_millis() + MINUTE_MS, // the `to` minute is included
        ),
    }
}

/// The window a range covers, as ISO timestamps: rows with
/// `created_at >= since` and, when the range ends before now,
/// `created_at < until`. Shared by the stats aggregations and the raw
/// export, so both always cover exactly the same rows.
pub fn range_bounds(range: &StatsRange, now: DateTime<Utc>) -> (String, Option<String>) {
    let buckets = buckets(range, now);
    (buckets.since, buckets.until)
}

fn iso(millis: i64) -> String {
    Utc.timestamp_millis_opt(millis)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn date(millis: i64) -> String {
    iso(millis)[..10].to_string()
}

/// `YYYY-MM-DDTHH:00`, the hour label the chart uses.
fn hour(millis: i64) -> String {
    format!("{}:00", &iso(millis)[..13])
}

const HOUR_SQL: &str = "substr(created_at, 1, 13) || ':00'";
const DAY_SQL: &str = "date(created_at)";
const WEEK_SQL: &str =
    "date(created_at, '-' || ((cast(strftime('%w', created_at) as integer) + 6) % 7) || ' days')";
const MONTH_SQL: &str = "substr(created_at, 1, 7) || '-01'";

/// The buckets of a window, exclusive at `end`. The bucket size follows
/// the span: hours for up to two days, days for up to 35, weeks
/// (Mondays) for up to 180, and months beyond. Buckets are calendar
/// aligned, but the first and last are clipped to the window itself.
fn span_buckets(start: i64, end: i64) -> Buckets {
    let span = end - start;
    let midnight = |millis: i64| {
        let dt = Utc.timestamp_millis_opt(millis).unwrap();
        dt.date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    };

    if span <= 48 * HOUR_MS {
        let first = start - start % HOUR_MS;
        let last = (end - 1) - (end - 1) % HOUR_MS;
        return Buckets {
            bucket: Some(Bucket::Hour),
            keys: (0..=(last - first) / HOUR_MS)
                .map(|i| hour(first + i * HOUR_MS))
                .collect(),
            since: iso(start),
            until: Some(iso(end)),
            sql: HOUR_SQL,
        };
    }
    if span <= 35 * DAY_MS {
        let first = midnight(start);
        let last = midnight(end - 1);
        return Buckets {
            bucket: Some(Bucket::Day),
            keys: (0..=(last - first) / DAY_MS)
                .map(|i| date(first + i * DAY_MS))
                .collect(),
            since: iso(start),
            until: Some(iso(end)),
            sql: DAY_SQL,
        };
    }
    if span <= 180 * DAY_MS {
        let monday = |millis: i64| {
            let dt = Utc.timestamp_millis_opt(millis).unwrap();
            let weekday = dt.weekday().num_days_from_sunday();
            millis - i64::from((weekday + 6) % 7) * DAY_MS
        };
        let first = monday(midnight(start));
        let last = monday(midnight(end - 1));
        return Buckets {
            bucket: Some(Bucket::Week),
            keys: (0..=(last - first) / (7 * DAY_MS))
                .map(|i| date(first + i * 7 * DAY_MS))
                .collect(),
            since: iso(start),
            until: Some(iso(end)),
            sql: WEEK_SQL,
        };
    }
    let first_dt = Utc
        .timestamp_millis_opt(start)
        .unwrap()
        .date_naive()
        .with_day(1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let last = Utc.timestamp_millis_opt(end - 1).unwrap();
    let months =
        (last.year() - first_dt.year()) * 12 + last.month() as i32 - first_dt.month() as i32 + 1;
    let first_month = first_dt.year() * 12 + first_dt.month() as i32 - 1;
    Buckets {
        bucket: Some(Bucket::Month),
        keys: (0..months)
            .map(|i| {
                let m = first_month + i;
                date(
                    Utc.with_ymd_and_hms(
                        m.div_euclid(12),
                        (m.rem_euclid(12) + 1) as u32,
                        1,
                        0,
                        0,
                        0,
                    )
                    .unwrap()
                    .timestamp_millis(),
                )
            })
            .collect(),
        since: iso(start),
        until: Some(iso(end)),
        sql: MONTH_SQL,
    }
}

/// The totals of one aggregation.
#[derive(sqlx::FromRow)]
struct Count {
    pageviews: i64,
    visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct DayRow {
    pub day: String,
    pub pageviews: i64,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct PathRow {
    pub path: String,
    pub pageviews: i64,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct CountryRow {
    pub country: String,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct DeviceRow {
    pub device: String,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct BrowserRow {
    pub browser: String,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct OsRow {
    pub os: String,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct ReferrerRow {
    pub referrer: String,
    pub pageviews: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct ClickRow {
    pub name: String,
    pub clicks: i64,
}

/// The stats of one dashboard: the totals, and the breakdowns the
/// dashboard shows, over a selected day range.
#[derive(Serialize, ToSchema)]
pub struct SiteStats {
    /// The length of the range in days (rounded up).
    pub days: i64,
    /// The bucket the chart's byDay rows are aggregated over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<Bucket>,
    pub pageviews: i64,
    pub visitors: i64,
    #[serde(rename = "byDay")]
    pub by_day: Vec<DayRow>,
    pub paths: Vec<PathRow>,
    pub countries: Vec<CountryRow>,
    pub devices: Vec<DeviceRow>,
    pub browsers: Vec<BrowserRow>,
    pub os: Vec<OsRow>,
    pub referrers: Vec<ReferrerRow>,
    pub clicks: Vec<ClickRow>,
}

/// One aggregation over the selected hits, mapped onto a row type by its
/// column names. The SQL is assembled from fixed fragments and `?`
/// placeholders only; every value is bound.
async fn query_rows<T>(
    db: &Db,
    in_sites: &str,
    within: &str,
    params: &[String],
    select: impl Into<String>,
    tail: String,
) -> sqlx::Result<Vec<T>>
where
    T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin,
{
    let select = select.into();
    let sql = format!("SELECT {select} FROM hits WHERE {in_sites} AND {within} {tail}");
    let mut query = sqlx::query_as(sqlx::AssertSqlSafe(sql));
    for param in params {
        query = query.bind(param.as_str());
    }
    query.fetch_all(db.reader()).await
}

/// Aggregate the hits of the given sites over a range. "Visitors" counts
/// distinct daily hashes, which include the site id, so the same person
/// on two of the sites counts as two visitors here.
pub async fn stats_for(
    db: &Db,
    site_ids: &[i64],
    range: &StatsRange,
    filters: &FacetFilters,
    now: DateTime<Utc>,
) -> sqlx::Result<SiteStats> {
    let days = days_field(range);
    if site_ids.is_empty() {
        return Ok(SiteStats {
            days,
            bucket: None,
            pageviews: 0,
            visitors: 0,
            by_day: Vec::new(),
            paths: Vec::new(),
            countries: Vec::new(),
            devices: Vec::new(),
            browsers: Vec::new(),
            os: Vec::new(),
            referrers: Vec::new(),
            clicks: Vec::new(),
        });
    }

    let Buckets {
        keys,
        since,
        until,
        sql,
        bucket,
    } = buckets(range, now);

    let in_sites = format!(
        "site_id IN ({})",
        site_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
    );
    // The filters apply to every aggregation, so a filtered view is
    // fully recomputed: the same facets, restricted to the selected values.
    let (filter_clause, filter_params) = filter_sql(filters);
    let within = match until {
        Some(_) => "created_at >= ? AND created_at < ?",
        None => "created_at >= ?",
    };
    let within = if filter_clause.is_empty() {
        within.to_string()
    } else {
        format!("{within} AND {filter_clause}")
    };
    let mut params: Vec<String> = Vec::new();
    params.extend(site_ids.iter().map(|id| id.to_string()));
    params.push(since.clone());
    if let Some(until) = &until {
        params.push(until.clone());
    }
    params.extend(filter_params);

    let pv = "kind = 'pageview'";
    // A macro, not a closure: each use maps onto a different row type.
    macro_rules! rows {
        ($row:ty, $select:expr, $tail:expr) => {
            query_rows::<$row>(db, &in_sites, &within, &params, $select, $tail)
        };
    }

    // The aggregations are independent reads: run them side by side on the
    // reader connections.
    let (totals, hit_days, paths, countries, devices, browsers, os, referrers, clicks) = tokio::try_join!(
        rows!(
            Count,
            "COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv}")
        ),
        rows!(
            DayRow,
            format!("{sql} AS day, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors"),
            format!("AND {pv} GROUP BY day ORDER BY day")
        ),
        rows!(
            PathRow,
            "path, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY path ORDER BY pageviews DESC LIMIT 50")
        ),
        rows!(
            CountryRow,
            "COALESCE(country, '??') AS country, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY country ORDER BY visitors DESC LIMIT 50")
        ),
        rows!(
            DeviceRow,
            "device, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY device ORDER BY visitors DESC")
        ),
        rows!(
            BrowserRow,
            "browser, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY browser ORDER BY visitors DESC")
        ),
        rows!(
            OsRow,
            "os, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY os ORDER BY visitors DESC")
        ),
        rows!(
            ReferrerRow,
            "referrer, COUNT(*) AS pageviews",
            format!(
                "AND {pv} AND referrer IS NOT NULL GROUP BY referrer ORDER BY pageviews DESC LIMIT 20"
            )
        ),
        rows!(
            ClickRow,
            "name, COUNT(*) AS clicks",
            "AND kind = 'click' GROUP BY name ORDER BY clicks DESC LIMIT 50".to_string()
        ),
    )?;
    let totals = totals.into_iter().next().unwrap_or(Count {
        pageviews: 0,
        visitors: 0,
    });

    // Zero fill the periods without hits, so the chart shows every bucket.
    let hits: std::collections::HashMap<String, DayRow> =
        hit_days.into_iter().map(|d| (d.day.clone(), d)).collect();
    let by_day = keys
        .into_iter()
        .map(|day| match hits.get(&day) {
            Some(h) => DayRow {
                day,
                pageviews: h.pageviews,
                visitors: h.visitors,
            },
            None => DayRow {
                day,
                pageviews: 0,
                visitors: 0,
            },
        })
        .collect();

    Ok(SiteStats {
        days,
        bucket,
        pageviews: totals.pageviews,
        visitors: totals.visitors,
        by_day,
        paths,
        countries,
        devices,
        browsers,
        os,
        referrers,
        clicks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parses_presets_and_ranges() {
        // The default is the rolling last 7 days.
        assert!(matches!(
            parse_range(None, None, None),
            Ok(StatsRange::Rolling { millis }) if millis == 7 * DAY_MS
        ));
        // "Today" is the dashboard's label for a midnight-to-now window,
        // sent as from/to; the backend has no named range.
        assert!(parse_range(Some("today"), None, None).is_err());
        // Durations are rolling windows of exactly their length.
        assert!(matches!(
            parse_range(Some("PT12H"), None, None),
            Ok(StatsRange::Rolling { millis }) if millis == 12 * HOUR_MS
        ));
        assert!(matches!(
            parse_range(Some("PT24H"), None, None),
            Ok(StatsRange::Rolling { millis }) if millis == DAY_MS
        ));
        assert!(matches!(
            parse_range(Some("P30D"), None, None),
            Ok(StatsRange::Rolling { millis }) if millis == 30 * DAY_MS
        ));
        assert!(matches!(
            parse_range(Some("P2W"), None, None),
            Ok(StatsRange::Rolling { millis }) if millis == 14 * DAY_MS
        ));
        assert!(matches!(
            parse_range(Some("P1Y"), None, None),
            Ok(StatsRange::Rolling { millis }) if millis == 365 * DAY_MS
        ));
        assert!(parse_range(Some("P400D"), None, None).is_err());
        assert!(parse_range(Some("7"), None, None).is_err());
        assert!(parse_range(Some("P"), None, None).is_err());
        assert!(parse_range(Some("P7X"), None, None).is_err());
        assert!(parse_range(Some("P7"), None, None).is_err());
        // Explicit ranges: both ends, minute precision, ordered.
        assert!(parse_range(None, Some("2026-10-09T00:00"), None).is_err());
        assert!(parse_range(None, Some("2026-10-09T00:00"), Some("2026-10-09T09:99")).is_err());
        assert!(matches!(
            parse_range(None, Some("2026-10-09T00:00"), Some("2026-10-10T00:00")),
            Ok(StatsRange::FromTo { .. })
        ));
        assert!(parse_range(None, Some("2026-10-10T00:00"), Some("2026-10-09T00:00")).is_err());
    }

    #[test]
    fn rolling_windows_are_exact() {
        // A fixed moment: 2026-10-09 is a Friday.
        let now = Utc.with_ymd_and_hms(2026, 10, 9, 14, 30, 0).unwrap();
        // PT12H: twelve hours back, hourly chart.
        let chart = buckets(
            &StatsRange::Rolling {
                millis: 12 * HOUR_MS,
            },
            now,
        );
        assert_eq!(chart.bucket, Some(Bucket::Hour));
        assert_eq!(chart.since, "2026-10-09T02:30:00.000Z");
        assert_eq!(chart.until, None);
        assert_eq!(chart.keys.first().unwrap(), "2026-10-09T02:00");
        assert_eq!(chart.keys.last().unwrap(), "2026-10-09T14:00");
        assert_eq!(chart.keys.len(), 13);
        // P7D: seven days back, daily buckets, the ends partial.
        let chart = buckets(&StatsRange::Rolling { millis: 7 * DAY_MS }, now);
        assert_eq!(chart.bucket, Some(Bucket::Day));
        assert_eq!(chart.since, "2026-10-02T14:30:00.000Z");
        assert_eq!(chart.until, None);
        assert_eq!(chart.keys.first().unwrap(), "2026-10-02");
        assert_eq!(chart.keys.last().unwrap(), "2026-10-09");
        assert_eq!(chart.keys.len(), 8);
        // The `days` field is the window's length in days, rounded up.
        assert_eq!(
            days_field(&StatsRange::Rolling {
                millis: 12 * HOUR_MS
            }),
            1
        );
        assert_eq!(days_field(&StatsRange::Rolling { millis: 7 * DAY_MS }), 7);
    }

    #[test]
    fn fromto_buckets_follow_the_span() {
        let from = Utc.with_ymd_and_hms(2026, 1, 1, 10, 30, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 1, 2, 12, 30, 0).unwrap();
        let chart = buckets(&StatsRange::FromTo { from, to }, from);
        assert_eq!(chart.bucket, Some(Bucket::Hour));
        assert_eq!(chart.keys.first().unwrap(), "2026-01-01T10:00");
        assert_eq!(chart.keys.last().unwrap(), "2026-01-02T12:00");
        assert_eq!(chart.since, "2026-01-01T10:30:00.000Z");
        assert_eq!(chart.until.as_deref(), Some("2026-01-02T12:31:00.000Z"));

        let from = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
        let chart = buckets(&StatsRange::FromTo { from, to }, from);
        assert_eq!(chart.bucket, Some(Bucket::Day));
        assert_eq!(chart.keys.len(), 32);
        assert_eq!(chart.keys.first().unwrap(), "2026-01-01");
    }
}
