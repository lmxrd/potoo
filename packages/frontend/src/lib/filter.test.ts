import { describe, expect, it } from "vitest";
import { buildFilter, parseFilter, type FacetFilters } from "./filter";

const filters = (entries: [string, string[]][]): FacetFilters =>
  new Map(entries.map(([facet, values]) => [facet, new Set(values)]));

describe("parseFilter", () => {
  it("reads OR within a facet and AND across facets", () => {
    expect(parseFilter("path==/pricing;(country==no,country==de)")).toEqual(
      filters([
        ["country", ["no", "de"]],
        ["path", ["/pricing"]],
      ]),
    );
  });

  it("stores the lowercase facets lowercase, and keeps paths as written", () => {
    expect(parseFilter("country==NO;browser==Chrome;path==/Docs")).toEqual(
      filters([
        ["country", ["no"]],
        ["browser", ["chrome"]],
        ["path", ["/Docs"]],
      ]),
    );
  });

  it("refuses what the dashboard cannot express", () => {
    expect(parseFilter("color==red")).toBeNull(); // an unknown facet
    expect(parseFilter("country!=no")).toBeNull(); // another operator
    expect(parseFilter("country==no,path==/")).toBeNull(); // OR across facets
    // AND binds tighter than OR: without parentheses this is an OR across
    // facets too.
    expect(parseFilter("country==no,country==de;path==/pricing")).toBeNull();
    expect(parseFilter("country==")).toBeNull(); // not RSQL
  });
});

describe("buildFilter", () => {
  it("writes nothing for no filters", () => {
    expect(buildFilter(new Map())).toBeUndefined();
    expect(buildFilter(filters([["country", []]]))).toBeUndefined();
  });

  it("writes what parseFilter reads back", () => {
    const view = filters([
      ["country", ["no", "de"]],
      ["path", ["/pricing"]],
      ["name", ["sign up"]],
    ]);
    const rsql = buildFilter(view);
    expect(rsql).toBeDefined();
    expect(parseFilter(rsql!)).toEqual(view);
  });

  it("writes a single comparison plainly", () => {
    expect(buildFilter(filters([["device", ["mobile"]]]))).toBe("device==mobile");
  });
});
