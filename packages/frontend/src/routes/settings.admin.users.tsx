import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import {
  changeUserRoleMutation,
  createUserMutation,
  deleteUserMutation,
  listUsersOptions,
  listUsersQueryKey,
  resetUserPasswordMutation,
  resetUserTotpMutation,
} from "potoo-sdk/react-query";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { ErrorText } from "../components/common/error-text";
import { Input } from "../components/ui/input";
import { errorMessage } from "../lib/api";
import { Checkbox } from "../components/ui/checkbox";
import { useConfirm } from "../components/common/confirm-dialog";

export const Route = createFileRoute("/settings/admin/users")({
  component: Users,
});

function Users() {
  const confirm = useConfirm();
  const { session } = Route.useRouteContext();
  // With single sign-on on, a user may have no password.
  const passwordOptional = session.sso !== null;
  // With single sign-on only (OIDC_ONLY) there are no passwords at all.
  const passwords = session.password_login;
  const { data: users, error } = useQuery(listUsersOptions());
  const queryClient = useQueryClient();
  const reload = () => queryClient.invalidateQueries({ queryKey: listUsersQueryKey() });
  const createUser = useMutation({ ...createUserMutation(), meta: { inline: true } });
  const changeRole = useMutation({ ...changeUserRoleMutation(), meta: { inline: true } });
  const deleteUser = useMutation({ ...deleteUserMutation(), meta: { inline: true } });
  const resetPassword = useMutation({ ...resetUserPasswordMutation(), meta: { inline: true } });
  const resetTotp = useMutation({ ...resetUserTotpMutation(), meta: { inline: true } });
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [admin, setAdmin] = useState(false);
  const [formError, setFormError] = useState<string>();
  const [reset, setReset] = useState<{ id: string; password: string; error?: string } | null>(null);
  const [rowError, setRowError] = useState<{ id: string; message: string } | null>(null);

  const showRowError = (id: string, err: unknown) =>
    setRowError({ id, message: errorMessage(err, "Failed") });

  const admins = users?.filter((u) => u.is_admin) ?? [];
  // With a single admin, demoting or deleting them is disabled: the app
  // must always keep at least one admin.
  const lastAdminId = admins.length === 1 ? admins[0].id : undefined;
  const guarded = { title: "At least one admin is required" };

  return (
    <div className="space-y-6">
      <ErrorText>{error && errorMessage(error)}</ErrorText>
      <Card>
        <CardHeader>
          <CardTitle>Users</CardTitle>
        </CardHeader>
        <CardContent>
          <ul className="divide-y divide-border">
            {users?.map((u) => (
              <li key={u.id} className="py-2">
                <div className="flex items-center justify-between">
                  <span>
                    {u.email}{" "}
                    {u.is_admin && (
                      <span className="ml-2 text-xs text-muted-foreground">admin</span>
                    )}
                    {u.single_sign_on && (
                      <span className="ml-2 text-xs text-muted-foreground">single sign-on</span>
                    )}
                    {!u.has_password && (
                      <span className="ml-2 text-xs text-muted-foreground">no password</span>
                    )}
                    {u.totp_enabled && (
                      <span className="ml-2 text-xs text-muted-foreground">2FA</span>
                    )}
                  </span>
                  <span className="flex gap-2">
                    {/* For someone who lost their app and their recovery codes. */}
                    {u.totp_enabled && u.id !== session.user?.id && (
                      <Button
                        onClick={async () => {
                          if (
                            !(await confirm({
                              title: `Turn off two-factor authentication for ${u.email}?`,
                              description: "They can set it up again at their next sign-in.",
                              action: "Turn off",
                            }))
                          )
                            return;
                          try {
                            await resetTotp.mutateAsync({ path: { id: u.id } });
                            setRowError(null);
                            reload();
                          } catch (err) {
                            showRowError(u.id, err);
                          }
                        }}
                      >
                        Reset 2FA
                      </Button>
                    )}
                    {passwords && (
                      <Button
                        onClick={() =>
                          setReset(reset?.id === u.id ? null : { id: u.id, password: "" })
                        }
                      >
                        Reset password
                      </Button>
                    )}
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
                      variant="destructive"
                      disabled={u.id === lastAdminId}
                      {...(u.id === lastAdminId ? guarded : {})}
                      onClick={async () => {
                        if (
                          await confirm({
                            title: `Delete ${u.email}?`,
                            action: "Delete",
                            destructive: true,
                          })
                        ) {
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
                      className="sm:w-60"
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
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Create a user</CardTitle>
        </CardHeader>
        <CardContent>
          <form
            className="flex flex-wrap items-center gap-2"
            onSubmit={async (e) => {
              e.preventDefault();
              setFormError(undefined);
              try {
                await createUser.mutateAsync({
                  body: { email, password: password || undefined, is_admin: admin },
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
              className="sm:w-60"
              type="email"
              placeholder="Email"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
            {passwords && (
              <Input
                className="sm:w-60"
                type="password"
                placeholder={passwordOptional ? "Password (optional)" : "Password (min. 10)"}
                required={!passwordOptional}
                minLength={10}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            )}
            <label className="flex items-center gap-2 text-sm">
              <Checkbox checked={admin} onCheckedChange={(checked) => setAdmin(checked === true)} />
              Admin
            </label>
            <Button type="submit">Create</Button>
          </form>
          <ErrorText>{formError}</ErrorText>
          {passwords && passwordOptional && (
            <p className="mt-2 text-xs text-muted-foreground">
              Without a password, the user signs in with single sign-on.
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
