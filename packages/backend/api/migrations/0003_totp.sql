-- Two-factor authentication with one-time passwords (src/totp.rs).
-- totp_secret is the base32 secret of an authenticator app, NULL while
-- two-factor authentication is off; totp_pending is a secret being set up,
-- until a code from it confirms it. totp_last_step is the time step of the
-- last code used, so a code cannot be used twice.
ALTER TABLE users ADD COLUMN totp_secret TEXT;
ALTER TABLE users ADD COLUMN totp_pending TEXT;
ALTER TABLE users ADD COLUMN totp_last_step INTEGER NOT NULL DEFAULT 0;

-- Single-use recovery codes, for a user who lost their authenticator: the
-- SHA-256 of the code (they are random, so a fast hash is enough).
CREATE TABLE recovery_codes (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  code_hash TEXT NOT NULL,
  PRIMARY KEY (user_id, code_hash)
);
