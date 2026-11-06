import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  localInput,
  parseSites,
  rangeQuery,
  rangeSummary,
  rangeWindow,
  searchRange,
  validateDashboardSearch,
} from "./date-range";

// The tests run in UTC (vite.config.ts), so local times are UTC times.
const NOW = new Date("2026-11-06T14:30:00Z");

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("rangeWindow", () => {
  it("ends a preset now, as long as it says", () => {
    expect(rangeWindow({ preset: "PT24H" })).toEqual({
      from: new Date("2026-11-05T14:30:00Z"),
      to: NOW,
    });
    expect(rangeWindow({ preset: "P7D" })?.from).toEqual(new Date("2026-10-30T14:30:00Z"));
    expect(rangeWindow({ preset: "P1Y" })?.from).toEqual(new Date("2025-11-06T14:30:00Z"));
  });

  it("starts today at UTC midnight", () => {
    expect(rangeWindow({ preset: "today" })).toEqual({
      from: new Date("2026-11-06T00:00:00Z"),
      to: NOW,
    });
  });

  it("takes a custom window as it is, and refuses one that ends before it starts", () => {
    const custom = { preset: "custom" as const, from: "2026-11-01T08:00", to: "2026-11-02T08:00" };
    expect(rangeWindow(custom)).toEqual({
      from: new Date("2026-11-01T08:00:00Z"),
      to: new Date("2026-11-02T08:00:00Z"),
    });
    expect(rangeWindow({ ...custom, to: custom.from })).toBeNull();
    expect(rangeWindow({ ...custom, from: "nonsense" })).toBeNull();
  });
});

describe("rangeQuery", () => {
  it("is the exact window, as ISO timestamps", () => {
    expect(rangeQuery({ preset: "PT24H" })).toEqual({
      from: "2026-11-05T14:30:00.000Z",
      to: "2026-11-06T14:30:00.000Z",
    });
    expect(rangeQuery({ preset: "custom", from: "b", to: "a" })).toBeNull();
  });
});

describe("rangeSummary", () => {
  it("says the window in the user's formats", () => {
    expect(rangeSummary({ preset: "today" }, "DD.MM.YYYY", false)).toBe(
      "06.11.2026 12:00 AM - 06.11.2026 2:30 PM",
    );
    expect(rangeSummary({ preset: "custom", from: "x", to: "y" })).toContain("Invalid range");
  });
});

describe("localInput", () => {
  it("writes a datetime-local value", () => {
    expect(localInput(new Date("2026-01-02T03:04:59Z"))).toBe("2026-01-02T03:04");
  });
});

describe("validateDashboardSearch", () => {
  it("keeps the well-formed params", () => {
    expect(
      validateDashboardSearch({
        range: "P30D",
        from: "2026-11-01T08:00",
        to: "2026-11-02T08:00",
        sites: "blog.example.com,shop.example.com",
        q: "country==no",
      }),
    ).toEqual({
      range: "P30D",
      from: "2026-11-01T08:00",
      to: "2026-11-02T08:00",
      sites: "blog.example.com,shop.example.com",
      q: "country==no",
    });
  });

  it("drops the rest", () => {
    expect(
      validateDashboardSearch({
        range: "today", // Today is never in the URL
        from: "yesterday",
        to: 5,
        sites: "<script>",
        q: "color==red",
        other: "x",
      }),
    ).toEqual({});
  });

  it("reads a hostname the router took for a number as text", () => {
    expect(validateDashboardSearch({ sites: 1234 })).toEqual({ sites: "1234" });
  });
});

describe("searchRange", () => {
  it("prefers a valid from/to, then a preset, then 7 days", () => {
    expect(
      searchRange({ from: "2026-11-01T08:00", to: "2026-11-02T08:00", range: "P30D" }),
    ).toEqual({ preset: "custom", from: "2026-11-01T08:00", to: "2026-11-02T08:00" });
    expect(
      searchRange({ from: "2026-11-02T08:00", to: "2026-11-01T08:00", range: "P30D" }),
    ).toEqual({ preset: "P30D" });
    expect(searchRange({})).toEqual({ preset: "P7D" });
  });
});

describe("parseSites", () => {
  it("is the set of hostnames, empty for every site", () => {
    expect(parseSites("a.com,b.com")).toEqual(new Set(["a.com", "b.com"]));
    expect(parseSites(undefined)).toEqual(new Set());
    expect(parseSites("")).toEqual(new Set());
  });
});
