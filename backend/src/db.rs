use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;

/// One connection behind an async mutex: SQLite has a single writer, and
/// every query here is small, so this keeps things simple and correct.
pub type Db = Arc<Mutex<Connection>>;

/// Open the database and make sure the schema is current. The schema is
/// idempotent, so this is also all the migration there is.
pub fn open(path: &str) -> Db {
    // The database's directory may not exist yet (a fresh clone, a new
    // volume): create it.
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let conn =
        Connection::open(path).unwrap_or_else(|e| panic!("cannot open database at {path}: {e}"));
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("cannot enable foreign keys");
    conn.execute_batch(SCHEMA)
        .unwrap_or_else(|e| panic!("cannot initialize schema: {e}"));
    Arc::new(Mutex::new(conn))
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  date_format TEXT NOT NULL DEFAULT 'YYYY-MM-DD',
  time_format TEXT NOT NULL DEFAULT '24h',
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
"#;
