/** The date formats the backend accepts, with a label for the dropdown. */
export const DATE_FORMATS = [
  { value: "YYYY-MM-DD", label: "YYYY-MM-DD (2026-10-09)" },
  { value: "DD/MM/YYYY", label: "DD/MM/YYYY (09/10/2026)" },
  { value: "MM/DD/YYYY", label: "MM/DD/YYYY (10/09/2026)" },
  { value: "DD.MM.YYYY", label: "DD.MM.YYYY (09.10.2026)" },
];

const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/**
 * A `HH:MM` time as configured: 24-hour by default, or 12-hour with AM/PM.
 */
export function formatTime(time: string, format = "24h"): string {
  if (format !== "12h") return time;
  const [hour, minute] = time.split(":").map(Number);
  const ampm = hour < 12 ? "AM" : "PM";
  const h12 = hour % 12 || 12;
  return `${h12}:${String(minute).padStart(2, "0")} ${ampm}`;
}

/**
 * The short axis label of one chart bucket, for the range it belongs to:
 * the hour for hourly buckets, the month for a year, and "9 Oct" style
 * days otherwise.
 */
export function chartLabel(
  day: string,
  range: number,
  timeFormat = "24h",
): string {
  if (range === 1) return formatTime(day.slice(11, 16), timeFormat);
  if (range > 90) {
    const [, month, year] = day.split("-");
    return Number(month) === 1
      ? `${MONTHS[0]} ${year.slice(2)}`
      : MONTHS[Number(month) - 1];
  }
  const [, month, date] = day.split("-");
  return `${Number(date)} ${MONTHS[Number(month) - 1]}`;
}

/**
 * The short axis label of one chart bucket, for the bucket kind an explicit
 * range is aggregated over: the hour for hourly buckets, the month for
 * monthly ones, and "9 Oct" style days and weeks otherwise.
 */
export function bucketLabel(
  day: string,
  bucket: "hour" | "day" | "week" | "month",
  timeFormat = "24h",
): string {
  if (bucket === "hour") return formatTime(day.slice(11, 16), timeFormat);
  if (bucket === "month") {
    const [, month] = day.split("-");
    return MONTHS[Number(month) - 1];
  }
  const [, month, date] = day.split("-");
  return `${Number(date)} ${MONTHS[Number(month) - 1]}`;
}

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
    default:
      return `${year}-${month}-${day}`;
  }
}
