import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { createFileRoute, useRouter } from "@tanstack/react-router";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { Input } from "../components/input";
import {
  changeEmailMutation,
  changePasswordMutation,
  changeSettingsMutation,
} from "../client/@tanstack/react-query.gen";
import { errorMessage } from "../lib/api";
import { DATE_FORMATS, formatDate, formatTime } from "../lib/dates";
import { THEMES, applyTheme, parseTheme, themeJson, type Theme } from "../lib/theme";

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
            <dt className="text-muted">Email</dt>
            <dd>{user.email}</dd>
          </div>
          <div className="flex justify-between gap-4">
            <dt className="text-muted">Role</dt>
            <dd>{user.is_admin ? "Admin" : "User"}</dd>
          </div>
        </dl>
      </Card>
      <Card title="Date format">
        <DateFormat current={user.date_format} />
      </Card>
      <Card title="Time format">
        <TimeFormat current={user.use_24} />
      </Card>
      <Card title="Theme">
        <ThemePicker current={parseTheme(user.theme)} />
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
  const change = useMutation(changeSettingsMutation());
  return (
    <div className="space-y-2">
      <select
        className="rounded border border-line-strong px-2 py-2 text-sm"
        value={value}
        onChange={async (e) => {
          const next = e.target.value;
          setValue(next);
          setError(undefined);
          try {
            await change.mutateAsync({ body: { date_format: next } });
            // The format comes from the session; refresh it so the whole
            // app picks the new format up.
            await router.invalidate();
          } catch (err) {
            setError(errorMessage(err, "Failed"));
          }
        }}
      >
        {DATE_FORMATS.map((f) => (
          <option key={f.value} value={f.value}>
            {f.label}
          </option>
        ))}
      </select>
      <p className="text-xs text-muted">
        Dates across the app, for example {formatDate(new Date().toISOString(), value)}.
      </p>
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

function TimeFormat({ current }: { current: boolean }) {
  const [value, setValue] = useState(current);
  const [error, setError] = useState<string>();
  const router = useRouter();
  const change = useMutation(changeSettingsMutation());
  return (
    <div className="space-y-2">
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={value}
          onChange={async (e) => {
            const next = e.target.checked;
            setValue(next);
            setError(undefined);
            try {
              await change.mutateAsync({ body: { use_24: next } });
              await router.invalidate();
            } catch (err) {
              setError(errorMessage(err, "Failed"));
            }
          }}
        />
        24-hour time
      </label>
      <p className="text-xs text-muted">
        Times across the app, for example {formatTime("14:30", value)}.
      </p>
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

function sameTheme(a: Theme, b: Theme): boolean {
  return a.base === b.base && a.accent.toLowerCase() === b.accent.toLowerCase();
}

/**
 * The theme: the presets as swatches, and the two knobs (the base and
 * the accent) for a custom one. Every choice previews live, and reverts
 * when saving fails.
 */
function ThemePicker({ current }: { current: Theme }) {
  const [error, setError] = useState<string>();
  const router = useRouter();
  const change = useMutation(changeSettingsMutation());
  const active = THEMES.find((preset) => sameTheme(preset.theme, current));

  const save = async (next: Theme) => {
    applyTheme(next);
    setError(undefined);
    try {
      await change.mutateAsync({ body: { theme: themeJson(next) } });
      // The theme also lives in the session; refresh so navigation
      // cannot reapply a stale one.
      await router.invalidate();
    } catch (err) {
      applyTheme(current);
      setError(errorMessage(err, "Failed"));
    }
  };

  return (
    <div className="space-y-2">
      <div className="grid grid-cols-3 gap-2" role="group" aria-label="Theme presets">
        {THEMES.map((preset) => (
          <button
            key={preset.label}
            aria-pressed={active?.label === preset.label}
            className={`rounded-lg border p-1.5 text-center ${
              active?.label === preset.label ? "border-accent" : "border-line hover:bg-subtle"
            }`}
            onClick={() => save(preset.theme)}
          >
            <span
              className="flex h-10 items-end overflow-hidden rounded border border-line p-1"
              style={{ background: preset.theme.base === "dark" ? "#0f172a" : "#f8fafc" }}
            >
              <span
                className="h-2.5 w-full rounded-sm"
                style={{ background: preset.theme.accent }}
              />
            </span>
            <span className="mt-1 block text-xs text-soft-ink">{preset.label}</span>
          </button>
        ))}
      </div>
      <div className="flex items-center justify-between gap-4 border-t border-line pt-3 text-sm">
        <label htmlFor="theme-base" className="text-muted">
          Base
        </label>
        <select
          id="theme-base"
          className="rounded border border-line-strong bg-card px-2 py-1.5"
          value={current.base}
          onChange={(e) => save({ ...current, base: e.target.value as Theme["base"] })}
        >
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
        <label htmlFor="theme-accent" className="text-muted">
          Accent
        </label>
        <input
          id="theme-accent"
          type="color"
          aria-label="Accent color"
          className="h-9 w-14 cursor-pointer rounded border border-line-strong bg-card p-0.5"
          value={current.accent}
          // input previews live while dragging; change saves.
          onInput={(e) => applyTheme({ ...current, accent: e.currentTarget.value })}
          onChange={(e) => save({ ...current, accent: e.target.value })}
        />
      </div>
      <p className="text-xs text-muted">Applies to the whole app, for your account.</p>
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

function ChangeEmail({ current }: { current: string }) {
  const [email, setEmail] = useState(current);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const router = useRouter();
  const changeEmail = useMutation(changeEmailMutation());
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        setSaved(false);
        try {
          await changeEmail.mutateAsync({ body: { email } });
          setSaved(true);
          // The nav bar shows the email from the session; refresh it.
          await router.invalidate();
        } catch (err) {
          setError(errorMessage(err));
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
      {saved && <p className="text-sm text-ok">Email updated.</p>}
      <Button type="submit">Update email</Button>
    </form>
  );
}

function ChangePassword() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const changePassword = useMutation(changePasswordMutation());
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        setSaved(false);
        try {
          await changePassword.mutateAsync({
            body: { current_password: current, new_password: next },
          });
          setCurrent("");
          setNext("");
          setSaved(true);
        } catch (err) {
          setError(errorMessage(err));
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
      {saved && <p className="text-sm text-ok">Password updated.</p>}
      <Button type="submit">Update password</Button>
    </form>
  );
}
