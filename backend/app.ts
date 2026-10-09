import { Hono } from "@hono/hono";
import { serveStatic } from "@hono/hono/deno";
import { logger } from "@hono/hono/logger";
import type { Db } from "./db.ts";
import type { GeoLookup } from "./lib/geo.ts";
import { authRoutes } from "./routes/auth.ts";
import { collectRoutes } from "./routes/collect.ts";
import { exportRoutes } from "./routes/export.ts";
import { sitesRoutes } from "./routes/sites.ts";
import { statsRoutes } from "./routes/stats.ts";

export function createApp(db: Db, geo: GeoLookup, staticDir: string) {
  const app = new Hono();
  app.use(logger());
  // Static caching: the hashed files under /assets are immutable, but the
  // shell (and anything else) must always be revalidated, so a stale
  // index.html can never reference an asset hash that a rebuild removed.
  app.use("*", async (c, next) => {
    await next();
    if (c.res.headers.get("cache-control") === null) {
      c.header(
        "cache-control",
        c.req.path.startsWith("/assets/")
          ? "public, max-age=31536000, immutable"
          : "no-cache",
      );
    }
  });
  app.route("/api", collectRoutes(db, geo));
  app.route("/api", exportRoutes(db));
  app.route("/api", authRoutes(db));
  app.route("/api/sites", sitesRoutes(db));
  app.route("/api/stats", statsRoutes(db));
  app.all("/api/*", (c) => c.json({ error: "Not found" }, 404));

  // Embedded CSR frontend: static files, falling back to index.html for client routes.
  app.use("*", serveStatic({ root: staticDir }));
  app.get("*", serveStatic({ root: staticDir, path: "index.html" }));
  return app;
}
