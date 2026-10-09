/** The date formats the backend accepts, with a label for the dropdown. */
export const DATE_FORMATS = [
  { value: "YYYY-MM-DD", label: "YYYY-MM-DD (2026-10-09)" },
  { value: "DD/MM/YYYY", label: "DD/MM/YYYY (09/10/2026)" },
  { value: "MM/DD/YYYY", label: "MM/DD/YYYY (10/09/2026)" },
  { value: "DD.MM.YYYY", label: "DD.MM.YYYY (09.10.2026)" },
  { value: "D MMM YYYY", label: "D MMM YYYY (9 Oct 2026)" },
];

/**
 * The short axis label of one chart bucket, for the range it belongs to:
 * the hour for hourly buckets, the month for a year, and "9 Oct" style
 * days otherwise.
 */
export function chartLabel(day: string, range: number | "today"): string {
  if (range === "today" || range === 1) return day.slice(11, 16);
  if (typeof range === "number" && range > 90) {
    const [, month, year] = day.split("-");
    return Number(month) === 1 ? `${MONTHS[0]} ${year.slice(2)}` : MONTHS[Number(month) - 1];
  }
  const [, month, date] = day.split("-");
  return `${Number(date)} ${MONTHS[Number(month) - 1]}`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/**
 * Show a date (a `YYYY-MM-DD` day, or a full timestamp) in one of the formats
 * of {@linkcode DATE_FORMATS}. Unknown formats fall back to YYYY-MM-DD.
 */
export function formatDate(value: string, format: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(value);
  if (!m) return value;
  const [, year, month, day] = m;
  switch (format) {
    case "DD/MM/YYYY":
      return `${day}/${month}/${year}`;
    case "MM/DD/YYYY":
      return `${month}/${day}/${year}`;
    case "DD.MM.YYYY":
      return `${day}.${month}.${year}`;
    case "D MMM YYYY":
      return `${Number(day)} ${MONTHS[Number(month) - 1]} ${year}`;
    default:
      return `${year}-${month}-${day}`;
  }
}
