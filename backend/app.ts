import { Hono } from "@hono/hono";
import { serveStatic } from "@hono/hono/deno";
import { logger } from "@hono/hono/logger";
import type { Db } from "./db.ts";
import type { GeoLookup } from "./lib/geo.ts";
import { authRoutes } from "./routes/auth.ts";
import { collectRoutes } from "./routes/collect.ts";
import { sitesRoutes } from "./routes/sites.ts";
import { statsRoutes } from "./routes/stats.ts";

export function createApp(db: Db, geo: GeoLookup, staticDir: string) {
  const app = new Hono();
  app.use(logger());
  app.route("/api", collectRoutes(db, geo));
  app.route("/api", authRoutes(db));
  app.route("/api/sites", sitesRoutes(db));
  app.route("/api/stats", statsRoutes(db));
  app.all("/api/*", (c) => c.json({ error: "Not found" }, 404));

  // Embedded CSR frontend: static files, falling back to index.html for client routes.
  app.use("*", serveStatic({ root: staticDir }));
  app.get("*", serveStatic({ root: staticDir, path: "index.html" }));
  return app;
}
