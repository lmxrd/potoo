import { useCallback, useEffect, useState } from "react";

export type User = { id: number; email: string; is_admin: number };
export type Session = { needsSetup: boolean; user: User | null };

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export async function api<T = void>(
  path: string,
  init?: { method?: string; body?: unknown },
): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method: init?.method ?? (init?.body ? "POST" : "GET"),
    headers: init?.body ? { "content-type": "application/json" } : undefined,
    body: init?.body ? JSON.stringify(init.body) : undefined,
  });
  if (!res.ok) {
    const data = await res.json().catch(() => null);
    const message = typeof data?.error === "string" ? data.error : "Request failed";
    throw new ApiError(res.status, message);
  }
  // 204 has no body; a 201 may (e.g. the created site) or may not (setup).
  if (res.status === 204) return undefined as T;
  return (await res.json().catch(() => null)) as T;
}

export function useApi<T>(path: string) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    api<T>(path).then(setData, (e: Error) => setError(e.message));
  }, [path]);
  useEffect(reload, [reload]);
  return { data, error, reload };
}
