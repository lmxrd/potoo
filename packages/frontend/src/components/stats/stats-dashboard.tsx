import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { BarList } from "./bar-list";
import { PageViewsChart } from "./page-views-chart";
import { useFormatters } from "../../lib/use-formatters";
import type { SiteStats } from "potoo-sdk";
import type { FacetFilters } from "../../lib/filter";
import { facetLabel } from "../../lib/labels";

/** The dashboard content: totals, the chart and the breakdowns. */
export function StatsDashboard({
  stats,
  filters,
  onToggleFilter,
}: {
  stats: SiteStats;
  filters: FacetFilters;
  onToggleFilter: (facet: string, value: string) => void;
}) {
  const format = useFormatters();
  const label = (day: string) =>
    stats.bucket ? format.bucketLabel(day, stats.bucket) : format.chartLabel(day, stats.days);
  return (
    <>
      <div className="grid grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle>Page views</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-semibold tabular-nums">{stats.pageviews}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Visitors (unique per day)</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-semibold tabular-nums">{stats.visitors}</div>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Page views</CardTitle>
        </CardHeader>
        <CardContent>
          <PageViewsChart rows={stats.byDay} axisLabel={label} describe={format.bucket} />
        </CardContent>
      </Card>

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Paths</CardTitle>
          </CardHeader>
          <CardContent>
            <BarList
              rows={stats.paths.map((p) => ({ label: p.path, value: p.pageviews }))}
              active={filters.get("path")}
              onToggle={(value) => onToggleFilter("path", value)}
            />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Referrers</CardTitle>
          </CardHeader>
          <CardContent>
            <BarList
              rows={stats.referrers.map((r) => ({ label: r.referrer, value: r.pageviews }))}
              active={filters.get("referrer")}
              onToggle={(value) => onToggleFilter("referrer", value)}
            />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Countries</CardTitle>
          </CardHeader>
          <CardContent>
            <BarList
              rows={stats.countries.map((c) => ({
                label: c.country,
                value: c.visitors,
              }))}
              active={filters.get("country")}
              onToggle={(value) => onToggleFilter("country", value)}
              format={(value) => facetLabel("country", value)}
            />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Devices</CardTitle>
          </CardHeader>
          <CardContent>
            <BarList
              rows={stats.devices.map((d) => ({
                label: d.device,
                value: d.visitors,
              }))}
              active={filters.get("device")}
              onToggle={(value) => onToggleFilter("device", value)}
              format={(value) => facetLabel("device", value)}
            />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Browsers</CardTitle>
          </CardHeader>
          <CardContent>
            <BarList
              rows={stats.browsers.map((b) => ({
                label: b.browser,
                value: b.visitors,
              }))}
              active={filters.get("browser")}
              onToggle={(value) => onToggleFilter("browser", value)}
              format={(value) => facetLabel("browser", value)}
            />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Operating systems</CardTitle>
          </CardHeader>
          <CardContent>
            <BarList
              rows={stats.os.map((o) => ({
                label: o.os,
                value: o.visitors,
              }))}
              active={filters.get("os")}
              onToggle={(value) => onToggleFilter("os", value)}
              format={(value) => facetLabel("os", value)}
            />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Clicks</CardTitle>
          </CardHeader>
          <CardContent>
            <BarList
              rows={stats.clicks.map((c) => ({
                label: c.name,
                value: c.clicks,
              }))}
              empty="No clicks tracked. Enable click tracking in the snippet."
              active={filters.get("name")}
              onToggle={(value) => onToggleFilter("name", value)}
            />
          </CardContent>
        </Card>
      </div>
    </>
  );
}
