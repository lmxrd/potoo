import { Hono } from "@hono/hono";
import { serveStatic } from "@hono/hono/deno";
import { logger } from "@hono/hono/logger";
import type { Db } from "./db.ts";
import { eventsRoutes } from "./routes/events.ts";

export function createApp(db: Db, staticDir: string) {
  const app = new Hono();
  app.use(logger());
  app.route("/api", eventsRoutes(db));
  app.all("/api/*", (c) => c.json({ error: "Not found" }, 404));

  // Embedded CSR frontend: static files, falling back to index.html for client routes.
  app.use("*", serveStatic({ root: staticDir }));
  app.get("*", serveStatic({ root: staticDir, path: "index.html" }));
  return app;
}
