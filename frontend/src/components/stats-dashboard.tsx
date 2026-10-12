import { Card } from "./card";
import { BarList } from "./bar-list";
import { PageViewsChart } from "./page-views-chart";
import { bucketLabel, chartLabel, formatDate, formatTime } from "../lib/dates";
import type { SiteStats } from "../client";
import type { FacetFilters } from "../lib/filter";
import { facetLabel } from "../lib/labels";

/** The dashboard content: totals, the chart and the breakdowns. */
export function StatsDashboard({
  stats,
  dateFormat = "YYYY-MM-DD",
  use24 = true,
  filters,
  onToggleFilter,
}: {
  stats: SiteStats;
  dateFormat?: string;
  use24?: boolean;
  filters: FacetFilters;
  onToggleFilter: (facet: string, value: string) => void;
}) {
  const label = (day: string) =>
    stats.bucket ? bucketLabel(day, stats.bucket, use24) : chartLabel(day, stats.days, use24);
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
        <PageViewsChart
          rows={stats.byDay}
          axisLabel={label}
          describe={(day) =>
            day.length > 10
              ? `${formatDate(day.slice(0, 10), dateFormat)} ${formatTime(day.slice(11, 16), use24)}`
              : formatDate(day, dateFormat)
          }
        />
      </Card>

      <div className="grid gap-4 md:grid-cols-2">
        <Card title="Paths">
          <BarList
            rows={stats.paths.map((p) => ({ label: p.path, value: p.pageviews }))}
            active={filters.get("path")}
            onToggle={(value) => onToggleFilter("path", value)}
          />
        </Card>
        <Card title="Referrers">
          <BarList
            rows={stats.referrers.map((r) => ({ label: r.referrer, value: r.pageviews }))}
            active={filters.get("referrer")}
            onToggle={(value) => onToggleFilter("referrer", value)}
          />
        </Card>
        <Card title="Countries">
          <BarList
            rows={stats.countries.map((c) => ({
              label: c.country,
              value: c.visitors,
            }))}
            active={filters.get("country")}
            onToggle={(value) => onToggleFilter("country", value)}
            format={(value) => facetLabel("country", value)}
          />
        </Card>
        <Card title="Devices">
          <BarList
            rows={stats.devices.map((d) => ({
              label: d.device,
              value: d.visitors,
            }))}
            active={filters.get("device")}
            onToggle={(value) => onToggleFilter("device", value)}
            format={(value) => facetLabel("device", value)}
          />
        </Card>
        <Card title="Browsers">
          <BarList
            rows={stats.browsers.map((b) => ({
              label: b.browser,
              value: b.visitors,
            }))}
            active={filters.get("browser")}
            onToggle={(value) => onToggleFilter("browser", value)}
            format={(value) => facetLabel("browser", value)}
          />
        </Card>
        <Card title="Operating systems">
          <BarList
            rows={stats.os.map((o) => ({
              label: o.os,
              value: o.visitors,
            }))}
            active={filters.get("os")}
            onToggle={(value) => onToggleFilter("os", value)}
            format={(value) => facetLabel("os", value)}
          />
        </Card>
        <Card title="Clicks">
          <BarList
            rows={stats.clicks.map((c) => ({
              label: c.name,
              value: c.clicks,
            }))}
            empty="No clicks tracked. Enable click tracking in the snippet."
            active={filters.get("name")}
            onToggle={(value) => onToggleFilter("name", value)}
          />
        </Card>
      </div>
    </>
  );
}
