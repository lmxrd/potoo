import { useQuery } from "@tanstack/react-query";
import { meOptions } from "potoo-sdk/react-query";

import { bucketLabel, chartLabel, formatDate, formatTime } from "./dates";

const DEFAULT_DATE_FORMAT = "YYYY-MM-DD";

/** The date and time formatters bound to the signed-in user's date format and clock. */
export function useFormatters() {
  // Only reads what the root route loaded (it fetches the user on every
  // navigation, and again after a settings change); never asks itself.
  const { data: me } = useQuery({ ...meOptions(), enabled: false });
  const dateFormat = me?.date_format ?? DEFAULT_DATE_FORMAT;
  const use24 = me?.use_24 ?? true;
  return {
    dateFormat,
    use24,
    /** A `YYYY-MM-DD` day, or the day of a timestamp. */
    date: (value: string) => formatDate(value, dateFormat),
    /** A `HH:MM` time. */
    time: (value: string) => formatTime(value, use24),
    /** A chart bucket: a day, or a `YYYY-MM-DDTHH:MM` hour. */
    bucket: (day: string) =>
      day.length > 10
        ? `${formatDate(day.slice(0, 10), dateFormat)} ${formatTime(day.slice(11, 16), use24)}`
        : formatDate(day, dateFormat),
    /** The short axis label of a chart bucket, for a range of days. */
    chartLabel: (day: string, range: number) => chartLabel(day, range, use24),
    /** The short axis label of a chart bucket, for an explicit bucket kind. */
    bucketLabel: (day: string, bucket: Parameters<typeof bucketLabel>[1]) =>
      bucketLabel(day, bucket, use24),
  };
}
