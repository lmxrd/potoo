# Potoo — notes for coding agents

Potoo is a small, privacy-friendly, self-hosted web analytics tool: a Rust
backend (axum, SQLite through sqlx) with a React frontend embedded in the
one binary. README.md is the user documentation and CONTRIBUTING.md the
human contributor guide (task list, code layout, conventions); read both
for the full picture. This file is what an agent must know before editing.

## Commands

Everything goes through [mise](https://mise.jdx.dev) (`mise install` once;
it provides Rust, Node and pnpm at the pinned versions).

| Command | Use |
| --- | --- |
| `mise run check` | rustfmt, clippy (`-D warnings`), oxfmt, oxlint, tsc. **Must pass.** |
| `mise run test` | The backend tests. **Must pass.** |
| `mise run fix` | Applies the format and lint fixes |
| `mise run dev` | Backend on :8000 plus the Vite dev server |
| `mise run seed` | A year of demo data (wipes every hit) into `data/potoo.db` |
| `mise run audit` / `audit:image` | Known vulnerabilities in the lockfiles (osv-scanner) / in an image's Alpine packages (Trivy; `IMAGE=`, by default the published one) |
| `mise run gen:spec` | Writes the OpenAPI spec to `packages/backend/target/specs/openapi.json` |
| `mise run gen:sdk` | Generates the frontend API client (`packages/sdk/src`) from the spec |
| `mise run build` / `build:linux` / `build:docker` | Release binary (`dist/potoo`) / static Linux binary (`dist/potoo-linux-<arch>`, zig cross-compiles) / image (podman) that copies it |

Run `mise run check` and `mise run test` before you call a change done
(`--force` runs a task that would be skipped).
Tasks with `sources`/`outputs` in `mise.toml` are skipped when unchanged,
so a repeat run is instant. Use `DATABASE_PATH=/some/tmp/potoo.db` when
you run the server or the seed and must not touch `data/`, and
`GEOIP_URL=` (empty) so the server does not download the GeoIP database.
The environment variables are in the README's Configuration table.

## Architecture you must not break

**The hit path** (`POST /api/collect`, `GET /api/pixel`, in
`routes/collect.rs`): validate → site lookup (in-memory cache) → Do-Not-Track
check → bot check (user agent) → country (local GeoIP) → visitor hash
(cached daily salt) → queue the hit in `ingest.rs` → `204` (the pixel
answers with the GIF). One writer task commits the queue in batches (group
commit; it never waits for a batch to fill). The request is answered
before the hit is on disk.

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
- `users.password_hash` is NULL for a user who has no password (single
  sign-on only): anything that reads it must treat NULL as "no password",
  never as a hash.

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
in through the provider before the first admin exists (`/setup`). A
provider account is linked to a user in `user_identities` (issuer +
subject); an unlinked one is matched only by an email the provider says is
verified, and not to a user linked to another account at that provider.
`OIDC_ONLY` (`oidc.passwords_allowed()`) refuses every password path with
403 (`HttpError::passwords_off`: login, its second step, setting or resetting
a password, a user created with one) but `/setup`; the session's
`password_login` tells the frontend. Forwarded headers count for the callback URL only with `TRUST_PROXY`. The
callback never redirects anywhere but `/`, `/login?step=<step>` and
`/login?error=<code>`, and
always clears the login cookie (even one it did not sign), with the
attributes it was set with: a browser ignores a `__Host-` removal without
`Secure`. The tests in `tests/api.rs` run a stand-in provider
that signs real ID tokens with the keys in `tests/fixtures/` (test only).

**Two-factor authentication** (`totp.rs` on top of `totp-rs`,
`routes/totp.rs`): `users.totp_secret` (base32, NULL = off),
`totp_pending` (a setup not yet confirmed) and `totp_last_step` (a code
works once: a step is accepted only when it is later, set with a
conditional UPDATE); recovery codes are SHA-256 hashes in `recovery_codes`,
deleted when used. A sign-in (password, or the single sign-on callback)
of a user with it on gets no session but the signed `potoo_2fa` cookie
(`__Host-` over https, `<user id>.<expiry>.<step>`, five minutes, the
`oidc.key()`): the password answers 200 `LoginStep`, the callback redirects
to `/login?step=totp`; `/login/totp` finishes it. With `TOTP_REQUIRED`
(`state.totp_required`) a user without it gets the `enroll` step instead
(`/login?step=totp-setup`, `/login/totp/setup`, `/login/totp/enable`), and
`/totp/disable` is refused. `next_step` decides, for both paths. Wrong codes count in `state.throttle` per user
(`totp|<id>`), not per address.

**Sessions** (`session.rs`): tower-sessions keeps them and axum-login
signs users in and out (`AuthSession`). The store is ours (the community's
sqlx store needs sqlx 0.8): the `sessions` table, keyed by the SHA-256 of
the session id, never the id; it writes only when a session changes
(sign-in, sign-out), and expired rows are deleted hourly by `main`. Every
sign-in ends in `session::sign_in` (a new session id, 30 days); never
create a session another way. axum-login loads the user on each request
(`Backend::get_user`, so a deleted user is signed out at once) and checks
the session against the user's `auth_hash` (the password hash and the TOTP
secret): changing either signs the other sessions out, so after the user
changes their own, call `session::refresh` to keep the current one. The
cookie is `potoo_session`, `SameSite=Strict`, `Secure` with
`SECURE_COOKIES`. The 2FA challenge and the single sign-on login cookies
are separate signed cookies, not sessions.

**Cross-site requests** (`csrf.rs`, a layer outside the sessions): a
POST, PUT, PATCH or DELETE passes with `Sec-Fetch-Site: same-origin` (or
`none`), or, without that header, an `Origin` matching the host (the
forwarded one with `TRUST_PROXY`), or neither header (not a browser).
`same-site` is refused on purpose: a sibling subdomain is the same site.
`/api/collect` and Bearer requests skip it. A new endpoint that must take
cross-origin writes (none should) goes in `allowed`, with the reason.

**API keys** (`routes/api_keys.rs`, `key_user` in `auth.rs`): `api_keys`
holds the SHA-256 of `potoo_<64 hex>` and its scope (`read`/`write`/
`admin`). `AuthUser(user, Access)` takes the session cookie or
`Authorization: Bearer`; a read key gets 403 on anything but GET/HEAD there,
and `require_admin` wants a session or an admin key (and an admin user).
Account endpoints (email, settings, password, TOTP, API keys) take
`SessionUser`, which refuses keys: use it for any new endpoint about the
account itself. A handler that takes `AuthUser` lists `("api_key" = [])` in
its `security`. `last_used_at` is written at most once a minute.

**Tests run on an in-memory database**, where the reader and the writer
pools are one and the same single connection. Two consequences: a write
made through `db.reader()` (read-only on a real file) passes the tests but
fails in production; and a read on `db.reader()` while a writer
transaction is open works in production but deadlocks in the tests. The
test in `db.rs` `readers_see_the_writes_and_do_not_block_the_writer` uses a
real file; add one when you change how the pools are used.

**The API is described by OpenAPI, and the frontend client is generated from
it** (nothing is committed or served):

- Every handler has a `#[utoipa::path]` (path, method, `operation_id`,
  tag, request body, every response including the errors, `security`) and
  is registered with `.routes(routes!(handler))` on the `OpenApiRouter` of
  its module's `router()` (utoipa-axum): the router is the one and only
  list, so a routed endpoint is documented and a documented one is routed
  (a test in `tests/api.rs` checks it). The path in the annotation is the
  route, relative to where the router is nested (`/api`, so `/sites`, not
  `/api/sites`; handlers with the same path go in one `routes!(a, b)`).
  Request and response bodies are named structs with `ToSchema` (never
  `json!` or a struct declared inside a handler); query parameters are
  `IntoParams`. The non-JSON endpoints (`/tracker.js`, `/api/pixel`,
  `/api/export`, `/api/import`, `/api/health`) are documented too. `openapi.rs` only holds
  the title, the tags and the cookie scheme.
- Request bodies are JSON only: handlers take `ApiJson<T>` (`src/extract.rs`),
  which requires `Content-Type: application/json` (a 415 otherwise), answers
  an unreadable body with a 400 `Invalid request body`, and uses the API's
  `{"error": "..."}` shape. Never `body: String` plus manual parsing. The one
  exception is `/api/collect`, which also takes `text/plain` (that is what
  `sendBeacon` sends) and answers 204 to anything; `/api/auth/login` takes
  `Result<ApiJson<_>, HttpError>` so that an unreadable body fails like a
  wrong password. The other exception is `/api/import` (`routes/import.rs`),
  which takes a multipart `file` part or a `text/csv` / `text/plain` body (up
  to 256 MB), reads it whole, checks every row, then inserts in one
  transaction: all or nothing. The export has a `visitor` column and the import
  takes it as an optional one; `hits.visitor` is NULL for a hit without one.
  The JSON endpoints document a 415 in their annotations.
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
- The spec is written by the `openapi` binary, built with `--features spec`,
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
  and shows errors with `errorMessage` from `lib/api.ts`. The one
  exception is the single sign-on button on the login page: a plain link to
  `/api/auth/oidc/login`, because the browser must go to the provider.
- `.pnpmfile.cjs` (at the root) gives hey-api its own TypeScript 6: the project
  type-checks with TypeScript 7, which has no compiler API for the generator
  to use.

**rust-embed** needs `packages/frontend/dist` to exist to compile, even in debug
(the mise tasks that compile the backend build the frontend first). Do not
run raw `cargo` commands on a fresh checkout without `mise run
build:frontend`.

## Privacy rules (non-negotiable)

No cookies or localStorage for visitors, no fingerprinting. The IP and the
user agent exist in memory only, to derive country, device, browser, OS
and the per-day, per-site visitor hash — never stored, logged or queued.
Single sign-on is for dashboard users only and changes none of this.
The one thing taken from a user agent that is not a known family is the
short name of the product or platform it announces (`product_name` /
`platform_name` in `visitor.rs`): at most 32 plain characters, lowercase,
no version, nothing else of the user agent.
Paths lose their query string and fragment; referrers are reduced to the
hostname; old salts are deleted. A change that breaks any of this is not
acceptable.

## Conventions

- **Simple.** No ORM, no abstraction layers. Comment the *why*, not the
  what.
- **Errors.** Handlers return `HttpError` (`src/error.rs`, thiserror):
  `HttpError::new(status, message)` and its helpers for what the client
  may read; a database error (`?` on sqlx) or `HttpError::internal(...)` is
  logged with its causes and answered with a plain 500, never its text. The
  binaries (main, the seed, the spec writer, `potoo healthcheck`) use
  anyhow with `.context(...)`.
- **Use the community's crates.** When a well-known, maintained crate (or
  npm package) solves the problem, use it rather than writing the same
  thing ourselves: sessions are axum-login and tower-sessions, errors
  thiserror and anyhow. Propose a new one first (name, version, why) and
  add it only once it is approved. Write it ourselves only when nothing
  maintained fits, and say why in a comment (the session store: the sqlx
  store crate needs sqlx 0.8).
- **Endpoints.** Static files at the root (`/tracker.js`); everything that
  writes or processes lives under `/api/`. Settings of the whole instance
  (single sign-on so far) are under `/api/admin/`, tagged `Admin`.
- **One range spec.** Time-filtered endpoints take `range` (ISO 8601
  duration) or `from`/`to`, plus `sites` and `q`; reuse `parse_range` /
  `range_and_filter`, do not invent a new syntax.
- **Duplicated lists.** The date and time format lists are in
  `packages/backend/src/auth.rs` (validation) and `packages/frontend/src/lib/dates.ts`
  (labels): change both together. So are the single sign-on error codes:
  `Failure` in `packages/backend/src/oidc.rs` and `SSO_ERRORS` in
  `packages/frontend/src/routes/login.tsx`.
- **Versions.** Node, pnpm, Rust, zig, cargo-zigbuild, osv-scanner and
  Trivy are pinned in `mise.toml`; `packages/frontend/package.json`
  (`packageManager`) must match pnpm. The image builds nothing: it is an
  exact Alpine release (`alpine:3.x.y`, with `apk upgrade`) and the binary
  built outside it (`scripts/build-linux.sh`), so no toolchain version lives
  in the Dockerfile. Its healthcheck is `potoo healthcheck` (no wget).
- **Dependency updates and security fixes** (`.github/dependabot.yml`):
  Dependabot's pull requests are `fix(deps)` for what ships (crates, the
  frontend's dependencies, the base image), so merging one releases a
  patch, and `chore(deps-dev)` for the frontend's devDependencies; `ci` for
  actions. Routine updates wait 5 days after their release (`cooldown`),
  security updates do not. Minor, patch and security updates merge on
  their own once CI passes (`dependabot-auto-merge.yml`); majors wait for a
  person. That merge is made with a workflow token and so starts no
  workflow: `release.yml` also runs daily to pick it up. pnpm's own
  `minimumReleaseAge` (1 day, `packages/frontend/pnpm-workspace.yaml`) must stay
  shorter than the cooldown, or it would block Dependabot's updates.
  `security.yml` runs `mise run audit` and scans the published image every
  day, CI scans the image it builds; a finding to accept goes in
  `osv-scanner.toml` with its reason.
- **The image copies a prebuilt binary.** `mise run build:linux` builds
  `dist/potoo-linux-<arch>` with the machine's cargo cache and target
  directory (copied out of it: cargo has no stable flag to put one artifact
  elsewhere), and the Dockerfile copies it in. `.dockerignore` lets only
  those binaries of `dist/` through; the load generator's image still
  builds from the sources in the same context.
- **Generated, do not edit by hand:** `CHANGELOG.md` and the version
  fields (release-please), `packages/frontend/src/routeTree.gen.ts` (TanStack
  Router), `packages/frontend/pnpm-lock.yaml` and `packages/backend/Cargo.lock` (use the
  package managers).
- **Frontend:** React 19, TanStack Router (file-based), Tailwind 4, visx
  for the chart (SVG, styled only with the theme classes such as `fill-accent`,
  never hard-coded colors, so a theme or accent change shows at once), oxlint
  and oxfmt, TypeScript. There are **no frontend tests yet** (planned);
  the backend end-to-end tests in `packages/backend/tests/api.rs` cover the API.

## Git and releases

Commit messages are **Conventional Commits** (`feat:`, `fix:`, `perf:`,
`ci:`, `docs:`, ...): release-please reads them to bump the version and
write the changelog. So are **pull request titles**: pull requests are
squash merged, the commit on main takes the title, and release-please
skips one that is not conventional (`Feat/import visitor (#6)` released
nothing). GitHub fills a new pull request's title from the branch name:
rewrite it. The `PR title` workflow fails a title that does not parse. Do not commit unless asked, and work on a branch, not
`main`. Releases are automated (see CONTRIBUTING.md); never tag by hand.

## Manual tooling (not run by CI)

`mise run loadtest` (`scripts/loadtest.sh`, `scripts/loadtest.Containerfile`,
`packages/backend/examples/loadtest.rs`), `mise run build:profiling` and the
`profiling` cargo feature (per-phase timers, opt-in flamegraph) are
**manual only**. Do not run the load test on your own: it builds images,
starts containers and takes minutes. Run it only when the user asks.
(`TARGET=goatcounter` compares against GoatCounter.)

They can rot unnoticed, so after a change to the hit path, `ingest.rs`,
`db.rs` or the Dockerfile, at least check that they still compile:
`cargo clippy --all-targets --features profiling`. If you changed
something performance-sensitive, say that the load test is worth a run;
leave it to the user to ask for it.

On an Apple Silicon Mac podman cannot build amd64 images reliably (QEMU
segfaults); images build natively for the host architecture.
