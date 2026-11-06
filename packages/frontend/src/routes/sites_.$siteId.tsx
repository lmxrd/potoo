import { useState } from "react";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  deleteSiteMutation,
  getSiteOptions,
  getSiteQueryKey,
  getStatsOptions,
  updateSiteMutation,
} from "potoo-sdk/react-query";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { DateRangeBar } from "../components/date-range-bar";
import { ErrorText } from "../components/error-text";
import { StatsDashboard } from "../components/stats-dashboard";
import { errorMessage } from "../lib/api";
import { exportHref, statsQuery } from "../lib/stats-query";
import { buildFilter, useDashboardFilters } from "../lib/filter";
import { useDateRange, validateDashboardSearch } from "../lib/date-range";
import { Checkbox } from "../components/ui/checkbox";
import { useConfirm } from "../components/confirm";
import { trackingSnippet } from "../lib/snippet";

export const Route = createFileRoute("/sites_/$siteId")({
  validateSearch: validateDashboardSearch,
  component: SitePage,
});

function SitePage() {
  const confirm = useConfirm();
  const { siteId } = Route.useParams();
  const { session } = Route.useRouteContext();
  const navigate = useNavigate();
  const { range, setRange, query } = useDateRange();
  const { filters, toggle, setFilters } = useDashboardFilters();
  const queryClient = useQueryClient();
  const isAdmin = session.user?.is_admin;

  const { data: site, error: siteError } = useQuery(getSiteOptions({ path: { id: siteId } }));
  // The stats are asked for by hostname, so they wait for the site.
  const params = site ? statsQuery(query, site.hostname, buildFilter(filters)) : null;
  // The previous numbers stay on screen while the next range loads.
  const { data: stats, error: statsError } = useQuery({
    // The query never runs while the range is invalid (`enabled`), so the
    // empty one it is built from is never sent.
    ...getStatsOptions({ query: params ?? { from: "", to: "" } }),
    enabled: params !== null,
    placeholderData: keepPreviousData,
  });
  const updateSite = useMutation({ ...updateSiteMutation(), meta: { inline: true } });
  const deleteSite = useMutation({ ...deleteSiteMutation(), meta: { inline: true } });
  const reloadSite = () =>
    queryClient.invalidateQueries({ queryKey: getSiteQueryKey({ path: { id: siteId } }) });

  if (siteError) return <ErrorText>{errorMessage(siteError)}</ErrorText>;
  if (!site) return <p className="text-muted-foreground">Loading…</p>;

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">{site.hostname}</h1>

      <DateRangeBar
        range={range}
        onRange={setRange}
        stats={stats}
        dateFormat={session.user?.date_format}
        use24={session.user?.use_24}
        filters={filters}
        onFilters={setFilters}
        exportUrl={params ? exportHref(params) : undefined}
      />

      <ErrorText>{statsError && errorMessage(statsError)}</ErrorText>
      {stats ? (
        <StatsDashboard
          stats={stats}
          dateFormat={session.user?.date_format}
          use24={session.user?.use_24}
          filters={filters}
          onToggleFilter={toggle}
        />
      ) : (
        !statsError && <p className="text-muted-foreground">Loading…</p>
      )}

      <Snippet hostname={site.hostname} />

      {isAdmin && (
        <Card>
          <CardHeader>
            <CardTitle>Site settings</CardTitle>
          </CardHeader>
          <CardContent>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={site.honor_dnt}
                onCheckedChange={async (checked) => {
                  try {
                    await updateSite.mutateAsync({
                      path: { id: siteId },
                      body: { honor_dnt: checked === true },
                    });
                  } catch {
                    // Not saved: the reload below shows what the server has.
                  }
                  await reloadSite();
                }}
              />
              Honor Do Not Track / Global Privacy Control
            </label>
            <p className="mt-2 text-xs text-muted-foreground">
              When enabled, visitors who ask not to be tracked are ignored.
            </p>
          </CardContent>
        </Card>
      )}

      {isAdmin && (
        <Button
          variant="destructive"
          onClick={async () => {
            if (
              await confirm({
                title: `Delete ${site.hostname}?`,
                description: "All its data is deleted with it.",
                action: "Delete",
                destructive: true,
              })
            ) {
              await deleteSite.mutateAsync({ path: { id: siteId } });
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
  const snippet = trackingSnippet(location.origin, hostname, { clicks, outbound, pixel });
  return (
    <Card>
      <CardHeader>
        <CardTitle>Tracking snippet</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="mb-3 flex flex-wrap gap-4 text-sm">
          <label className="flex items-center gap-2">
            <Checkbox checked={clicks} onCheckedChange={(checked) => setClicks(checked === true)} />
            Track elements with data-potoo-click="label"
          </label>
          <label className="flex items-center gap-2">
            <Checkbox
              checked={outbound}
              onCheckedChange={(checked) => setOutbound(checked === true)}
            />
            Track outbound links
          </label>
          <label className="flex items-center gap-2">
            <Checkbox checked={pixel} onCheckedChange={(checked) => setPixel(checked === true)} />
            No-JS pixel fallback
          </label>
        </div>
        <pre className="overflow-x-auto rounded bg-muted p-3 text-xs">{snippet}</pre>
        <p className="mt-2 text-xs text-muted-foreground">
          No cookies or IP addresses are stored. Visitors are counted with a hash that rotates
          daily.
        </p>
      </CardContent>
    </Card>
  );
}
