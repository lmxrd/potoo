# Potoo — notes for coding agents

Potoo is a small, privacy-friendly, self-hosted web analytics tool: a Rust
backend (axum, SQLite through sqlx) with a React frontend embedded in the
one binary. README.md is the user documentation and CONTRIBUTING.md the
human contributor guide (task list, code layout, conventions); read both
for the full picture. This file is what an agent must know before editing.

## Commands

Everything goes through [mise](https://mise.jdx.dev) (`mise install` once;
it provides Rust, Node, pnpm and the other tools at their pinned versions).

| Command | Use |
| --- | --- |
| `mise run check` | rustfmt, clippy (`-D warnings`), oxfmt, oxlint, tsc. **Must pass.** |
| `mise run test` | The unit and integration tests: backend (cargo) and frontend (Vitest). **Must pass.** |
| `mise run test:e2e` | The end-to-end smoke tests: builds the image, then Playwright drives a browser through it (`packages/e2e`, `scripts/e2e.sh`) |
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
so a repeat run is instant. Use `DATABASE_URL=sqlite:/some/tmp/potoo.db` when
you run the server or the seed and must not touch `data/`, and
`GEOIP_URL=` (empty) so the server does not download the GeoIP database.
The environment variables are in the README's Configuration table.

## Structure

- `packages/backend`: the Rust server (its rules: `packages/backend/AGENTS.md`).
- `packages/frontend`: the React app embedded in it (`packages/frontend/AGENTS.md`).
- `packages/sdk`: the API client, generated from the backend's OpenAPI spec
  (`mise run gen:sdk`), never committed.
- `packages/e2e`: the end-to-end smoke tests (Playwright).
- One pnpm workspace and lockfile at the root; `scripts/` for the Linux
  build, the end-to-end run and the load test; `.github/` for CI, releases,
  Dependabot and the daily security scans.

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

- **Use the community's crates.** When a well-known, maintained crate (or
  npm package) solves the problem, use it rather than writing the same
  thing ourselves: sessions are axum-login and tower-sessions, errors
  thiserror and anyhow. Propose a new one first (name, version, why) and
  add it only once it is approved. Write it ourselves only when nothing
  maintained fits, and say why in a comment (the session store: the sqlx
  store crate needs sqlx 0.8).
- **Versions live in one place each**, read by mise
  (`idiomatic_version_file_enable_tools`): Node and pnpm in the root
  `package.json` (`devEngines.runtime`, `packageManager`), Rust in
  `rust-toolchain.toml` (which rustup reads too), the other tools (zig,
  cargo-zigbuild, watchexec, osv-scanner, Trivy) in `mise.toml`. Crates are
  pinned to the exact version in `Cargo.toml`. The image builds nothing: it is an
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
  `minimumReleaseAge` (1 day, `pnpm-workspace.yaml`) must stay
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
  Router), `pnpm-lock.yaml` and `packages/backend/Cargo.lock` (use the
  package managers), and `packages/sdk/src` (`mise run gen:sdk`).
- **Frontend:** React 19, TanStack Router (file-based), Tailwind 4,
  shadcn/ui on Base UI (`base-vega` style, `packages/frontend/src/components/ui`,
  lucide icons),
  visx for the chart (SVG, styled only with the theme classes such as
  `fill-primary`, never hard-coded colors, so a theme or accent change shows
  at once), oxlint and oxfmt, TypeScript, Vitest for the pure logic in
  `src/lib`. The frontend's own rules are in `packages/frontend/AGENTS.md`.
- **Backend:** its architecture (the hit path, the database, sessions,
  single sign-on, two-factor, limits, CSRF, API keys, the OpenAPI spec) and
  its conventions are in `packages/backend/AGENTS.md`.
- **Tests:** unit tests next to the code, integration tests of the API on
  axum-test (`packages/backend/tests/api.rs`), Vitest for the frontend's
  logic, and a few end-to-end smoke tests (`packages/e2e`) that a real
  browser runs against the built image.

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
