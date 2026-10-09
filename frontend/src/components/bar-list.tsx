/** Rows of label + count with a proportional bar. */
export function BarList({
  rows,
  empty = "No data yet.",
}: {
  rows: { label: string; value: number }[];
  empty?: string;
}) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  if (!rows.length) return <p className="text-sm text-slate-500">{empty}</p>;
  return (
    <ul className="space-y-1">
      {rows.map((r) => (
        <li key={r.label} className="relative flex justify-between rounded px-2 py-1 text-sm">
          <span
            className="absolute inset-y-0 left-0 rounded bg-slate-100"
            style={{ width: `${(r.value / max) * 100}%` }}
          />
          <span className="relative truncate">{r.label}</span>
          <span className="relative tabular-nums text-slate-600">{r.value}</span>
        </li>
      ))}
    </ul>
  );
}
