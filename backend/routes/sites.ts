import { Hono } from "@hono/hono";
import { z } from "@zod/zod";
import { all, type Db, one } from "../db.ts";
import { type AuthEnv, requireAdmin, requireUser } from "../lib/auth.ts";

const Hostname = z.string().trim().toLowerCase().max(253).regex(
  /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)*[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(:\d{1,5})?$/,
  "Expected a hostname such as example.com or blog.example.com",
);
const SiteInput = z.object({
  hostname: Hostname,
  honor_dnt: z.boolean().default(true),
});
const Days = z.coerce.number().int().min(1).max(365).default(7);

export function sitesRoutes(db: Db) {
  const app = new Hono<AuthEnv>();
  app.use(requireUser(db));

  app.get("/", async (c) =>
    c.json(
      await all(
        db,
        "SELECT id, hostname, honor_dnt, created_at FROM sites ORDER BY hostname",
      ),
    ));

  app.post("/", requireAdmin, async (c) => {
    const parsed = SiteInput.safeParse(await c.req.json().catch(() => null));
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const { hostname, honor_dnt } = parsed.data;
    if (await one(db, "SELECT 1 FROM sites WHERE hostname = ?", [hostname])) {
      return c.json({ error: "Site already exists" }, 409);
    }
    const res = await db.execute(
      "INSERT INTO sites (hostname, honor_dnt) VALUES (?, ?)",
      [hostname, honor_dnt ? 1 : 0],
    );
    return c.json({ id: Number(res.lastInsertId), hostname, honor_dnt }, 201);
  });

  app.patch("/:id", requireAdmin, async (c) => {
    const parsed = z.object({ honor_dnt: z.boolean() }).safeParse(
      await c.req.json().catch(() => null),
    );
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    await db.execute("UPDATE sites SET honor_dnt = ? WHERE id = ?", [
      parsed.data.honor_dnt ? 1 : 0,
      Number(c.req.param("id")),
    ]);
    return c.body(null, 204);
  });

  // Deleting a site deletes all of its hits.
  app.delete("/:id", requireAdmin, async (c) => {
    await db.execute("DELETE FROM sites WHERE id = ?", [
      Number(c.req.param("id")),
    ]);
    return c.body(null, 204);
  });

  app.get("/:id/stats", async (c) => {
    const id = Number(c.req.param("id"));
    const days = Days.safeParse(c.req.query("days"));
    if (!days.success) {
      return c.json({ error: z.treeifyError(days.error) }, 400);
    }
    const site = await one<{ id: number; hostname: string }>(
      db,
      "SELECT id, hostname FROM sites WHERE id = ?",
      [id],
    );
    if (!site) return c.json({ error: "Not found" }, 404);

    const since = new Date(Date.now() - days.data * 86_400_000).toISOString();
    const q = <T>(select: string, tail = "") =>
      all<T>(
        db,
        `SELECT ${select} FROM hits WHERE site_id = ? AND created_at >= ? ${tail}`,
        [id, since],
      );
    const pv = "kind = 'pageview'";

    const [totals, byDay, paths, countries, devices, referrers, clicks] =
      await Promise.all([
        q<{ pageviews: number; visitors: number }>(
          "COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
          `AND ${pv}`,
        ),
        q(
          "date(created_at) AS day, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
          `AND ${pv} GROUP BY day ORDER BY day`,
        ),
        q(
          "path, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
          `AND ${pv} GROUP BY path ORDER BY pageviews DESC LIMIT 50`,
        ),
        q(
          "COALESCE(country, '??') AS country, COUNT(DISTINCT visitor) AS visitors",
          `AND ${pv} GROUP BY country ORDER BY visitors DESC LIMIT 50`,
        ),
        q(
          "device, COUNT(DISTINCT visitor) AS visitors",
          `AND ${pv} GROUP BY device ORDER BY visitors DESC`,
        ),
        q(
          "referrer, COUNT(*) AS pageviews",
          `AND ${pv} AND referrer IS NOT NULL GROUP BY referrer ORDER BY pageviews DESC LIMIT 20`,
        ),
        q(
          "name, COUNT(*) AS clicks",
          "AND kind = 'click' GROUP BY name ORDER BY clicks DESC LIMIT 50",
        ),
      ]);
    return c.json({
      site,
      days: days.data,
      ...totals[0],
      byDay,
      paths,
      countries,
      devices,
      referrers,
      clicks,
    });
  });

  return app;
}
