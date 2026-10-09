import { all, type Db } from "../db.ts";

/** The buckets a custom range's chart can use. */
export type Bucket = "hour" | "day" | "week" | "month";

/**
 * The stats of one dashboard: the totals, and the breakdowns the dashboard
 * shows, over a selected day range. The shape is the same for one site and
 * for several sites combined.
 */
export type SiteStats = {
  days: number | "today";
  // Set only for explicit from/to ranges: the bucket the chart shows.
  bucket?: Bucket;
  pageviews: number;
  visitors: number;
  byDay: { day: string; pageviews: number; visitors: number }[];
  paths: { path: string; pageviews: number; visitors: number }[];
  countries: { country: string; visitors: number }[];
  devices: { device: string; visitors: number }[];
  browsers: { browser: string; visitors: number }[];
  os: { os: string; visitors: number }[];
  referrers: { referrer: string; pageviews: number }[];
  clicks: { name: string; clicks: number }[];
};

/** A preset range of days, or an explicit UTC range with minute precision. */
export type StatsRange =
  | { days: number | "today" }
  | { from: Date; to: Date };

const DAY = 86_400_000;
const MINUTE = 60_000;
const MINUTE_ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/;

const EMPTY: Omit<SiteStats, "days" | "bucket"> = {
  pageviews: 0,
  visitors: 0,
  byDay: [],
  paths: [],
  countries: [],
  devices: [],
  browsers: [],
  os: [],
  referrers: [],
  clicks: [],
};

/**
 * Parse the range of a stats request: a `days` preset, or an explicit
 * `from`/`to` pair as `YYYY-MM-DDTHH:MM` in UTC. The `to` minute is
 * included.
 */
export function parseRange(
  days: string | undefined,
  from: string | undefined,
  to: string | undefined,
): { ok: true; range: StatsRange } | { ok: false; error: string } {
  if (from !== undefined || to !== undefined) {
    if (!from || !to) {
      return {
        ok: false,
        error: "from and to are both required, as YYYY-MM-DDTHH:MM in UTC",
      };
    }
    if (!MINUTE_ISO.test(from) || !MINUTE_ISO.test(to)) {
      return {
        ok: false,
        error: "from and to must be YYYY-MM-DDTHH:MM in UTC",
      };
    }
    const start = new Date(`${from}:00Z`).getTime();
    const end = new Date(`${to}:00Z`).getTime();
    if (Number.isNaN(start) || Number.isNaN(end) || start >= end) {
      return { ok: false, error: "from must be before to" };
    }
    if (end - start > 730 * DAY) {
      return { ok: false, error: "the range can span at most 730 days" };
    }
    return { ok: true, range: { from: new Date(start), to: new Date(end) } };
  }
  if (days === undefined || days === "today") {
    return { ok: true, range: { days: days === "today" ? "today" : 7 } };
  }
  const n = Number(days);
  if (!Number.isInteger(n) || n < 1 || n > 365) {
    return {
      ok: false,
      error: 'days must be an integer from 1 to 365, or "today"',
    };
  }
  return { ok: true, range: { days: n } };
}

/** The buckets a range's chart is aggregated over. */
type Buckets = {
  keys: string[];
  since: string;
  until?: string;
  sql: string;
  bucket?: Bucket;
};

function buckets(range: StatsRange): Buckets {
  return "days" in range ? presetBuckets(range.days) : customBuckets(range);
}

/**
 * The buckets of a preset range. The periods are calendar aligned and
 * always complete: today is its 24 hours, the last 24 hours are the 24
 * started hours up to now, up to 30 days is one bucket per day, up to 90
 * days is one per week (Mondays), and a year is one per month.
 */
function presetBuckets(days: number | "today"): Buckets {
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
 * The buckets of an explicit range. The bucket size follows the span: hours
 * for up to two days, days for up to 35, weeks (Mondays) for up to 180, and
 * months beyond. Buckets are calendar aligned, but the first and last are
 * clipped to the range itself.
 */
function customBuckets({ from, to }: { from: Date; to: Date }): Buckets {
  const HOUR = 3_600_000;
  const start = from.getTime();
  const end = to.getTime() + MINUTE; // the `to` minute is included
  const span = end - start;
  const iso = (ms: number) => new Date(ms).toISOString();
  const date = (ms: number) => iso(ms).slice(0, 10);
  const midnight = (ms: number) => {
    const d = new Date(ms);
    return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
  };
  const until = iso(end);

  if (span <= 48 * HOUR) {
    const first = Math.floor(start / HOUR) * HOUR;
    const last = Math.floor((end - 1) / HOUR) * HOUR;
    return {
      bucket: "hour",
      keys: Array.from(
        { length: (last - first) / HOUR + 1 },
        (_, i) => iso(first + i * HOUR).slice(0, 13) + ":00",
      ),
      since: iso(start),
      until,
      sql: `substr(created_at, 1, 13) || ':00'`,
    };
  }
  if (span <= 35 * DAY) {
    const first = midnight(start);
    const last = midnight(end - 1);
    return {
      bucket: "day",
      keys: Array.from(
        { length: (last - first) / DAY + 1 },
        (_, i) => date(first + i * DAY),
      ),
      since: iso(start),
      until,
      sql: "date(created_at)",
    };
  }
  if (span <= 180 * DAY) {
    const monday = (ms: number) =>
      ms - ((new Date(ms).getUTCDay() + 6) % 7) * DAY;
    const first = monday(midnight(start));
    const last = monday(midnight(end - 1));
    return {
      bucket: "week",
      keys: Array.from(
        { length: (last - first) / (7 * DAY) + 1 },
        (_, i) => date(first + i * 7 * DAY),
      ),
      since: iso(start),
      until,
      sql:
        `date(created_at, '-' || ((cast(strftime('%w', created_at) as integer) + 6) % 7) || ' days')`,
    };
  }
  const firstYear = new Date(start).getUTCFullYear();
  const firstMonth = new Date(start).getUTCMonth();
  const last = new Date(end - 1);
  const months = (last.getUTCFullYear() - firstYear) * 12 +
    last.getUTCMonth() - firstMonth + 1;
  return {
    bucket: "month",
    keys: Array.from(
      { length: months },
      (_, i) => date(Date.UTC(firstYear, firstMonth + i, 1)),
    ),
    since: iso(start),
    until,
    sql: `substr(created_at, 1, 7) || '-01'`,
  };
}

/**
 * Aggregate the hits of the given sites over a range.
 *
 * "Visitors" counts distinct daily hashes, which include the site id, so
 * the same person on two of the sites counts as two visitors here: visitors
 * cannot be linked across sites.
 */
export async function statsFor(
  db: Db,
  siteIds: number[],
  range: StatsRange,
): Promise<SiteStats> {
  const days = "days" in range
    ? range.days
    : Math.ceil((range.to.getTime() - range.from.getTime()) / DAY);
  if (!siteIds.length) return { days, ...EMPTY };
  const { keys, since, until, sql, bucket } = buckets(range);
  const inSites = `site_id IN (${siteIds.map(() => "?").join(", ")})`;
  const within = until
    ? "created_at >= ? AND created_at < ?"
    : "created_at >= ?";
  const params = until ? [since, until] : [since];
  const q = <T>(select: string, tail = "") =>
    all<T>(
      db,
      `SELECT ${select} FROM hits
       WHERE ${inSites} AND ${within} ${tail}`,
      [...siteIds, ...params],
    );
  const pv = "kind = 'pageview'";

  const [
    totals,
    hitDays,
    paths,
    countries,
    devices,
    browsers,
    os,
    referrers,
    clicks,
  ] = await Promise.all([
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
    q<{ browser: string; visitors: number }>(
      "browser, COUNT(DISTINCT visitor) AS visitors",
      `AND ${pv} GROUP BY browser ORDER BY visitors DESC`,
    ),
    q<{ os: string; visitors: number }>(
      "os, COUNT(DISTINCT visitor) AS visitors",
      `AND ${pv} GROUP BY os ORDER BY visitors DESC`,
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
  const stats: SiteStats = {
    days,
    ...totals[0],
    byDay,
    paths,
    countries,
    devices,
    browsers,
    os,
    referrers,
    clicks,
  };
  if (bucket) stats.bucket = bucket;
  return stats;
}
