# AGENTS.md

The Rust backend (`packages/backend`): axum, SQLite through sqlx, the
OpenAPI spec (utoipa), and the frontend embedded (rust-embed). Read the
root `AGENTS.md` first (commands, privacy rules, releases); this file is
what the backend's code must keep.

## Commands

Run them through mise from the repository root (`mise run check`,
`mise run test:backend`, `mise run gen:spec`, `mise run seed`), or with
cargo here once `packages/frontend/dist` exists (`mise run build:frontend`):
`cargo test`, `cargo clippy --all-targets -- -D warnings`.

## Structure

- One crate, `potoo`: the library in `src/lib.rs` (the app: routes,
  database, everything else), the server binary in `src/main.rs`, and the
  `seed` and `specs` tools in `src/bin/` (not part of the release build).
  `examples/loadtest.rs` is the load generator, `migrations/` the schema,
  and `tests/it/` the integration tests (one binary, `main.rs` lists the
  modules). CONTRIBUTING.md maps every file.
- **rust-embed** needs `packages/frontend/dist` to exist to compile, even in
  debug (the mise tasks that compile the backend build the frontend first). Do
  not run raw `cargo` commands on a fresh checkout without `mise run
  build:frontend`.

## Conventions

- **Simple.** No ORM, no abstraction layers. Comment the *why*, not the
  what.
- **Dependencies.** Prefer a well-known, maintained crate over writing the
  same thing ourselves. Propose it first (name, exact version, why) and get
  approval before adding it. Write it ourselves only when no maintained one
  fits, and say why in a comment where it lives (for example the session
  store: the community SQL store needs sqlx 0.8, and two bundled SQLites do
  not link).
- **Errors.** Handlers return `ApiError` (`src/error.rs`, thiserror):
  `ApiError::new(status, message)` and its helpers (`bad_request`,
  `unauthorized`, `forbidden`, `not_found`, `conflict`, ...) for what the
  client may read, in sentence case; a database error (`?` on sqlx) or
  `ApiError::internal(...)` is logged with its causes and answered with a
  plain 500 "Internal server error", never its text. The binaries (main, the
  seed, the spec writer, `potoo healthcheck`) use anyhow with
  `.context(...)`.
- **Endpoints.** Static files at the root (`/tracker.js`); everything that
  writes or processes lives under `/api/`. What only an admin manages (the
  users, and single sign-on, the one setting of the whole instance so far)
  is under `/api/admin/`; the instance's settings are tagged `Admin`.
- **One range spec.** Time-filtered endpoints take `from`/`to` (ISO 8601
  timestamps), plus `sites` and `q`; reuse `parse_range` /
  `range_and_filter`, do not invent a new syntax.
- **Duplicated lists.** The date and time format lists are in
  `packages/backend/src/auth.rs` (validation) and `packages/frontend/src/lib/dates.ts`
  (labels): change both together. So are the single sign-on error codes:
  `Failure` in `packages/backend/src/oidc.rs` and `SSO_ERRORS` in
  `packages/frontend/src/routes/login.tsx`.

**The database** (`db.rs`, sqlx, SQLite only):

- `db.writer()` is a pool of **one** connection; `db.reader()` is a pool of
  read-only connections. Use the reader for reads that need no atomicity.
- Never run a query on the writer pool while you hold a transaction or
  connection from it: with one connection that waits for itself. Inside a
  transaction, use the transaction (`&mut *tx`).
- A read-then-write (check an email, count admins, ...) belongs in a
  transaction on the writer. Do not hold a transaction during slow work
  (password hashing).
- Hits are queued, so anything that must see every accepted hit (stats,
  export) calls `state.ingest.flush().await` first.
- Queries are run-time `sqlx::query` / `query_as` with `FromRow`; never the
  `query!` macros (they need a database at compile time). SQL is
  SQLite-flavoured (`?1` placeholders, `strftime`, ...). Dynamic SQL (stats
  filters, export) is wrapped in `sqlx::AssertSqlSafe` and is built only
  from fixed fragments and `?` placeholders; every value is bound. Keep it
  that way.
- Schema changes are **new numbered files** in `packages/backend/migrations/`.
  Never edit one that has shipped (its checksum is recorded).
- The site lookup and the daily salt are cached in `Database`. After any
  change to a site, call `db.forget_sites()`.
- A clean stop (SIGTERM/SIGINT) drains the queue and closes the pools; a
  crash loses the hits still queued. A full queue answers `503`.
- A migration that rebuilds a table (SQLite cannot change a column) runs
  with foreign keys off, as `0002_oidc.sql` does (`-- no-transaction`, then
  its own `BEGIN`/`COMMIT`): with them on, dropping `users` would delete
  every session. Test it on a real file with data, like
  `the_single_sign_on_migration_keeps_users_and_sessions` in `db.rs`.
- Users, sites and API keys have UUIDv7 ids (`uuid::Uuid::now_v7()`,
  made by the caller: an insert names its `id`), stored as 16-byte BLOBs,
  `NOT NULL`; the API sends them as strings. Hits keep integer ids (the big
  table, never addressed by id). Where ids go into dynamic SQL as text
  params (stats, export), they are bound as hex: `unhex(?)`.
- Dates and times are jiff (`jiff::Timestamp`), stored as
  `YYYY-MM-DDTHH:MM:SS.mmmZ` text (`format!("{t:.3}")`, `time::iso`). chrono
  is only in the tests, for openidconnect's ID token claims.
- `users.password_hash` is NULL for a user who has no password (single
  sign-on only): anything that reads it must treat NULL as "no password",
  never as a hash.

**Tests run on an in-memory database**, where the reader and the writer
pools are one and the same single connection. Two consequences: a write
made through `db.reader()` (read-only on a real file) passes the tests but
fails in production; and a read on `db.reader()` while a writer
transaction is open works in production but deadlocks in the tests. The
test in `db.rs` `readers_see_the_writes_and_do_not_block_the_writer` uses a
real file; add one when you change how the pools are used.

**The API is described by OpenAPI, and the frontend client is generated from
it** (nothing generated is committed; the server serves the spec only with
`SWAGGER_UI=true`, at `/swagger-ui`):

- Every handler has a `#[utoipa::path]` (path, method, `operation_id`,
  tag, request body, every response including the errors, `security`) and
  is registered with `.routes(routes!(handler))` on the `OpenApiRouter` of
  its module's `router()` (utoipa-axum): the router is the one and only
  list, so a routed endpoint is documented and a documented one is routed
  (`every_documented_operation_is_routed` in `tests/it/api.rs` checks it).
  The path in the annotation is the
  route, relative to where the router is nested (`/api`, so `/sites`, not
  `/api/sites`; handlers with the same path go in one `routes!(a, b)`).
  Request and response bodies are named structs with `ToSchema` (never
  `json!` or a struct declared inside a handler); query parameters are
  `IntoParams`. The non-JSON endpoints (`/tracker.js`, `/api/pixel`,
  `/api/export`, `/api/import`) are documented too. `openapi.rs` only holds
  the title, the tags and the cookie scheme.
- Request bodies are JSON only: handlers take `ApiJson<T>` (`src/extract.rs`),
  which requires `Content-Type: application/json` (a 415 otherwise), answers
  an unreadable body with a 400 `Invalid request body`, and uses the API's
  `{"error": "..."}` shape. Never `body: String` plus manual parsing. The one
  exception is `/api/collect`, which also takes `text/plain` (that is what
  `sendBeacon` sends) and answers 204 to anything; `/api/auth/login` takes
  `Result<ApiJson<_>, ApiError>` so that an unreadable body fails like a
  wrong password. The other exception is `/api/import` (`routes/import.rs`),
  which takes a multipart `file` part or a `text/csv` / `text/plain` body (up
  to 256 MB), reads it whole, checks every row, then inserts in one
  transaction: all or nothing. The export has a `visitor` column and the import
  takes it as an optional one; `hits.visitor` is NULL for a hit without one.
  The JSON endpoints document a 415 in their annotations.
- `GET /api/health` (`routes/health.rs`) answers `{"status", "db"}` after a
  database round-trip, and 503 when the database is unreachable; `potoo
  healthcheck` (the image's health check) asks it.
- **Facet values are lowercase in the database**: `country` (ISO alpha-2),
  `device`, `browser`, `os`, and the referrer's hostname. A path and a click
  label are as the site wrote them. The frontend owns the display names
  (`packages/frontend/src/lib/labels.ts`); filters on the lowercase facets are not
  case sensitive. A value the parsers do not know is **stored as it is**
  (the name it announces), never as a generic `other`, and is logged once
  (`no known name for this value`) so that a migration can name it later; add
  its label to `labels.ts` too (an unlabeled value shows as it is, with a
  console warning). `other` is only for a client that announced nothing.
  A new known family means: the parser, `labels.ts`, and a migration that
  renames what was stored under the old name.
- Flags are booleans in both requests and responses (`is_admin`,
  `honor_dnt`), never 0/1.
- The spec is written by the `specs` binary, built with `--features spec`,
  which is the only build allowed to compile without `packages/frontend/dist`
  (`allow_missing`). Never ship a `spec` build.
- The SDK, `packages/sdk` (the `potoo-sdk` workspace package), is generated
  by hey-api (`packages/sdk/openapi-ts.config.ts`: fetch client, TanStack
  Query, zod, with request/response validation) into `packages/sdk/src`,
  which is gitignored: never edit or commit it. The order is always spec →
  SDK → frontend build → backend build with the frontend embedded (the mise
  tasks keep it). The package scripts `build`, `check` and `dev` do not
  regenerate it: use the mise tasks (`mise run gen:sdk`).
- Frontend code calls the API through the generated query options and
  mutations (`potoo-sdk/react-query`) with TanStack Query,
  and shows errors inline with `errorMessage` from `lib/api.ts`, or as a
  toast (see `packages/frontend/AGENTS.md`). The one
  exception is the single sign-on button on the login page: a plain link to
  `/api/auth/oidc/login`, because the browser must go to the provider.
- `.pnpmfile.cjs` (at the root) gives hey-api its own TypeScript 6: the project
  type-checks with TypeScript 7, which has no compiler API for the generator
  to use.

## Conventions by feature

**The hit path** (`POST /api/collect`, `GET /api/pixel`, in
`routes/collect.rs`): validate → site lookup (in-memory cache) → Do-Not-Track
check → bot check (user agent) → country (local GeoIP) → visitor hash
(cached daily salt) → queue the hit in `ingest.rs` → `204` (the pixel
answers with the GIF). One writer task commits the queue in batches (group
commit; it never waits for a batch to fill). The request is answered
before the hit is on disk.

**Single sign-on** (`oidc.rs`, `routes/oidc.rs`): OpenID Connect through
the `openidconnect` crate, without its default features (it would bring a
second reqwest); our reqwest is handed to it (`Http` in `oidc.rs`, redirects
off). The settings are the `oidc_settings` row with the `OIDC_*`
environment variables on top (a variable locks its field); they live in
memory in `state.oidc` and are replaced when an admin saves, after the
provider answered. A login in progress (state, nonce, PKCE verifier,
callback URL) is not kept on the server: it is the `potoo_oidc` cookie
(`__Host-potoo_oidc` over https), signed with a key made at start
(`oidc.key()`), good for ten minutes, so a flood of logins fills nothing.
The provider's metadata is cached by issuer (the last two) for an hour, and
fetched at once for a token signed with a key it does not have; a provider
that failed to answer is not asked again for 30 seconds, and only one fetch
runs at a time (logins and saves alike). Its endpoints must be https (or on
this machine), and its answers at most 1 MB. A save of the settings runs as
its own task holding `oidc.lock_saves()` from reading the row to putting it
in effect, so a dropped request cannot store it without applying it; a new
issuer drops the stored secret unless one is sent with it. Nobody is signed
in through the provider before the first admin exists (`/setup`, which
calls `POST /api/auth/bootstrap`). A
provider account is linked to a user in `user_identities` (issuer +
subject); an unlinked one is matched only by an email the provider says is
verified, and not to a user linked to another account at that provider.
`OIDC_ONLY` (`oidc.passwords_allowed()`) refuses every password path with
403 (`ApiError::passwords_off`: login, its second step, setting or resetting
a password, a user created with one) but the bootstrap; `password_login` in
`GET /api/auth/status` tells the frontend. The callback URL is `OIDC_REDIRECT_URL`, else under `PUBLIC_URL`, else plain http on the request's host; forwarded headers never count. The
callback never redirects anywhere but `/`, `/login?step=<step>` and
`/login?error=<code>`, and
always clears the login cookie (even one it did not sign), with the
attributes it was set with: a browser ignores a `__Host-` removal without
`Secure`. The tests in `tests/it/oidc.rs` run a stand-in provider (`tests/it/common/`)
that signs real ID tokens with the keys in `tests/it/fixtures/` (test only).

**Two-factor authentication** (`totp.rs` on top of `totp-rs`,
`routes/totp.rs`): `users.totp_secret` (base32, NULL = off),
`totp_pending` (a setup not yet confirmed) and `totp_last_step` (a code
works once: a step is accepted only when it is later, set with a
conditional UPDATE); recovery codes are SHA-256 hashes in `recovery_codes`,
deleted when used. A sign-in (password, or the single sign-on callback)
of a user with it on gets no session but the signed `potoo_2fa` cookie
(`__Host-` over https, `<user id>.<expiry>.<step>`, five minutes, the
`state.challenge_key`, its own): the password answers 200 `LoginStep`, the callback redirects
to `/login?step=totp`; `/login/totp` finishes it. With `TOTP_REQUIRED`
(`state.totp_required`) a user without it gets the `enroll` step instead
(`/login?step=totp-setup`, `/login/totp/setup`, `/login/totp/enable`), and
`/totp/disable` is refused. `next_step` decides, for both paths.

**Limits** (`limits.rs`, governor): every attempt counts, right or wrong,
and over the quota is a 429 with `Retry-After` (`ApiError::TooManyRequests`),
never a lockout. `state.limits.email(email)` (5 per email) and
`state.limits.address(ip)` (30 per address) for a password sign-in (the
address also for the code step and setup at sign-in), and
`state.limits.user(UserAction::Code | Password, id)` for a code or a
password change (5 per user, each its own budget). Any new endpoint that
checks a secret goes through them. The tests get large quotas
(`Limits::generous()`), but for `tests/it/limits.rs`; the quotas are
woodmouse's too: change both. `jobs.rs` calls `retain_recent()` so idle keys
are forgotten.

**Sessions** (`sessions.rs`): tower-sessions keeps them and axum-login
signs users in and out (`AuthSession`). The store is ours (the community's
sqlx store needs sqlx 0.8): the `sessions` table, keyed by the SHA-256 of
the session id, never the id; it writes only when a session changes
(sign-in, sign-out), and expired rows are deleted hourly by `jobs.rs`. Every
sign-in ends in `sessions::sign_in` (a new session id, 30 days); never
create a session another way. axum-login loads the user on each request
(`Backend::get_user`, so a deleted user is signed out at once) and checks
the session against the user's `auth_hash` (the password hash and the TOTP
secret): changing either signs the other sessions out, so after the user
changes their own, call `sessions::refresh` to keep the current one. The
cookie is `potoo_session`, `SameSite=Strict` (`__Host-potoo_session`,
`Secure` when `PUBLIC_URL` is https: `state.secure_cookies`, which decides
for every cookie). The 2FA challenge and the single sign-on login cookies
are separate signed cookies, not sessions.

**Cross-site requests** (`csrf.rs`, a layer outside the sessions): a
POST, PUT, PATCH or DELETE passes with `Sec-Fetch-Site: same-origin` (or
`none`), or, without that header, an `Origin` that is `PUBLIC_URL`'s (or,
without it, matches `Host`; forwarded headers never count), or neither
header (not a browser).
`same-site` is refused on purpose: a sibling subdomain is the same site.
`/api/collect` and Bearer requests skip it. A new endpoint that must take
cross-origin writes (none should) goes in `allowed`, with the reason.

**API keys** (`routes/api_keys.rs`, `key_user` in `auth.rs`): `api_keys`
holds the SHA-256 of `potoo_<64 hex>` and its scope (`read`/`write`/
`admin`). `CurrentUser` (the user, and the key when there is one) takes the
session cookie or `Authorization: Bearer`; a read key gets 403 on anything
but GET/HEAD there, and `require_admin` wants a session or an admin key (and
an admin user). Account endpoints (email, settings, password, TOTP, API
keys) take `SessionOnlyUser`, which refuses keys: use it for any new
endpoint about the account itself. A handler that takes `CurrentUser` lists
`("api_key" = [])` in its `security`. `last_used_at` is written at most once
a minute.
