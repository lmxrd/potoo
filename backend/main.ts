import { createApp } from "./app.ts";
import { openDb } from "./db.ts";
import { openGeo } from "./lib/geo.ts";

const db = await openDb(Deno.env.get("DATABASE_PATH") ?? "data/potoo.db");
const geo = await openGeo(
  Deno.env.get("GEOIP_DB_PATH") ?? "data/country.mmdb",
);
const app = createApp(db, geo, Deno.env.get("STATIC_DIR") ?? "frontend/dist");

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
