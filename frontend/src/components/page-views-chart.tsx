import { AxisBottom } from "@visx/axis";
import { Group } from "@visx/group";
import { ParentSize } from "@visx/responsive";
import { scaleBand, scaleLinear } from "@visx/scale";
import { Bar } from "@visx/shape";
import { TooltipWithBounds, useTooltip } from "@visx/tooltip";
import type { DayRow } from "../client";

const HEIGHT = 140;
const AXIS = 34;
const MARGIN = { top: 6, left: 14, right: 14 };
/** The room one axis label needs; the labels shown are as many as fit. */
const LABEL_WIDTH = 60;

/**
 * The page views per period as bars, in the page's theme: the bars and the
 * text take their colors from the same CSS variables as everything else
 * (`fill-accent`, `fill-faint`), so a theme or accent change shows at once.
 * The bars share the width equally, and the axis shows only as many labels
 * as fit, whatever the width and the number of bars.
 *
 * Hover, or tap, a bar for its numbers. `axisLabel` is the short text under
 * a bar, `describe` its full date for the tooltip.
 */
export function PageViewsChart({
  rows,
  axisLabel,
  describe,
}: {
  rows: DayRow[];
  axisLabel: (period: string) => string;
  describe: (period: string) => string;
}) {
  return (
    <>
      <ParentSize parentSizeStyles={{ width: "100%", height: HEIGHT }} debounceTime={50}>
        {({ width }) =>
          width > 0 ? (
            <Chart rows={rows} width={width} axisLabel={axisLabel} describe={describe} />
          ) : null
        }
      </ParentSize>
      {/* The chart is a picture; this is its text. */}
      <ul className="sr-only">
        {rows.map((row) => (
          <li key={row.day}>
            {describe(row.day)}: {row.pageviews} page views, {row.visitors} visitors
          </li>
        ))}
      </ul>
    </>
  );
}

function Chart({
  rows,
  width,
  axisLabel,
  describe,
}: {
  rows: DayRow[];
  width: number;
  axisLabel: (period: string) => string;
  describe: (period: string) => string;
}) {
  const { tooltipData, tooltipLeft, tooltipTop, tooltipOpen, showTooltip, hideTooltip } =
    useTooltip<DayRow>();

  const innerWidth = Math.max(0, width - MARGIN.left - MARGIN.right);
  const innerHeight = HEIGHT - AXIS - MARGIN.top;
  const x = scaleBand<string>({
    domain: rows.map((row) => row.day),
    range: [0, innerWidth],
    padding: rows.length > 40 ? 0.1 : 0.2,
  });
  const y = scaleLinear<number>({
    domain: [0, Math.max(1, ...rows.map((row) => row.pageviews))],
    range: [innerHeight, 0],
  });

  // Every n-th label, so that the ones shown have room.
  const fit = Math.max(2, Math.floor(innerWidth / LABEL_WIDTH));
  const every = Math.max(1, Math.ceil(rows.length / fit));
  const ticks = rows.filter((_, i) => i % every === 0).map((row) => row.day);

  const show = (row: DayRow) =>
    showTooltip({
      tooltipData: row,
      tooltipLeft: MARGIN.left + (x(row.day) ?? 0) + x.bandwidth() / 2,
      tooltipTop: MARGIN.top + y(row.pageviews),
    });
  // A mouse leaving a bar closes its tooltip; a finger lifting must not,
  // or there would be nothing to read: it stays until another tap.
  const leave = (e: React.PointerEvent) => {
    if (e.pointerType !== "touch") hideTooltip();
  };

  return (
    <div className="relative">
      <svg
        width={width}
        height={HEIGHT}
        aria-hidden="true"
        onPointerDown={(e) => {
          if (e.target === e.currentTarget) hideTooltip();
        }}
      >
        <Group left={MARGIN.left} top={MARGIN.top}>
          {rows.map((row) => {
            const dimmed = tooltipOpen && tooltipData?.day !== row.day;
            return (
              <Bar
                key={row.day}
                x={x(row.day)}
                y={y(row.pageviews)}
                width={x.bandwidth()}
                height={innerHeight - y(row.pageviews)}
                rx={Math.min(2, x.bandwidth() / 2)}
                className={`fill-accent${dimmed ? " opacity-40" : ""}`}
              />
            );
          })}
          <AxisBottom
            top={innerHeight}
            scale={x}
            tickValues={ticks}
            tickFormat={axisLabel}
            hideTicks
            axisLineClassName="stroke-line"
            tickLabelProps={{ className: "fill-faint text-[10px]", textAnchor: "middle", dy: 4 }}
          />
          {/* A bar and its gaps are one target, so a thin or empty bar is easy to hit. */}
          {rows.map((row) => (
            <rect
              key={row.day}
              x={(x(row.day) ?? 0) - (x.step() - x.bandwidth()) / 2}
              y={0}
              width={x.step()}
              height={innerHeight}
              fill="transparent"
              onPointerEnter={() => show(row)}
              onPointerMove={() => show(row)}
              onPointerLeave={leave}
            />
          ))}
        </Group>
      </svg>
      {tooltipOpen && tooltipData && (
        <TooltipWithBounds
          unstyled
          applyPositionStyle
          top={tooltipTop}
          left={tooltipLeft}
          className="pointer-events-none z-10 rounded border border-line bg-card px-2 py-1 text-xs text-ink shadow-md"
        >
          <div className="font-medium">{describe(tooltipData.day)}</div>
          <div className="tabular-nums text-soft-ink">
            {tooltipData.pageviews} page views · {tooltipData.visitors} visitors
          </div>
        </TooltipWithBounds>
      )}
    </div>
  );
}
