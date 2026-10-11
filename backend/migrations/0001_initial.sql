-- The schema as it was before migrations existed. Every statement is
-- idempotent, so a database created by an earlier build adopts this one
-- unchanged. Later changes go in new files; never edit one that shipped.
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
