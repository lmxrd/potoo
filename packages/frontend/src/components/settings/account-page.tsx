import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { useRouteContext, useRouter } from "@tanstack/react-router";
import { changePasswordMutation, updateMeMutation } from "potoo-sdk/react-query";
import { Button } from "../ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { Checkbox } from "../ui/checkbox";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { ErrorText } from "../common/error-text";
import { errorMessage } from "../../lib/api";
import { DATE_FORMATS, formatDate, formatTime } from "../../lib/dates";
import { THEMES, applyTheme, parseTheme, sameTheme, themeJson, type Theme } from "../../lib/themes";

/** The signed-in user and the auth status, from the root route. */
function useSession() {
  const { session } = useRouteContext({ from: "__root__" });
  return { session, user: session.user! };
}

/** The account (email and role), its email, and how dates and times show. */
export function AccountSection() {
  const { user } = useSession();
  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>Account</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="space-y-2 text-sm">
            <div className="flex justify-between gap-4">
              <dt className="text-muted-foreground">Email</dt>
              <dd>{user.email}</dd>
            </div>
            <div className="flex justify-between gap-4">
              <dt className="text-muted-foreground">Role</dt>
              <dd>{user.is_admin ? "Admin" : "User"}</dd>
            </div>
          </dl>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Date format</CardTitle>
        </CardHeader>
        <CardContent>
          <DateFormat current={user.date_format} />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Time format</CardTitle>
        </CardHeader>
        <CardContent>
          <TimeFormat current={user.use_24} />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Change email</CardTitle>
        </CardHeader>
        <CardContent>
          <ChangeEmail current={user.email} />
        </CardContent>
      </Card>
    </>
  );
}

/** The theme of the whole app, for the signed-in user. */
export function AppearanceSettings() {
  const { user } = useSession();
  return (
    <Card>
      <CardHeader>
        <CardTitle>Theme</CardTitle>
      </CardHeader>
      <CardContent>
        <ThemePicker current={parseTheme(user.theme)} />
      </CardContent>
    </Card>
  );
}

/** The password: change it, or set one after signing in with single sign-on only. */
export function PasswordSection() {
  const { user } = useSession();
  return (
    <Card>
      <CardHeader>
        <CardTitle>{user.has_password ? "Change password" : "Set a password"}</CardTitle>
      </CardHeader>
      <CardContent>
        <ChangePassword hasPassword={user.has_password} />
      </CardContent>
    </Card>
  );
}

function DateFormat({ current }: { current: string }) {
  const [value, setValue] = useState(current);
  // The example date, taken once (a render must not read the clock).
  const [today] = useState(() => new Date().toISOString());
  const [error, setError] = useState<string>();
  const router = useRouter();
  const change = useMutation({ ...updateMeMutation(), meta: { inline: true } });
  return (
    <div className="space-y-2">
      <Select
        value={value}
        items={DATE_FORMATS}
        onValueChange={async (next) => {
          if (next === null) return;
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
        <SelectTrigger aria-label="Date format">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {DATE_FORMATS.map((f) => (
            <SelectItem key={f.value} value={f.value}>
              {f.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <p className="text-xs text-muted-foreground">
        Dates across the app, for example {formatDate(today, value)}.
      </p>
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

function TimeFormat({ current }: { current: boolean }) {
  const [value, setValue] = useState(current);
  const [error, setError] = useState<string>();
  const router = useRouter();
  const change = useMutation({ ...updateMeMutation(), meta: { inline: true } });
  return (
    <div className="space-y-2">
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          checked={value}
          onCheckedChange={async (checked) => {
            const next = checked === true;
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
      <p className="text-xs text-muted-foreground">
        Times across the app, for example {formatTime("14:30", value)}.
      </p>
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

/**
 * The theme: the presets as swatches, and the two knobs (the base and
 * the accent) for a custom one. Every choice previews live, and reverts
 * when saving fails.
 */
const BASES: { value: Theme["base"]; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

function ThemePicker({ current }: { current: Theme }) {
  const [error, setError] = useState<string>();
  const router = useRouter();
  const change = useMutation({ ...updateMeMutation(), meta: { inline: true } });
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
              active?.label === preset.label ? "border-primary" : "border-border hover:bg-muted"
            }`}
            onClick={() => save(preset.theme)}
          >
            <span
              className="flex h-10 items-end overflow-hidden rounded border border-border p-1"
              style={{ background: preset.theme.base === "dark" ? "#0f172a" : "#f8fafc" }}
            >
              <span
                className="h-2.5 w-full rounded-sm"
                style={{ background: preset.theme.accent }}
              />
            </span>
            <span className="mt-1 block text-xs text-soft-foreground">{preset.label}</span>
          </button>
        ))}
      </div>
      <div className="flex items-center justify-between gap-4 border-t border-border pt-3 text-sm">
        <Label htmlFor="theme-base" className="font-normal text-muted-foreground">
          Base
        </Label>
        <Select
          value={current.base}
          items={BASES}
          onValueChange={(base) => base && save({ ...current, base })}
        >
          <SelectTrigger id="theme-base" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {BASES.map((b) => (
              <SelectItem key={b.value} value={b.value}>
                {b.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <label htmlFor="theme-accent" className="text-muted-foreground">
          Accent
        </label>
        <input
          id="theme-accent"
          type="color"
          aria-label="Accent color"
          className="h-9 w-14 cursor-pointer rounded border border-input bg-card p-0.5"
          value={current.accent}
          // input previews live while dragging; change saves.
          onInput={(e) => applyTheme({ ...current, accent: e.currentTarget.value })}
          onChange={(e) => save({ ...current, accent: e.target.value })}
        />
      </div>
      <p className="text-xs text-muted-foreground">Applies to the whole app, for your account.</p>
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

function ChangeEmail({ current }: { current: string }) {
  const [email, setEmail] = useState(current);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const router = useRouter();
  const changeEmail = useMutation({ ...updateMeMutation(), meta: { inline: true } });
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
      {saved && <p className="text-sm text-success">Email updated.</p>}
      <Button type="submit">Update email</Button>
    </form>
  );
}

/**
 * A user who signs in through single sign-on only has no password: they
 * can set one, without a current one, to sign in with it too.
 */
function ChangePassword({ hasPassword }: { hasPassword: boolean }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const router = useRouter();
  const changePassword = useMutation({ ...changePasswordMutation(), meta: { inline: true } });
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        setSaved(false);
        try {
          await changePassword.mutateAsync({
            body: hasPassword
              ? { current_password: current, new_password: next }
              : { new_password: next },
          });
          setCurrent("");
          setNext("");
          setSaved(true);
          // Whether there is a password comes from the session.
          if (!hasPassword) await router.invalidate();
        } catch (err) {
          setError(errorMessage(err));
        }
      }}
    >
      {hasPassword ? (
        <Input
          type="password"
          placeholder="Current password"
          required
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
        />
      ) : (
        <p className="text-xs text-muted-foreground">
          You sign in with single sign-on. A password lets you sign in without it too.
        </p>
      )}
      <Input
        type="password"
        placeholder="New password (min. 10 characters)"
        required
        minLength={10}
        value={next}
        onChange={(e) => setNext(e.target.value)}
      />
      <ErrorText>{error}</ErrorText>
      {saved && <p className="text-sm text-success">Password updated.</p>}
      <Button type="submit">{hasPassword ? "Update password" : "Set password"}</Button>
    </form>
  );
}
