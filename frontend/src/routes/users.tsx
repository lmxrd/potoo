import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { createFileRoute, redirect } from "@tanstack/react-router";
import {
  changeUserRoleMutation,
  createUserMutation,
  deleteUserMutation,
  listUsersOptions,
  listUsersQueryKey,
  resetUserPasswordMutation,
} from "../client/@tanstack/react-query.gen";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { Input } from "../components/input";
import { errorMessage } from "../lib/api";

export const Route = createFileRoute("/users")({
  beforeLoad: ({ context }) => {
    if (!context.session.user?.is_admin) throw redirect({ to: "/" });
  },
  component: Users,
});

function Users() {
  const { data: users, error } = useQuery(listUsersOptions());
  const queryClient = useQueryClient();
  const reload = () => queryClient.invalidateQueries({ queryKey: listUsersQueryKey() });
  const createUser = useMutation(createUserMutation());
  const changeRole = useMutation(changeUserRoleMutation());
  const deleteUser = useMutation(deleteUserMutation());
  const resetPassword = useMutation(resetUserPasswordMutation());
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [admin, setAdmin] = useState(false);
  const [formError, setFormError] = useState<string>();
  const [reset, setReset] = useState<{ id: number; password: string; error?: string } | null>(null);
  const [rowError, setRowError] = useState<{ id: number; message: string } | null>(null);

  const showRowError = (id: number, err: unknown) =>
    setRowError({ id, message: errorMessage(err, "Failed") });

  const admins = users?.filter((u) => u.is_admin) ?? [];
  // With a single admin, demoting or deleting them is disabled: the app
  // must always keep at least one admin.
  const lastAdminId = admins.length === 1 ? admins[0].id : undefined;
  const guarded = { title: "At least one admin is required" };

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Users</h1>
      <ErrorText>{error && errorMessage(error)}</ErrorText>
      <Card>
        <ul className="divide-y divide-line">
          {users?.map((u) => (
            <li key={u.id} className="py-2">
              <div className="flex items-center justify-between">
                <span>
                  {u.email} {u.is_admin && <span className="ml-2 text-xs text-muted">admin</span>}
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
                        await changeRole.mutateAsync({
                          path: { id: u.id },
                          body: { is_admin: !u.is_admin },
                        });
                        setRowError(null);
                        reload();
                      } catch (err) {
                        showRowError(u.id, err);
                      }
                    }}
                  >
                    {u.is_admin ? "Make regular" : "Make admin"}
                  </Button>
                  <Button
                    variant="danger"
                    disabled={u.id === lastAdminId}
                    {...(u.id === lastAdminId ? guarded : {})}
                    onClick={async () => {
                      if (confirm(`Delete ${u.email}?`)) {
                        try {
                          await deleteUser.mutateAsync({ path: { id: u.id } });
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
                      await resetPassword.mutateAsync({
                        path: { id: u.id },
                        body: { new_password: reset.password },
                      });
                      setReset(null);
                    } catch (err) {
                      setReset({
                        ...reset,
                        error: errorMessage(err, "Failed"),
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
              await createUser.mutateAsync({
                body: { email, password, is_admin: admin },
              });
              setEmail("");
              setPassword("");
              setAdmin(false);
              reload();
            } catch (err) {
              setFormError(errorMessage(err, "Failed"));
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
