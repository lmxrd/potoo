-- Sessions move to tower-sessions, with axum-login signing users in and
-- out (src/session.rs). Their store keeps each session's record (the
-- signed-in user and a hash of their credentials), keyed by the SHA-256 of
-- the session id, never the id itself: a copy of the database holds no
-- session anyone could use. The old sessions go: everyone signs in again
-- once.
DROP TABLE sessions;
CREATE TABLE sessions (
  id_hash TEXT PRIMARY KEY,
  -- The record's data, JSON.
  data TEXT NOT NULL,
  -- Unix seconds.
  expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_expiry ON sessions (expires_at);
