# Contributing to Potoo

Potoo is a Rust backend with a React frontend, built to be simple: one
binary, one SQLite file, no external services. Changes that keep it that
way are the changes we want.

## Getting set up

Install [mise](https://mise.jdx.dev) and run `mise install` — it brings
the Rust toolchain and pnpm. Then:

```bash
mise run dev     # the backend on :8000 and the Vite dev server together
mise run seed    # a year of demo data, so the dashboard has something to show
```

Open the Vite URL (Vite proxies `/api` and the tracking endpoints to the
backend).

## Tasks

| Task                    | What it does                                                 |
| ----------------------- | ------------------------------------------------------------ |
| `mise run dev`          | Runs the backend and the frontend (Vite) together            |
| `mise run start`        | Runs the server from source, against a built frontend         |
| `mise run build`        | Builds the frontend and compiles the backend binary            |
| `mise run build:docker` | Builds the Docker image                                       |
| `mise run check`        | Format, lint and type checks for backend and frontend         |
| `mise run fix`          | Applies the format and lint fixes                             |
| `mise run test`         | The backend end-to-end tests                                  |
| `mise run seed`         | A year of demo data (wipes every hit; deterministic)          |
| `mise run clean`        | Removes the local data: the database and the GeoIP database   |

`mise run check` and `mise run test` must pass before a change can land.
The checks cover formatting (rustfmt, oxfmt), linting (clippy, oxlint) and
types (tsc) for both projects; the tests are end-to-end over real HTTP,
against an in-memory database: setup, login and its throttle, users and
the last-admin rules, sites, collection (bots and Do-Not-Track
included), the stats with a filter, and the export.

## The code

```
backend/src/main.rs        entrypoint: env config, geo refresh, retention
backend/src/lib.rs         the app: routes, cache headers, static serving
backend/src/db.rs          the SQLite connection + schema, applied on startup
backend/src/auth.rs        password hashing, sessions, the login throttle
backend/src/visitor.rs     user agent parsing, client IP, visitor hashes
backend/src/geo.rs         country lookup + the weekly DB-IP download
backend/src/stats.rs       ranges, chart buckets, the aggregations
backend/src/filter.rs      the RSQL filter subset the dashboard uses
backend/src/csv.rs         the CSV writer + the export download
backend/src/tracker.rs     the tracker script and the pixel GIF
backend/src/routes/        auth, users, sites, stats, export, collect
backend/src/bin/seed.rs    a year of demo data, for development
frontend/src/routes/       dashboard, sites table, site page, users, settings
frontend/src/lib/          the API client, ranges, filters, date formatting
```

The database schema lives in `db.rs` and is applied (idempotently) on
startup — there is no migration tooling to run.

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
- **Keep it simple.** One SQLite connection behind an async mutex, no
  ORM, no abstraction layers. If a change needs a new dependency or a
  new concept, first check whether the naive version is enough.
- **Comment the why, not the what** — the reasoning behind a decision
  is the part the code cannot show.

## Submitting

Small, described commits; one logical change each. The backend and the
frontend are separately checkable (`mise run check:backend`,
`mise run check:frontend`) — a change that only touches one should not
leave the other dirty.
