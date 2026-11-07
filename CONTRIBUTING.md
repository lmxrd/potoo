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
| `mise run gen:spec`        | Writes the OpenAPI spec to `packages/backend/target/specs/` (JSON and YAML)                                             |
| `mise run gen:sdk`      | Generates the frontend API client from the spec (`packages/sdk/src`, not committed)                         |
| `mise run build`           | Builds the frontend and the release binary for this machine, copied to `dist/potoo`                            |
| `mise run build:linux`     | The static Linux binary the image copies, `dist/potoo-linux-<arch>` (zig cross-compiles; `ARCH=amd64`/`arm64`) |
| `mise run build:docker`    | `build:linux`, then the Docker image (podman), which only copies that binary                                   |
| `mise run build:profiling` | The image with per-phase timers and an opt-in CPU flamegraph (never published)                                 |
| `mise run loadtest`        | Write load against the image in a 2 CPU, 2 GB container (`scripts/loadtest.sh`; `TARGET=goatcounter` compares) |
| `mise run check`           | Format, lint and type checks for backend and frontend                                                          |
| `mise run audit`           | Known vulnerabilities in `Cargo.lock` and `pnpm-lock.yaml` (osv-scanner; also daily in CI)                     |
| `mise run audit:image`     | Known vulnerabilities in the image's Alpine packages (Trivy); `IMAGE=` picks one, by default the published one |
| `mise run fix`             | Applies the format and lint fixes                                                                              |
| `mise run test`            | Unit and integration tests: the backend's (cargo, axum-test) and the frontend's (Vitest)                       |
| `mise run test:e2e`        | The end-to-end smoke tests: builds the image, then a real browser goes through it (Playwright)                 |
| `mise run seed`            | A year of demo data (wipes every hit; deterministic)                                                           |
| `mise run clean`           | Removes the local data: the database and the GeoIP database                                                    |

Tasks that build something are cached by their `sources` and `outputs` in
`mise.toml`: an unchanged task is skipped.

`mise run check` and `mise run test` must pass before a change can land.
The checks cover formatting (rustfmt, oxfmt), linting (clippy, oxlint) and
types (tsc) for both projects. The tests come in three layers:

- **Unit tests** next to the code (Rust `#[cfg(test)]`, and Vitest for the
  frontend's `src/lib`).
- **Integration tests** of the API (`packages/backend/api/tests/it/`, one file per area, on
  axum-test, against an in-memory database): setup, login and its limits,
  two-factor, sessions, single sign-on with a stand-in provider, API keys,
  CSRF, users and the last-admin rules, sites, collection (bots and
  Do-Not-Track included), the stats with a filter, the export and import.
- **End-to-end smoke tests** (`packages/e2e`, Playwright): a few flows a
  real browser runs against the built image. CI runs all three.

## The repository

The JavaScript side is a pnpm workspace with one lockfile at the root.

```
packages/backend/    the Rust server: axum, utoipa (OpenAPI) and sqlx on SQLite
packages/frontend/   the React app (Vite, TanStack Router and Query, Tailwind)
packages/sdk/        the frontend's API client, generated from the OpenAPI spec (hey-api; gitignored, never edited)
packages/e2e/        the end-to-end smoke tests (Playwright), run against the built image
scripts/             the Linux build, the end-to-end run (e2e.sh), the load test and its generator image
.github/             CI, the release workflow, Dependabot and the security scans
mise.toml            the tools (other than Node, pnpm, Rust) and every task
package.json         the Node (devEngines.runtime) and pnpm (packageManager) versions
rust-toolchain.toml  the Rust toolchain
```

Inside the backend, a Cargo workspace of two crates: the server binary `potoo` (the root) and the
library `api` (`api/`) with everything else, so the library and the `specs` tool build without the frontend:

```
src/main.rs              the `potoo` binary: env config, swagger-ui (SWAGGER_UI), the background jobs, graceful stop, `potoo healthcheck`
src/frontend.rs          the embedded frontend bundle (only this binary needs `packages/frontend/dist`): static files, cache headers, the SPA fallback
api/src/lib.rs           the `api` library crate the server and the tools share, and the TLS provider
api/src/config.rs        settings read from the environment
api/src/state.rs         the shared AppState
api/src/error.rs         ApiError, answered as {"error": "..."}
api/src/extract.rs       ApiJson: the JSON request body extractor
api/src/db.rs            the SQLite pools (one writer, read-only readers), caches, migrations on startup
api/src/ingest.rs        the hit queue and the batched writer
api/src/jobs.rs          the background jobs: limits, expired sessions, retention, the GeoIP refresh, the checkpoint on stop
api/src/profile.rs       per-phase timers, compiled in only with the profiling feature
api/src/openapi.rs       the OpenAPI document: title, tags, the cookie scheme (paths come from the routers)
api/src/auth.rs          password hashing, API keys, the signed-in user (CurrentUser, SessionOnlyUser), sha256_hex
api/src/sessions.rs      the session store and signing in and out (axum-login on tower-sessions)
api/src/csrf.rs          the cross-site request check (Fetch Metadata)
api/src/limits.rs        the sign-in limits (governor)
api/src/totp.rs          two-factor authentication: codes, the stored state, the signed challenge cookie
api/src/oidc.rs          single sign-on: the settings (env over stored), the OIDC flow, the signed login cookie
api/src/visitor.rs       user agent parsing, client IP, visitor hashes
api/src/geo.rs           country lookup + the weekly DB-IP download
api/src/stats.rs         ranges, chart buckets, the aggregations
api/src/filter.rs        the RSQL filter subset the dashboard uses
api/src/csv.rs           the CSV writer + the export download (the import reads CSV in routes/import.rs)
api/src/tracker.rs       the tracker script and the pixel GIF
api/src/time.rs          dates and times (jiff), stored as UTC text
api/src/random.rs        random bytes for secrets (`random::hex`)
api/src/routes/          the HTTP handlers, one module per area (`routes::api` puts them together with their layers, `routes::api_doc` is their OpenAPI document): auth (sign-in, your account, the admin's users), totp, api_keys, oidc (login, callback, admin settings), sites, stats, export, import, collect, health
api/src/bin/             tools that are not part of the release build: `seed` (a year of demo data), `specs` (the OpenAPI spec)
api/examples/            the load generator behind `mise run loadtest`
api/migrations/          the schema, one numbered SQL file per change (sqlx migrations)
api/tests/it/            the integration tests, one file per area (axum-test; common/ holds the shared helpers and a stand-in OpenID provider, its test keys in fixtures/)
```

Inside the frontend's `src/`: `routes/` (the dashboard, the sites table, a
site's page, the import, sign-in and setup, and the settings with the admin
pages under `settings.admin.*`), `components/` (the UI, grouped by feature),
and `lib/` (error messages, ranges, filters, date formatting, themes).

Each stack has its own `AGENTS.md` with the detailed conventions: read
[the backend's](packages/backend/AGENTS.md) and
[the frontend's](packages/frontend/AGENTS.md) before a larger change.

## The API and its spec

Every endpoint is documented in code: a `#[utoipa::path]` on the handler, named
request and response structs with `ToSchema`, and the handler registered with
`routes!` on its module's `OpenApiRouter` (utoipa-axum), which is both the route
and the spec entry, so the two cannot drift. `mise run gen:spec` writes the spec
to `packages/backend/target/specs/` (`openapi.json` and `openapi.yaml`); the
server serves it only with `SWAGGER_UI=true`, at `/swagger-ui`. The frontend's
client is generated from it (`mise run gen:sdk`) with hey-api, and uses
TanStack Query and zod; it is not committed.

## Changing the schema

Add a new numbered file to `packages/backend/api/migrations/` (`0007_what_it_does.sql`).
It is applied, in order, when the server or the seed starts, and recorded in
the `_sqlx_migrations` table. Never edit a migration that has shipped: its
checksum is recorded, and a changed file stops the server from starting.

SQLite cannot change a column: a change like that rebuilds the table, with
foreign keys off, or `ON DELETE CASCADE` empties the tables that point at it.
`0002_oidc.sql` shows how (`-- no-transaction`, then its own transaction), and
`db.rs` has a test that runs it on a real file with data in it.

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
  the same params — `from`/`to` as ISO 8601 timestamps, plus `sites` and
  the `q` filter — parsed by the same code (`parse_range` in `stats.rs`,
  `range_and_filter` in `routes/sites.rs`). Add a new time-filtered
  endpoint by reusing them, not by inventing a new syntax.
- **Keep it simple.** SQLite through sqlx with run-time queries (one
  writer connection, read-only readers), no ORM, no abstraction layers.
- **Use what the community maintains.** Prefer a well-known, maintained
  crate or npm package over writing the same thing ourselves. Propose it in
  the issue or pull request (name, exact version, why) and get a
  maintainer's approval before adding it. Write it ourselves only when no
  maintained one fits, and say why in a comment where it lives.
- **Comment the why, not the what** — the reasoning behind a decision
  is the part the code cannot show.

## Pull requests, commits and releases

Small commits, one logical change each, with a conventional message. The
backend and the frontend are separately checkable (`mise run check:backend`,
`mise run check:frontend`) — a change that only touches one should not
leave the other dirty.

Releases are automated with [release-please](https://github.com/googleapis/release-please),
driven by the commit messages on `main`. Write them as
[Conventional Commits](https://www.conventionalcommits.org): `feat:` bumps
the minor version, `fix:` the patch, and `feat!:` (or a `BREAKING CHANGE:`
footer) the major. `ci:`, `chore:`, `docs:` and the like release nothing.

Pull requests are squash merged with the pull request's title as the commit
message, so **the title must be a Conventional Commit** (`feat: add ...`,
`fix(auth): ...`), not the branch name GitHub fills in. A `PR title` check fails
the pull request otherwise, and release-please skips a commit that is not
conventional: nothing is released for it, and the log says "commit could
not be parsed".

Every push to `main` updates one open release PR with the version bump in
`packages/backend/Cargo.toml` and `Cargo.lock`, `packages/frontend/package.json`,
`packages/sdk/package.json` and `packages/e2e/package.json`, plus the
changelog. Merging it tags the release (`v1.2.3`), and the release workflow:

- builds the static Linux binaries for `amd64` and `arm64` natively, on a
  runner of each architecture, and attaches them to the release with the
  OpenAPI spec (JSON and YAML);
- writes one `SHA256SUMS` for everything attached (both binaries and the spec);
- packages those same binaries into the Docker image and pushes it to
  `ghcr.io/lmxrd/potoo` as one multi-arch image, tagged `1.2.3`, `1.2` and
  `latest`.

Dependencies update themselves: Dependabot opens weekly pull requests (and
one at once for a security advisory), and the minor, patch and security
ones merge on their own when CI passes; majors wait for review. Those that
ship are titled `fix(deps)`, so the release PR picks them up as a patch
release. Bot merges start no workflow, so the release workflow also runs
once a day to update the release PR. Merging the release PR is still up to
a person.

`CHANGELOG.md` is written by release-please: do not edit it by hand. The
release PR needs "Allow GitHub Actions to create and approve pull
requests" turned on in the repository's Actions settings.

### Building a release binary yourself

`mise run build:linux` produces a static `musl` binary for your machine's CPU
architecture, with the frontend built in. zig is the C toolchain (through
cargo zigbuild), so the same command works on Linux and on a Mac, where it
produces a Linux binary. It builds with your own cargo cache and target
directory and copies the result to `dist/potoo-linux-<arch>`. `ARCH=amd64`
(or `arm64`) builds for the other architecture, and `PROFILE` and `FEATURES`
pick a cargo profile and features (a profile other than release adds a
`-<profile>` suffix):

```bash
PROFILE=dev scripts/build-linux.sh
podman build --build-arg BINARY=dist/potoo-linux-arm64-dev -t potoo:dev .
```

`mise run build:docker` packages the binary into the image: the Dockerfile is
Alpine plus a `COPY` of it, and nothing compiles inside.
