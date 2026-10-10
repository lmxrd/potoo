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
    if let Some(parent) = std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn =
        Connection::open(path).unwrap_or_else(|e| panic!("cannot open database at {path}: {e}"));
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("cannot enable foreign keys");
    conn.execute_batch(SCHEMA)
        .unwrap_or_else(|e| panic!("cannot initialize schema: {e}"));
    ensure_columns(&conn);
    Arc::new(Mutex::new(conn))
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
