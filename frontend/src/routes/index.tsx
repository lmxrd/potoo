import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { getStatsOptions, listSitesOptions } from "../client/@tanstack/react-query.gen";
import { DateRangeBar } from "../components/date-range-bar";
import { ErrorText } from "../components/error-text";
import { SiteSelect } from "../components/site-select";
import { StatsDashboard } from "../components/stats-dashboard";
import { errorMessage } from "../lib/api";
import { exportHref, statsQuery } from "../lib/stats-query";
import { buildFilter, useDashboardFilters } from "../lib/filter";
import { parseSites, useDateRange, validateDashboardSearch } from "../lib/date-range";

export const Route = createFileRoute("/")({
  validateSearch: validateDashboardSearch,
  component: Overview,
});

function Overview() {
  const { session } = Route.useRouteContext();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const { data: sites, error: sitesError } = useQuery(listSitesOptions());
  const { range, setRange, query } = useDateRange();
  const { filters, toggle, setFilters } = useDashboardFilters();

  // The selected hostnames; empty means every site. In the URL, so a view
  // can be shared.
  const selected = parseSites(search.sites);
  const toggleSite = (hostname: string) => {
    const next = new Set(selected);
    if (next.has(hostname)) next.delete(hostname);
    else next.add(hostname);
    navigate({
      to: ".",
      search: (prev) => ({
        ...prev,
        sites: next.size ? [...next].join(",") : undefined,
      }),
    });
  };
  const clear = () => navigate({ to: ".", search: (prev) => ({ ...prev, sites: undefined }) });

  const included = [...selected].join(",");
  const rsql = buildFilter(filters);
  const params = statsQuery(query, included, rsql);
  // The previous numbers stay on screen while the next range loads.
  const { data: stats, error: statsError } = useQuery({
    // The query never runs while the range is invalid (`enabled`), so the
    // empty one it is built from is never sent.
    ...getStatsOptions({ query: params ?? { from: "", to: "" } }),
    enabled: params !== null,
    placeholderData: keepPreviousData,
  });

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Overview</h1>
      <ErrorText>{sitesError && errorMessage(sitesError)}</ErrorText>
      <SiteSelect sites={sites} selected={selected} onToggle={toggleSite} onClear={clear} />

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
        !statsError && <p className="text-muted">Loading…</p>
      )}
    </div>
  );
}
