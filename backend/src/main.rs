use potoo::{app, db, geo, state::AppState};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    // The binary is built for musl (the Docker image is Alpine), so the
    // TLS stack is ring rather than aws-lc; reqwest is built without a
    // provider and uses this default one.
    potoo::install_tls_provider();

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

    // How hard the database syncs to disk: OFF, NORMAL, FULL or EXTRA.
    let synchronous =
        std::env::var("SQLITE_SYNCHRONOUS").unwrap_or_else(|_| db::DEFAULT_SYNCHRONOUS.into());
    let db = db::open_with(&db_path, &synchronous).await;
    let (ingest, writer) = potoo::ingest::Ingest::spawn(db.clone());
    let geo = Arc::new(geo::Geo::open(&geo_path, geo_url).await);
    let state = AppState {
        db: db.clone(),
        ingest,
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

    #[cfg(feature = "profiling")]
    potoo::profile::spawn_reporter();
    // The flamegraph is opt-in, so the phase timers can run without it.
    #[cfg(feature = "profiling")]
    let flamegraph = std::env::var("PROFILE_FLAMEGRAPH").ok().map(|out| {
        let profiler = pprof::ProfilerGuardBuilder::default()
            .frequency(99)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .expect("cannot start the profiler");
        (profiler, out)
    });

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| panic!("cannot listen on {port}: {e}"));
    println!("potoo listening on http://0.0.0.0:{port}");
    // On SIGINT or SIGTERM the server stops accepting, finishes the requests
    // in flight, and the writer then drains the queue: no accepted hit is
    // lost to a clean stop (`docker stop`, a deploy).
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(stop_signal())
    .await
    .unwrap();
    writer.await.unwrap();
    db.close().await;
    #[cfg(feature = "profiling")]
    if let Some((profiler, out)) = &flamegraph {
        write_profile(profiler, out);
    }
}

/// Resolves on SIGINT or SIGTERM.
async fn stop_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("cannot listen for SIGTERM");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(feature = "profiling")]
fn write_profile(profiler: &pprof::ProfilerGuard<'_>, out: &str) {
    match profiler.report().build() {
        Ok(report) => match std::fs::File::create(out) {
            Ok(file) => match report.flamegraph(file) {
                Ok(()) => println!("flamegraph written to {out}"),
                Err(e) => eprintln!("cannot write the flamegraph: {e}"),
            },
            Err(e) => eprintln!("cannot create {out}: {e}"),
        },
        Err(e) => eprintln!("cannot build the profile: {e}"),
    }
}

async fn purge(db: &db::Db, retention_days: i64) {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    if let Err(e) = sqlx::query("DELETE FROM hits WHERE created_at < ?1")
        .bind(cutoff)
        .execute(db.writer())
        .await
    {
        eprintln!("retention purge failed: {e}");
    }
}
