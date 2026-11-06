import { client } from "potoo-sdk/client";

// TanStack Query base wrapper: configure the generated client (packages/sdk,
// `mise run gen:sdk`) once, before any request fires. Same-origin in
// production (the bundle is embedded in the binary) and in development (vite
// proxies /api to the backend), so the session cookie travels automatically.
// VITE_API_URL points the client elsewhere when needed.
client.setConfig({
  baseUrl: import.meta.env.VITE_API_URL ?? "",
  credentials: "include",
});

let onSessionExpired: () => void = () => {};
let lastUnauthorizedAt = 0;

/** True just after a 401, so the failed request's own error toast can stay quiet. */
export function justLostSession(): boolean {
  return Date.now() - lastUnauthorizedAt < 2000;
}

/** Register what happens when the API says the session is gone (set once, at startup). */
export function setSessionExpiredHandler(handler: () => void) {
  onSessionExpired = handler;
}

/**
 * A 401 anywhere outside /api/auth/* means the session expired or was revoked. The auth calls are
 * excluded because there a 401 is an answer ("wrong password", "not logged in"), not a lost session.
 */
export function reportResponse(response: Response) {
  if (response.status === 401 && !new URL(response.url).pathname.startsWith("/api/auth/")) {
    lastUnauthorizedAt = Date.now();
    onSessionExpired();
  }
}

client.interceptors.response.use((response) => {
  reportResponse(response);
  return response;
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
