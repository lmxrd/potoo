import { describe, expect, it } from "vitest";
import { DATE_FORMATS, bucketLabel, chartLabel, formatDate, formatTime } from "./dates";

describe("formatTime", () => {
  it("keeps 24-hour times as they are", () => {
    expect(formatTime("14:30")).toBe("14:30");
    expect(formatTime("00:05", true)).toBe("00:05");
  });

  it("writes 12-hour times with AM and PM", () => {
    expect(formatTime("14:30", false)).toBe("2:30 PM");
    expect(formatTime("00:05", false)).toBe("12:05 AM");
    expect(formatTime("12:00", false)).toBe("12:00 PM");
    expect(formatTime("09:07", false)).toBe("9:07 AM");
  });
});

describe("formatDate", () => {
  it("writes a day in each format the backend accepts", () => {
    const shown = DATE_FORMATS.map((f) => formatDate("2026-10-09", f.value));
    expect(shown).toEqual(["2026-10-09", "09/10/2026", "10/09/2026", "09.10.2026"]);
  });

  it("takes the day of a full timestamp", () => {
    expect(formatDate("2026-10-09T23:59:00.000Z", "DD.MM.YYYY")).toBe("09.10.2026");
  });

  it("falls back to YYYY-MM-DD, and leaves what is not a date alone", () => {
    expect(formatDate("2026-10-09", "nonsense")).toBe("2026-10-09");
    expect(formatDate("yesterday", "DD/MM/YYYY")).toBe("yesterday");
  });

  it("labels every format with its own example", () => {
    for (const f of DATE_FORMATS) {
      expect(f.label).toBe(`${f.value} (${formatDate("2026-10-09", f.value)})`);
    }
  });
});

describe("chartLabel", () => {
  it("shows the hour for a day's range", () => {
    expect(chartLabel("2026-10-09T14:00:00Z", 1)).toBe("14:00");
    expect(chartLabel("2026-10-09T14:00:00Z", 1, false)).toBe("2:00 PM");
  });

  it("shows the day for ranges up to 90 days", () => {
    expect(chartLabel("2026-10-09", 30)).toBe("9 Oct");
  });

  it("shows the month for longer ranges, with the year in January", () => {
    expect(chartLabel("2026-10-01", 365)).toBe("Oct");
    expect(chartLabel("2027-01-01", 365)).toBe("Jan 27");
  });
});

describe("bucketLabel", () => {
  it("labels each kind of bucket", () => {
    expect(bucketLabel("2026-10-09T08:00:00Z", "hour", false)).toBe("8:00 AM");
    expect(bucketLabel("2026-10-09", "day")).toBe("9 Oct");
    expect(bucketLabel("2026-10-05", "week")).toBe("5 Oct");
    expect(bucketLabel("2026-12-01", "month")).toBe("Dec");
  });
});
