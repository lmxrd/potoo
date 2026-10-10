import { stringify } from "@std/csv";
import type { SiteStats, StatsRange } from "./stats.ts";

/** The range of a stats view, as one CSV header line. */
export function rangeLine(range: StatsRange): string {
  if ("days" in range) {
    if (range.days === "today") return "today";
    return range.days === 1 ? "last 24 hours" : `last ${range.days} days`;
  }
  const minute = (d: Date) => `${d.toISOString().slice(0, 16)}Z`;
  return `${minute(range.from)} to ${minute(range.to)}`;
}

/**
 * One dashboard view's aggregated numbers, as one CSV document: a header
 * with the range, the filter and the totals, then one section per
 * breakdown. The columns match the breakdowns exactly.
 */
export function statsCsv(
  stats: SiteStats,
  meta: { range: string; filter?: string },
): string {
  const rows: (string | number)[][] = [];
  const line = (...cells: (string | number)[]) => rows.push(cells);
  line("# potoo stats", meta.range);
  if (meta.filter) line("# filter", meta.filter);
  line("# pageviews", stats.pageviews);
  line("# visitors", stats.visitors);

  let first = true;
  const section = (
    title: string,
    columns: string[],
    data: (string | number)[][],
  ) => {
    if (!first) line();
    first = false;
    line(`# ${title}`);
    line(...columns);
    rows.push(...data);
  };

  section(
    "Page views per period",
    ["period", "pageviews", "visitors"],
    stats.byDay.map((d) => [d.day, d.pageviews, d.visitors]),
  );
  section(
    "Paths",
    ["path", "pageviews", "visitors"],
    stats.paths.map((p) => [p.path, p.pageviews, p.visitors]),
  );
  section(
    "Referrers",
    ["referrer", "pageviews"],
    stats.referrers.map((r) => [r.referrer, r.pageviews]),
  );
  section(
    "Countries",
    ["country", "visitors"],
    stats.countries.map((c) => [c.country, c.visitors]),
  );
  section(
    "Devices",
    ["device", "visitors"],
    stats.devices.map((d) => [d.device, d.visitors]),
  );
  section(
    "Browsers",
    ["browser", "visitors"],
    stats.browsers.map((b) => [b.browser, b.visitors]),
  );
  section(
    "Operating systems",
    ["os", "visitors"],
    stats.os.map((o) => [o.os, o.visitors]),
  );
  section(
    "Clicks",
    ["name", "clicks"],
    stats.clicks.map((c) => [c.name, c.clicks]),
  );
  return stringify(rows);
}
