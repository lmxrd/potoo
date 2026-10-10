import { useEffect, useState } from "react";
import { IconDownload } from "@tabler/icons-react";
import { Button } from "./button";
import type { SiteStats } from "../lib/api";
import { download, statsCsv } from "../lib/csv";
import {
  RANGE_PRESETS,
  localInput,
  rangeQuery,
  rangeSummary,
  rangeWindow,
  type DateRange,
} from "../lib/date-range";

/**
 * The toolbar above the dashboard: relative preset chips, a Custom chip
 * that opens the absolute from and to times down to the minute, and the
 * CSV export of the selected view.
 *
 * The range lives in the page's URL; the bar only reads and writes it.
 * The custom times use the browser's native datetime picker, which always
 * displays in the browser's locale — the configured formats apply to the
 * app's own text: the summary line and the chart.
 */
export function DateRangeBar({
  range,
  onRange,
  stats = null,
  dateFormat,
  timeFormat,
}: {
  range: DateRange;
  onRange: (range: DateRange) => void;
  stats?: SiteStats | null;
  dateFormat?: string;
  timeFormat?: string;
}) {
  const window = rangeWindow(range);
  const applied = {
    from: range.preset === "custom" ? range.from : (window && localInput(window.from)) || "",
    to: range.preset === "custom" ? range.to : (window && localInput(window.to)) || "",
  };
  // The absolute row: open for custom ranges, and while one is being
  // edited. Its inputs hold their own text until Apply puts it in the URL.
  const [open, setOpen] = useState(range.preset === "custom");
  const [draft, setDraft] = useState<{ from: string; to: string } | null>(null);
  useEffect(() => {
    setOpen(range.preset === "custom");
    setDraft(null);
  }, [range.preset]);

  const shown = draft ?? applied;
  const canApply = rangeQuery({ preset: "custom", from: shown.from, to: shown.to }) !== null;

  const apply = () => {
    if (!canApply) return;
    // The draft stays: clearing it here would show the old range for the
    // moment before the navigation lands. The sync effect below clears it
    // once the URL holds the new range.
    onRange({ preset: "custom", from: shown.from, to: shown.to });
  };

  const exportCsv = () => {
    if (!stats) return;
    download("potoo-stats.csv", statsCsv(stats, rangeSummary(range, dateFormat, timeFormat)));
  };

  const chip = "rounded px-2.5 py-1.5 text-sm";

  return (
    <div className="rounded-lg border border-slate-200 bg-slate-50 p-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-1" role="group" aria-label="Date range">
          {RANGE_PRESETS.map((preset) => (
            <button
              key={preset.label}
              className={`${chip} ${
                !open && range.preset === preset.value
                  ? "bg-slate-900 font-medium text-white"
                  : "border border-slate-300 bg-white hover:bg-slate-100"
              }`}
              onClick={() => {
                // Explicit: the URL effect alone would miss the case where
                // this preset is already the one in the URL.
                setOpen(false);
                setDraft(null);
                onRange({ preset: preset.value });
              }}
            >
              {preset.label}
            </button>
          ))}
          <button
            className={`${chip} ${
              open
                ? "bg-slate-900 font-medium text-white"
                : "border border-slate-300 bg-white hover:bg-slate-100"
            }`}
            onClick={() => {
              setOpen(true);
              setDraft(null);
            }}
          >
            Custom&hellip;
          </button>
        </div>
        <Button className="flex items-center gap-1.5" disabled={!stats} onClick={exportCsv}>
          <IconDownload size={16} stroke={2} aria-hidden />
          Export
        </Button>
      </div>

      {open && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <input
            type="datetime-local"
            aria-label="Range start"
            className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-1.5 text-sm sm:flex-none"
            value={shown.from}
            onChange={(e) => setDraft({ from: e.target.value, to: shown.to })}
          />
          <span aria-hidden="true" className="text-slate-400">
            &ndash;
          </span>
          <input
            type="datetime-local"
            aria-label="Range end"
            className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-1.5 text-sm sm:flex-none"
            value={shown.to}
            onChange={(e) => setDraft({ from: shown.from, to: e.target.value })}
          />
          <Button disabled={!canApply} onClick={apply}>
            Apply
          </Button>
        </div>
      )}

      <p className="mt-2 text-xs text-slate-500">
        {rangeSummary(range, dateFormat, timeFormat)}
        {stats &&
          ` \u00b7 ${stats.pageviews.toLocaleString()} page views \u00b7 ${stats.visitors.toLocaleString()} visitors`}
      </p>
    </div>
  );
}
