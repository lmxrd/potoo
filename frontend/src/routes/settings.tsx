import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { api } from "../api";
import { Button, Card, ErrorText, Input } from "../components";

export const Route = createFileRoute("/settings")({ component: Settings });

function Settings() {
  const { session } = Route.useRouteContext();
  const user = session.user!;
  return (
    <div className="max-w-sm space-y-6">
      <h1 className="text-xl font-semibold">Settings</h1>
      <Card title="Account">
        <dl className="space-y-2 text-sm">
          <div className="flex justify-between gap-4">
            <dt className="text-slate-500">Email</dt>
            <dd>{user.email}</dd>
          </div>
          <div className="flex justify-between gap-4">
            <dt className="text-slate-500">Role</dt>
            <dd>{user.is_admin === 1 ? "Admin" : "User"}</dd>
          </div>
        </dl>
      </Card>
      <Card title="Change password">
        <ChangePassword />
      </Card>
    </div>
  );
}

function ChangePassword() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        setSaved(false);
        try {
          await api("/password", {
            body: { current_password: current, new_password: next },
          });
          setCurrent("");
          setNext("");
          setSaved(true);
        } catch (err) {
          setError(err instanceof Error ? err.message : "Something went wrong");
        }
      }}
    >
      <Input
        type="password"
        placeholder="Current password"
        required
        value={current}
        onChange={(e) => setCurrent(e.target.value)}
      />
      <Input
        type="password"
        placeholder="New password (min. 10 characters)"
        required
        minLength={10}
        value={next}
        onChange={(e) => setNext(e.target.value)}
      />
      <ErrorText>{error}</ErrorText>
      {saved && <p className="text-sm text-emerald-600">Password updated.</p>}
      <Button type="submit">Update password</Button>
    </form>
  );
}
