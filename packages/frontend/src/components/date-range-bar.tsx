import { useEffect, useState } from "react";
import { DownloadIcon } from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import type { SiteStats } from "potoo-sdk";
import { FACETS, type FacetFilters } from "../lib/filter";
import { facetLabel } from "../lib/labels";
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
 * that opens the absolute from and to times down to the minute, the
 * active filters as removable chips, and the CSV download of the
 * selected view.
 *
 * The range and the filters live in the page's URL; the bar only reads and
 * writes them. The custom times use the browser's native datetime picker,
 * which always displays in the browser's locale — the configured formats
 * apply to the app's own text: the summary line and the chart.
 */
export function DateRangeBar({
  range,
  onRange,
  stats = null,
  dateFormat,
  use24,
  filters,
  onFilters,
  exportUrl,
}: {
  range: DateRange;
  onRange: (range: DateRange) => void;
  stats?: SiteStats | null;
  dateFormat?: string;
  use24?: boolean;
  filters: FacetFilters;
  onFilters: (filters: FacetFilters) => void;
  /** Where the view downloads as CSV; none while the range is invalid. */
  exportUrl?: string;
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

  const removeFilter = (facet: string, value: string) => {
    const next = new Map(filters);
    const values = new Set(next.get(facet) ?? []);
    values.delete(value);
    if (values.size) next.set(facet, values);
    else next.delete(facet);
    onFilters(next);
  };

  const count = [...filters.values()].reduce((n, values) => n + values.size, 0);

  return (
    <div className="rounded-lg border border-border bg-background p-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-1" role="group" aria-label="Date range">
          {RANGE_PRESETS.map((preset) => (
            <Button
              key={preset.label}
              size="sm"
              variant={!open && range.preset === preset.value ? "default" : "outline"}
              onClick={() => {
                // Explicit: the URL effect alone would miss the case where
                // this preset is already the one in the URL.
                setOpen(false);
                setDraft(null);
                onRange({ preset: preset.value });
              }}
            >
              {preset.label}
            </Button>
          ))}
          <Button
            size="sm"
            variant={open ? "default" : "outline"}
            onClick={() => {
              setOpen(true);
              setDraft(null);
            }}
          >
            Custom&hellip;
          </Button>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="icon-sm"
            nativeButton={false}
            render={
              <a href={exportUrl} download aria-label="Export as CSV" title="Export as CSV" />
            }
          >
            <DownloadIcon aria-hidden />
          </Button>
        </div>
      </div>

      {open && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Input
            type="datetime-local"
            aria-label="Range start"
            className="min-w-0 flex-1 sm:w-auto sm:flex-none"
            value={shown.from}
            onChange={(e) => setDraft({ from: e.target.value, to: shown.to })}
          />
          <span aria-hidden="true" className="text-faint">
            &ndash;
          </span>
          <Input
            type="datetime-local"
            aria-label="Range end"
            className="min-w-0 flex-1 sm:w-auto sm:flex-none"
            value={shown.to}
            onChange={(e) => setDraft({ from: shown.from, to: e.target.value })}
          />
          <Button disabled={!canApply} onClick={apply}>
            Apply
          </Button>
        </div>
      )}

      <div className="mt-3 flex flex-wrap items-center gap-1.5 text-sm">
        {[...filters].flatMap(([facet, values]) =>
          [...values].map((value) => (
            <Badge key={`${facet}:${value}`}>
              {FACETS[facet]}: {facetLabel(facet, value)}{" "}
              <button
                aria-label={`Remove the ${FACETS[facet]} ${facetLabel(facet, value)} filter`}
                className="text-primary-foreground/70 hover:text-primary-foreground"
                onClick={() => removeFilter(facet, value)}
              >
                &times;
              </button>
            </Badge>
          )),
        )}
        {count === 0 ? (
          <span className="text-xs text-muted-foreground">
            Click any item of a breakdown to filter by it.
          </span>
        ) : (
          <Button
            variant="link"
            size="xs"
            className="text-muted-foreground"
            onClick={() => onFilters(new Map())}
          >
            Clear all
          </Button>
        )}
      </div>

      <p className="mt-2 text-xs text-muted-foreground">
        {rangeSummary(range, dateFormat, use24)}
        {stats &&
          ` \u00b7 ${stats.pageviews.toLocaleString()} page views \u00b7 ${stats.visitors.toLocaleString()} visitors`}
      </p>
    </div>
  );
}
