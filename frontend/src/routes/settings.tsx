import { useState } from "react";
import { createFileRoute, useRouter } from "@tanstack/react-router";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { Input } from "../components/input";
import { api } from "../lib/api";
import { DATE_FORMATS, formatDate } from "../lib/dates";

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
      <Card title="Date format">
        <DateFormat current={user.date_format} />
      </Card>
      <Card title="Change email">
        <ChangeEmail current={user.email} />
      </Card>
      <Card title="Change password">
        <ChangePassword />
      </Card>
    </div>
  );
}

function DateFormat({ current }: { current: string }) {
  const [value, setValue] = useState(current);
  const [error, setError] = useState<string>();
  const router = useRouter();
  return (
    <div className="space-y-2">
      <select
        className="rounded border border-slate-300 px-2 py-2 text-sm"
        value={value}
        onChange={async (e) => {
          const next = e.target.value;
          setValue(next);
          setError(undefined);
          try {
            await api("/settings", { body: { date_format: next } });
            // The format comes from the session; refresh it so the whole
            // app picks the new format up.
            await router.invalidate();
          } catch (err) {
            setError(err instanceof Error ? err.message : "Failed");
          }
        }}
      >
        {DATE_FORMATS.map((f) => (
          <option key={f.value} value={f.value}>
            {f.label}
          </option>
        ))}
      </select>
      <p className="text-xs text-slate-500">
        Dates across the app, for example {formatDate(new Date().toISOString(), value)}.
      </p>
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

function ChangeEmail({ current }: { current: string }) {
  const [email, setEmail] = useState(current);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const router = useRouter();
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        setSaved(false);
        try {
          await api("/email", { body: { email } });
          setSaved(true);
          // The nav bar shows the email from the session; refresh it.
          await router.invalidate();
        } catch (err) {
          setError(err instanceof Error ? err.message : "Something went wrong");
        }
      }}
    >
      <Input
        type="email"
        placeholder="you@example.com"
        required
        value={email}
        onChange={(e) => setEmail(e.target.value)}
      />
      <ErrorText>{error}</ErrorText>
      {saved && <p className="text-sm text-emerald-600">Email updated.</p>}
      <Button type="submit">Update email</Button>
    </form>
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
