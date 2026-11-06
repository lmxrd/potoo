-- API keys: a user's tokens for scripts, sent as `Authorization: Bearer`.
-- The token itself is shown once; what is stored is its SHA-256 (it is
-- random, so a fast hash is enough) and its start, to tell keys apart.
-- `scope` caps what the key may do: `read` (GET only), `write` (also
-- changes, no admin endpoints) or `admin`; never more than the user may.
CREATE TABLE api_keys (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  prefix TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  scope TEXT NOT NULL CHECK (scope IN ('read', 'write', 'admin')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_used_at TEXT,
  -- NULL for a key that does not expire.
  expires_at TEXT
);
CREATE INDEX api_keys_user ON api_keys (user_id);
