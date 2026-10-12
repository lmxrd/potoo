-- The schema. Facet values with a fixed vocabulary (country, device,
-- browser, os) are stored as lowercase keys (`no`, `desktop`, `chrome`,
-- `macos`); what is shown for them is the frontend's business.
CREATE TABLE users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  date_format TEXT NOT NULL DEFAULT 'YYYY-MM-DD',
  use_24 INTEGER NOT NULL DEFAULT 1,
  theme TEXT NOT NULL DEFAULT '{"base":"light","accent":"#0f172a"}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at TEXT NOT NULL
);
CREATE TABLE sites (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  hostname TEXT NOT NULL UNIQUE,
  honor_dnt INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE hits (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('pageview', 'click')),
  path TEXT NOT NULL,
  name TEXT,
  referrer TEXT,
  -- country, device, browser and os are NULL when unknown (an imported hit
  -- may have none of them); the stats show NULL as `unknown`.
  country TEXT,
  device TEXT,
  browser TEXT,
  os TEXT,
  -- NULL for an imported hit: the visitor hash cannot be rebuilt from an
  -- export, and COUNT(DISTINCT visitor) ignores NULLs, so an import adds
  -- pageviews but no unique visitors.
  visitor TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX hits_site_time ON hits (site_id, created_at);
CREATE TABLE salts (day TEXT PRIMARY KEY, salt TEXT NOT NULL);
