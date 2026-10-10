use crate::db::Db;
use crate::filter::{FacetFilters, filter_sql};
use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use rusqlite::params_from_iter;
use serde::Serialize;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
const MINUTE_MS: i64 = 60_000;

/// The buckets a custom range's chart can use.
pub type Bucket = &'static str;

/// A preset range of days, or an explicit UTC range with minute precision.
pub enum StatsRange {
    /// Today, as its 24 hours.
    Today,
    /// The last `n` days, calendar aligned.
    Days(u32),
    /// An explicit range; the `to` minute is included.
    FromTo {
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
}

/// Parse the range of a stats request: a `days` preset, or an explicit
/// `from`/`to` pair as YYYY-MM-DDTHH:MM in UTC.
pub fn parse_range(
    days: Option<&str>,
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
    match days {
        None => Ok(StatsRange::Days(7)),
        Some("today") => Ok(StatsRange::Today),
        Some(days) => {
            let n: u32 = days
                .parse()
                .map_err(|_| "days must be an integer from 1 to 365, or \"today\"".to_string())?;
            if !(1..=365).contains(&n) {
                return Err("days must be an integer from 1 to 365, or \"today\"".into());
            }
            Ok(StatsRange::Days(n))
        }
    }
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
fn days_field(range: &StatsRange) -> serde_json::Value {
    match range {
        StatsRange::Today => serde_json::json!("today"),
        StatsRange::Days(n) => serde_json::json!(n),
        StatsRange::FromTo { from, to } => {
            serde_json::json!(((*to - *from).num_milliseconds() + DAY_MS - 1) / DAY_MS)
        }
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
        StatsRange::Days(n) => preset_buckets(*n, now),
        StatsRange::Today => preset_buckets_today(now),
        StatsRange::FromTo { from, to } => custom_buckets(*from, *to),
    }
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

/// Today, as its 24 hours.
fn preset_buckets_today(now: DateTime<Utc>) -> Buckets {
    let day_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let start = day_start.timestamp_millis();
    Buckets {
        keys: (0..24).map(|h| hour(start + h * HOUR_MS)).collect(),
        since: format!("{}T00:00:00.000", date(start)),
        until: None,
        sql: HOUR_SQL,
        bucket: None,
    }
}

/// The last `n` days, calendar aligned: up to 30 days is one bucket per
/// day, up to 90 is one per week (Mondays), a year is one per month.
/// Days(1) is the last 24 started hours, ending with the current one.
fn preset_buckets(n: u32, now: DateTime<Utc>) -> Buckets {
    let day_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let start = day_start.timestamp_millis();
    if n == 1 {
        let hour_start = start + i64::from(now.hour()) * HOUR_MS;
        return Buckets {
            keys: (0..24)
                .map(|i| hour(hour_start - (23 - i) * HOUR_MS))
                .collect(),
            since: iso(hour_start - 23 * HOUR_MS),
            until: None,
            sql: HOUR_SQL,
            bucket: None,
        };
    }
    if n <= 30 {
        let days = i64::from(n);
        return Buckets {
            keys: (0..days)
                .map(|i| date(start - (days - 1 - i) * DAY_MS))
                .collect(),
            since: format!("{}T00:00:00.000", date(start - (days - 1) * DAY_MS)),
            until: None,
            sql: DAY_SQL,
            bucket: None,
        };
    }
    if n <= 90 {
        // Weeks start on Monday; 13 of them cover 90 days.
        let weekday = now.weekday().num_days_from_sunday(); // 0 is Sunday
        let this_monday = start - i64::from((weekday + 6) % 7) * DAY_MS;
        return Buckets {
            keys: (0..13)
                .map(|i| date(this_monday - (12 - i) * 7 * DAY_MS))
                .collect(),
            since: format!("{}T00:00:00.000", date(this_monday - 12 * 7 * DAY_MS)),
            until: None,
            sql: WEEK_SQL,
            bucket: None,
        };
    }
    // Months: the current one and the 11 before it.
    let month_index = now.year() * 12 + now.month() as i32 - 1; // 0-based
    let month_start = |i: i32| {
        let m = month_index - (11 - i);
        Utc.with_ymd_and_hms(m.div_euclid(12), (m.rem_euclid(12) + 1) as u32, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis()
    };
    Buckets {
        keys: (0..12).map(|i| date(month_start(i))).collect(),
        since: format!("{}T00:00:00.000", date(month_start(0))),
        until: None,
        sql: MONTH_SQL,
        bucket: None,
    }
}

/// The buckets of an explicit range. The bucket size follows the span:
/// hours for up to two days, days for up to 35, weeks (Mondays) for up to
/// 180, and months beyond. Buckets are calendar aligned, but the first
/// and last are clipped to the range itself.
fn custom_buckets(from: DateTime<Utc>, to: DateTime<Utc>) -> Buckets {
    let start = from.timestamp_millis();
    let end = to.timestamp_millis() + MINUTE_MS; // the `to` minute is included
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
            bucket: Some("hour"),
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
            bucket: Some("day"),
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
            bucket: Some("week"),
            keys: (0..=(last - first) / (7 * DAY_MS))
                .map(|i| date(first + i * 7 * DAY_MS))
                .collect(),
            since: iso(start),
            until: Some(iso(end)),
            sql: WEEK_SQL,
        };
    }
    let first_dt = from
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
        bucket: Some("month"),
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

/// One row of a breakdown.
#[derive(Serialize)]
struct Count {
    #[serde(rename = "pageviews")]
    pageviews: i64,
    #[serde(rename = "visitors")]
    visitors: i64,
}

#[derive(Serialize)]
pub struct DayRow {
    pub day: String,
    pub pageviews: i64,
    pub visitors: i64,
}

#[derive(Serialize)]
pub struct PathRow {
    pub path: String,
    pub pageviews: i64,
    pub visitors: i64,
}

#[derive(Serialize)]
pub struct CountryRow {
    pub country: String,
    pub visitors: i64,
}

#[derive(Serialize)]
pub struct DeviceRow {
    pub device: String,
    pub visitors: i64,
}

#[derive(Serialize)]
pub struct BrowserRow {
    pub browser: String,
    pub visitors: i64,
}

#[derive(Serialize)]
pub struct OsRow {
    pub os: String,
    pub visitors: i64,
}

#[derive(Serialize)]
pub struct ReferrerRow {
    pub referrer: String,
    pub pageviews: i64,
}

#[derive(Serialize)]
pub struct ClickRow {
    pub name: String,
    pub clicks: i64,
}

/// The stats of one dashboard: the totals, and the breakdowns the
/// dashboard shows, over a selected day range.
#[derive(Serialize)]
pub struct SiteStats {
    pub days: serde_json::Value,
    /// Set only for explicit from/to ranges: the bucket the chart shows.
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

/// Aggregate the hits of the given sites over a range. "Visitors" counts
/// distinct daily hashes, which include the site id, so the same person
/// on two of the sites counts as two visitors here.
pub async fn stats_for(
    db: &Db,
    site_ids: &[i64],
    range: &StatsRange,
    filters: &FacetFilters,
    now: DateTime<Utc>,
) -> rusqlite::Result<SiteStats> {
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

    fn query_rows<T>(
        conn: &rusqlite::Connection,
        in_sites: &str,
        within: &str,
        params: &[String],
        select: &str,
        tail: &str,
        map_row: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<Vec<T>> {
        let sql = format!("SELECT {select} FROM hits WHERE {in_sites} AND {within} {tail}");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params.iter()), map_row)?;
        rows.collect()
    }

    let conn = db.lock().await;
    let pv = "kind = 'pageview'";

    let totals = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
        &format!("AND {pv}"),
        |row| {
            Ok(Count {
                pageviews: row.get(0)?,
                visitors: row.get(1)?,
            })
        },
    )?
    .into_iter()
    .next()
    .unwrap_or(Count {
        pageviews: 0,
        visitors: 0,
    });

    let hit_days = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        &format!("{sql} AS day, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors"),
        &format!("AND {pv} GROUP BY day ORDER BY day"),
        |row| {
            Ok(DayRow {
                day: row.get(0)?,
                pageviews: row.get(1)?,
                visitors: row.get(2)?,
            })
        },
    )?;

    let paths = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "path, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
        &format!("AND {pv} GROUP BY path ORDER BY pageviews DESC LIMIT 50"),
        |row| {
            Ok(PathRow {
                path: row.get(0)?,
                pageviews: row.get(1)?,
                visitors: row.get(2)?,
            })
        },
    )?;

    let countries = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "COALESCE(country, '??') AS country, COUNT(DISTINCT visitor) AS visitors",
        &format!("AND {pv} GROUP BY country ORDER BY visitors DESC LIMIT 50"),
        |row| {
            Ok(CountryRow {
                country: row.get(0)?,
                visitors: row.get(1)?,
            })
        },
    )?;

    let devices = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "device, COUNT(DISTINCT visitor) AS visitors",
        &format!("AND {pv} GROUP BY device ORDER BY visitors DESC"),
        |row| {
            Ok(DeviceRow {
                device: row.get(0)?,
                visitors: row.get(1)?,
            })
        },
    )?;

    let browsers = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "browser, COUNT(DISTINCT visitor) AS visitors",
        &format!("AND {pv} GROUP BY browser ORDER BY visitors DESC"),
        |row| {
            Ok(BrowserRow {
                browser: row.get(0)?,
                visitors: row.get(1)?,
            })
        },
    )?;

    let os = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "os, COUNT(DISTINCT visitor) AS visitors",
        &format!("AND {pv} GROUP BY os ORDER BY visitors DESC"),
        |row| {
            Ok(OsRow {
                os: row.get(0)?,
                visitors: row.get(1)?,
            })
        },
    )?;

    let referrers = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "referrer, COUNT(*) AS pageviews",
        &format!(
            "AND {pv} AND referrer IS NOT NULL GROUP BY referrer ORDER BY pageviews DESC LIMIT 20"
        ),
        |row| {
            Ok(ReferrerRow {
                referrer: row.get(0)?,
                pageviews: row.get(1)?,
            })
        },
    )?;

    let clicks = query_rows(
        &conn,
        &in_sites,
        &within,
        &params,
        "name, COUNT(*) AS clicks",
        "AND kind = 'click' GROUP BY name ORDER BY clicks DESC LIMIT 50",
        |row| {
            Ok(ClickRow {
                name: row.get(0)?,
                clicks: row.get(1)?,
            })
        },
    )?;

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
