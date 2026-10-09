import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { useApi } from "../api";
import { Button, Card, ErrorText, Input } from "../components";
import { SiteSelect } from "../site-select";

type Site = { id: number; hostname: string };

export const Route = createFileRoute("/export")({ component: ExportPage });

function ExportPage() {
  const { data: sites, error } = useApi<Site[]>("/sites");
  // The selected site ids; empty means every site.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");

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
