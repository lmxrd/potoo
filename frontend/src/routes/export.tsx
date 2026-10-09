import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { Input } from "../components/input";
import { SiteSelect } from "../components/site-select";
import { useApi } from "../lib/api";

type Site = { id: number; hostname: string };

const day = (offset = 0) => new Date(Date.now() - offset * 86_400_000).toISOString().slice(0, 10);

export const Route = createFileRoute("/export")({ component: ExportPage });

function ExportPage() {
  const { data: sites, error } = useApi<Site[]>("/sites");
  // The selected site ids; empty means every site.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  // Defaults: the last week, up to and including today.
  const [from, setFrom] = useState(day(6));
  const [to, setTo] = useState(day());

  /** A range of the last `days` calendar days, ending today. */
  const range = (days: number) => {
    setFrom(day(days - 1));
    setTo(day());
  };

  const toggle = (id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const params = new URLSearchParams();
  const ids = [...selected].join(",");
  if (ids) params.set("sites", ids);
  if (from) params.set("from", from);
  if (to) params.set("to", to);
  const query = params.toString();

  return (
    <div className="max-w-xl space-y-6">
      <h1 className="text-xl font-semibold">Export</h1>
      <ErrorText>{error}</ErrorText>
      <Card>
        <div className="space-y-4">
          <SiteSelect
            sites={sites}
            selected={selected}
            onToggle={toggle}
            onClear={() => setSelected(new Set())}
          />
          <div className="flex flex-wrap gap-2">
            {[
              ["Today", 1],
              ["Past 7 days", 7],
              ["Past 30 days", 30],
            ].map(([label, days]) => (
              <Button
                key={label as string}
                className="!px-2 !py-1 !text-xs"
                onClick={() => range(days as number)}
              >
                {label}
              </Button>
            ))}
          </div>
          <div className="flex flex-wrap items-center gap-4 text-sm">
            <label className="flex items-center gap-2">
              <span className="text-slate-500">From</span>
              <Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
            </label>
            <label className="flex items-center gap-2">
              <span className="text-slate-500">To</span>
              <Input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
            </label>
          </div>
          <a href={`/api/export.csv${query ? `?${query}` : ""}`} download>
            <Button>Download CSV</Button>
          </a>
          <p className="text-xs text-slate-500">
            One row per recorded hit, oldest first: site, timestamp, kind, path, click label,
            referrer, country and device. No dates selected means everything. IP addresses and user
            agents are never stored, so they are never exported.
          </p>
        </div>
      </Card>
    </div>
  );
}
