import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { DateRangeBar } from "../components/date-range-bar";
import { ErrorText } from "../components/error-text";
import { SiteSelect } from "../components/site-select";
import { StatsDashboard } from "../components/stats-dashboard";
import { useApi, type SiteStats } from "../lib/api";
import { buildFilter, useDashboardFilters } from "../lib/filter";
import { parseSites, useDateRange, validateDashboardSearch } from "../lib/date-range";

type Site = { id: number; hostname: string; honor_dnt: number };

export const Route = createFileRoute("/")({
  validateSearch: validateDashboardSearch,
  component: Overview,
});

function Overview() {
  const { session } = Route.useRouteContext();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const { data: sites, error: sitesError } = useApi<Site[]>("/sites");
  const { range, setRange, query } = useDateRange();
  const { filters, toggle, setFilters } = useDashboardFilters();

  // The selected site ids; empty means every site. In the URL, so a view
  // can be shared.
  const selected = parseSites(search.sites);
  const toggleSite = (id: number) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
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
  const params = `${query}${included ? `&sites=${included}` : ""}${
    rsql ? `&q=${encodeURIComponent(rsql)}` : ""
  }`;
  const statsPath = `/stats?${params}`;
  const { data: stats, error: statsError } = useApi<SiteStats>(statsPath);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Overview</h1>
      <ErrorText>{sitesError}</ErrorText>
      <SiteSelect sites={sites} selected={selected} onToggle={toggleSite} onClear={clear} />

      <DateRangeBar
        range={range}
        onRange={setRange}
        stats={stats}
        dateFormat={session.user?.date_format}
        timeFormat={session.user?.time_format}
        filters={filters}
        onFilters={setFilters}
        exportUrl={`/api/stats.csv?${params}`}
      />
      <ErrorText>{statsError}</ErrorText>
      {stats ? (
        <StatsDashboard
          stats={stats}
          dateFormat={session.user?.date_format}
          timeFormat={session.user?.time_format}
          filters={filters}
          onToggleFilter={toggle}
        />
      ) : (
        !statsError && <p className="text-slate-500">Loading…</p>
      )}
    </div>
  );
}
