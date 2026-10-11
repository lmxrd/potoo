import type { GetStatsData } from "../client";
import { client } from "../client/client.gen";
import type { RangeQuery } from "./date-range";

/** The parameters of the stats and the export: one shape for both. */
export type StatsQuery = NonNullable<GetStatsData["query"]>;

/**
 * The query of a dashboard view: its range (null for an invalid custom
 * one), the selected sites (a comma separated list of hostnames, empty
 * for all of them) and the RSQL filter (empty for none).
 */
export function statsQuery(
  range: RangeQuery | null,
  sites: string,
  filter: string | undefined,
): StatsQuery | null {
  if (!range) return null;
  return { ...range, ...(sites ? { sites } : {}), ...(filter ? { q: filter } : {}) };
}

/** The link that downloads the view as CSV: the same query as the stats. */
export function exportHref(query: StatsQuery): string {
  return client.buildUrl({ url: "/api/export", query });
}
