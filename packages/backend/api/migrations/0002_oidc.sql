-- no-transaction
-- Single sign-on (OpenID Connect). A user signed in through a provider may
-- have no password, so users.password_hash becomes nullable. SQLite cannot
-- change a column, so the table is rebuilt (sqlite.org/lang_altertable.html,
-- "making other kinds of table schema changes"). Foreign keys are off while
-- it is: with them on, dropping the old table would delete every session
-- (ON DELETE CASCADE). That PRAGMA does nothing inside a transaction, hence
-- `no-transaction` above and the explicit one below.
--
-- sqlx records a migration like this one only after it ran: a crash in
-- between runs it again on the next start. Every step can run twice (the
-- rebuild copies the rebuilt table as it is; the new tables and the
-- settings row are created only when missing).
PRAGMA foreign_keys = OFF;

BEGIN;

CREATE TABLE users_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL UNIQUE,
  -- NULL for a user who has no password (signs in through the provider).
  password_hash TEXT,
  is_admin INTEGER NOT NULL DEFAULT 0,
  date_format TEXT NOT NULL DEFAULT 'YYYY-MM-DD',
  use_24 INTEGER NOT NULL DEFAULT 1,
  theme TEXT NOT NULL DEFAULT '{"base":"light","accent":"#0f172a"}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO users_new (id, email, password_hash, is_admin, date_format, use_24, theme, created_at)
  SELECT id, email, password_hash, is_admin, date_format, use_24, theme, created_at FROM users;
-- Keep the AUTOINCREMENT counter, so the id of a deleted user is never
-- given out again.
DELETE FROM sqlite_sequence WHERE name = 'users_new';
INSERT INTO sqlite_sequence (name, seq) SELECT 'users_new', seq FROM sqlite_sequence WHERE name = 'users';
DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

-- The provider accounts a user signs in with: the provider's issuer and its
-- stable id for the account (`sub`). The email may change at the provider;
-- this link does not.
CREATE TABLE IF NOT EXISTS user_identities (
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (issuer, subject)
);
CREATE INDEX IF NOT EXISTS user_identities_user ON user_identities (user_id);

-- The single sign-on settings made in the admin settings, one row. An
-- empty string is a field that is not set; an OIDC_* environment variable
-- overrides its field (see src/oidc.rs).
CREATE TABLE IF NOT EXISTS oidc_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  issuer TEXT NOT NULL DEFAULT '',
  client_id TEXT NOT NULL DEFAULT '',
  client_secret TEXT NOT NULL DEFAULT '',
  scopes TEXT NOT NULL DEFAULT '',
  label TEXT NOT NULL DEFAULT '',
  redirect_url TEXT NOT NULL DEFAULT '',
  auto_create INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO oidc_settings (id) VALUES (1);

COMMIT;

PRAGMA foreign_keys = ON;
