/** The date formats the backend accepts, with a label for the dropdown. */
export const DATE_FORMATS = [
  { value: "YYYY-MM-DD", label: "YYYY-MM-DD (2026-10-09)" },
  { value: "DD/MM/YYYY", label: "DD/MM/YYYY (09/10/2026)" },
  { value: "MM/DD/YYYY", label: "MM/DD/YYYY (10/09/2026)" },
  { value: "DD.MM.YYYY", label: "DD.MM.YYYY (09.10.2026)" },
  { value: "D MMM YYYY", label: "D MMM YYYY (9 Oct 2026)" },
];

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
