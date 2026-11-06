import type { ImportProblem } from "potoo-sdk/types";

/**
 * What a refused import looks like: the API answers which rows are wrong
 * (some of them, `problems`) and how many there are in all
 * (`total_problems`). Null for any other error.
 */
export function problemsOf(error: unknown): { problems: ImportProblem[]; total: number } | null {
  if (typeof error !== "object" || error === null) return null;
  if (!("problems" in error) || !Array.isArray(error.problems)) return null;
  const total = "total_problems" in error ? Number(error.total_problems) : error.problems.length;
  return { problems: error.problems, total };
}
