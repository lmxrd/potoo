import { useCallback } from "react";
import { useNavigate, useSearch } from "@tanstack/react-router";
import {
  type ComparisonNode,
  getSelector,
  getValue,
  isComparisonNode,
  isLogicNode,
} from "@rsql/ast";
import { parse } from "@rsql/parser";
import builder from "@rsql/builder";
import { emit } from "@rsql/emitter";
import type { DashboardSearch } from "./date-range";

/**
 * The dashboard's filters, as RSQL in the page's `?q=` param: equality
 * comparisons of the facets, OR (`,`) within a facet and AND (`;`) across
 * facets — for example `country==NO,country==DE;path==/pricing`.
 */

/** Each facet a filter can select, with its label: the dashboard's breakdowns. */
export const FACETS: Record<string, string> = {
  path: "Path",
  country: "Country",
  device: "Device",
  browser: "Browser",
  os: "OS",
  referrer: "Referrer",
  name: "Click",
};

/** The active filters: facet to its selected values. */
export type FacetFilters = Map<string, Set<string>>;

type Node = ReturnType<typeof parse>;

/**
 * Parse a filter expression into its facets, or null when it does not fit
 * what the dashboard can express: unknown facets, other comparison
 * operators, or OR across facets.
 */
export function parseFilter(rsql: string): FacetFilters | null {
  const facets = new Map<string, Set<string>>();
  try {
    walk(parse(rsql), facets);
  } catch {
    return null;
  }
  return facets;
}

/** The filter expression of a set of filters, or undefined when empty. */
export function buildFilter(filters: FacetFilters): string | undefined {
  const groups = [...filters]
    .filter(([, values]) => values.size)
    .map(([facet, values]) => builder.or(...[...values].map((value) => builder.eq(facet, value))));
  if (!groups.length) return undefined;
  return emit(groups.length === 1 ? groups[0] : builder.and(...groups));
}

/** One comparison into the facets: equality, on a known facet. */
function add(node: Node, facets: FacetFilters) {
  if (node.operator !== "==") throw new Error("operator");
  const facet = getSelector(node);
  if (!FACETS[facet]) throw new Error("facet");
  const values = facets.get(facet) ?? new Set<string>();
  values.add(String(getValue(node)));
  facets.set(facet, values);
}

/** AND applies every operand; OR must stay within one facet. */
function walk(node: Node, facets: FacetFilters) {
  if (isComparisonNode(node)) {
    add(node, facets);
    return;
  }
  if (!isLogicNode(node)) throw new Error("node");
  const operator = node.operator.toLowerCase();
  if (operator === "and" || operator === ";") {
    walk(node.left, facets);
    walk(node.right, facets);
    return;
  }
  if (operator === "or" || operator === ",") {
    const group = comparisons(node);
    if (new Set(group.map(getSelector)).size > 1) throw new Error("or");
    for (const comparison of group) add(comparison, facets);
    return;
  }
  throw new Error("operator");
}

/** Every comparison below a node. */
function comparisons(node: Node): ComparisonNode[] {
  if (isComparisonNode(node)) return [node];
  if (!isLogicNode(node)) throw new Error("node");
  return [...comparisons(node.left), ...comparisons(node.right)];
}

/**
 * The dashboard filters, read from and written to the page's `?q=`
 * param, so a filtered view can be shared and the back button steps
 * through it. Works on any route that validates its search with
 * validateDashboardSearch.
 */
export function useDashboardFilters() {
  const search = useSearch({ strict: false }) as DashboardSearch;
  const navigate = useNavigate();
  const filters = parseFilter(search.q ?? "") ?? new Map<string, Set<string>>();
  const setFilters = useCallback(
    (next: FacetFilters) => {
      const rsql = buildFilter(next);
      navigate({
        to: ".",
        search: (prev) => ({
          ...(prev as DashboardSearch),
          q: rsql ?? undefined,
        }),
      });
    },
    [navigate],
  );
  const toggle = useCallback(
    (facet: string, value: string) => {
      const next = new Map(filters);
      const values = new Set(next.get(facet) ?? []);
      if (values.has(value)) values.delete(value);
      else values.add(value);
      if (values.size) next.set(facet, values);
      else next.delete(facet);
      setFilters(next);
    },
    [filters, setFilters],
  );
  return { filters, toggle, setFilters };
}
