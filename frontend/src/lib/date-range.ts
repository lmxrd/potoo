import { useCallback } from "react";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { parseFilter } from "./filter";
import { formatDate, formatTime } from "./dates";

/**
 * The preset ranges, as HTML duration strings: PT24H is the rolling last
 * 24 hours, P7D and friends the last N days, P1Y a year. `today` is a named
 * calendar range instead: since UTC midnight, not a rolling duration.
 */
export type RangePreset = "today" | "PT24H" | "P7D" | "P30D" | "P90D" | "P1Y";

/**
 * The range a dashboard shows: a preset, or an exact custom window as
 * `datetime-local` values, in the browser's timezone, with minute precision.
 */
export type DateRange = { preset: RangePreset } | { preset: "custom"; from: string; to: string };

export const RANGE_PRESETS: { value: RangePreset; label: string }[] = [
  { value: "today", label: "Today" },
  { value: "PT24H", label: "24h" },
  { value: "P7D", label: "7d" },
  { value: "P30D", label: "30d" },
  { value: "P90D", label: "90d" },
  { value: "P1Y", label: "12mo" },
];

const DAY = 86_400_000;
const HOUR = 3_600_000;
const MINUTE_ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/;

/** A local `datetime-local` value as UTC with minute precision, or null. */
function utcMinute(value: string): string | null {
  const time = Date.parse(value);
  return Number.isNaN(time) ? null : new Date(time).toISOString().slice(0, 16);
}

/** A UTC minute value as a local `datetime-local` value, or null. */
function localMinute(utc: string): string | null {
  const time = Date.parse(`${utc}:00Z`);
  return Number.isNaN(time) ? null : localInput(new Date(time));
}

/**
 * The query params of a range — the same ones the stats and the export
 * endpoints take — or null when a custom range is invalid. Presets go as
 * the duration they are (`range=P7D`); Today is a label the dashboard
 * expands into the UTC-midnight-to-now window; custom ranges are ISO
 * timestamps in UTC, minute precision.
 */
export function rangeQuery(range: DateRange): string | null {
  if (range.preset === "today") {
    const now = new Date();
    const midnight = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
    const from = midnight.toISOString().slice(0, 16);
    // Not before midnight in practice, but the backend requires from < to.
    const to = (midnight < now ? now : new Date(+midnight + 60_000)).toISOString().slice(0, 16);
    return `from=${from}&to=${to}`;
  }
  if (range.preset !== "custom") {
    return `range=${range.preset}`;
  }
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
    case "PT24H":
      return {
        from: new Date(Math.floor(+now / HOUR) * HOUR - 23 * HOUR),
        to: now,
      };
    case "P7D":
      return { from: new Date(midnight - 6 * DAY), to: now };
    case "P30D":
      return { from: new Date(midnight - 29 * DAY), to: now };
    case "P90D": {
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
export function rangeSummary(range: DateRange, format = "YYYY-MM-DD", timeFormat = "24h"): string {
  const window = rangeWindow(range);
  if (!window) return "Invalid range: the start must be before the end.";
  const p = (n: number) => String(n).padStart(2, "0");
  const day = (d: Date) => `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  const time = (d: Date) => formatTime(`${p(d.getHours())}:${p(d.getMinutes())}`, timeFormat);
  return `${formatDate(
    day(window.from),
    format,
  )} ${time(window.from)} – ${formatDate(day(window.to), format)} ${time(window.to)}`;
}

/**
 * The dashboard settings kept in the URL, so a view can be shared and the
 * back button steps through changes: `?range=P7D` or `?from=..&to=..` for
 * the range (from/to in UTC, so a shared link is the same absolute window
 * in any timezone), `?sites=1,3` for the overview's site selection.
 */
export type DashboardSearch = {
  range?: string;
  from?: string;
  to?: string;
  sites?: string;
  q?: string;
};

/** The routes' validateSearch: keep only well-formed dashboard params. */
export function validateDashboardSearch(search: Record<string, unknown>): DashboardSearch {
  const out: DashboardSearch = {};
  if (
    typeof search.range === "string" &&
    RANGE_PRESETS.some((preset) => preset.value === search.range)
  ) {
    out.range = search.range;
  }
  if (typeof search.from === "string" && MINUTE_ISO.test(search.from)) {
    out.from = search.from;
  }
  if (typeof search.to === "string" && MINUTE_ISO.test(search.to)) {
    out.to = search.to;
  }
  if (typeof search.sites === "string" && /^\d+(,\d+)*$/.test(search.sites)) {
    out.sites = search.sites;
  }
  // Only expressions the dashboard can express survive: unknown facets,
  // other operators, or OR across facets degrade to the unfiltered view.
  if (typeof search.q === "string" && parseFilter(search.q)) {
    out.q = search.q;
  }
  return out;
}

/** The range the URL encodes: custom as UTC from/to, presets as durations. */
export function searchRange(search: DashboardSearch): DateRange {
  const from = search.from && localMinute(search.from);
  const to = search.to && localMinute(search.to);
  if (from && to && from < to) return { preset: "custom", from, to };
  const preset = RANGE_PRESETS.find((option) => option.value === search.range)?.value;
  if (preset !== undefined) return { preset };
  return { preset: "P7D" };
}

/** The selected site ids of `?sites=1,3`; an empty set means every site. */
export function parseSites(sites: string | undefined): Set<number> {
  return new Set((sites ?? "").split(",").filter(Boolean).map(Number));
}

/** The search params a range writes; presets as durations, custom as UTC. */
function rangeParams(range: DateRange): DashboardSearch {
  if (range.preset !== "custom") return { range: range.preset };
  const from = utcMinute(range.from);
  const to = utcMinute(range.to);
  return from && to && from < to ? { from, to } : {};
}

/**
 * The dashboard range, read from and written to the page's URL. Works on
 * any route that validates its search with {@linkcode
 * validateDashboardSearch}. Every change is a history entry, so the back
 * button steps through ranges.
 */
export function useDateRange() {
  const search = useSearch({ strict: false }) as DashboardSearch;
  const navigate = useNavigate();
  const range = searchRange(search);
  const setRange = useCallback(
    (next: DateRange) => {
      const params = rangeParams(next);
      navigate({
        to: ".",
        search: (prev) => {
          const merged: DashboardSearch = { ...(prev as DashboardSearch), ...params };
          // Presets and custom ranges replace each other, never stack. The
          // discriminator is what the new range carries: merged alone can
          // still hold the stale `range` of a preset it is replacing.
          if (params.range) {
            delete merged.from;
            delete merged.to;
          } else {
            delete merged.range;
          }
          return merged;
        },
      });
    },
    [navigate],
  );
  return { range, setRange, query: rangeQuery(range) };
}
