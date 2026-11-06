// The small helpers of src/lib, one describe each.
import { describe, expect, it } from "vitest";
import { errorMessage } from "./api";
import { countryName } from "./countries";
import { problemsOf } from "./import";
import { trackingSnippet } from "./snippet";
import { exportHref, statsQuery } from "./stats-query";
import { cn } from "./utils";

describe("errorMessage", () => {
  it("reads an Error, or the API's {error} body", () => {
    expect(errorMessage(new Error("Boom"))).toBe("Boom");
    expect(errorMessage({ error: "Email already in use" })).toBe("Email already in use");
  });

  it("falls back for anything else", () => {
    expect(errorMessage({ error: 42 })).toBe("Something went wrong");
    expect(errorMessage(null, "Failed")).toBe("Failed");
    expect(errorMessage("text", "Failed")).toBe("Failed");
  });
});

describe("countryName", () => {
  it("names a code in English, whatever its case", () => {
    expect(countryName("no")).toBe("Norway");
    expect(countryName("DE")).toBe("Germany");
  });

  it("is undefined for what is not a code", () => {
    expect(countryName("not a code")).toBeUndefined();
  });
});

describe("problemsOf", () => {
  const problem = { line: 2, column: "site", message: "Unknown site" };

  it("reads a refused import, with the total when the API sends it", () => {
    expect(problemsOf({ error: "x", problems: [problem], total_problems: 12 })).toEqual({
      problems: [problem],
      total: 12,
    });
    expect(problemsOf({ problems: [problem, problem] })?.total).toBe(2);
  });

  it("is null for any other error", () => {
    expect(problemsOf({ error: "Forbidden" })).toBeNull();
    expect(problemsOf(new Error("x"))).toBeNull();
    expect(problemsOf(null)).toBeNull();
  });
});

describe("trackingSnippet", () => {
  const none = { clicks: false, outbound: false, pixel: false };

  it("is the script tag of this Potoo", () => {
    expect(trackingSnippet("https://stats.example.com", "blog.example.com", none)).toBe(
      '<script async src="https://stats.example.com/tracker.js"></script>',
    );
  });

  it("adds the options, and the pixel for browsers without JavaScript", () => {
    const all = trackingSnippet("https://s.example.com", "blog.example.com", {
      clicks: true,
      outbound: true,
      pixel: true,
    });
    expect(all).toBe(
      '<script async src="https://s.example.com/tracker.js" data-clicks data-outbound></script>\n' +
        '<noscript><img src="https://s.example.com/api/pixel?h=blog.example.com" alt="" /></noscript>',
    );
  });
});

describe("statsQuery", () => {
  const range = { from: "2026-11-01T00:00:00.000Z", to: "2026-11-02T00:00:00.000Z" };

  it("is the range, with the sites and the filter when there are any", () => {
    expect(statsQuery(range, "", undefined)).toEqual(range);
    expect(statsQuery(range, "a.com,b.com", "country==no")).toEqual({
      ...range,
      sites: "a.com,b.com",
      q: "country==no",
    });
  });

  it("is null without a valid range", () => {
    expect(statsQuery(null, "a.com", undefined)).toBeNull();
  });

  it("makes the export link of the same query", () => {
    const href = exportHref({ ...range, sites: "a.com", q: "country==no" });
    expect(href.startsWith("/api/export?")).toBe(true);
    const params = new URLSearchParams(href.split("?")[1]);
    expect(Object.fromEntries(params)).toEqual({ ...range, sites: "a.com", q: "country==no" });
  });
});

describe("cn", () => {
  it("joins class names and lets the last Tailwind utility win", () => {
    expect(cn("px-2 text-sm", false && "hidden", "px-4")).toBe("text-sm px-4");
  });
});
