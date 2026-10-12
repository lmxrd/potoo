use crate::db::Db;
use crate::filter::{FacetFilters, filter_sql};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::Serialize;
use utoipa::ToSchema;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;

/// The bucket a custom range's chart is aggregated over.
#[derive(Serialize, ToSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Hour,
    Day,
    Week,
    Month,
}

/// The window every time-filtered endpoint aggregates over: rows with
/// `created_at >= from` and `created_at <= to` — the `to` instant
/// included, exactly as the request named it. Durations and named ranges
/// are the frontend's concern: it turns them into this before the
/// request.
pub struct StatsRange {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

/// Parse the from/to of a stats or export request — the one shape every
/// endpoint that filters by time accepts: a pair of ISO 8601 timestamps,
/// with any offset, the `to` instant included, spanning at most 730 days
/// (two years).
pub fn parse_range(from: Option<&str>, to: Option<&str>) -> Result<StatsRange, String> {
    let (Some(from), Some(to)) = (from, to) else {
        return Err("from and to are both required, as ISO 8601 timestamps".into());
    };
    let (Ok(from), Ok(to)) = (
        DateTime::parse_from_rfc3339(from),
        DateTime::parse_from_rfc3339(to),
    ) else {
        return Err("from and to must be ISO 8601 timestamps".into());
    };
    let range = StatsRange {
        from: from.with_timezone(&Utc),
        to: to.with_timezone(&Utc),
    };
    if range.from >= range.to {
        return Err("from must be before to".into());
    }
    if (range.to - range.from).num_milliseconds() > 730 * DAY_MS {
        return Err("the range can span at most 730 days (two years)".into());
    }
    Ok(range)
}

/// The range as the `days` field of the stats response: the length of
/// the window in days, rounded up.
fn days_field(range: &StatsRange) -> i64 {
    ((range.to - range.from).num_milliseconds() + DAY_MS - 1) / DAY_MS
}

/// The buckets a range's chart is aggregated over.
struct Buckets {
    keys: Vec<String>,
    since: String,
    until: String,
    sql: &'static str,
    bucket: Option<Bucket>,
}

fn buckets(range: &StatsRange) -> Buckets {
    span_buckets(
        range.from.timestamp_millis(),
        range.to.timestamp_millis() + 1, // the `to` instant is included
    )
}

/// The window a range covers, as ISO timestamps: rows with
/// `created_at >= since` and `created_at < until`. Shared by the stats
/// aggregations and the raw export, so both always cover exactly the
/// same rows.
pub fn range_bounds(range: &StatsRange) -> (String, String) {
    let buckets = buckets(range);
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
            until: iso(end),
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
            until: iso(end),
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
            until: iso(end),
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
        until: iso(end),
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
    /// A lowercase ISO 3166-1 alpha-2 code (`no`, `us`), or `unknown` for hits
    /// whose country could not be resolved.
    pub country: String,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct DeviceRow {
    /// `desktop`, `mobile` or `tablet`, or `unknown` for hits with none.
    pub device: String,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct BrowserRow {
    /// A lowercase key: `chrome`, `safari`, `firefox`, `edge`, `opera` or
    /// `other`, or `unknown` for hits with none.
    pub browser: String,
    pub visitors: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct OsRow {
    /// A lowercase key: `windows`, `macos`, `ios`, `ipados`, `android`,
    /// `linux` or `other`, or `unknown` for hits with none.
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
    #[schema(nullable = false)]
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
    } = buckets(range);

    let in_sites = format!(
        "site_id IN ({})",
        site_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
    );
    // The filters apply to every aggregation, so a filtered view is
    // fully recomputed: the same facets, restricted to the selected values.
    let (filter_clause, filter_params) = filter_sql(filters);
    let within = if filter_clause.is_empty() {
        "created_at >= ? AND created_at < ?".to_string()
    } else {
        format!("created_at >= ? AND created_at < ? AND {filter_clause}")
    };
    let mut params: Vec<String> = Vec::new();
    params.extend(site_ids.iter().map(|id| id.to_string()));
    params.push(since.clone());
    params.push(until.clone());
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
            "COALESCE(country, 'unknown') AS country, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY country ORDER BY visitors DESC LIMIT 50")
        ),
        rows!(
            DeviceRow,
            "COALESCE(device, 'unknown') AS device, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY device ORDER BY visitors DESC")
        ),
        rows!(
            BrowserRow,
            "COALESCE(browser, 'unknown') AS browser, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY browser ORDER BY visitors DESC LIMIT 50")
        ),
        rows!(
            OsRow,
            "COALESCE(os, 'unknown') AS os, COUNT(DISTINCT visitor) AS visitors",
            format!("AND {pv} GROUP BY os ORDER BY visitors DESC LIMIT 50")
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
    fn parses_iso_windows() {
        // Both ends are required.
        assert!(parse_range(None, None).is_err());
        assert!(parse_range(Some("2026-10-09T00:00:00Z"), None).is_err());
        assert!(parse_range(None, Some("2026-10-10T00:00:00Z")).is_err());
        // They are ISO 8601 timestamps, with any offset.
        assert!(parse_range(Some("nonsense"), Some("2026-10-10T00:00:00Z")).is_err());
        assert!(parse_range(Some("2026-10-09T00:00:00Z"), Some("nonsense")).is_err());
        let range = parse_range(
            Some("2026-10-09T00:00:00.500Z"),
            Some("2026-10-10T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(
            range.from,
            Utc.with_ymd_and_hms(2026, 10, 9, 0, 0, 0).unwrap()
                + chrono::Duration::milliseconds(500)
        );
        assert_eq!(
            range.to,
            Utc.with_ymd_and_hms(2026, 10, 10, 0, 0, 0).unwrap()
        );
        let range = parse_range(
            Some("2026-10-09T02:00:00+02:00"),
            Some("2026-10-10T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(
            range.from,
            Utc.with_ymd_and_hms(2026, 10, 9, 0, 0, 0).unwrap()
        );
        // from must be before to, and the window spans at most 730 days
        // (two years) — exactly 730 is allowed.
        assert!(parse_range(Some("2026-10-10T00:00:00Z"), Some("2026-10-09T00:00:00Z")).is_err());
        assert!(parse_range(Some("2026-01-01T00:00:00Z"), Some("2029-01-01T00:00:00Z")).is_err());
        assert!(parse_range(Some("2026-01-01T00:00:00Z"), Some("2028-01-01T00:00:00Z")).is_ok());
    }

    #[test]
    fn days_field_rounds_up() {
        let from = Utc.with_ymd_and_hms(2026, 1, 1, 10, 30, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 1, 2, 12, 30, 0).unwrap();
        assert_eq!(days_field(&StatsRange { from, to }), 2); // 26 hours
        let from = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(days_field(&StatsRange { from, to }), 1); // 12 hours
    }

    #[test]
    fn buckets_follow_the_span() {
        let range = StatsRange {
            from: Utc.with_ymd_and_hms(2026, 1, 1, 10, 30, 0).unwrap(),
            to: Utc.with_ymd_and_hms(2026, 1, 2, 12, 30, 0).unwrap(),
        };
        let chart = buckets(&range);
        assert_eq!(chart.bucket, Some(Bucket::Hour));
        assert_eq!(chart.keys.first().unwrap(), "2026-01-01T10:00");
        assert_eq!(chart.keys.last().unwrap(), "2026-01-02T12:00");
        assert_eq!(chart.since, "2026-01-01T10:30:00.000Z");
        // The to instant is included: the bound is the moment after it.
        assert_eq!(chart.until, "2026-01-02T12:30:00.001Z");

        let range = StatsRange {
            from: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            to: Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap(),
        };
        let chart = buckets(&range);
        assert_eq!(chart.bucket, Some(Bucket::Day));
        assert_eq!(chart.keys.len(), 32);
        assert_eq!(chart.keys.first().unwrap(), "2026-01-01");
    }
}
