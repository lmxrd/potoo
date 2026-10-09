# Potoo

A small, privacy-friendly web analytics tool you host yourself. One binary
serves the dashboard and collects the numbers; your visitors keep their
identity, and you keep your data.

![The dashboard](docs/screenshots/dashboard.png)

## Why Potoo

- **No cookies, no consent banner.** There is nothing to consent to: no cookies,
  no localStorage, no fingerprinting.
- **No personal data stored.** IP addresses and user agents are used in memory
  to derive the country, the device class, the browser family and the operating
  system, and are then discarded.
- **Visitors cannot be tracked across days or sites.** Unique visitors are
  counted with a hash of a daily salt, so yesterday's visitor and today's
  visitor are two different numbers, and the same person on two of your sites is
  two visitors.
- **Your data is yours.** One SQLite file, a full CSV export of every recorded
  hit, and optional retention that deletes old data for you.

## What you get

- **Dashboards per site and across sites**, with a multi-select to combine any
  subset of your sites into one view.
- **A complete chart**: hours for today and the last 24 hours, days up to a
  month, weeks up to 90 days, and months for a year — always aligned to the
  calendar, always showing every bucket, empty or not.
- **Breakdowns**: top paths, referrers, countries, devices, browsers, operating
  systems, and clicks on elements you label.
- **A snippet generator** on every site's page: a single script tag, with
  optional click tracking, outbound link tracking, and a no-JS pixel fallback.
  SPA navigation via `pushState` is tracked.
- **CSV export** of all hits, filtered by sites and a date range.
- **Multiple users, any number of admins**, with role management, password
  resets, and per-user settings (email, password, date format).

![A site with its snippet](docs/screenshots/site.png)

## Self-hosting

### Docker

```bash
docker build -t potoo .
docker run -d --name potoo -p 8000:8000 -v potoo-data:/data potoo
```

The image is Debian slim holding a single statically self-contained binary: the
frontend and the database migrations are embedded in it. The database and the
GeoIP database live in the `/data` volume. For both architectures:

```bash
docker buildx build --platform linux/amd64,linux/arm64 -t <registry>/potoo --push .
```

Behind a reverse proxy, set `TRUST_PROXY=1` so the client IP is read from
`X-Forwarded-For`, and terminate TLS there.

### A single binary

With [Deno](https://deno.com) and [pnpm](https://pnpm.io) installed:

```bash
deno task build
./dist/potoo-<your platform>
```

The task builds the frontend and compiles everything into binaries for macOS,
Linux, and both architectures in `dist/`. With just Deno you can also run from
source: `deno task start`.

### First run

Open `http://localhost:8000`; with no users yet, every page redirects to
`/setup`, where you create the first admin. Then:

1. Add a site on the **Sites** page (each subdomain is its own site).
2. Open the site's page and copy the tracking snippet into your pages.
3. Watch the dashboard.

## Tracking

The snippet generator produces a single script tag:

```html
<script async src="https://your-potoo.example/api/tracker.js"></script>
```

Options, combined as needed:

| Option             | What it does                                               |
| ------------------ | ---------------------------------------------------------- |
| `data-clicks`      | Records clicks on elements with `data-potoo-click="label"` |
| `data-outbound`    | Records clicks on links leaving your site                  |
| The pixel fallback | A `<noscript>` image tag for visitors without JavaScript   |

Visitors who ask not to be tracked (Do Not Track or Global Privacy Control) are
ignored by default; each site can turn that off.

## Configuration

| Variable         | Default             | Purpose                                                                                                                      |
| ---------------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `PORT`           | `8000`              | HTTP port                                                                                                                    |
| `DATABASE_PATH`  | `data/potoo.db`     | SQLite file                                                                                                                  |
| `GEOIP_DB_PATH`  | `data/country.mmdb` | MaxMind-format country DB. Downloaded from DB-IP on boot when missing or older than a week, and refreshed weekly             |
| `GEOIP_URL`      | unset               | Override the download URL, e.g. your own mirror. Empty string disables downloading; a missing file then leaves country empty |
| `STATIC_DIR`     | `frontend/dist`     | Built frontend (only used when running from source)                                                                          |
| `TRUST_PROXY`    | unset               | Set to `1` to read the client IP from `X-Forwarded-For` (behind a reverse proxy)                                             |
| `RETENTION_DAYS` | unset               | Delete hits older than this                                                                                                  |

This site or product includes IP data from db-ip.com, licensed under CC BY 4.0.

## CLI tasks

| Task                     | What it does                                                               |
| ------------------------ | -------------------------------------------------------------------------- |
| `deno task dev`          | Runs the backend (watch) and the frontend (Vite) together, with hot reload |
| `deno task start`        | Runs the server from source                                                |
| `deno task build`        | Builds the frontend and compiles binaries for every platform into `dist/`  |
| `deno task build:docker` | Builds the Docker image                                                    |
| `deno task check`        | Format, lint and type checks for backend and frontend                      |
| `deno task fix`          | Applies the format and lint fixes                                          |
| `deno task test`         | The backend end-to-end tests                                               |

## Development

```
backend/main.ts           entrypoint: env config, geo refresh, retention
backend/app.ts            Hono app: routes, cache headers, static serving
backend/db.ts             query helpers + nessie migration runner
backend/db/migrations/    nessie migrations, applied on startup
backend/lib/              auth, sessions, visitor parsing, geo, stats, tracker.js
backend/routes/           auth, users, sites, stats, export, collect
frontend/src/routes/      dashboard, sites table, site page, export, users, settings
```

Checks: `deno task check` and `deno task test` cover format, lint and types for
both projects, and the end-to-end tests of setup, users, collection and stats.

## Notes

- The tracker accepts hits for registered sites but does not verify `Origin`, so
  a determined visitor can send spoofed hits. Acceptable for analytics, worth
  knowing.
- Compiled binaries use the experimental QuickJS engine for their smaller size;
  drop `--engine quickjs` from `compile:base` for V8.
