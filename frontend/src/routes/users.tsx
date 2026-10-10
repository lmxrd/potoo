import { useState } from "react";
import { createFileRoute, redirect } from "@tanstack/react-router";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { Input } from "../components/input";
import { api, useApi } from "../lib/api";

type Row = { id: number; email: string; is_admin: number; created_at: string };

export const Route = createFileRoute("/users")({
  beforeLoad: ({ context }) => {
    if (context.session.user?.is_admin !== 1) throw redirect({ to: "/" });
  },
  component: Users,
});

function Users() {
  const { data: users, error, reload } = useApi<Row[]>("/users");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [admin, setAdmin] = useState(false);
  const [formError, setFormError] = useState<string>();
  const [reset, setReset] = useState<{ id: number; password: string; error?: string } | null>(null);
  const [rowError, setRowError] = useState<{ id: number; message: string } | null>(null);

  const showRowError = (id: number, err: unknown) =>
    setRowError({
      id,
      message: err instanceof Error ? err.message : "Failed",
    });

  const admins = users?.filter((u) => u.is_admin === 1) ?? [];
  // With a single admin, demoting or deleting them is disabled: the app
  // must always keep at least one admin.
  const lastAdminId = admins.length === 1 ? admins[0].id : undefined;
  const guarded = { title: "At least one admin is required" };

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Users</h1>
      <ErrorText>{error}</ErrorText>
      <Card>
        <ul className="divide-y divide-line">
          {users?.map((u) => (
            <li key={u.id} className="py-2">
              <div className="flex items-center justify-between">
                <span>
                  {u.email}{" "}
                  {u.is_admin === 1 && <span className="ml-2 text-xs text-muted">admin</span>}
                </span>
                <span className="flex gap-2">
                  <Button
                    onClick={() => setReset(reset?.id === u.id ? null : { id: u.id, password: "" })}
                  >
                    Reset password
                  </Button>
                  <Button
                    disabled={u.id === lastAdminId}
                    {...(u.id === lastAdminId ? guarded : {})}
                    onClick={async () => {
                      try {
                        await api(`/users/${u.id}`, {
                          method: "PATCH",
                          body: { is_admin: u.is_admin !== 1 },
                        });
                        setRowError(null);
                        reload();
                      } catch (err) {
                        showRowError(u.id, err);
                      }
                    }}
                  >
                    {u.is_admin === 1 ? "Make regular" : "Make admin"}
                  </Button>
                  <Button
                    variant="danger"
                    disabled={u.id === lastAdminId}
                    {...(u.id === lastAdminId ? guarded : {})}
                    onClick={async () => {
                      if (confirm(`Delete ${u.email}?`)) {
                        try {
                          await api(`/users/${u.id}`, { method: "DELETE" });
                          setRowError(null);
                          reload();
                        } catch (err) {
                          showRowError(u.id, err);
                        }
                      }
                    }}
                  >
                    Delete
                  </Button>
                </span>
              </div>
              {reset?.id === u.id && (
                <form
                  className="mt-2 flex flex-wrap gap-2"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    try {
                      await api(`/users/${u.id}/password`, {
                        body: { new_password: reset.password },
                      });
                      setReset(null);
                    } catch (err) {
                      setReset({
                        ...reset,
                        error: err instanceof Error ? err.message : "Failed",
                      });
                    }
                  }}
                >
                  <Input
                    type="password"
                    placeholder="New password (min. 10)"
                    required
                    minLength={10}
                    value={reset.password}
                    onChange={(e) => setReset({ ...reset, password: e.target.value })}
                  />
                  <Button type="submit">Save</Button>
                  <Button onClick={() => setReset(null)}>Cancel</Button>
                </form>
              )}
              {reset?.id === u.id && <ErrorText>{reset.error}</ErrorText>}
              {rowError?.id === u.id && <ErrorText>{rowError.message}</ErrorText>}
            </li>
          ))}
        </ul>
      </Card>
      <Card title="Create a user">
        <form
          className="flex flex-wrap items-center gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            setFormError(undefined);
            try {
              await api("/users", {
                body: { email, password, is_admin: admin },
              });
              setEmail("");
              setPassword("");
              setAdmin(false);
              reload();
            } catch (err) {
              setFormError(err instanceof Error ? err.message : "Failed");
            }
          }}
        >
          <Input
            type="email"
            placeholder="Email"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <Input
            type="password"
            placeholder="Password (min. 10)"
            required
            minLength={10}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={admin} onChange={(e) => setAdmin(e.target.checked)} />
            Admin
          </label>
          <Button type="submit">Create</Button>
        </form>
        <ErrorText>{formError}</ErrorText>
      </Card>
    </div>
  );
}
