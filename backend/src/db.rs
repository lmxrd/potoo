use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// How many read-only connections serve the dashboard and the export.
const READERS: u32 = 4;

/// How long a site stays cached. Sites change through this process, which
/// forgets them at once; the expiry only covers another process (the seed
/// binary) changing the database.
const SITE_TTL: Duration = Duration::from_secs(60);

/// A registered site, as the tracker needs it.
#[derive(Clone, Copy)]
pub struct Site {
    pub id: i64,
    pub honor_dnt: bool,
}

/// The database: a pool of one writer connection and a pool of read-only
/// ones. SQLite has a single writer, so the writer pool has a single
/// connection: writes queue up for it in order, and a transaction holds it
/// until it ends. In WAL mode readers never block the writer (nor each
/// other), so the heavy aggregate queries get their own connections and
/// cannot hold up the inserts of the tracker.
///
/// Never run a query on the writer pool while holding a transaction (or a
/// connection) from it: with one connection that waits for itself. Inside
/// a transaction, use the transaction.
pub struct Database {
    writer: SqlitePool,
    reader: SqlitePool,
    /// The hot path of the tracker looks up the site and the day's salt of
    /// every hit; both hardly ever change, so they are kept in memory
    /// instead of taking a database connection per hit. Misses (unknown
    /// hostnames, which anyone can send) are not cached.
    sites: RwLock<HashMap<String, (Site, Instant)>>,
    salt: Mutex<Option<(String, String)>>,
}

pub type Db = Arc<Database>;

impl Database {
    /// For everything that changes data, and for reads that must be part of
    /// such a change (inside its transaction).
    pub fn writer(&self) -> &SqlitePool {
        &self.writer
    }

    /// For reads. An in-memory database (the tests) is private to its one
    /// connection, so there it is the writer pool.
    pub fn reader(&self) -> &SqlitePool {
        &self.reader
    }

    /// The site registered for a hostname. A hit is answered from memory;
    /// a miss reads the database once.
    pub async fn site(&self, hostname: &str) -> sqlx::Result<Option<Site>> {
        if let Some((site, since)) = self.sites.read().unwrap().get(hostname)
            && since.elapsed() < SITE_TTL
        {
            return Ok(Some(*site));
        }
        let site: Option<(i64, i64)> =
            sqlx::query_as("SELECT id, honor_dnt FROM sites WHERE hostname = ?1")
                .bind(hostname)
                .fetch_optional(self.reader())
                .await?;
        let site = site.map(|(id, honor_dnt)| Site {
            id,
            honor_dnt: honor_dnt == 1,
        });
        if let Some(site) = site {
            self.sites
                .write()
                .unwrap()
                .insert(hostname.to_string(), (site, Instant::now()));
        }
        Ok(site)
    }

    /// Forget every cached site: call after a site is changed or deleted.
    pub fn forget_sites(&self) {
        self.sites.write().unwrap().clear();
    }

    /// The salt of a day, if it is the one in memory.
    pub fn cached_salt(&self, day: &str) -> Option<String> {
        match &*self.salt.lock().unwrap() {
            Some((cached_day, salt)) if cached_day == day => Some(salt.clone()),
            _ => None,
        }
    }

    pub fn remember_salt(&self, day: &str, salt: &str) {
        *self.salt.lock().unwrap() = Some((day.to_string(), salt.to_string()));
    }

    /// Close the connections, which checkpoints the WAL: call on a clean
    /// stop, after the last write.
    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }
}

/// The `synchronous` modes SQLite accepts.
const SYNCHRONOUS_MODES: [&str; 4] = ["OFF", "NORMAL", "FULL", "EXTRA"];

/// How hard the writer syncs to disk. NORMAL is safe in WAL mode: a crash
/// of the process loses nothing, and a power cut can lose the last
/// transactions but never corrupts the database.
pub const DEFAULT_SYNCHRONOUS: &str = "NORMAL";

/// Open the database with the default `synchronous` mode.
pub async fn open(path: &str) -> Db {
    open_with(path, DEFAULT_SYNCHRONOUS).await
}

/// Open the database and bring its schema up to date (the files in
/// `migrations/`, applied in order and recorded in `_sqlx_migrations`).
/// `synchronous` is one of OFF, NORMAL, FULL and EXTRA, in any case.
pub async fn open_with(path: &str, synchronous: &str) -> Db {
    let synchronous = synchronous.to_ascii_uppercase();
    assert!(
        SYNCHRONOUS_MODES.contains(&synchronous.as_str()),
        "SQLITE_SYNCHRONOUS must be one of {}, not {synchronous}",
        SYNCHRONOUS_MODES.join(", ")
    );
    let synchronous = SqliteSynchronous::from_str(&synchronous).expect("a valid mode");

    let memory = path == ":memory:";
    // The database's directory may not exist yet (a fresh clone, a new
    // volume): create it.
    if !memory
        && let Some(parent) = std::path::Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
    {
        let _ = std::fs::create_dir_all(parent);
    }

    let options = if memory {
        SqliteConnectOptions::from_str("sqlite::memory:").expect("a valid url")
    } else {
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
    }
    .synchronous(synchronous)
    .foreign_keys(true)
    .busy_timeout(Duration::from_secs(5));

    // The connections live as long as the pool: an in-memory database
    // disappears with its connection.
    let pool = |options: SqliteConnectOptions, connections: u32| {
        SqlitePoolOptions::new()
            .max_connections(connections)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_lazy_with(options)
    };
    let writer = pool(options.clone(), 1);
    migrate(&writer).await;

    // The readers open after the schema exists, so they see a finished
    // database in WAL mode.
    let reader = if memory {
        writer.clone()
    } else {
        pool(options.create_if_missing(false).read_only(true), READERS)
    };
    Arc::new(Database {
        writer,
        reader,
        sites: RwLock::new(HashMap::new()),
        salt: Mutex::new(None),
    })
}

/// Apply the migrations, in order; sqlx records their history in
/// `_sqlx_migrations`.
async fn migrate(pool: &SqlitePool) {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .unwrap_or_else(|e| panic!("cannot migrate the database: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn readers_see_the_writes_and_do_not_block_the_writer() {
        let dir = std::env::temp_dir().join(format!("potoo-db-test-{}", std::process::id()));
        let path = dir.join("test.db");
        let db = open(path.to_str().unwrap()).await;

        // A transaction holds the only writer connection ...
        let mut tx = db.writer().begin().await.unwrap();
        sqlx::query("INSERT INTO salts (day, salt) VALUES ('2026-01-01', 'a')")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let held = db.writer().acquire().await.unwrap();

        // ... and a read goes through anyway, seeing the committed write.
        let salt: String = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            sqlx::query_scalar("SELECT salt FROM salts WHERE day = '2026-01-01'")
                .fetch_one(db.reader()),
        )
        .await
        .expect("a reader waited for the writer")
        .unwrap();
        assert_eq!(salt, "a");
        // And it really is read-only.
        assert!(
            sqlx::query("DELETE FROM salts")
                .execute(db.reader())
                .await
                .is_err()
        );

        drop(held);
        db.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn synchronous_is_normal_by_default_and_configurable() {
        let mode = |db: Db| async move {
            sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
                .fetch_one(db.writer())
                .await
                .unwrap()
        };
        assert_eq!(mode(open(":memory:").await).await, 1); // NORMAL
        assert_eq!(mode(open_with(":memory:", "full").await).await, 2); // FULL
    }

    #[tokio::test]
    #[should_panic(expected = "SQLITE_SYNCHRONOUS")]
    async fn an_unknown_synchronous_mode_is_refused() {
        open_with(":memory:", "sometimes").await;
    }

    #[tokio::test]
    async fn sites_are_cached_until_forgotten() {
        let db = open(":memory:").await;
        sqlx::query("INSERT INTO sites (hostname, honor_dnt) VALUES ('a.com', 0)")
            .execute(db.writer())
            .await
            .unwrap();
        assert!(!db.site("a.com").await.unwrap().unwrap().honor_dnt);
        assert!(db.site("nope.com").await.unwrap().is_none());

        // Changed behind the cache's back: still the cached value ...
        sqlx::query("UPDATE sites SET honor_dnt = 1")
            .execute(db.writer())
            .await
            .unwrap();
        assert!(!db.site("a.com").await.unwrap().unwrap().honor_dnt);
        // ... until the site is forgotten, as the routes do after an edit.
        db.forget_sites();
        assert!(db.site("a.com").await.unwrap().unwrap().honor_dnt);

        // A site added later is found: misses are not cached.
        sqlx::query("INSERT INTO sites (hostname) VALUES ('nope.com')")
            .execute(db.writer())
            .await
            .unwrap();
        assert!(db.site("nope.com").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn the_lowercase_migration_rewrites_old_rows_and_only_those() {
        let db = open(":memory:").await;
        sqlx::query("INSERT INTO sites (hostname) VALUES ('a.com')")
            .execute(db.writer())
            .await
            .unwrap();
        // Rows as an older build wrote them, and one that is lowercase already.
        for (country, device, browser, os, path, name) in [
            (Some("NO"), "desktop", "Chrome", "macOS", "/Pricing", None),
            (None, "mobile", "Safari", "iOS", "/a", Some("SignUp")),
            (Some("us"), "tablet", "edge", "windows", "/b", None),
        ] {
            sqlx::query(
                "INSERT INTO hits (site_id, kind, path, name, country, device, browser, os, visitor)
                 VALUES (1, 'pageview', ?1, ?2, ?3, ?4, ?5, ?6, 'v')",
            )
            .bind(path)
            .bind(name)
            .bind(country)
            .bind(device)
            .bind(browser)
            .bind(os)
            .execute(db.writer())
            .await
            .unwrap();
        }

        sqlx::raw_sql(include_str!("../migrations/0002_lowercase_facets.sql"))
            .execute(db.writer())
            .await
            .unwrap();

        #[derive(sqlx::FromRow, PartialEq, Debug)]
        struct Hit {
            country: Option<String>,
            device: String,
            browser: String,
            os: String,
            path: String,
            name: Option<String>,
        }
        let hit = |country: Option<&str>,
                   device: &str,
                   browser: &str,
                   os: &str,
                   path: &str,
                   name: Option<&str>| Hit {
            country: country.map(String::from),
            device: device.into(),
            browser: browser.into(),
            os: os.into(),
            path: path.into(),
            name: name.map(String::from),
        };
        let rows: Vec<Hit> =
            sqlx::query_as("SELECT country, device, browser, os, path, name FROM hits ORDER BY id")
                .fetch_all(db.writer())
                .await
                .unwrap();
        assert_eq!(
            rows,
            [
                hit(Some("no"), "desktop", "chrome", "macos", "/Pricing", None),
                hit(None, "mobile", "safari", "ios", "/a", Some("SignUp")),
                hit(Some("us"), "tablet", "edge", "windows", "/b", None),
            ],
            "the facets are lowercase, a missing country stays missing, a path and a click label keep their case"
        );
    }

    #[tokio::test]
    async fn the_use_24_migration_carries_the_time_format_over() {
        // The migration adds a column, so it is not replayable: it runs on
        // the users table of the build before it, not on the already
        // migrated database.
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, time_format TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for format in ["24h", "12h"] {
            sqlx::query("INSERT INTO users (time_format) VALUES (?1)")
                .bind(format)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::raw_sql(include_str!("../migrations/0003_use_24.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let flags: Vec<bool> = sqlx::query_scalar("SELECT use_24 FROM users ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(flags, [true, false], "24h is true, 12h is false");
    }
}
