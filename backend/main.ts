import { createApp } from "./app.ts";
import { openDb } from "./db.ts";
import { GEO_REFRESH_MS, openGeo } from "./lib/geo.ts";

const db = await openDb(Deno.env.get("DATABASE_PATH") ?? "data/potoo.db");
const geo = await openGeo(
  Deno.env.get("GEOIP_DB_PATH") ?? "data/country.mmdb",
);
// The boot check inside openGeo downloads a missing or week-old database;
// while running, refresh once a week.
setInterval(() => geo.refresh(), GEO_REFRESH_MS);
const app = createApp(
  db,
  geo,
  // Module-relative so it works both from the repo (deno task start) and
  // inside a deno compile binary, where included files live in a virtual
  // filesystem keyed to the module's location.
  Deno.env.get("STATIC_DIR") ?? `${import.meta.dirname}/../frontend/dist`,
);

// Optional data retention: delete hits older than RETENTION_DAYS.
const retention = Number(Deno.env.get("RETENTION_DAYS") ?? 0);
if (retention > 0) {
  const purge = () =>
    db.execute("DELETE FROM hits WHERE created_at < ?", [
      new Date(Date.now() - retention * 86_400_000).toISOString(),
    ]);
  await purge();
  setInterval(purge, 6 * 3_600_000);
}

Deno.serve({ port: Number(Deno.env.get("PORT") ?? 8000) }, app.fetch);
