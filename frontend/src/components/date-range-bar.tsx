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
 */
export function DateRangeBar({
  range,
  onRange,
  stats = null,
  dateFormat,
}: {
  range: DateRange;
  onRange: (range: DateRange) => void;
  stats?: SiteStats | null;
  dateFormat?: string;
}) {
  const window = rangeWindow(range);
  const from = range.preset === "custom" ? range.from : (window && localInput(window.from)) || "";
  const to = range.preset === "custom" ? range.to : (window && localInput(window.to)) || "";

  const exportCsv = () => {
    if (!stats) return;
    download("potoo-stats.csv", statsCsv(stats, rangeSummary(range, dateFormat)));
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
        <div className="flex items-center gap-2 text-sm text-slate-500">
          <input
            type="datetime-local"
            aria-label="Range start"
            className="rounded border border-slate-300 px-2 py-1"
            value={from}
            onChange={(e) => onRange({ preset: "custom", from: e.target.value, to })}
          />
          <span aria-hidden="true">&ndash;</span>
          <input
            type="datetime-local"
            aria-label="Range end"
            className="rounded border border-slate-300 px-2 py-1"
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
        {rangeSummary(range, dateFormat)}
        {stats &&
          ` \u00b7 ${stats.pageviews.toLocaleString()} page views \u00b7 ${stats.visitors.toLocaleString()} visitors`}
      </p>
    </div>
  );
}
