use chrono::{DateTime, Datelike, Utc};
use flate2::read::GzDecoder;
use maxminddb::Reader;
use std::io::Read;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Refresh the GeoIP database weekly, and download one when the file is
/// older than that (a missing file counts as stale).
pub const REFRESH_PERIOD: Duration = Duration::from_hours(24 * 7);

/// Country lookup from a local MaxMind-format database, downloaded from
/// DB-IP when missing or stale. Lookups themselves are always local.
pub struct Geo {
    path: PathBuf,
    source: Source,
    reader: RwLock<Option<Arc<Reader<Vec<u8>>>>>,
}

/// Where the database is downloaded from.
pub enum Source {
    /// Nowhere: only the file on disk is used.
    Disabled,
    /// DB-IP's free country database. Its file names carry the month, so
    /// the URL is worked out at every download, not once at startup: a
    /// server that runs for months must ask for the month it is in, and
    /// not for the one it started in.
    DbIp,
    /// A fixed URL, e.g. a mirror.
    Url(String),
}

impl Source {
    /// The URLs to try, in order, at the given moment.
    fn urls(&self, now: DateTime<Utc>) -> Vec<String> {
        match self {
            Source::Disabled => Vec::new(),
            Source::Url(url) => vec![url.clone()],
            // This month's build, and last month's for the first days of a
            // month, before the new one is published.
            Source::DbIp => {
                let last_month = now.with_day(1).unwrap_or(now) - chrono::Duration::days(1);
                [now, last_month].map(dbip_url).to_vec()
            }
        }
    }
}

/// The DB-IP country lite database of the month of a date.
fn dbip_url(date: DateTime<Utc>) -> String {
    format!(
        "https://download.db-ip.com/free/dbip-country-lite-{}.mmdb.gz",
        date.format("%Y-%m")
    )
}

impl Geo {
    pub async fn open(path: &str, source: Source) -> Geo {
        let geo = Geo {
            path: PathBuf::from(path),
            source,
            reader: RwLock::new(None),
        };
        let loaded = geo.load_from_disk().await;
        if !loaded || geo.is_stale() {
            let fresh = geo.refresh().await;
            if !fresh && !loaded {
                eprintln!(
                    "GeoIP database not found at {}; country will be empty",
                    geo.path.display()
                );
            }
        }
        geo
    }

    /// The country code of an IP, or None when it cannot be resolved.
    pub async fn lookup(&self, ip: &str) -> Option<String> {
        let addr: IpAddr = ip.parse().ok()?;
        let reader = self.reader.read().await.clone()?;
        let result = reader.lookup(addr).ok()?;
        if !result.has_data() {
            return None;
        }
        let country: maxminddb::geoip2::Country = result.decode().ok()??;
        country.country.iso_code.map(|code| code.to_string())
    }

    async fn load_from_disk(&self) -> bool {
        let Ok(bytes) = std::fs::read(&self.path) else {
            return false;
        };
        match Reader::from_source(bytes) {
            Ok(reader) => {
                *self.reader.write().await = Some(Arc::new(reader));
                true
            }
            Err(_) => false,
        }
    }

    fn is_stale(&self) -> bool {
        std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|age| age > REFRESH_PERIOD).unwrap_or(false))
            .unwrap_or(true)
    }

    /// Download, decompress and install a fresh database, from the first
    /// of the source's URLs that works. Returns success; the old database
    /// is only replaced once the new one actually opens.
    pub async fn refresh(&self) -> bool {
        self.refresh_from(self.source.urls(Utc::now())).await
    }

    async fn refresh_from(&self, urls: Vec<String>) -> bool {
        let mut last_error = None;
        for url in urls {
            match download(&url).await {
                Ok(bytes) => match Reader::from_source(bytes.clone()) {
                    Ok(reader) => {
                        if let Some(dir) = self.path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        if std::fs::write(&self.path, &bytes).is_err() {
                            return false;
                        }
                        *self.reader.write().await = Some(Arc::new(reader));
                        return true;
                    }
                    Err(e) => last_error = Some(format!("{url}: {e}")),
                },
                Err(e) => last_error = Some(format!("{url}: {e}")),
            }
        }
        if let Some(error) = last_error {
            eprintln!("GeoIP database download failed: {error}");
        }
        false
    }
}

async fn download(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    let gz = res.bytes().await.map_err(|e| e.to_string())?.to_vec();
    let mut out = Vec::new();
    GzDecoder::new(&gz[..])
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).unwrap()
    }

    #[test]
    fn dbip_asks_for_the_month_it_is_in_then_the_one_before() {
        let urls = Source::DbIp.urls(at(2026, 11, 3));
        assert_eq!(
            urls,
            [
                "https://download.db-ip.com/free/dbip-country-lite-2026-11.mmdb.gz",
                "https://download.db-ip.com/free/dbip-country-lite-2026-10.mmdb.gz",
            ]
        );
        // The month is the one of the moment of the download: the same
        // source gives another URL a month later.
        assert!(Source::DbIp.urls(at(2026, 12, 1))[0].ends_with("2026-12.mmdb.gz"));
    }

    #[test]
    fn the_month_before_january_is_december_of_the_year_before() {
        let urls = Source::DbIp.urls(at(2027, 1, 31));
        assert!(urls[0].ends_with("2027-01.mmdb.gz"));
        assert!(urls[1].ends_with("2026-12.mmdb.gz"));
        // The end of a long month does not skip a short one.
        assert!(Source::DbIp.urls(at(2026, 3, 31))[1].ends_with("2026-02.mmdb.gz"));
    }

    #[test]
    fn a_fixed_url_is_used_as_it_is_and_disabled_has_none() {
        let url = Source::Url("https://mirror.example/country.mmdb.gz".into());
        assert_eq!(
            url.urls(at(2026, 5, 5)),
            ["https://mirror.example/country.mmdb.gz"]
        );
        assert!(Source::Disabled.urls(at(2026, 5, 5)).is_empty());
    }

    #[tokio::test]
    async fn a_refresh_tries_the_next_url_when_one_fails() {
        use axum::{Router, http::StatusCode, routing::get};
        use std::sync::atomic::{AtomicUsize, Ordering};

        crate::install_tls_provider();
        let hits = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
        let (first, second) = (hits.clone(), hits.clone());
        let app = Router::new()
            .route(
                "/this-month.gz",
                get(move || async move {
                    first[0].fetch_add(1, Ordering::Relaxed);
                    StatusCode::NOT_FOUND // not published yet
                }),
            )
            .route(
                "/last-month.gz",
                get(move || async move {
                    second[1].fetch_add(1, Ordering::Relaxed);
                    "not a gzip file" // downloaded, but no database
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let geo = Geo::open("/nonexistent/country.mmdb", Source::Disabled).await;
        let urls = vec![
            format!("{base}/this-month.gz"),
            format!("{base}/last-month.gz"),
        ];
        // Neither gives a database, so the refresh fails: but it asked both.
        assert!(!geo.refresh_from(urls).await);
        assert_eq!(hits[0].load(Ordering::Relaxed), 1);
        assert_eq!(hits[1].load(Ordering::Relaxed), 1);
        // And a database it did not get does not replace the one in use.
        assert!(geo.lookup("8.8.8.8").await.is_none());
    }
}
