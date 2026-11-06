import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "./ui/button";
import { ErrorText } from "./error-text";
import { Input } from "./ui/input";
import {
  createApiKeyMutation,
  listApiKeysOptions,
  listApiKeysQueryKey,
  revokeApiKeyMutation,
} from "potoo-sdk/react-query";
import type { Scope } from "potoo-sdk/types";
import { errorMessage } from "../lib/api";
import { formatDate } from "../lib/dates";
import { Label } from "./ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { useConfirm } from "./confirm";

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
  const confirm = useConfirm();
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
        <div className="space-y-2 rounded border border-border bg-muted p-3">
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
          <div className="flex flex-1 flex-col gap-1">
            <Label htmlFor="api-key-scope" className="font-normal text-muted-foreground">
              Permission
            </Label>
            <Select value={scope} items={scopes} onValueChange={(next) => next && setScope(next)}>
              <SelectTrigger id="api-key-scope" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {scopes.map((s) => (
                  <SelectItem key={s.value} value={s.value}>
                    {s.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-1 flex-col gap-1">
            <Label htmlFor="api-key-expiry" className="font-normal text-muted-foreground">
              Expires
            </Label>
            <Select
              value={expiry}
              items={EXPIRIES.map((x, i) => ({ value: i, label: x.label }))}
              onValueChange={(next) => next !== null && setExpiry(next)}
            >
              <SelectTrigger id="api-key-expiry" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {EXPIRIES.map((x, i) => (
                  <SelectItem key={x.label} value={i}>
                    {x.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          {SCOPES.find((s) => s.value === scope)?.help}
        </p>
        <ErrorText>{error}</ErrorText>
        <Button type="submit" disabled={create.isPending}>
          Create key
        </Button>
      </form>
      <ErrorText>{listError && errorMessage(listError)}</ErrorText>
      {keys && keys.length > 0 && (
        <ul className="divide-y divide-border border-t border-border text-sm">
          {keys.map((k) => {
            const expired = k.expires_at !== null && new Date(k.expires_at) < new Date();
            return (
              <li key={k.id} className="flex items-center justify-between gap-2 py-2">
                <div className="min-w-0">
                  <div className="truncate">
                    {k.name}{" "}
                    <span className="text-xs text-muted-foreground">
                      {SCOPES.find((s) => s.value === k.scope)?.label}
                    </span>
                    {expired && <span className="ml-2 text-xs text-destructive">expired</span>}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    <code className="font-mono">{k.prefix}…</code> · created {date(k.created_at)}
                    {" · "}
                    {k.last_used_at ? `last used ${date(k.last_used_at)}` : "never used"}
                    {k.expires_at && !expired && ` · expires ${date(k.expires_at)}`}
                  </div>
                </div>
                <Button
                  variant="destructive"
                  onClick={async () => {
                    if (
                      !(await confirm({
                        title: `Revoke ${k.name}?`,
                        description: "Whatever uses it stops working.",
                        action: "Revoke",
                        destructive: true,
                      }))
                    )
                      return;
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
