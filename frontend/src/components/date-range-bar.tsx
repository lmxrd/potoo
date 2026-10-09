import { IconDownload } from "@tabler/icons-react";
import { Button } from "./button";
import type { SiteStats } from "../lib/api";
import { download, statsCsv } from "../lib/csv";
import {
  RANGE_PRESETS,
  localInput,
  rangeSummary,
  rangeWindow,
  type DateRange,
} from "../lib/date-range";

/**
 * The toolbar above the dashboard: preset ranges, exact from and to times
 * down to the minute, and the CSV export of the selected view.
 *
 * The custom times use the browser's native datetime picker: it is the most
 * robust input there is, but it always displays and opens in the browser's
 * locale, so the configured formats apply to the app's own text (the
 * summary line, the chart), not to the picker.
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
  const from = range.preset === "custom" ? range.from : (window && localInput(window.from)) || "";
  const to = range.preset === "custom" ? range.to : (window && localInput(window.to)) || "";

  const exportCsv = () => {
    if (!stats) return;
    download("potoo-stats.csv", statsCsv(stats, rangeSummary(range, dateFormat, timeFormat)));
  };

  return (
    <div className="rounded-lg border border-slate-200 bg-slate-50 p-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-1" role="group" aria-label="Date range">
          {RANGE_PRESETS.map((preset) => (
            <button
              key={preset.label}
              className={`rounded px-2.5 py-1.5 text-sm ${
                range.preset === preset.value
                  ? "bg-slate-900 font-medium text-white"
                  : "border border-slate-300 bg-white hover:bg-slate-100"
              }`}
              onClick={() => onRange({ preset: preset.value })}
            >
              {preset.label}
            </button>
          ))}
        </div>
        <div className="flex w-full items-center gap-2 sm:w-auto">
          <input
            type="datetime-local"
            aria-label="Range start"
            className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-1.5 sm:flex-none"
            value={from}
            onChange={(e) => onRange({ preset: "custom", from: e.target.value, to })}
          />
          <span aria-hidden="true" className="text-slate-400">
            &ndash;
          </span>
          <input
            type="datetime-local"
            aria-label="Range end"
            className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-1.5 sm:flex-none"
            value={to}
            onChange={(e) => onRange({ preset: "custom", from, to: e.target.value })}
          />
        </div>
        <Button className="flex items-center gap-1.5" disabled={!stats} onClick={exportCsv}>
          <IconDownload size={16} stroke={2} aria-hidden />
          Export
        </Button>
      </div>
      <p className="mt-2 text-xs text-slate-500">
        {rangeSummary(range, dateFormat, timeFormat)}
        {stats &&
          ` \u00b7 ${stats.pageviews.toLocaleString()} page views \u00b7 ${stats.visitors.toLocaleString()} visitors`}
      </p>
    </div>
  );
}
