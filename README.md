# Potoo

A small, privacy-friendly web analytics tool inspired by [GoatCounter](https://www.goatcounter.com/). One Deno backend serves a JSON API and the built React frontend.

## Requirements

### Product
- Show stats for **multiple sites**. One site = one hostname; each subdomain is its own site (`example.com`, `blog.example.com`).
- Track **page views by path**, plus optional **clicks** on links and buttons.
- Store only very basic visitor information: **country** and **device** (desktop / mobile / tablet). Bots are ignored.
- Dashboard per site: page views, unique visitors, views per day, top paths, referrers, countries, devices, clicks. Selectable range (1–365 days).
- Tracking options are chosen by whoever embeds it (the site page has a snippet generator):
  - JS snippet that records page views, including SPA navigation via `pushState`.
  - Clicks on elements with `data-potoo-click="label"` (`data-clicks` on the script tag).
  - Outbound link clicks (`data-outbound` on the script tag).
  - No-JS pixel fallback: `/api/collect.gif?h=<host>&p=<path>`.
  - Honoring Do Not Track / Global Privacy Control, set per site (`honor_dnt`, on by default).

### EU / GDPR compliance (design constraints, keep these)
- No cookies, no localStorage, so no consent banner.
- **IP addresses and full user agents are never stored.** The IP is used in memory only, for the country lookup and the visitor hash.
- Unique visitors are a hash of `daily salt + site id + IP + user agent`. The salt is generated daily and older salts are deleted, so visitors cannot be linked across days or sites. "Visitors" over a multi-day range is therefore a sum of daily uniques.
- Query strings and fragments are stripped from paths and click labels, since they can contain personal data. Only the referrer's hostname is kept.
- Country comes from a local GeoIP database file, with no third-party calls.
- Optional retention: set `RETENTION_DAYS` to delete older hits.

### Users and access
- Multiple users, **exactly one admin**, no organisations or teams. A partial unique index enforces a single admin.
- On first start (no users) **every page redirects to `/setup`**, which creates the admin. `POST /api/setup` only works while there are no users.
- Only the admin creates and deletes other users (`/users`) and adds, edits and deletes sites.
- All signed-in users can view all sites.
- Auth: email + password (PBKDF2-SHA256), opaque server-side session in an HttpOnly `SameSite=Strict` cookie.

## Stack
- **Backend:** Deno, `jsr:@hono/hono`, `jsr:@stdx/database` (SQLite driver, pinned to `^0.1.1` because 0.2.0 was newer than Deno's minimum dependency age at the time), `jsr:@zod/zod`, `npm:maxmind` for GeoIP, `jsr:@halvardm/nessie` for migrations (runs automatically on startup).
- **Frontend:** Vite, React, TanStack Router (file-based), Tailwind v4, oxlint, oxfmt. Client-side rendered and served statically by the backend, with an `index.html` fallback for client routes.

## Layout
```
deno.json                 imports + tasks
backend/main.ts           entrypoint, env config, retention job
backend/app.ts            Hono app, routes, static serving
backend/db.ts             query helpers + migration runner (nessie)
backend/db/migrations/    nessie migrations, applied on startup
backend/lib/              auth (passwords, sessions), visitor (device, daily hash), geo, tracker.js source
backend/routes/           auth (session/setup/login/users/password), sites (CRUD + stats), collect (tracker, pixel)
backend/app.test.ts       end-to-end tests: setup → users → sites → collection → stats, login rate limiting, password change/reset
frontend/src/routes/      __root (redirect guard), setup, login, index (sites), sites.$siteId (stats), users, password
```

## Configuration (env)
| Variable | Default | Purpose |
| --- | --- | --- |
| `PORT` | `8000` | HTTP port |
| `DATABASE_PATH` | `data/potoo.db` | SQLite file |
| `GEOIP_DB_PATH` | `data/country.mmdb` | MaxMind-format country DB (GeoLite2 or DB-IP Lite). Without it, country is empty |
| `STATIC_DIR` | `frontend/dist` | Built frontend |
| `TRUST_PROXY` | unset | Set to `1` to read the client IP from `X-Forwarded-For` (behind a reverse proxy) |
| `RETENTION_DAYS` | unset | Delete hits older than this |

## Running
```bash
cd frontend && pnpm install && pnpm build && cd ..
deno task start            # serves API + frontend on :8000
```
`deno task build` builds the frontend and compiles the backend, frontend assets and migrations into a single executable, `./potoo`. The binary serves the frontend from its embedded virtual filesystem and applies migrations from the embedded migration files; `DATABASE_PATH` and the other env vars work as above.
Development: `deno task dev` (backend, watch) and `pnpm dev` in `frontend/` (Vite proxies `/api` to :8000).
Checks: `deno task check`, `deno task test`, `pnpm typecheck`, `pnpm lint`, `pnpm fmt:check`.

## Status
Implemented and passing: backend type check and tests, frontend build, typecheck, lint and format, and a scripted end-to-end smoke run against the running server (setup, users, login rate limiting, password change and admin reset, sites with `honor_dnt` toggle, tracker, pixel, DNT/GPC and bot handling, stats, static serving). Known limitations:
- Login rate limiting is in memory (5 failures per IP + email within 15 minutes) and resets when the server restarts.
- No automatic GeoIP database download or update; drop a MaxMind-format file at `GEOIP_DB_PATH` (GeoLite2 requires a MaxMind account, DB-IP Lite works too).
- The tracker only accepts hits whose host matches a registered site, and `Origin` is not verified, so hits can be spoofed. This is acceptable for analytics but worth knowing.
- Stats show counts only, with a simple bar chart.
- No manual click-through of the UI in a real browser has been done yet; the smoke run drives the HTTP API directly.
Possible additions: per-site "view as public" dashboards, CSV export, filtering by path or country, Dockerfile.
