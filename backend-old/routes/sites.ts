import { Hono } from "@hono/hono";
import { z } from "@zod/zod";
import { all, type Db, one } from "../db.ts";
import { type AuthEnv, requireAdmin, requireUser } from "../lib/auth.ts";
import { rangeLine, statsCsv } from "../lib/csv.ts";
import { parseFilterParam } from "../lib/filter.ts";
import { parseRange, statsFor } from "../lib/stats.ts";

const Hostname = z.string().trim().toLowerCase().max(253).regex(
  /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)*[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(:\d{1,5})?$/,
  "Expected a hostname such as example.com or blog.example.com",
);
const SiteInput = z.object({
  hostname: Hostname,
  honor_dnt: z.boolean().default(true),
});

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
    const range = parseRange(
      c.req.query("days"),
      c.req.query("from"),
      c.req.query("to"),
    );
    if (!range.ok) {
      return c.json({ error: range.error }, 400);
    }
    const filter = parseFilterParam(c.req.query("q"));
    if (!filter.ok) {
      return c.json({ error: filter.error }, 400);
    }
    const site = await one<{
      id: number;
      hostname: string;
      honor_dnt: number;
    }>(
      db,
      "SELECT id, hostname, honor_dnt FROM sites WHERE id = ?",
      [id],
    );
    if (!site) return c.json({ error: "Not found" }, 404);
    return c.json({
      site,
      ...await statsFor(db, [id], range.range, filter.filters),
    });
  });

  // The same stats as a CSV document: one dashboard view of one site.
  app.get("/:id/stats.csv", async (c) => {
    const id = Number(c.req.param("id"));
    const range = parseRange(
      c.req.query("days"),
      c.req.query("from"),
      c.req.query("to"),
    );
    if (!range.ok) {
      return c.json({ error: range.error }, 400);
    }
    const filter = parseFilterParam(c.req.query("q"));
    if (!filter.ok) {
      return c.json({ error: filter.error }, 400);
    }
    const site = await one<{ id: number }>(
      db,
      "SELECT id FROM sites WHERE id = ?",
      [id],
    );
    if (!site) return c.json({ error: "Not found" }, 404);
    const stats = await statsFor(db, [id], range.range, filter.filters);
    const csv = statsCsv(stats, {
      range: rangeLine(range.range),
      filter: c.req.query("q"),
    });
    return c.body(csv, 200, {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": 'attachment; filename="potoo-stats.csv"',
    });
  });

  return app;
}
