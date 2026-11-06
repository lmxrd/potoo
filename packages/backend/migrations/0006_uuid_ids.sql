-- no-transaction
-- Users, sites and API keys get UUIDv7 ids, as woodmouse's, stored as 16
-- bytes (BLOB), and everything that points at them follows. Hits keep their
-- own integer ids: theirs is the big table, and a hit is never addressed by
-- its id. The new ids are time-ordered by the rows' created_at, so an index
-- on them stays in creation order.
--
-- SQLite cannot change a column's type, so the tables are rebuilt
-- (sqlite.org/lang_altertable.html), with foreign keys off, as in
-- 0002_oidc.sql: with them on, dropping `users` would delete every row
-- that points at it. That PRAGMA does nothing inside a transaction, hence
-- `no-transaction` above and the explicit one below.
--
-- sqlx records a migration like this one only after it ran: a crash in
-- between runs it again. Every step can run twice: ids are only made for
-- rows that still have an integer one, and the copies convert only those.
PRAGMA foreign_keys = OFF;

BEGIN;

-- A UUIDv7 from a stored time (`YYYY-MM-DDTHH:MM:SS.mmmZ`): 48 bits of Unix
-- milliseconds, the version (7), 12 random bits, the variant (8 to b), and
-- 62 more random bits.
CREATE TABLE IF NOT EXISTS uuid_map (
  tbl TEXT NOT NULL,
  old INTEGER NOT NULL,
  new BLOB NOT NULL,
  PRIMARY KEY (tbl, old)
);
INSERT OR IGNORE INTO uuid_map (tbl, old, new)
  SELECT t.tbl, t.id, unhex(
      printf('%012X', CAST(round((julianday(t.created_at) - 2440587.5) * 86400000) AS INTEGER))
      || '7' || substr(hex(randomblob(2)), 2)
      || substr('89AB', 1 + abs(random()) % 4, 1) || substr(hex(randomblob(2)), 2)
      || hex(randomblob(6)))
  FROM (
    SELECT 'users' AS tbl, id, created_at FROM users WHERE typeof(id) = 'integer'
    UNION ALL
    SELECT 'sites', id, created_at FROM sites WHERE typeof(id) = 'integer'
    UNION ALL
    SELECT 'api_keys', id, created_at FROM api_keys WHERE typeof(id) = 'integer'
  ) AS t;

-- Below, each id becomes its mapped UUID when it is still an integer, and
-- stays as it is when it was converted already.

CREATE TABLE users_new (
  id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 16),
  email TEXT NOT NULL UNIQUE,
  -- NULL for a user who has no password (signs in through the provider).
  password_hash TEXT,
  is_admin INTEGER NOT NULL DEFAULT 0,
  date_format TEXT NOT NULL DEFAULT 'YYYY-MM-DD',
  use_24 INTEGER NOT NULL DEFAULT 1,
  theme TEXT NOT NULL DEFAULT '{"base":"light","accent":"#0f172a"}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  totp_secret TEXT,
  totp_pending TEXT,
  totp_last_step INTEGER NOT NULL DEFAULT 0
);
INSERT INTO users_new
  SELECT coalesce((SELECT new FROM uuid_map WHERE tbl = 'users' AND old = u.id), u.id),
         u.email, u.password_hash, u.is_admin, u.date_format, u.use_24, u.theme,
         u.created_at, u.totp_secret, u.totp_pending, u.totp_last_step
  FROM users AS u;
DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE TABLE sites_new (
  id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 16),
  hostname TEXT NOT NULL UNIQUE,
  honor_dnt INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO sites_new
  SELECT coalesce((SELECT new FROM uuid_map WHERE tbl = 'sites' AND old = s.id), s.id),
         s.hostname, s.honor_dnt, s.created_at
  FROM sites AS s;
DROP TABLE sites;
ALTER TABLE sites_new RENAME TO sites;

-- The hits keep their ids, and the AUTOINCREMENT counter, so the id of a
-- deleted hit is never given out again.
CREATE TABLE hits_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  site_id BLOB NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
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
INSERT INTO hits_new
  SELECT h.id,
         coalesce((SELECT new FROM uuid_map WHERE tbl = 'sites' AND old = h.site_id), h.site_id),
         h.kind, h.path, h.name, h.referrer, h.country, h.device, h.browser, h.os,
         h.visitor, h.created_at
  FROM hits AS h;
DELETE FROM sqlite_sequence WHERE name = 'hits_new';
INSERT INTO sqlite_sequence (name, seq) SELECT 'hits_new', seq FROM sqlite_sequence WHERE name = 'hits';
DROP TABLE hits;
ALTER TABLE hits_new RENAME TO hits;
CREATE INDEX hits_site_time ON hits (site_id, created_at);

CREATE TABLE user_identities_new (
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  user_id BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (issuer, subject)
);
INSERT INTO user_identities_new
  SELECT i.issuer, i.subject,
         coalesce((SELECT new FROM uuid_map WHERE tbl = 'users' AND old = i.user_id), i.user_id),
         i.created_at
  FROM user_identities AS i;
DROP TABLE user_identities;
ALTER TABLE user_identities_new RENAME TO user_identities;
CREATE INDEX user_identities_user ON user_identities (user_id);

CREATE TABLE recovery_codes_new (
  user_id BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  code_hash TEXT NOT NULL,
  PRIMARY KEY (user_id, code_hash)
);
INSERT INTO recovery_codes_new
  SELECT coalesce((SELECT new FROM uuid_map WHERE tbl = 'users' AND old = r.user_id), r.user_id),
         r.code_hash
  FROM recovery_codes AS r;
DROP TABLE recovery_codes;
ALTER TABLE recovery_codes_new RENAME TO recovery_codes;

CREATE TABLE api_keys_new (
  id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 16),
  user_id BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  prefix TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  scope TEXT NOT NULL CHECK (scope IN ('read', 'write', 'admin')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_used_at TEXT,
  -- NULL for a key that does not expire.
  expires_at TEXT
);
INSERT INTO api_keys_new
  SELECT coalesce((SELECT new FROM uuid_map WHERE tbl = 'api_keys' AND old = k.id), k.id),
         coalesce((SELECT new FROM uuid_map WHERE tbl = 'users' AND old = k.user_id), k.user_id),
         k.name, k.prefix, k.token_hash, k.scope, k.created_at, k.last_used_at, k.expires_at
  FROM api_keys AS k;
DROP TABLE api_keys;
ALTER TABLE api_keys_new RENAME TO api_keys;
CREATE INDEX api_keys_user ON api_keys (user_id);

-- Sessions name their user by the old integer id: everyone signs in again.
DELETE FROM sessions;

DROP TABLE uuid_map;
DELETE FROM sqlite_sequence WHERE name IN ('users', 'sites', 'api_keys');

COMMIT;

PRAGMA foreign_keys = ON;
