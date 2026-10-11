# Contributing to Potoo

Potoo is a Rust backend with a React frontend, built to be simple: one
binary, one SQLite file, no external services. Changes that keep it that
way are the changes we want.

## Getting set up

Install [mise](https://mise.jdx.dev) and run `mise install` — it brings
the Rust toolchain, Node and pnpm. Then:

```bash
mise run dev     # the backend on :8000 and the Vite dev server together
mise run seed    # a year of demo data, so the dashboard has something to show
```

Open the Vite URL (Vite proxies `/api` and the tracking endpoints to the
backend).

## Tasks

| Task                       | What it does                                                                                                   |
| -------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `mise run dev`             | Runs the backend and the frontend (Vite) together                                                              |
| `mise run start`           | Builds, then runs the server (never a stale binary)                                                            |
| `mise run gen:spec`        | Writes the OpenAPI spec to `backend/target/specs/openapi.json`                                                 |
| `mise run gen:client`      | Generates the frontend API client from the spec (`frontend/src/client`, not committed)                         |
| `mise run build`           | Builds the frontend and compiles the backend binary                                                            |
| `mise run build:docker`    | Builds the Docker image (podman), natively for your machine                                                    |
| `mise run build:profiling` | The image with per-phase timers and an opt-in CPU flamegraph (never published)                                 |
| `mise run loadtest`        | Write load against the image in a 2 CPU, 2 GB container (`scripts/loadtest.sh`; `TARGET=goatcounter` compares) |
| `mise run check`           | Format, lint and type checks for backend and frontend                                                          |
| `mise run fix`             | Applies the format and lint fixes                                                                              |
| `mise run test`            | The backend tests: end-to-end over HTTP, and unit tests                                                        |
| `mise run seed`            | A year of demo data (wipes every hit; deterministic)                                                           |
| `mise run clean`           | Removes the local data: the database and the GeoIP database                                                    |

Tasks that build something are cached by their `sources` and `outputs` in
`mise.toml`: an unchanged task is skipped.

`mise run check` and `mise run test` must pass before a change can land.
The checks cover formatting (rustfmt, oxfmt), linting (clippy, oxlint) and
types (tsc) for both projects; the tests are end-to-end over real HTTP,
against an in-memory database: setup, login and its throttle, users and
the last-admin rules, sites, collection (bots and Do-Not-Track
included), the stats with a filter, and the export.

## The API and its spec

Every endpoint is documented in code: a `#[utoipa::path]` on the handler, named
request and response structs with `ToSchema`, and the handler registered with
`routes!` on its module's `OpenApiRouter` (utoipa-axum), which is both the route
and the spec entry, so the two cannot drift. The spec is written to `backend/target/specs/openapi.json` by `mise run gen:spec` and is
not served. The frontend's client is generated from it (`mise run gen:client`)
with hey-api, and uses TanStack Query and zod; it is not committed.

## Changing the schema

Add a new numbered file to `backend/migrations/` (`0002_what_it_does.sql`).
It is applied, in order, when the server or the seed starts, and recorded in
the `_sqlx_migrations` table. Never edit a migration that has shipped: its
checksum is recorded, and a changed file stops the server from starting.
The first one is idempotent so that databases created before migrations
existed adopt it unchanged.

## The code

```
backend/src/main.rs        entrypoint: env config, geo refresh, retention, graceful stop
backend/src/lib.rs         the app: routes, cache headers, static serving
backend/src/state.rs       the shared AppState
backend/src/error.rs       HttpError, answered as {"error": "..."}
backend/src/db.rs          the SQLite pools (one writer, read-only readers), caches, migrations on startup
backend/src/ingest.rs      the hit queue and the batched writer
backend/src/profile.rs     per-phase timers, compiled in only with the profiling feature
backend/src/openapi.rs     the OpenAPI document: title, tags, the cookie scheme (paths come from the routers)
backend/src/bin/openapi.rs writes the spec to a file (`mise run gen:spec`)
backend/migrations/        the schema, one numbered SQL file per change (sqlx migrations)
backend/src/auth.rs        password hashing, sessions, the login throttle
backend/src/visitor.rs     user agent parsing, client IP, visitor hashes
backend/src/geo.rs         country lookup + the weekly DB-IP download
backend/src/stats.rs       ranges, chart buckets, the aggregations
backend/src/filter.rs      the RSQL filter subset the dashboard uses
backend/src/csv.rs         the CSV writer + the export download
backend/src/tracker.rs     the tracker script and the pixel GIF
backend/src/util.rs        sha256 and random hex helpers
backend/src/routes/        auth, users, sites, stats, export, collect
backend/src/bin/seed.rs    a year of demo data, for development
backend/examples/          the load generator behind `mise run loadtest`
backend/tests/api.rs       the end-to-end tests
scripts/                   the load test script and its generator image
frontend/src/routes/       dashboard, sites table, site page, users, settings
frontend/src/components/   the shared UI pieces
frontend/src/client/       the generated API client (hey-api; gitignored, never edited)
frontend/src/lib/          error messages, ranges, filters, date formatting
```

## The rules a change must keep

Potoo's whole value is its privacy promise; a change that breaks it is
not a change we can take. Concretely:

- **No cookies, no localStorage, no fingerprinting** of visitors, ever.
- **No personal data at rest.** The IP and the user agent exist in
  memory only, to derive the country, device, browser and operating
  system, and the per-day, per-site visitor hash. They are never
  written down.
- **Paths are stripped** of their query string and fragment before
  storage; referrers are reduced to the hostname.
- **Visitors cannot be linked** across days or sites: the visitor hash
  is salted with a daily salt, and old salts are deleted.

## Conventions

- **Endpoint layout.** Static files are served from the root with
  their file ending (`/tracker.js`); everything that writes to the
  database or processes anything lives under `/api/`.
- **One shared range spec.** Every endpoint that filters by time takes
  the same params — `range` as an ISO 8601 duration, or `from`/`to` as
  ISO timestamps, plus `sites` and the `q` filter — parsed by the same
  code in `stats.rs` and `routes/sites.rs`. Add a new time-filtered
  endpoint by reusing them, not by inventing a new syntax.
- **Keep it simple.** SQLite through sqlx with run-time queries (one
  writer connection, read-only readers), no ORM, no abstraction layers. If a change needs a new dependency or a
  new concept, first check whether the naive version is enough.
- **Comment the why, not the what** — the reasoning behind a decision
  is the part the code cannot show.

## Releasing

Releases are automated with [release-please](https://github.com/googleapis/release-please),
driven by the commit messages on `main`. Write them as
[Conventional Commits](https://www.conventionalcommits.org): `feat:` bumps
the minor version, `fix:` the patch, `feat!:` (or a `BREAKING CHANGE:`
footer) the major; `ci:`, `chore:`, `docs:` and the like release nothing.

1. Every push to `main` updates one open "release PR": the version in
   `backend/Cargo.toml`, `backend/Cargo.lock` and `frontend/package.json`,
   and the changelog.
2. Merge that PR when you want to ship. The release (tag `v1.2.3`) is
   created, and the release workflow builds the Linux binaries (static
   musl, and glibc suffixed `-gnu`, for amd64 and arm64) and attaches them,
   and pushes the multi-arch image as `1.2.3`, `1.2` and `latest`.

`CHANGELOG.md` is written by release-please: do not edit it by hand. The
release PR needs "Allow GitHub Actions to create and approve pull
requests" turned on in the repository's Actions settings.

## Submitting

Small commits, one logical change each, with a conventional message (see
Releasing). The backend and the
frontend are separately checkable (`mise run check:backend`,
`mise run check:frontend`) — a change that only touches one should not
leave the other dirty.
