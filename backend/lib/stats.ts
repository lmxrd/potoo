import { all, type Db } from "../db.ts";

/**
 * The stats of one dashboard: the totals, and the breakdowns the dashboard
 * shows, over a selected day range. The shape is the same for one site and
 * for several sites combined.
 */
export type SiteStats = {
  days: number;
  pageviews: number;
  visitors: number;
  byDay: { day: string; pageviews: number; visitors: number }[];
  paths: { path: string; pageviews: number; visitors: number }[];
  countries: { country: string; visitors: number }[];
  devices: { device: string; visitors: number }[];
  referrers: { referrer: string; pageviews: number }[];
  clicks: { name: string; clicks: number }[];
};

const EMPTY: Omit<SiteStats, "days"> = {
  pageviews: 0,
  visitors: 0,
  byDay: [],
  paths: [],
  countries: [],
  devices: [],
  referrers: [],
  clicks: [],
};

/**
 * Aggregate the hits of the given sites over the last `days` days.
 *
 * "Visitors" counts distinct daily hashes, which include the site id, so
 * the same person on two of the sites counts as two visitors here: visitors
 * cannot be linked across sites.
 */
export async function statsFor(
  db: Db,
  siteIds: number[],
  days: number,
): Promise<SiteStats> {
  if (!siteIds.length) return { days, ...EMPTY };
  const since = new Date(Date.now() - days * 86_400_000).toISOString();
  const inSites = `site_id IN (${siteIds.map(() => "?").join(", ")})`;
  const q = <T>(select: string, tail = "") =>
    all<T>(
      db,
      `SELECT ${select} FROM hits
       WHERE ${inSites} AND created_at >= ? ${tail}`,
      [...siteIds, since],
    );
  const pv = "kind = 'pageview'";

  const [totals, byDay, paths, countries, devices, referrers, clicks] =
    await Promise.all([
      q<{ pageviews: number; visitors: number }>(
        "COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
        `AND ${pv}`,
      ),
      q<{ day: string; pageviews: number; visitors: number }>(
        "date(created_at) AS day, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
        `AND ${pv} GROUP BY day ORDER BY day`,
      ),
      q<{ path: string; pageviews: number; visitors: number }>(
        "path, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
        `AND ${pv} GROUP BY path ORDER BY pageviews DESC LIMIT 50`,
      ),
      q<{ country: string; visitors: number }>(
        "COALESCE(country, '??') AS country, COUNT(DISTINCT visitor) AS visitors",
        `AND ${pv} GROUP BY country ORDER BY visitors DESC LIMIT 50`,
      ),
      q<{ device: string; visitors: number }>(
        "device, COUNT(DISTINCT visitor) AS visitors",
        `AND ${pv} GROUP BY device ORDER BY visitors DESC`,
      ),
      q<{ referrer: string; pageviews: number }>(
        "referrer, COUNT(*) AS pageviews",
        `AND ${pv} AND referrer IS NOT NULL GROUP BY referrer ORDER BY pageviews DESC LIMIT 20`,
      ),
      q<{ name: string; clicks: number }>(
        "name, COUNT(*) AS clicks",
        `AND kind = 'click' GROUP BY name ORDER BY clicks DESC LIMIT 50`,
      ),
    ]);
  return {
    days,
    ...totals[0],
    byDay,
    paths,
    countries,
    devices,
    referrers,
    clicks,
  };
}
