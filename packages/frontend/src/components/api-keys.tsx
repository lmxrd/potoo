import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "./button";
import { ErrorText } from "./error-text";
import { Input } from "./input";
import {
  createApiKeyMutation,
  listApiKeysOptions,
  listApiKeysQueryKey,
  revokeApiKeyMutation,
} from "potoo-sdk/react-query";
import type { Scope } from "potoo-sdk/types";
import { errorMessage } from "../lib/api";
import { formatDate } from "../lib/dates";

const SCOPES: { value: Scope; label: string; help: string }[] = [
  { value: "read", label: "Read", help: "Reads only: stats, sites, exports." },
  { value: "write", label: "Read & write", help: "Also changes things, but no admin settings." },
  { value: "admin", label: "Admin", help: "Everything you can do as an admin." },
];

const EXPIRIES = [
  { days: 30, label: "30 days" },
  { days: 90, label: "90 days" },
  { days: 365, label: "1 year" },
  { days: undefined, label: "Never" },
];

/**
 * The signed-in user's API keys: make one (name, permission, expiry), see
 * its token once, and revoke them. Keys are sent as
 * `Authorization: Bearer <token>`.
 */
export function ApiKeys({ isAdmin, dateFormat }: { isAdmin: boolean; dateFormat: string }) {
  const queryClient = useQueryClient();
  const { data: keys, error: listError } = useQuery(listApiKeysOptions());
  const create = useMutation(createApiKeyMutation());
  const revoke = useMutation(revokeApiKeyMutation());
  const [name, setName] = useState("");
  const [scope, setScope] = useState<Scope>("read");
  const [expiry, setExpiry] = useState(0);
  const [token, setToken] = useState<string>();
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string>();
  const reload = () => queryClient.invalidateQueries({ queryKey: listApiKeysQueryKey() });
  const scopes = SCOPES.filter((s) => s.value !== "admin" || isAdmin);
  const date = (value: string) => formatDate(value, dateFormat);

  return (
    <div className="space-y-4">
      {token && (
        <div className="space-y-2 rounded border border-line bg-subtle p-3">
          <p className="text-sm">Your new key. Copy it now: it is not shown again.</p>
          <code className="block break-all font-mono text-xs">{token}</code>
          <div className="flex gap-2">
            <Button
              onClick={async () => {
                await navigator.clipboard.writeText(token);
                setCopied(true);
              }}
            >
              {copied ? "Copied" : "Copy"}
            </Button>
            <Button onClick={() => setToken(undefined)}>Done</Button>
          </div>
        </div>
      )}
      <form
        className="flex flex-col gap-3"
        onSubmit={async (e) => {
          e.preventDefault();
          setError(undefined);
          try {
            const created = await create.mutateAsync({
              body: { name, scope, expires_in_days: EXPIRIES[expiry].days },
            });
            setName("");
            setCopied(false);
            setToken(created.token);
            reload();
          } catch (err) {
            setError(errorMessage(err));
          }
        }}
      >
        <Input
          required
          maxLength={100}
          placeholder="Name, e.g. Reporting script"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <div className="flex gap-2 text-sm">
          <label className="flex flex-1 flex-col gap-1">
            <span className="text-muted">Permission</span>
            <select
              className="rounded border border-line-strong bg-card px-2 py-2"
              value={scope}
              onChange={(e) => setScope(e.target.value as Scope)}
            >
              {scopes.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-1 flex-col gap-1">
            <span className="text-muted">Expires</span>
            <select
              className="rounded border border-line-strong bg-card px-2 py-2"
              value={expiry}
              onChange={(e) => setExpiry(Number(e.target.value))}
            >
              {EXPIRIES.map((x, i) => (
                <option key={x.label} value={i}>
                  {x.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <p className="text-xs text-muted">{SCOPES.find((s) => s.value === scope)?.help}</p>
        <ErrorText>{error}</ErrorText>
        <Button type="submit" disabled={create.isPending}>
          Create key
        </Button>
      </form>
      <ErrorText>{listError && errorMessage(listError)}</ErrorText>
      {keys && keys.length > 0 && (
        <ul className="divide-y divide-line border-t border-line text-sm">
          {keys.map((k) => {
            const expired = k.expires_at !== null && new Date(k.expires_at) < new Date();
            return (
              <li key={k.id} className="flex items-center justify-between gap-2 py-2">
                <div className="min-w-0">
                  <div className="truncate">
                    {k.name}{" "}
                    <span className="text-xs text-muted">
                      {SCOPES.find((s) => s.value === k.scope)?.label}
                    </span>
                    {expired && <span className="ml-2 text-xs text-danger">expired</span>}
                  </div>
                  <div className="text-xs text-muted">
                    <code className="font-mono">{k.prefix}…</code> · created {date(k.created_at)}
                    {" · "}
                    {k.last_used_at ? `last used ${date(k.last_used_at)}` : "never used"}
                    {k.expires_at && !expired && ` · expires ${date(k.expires_at)}`}
                  </div>
                </div>
                <Button
                  variant="danger"
                  onClick={async () => {
                    if (!confirm(`Revoke ${k.name}? Whatever uses it stops working.`)) return;
                    try {
                      await revoke.mutateAsync({ path: { id: k.id } });
                      reload();
                    } catch (err) {
                      setError(errorMessage(err));
                    }
                  }}
                >
                  Revoke
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
