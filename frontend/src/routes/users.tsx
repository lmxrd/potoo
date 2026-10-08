import { useState } from "react";
import { createFileRoute, redirect } from "@tanstack/react-router";
import { api, useApi } from "../api";
import { Button, Card, ErrorText, Input } from "../components";

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
  const [formError, setFormError] = useState<string>();
  const [reset, setReset] = useState<{ id: number; password: string; error?: string } | null>(null);
  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Users</h1>
      <ErrorText>{error}</ErrorText>
      <Card>
        <ul className="divide-y divide-slate-200">
          {users?.map((u) => (
            <li key={u.id} className="py-2">
              <div className="flex items-center justify-between">
                <span>
                  {u.email}{" "}
                  {u.is_admin === 1 && <span className="ml-2 text-xs text-slate-500">admin</span>}
                </span>
                {u.is_admin !== 1 && (
                  <span className="flex gap-2">
                    <Button
                      onClick={() =>
                        setReset(reset?.id === u.id ? null : { id: u.id, password: "" })
                      }
                    >
                      Reset password
                    </Button>
                    <Button
                      variant="danger"
                      onClick={async () => {
                        if (confirm(`Delete ${u.email}?`)) {
                          await api(`/users/${u.id}`, { method: "DELETE" });
                          reload();
                        }
                      }}
                    >
                      Delete
                    </Button>
                  </span>
                )}
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
            </li>
          ))}
        </ul>
      </Card>
      <Card title="Create a user">
        <form
          className="flex flex-wrap gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            setFormError(undefined);
            try {
              await api("/users", { body: { email, password } });
              setEmail("");
              setPassword("");
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
          <Button type="submit">Create</Button>
        </form>
        <ErrorText>{formError}</ErrorText>
      </Card>
    </div>
  );
}
