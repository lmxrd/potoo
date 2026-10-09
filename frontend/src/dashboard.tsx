import { BarList, Card } from "./components";
import { formatDate } from "./dates";

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
  referrers: { referrer: string; pageviews: number }[];
  clicks: { name: string; clicks: number }[];
};

/** The dashboard content: totals, views per day and the breakdowns. */
export function StatsDashboard({
  stats,
  dateFormat = "YYYY-MM-DD",
  granularity = "day",
  onGranularity,
  hourAvailable = false,
}: {
  stats: SiteStats;
  dateFormat?: string;
  granularity?: "day" | "hour";
  onGranularity?: (granularity: "day" | "hour") => void;
  hourAvailable?: boolean;
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

      <Card>
        <div className="mb-3 flex items-center justify-between gap-4">
          <h2 className="text-sm font-medium text-slate-500">Page views per {granularity}</h2>
          {onGranularity && (
            <select
              className="rounded border border-slate-300 px-2 py-1 text-sm"
              value={granularity}
              onChange={(e) => onGranularity(e.target.value as "day" | "hour")}
            >
              <option value="day">Per day</option>
              <option value="hour" disabled={!hourAvailable}>
                Per hour
              </option>
            </select>
          )}
        </div>
        <div className="flex h-24 items-end gap-1">
          {stats.byDay.map((d) => (
            <div
              key={d.day}
              title={`${
                d.day.length > 10
                  ? `${formatDate(d.day.slice(0, 10), dateFormat)} ${d.day.slice(11, 16)}`
                  : formatDate(d.day, dateFormat)
              }: ${d.pageviews}`}
              className="flex-1 rounded-t bg-slate-800"
              style={{ height: `${(d.pageviews / maxDay) * 100}%` }}
            />
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
