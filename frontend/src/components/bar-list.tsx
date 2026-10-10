/**
 * Rows of label + count with a proportional bar. A row with `onToggle` is
 * a filter: clicking it toggles the row as active, shown in the bar color.
 * `format` only changes the displayed text; the label stays the value the
 * filter works on.
 */
export function BarList({
  rows,
  empty = "No data yet.",
  active,
  onToggle,
  format,
}: {
  rows: { label: string; value: number }[];
  empty?: string;
  active?: Set<string>;
  onToggle?: (label: string) => void;
  format?: (label: string) => string;
}) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  if (!rows.length) return <p className="text-sm text-slate-500">{empty}</p>;
  return (
    <ul className="space-y-1">
      {rows.map((r) => {
        const on = active?.has(r.label) === true;
        return (
          <li
            key={r.label}
            role={onToggle ? "button" : undefined}
            tabIndex={onToggle ? 0 : undefined}
            aria-pressed={onToggle ? on : undefined}
            title={onToggle ? "Toggle as a filter" : undefined}
            onClick={onToggle ? () => onToggle(r.label) : undefined}
            onKeyDown={
              onToggle
                ? (e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      onToggle(r.label);
                    }
                  }
                : undefined
            }
            className={`relative flex justify-between rounded px-2 py-1 text-sm${
              onToggle ? " cursor-pointer" : ""
            }${on ? " bg-slate-800 text-white" : ""}`}
          >
            <span
              className={`absolute inset-y-0 left-0 rounded ${on ? "hidden" : "bg-slate-100"}`}
              style={{ width: `${(r.value / max) * 100}%` }}
            />
            <span className="relative truncate">{format ? format(r.label) : r.label}</span>
            <span className={`relative tabular-nums ${on ? "text-slate-300" : "text-slate-600"}`}>
              {r.value}
            </span>
          </li>
        );
      })}
    </ul>
  );
}
