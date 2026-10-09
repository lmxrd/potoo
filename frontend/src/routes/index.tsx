import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { DateRangeBar } from "../components/date-range-bar";
import { ErrorText } from "../components/error-text";
import { SiteSelect } from "../components/site-select";
import { StatsDashboard } from "../components/stats-dashboard";
import { useApi, type SiteStats } from "../lib/api";
import { useDateRange } from "../lib/date-range";

type Site = { id: number; hostname: string; honor_dnt: number };

export const Route = createFileRoute("/")({ component: Overview });

function Overview() {
  const { session } = Route.useRouteContext();
  const { data: sites, error: sitesError } = useApi<Site[]>("/sites");
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const { range, setRange, query } = useDateRange();

  // The selected site ids; empty means every site.
  const included = [...selected].join(",");
  const statsPath = `/stats?${query}${included ? `&sites=${included}` : ""}`;
  const { data: stats, error: statsError } = useApi<SiteStats>(statsPath);

  const toggle = (id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Overview</h1>
      <ErrorText>{sitesError}</ErrorText>
      <SiteSelect
        sites={sites}
        selected={selected}
        onToggle={toggle}
        onClear={() => setSelected(new Set())}
      />

      <DateRangeBar
        range={range}
        onRange={setRange}
        stats={stats}
        dateFormat={session.user?.date_format}
      />
      <ErrorText>{statsError}</ErrorText>
      {stats ? (
        <StatsDashboard stats={stats} dateFormat={session.user?.date_format} />
      ) : (
        !statsError && <p className="text-slate-500">Loading…</p>
      )}
    </div>
  );
}
