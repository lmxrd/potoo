import { afterEach, describe, expect, it, vi } from "vitest";
import { facetLabel, storedValue } from "./labels";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("facetLabel", () => {
  it("names the known values of the lowercase facets", () => {
    expect(facetLabel("os", "macos")).toBe("macOS");
    expect(facetLabel("browser", "chrome")).toBe("Chrome");
    expect(facetLabel("device", "tablet")).toBe("Tablet");
    expect(facetLabel("country", "no")).toBe("Norway");
  });

  it("names the hits nothing is known of", () => {
    expect(facetLabel("country", "unknown")).toBe("Unknown");
    expect(facetLabel("browser", "unknown")).toBe("Unknown");
  });

  it("shows paths, referrers and clicks as they are", () => {
    expect(facetLabel("path", "/pricing")).toBe("/pricing");
    expect(facetLabel("referrer", "news.ycombinator.com")).toBe("news.ycombinator.com");
    expect(facetLabel("name", "unknown")).toBe("unknown");
  });

  it("shows an unlabelled value as it is, and warns about it once", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(facetLabel("browser", "palemoon")).toBe("palemoon");
    expect(facetLabel("browser", "palemoon")).toBe("palemoon");
    expect(warn).toHaveBeenCalledOnce();
    expect(warn.mock.calls[0][0]).toContain('browser "palemoon"');
  });
});

describe("storedValue", () => {
  it("lowercases the facets the backend stores lowercase", () => {
    expect(storedValue("country", "NO")).toBe("no");
    expect(storedValue("referrer", "Google.com")).toBe("google.com");
  });

  it("keeps paths and click labels as written", () => {
    expect(storedValue("path", "/Docs")).toBe("/Docs");
    expect(storedValue("name", "Sign Up")).toBe("Sign Up");
  });
});
