import { useState } from "react";
import { createFileRoute, Link } from "@tanstack/react-router";
import { api, useApi } from "../api";
import { Button, Card, ErrorText, Input } from "../components";
import { StatsDashboard, type SiteStats } from "../dashboard";

type Site = { id: number; hostname: string; honor_dnt: number };

export const Route = createFileRoute("/")({ component: Overview });

function Overview() {
  const { session } = Route.useRouteContext();
  const { data: sites, error: sitesError, reload } = useApi<Site[]>("/sites");
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
      <div className="flex flex-wrap gap-2 text-sm">
        <label className="flex items-center gap-2 rounded border border-slate-300 bg-white px-3 py-1.5">
          <input
            type="checkbox"
            checked={selected.size === 0}
            onChange={() => setSelected(new Set())}
          />
          All sites
        </label>
        {sites?.map((s) => (
          <label
            key={s.id}
            className="flex items-center gap-2 rounded border border-slate-300 bg-white px-3 py-1.5"
          >
            <input type="checkbox" checked={selected.has(s.id)} onChange={() => toggle(s.id)} />
            {s.hostname}
          </label>
        ))}
      </div>

      <ErrorText>{statsError}</ErrorText>
      {stats ? (
        <StatsDashboard stats={stats} />
      ) : (
        !statsError && <p className="text-slate-500">Loading…</p>
      )}

      <Card title="Sites">
        <ul className="divide-y divide-slate-200">
          {sites?.length === 0 && (
            <li className="py-2 text-slate-500">
              No sites yet. Add one below, then embed the snippet from its page.
            </li>
          )}
          {sites?.map((s) => (
            <li key={s.id} className="py-2">
              <Link
                to="/sites/$siteId"
                params={{ siteId: String(s.id) }}
                className="font-medium hover:underline"
              >
                {s.hostname}
              </Link>
            </li>
          ))}
        </ul>
        {session.user?.is_admin === 1 && <AddSite onAdded={reload} />}
      </Card>
    </div>
  );
}

function AddSite({ onAdded }: { onAdded: () => void }) {
  const [hostname, setHostname] = useState("");
  const [formError, setFormError] = useState<string>();
  return (
    <form
      className="mt-4 flex gap-2"
      onSubmit={async (e) => {
        e.preventDefault();
        setFormError(undefined);
        try {
          await api("/sites", { body: { hostname } });
          setHostname("");
          onAdded();
        } catch (err) {
          setFormError(err instanceof Error ? err.message : "Failed");
        }
      }}
    >
      <Input
        className="flex-1"
        placeholder="blog.example.com"
        required
        value={hostname}
        onChange={(e) => setHostname(e.target.value)}
      />
      <Button type="submit">Add</Button>
      <ErrorText>{formError}</ErrorText>
    </form>
  );
}
