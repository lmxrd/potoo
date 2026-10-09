import { Hono } from "@hono/hono";
import { z } from "@zod/zod";
import { all, type Db } from "../db.ts";
import { type AuthEnv, requireUser } from "../lib/auth.ts";

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
};

const COLUMNS = "site,created_at,kind,path,name,referrer,country,device";

const cell = (v: string | null) =>
  v == null ? "" : `"${v.replaceAll('"', '""')}"`;

export function exportRoutes(db: Db) {
  const app = new Hono<AuthEnv>();

  // The middleware is on the route, not on `app.use`: this app is mounted
  // under the shared /api prefix, where a pathless middleware would
  // intercept every request, including the public ones.
  app.get("/export.csv", requireUser(db), async (c) => {
    const parsed = Query.safeParse(c.req.query());
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const { sites, from, to } = parsed.data;
    const siteIds = sites
      ? sites.split(",").map(Number)
      : (await all<{ id: number }>(db, "SELECT id FROM sites")).map((s) =>
        s.id
      );
    // The days are inclusive: from its first to the last millisecond.
    const where: string[] = [];
    const params: (string | number)[] = [];
    if (siteIds.length) {
      where.push(`h.site_id IN (${siteIds.map(() => "?").join(", ")})`);
      params.push(...siteIds);
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
              h.referrer, h.country, h.device
       FROM hits h JOIN sites s ON s.id = h.site_id
       WHERE ${where.join(" AND ")}
       ORDER BY h.created_at, h.id`,
      params,
    );
    const csv = [COLUMNS]
      .concat(
        hits.map((h) =>
          [
            h.site,
            h.created_at,
            h.kind,
            h.path,
            h.name,
            h.referrer,
            h.country,
            h.device,
          ].map(cell).join(",")
        ),
      )
      .join("\n") + "\n";
    return c.body(csv, 200, {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": 'attachment; filename="potoo-export.csv"',
    });
  });

  return app;
}
