use potoo::{app, db, geo, state::AppState};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "data/potoo.db".into());
    let geo_path = std::env::var("GEOIP_DB_PATH").unwrap_or_else(|_| "data/country.mmdb".into());
    // Set GEOIP_URL to another URL, or to an empty string to never
    // download anything.
    let geo_url = match std::env::var("GEOIP_URL") {
        Ok(url) if url.is_empty() => None,
        Ok(url) => Some(url),
        Err(_) => Some(geo::Geo::default_url()),
    };

    let db = db::open(&db_path);
    let geo = Arc::new(geo::Geo::open(&geo_path, geo_url).await);
    let state = AppState {
        db: db.clone(),
        geo: geo.clone(),
        trust_proxy: std::env::var("TRUST_PROXY")
            .map(|v| v == "1")
            .unwrap_or(false),
        throttle: Arc::new(potoo::auth::LoginThrottle::default()),
    };

    // Optional data retention: delete hits older than RETENTION_DAYS.
    let retention = std::env::var("RETENTION_DAYS")
        .ok()
        .and_then(|days| days.parse::<i64>().ok())
        .filter(|&days| days > 0);
    if let Some(retention) = retention {
        let db = db.clone();
        purge(&db, retention).await;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(6 * 3_600)).await;
                purge(&db, retention).await;
            }
        });
    }

    // While running, refresh the GeoIP database once a week.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(geo::REFRESH_PERIOD).await;
            geo.refresh().await;
        }
    });

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| panic!("cannot listen on {port}: {e}"));
    println!("potoo listening on http://0.0.0.0:{port}");
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn purge(db: &db::Db, retention_days: i64) {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let conn = db.lock().await;
    if let Err(e) = conn.execute(
        "DELETE FROM hits WHERE created_at < ?1",
        rusqlite::params![cutoff],
    ) {
        eprintln!("retention purge failed: {e}");
    }
}
