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
| `mise run gen:spec` | Writes the OpenAPI spec to `backend/target/specs/openapi.json` |
| `mise run gen:client` | Generates the frontend API client (`frontend/src/client`) from the spec |
| `mise run build` / `build:docker` | Release binary / image (podman) |

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
- Schema changes are **new numbered files** in `backend/migrations/`.
  Never edit one that has shipped (its checksum is recorded). The first is
  idempotent so pre-migration databases adopt it.
- The site lookup and the daily salt are cached in `Database`. After any
  change to a site, call `db.forget_sites()`.
- A clean stop (SIGTERM/SIGINT) drains the queue and closes the pools; a
  crash loses the hits still queued. A full queue answers `503`.

**Tests run on an in-memory database**, where the reader and the writer
pools are one and the same single connection. Two consequences: a write
made through `db.reader()` (read-only on a real file) passes the tests but
fails in production; and a read on `db.reader()` while a writer
transaction is open works in production but deadlocks in the tests. The
two tests in `db.rs` (`readers_see_the_writes_and_do_not_block_the_writer`,
`a_database_from_before_migrations_is_adopted`) use a real file; add one
when you change how the pools are used.

**The API is described by OpenAPI, and the frontend client is generated from
it** (nothing is committed or served):

- Every handler has a `#[utoipa::path]` (path, method, `operation_id`,
  tag, request body, every response including the errors, `security`), and
  is listed in `paths(...)` in `openapi.rs`. Request and response bodies are
  named structs with `ToSchema` (never `json!` or a struct declared inside
  a handler); query parameters are `IntoParams`. `openapi.rs` has a test
  with the list of expected endpoints: adding or removing one means changing
  it. The non-JSON endpoints (`/tracker.js`, `/api/pixel`, `/api/export`,
  `/api/health`) are documented too.
- Flags are booleans in both requests and responses (`is_admin`,
  `honor_dnt`), never 0/1.
- The spec is written by the `openapi` binary, built with `--features spec`,
  which is the only build allowed to compile without `frontend/dist`
  (`allow_missing`). Never ship a `spec` build.
- The frontend client is generated by hey-api (`frontend/openapi-ts.config.ts`:
  fetch client, TanStack Query, zod, with request/response validation)
  into `frontend/src/client`, which is gitignored: never edit or commit it.
  The order is always spec → client → frontend build → backend build with
  the frontend embedded (the Dockerfile has a `spec` stage for the first).
  The package scripts `build`, `check` and `dev` do not regenerate it: use
  the mise tasks, or `pnpm run gen:api` after `mise run gen:spec`.
- Frontend code calls the API through the generated query options and
  mutations (`../client/@tanstack/react-query.gen`) with TanStack Query,
  and shows errors with `errorMessage` from `lib/api.ts`.
- `frontend/.pnpmfile.cjs` gives hey-api its own TypeScript 6: the project
  type-checks with TypeScript 7, which has no compiler API for the generator
  to use.

**rust-embed** needs `frontend/dist` to exist to compile, even in debug
(the mise tasks that compile the backend build the frontend first). Do not
run raw `cargo` commands on a fresh checkout without `mise run
build:frontend`.

## Privacy rules (non-negotiable)

No cookies or localStorage for visitors, no fingerprinting. The IP and the
user agent exist in memory only, to derive country, device, browser, OS
and the per-day, per-site visitor hash — never stored, logged or queued.
Paths lose their query string and fragment; referrers are reduced to the
hostname; old salts are deleted. A change that breaks any of this is not
acceptable.

## Conventions

- **Simple.** No ORM, no abstraction layers, no new dependency without
  checking the naive version first. Comment the *why*, not the what.
- **Endpoints.** Static files at the root (`/tracker.js`); everything that
  writes or processes lives under `/api/`.
- **One range spec.** Time-filtered endpoints take `range` (ISO 8601
  duration) or `from`/`to`, plus `sites` and `q`; reuse `parse_range` /
  `range_and_filter`, do not invent a new syntax.
- **Duplicated lists.** The date and time format lists are in
  `backend/src/auth.rs` (validation) and `frontend/src/lib/dates.ts`
  (labels): change both together.
- **Versions.** Node, pnpm and Rust are pinned in `mise.toml`; the Docker
  image (`node:24`, `rust:1.99`) and `frontend/package.json`
  (`packageManager`) must match them — change them together.
- **Generated, do not edit by hand:** `CHANGELOG.md` and the version
  fields (release-please), `frontend/src/routeTree.gen.ts` (TanStack
  Router), `frontend/pnpm-lock.yaml` and `backend/Cargo.lock` (use the
  package managers).
- **Frontend:** React 19, TanStack Router (file-based), Tailwind 4, oxlint
  and oxfmt, TypeScript. There are **no frontend tests yet** (planned);
  the backend end-to-end tests in `backend/tests/api.rs` cover the API.

## Git and releases

Commit messages are **Conventional Commits** (`feat:`, `fix:`, `perf:`,
`ci:`, `docs:`, ...): release-please reads them to bump the version and
write the changelog. Do not commit unless asked, and work on a branch, not
`main`. Releases are automated (see CONTRIBUTING.md); never tag by hand.

## Manual tooling (not run by CI)

`mise run loadtest` (`scripts/loadtest.sh`, `scripts/loadtest.Containerfile`,
`backend/examples/loadtest.rs`), `mise run build:profiling` and the
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
