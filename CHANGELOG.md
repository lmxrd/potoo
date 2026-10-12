# Changelog

## [0.4.0](https://github.com/lmxrd/potoo/compare/v0.3.0...v0.4.0) (2026-10-12)


### Features

* a facet with no value shows as unknown ([4520ace](https://github.com/lmxrd/potoo/commit/4520acea38e27fabccc0f202a4507be9ad756a5b))

## [0.3.0](https://github.com/lmxrd/potoo/compare/v0.2.0...v0.3.0) (2026-10-12)


### Features

* admins can import hits from an export's CSV ([5c58cc2](https://github.com/lmxrd/potoo/commit/5c58cc2913ad815e4c80e340eb757ee0269e1082))

## [0.2.0](https://github.com/lmxrd/potoo/compare/v0.1.0...v0.2.0) (2026-10-12)


### ⚠ BREAKING CHANGES

* This release breaks all previous databases, so delete the database and start from scratch. Please manually add the data if you need it to a newly initialized database if you need it.

### Features

* an OpenAPI spec drives the frontend's API client ([4b11e9f](https://github.com/lmxrd/potoo/commit/4b11e9ff021010af9610adaf70ff217505ff529f))
* ApiJson is the JSON request body extractor ([8bf8c3a](https://github.com/lmxrd/potoo/commit/8bf8c3afbd95fe7e68cbd71d60150610ca4ab68e))
* facet values are stored as lowercase keys ([3870465](https://github.com/lmxrd/potoo/commit/3870465cc296c464100d2caaea3e43320167dae8))
* no adoption of databases from before migrations ([3972ca7](https://github.com/lmxrd/potoo/commit/3972ca7eb9150680212f210f2e821c38ce97c6c9))
* the export's filename carries its timestamp ([6c24b12](https://github.com/lmxrd/potoo/commit/6c24b124e707c07a2b353fa0ad042f446563370e))
* the page views chart is a visx chart ([8bb2597](https://github.com/lmxrd/potoo/commit/8bb2597ee833801adcb45e6e9fea70160b66229e))
* the stats and the export select sites by hostname ([971c809](https://github.com/lmxrd/potoo/commit/971c809c782aa0ff3046282601bede66fa5e40df))


### Bug Fixes

* a duration range is the exact window it names ([670cf16](https://github.com/lmxrd/potoo/commit/670cf16ad876521e9d505001dd079fc3170680df))
* seconds in the export's ISO timestamp ([4115755](https://github.com/lmxrd/potoo/commit/4115755c11ddb67ddd3c06eb13a2a134ed45afec))
* speed up db writes by using WAL and sunchronous mode ([2404522](https://github.com/lmxrd/potoo/commit/24045225697e151a598dcd62d076e3e7a23e563a))
* the export's timestamp is plain ISO ([cc84156](https://github.com/lmxrd/potoo/commit/cc8415654b2dba80ed2f01871f159b6aa8d1ca50))
* the GeoIP download knows the month it is in ([d384ad5](https://github.com/lmxrd/potoo/commit/d384ad57f6ea59728a04f0440982de0cd82a17c6))
* the openapi bin writes to the path argument it is given ([0b87d9d](https://github.com/lmxrd/potoo/commit/0b87d9de606ec06dec27f9bc3620eac4e4b95b34))


### Performance Improvements

* a pooled sqlx data layer with a batched write path ([4f9a6b7](https://github.com/lmxrd/potoo/commit/4f9a6b7f29f1152eab3bab051736164a669fca07))

## [0.1.0](https://github.com/lmxrd/potoo/compare/v0.1.0...v0.1.0) (2026-10-10)


### Features

* alpine runtime image via a static musl build ([dc46611](https://github.com/lmxrd/potoo/commit/dc4661121ccfa489f2693459cfa2dc6d42ed930e))
* **auth:** password change endpoint and admin password reset ([47fbe50](https://github.com/lmxrd/potoo/commit/47fbe504eb44f2efeddd99e653f057781faaed23))
* **auth:** rate limit failed logins per IP and email (5/15min, in-memory) ([3cc1b06](https://github.com/lmxrd/potoo/commit/3cc1b06051337995d8a5c79686f1c3031ba461d3))
* **build:** QuickJS engine, multi-stage Dockerfile on debian slim ([65d0ed1](https://github.com/lmxrd/potoo/commit/65d0ed17370cf8e03082a9e3dfd14d935cb48f7a))
* **build:** single-executable build with embedded frontend and migrations ([29a3ec0](https://github.com/lmxrd/potoo/commit/29a3ec02a9297a80376f98ef27c6c0590435cf2d))
* **dashboard:** global overview with site multi-select replaces sites landing page ([8fe6000](https://github.com/lmxrd/potoo/commit/8fe60005b8073857875d89e4d78098be0e0cb6fd))
* **db:** manage schema with nessie migrations, applied on startup ([0db9a7e](https://github.com/lmxrd/potoo/commit/0db9a7e988bf2fccdfc52b8fc32682085f5fa196))
* **export:** dedicated export page with site and date selection ([1e403d5](https://github.com/lmxrd/potoo/commit/1e403d56dee20868f90ff9d3724d8da293d2add1))
* **export:** default to the last week, quick range buttons ([8aea2d1](https://github.com/lmxrd/potoo/commit/8aea2d14b06782dc3ce59d40a638c4c15980854a))
* **frontend:** change-password page and admin password reset in users view ([fbbf488](https://github.com/lmxrd/potoo/commit/fbbf488c0ac04817d570f3c285adbf0962fcb008))
* **geo:** auto-download country database from DB-IP, refresh weekly ([a7df30e](https://github.com/lmxrd/potoo/commit/a7df30e248082407437096879e37f140836e0ca4))
* **nav:** add-site button with popup form in the nav bar ([6cc1997](https://github.com/lmxrd/potoo/commit/6cc19978dc49f614f91b023b2b0b275afbbc62d7))
* **nav:** Potoo -&gt; dashboard, Sites page with table, email popover menu ([3816381](https://github.com/lmxrd/potoo/commit/381638129bb40b64dc8cdac5dd8fe438813030f1))
* **range:** dashboard view in the URL, relative presets as HTML durations ([fca1274](https://github.com/lmxrd/potoo/commit/fca1274ab11bd98bb1000a077dd34c80c4acedc5))
* **range:** responsive custom range inputs in the dashboard toolbar ([558541b](https://github.com/lmxrd/potoo/commit/558541b4c5f8f893c8a24bccd32063e3405b1cb5))
* seed and clean tasks, deno removal, novice tracking docs ([167277e](https://github.com/lmxrd/potoo/commit/167277e9f1fff081c539171e901fb1752479796a))
* **seed:** a year of demo data behind deno task seed ([8e28eff](https://github.com/lmxrd/potoo/commit/8e28effbcc2e0348b644f7e255e68d52726c929b))
* **settings:** per-user 12/24-hour time format, defaulting to 24 hours ([3b4005e](https://github.com/lmxrd/potoo/commit/3b4005e847bfe49dddb66c0dd0c36b0ae7c62c8a))
* **settings:** per-user date format, YYYY-MM-DD by default ([1b63134](https://github.com/lmxrd/potoo/commit/1b63134c60bb6df5162530e525ba4dc3a620e537))
* **settings:** settings page with password change, email in nav links to it ([3a56a18](https://github.com/lmxrd/potoo/commit/3a56a183769bdd194a5df1941be1b3397acd42b6))
* **sites:** CSV export of a site's hits ([06b23ca](https://github.com/lmxrd/potoo/commit/06b23caee788062631ecd04f66087a90bdcca9bc))
* **sites:** honor_dnt in stats response and settings toggle in site UI ([9e7576f](https://github.com/lmxrd/potoo/commit/9e7576f7e14bdf69f382f077e851ffd42542ab9f))
* **stats:** complete calendar-aligned chart buckets with date labels ([29c385c](https://github.com/lmxrd/potoo/commit/29c385c94f7ef3907189e36de3743de94fe3e728))
* **stats:** country names in the dashboard, via Intl.DisplayNames ([8185ed0](https://github.com/lmxrd/potoo/commit/8185ed001691e47538ede55db45fd71c2cbe949f))
* **stats:** minute-precise date ranges and view export on the dashboard ([4bf3abf](https://github.com/lmxrd/potoo/commit/4bf3abf7751f04e13135906eb0930a401d7db77d))
* **stats:** per-hour view of page views for short ranges ([fc838f5](https://github.com/lmxrd/potoo/commit/fc838f5ba8fa816a05f7c3f3de0cca6cd6239150))
* **stats:** RSQL filtering on the dashboards, CSV exports server-side ([8a3124b](https://github.com/lmxrd/potoo/commit/8a3124b5b82a4d7e0be171db91ac9add6544454d))
* **stats:** Today range, since midnight rather than the last 24 hours ([d7a5f46](https://github.com/lmxrd/potoo/commit/d7a5f463b87f45c44c968509676b89f3f0df8d0e))
* themes in the user settings ([a3e8238](https://github.com/lmxrd/potoo/commit/a3e8238c135425fb75a4fda8bcd82fee25b6bfec))
* **tracking:** coarse browser and os, derived from the user agent ([4284464](https://github.com/lmxrd/potoo/commit/428446455252ba2d2155a02416d12d84685834f9))
* **users:** disable demote and delete for the last admin ([b93cbdc](https://github.com/lmxrd/potoo/commit/b93cbdcf83fa0a7cabb1b860cfdeb9bd5665ba32))
* **users:** multiple admins with role changes, email change in settings ([56c2772](https://github.com/lmxrd/potoo/commit/56c27726bd3cb2702ed56a8a61791fd8232a8c8d))


### Bug Fixes

* default-run so a bare cargo run starts the server ([0fa00de](https://github.com/lmxrd/potoo/commit/0fa00dec2aaff75a3ed3b5f75f2f2183ea14696a))
* mise run start builds before serving ([b78f955](https://github.com/lmxrd/potoo/commit/b78f95555212e963d801cfa9fa812acffea52e07))
* **range:** apply a custom range without wiping it or flashing ([6e6fa53](https://github.com/lmxrd/potoo/commit/6e6fa53832655511b967a45b88e1c18a72deaedf))
* **routes:** un-nest the site page from the /sites layout with sites_.$siteId ([33d23c0](https://github.com/lmxrd/potoo/commit/33d23c0b5e446fb2b0ae695376161c5801a2851e))
* **static:** cache headers so the shell is always revalidated ([4f09ec4](https://github.com/lmxrd/potoo/commit/4f09ec4b513df258af74c04bcbbe539df3079d02))
* the Today button writes its window to the URL, not its name ([06ab47b](https://github.com/lmxrd/potoo/commit/06ab47b187c66453c8c18f1200e56591818ae5dd))


### Continuous Integration

* Implemented release workflow ([6e1ae8d](https://github.com/lmxrd/potoo/commit/6e1ae8dc2e8d3e8d6ccdbff0ce79e2c0e4194b18))
