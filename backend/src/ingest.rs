//! The write path of the tracker. A handler validates a hit and queues it;
//! one task writes the queue to the database, committing everything that
//! has accumulated in a single transaction. Under light load that is one
//! hit per transaction with no added delay (the task never waits for a
//! batch to fill); under heavy load, while one transaction runs, the next
//! batch builds up, so the cost of a commit is shared by hundreds of hits.
//!
//! A hit is in memory until its transaction commits, typically a few
//! milliseconds: a crash (not a clean shutdown, which drains the queue)
//! loses the hits still queued, as the default `synchronous = NORMAL` may
//! lose the last commits to a power cut. Reads that must see every hit
//! accepted so far (the dashboard, the export) call [`Ingest::flush`].

use crate::db::Db;
use crate::error::HttpError;
use crate::profile::{Phase, time, timed};
use axum::http::StatusCode;
use sqlx::{Connection, SqliteConnection};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Hits waiting for the writer. Beyond this the tracker answers 503
/// instead of piling up memory.
const QUEUE: usize = 20_000;

/// The most hits one transaction takes: a bound on how long the writer
/// holds the database connection (and the async worker) at a time.
const MAX_BATCH: usize = 500;

/// A hit, ready to be stored.
pub struct NewHit {
    pub created_at: String,
    pub site_id: i64,
    pub kind: &'static str,
    pub path: String,
    pub name: Option<String>,
    pub referrer: Option<String>,
    pub country: Option<String>,
    pub device: String,
    pub browser: String,
    pub os: String,
    pub visitor: String,
}

enum Message {
    Hit(Box<NewHit>),
    /// Answered once everything queued before it is written.
    Flush(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct Ingest {
    queue: mpsc::Sender<Message>,
}

impl Ingest {
    /// Start the writer. It ends, after writing what is queued, when every
    /// `Ingest` is dropped; await the handle to wait for that.
    pub fn spawn(db: Db) -> (Ingest, JoinHandle<()>) {
        let (queue, hits) = mpsc::channel(QUEUE);
        (Ingest { queue }, tokio::spawn(write_loop(db, hits)))
    }

    /// Queue a hit without waiting for it to be written.
    pub fn submit(&self, hit: NewHit) -> Result<(), HttpError> {
        time(Phase::Enqueue, || {
            self.queue.try_send(Message::Hit(Box::new(hit)))
        })
        .map_err(|_| HttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "Too many hits queued, try again".into(),
        })
    }

    /// Wait until every hit submitted before this call is written.
    pub async fn flush(&self) {
        let (done, written) = oneshot::channel();
        if self.queue.send(Message::Flush(done)).await.is_ok() {
            let _ = written.await;
        }
    }
}

async fn write_loop(db: Db, mut messages: mpsc::Receiver<Message>) {
    let mut hits: Vec<NewHit> = Vec::with_capacity(MAX_BATCH);
    let mut flushes: Vec<oneshot::Sender<()>> = Vec::new();
    while let Some(first) = messages.recv().await {
        let mut next = Some(first);
        while let Some(message) = next.take() {
            match message {
                Message::Hit(hit) => hits.push(*hit),
                Message::Flush(done) => flushes.push(done),
            }
            if hits.len() < MAX_BATCH {
                next = messages.try_recv().ok();
            }
        }
        timed(Phase::Batch, write(&db, &mut hits)).await;
        for done in flushes.drain(..) {
            let _ = done.send(());
        }
    }
}

const INSERT: &str = "INSERT INTO hits
       (created_at, site_id, kind, path, name, referrer, country, device, browser, os, visitor)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)";

async fn insert(conn: &mut SqliteConnection, hit: &NewHit) -> sqlx::Result<()> {
    sqlx::query(INSERT)
        .bind(&hit.created_at)
        .bind(hit.site_id)
        .bind(hit.kind)
        .bind(&hit.path)
        .bind(&hit.name)
        .bind(&hit.referrer)
        .bind(&hit.country)
        .bind(&hit.device)
        .bind(&hit.browser)
        .bind(&hit.os)
        .bind(&hit.visitor)
        .execute(conn)
        .await?;
    Ok(())
}

async fn write(db: &Db, hits: &mut Vec<NewHit>) {
    if hits.is_empty() {
        return;
    }
    // The writer connection, held for the whole batch.
    let mut conn = match timed(Phase::WriterLockWait, db.writer().acquire()).await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("cannot write {} hits: {e}", hits.len());
            hits.clear();
            return;
        }
    };
    let batch = timed(Phase::WriterExec, async {
        let mut tx = conn.begin().await?;
        for hit in hits.iter() {
            insert(&mut tx, hit).await?;
        }
        tx.commit().await
    })
    .await;
    if batch.is_err() {
        // One bad hit (its site was deleted while it was queued) must not
        // take the others down with it: write them one by one.
        let mut lost = 0;
        for hit in hits.iter() {
            if insert(&mut conn, hit).await.is_err() {
                lost += 1;
            }
        }
        if lost > 0 {
            eprintln!(
                "dropped {lost} of {} hits that could not be stored",
                hits.len()
            );
        }
    }
    hits.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(site_id: i64, n: usize) -> NewHit {
        NewHit {
            created_at: crate::auth::now_iso(),
            site_id,
            kind: "pageview",
            path: format!("/page/{n}"),
            name: None,
            referrer: None,
            country: None,
            device: "desktop".into(),
            browser: "Firefox".into(),
            os: "Linux".into(),
            visitor: format!("visitor-{n}"),
        }
    }

    async fn setup() -> (Db, Ingest, JoinHandle<()>) {
        let db = crate::db::open(":memory:").await;
        sqlx::query("INSERT INTO sites (hostname) VALUES ('example.com')")
            .execute(db.writer())
            .await
            .unwrap();
        let (ingest, writer) = Ingest::spawn(db.clone());
        (db, ingest, writer)
    }

    async fn stored(db: &Db) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM hits")
            .fetch_one(db.writer())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn flush_waits_for_every_queued_hit() {
        let (db, ingest, _writer) = setup().await;
        for n in 0..1500 {
            ingest.submit(hit(1, n)).unwrap();
        }
        ingest.flush().await;
        assert_eq!(stored(&db).await, 1500);
    }

    #[tokio::test]
    async fn a_clean_stop_drains_the_queue() {
        let (db, ingest, writer) = setup().await;
        for n in 0..300 {
            ingest.submit(hit(1, n)).unwrap();
        }
        drop(ingest);
        writer.await.unwrap();
        assert_eq!(stored(&db).await, 300);
    }

    #[tokio::test]
    async fn one_bad_hit_does_not_take_its_batch_down() {
        let (db, ingest, _writer) = setup().await;
        for n in 0..10 {
            ingest.submit(hit(1, n)).unwrap();
        }
        ingest.submit(hit(999, 0)).unwrap(); // a site that does not exist
        for n in 10..20 {
            ingest.submit(hit(1, n)).unwrap();
        }
        ingest.flush().await;
        assert_eq!(stored(&db).await, 20);
    }

    #[tokio::test]
    async fn a_full_queue_answers_503() {
        let (queue, _hits) = mpsc::channel(1); // nobody reads it
        let ingest = Ingest { queue };
        ingest.submit(hit(1, 0)).unwrap();
        let error = ingest.submit(hit(1, 1)).unwrap_err();
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
