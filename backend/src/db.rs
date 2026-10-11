use rusqlite::{Connection, OpenFlags};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Mutex, MutexGuard};

/// How many read-only connections serve the dashboard and the export.
const READERS: usize = 4;

/// The database: one writer connection and a few read-only ones. SQLite
/// has a single writer, and every write here is small, so the writer sits
/// behind one async mutex. In WAL mode readers never block the writer
/// (nor each other), so the heavy aggregate queries get their own
/// connections and can no longer hold up the inserts of the tracker.
pub struct Database {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

pub type Db = Arc<Database>;

impl Database {
    /// The writer, for anything that changes data (or needs to read its
    /// own writes in the same step).
    pub async fn lock(&self) -> MutexGuard<'_, Connection> {
        self.writer.lock().await
    }

    /// The writer from non-async code (the seed binary).
    pub fn blocking_lock(&self) -> MutexGuard<'_, Connection> {
        self.writer.blocking_lock()
    }

    /// A read-only connection: a free one if there is one, otherwise the
    /// next in turn. An in-memory database (the tests) is private to its
    /// connection and has no readers, so reads use the writer there.
    pub async fn read(&self) -> MutexGuard<'_, Connection> {
        if self.readers.is_empty() {
            return self.writer.lock().await;
        }
        for reader in &self.readers {
            if let Ok(conn) = reader.try_lock() {
                return conn;
            }
        }
        let i = self.next.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        self.readers[i].lock().await
    }
}

/// The `synchronous` modes SQLite accepts.
const SYNCHRONOUS_MODES: [&str; 4] = ["OFF", "NORMAL", "FULL", "EXTRA"];

/// How hard the writer syncs to disk. NORMAL is safe in WAL mode: a crash
/// of the process loses nothing, and a power cut can lose the last
/// transactions but never corrupts the database.
pub const DEFAULT_SYNCHRONOUS: &str = "NORMAL";

/// Open the database with the default `synchronous` mode.
pub fn open(path: &str) -> Db {
    open_with(path, DEFAULT_SYNCHRONOUS)
}

/// Open the database and make sure the schema is current. The schema is
/// idempotent, so this is also all the migration there is. `synchronous`
/// is one of OFF, NORMAL, FULL and EXTRA, in any case.
pub fn open_with(path: &str, synchronous: &str) -> Db {
    let synchronous = synchronous.to_ascii_uppercase();
    assert!(
        SYNCHRONOUS_MODES.contains(&synchronous.as_str()),
        "SQLITE_SYNCHRONOUS must be one of {}, not {synchronous}",
        SYNCHRONOUS_MODES.join(", ")
    );
    // The database's directory may not exist yet (a fresh clone, a new
    // volume): create it.
    if let Some(parent) = std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn =
        Connection::open(path).unwrap_or_else(|e| panic!("cannot open database at {path}: {e}"));
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    conn.pragma_update(None, "synchronous", &synchronous)
        .expect("cannot set synchronous");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("cannot enable foreign keys");
    conn.execute_batch(SCHEMA)
        .unwrap_or_else(|e| panic!("cannot initialize schema: {e}"));
    ensure_columns(&conn);

    // The readers open after the schema exists, so they see a finished
    // database in WAL mode.
    let readers = if path == ":memory:" {
        Vec::new()
    } else {
        (0..READERS)
            .map(|_| {
                let reader = Connection::open_with_flags(
                    path,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )
                .unwrap_or_else(|e| panic!("cannot open a reader on {path}: {e}"));
                Mutex::new(reader)
            })
            .collect()
    };
    Arc::new(Database {
        writer: Mutex::new(conn),
        readers,
        next: AtomicUsize::new(0),
    })
}

/// Columns the schema grew after databases already existed. Each runs
/// when its column is missing, so an existing database keeps working.
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[(
    "users",
    "theme",
    "ALTER TABLE users ADD COLUMN theme TEXT NOT NULL DEFAULT '{\"base\":\"light\",\"accent\":\"#0f172a\"}'",
)];

fn ensure_columns(conn: &Connection) {
    for (table, column, alter) in ADDED_COLUMNS {
        let present: bool = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .and_then(|mut stmt| {
                let mut rows = stmt.query([])?;
                let mut found = false;
                while let Some(row) = rows.next()? {
                    if row.get::<_, String>(1)? == *column {
                        found = true;
                    }
                }
                Ok(found)
            })
            .unwrap_or(false);
        if !present {
            conn.execute(alter, [])
                .unwrap_or_else(|e| panic!("cannot add the missing column: {e}"));
        }
    }
}

const SCHEMA: &str = r##"
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  date_format TEXT NOT NULL DEFAULT 'YYYY-MM-DD',
  time_format TEXT NOT NULL DEFAULT '24h',
  theme TEXT NOT NULL DEFAULT '{"base":"light","accent":"#0f172a"}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sites (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  hostname TEXT NOT NULL UNIQUE,
  honor_dnt INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE IF NOT EXISTS hits (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('pageview', 'click')),
  path TEXT NOT NULL,
  name TEXT,
  referrer TEXT,
  country TEXT,
  device TEXT NOT NULL,
  browser TEXT NOT NULL DEFAULT 'Other',
  os TEXT NOT NULL DEFAULT 'Other',
  visitor TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS hits_site_time ON hits (site_id, created_at);
CREATE TABLE IF NOT EXISTS salts (day TEXT PRIMARY KEY, salt TEXT NOT NULL);
PRAGMA user_version = 1;
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn readers_see_the_writes_and_do_not_block_the_writer() {
        let dir = std::env::temp_dir().join(format!("potoo-db-test-{}", std::process::id()));
        let path = dir.join("test.db");
        let db = open(path.to_str().unwrap());

        let writer = db.lock().await;
        writer
            .execute(
                "INSERT INTO salts (day, salt) VALUES ('2026-01-01', 'a')",
                [],
            )
            .unwrap();

        // A read while the writer is held: it must not wait for it.
        let reader = tokio::time::timeout(std::time::Duration::from_secs(2), db.read())
            .await
            .expect("a reader waited for the writer");
        let salt: String = reader
            .query_row("SELECT salt FROM salts WHERE day = '2026-01-01'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(salt, "a");
        // And it really is read-only.
        assert!(reader.execute("DELETE FROM salts", []).is_err());

        drop((reader, writer));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn synchronous_is_normal_by_default_and_configurable() {
        let mode = |db: Db| -> i64 {
            db.blocking_lock()
                .query_row("PRAGMA synchronous", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(mode(open(":memory:")), 1); // NORMAL
        assert_eq!(mode(open_with(":memory:", "full")), 2); // FULL
    }

    #[test]
    #[should_panic(expected = "SQLITE_SYNCHRONOUS")]
    fn an_unknown_synchronous_mode_is_refused() {
        open_with(":memory:", "sometimes");
    }
}
