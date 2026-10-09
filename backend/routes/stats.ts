import { Hono } from "@hono/hono";
import { z } from "@zod/zod";
import { all, type Db } from "../db.ts";
import { type AuthEnv, requireUser } from "../lib/auth.ts";
import { statsFor } from "../lib/stats.ts";

const Query = z.object({
  days: z.union([
    z.coerce.number().int().min(1).max(365),
    z.literal("today"),
  ]).default(7),
  granularity: z.enum(["day", "hour"]).default("day"),
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
    const { days, sites, granularity } = parsed.data;
    // The per-hour view only makes sense for short ranges.
    if (granularity === "hour" && days !== "today" && days > 3) {
      return c.json({ error: "Hourly is only available up to 3 days" }, 400);
    }
    const siteIds = sites
      ? sites.split(",").map(Number)
      : (await all<{ id: number }>(db, "SELECT id FROM sites")).map((s) =>
        s.id
      );
    return c.json(await statsFor(db, siteIds, days, granularity));
  });

  return app;
}
