import { Hono } from "@hono/hono";
import { stringify } from "@std/csv";
import { z } from "@zod/zod";
import { all, type Db } from "../db.ts";
import { type AuthEnv, requireUser } from "../lib/auth.ts";
import { rangeLine, statsCsv } from "../lib/csv.ts";
import { parseFilterParam } from "../lib/filter.ts";
import { parseRange, statsFor } from "../lib/stats.ts";

const Query = z.object({
  // Comma separated site ids. Without it, every site is included.
  sites: z.string().regex(/^\d+(,\d+)*$/).optional(),
  // Inclusive days, as YYYY-MM-DD. Without them, everything is exported.
  from: z.iso.date().optional(),
  to: z.iso.date().optional(),
});

/** The export of a hit, as one CSV row. */
type HitRow = {
  site: string;
  created_at: string;
  kind: string;
  path: string;
  name: string | null;
  referrer: string | null;
  country: string | null;
  device: string;
  browser: string;
  os: string;
};

const COLUMNS = [
  "site",
  "created_at",
  "kind",
  "path",
  "name",
  "referrer",
  "country",
  "device",
  "browser",
  "os",
];

/** Every site's ids, or the given ones. */
async function siteIds(db: Db, sites: string | undefined): Promise<number[]> {
  return sites
    ? sites.split(",").map(Number)
    : (await all<{ id: number }>(db, "SELECT id FROM sites")).map((s) => s.id);
}

export function exportRoutes(db: Db) {
  const app = new Hono<AuthEnv>();

  // The middleware is on the routes, not on `app.use`: this app is mounted
  // under the shared /api prefix, where a pathless middleware would
  // intercept every request, including the public ones.

  // The aggregated numbers of one dashboard view: the same endpoint as
  // /api/stats, but as a CSV document.
  app.get("/stats.csv", requireUser(db), async (c) => {
    const sites = Query.shape.sites.safeParse(c.req.query("sites"));
    if (!sites.success) {
      return c.json({ error: "sites must be comma separated site ids" }, 400);
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
    const stats = await statsFor(
      db,
      await siteIds(db, sites.data),
      range.range,
      filter.filters,
    );
    const csv = statsCsv(stats, {
      range: rangeLine(range.range),
      filter: c.req.query("q"),
    });
    return c.body(csv, 200, {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": 'attachment; filename="potoo-stats.csv"',
    });
  });

  // Every recorded hit of a range, one row per hit, oldest first.
  app.get("/export.csv", requireUser(db), async (c) => {
    const parsed = Query.safeParse(c.req.query());
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const { sites, from, to } = parsed.data;
    const ids = await siteIds(db, sites);
    // The days are inclusive: from its first to the last millisecond.
    const where: string[] = [];
    const params: (string | number)[] = [];
    if (ids.length) {
      where.push(`h.site_id IN (${ids.map(() => "?").join(", ")})`);
      params.push(...ids);
    } else {
      where.push("1 = 0"); // no sites: an empty export
    }
    if (from) {
      where.push("h.created_at >= ?");
      params.push(`${from}T00:00:00.000`);
    }
    if (to) {
      where.push("h.created_at <= ?");
      params.push(`${to}T23:59:59.999`);
    }

    const hits = await all<HitRow>(
      db,
      `SELECT s.hostname AS site, h.created_at, h.kind, h.path, h.name,
              h.referrer, h.country, h.device, h.browser, h.os
       FROM hits h JOIN sites s ON s.id = h.site_id
       WHERE ${where.join(" AND ")}
       ORDER BY h.created_at, h.id`,
      params,
    );
    const csv = stringify([
      COLUMNS,
      ...hits.map((h) => [
        h.site,
        h.created_at,
        h.kind,
        h.path,
        h.name ?? "",
        h.referrer ?? "",
        h.country ?? "",
        h.device,
        h.browser,
        h.os,
      ]),
    ]);
    return c.body(csv, 200, {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": 'attachment; filename="potoo-export.csv"',
    });
  });

  return app;
}
