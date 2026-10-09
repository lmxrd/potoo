import { all, type Db } from "../db.ts";

/**
 * The stats of one dashboard: the totals, and the breakdowns the dashboard
 * shows, over a selected day range. The shape is the same for one site and
 * for several sites combined.
 */
export type SiteStats = {
  days: number | "today";
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
 * The buckets the per-period chart shows for a range. The periods are
 * calendar aligned and always complete: today is its 24 hours, the last 24
 * hours are the 24 started hours up to now, up to 30 days is one bucket per
 * day, up to 90 days is one per week (Mondays), and a year is one per month.
 */
function buckets(
  days: number | "today",
): { keys: string[]; since: string; sql: string } {
  const DAY = 86_400_000;
  const now = new Date();
  const todayStart = Date.UTC(
    now.getUTCFullYear(),
    now.getUTCMonth(),
    now.getUTCDate(),
  );
  const date = (ms: number) => new Date(ms).toISOString().slice(0, 10);

  if (days === "today") {
    const day = date(todayStart);
    return {
      keys: Array.from(
        { length: 24 },
        (_, h) => `${day}T${String(h).padStart(2, "0")}:00`,
      ),
      since: `${day}T00:00:00.000`,
      sql: `substr(created_at, 1, 13) || ':00'`,
    };
  }
  if (days === 1) {
    // The last 24 started hours, ending with the current one.
    const hourStart = todayStart + now.getUTCHours() * 3_600_000;
    const label = (ms: number) =>
      new Date(ms).toISOString().slice(0, 13) + ":00";
    return {
      keys: Array.from(
        { length: 24 },
        (_, i) => label(hourStart - (23 - i) * 3_600_000),
      ),
      since: new Date(hourStart - 23 * 3_600_000).toISOString(),
      sql: `substr(created_at, 1, 13) || ':00'`,
    };
  }
  if (days <= 30) {
    return {
      keys: Array.from(
        { length: days },
        (_, i) => date(todayStart - (days - 1 - i) * DAY),
      ),
      since: `${date(todayStart - (days - 1) * DAY)}T00:00:00.000`,
      sql: "date(created_at)",
    };
  }
  if (days <= 90) {
    // Weeks start on Monday; 13 of them cover 90 days.
    const weekday = new Date(todayStart).getUTCDay(); // 0 is Sunday
    const thisMonday = todayStart - ((weekday + 6) % 7) * DAY;
    return {
      keys: Array.from(
        { length: 13 },
        (_, i) => date(thisMonday - (12 - i) * 7 * DAY),
      ),
      since: `${date(thisMonday - 12 * 7 * DAY)}T00:00:00.000`,
      sql:
        `date(created_at, '-' || ((cast(strftime('%w', created_at) as integer) + 6) % 7) || ' days')`,
    };
  }
  // Months: the current one and the 11 before it.
  return {
    keys: Array.from(
      { length: 12 },
      (_, i) =>
        date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - (11 - i), 1)),
    ),
    since: `${
      date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - 11, 1))
    }T00:00:00.000`,
    sql: `substr(created_at, 1, 7) || '-01'`,
  };
}

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
  days: number | "today",
): Promise<SiteStats> {
  if (!siteIds.length) return { days, ...EMPTY };
  const { keys, since, sql } = buckets(days);
  const inSites = `site_id IN (${siteIds.map(() => "?").join(", ")})`;
  const q = <T>(select: string, tail = "") =>
    all<T>(
      db,
      `SELECT ${select} FROM hits
       WHERE ${inSites} AND created_at >= ? ${tail}`,
      [...siteIds, since],
    );
  const pv = "kind = 'pageview'";

  const [totals, hitDays, paths, countries, devices, referrers, clicks] =
    await Promise.all([
      q<{ pageviews: number; visitors: number }>(
        "COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors",
        `AND ${pv}`,
      ),
      q<{ day: string; pageviews: number; visitors: number }>(
        `${sql} AS day, COUNT(*) AS pageviews, COUNT(DISTINCT visitor) AS visitors`,
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
  // Zero fill the periods without hits, so the chart shows every bucket.
  const hits = new Map(hitDays.map((d) => [d.day, d]));
  const byDay = keys.map((day) => ({
    day,
    pageviews: hits.get(day)?.pageviews ?? 0,
    visitors: hits.get(day)?.visitors ?? 0,
  }));
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
