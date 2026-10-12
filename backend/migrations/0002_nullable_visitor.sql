-- hits.visitor is NULL for an imported hit: the visitor hash cannot be
-- rebuilt from an export, and COUNT(DISTINCT visitor) ignores NULLs, so an
-- import adds pageviews but no unique visitors. SQLite cannot drop a NOT
-- NULL in place, so the table is rebuilt.
CREATE TABLE hits_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('pageview', 'click')),
  path TEXT NOT NULL,
  name TEXT,
  referrer TEXT,
  country TEXT,
  device TEXT NOT NULL,
  browser TEXT NOT NULL,
  os TEXT NOT NULL,
  visitor TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO hits_new
  (id, site_id, kind, path, name, referrer, country, device, browser, os, visitor, created_at)
  SELECT id, site_id, kind, path, name, referrer, country, device, browser, os, visitor, created_at
  FROM hits;
DROP TABLE hits;
ALTER TABLE hits_new RENAME TO hits;
CREATE INDEX hits_site_time ON hits (site_id, created_at);
