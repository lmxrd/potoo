import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { useApi } from "../api";
import { ErrorText } from "../components";
import { StatsDashboard, type SiteStats } from "../dashboard";
import { SiteSelect } from "../site-select";

type Site = { id: number; hostname: string; honor_dnt: number };

export const Route = createFileRoute("/")({ component: Overview });

function Overview() {
  const { data: sites, error: sitesError } = useApi<Site[]>("/sites");
  const [days, setDays] = useState(7);
  // The selected site ids; empty means every site.
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const included = [...selected].join(",");
  const statsPath = `/stats?days=${days}${included ? `&sites=${included}` : ""}`;
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
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-semibold">Overview</h1>
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

      <ErrorText>{sitesError}</ErrorText>
      <SiteSelect
        sites={sites}
        selected={selected}
        onToggle={toggle}
        onClear={() => setSelected(new Set())}
      />

      <ErrorText>{statsError}</ErrorText>
      {stats ? (
        <StatsDashboard stats={stats} />
      ) : (
        !statsError && <p className="text-slate-500">Loading…</p>
      )}
    </div>
  );
}
