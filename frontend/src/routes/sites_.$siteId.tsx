import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { StatsDashboard, type SiteStats } from "../components/stats-dashboard";
import { api, useApi } from "../lib/api";

type Stats = SiteStats & {
  site: { id: number; hostname: string; honor_dnt: number };
};

export const Route = createFileRoute("/sites_/$siteId")({ component: SitePage });

function SitePage() {
  const { siteId } = Route.useParams();
  const { session } = Route.useRouteContext();
  const navigate = useNavigate();
  const [days, setDays] = useState<number | "today">(7);
  const { data, error, reload } = useApi<Stats>(`/sites/${siteId}/stats?days=${days}`);
  const isAdmin = session.user?.is_admin === 1;

  if (error) return <ErrorText>{error}</ErrorText>;
  if (!data) return <p className="text-slate-500">Loading…</p>;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-semibold">{data.site.hostname}</h1>
        <div className="flex items-center gap-4">
          <select
            className="rounded border border-slate-300 px-2 py-1 text-sm"
            value={days}
            onChange={(e) => {
              setDays(e.target.value === "today" ? ("today" as const) : Number(e.target.value));
            }}
          >
            <option value="today">Today</option>
            {[1, 7, 30, 90, 365].map((d) => (
              <option key={d} value={d}>
                {d === 1 ? "Last 24 hours" : `Last ${d} days`}
              </option>
            ))}
          </select>
        </div>
      </div>

      <StatsDashboard stats={data} dateFormat={session.user?.date_format} />

      <Snippet hostname={data.site.hostname} />

      {isAdmin && (
        <Card title="Site settings">
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={data.site.honor_dnt === 1}
              onChange={async (e) => {
                const honorDnt = e.target.checked;
                try {
                  await api(`/sites/${siteId}`, {
                    method: "PATCH",
                    body: { honor_dnt: honorDnt },
                  });
                  reload();
                } catch {
                  reload();
                }
              }}
            />
            Honor Do Not Track / Global Privacy Control
          </label>
          <p className="mt-2 text-xs text-slate-500">
            When enabled, visitors who ask not to be tracked are ignored.
          </p>
        </Card>
      )}

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
