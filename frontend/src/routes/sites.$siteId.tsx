import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { api, useApi } from "../api";
import { BarList, Button, Card, ErrorText } from "../components";

type Stats = {
  site: { id: number; hostname: string };
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

export const Route = createFileRoute("/sites/$siteId")({ component: SitePage });

function SitePage() {
  const { siteId } = Route.useParams();
  const { session } = Route.useRouteContext();
  const navigate = useNavigate();
  const [days, setDays] = useState(7);
  const { data, error } = useApi<Stats>(`/sites/${siteId}/stats?days=${days}`);
  const isAdmin = session.user?.is_admin === 1;

  if (error) return <ErrorText>{error}</ErrorText>;
  if (!data) return <p className="text-slate-500">Loading…</p>;
  const maxDay = Math.max(1, ...data.byDay.map((d) => d.pageviews));

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">{data.site.hostname}</h1>
        <select
          className="rounded border border-slate-300 px-2 py-1 text-sm"
          value={days}
          onChange={(e) => setDays(Number(e.target.value))}
        >
          {[1, 7, 30, 90, 365].map((d) => (
            <option key={d} value={d}>
              Last {d} day{d > 1 && "s"}
            </option>
          ))}
        </select>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <Card title="Page views">
          <div className="text-3xl font-semibold tabular-nums">{data.pageviews}</div>
        </Card>
        <Card title="Visitors (unique per day)">
          <div className="text-3xl font-semibold tabular-nums">{data.visitors}</div>
        </Card>
      </div>

      <Card title="Page views per day">
        <div className="flex h-24 items-end gap-1">
          {data.byDay.map((d) => (
            <div
              key={d.day}
              title={`${d.day}: ${d.pageviews}`}
              className="flex-1 rounded-t bg-slate-800"
              style={{ height: `${(d.pageviews / maxDay) * 100}%` }}
            />
          ))}
        </div>
      </Card>

      <div className="grid gap-4 md:grid-cols-2">
        <Card title="Paths">
          <BarList rows={data.paths.map((p) => ({ label: p.path, value: p.pageviews }))} />
        </Card>
        <Card title="Referrers">
          <BarList rows={data.referrers.map((r) => ({ label: r.referrer, value: r.pageviews }))} />
        </Card>
        <Card title="Countries">
          <BarList rows={data.countries.map((c) => ({ label: c.country, value: c.visitors }))} />
        </Card>
        <Card title="Devices">
          <BarList rows={data.devices.map((d) => ({ label: d.device, value: d.visitors }))} />
        </Card>
        <Card title="Clicks">
          <BarList
            rows={data.clicks.map((c) => ({ label: c.name, value: c.clicks }))}
            empty="No clicks tracked. Enable click tracking in the snippet below."
          />
        </Card>
      </div>

      <Snippet hostname={data.site.hostname} />

      {isAdmin && (
        <Button
          variant="danger"
          onClick={async () => {
            if (confirm(`Delete ${data.site.hostname} and all its data?`)) {
              await api(`/sites/${siteId}`, { method: "DELETE" });
              await navigate({ to: "/" });
            }
          }}
        >
          Delete site
        </Button>
      )}
    </div>
  );
}

function Snippet({ hostname }: { hostname: string }) {
  const [outbound, setOutbound] = useState(false);
  const [clicks, setClicks] = useState(false);
  const [pixel, setPixel] = useState(false);
  const origin = location.origin;
  const script = `<script async src="${origin}/api/tracker.js"${clicks ? " data-clicks" : ""}${
    outbound ? " data-outbound" : ""
  }></script>`;
  const img = `<noscript><img src="${origin}/api/collect.gif?h=${hostname}" alt="" /></noscript>`;
  return (
    <Card title="Tracking snippet">
      <div className="mb-3 flex flex-wrap gap-4 text-sm">
        <label className="flex items-center gap-2">
          <input type="checkbox" checked={clicks} onChange={(e) => setClicks(e.target.checked)} />
          Track elements with data-potoo-click="label"
        </label>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={outbound}
            onChange={(e) => setOutbound(e.target.checked)}
          />
          Track outbound links
        </label>
        <label className="flex items-center gap-2">
          <input type="checkbox" checked={pixel} onChange={(e) => setPixel(e.target.checked)} />
          No-JS pixel fallback
        </label>
      </div>
      <pre className="overflow-x-auto rounded bg-slate-100 p-3 text-xs">
        {script + (pixel ? `\n${img}` : "")}
      </pre>
      <p className="mt-2 text-xs text-slate-500">
        No cookies or IP addresses are stored. Visitors are counted with a hash that rotates daily.
      </p>
    </Card>
  );
}
