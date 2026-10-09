import { useCallback, useState } from "react";
import { formatDate } from "./dates";

/** The preset ranges, the same ones the backend's `days` param accepts. */
export type RangePreset = "today" | 1 | 7 | 30 | 90 | 365;

/**
 * The range a dashboard shows: a preset, or an exact custom window as
 * `datetime-local` values, in the browser's timezone, with minute precision.
 */
export type DateRange = { preset: RangePreset } | { preset: "custom"; from: string; to: string };

export const RANGE_PRESETS: { value: RangePreset; label: string }[] = [
  { value: "today", label: "Today" },
  { value: 1, label: "24h" },
  { value: 7, label: "7d" },
  { value: 30, label: "30d" },
  { value: 90, label: "90d" },
  { value: 365, label: "12mo" },
];

const DAY = 86_400_000;
const HOUR = 3_600_000;

/** A local `datetime-local` value as UTC with minute precision, or null. */
function utcMinute(value: string): string | null {
  const time = Date.parse(value);
  return Number.isNaN(time) ? null : new Date(time).toISOString().slice(0, 16);
}

/** The stats query params of a range, or null when a custom range is invalid. */
export function rangeQuery(range: DateRange): string | null {
  if (range.preset !== "custom") return `days=${range.preset}`;
  const from = utcMinute(range.from);
  const to = utcMinute(range.to);
  return from && to && from < to ? `from=${from}&to=${to}` : null;
}

/**
 * The exact window a range covers. Presets mirror the backend's calendar
 * alignment (UTC days, weeks from Monday) and end now; custom is exactly
 * what its inputs say.
 */
export function rangeWindow(range: DateRange): { from: Date; to: Date } | null {
  if (range.preset === "custom") {
    const from = new Date(range.from);
    const to = new Date(range.to);
    return Number.isNaN(+from) || Number.isNaN(+to) || from >= to ? null : { from, to };
  }
  const now = new Date();
  const midnight = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
  switch (range.preset) {
    case "today":
      return { from: new Date(midnight), to: now };
    case 1:
      return {
        from: new Date(Math.floor(+now / HOUR) * HOUR - 23 * HOUR),
        to: now,
      };
    case 7:
      return { from: new Date(midnight - 6 * DAY), to: now };
    case 30:
      return { from: new Date(midnight - 29 * DAY), to: now };
    case 90: {
      const monday = midnight - ((now.getUTCDay() + 6) % 7) * DAY;
      return { from: new Date(monday - 12 * 7 * DAY), to: now };
    }
    default:
      return {
        from: new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - 11, 1)),
        to: now,
      };
  }
}

/** A date as a `datetime-local` value, in the browser's timezone. */
export function localInput(date: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${p(date.getMonth() + 1)}-${p(date.getDate())}T${p(date.getHours())}:${p(date.getMinutes())}`;
}

/** The range as one line of local text, e.g. "9 Oct 2026 14:30 – 16:00". */
export function rangeSummary(range: DateRange, format = "YYYY-MM-DD"): string {
  const window = rangeWindow(range);
  if (!window) return "Invalid range: the start must be before the end.";
  const p = (n: number) => String(n).padStart(2, "0");
  const day = (d: Date) => `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  const time = (d: Date) => `${p(d.getHours())}:${p(d.getMinutes())}`;
  return `${formatDate(
    day(window.from),
    format,
  )} ${time(window.from)} – ${formatDate(day(window.to), format)} ${time(window.to)}`;
}

/**
 * Range state for a dashboard. The query is the last valid one, so typing a
 * custom range keeps the previous stats until it becomes valid again.
 */
export function useDateRange() {
  const [range, setRange] = useState<DateRange>({ preset: 7 });
  const [query, setQuery] = useState("days=7");
  const update = useCallback((next: DateRange) => {
    setRange(next);
    const q = rangeQuery(next);
    if (q) setQuery(q);
  }, []);
  return { range, setRange: update, query };
}
