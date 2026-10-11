import { QueryClient } from "@tanstack/react-query";

/**
 * The one query cache of the app. The API is generated from the backend's
 * OpenAPI spec (`src/client`, see `pnpm run gen:api`): routes use its query
 * options and mutations through TanStack Query, and this module holds what
 * is shared around them.
 */
export const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

/**
 * The message of a failed request. The API answers errors as
 * `{"error": "..."}`, which the generated client throws as it is.
 */
export function errorMessage(error: unknown, fallback = "Something went wrong"): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "error" in error) {
    if (typeof error.error === "string") return error.error;
  }
  return fallback;
}
