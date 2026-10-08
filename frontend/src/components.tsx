import type { ComponentProps, ReactNode } from "react";

export const Card = ({ title, children }: { title?: string; children: ReactNode }) => (
  <section className="rounded-lg border border-slate-200 bg-white p-4">
    {title && <h2 className="mb-3 text-sm font-medium text-slate-500">{title}</h2>}
    {children}
  </section>
);

export const Input = (props: ComponentProps<"input">) => (
  <input
    {...props}
    className={`rounded border border-slate-300 px-3 py-2 text-sm ${props.className ?? ""}`}
  />
);

export const Button = ({
  variant,
  ...props
}: ComponentProps<"button"> & { variant?: "danger" }) => (
  <button
    {...props}
    className={`rounded px-3 py-2 text-sm font-medium text-white disabled:opacity-50 ${
      variant === "danger" ? "bg-red-600 hover:bg-red-700" : "bg-slate-900 hover:bg-slate-700"
    }`}
  />
);

export const ErrorText = ({ children }: { children?: ReactNode }) =>
  children ? <p className="text-sm text-red-600">{children}</p> : null;

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
