use anyhow::Context;
use potoo::{app, db, geo, state::AppState};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The container healthcheck: no database, no server, only a request to
    // the one already running (the same command as woodmouse's). Its error
    // ends the process with status 1.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck().await.context("unhealthy");
    }

    // RUST_LOG picks what is logged (default: info).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

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
    let geo_source = match std::env::var("GEOIP_URL") {
        Ok(url) if url.is_empty() => geo::Source::Disabled,
        Ok(url) => geo::Source::Url(url),
        Err(_) => geo::Source::DbIp,
    };

    // How hard the database syncs to disk: OFF, NORMAL, FULL or EXTRA.
    let synchronous =
        std::env::var("SQLITE_SYNCHRONOUS").unwrap_or_else(|_| db::DEFAULT_SYNCHRONOUS.into());
    let db = db::open_with(&db_path, &synchronous).await;
    let (ingest, writer) = potoo::ingest::Ingest::spawn(db.clone());
    let geo = Arc::new(geo::Geo::open(&geo_path, geo_source).await);
    let public_url = potoo::config::public_url();
    let limits = Arc::new(potoo::limits::Limits::default());
    let state = AppState {
        db: db.clone(),
        ingest,
        geo: geo.clone(),
        trust_proxy: env_flag("TRUST_PROXY")?,
        limits: limits.clone(),
        secure_cookies: public_url
            .as_deref()
            .is_some_and(|u| u.starts_with("https://")),
        public_url,
        challenge_key: axum_extra::extract::cookie::Key::generate(),
        // Single sign-on: the stored settings, with the OIDC_* environment
        // variables on top.
        oidc: Arc::new(
            potoo::oidc::Oidc::load(
                &db,
                potoo::oidc::Env::from_env().map_err(anyhow::Error::msg)?,
            )
            .await,
        ),
        totp_required: env_flag("TOTP_REQUIRED")?,
    };

    // The login limits forget the keys that are back to their full quota.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(600)).await;
            limits.forget_idle();
        }
    });

    // Expired sessions are deleted every hour (a session past its expiry
    // is refused anyway: this only keeps the table small).
    {
        use tower_sessions::ExpiredDeletion;
        let store = potoo::session::Store::new(db.clone());
        tokio::spawn(async move {
            loop {
                if let Err(e) = store.delete_expired().await {
                    tracing::warn!("cannot delete the expired sessions: {e}");
                }
                tokio::time::sleep(Duration::from_secs(3_600)).await;
            }
        });
    }

    // With OIDC_ONLY and no provider, nobody can sign in (but the first
    // admin on /setup, who can then set the provider up): say so.
    if !state.oidc.passwords_allowed() && !state.oidc.settings().enabled() {
        tracing::warn!(
            "OIDC_ONLY is set but single sign-on is not set up: nobody can sign in \
             until it is (set the OIDC_* variables, or set it up in the admin settings)"
        );
    }

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
            .context("cannot start the profiler")?;
        anyhow::Ok((profiler, out))
    });
    #[cfg(feature = "profiling")]
    let flamegraph = flamegraph.transpose()?;

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("cannot listen on {port}"))?;
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
    .context("the server failed")?;
    writer.await.context("the writer failed")?;
    db.close().await;
    #[cfg(feature = "profiling")]
    if let Some((profiler, out)) = &flamegraph {
        write_profile(profiler, out);
    }
    Ok(())
}

/// A yes or no environment variable: unset or empty is no, and a value
/// that is not understood stops the start rather than quietly meaning no.
fn env_flag(name: &str) -> anyhow::Result<bool> {
    Ok(potoo::config::flag(name)
        .map_err(anyhow::Error::msg)?
        .unwrap_or(false))
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

/// `GET /api/health` on this machine, by hand, so that the healthcheck needs
/// nothing from the image (no wget or curl). Succeeds on a 200, fails
/// otherwise.
async fn healthcheck() -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    stream
        .write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .await?;
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).await?;
    anyhow::ensure!(
        &head == b"HTTP/1.1 200" || &head == b"HTTP/1.0 200",
        "the health endpoint did not answer 200"
    );
    Ok(())
}
