import { useEffect, useState } from "react";
import { createFileRoute } from "@tanstack/react-router";

type Stats = {
  total: number;
  byName: { name: string; count: number }[];
  byDay: { day: string; count: number }[];
};

export const Route = createFileRoute("/")({ component: Dashboard });

function Dashboard() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch("/api/stats?days=7")
      .then((r) => (r.ok ? (r.json() as Promise<Stats>) : Promise.reject(new Error(r.statusText))))
      .then(setStats)
      .catch((e: Error) => setError(e.message));
  }, []);

  if (error) return <p className="text-red-600">Failed to load stats: {error}</p>;
  if (!stats) return <p className="text-slate-500">Loading…</p>;

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-slate-200 bg-white p-4">
        <div className="text-sm text-slate-500">Events, last 7 days</div>
        <div className="text-3xl font-semibold">{stats.total}</div>
      </div>
      <ul className="divide-y divide-slate-200 rounded-lg border border-slate-200 bg-white">
        {stats.byName.length === 0 && <li className="p-4 text-slate-500">No events yet.</li>}
        {stats.byName.map((e) => (
          <li key={e.name} className="flex justify-between p-4">
            <span>{e.name}</span>
            <span className="tabular-nums text-slate-600">{e.count}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
