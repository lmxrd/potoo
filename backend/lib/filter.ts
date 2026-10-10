import {
  type ComparisonNode,
  getSelector,
  getValue,
  isComparisonNode,
  isLogicNode,
} from "@rsql/ast";
import { parse } from "@rsql/parser";

/**
 * The RSQL the stats endpoints accept: equality comparisons of the facets
 * the dashboard shows, OR (`,`) within a facet and AND (`;`) across
 * facets — the dashboard's own semantics, in the standard language.
 *
 *     country==NO,country==DE;path==/pricing
 *
 * Anything outside that subset — other comparison operators, unknown
 * facets, OR across facets — is an error.
 */

/** Each facet, as the column its values are stored in. */
const COLUMNS: Record<string, string> = {
  path: "path",
  country: "country",
  device: "device",
  browser: "browser",
  os: "os",
  referrer: "referrer",
  name: "name",
};

/** The filters of one stats request: facet to values, OR within a facet. */
export type FacetFilters = Record<string, string[]>;

type Node = ReturnType<typeof parse>;

/** Parse an RSQL filter into the facets it selects. Throws with a message fit for a 400. */
export function parseFilter(rsql: string): FacetFilters {
  let node: Node;
  try {
    node = parse(rsql);
  } catch (e) {
    throw new Error(`Invalid filter: ${(e as Error).message}`);
  }
  const facets = new Map<string, Set<string>>();
  walk(node, facets);
  return Object.fromEntries(
    [...facets].map(([facet, values]) => [facet, [...values]]),
  );
}

/** One comparison into the facets; equality only, on a known facet. */
function add(node: Node, facets: Map<string, Set<string>>) {
  if (node.operator !== "==") {
    throw new Error(
      `Invalid filter: the ${node.operator} operator is not supported`,
    );
  }
  const facet = getSelector(node);
  if (!COLUMNS[facet]) {
    throw new Error(`Invalid filter: unknown facet '${facet}'`);
  }
  const values = facets.get(facet) ?? new Set<string>();
  values.add(String(getValue(node)));
  facets.set(facet, values);
}

/**
 * AND (`;`) applies every operand as its own filter; OR (`,`) must stay
 * within one facet, since the dashboard cannot express anything else.
 */
function walk(node: Node, facets: Map<string, Set<string>>) {
  if (isComparisonNode(node)) {
    add(node, facets);
    return;
  }
  if (!isLogicNode(node)) {
    throw new Error("Invalid filter");
  }
  const operator = node.operator.toLowerCase();
  if (operator === "and" || operator === ";") {
    walk(node.left, facets);
    walk(node.right, facets);
    return;
  }
  if (operator === "or" || operator === ",") {
    const group = comparisons(node);
    const fields = new Set(group.map(getSelector));
    if (fields.size > 1) {
      throw new Error(
        "Invalid filter: OR is only supported within a single facet",
      );
    }
    for (const comparison of group) add(comparison, facets);
    return;
  }
  throw new Error(
    `Invalid filter: the ${node.operator} operator is not supported`,
  );
}

/** Every comparison below a node. */
function comparisons(node: Node): ComparisonNode[] {
  if (isComparisonNode(node)) return [node];
  if (!isLogicNode(node)) throw new Error("Invalid filter");
  return [...comparisons(node.left), ...comparisons(node.right)];
}

/** The filters as a SQL clause over the hits table, with its parameters. */
export function filterSql(
  filters: FacetFilters,
): { clause: string; params: string[] } {
  const clauses: string[] = [];
  const params: string[] = [];
  for (const [facet, values] of Object.entries(filters)) {
    clauses.push(
      `${COLUMNS[facet]} IN (${values.map(() => "?").join(", ")})`,
    );
    params.push(...values);
  }
  return { clause: clauses.join(" AND "), params };
}

/** The filter of a request's query param, or the error to answer it with. */
export function parseFilterParam(
  raw: string | undefined,
): { ok: true; filters: FacetFilters } | { ok: false; error: string } {
  if (!raw) return { ok: true, filters: {} };
  try {
    return { ok: true, filters: parseFilter(raw) };
  } catch (e) {
    return { ok: false, error: (e as Error).message };
  }
}
