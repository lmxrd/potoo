import { BarList, Card } from "./components";
import { chartLabel, formatDate } from "./dates";

/**
 * The stats of one dashboard: the same shape the backend returns for one
 * site and for several sites combined.
 */
export type SiteStats = {
  days: number | "today";
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

/** The dashboard content: totals, the chart and the breakdowns. */
export function StatsDashboard({
  stats,
  dateFormat = "YYYY-MM-DD",
}: {
  stats: SiteStats;
  dateFormat?: string;
}) {
  const maxDay = Math.max(1, ...stats.byDay.map((d) => d.pageviews));
  return (
    <>
      <div className="grid grid-cols-2 gap-4">
        <Card title="Page views">
          <div className="text-3xl font-semibold tabular-nums">{stats.pageviews}</div>
        </Card>
        <Card title="Visitors (unique per day)">
          <div className="text-3xl font-semibold tabular-nums">{stats.visitors}</div>
        </Card>
      </div>

      <Card title="Page views">
        <div className="flex items-end gap-1">
          {stats.byDay.map((d, i) => (
            <div key={d.day} className="flex flex-1 flex-col items-center gap-1">
              <div
                className="flex h-24 w-full items-end"
                title={`${
                  d.day.length > 10
                    ? `${formatDate(d.day.slice(0, 10), dateFormat)} ${d.day.slice(11, 16)}`
                    : formatDate(d.day, dateFormat)
                }: ${d.pageviews}`}
              >
                <div
                  className="w-full rounded-t bg-slate-800"
                  style={{ height: `${(d.pageviews / maxDay) * 100}%` }}
                />
              </div>
              {/* Thin the labels, so at most about 12 of them show. */}
              <span className="h-3 text-[10px] text-slate-400">
                {i % Math.max(1, Math.ceil(stats.byDay.length / 12)) === 0
                  ? chartLabel(d.day, stats.days)
                  : "\u00a0"}
              </span>
            </div>
          ))}
        </div>
      </Card>

      <div className="grid gap-4 md:grid-cols-2">
        <Card title="Paths">
          <BarList rows={stats.paths.map((p) => ({ label: p.path, value: p.pageviews }))} />
        </Card>
        <Card title="Referrers">
          <BarList
            rows={stats.referrers.map((r) => ({
              label: r.referrer,
              value: r.pageviews,
            }))}
          />
        </Card>
        <Card title="Countries">
          <BarList
            rows={stats.countries.map((c) => ({
              label: c.country,
              value: c.visitors,
            }))}
          />
        </Card>
        <Card title="Devices">
          <BarList
            rows={stats.devices.map((d) => ({
              label: d.device,
              value: d.visitors,
            }))}
          />
        </Card>
        <Card title="Browsers">
          <BarList
            rows={stats.browsers.map((b) => ({
              label: b.browser,
              value: b.visitors,
            }))}
          />
        </Card>
        <Card title="Operating systems">
          <BarList
            rows={stats.os.map((o) => ({
              label: o.os,
              value: o.visitors,
            }))}
          />
        </Card>
        <Card title="Clicks">
          <BarList
            rows={stats.clicks.map((c) => ({
              label: c.name,
              value: c.clicks,
            }))}
            empty="No clicks tracked. Enable click tracking in the snippet."
          />
        </Card>
      </div>
    </>
  );
}
