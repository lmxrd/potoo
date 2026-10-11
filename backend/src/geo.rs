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
    url: Option<String>,
    reader: RwLock<Option<Arc<Reader<Vec<u8>>>>>,
}

impl Geo {
    pub async fn open(path: &str, url: Option<String>) -> Geo {
        let geo = Geo {
            path: PathBuf::from(path),
            url,
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

    /// The default database: DB-IP's free country lite, this month's build.
    pub fn default_url() -> String {
        format!(
            "https://download.db-ip.com/free/dbip-country-lite-{}.mmdb.gz",
            chrono::Utc::now().format("%Y-%m")
        )
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

    /// Download, decompress and install a fresh database. Returns success;
    /// the old database is only replaced once the new one actually opens.
    pub async fn refresh(&self) -> bool {
        let Some(url) = &self.url else {
            return false;
        };
        match download(url).await {
            Ok(bytes) => match Reader::from_source(bytes.clone()) {
                Ok(reader) => {
                    if let Some(dir) = self.path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if std::fs::write(&self.path, &bytes).is_err() {
                        return false;
                    }
                    *self.reader.write().await = Some(Arc::new(reader));
                    true
                }
                Err(e) => {
                    eprintln!("GeoIP database download failed: {e}");
                    false
                }
            },
            Err(e) => {
                eprintln!("GeoIP database download failed: {e}");
                false
            }
        }
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
