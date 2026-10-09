import type { SiteStats } from "./api";

const cell = (value: string | number) =>
  typeof value === "number" ? String(value) : `"${value.replaceAll('"', '""')}"`;

const section = (title: string, columns: string[], rows: (string | number)[][]) =>
  [`# ${title}`, columns.join(","), ...rows.map((row) => row.map(cell).join(","))].join("\n");

/** The aggregated numbers of one dashboard view, as one CSV document. */
export function statsCsv(stats: SiteStats, range: string): string {
  return (
    [
      `# potoo stats, ${range}`,
      `# pageviews,${stats.pageviews}`,
      `# visitors,${stats.visitors}`,
      "",
      section(
        "Page views per period",
        ["period", "pageviews", "visitors"],
        stats.byDay.map((d) => [d.day, d.pageviews, d.visitors]),
      ),
      "",
      section(
        "Paths",
        ["path", "pageviews", "visitors"],
        stats.paths.map((p) => [p.path, p.pageviews, p.visitors]),
      ),
      "",
      section(
        "Referrers",
        ["referrer", "pageviews"],
        stats.referrers.map((r) => [r.referrer, r.pageviews]),
      ),
      "",
      section(
        "Countries",
        ["country", "visitors"],
        stats.countries.map((c) => [c.country, c.visitors]),
      ),
      "",
      section(
        "Devices",
        ["device", "visitors"],
        stats.devices.map((d) => [d.device, d.visitors]),
      ),
      "",
      section(
        "Browsers",
        ["browser", "visitors"],
        stats.browsers.map((b) => [b.browser, b.visitors]),
      ),
      "",
      section(
        "Operating systems",
        ["os", "visitors"],
        stats.os.map((o) => [o.os, o.visitors]),
      ),
      "",
      section(
        "Clicks",
        ["name", "clicks"],
        stats.clicks.map((c) => [c.name, c.clicks]),
      ),
    ].join("\n") + "\n"
  );
}

/** Download text as a file, without leaving the page. */
export function download(filename: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/csv" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}
