//! Background jobs, started once by the server: the sign-in limits forget
//! refilled keys, expired sessions are deleted, old hits are purged
//! (`RETENTION_DAYS`) and the GeoIP database is refreshed; and the database
//! is checkpointed when the server stops.

use std::time::Duration;

use tower_sessions::ExpiredDeletion;

use crate::db::Db;
use crate::state::AppState;

/// Move everything in the write-ahead log into the database file and empty
/// the log. Run when the server stops, so the database file alone is
/// complete and the `-wal` file holds nothing.
pub async fn checkpoint(db: &Db) {
    match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(db.writer())
        .await
    {
        Ok(_) => tracing::info!("database checkpointed"),
        Err(e) => tracing::warn!(%e, "could not checkpoint the database"),
    }
}

/// Delete the hits older than `retention_days`.
pub async fn purge(db: &Db, retention_days: i64) {
    let cutoff = crate::time::days_from_now_iso(-retention_days);
    if let Err(e) = sqlx::query("DELETE FROM hits WHERE created_at < ?1")
        .bind(cutoff)
        .execute(db.writer())
        .await
    {
        tracing::error!("retention purge failed: {e}");
    }
}

/// Start the jobs: the sign-in limits forget refilled keys every ten
/// minutes; expired sessions are deleted every hour (a session past its
/// expiry is refused anyway: this only keeps the table small); hits older
/// than `retention_days` are purged now and every six hours; the GeoIP
/// database is refreshed once a week.
pub fn spawn(state: &AppState, retention_days: Option<i64>) {
    let limits = state.limits.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(600)).await;
            limits.retain_recent();
        }
    });

    let store = crate::sessions::Store::new(state.db.clone());
    tokio::spawn(async move {
        loop {
            if let Err(e) = store.delete_expired().await {
                tracing::warn!("cannot delete the expired sessions: {e}");
            }
            tokio::time::sleep(Duration::from_secs(3_600)).await;
        }
    });

    if let Some(retention_days) = retention_days {
        let db = state.db.clone();
        tokio::spawn(async move {
            loop {
                purge(&db, retention_days).await;
                tokio::time::sleep(Duration::from_secs(6 * 3_600)).await;
            }
        });
    }

    let geo = state.geo.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(crate::geo::REFRESH_PERIOD).await;
            geo.refresh().await;
        }
    });
}
