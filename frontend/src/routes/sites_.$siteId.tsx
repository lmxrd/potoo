import { useState } from "react";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  deleteSiteMutation,
  getSiteOptions,
  getSiteQueryKey,
  getStatsOptions,
  updateSiteMutation,
} from "../client/@tanstack/react-query.gen";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { DateRangeBar } from "../components/date-range-bar";
import { ErrorText } from "../components/error-text";
import { StatsDashboard } from "../components/stats-dashboard";
import { errorMessage } from "../lib/api";
import { exportHref, statsQuery } from "../lib/stats-query";
import { buildFilter, useDashboardFilters } from "../lib/filter";
import { useDateRange, validateDashboardSearch } from "../lib/date-range";

export const Route = createFileRoute("/sites_/$siteId")({
  validateSearch: validateDashboardSearch,
  component: SitePage,
});

function SitePage() {
  const { siteId } = Route.useParams();
  const { session } = Route.useRouteContext();
  const navigate = useNavigate();
  const { range, setRange, query } = useDateRange();
  const { filters, toggle, setFilters } = useDashboardFilters();
  const queryClient = useQueryClient();
  const params = statsQuery(query, siteId, buildFilter(filters));
  const isAdmin = session.user?.is_admin;

  const { data: site, error: siteError } = useQuery(
    getSiteOptions({ path: { id: Number(siteId) } }),
  );
  // The previous numbers stay on screen while the next range loads.
  const { data: stats, error: statsError } = useQuery({
    ...getStatsOptions({ query: params ?? undefined }),
    enabled: params !== null,
    placeholderData: keepPreviousData,
  });
  const updateSite = useMutation(updateSiteMutation());
  const deleteSite = useMutation(deleteSiteMutation());
  const reloadSite = () =>
    queryClient.invalidateQueries({ queryKey: getSiteQueryKey({ path: { id: Number(siteId) } }) });

  if (siteError) return <ErrorText>{errorMessage(siteError)}</ErrorText>;
  if (!site) return <p className="text-muted">Loading…</p>;

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">{site.hostname}</h1>

      <DateRangeBar
        range={range}
        onRange={setRange}
        stats={stats}
        dateFormat={session.user?.date_format}
        timeFormat={session.user?.time_format}
        filters={filters}
        onFilters={setFilters}
        exportUrl={params ? exportHref(params) : undefined}
      />

      <ErrorText>{statsError && errorMessage(statsError)}</ErrorText>
      {stats ? (
        <StatsDashboard
          stats={stats}
          dateFormat={session.user?.date_format}
          timeFormat={session.user?.time_format}
          filters={filters}
          onToggleFilter={toggle}
        />
      ) : (
        !statsError && <p className="text-muted">Loading…</p>
      )}

      <Snippet hostname={site.hostname} />

      {isAdmin && (
        <Card title="Site settings">
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={site.honor_dnt}
              onChange={async (e) => {
                try {
                  await updateSite.mutateAsync({
                    path: { id: Number(siteId) },
                    body: { honor_dnt: e.target.checked },
                  });
                } catch {
                  // Not saved: the reload below shows what the server has.
                }
                await reloadSite();
              }}
            />
            Honor Do Not Track / Global Privacy Control
          </label>
          <p className="mt-2 text-xs text-muted">
            When enabled, visitors who ask not to be tracked are ignored.
          </p>
        </Card>
      )}

      {isAdmin && (
        <Button
          variant="danger"
          onClick={async () => {
            if (confirm(`Delete ${site.hostname} and all its data?`)) {
              await deleteSite.mutateAsync({ path: { id: Number(siteId) } });
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
  const script = `<script async src="${origin}/tracker.js"${clicks ? " data-clicks" : ""}${
    outbound ? " data-outbound" : ""
  }></script>`;
  const img = `<noscript><img src="${origin}/api/pixel?h=${hostname}" alt="" /></noscript>`;
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
      <pre className="overflow-x-auto rounded bg-subtle p-3 text-xs">
        {script + (pixel ? `\n${img}` : "")}
      </pre>
      <p className="mt-2 text-xs text-muted">
        No cookies or IP addresses are stored. Visitors are counted with a hash that rotates daily.
      </p>
    </Card>
  );
}
