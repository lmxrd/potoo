//! A year of demo data, for development. Wipes every hit — not the
//! sites or users — and inserts pageviews, referrers, countries,
//! devices, browsers, operating systems and clicks over the past year,
//! for every site in the database; a demo site is created when there is
//! none. Deterministic: seeding twice gives the same data.
//!
//!     mise run seed

use chrono::{Datelike, TimeZone, Utc};
use potoo::db;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

const DAY_MS: i64 = 86_400_000;
const YEAR: i64 = 365;

const PATHS: &[(&str, u32)] = &[
    ("/", 30),
    ("/docs/quickstart", 15),
    ("/pricing", 15),
    ("/blog/self-hosting", 12),
    ("/blog/why-potoo", 10),
    ("/changelog", 10),
    ("/about", 8),
];
const REFERRERS: &[(&str, u32)] = &[
    ("news.ycombinator.com", 20),
    ("google.com", 35),
    ("duckduckgo.com", 15),
    ("reddit.com", 15),
    ("lobste.rs", 15),
];
const COUNTRIES: &[(&str, u32)] = &[
    ("no", 12),
    ("us", 28),
    ("de", 12),
    ("gb", 10),
    ("fr", 8),
    ("nl", 6),
    ("se", 6),
    ("in", 8),
    ("jp", 5),
    ("br", 5),
];
const BROWSERS: &[(&str, u32)] = &[("chrome", 64), ("safari", 20), ("firefox", 9), ("edge", 7)];
const OSES: &[(&str, u32)] = &[
    ("windows", 48),
    ("ios", 20),
    ("macos", 13),
    ("android", 13),
    ("linux", 6),
];
const DEVICES: &[(&str, u32)] = &[("desktop", 62), ("mobile", 33), ("tablet", 5)];
const CLICKS: &[(&str, u32)] = &[("signup", 30), ("docs", 30), ("demo", 25), ("download", 15)];

fn pick<'a>(rng: &mut StdRng, options: &[(&'a str, u32)]) -> &'a str {
    let total: u32 = options.iter().map(|option| option.1).sum();
    let mut roll = rng.random_range(0..total);
    for (value, weight) in options {
        if roll < *weight {
            return value;
        }
        roll -= *weight;
    }
    options.last().unwrap().0
}

/// One row of the hits table.
struct Hit {
    site_id: i64,
    kind: &'static str,
    path: &'static str,
    name: Option<&'static str>,
    referrer: Option<&'static str>,
    country: &'static str,
    device: &'static str,
    browser: &'static str,
    os: &'static str,
    visitor: String,
    created_at: String,
}

#[tokio::main]
async fn main() {
    // Deterministic: the same seed gives the same data.
    let mut rng = StdRng::seed_from_u64(42);

    let path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "data/potoo.db".into());
    let db = db::open(&path).await;
    let pool = db.writer();

    let mut sites: Vec<(i64, String)> = sqlx::query_as("SELECT id, hostname FROM sites")
        .fetch_all(pool)
        .await
        .unwrap();
    let inserted_demo_site = sites.is_empty();
    if inserted_demo_site {
        let id = sqlx::query("INSERT INTO sites (hostname) VALUES ('blog.example.com')")
            .execute(pool)
            .await
            .unwrap()
            .last_insert_rowid();
        sites = vec![(id, "blog.example.com".to_string())];
    }
    // The demo data replaces whatever hits are in the database.
    sqlx::query("DELETE FROM hits").execute(pool).await.unwrap();

    let now = Utc::now();
    let midnight = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .unwrap();
    let mut inserted = 0usize;
    for (site_id, _) in &sites {
        let base = rng.random_range(40..120);
        for i in 0..YEAR {
            let start = midnight.timestamp_millis() - (YEAR - 1 - i) * DAY_MS;
            let end = (start + DAY_MS).min(now.timestamp_millis());
            // Traffic grows over the year; today ends now, not in the
            // future.
            let trend = 0.55 + 0.9 * (i as f64) / (YEAR - 1) as f64;
            let hits = day_hits(&mut rng, *site_id, start, end, base, trend);
            inserted += hits.len();
            let mut tx = pool.begin().await.unwrap();
            for hit in &hits {
                sqlx::query(
                    "INSERT INTO hits
                       (site_id, kind, path, name, referrer, country, device, browser, os, visitor, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                )
                .bind(hit.site_id)
                .bind(hit.kind)
                .bind(hit.path)
                .bind(hit.name)
                .bind(hit.referrer)
                .bind(hit.country)
                .bind(hit.device)
                .bind(hit.browser)
                .bind(hit.os)
                .bind(&hit.visitor)
                .bind(&hit.created_at)
                .execute(&mut *tx)
                .await
                .unwrap();
            }
            tx.commit().await.unwrap();
        }
    }
    db.close().await;

    if inserted_demo_site {
        println!("Created the demo site blog.example.com.");
    }
    println!(
        "Seeded {inserted} hits over the past year into {}.",
        sites
            .iter()
            .map(|(_, host)| host.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// One site's pageviews for one day: a few visitors, one or more each.
fn day_hits(
    rng: &mut StdRng,
    site_id: i64,
    start: i64,
    end: i64,
    base: i64,
    trend: f64,
) -> Vec<Hit> {
    let weekday = Utc.timestamp_millis_opt(start).unwrap().weekday();
    let weekend = if weekday.num_days_from_sunday().is_multiple_of(6) {
        0.55
    } else {
        1.0
    };
    let volume = (base as f64 * weekend * trend * rng.random_range(0.8..1.2)).round() as i64;
    let at = |rng: &mut StdRng| {
        let millis = rng.random_range(start..end);
        Utc.timestamp_millis_opt(millis)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };

    let mut hits = Vec::new();
    for _ in 0..(volume as f64 * 0.6) as i64 {
        // The same visitor hash ties one person's views of one day
        // together.
        let visitor = format!("{:032x}", rng.random::<u128>());
        let device = pick(rng, DEVICES);
        let browser = pick(rng, BROWSERS);
        let os = pick(rng, OSES);
        let country = pick(rng, COUNTRIES);
        let views = rng.random_range(1..=3);
        for i in 0..views {
            hits.push(Hit {
                site_id,
                kind: "pageview",
                path: pick(rng, PATHS),
                name: None,
                referrer: if i == 0 && rng.random::<f64>() < 0.35 {
                    Some(pick(rng, REFERRERS))
                } else {
                    None
                },
                country,
                device,
                browser,
                os,
                visitor: visitor.clone(),
                created_at: at(rng),
            });
        }
        // Some visitors click a tracked element.
        if rng.random::<f64>() < 0.15 {
            hits.push(Hit {
                site_id,
                kind: "click",
                path: "/",
                name: Some(pick(rng, CLICKS)),
                referrer: None,
                country,
                device,
                browser,
                os,
                visitor: visitor.clone(),
                created_at: at(rng),
            });
        }
    }
    hits
}
