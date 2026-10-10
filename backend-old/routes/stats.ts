import { Hono } from "@hono/hono";
import { z } from "@zod/zod";
import { all, type Db } from "../db.ts";
import { type AuthEnv, requireUser } from "../lib/auth.ts";
import { parseFilterParam } from "../lib/filter.ts";
import { parseRange, statsFor } from "../lib/stats.ts";

const Query = z.object({
  // Comma separated site ids. Without it, every site is included.
  sites: z.string().regex(/^\d+(,\d+)*$/).optional(),
});

export function statsRoutes(db: Db) {
  const app = new Hono<AuthEnv>();
  app.use(requireUser(db));

  app.get("/", async (c) => {
    const parsed = Query.safeParse(c.req.query());
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
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
    const siteIds = parsed.data.sites
      ? parsed.data.sites.split(",").map(Number)
      : (await all<{ id: number }>(db, "SELECT id FROM sites")).map((s) =>
        s.id
      );
    return c.json(await statsFor(db, siteIds, range.range, filter.filters));
  });

  return app;
}
