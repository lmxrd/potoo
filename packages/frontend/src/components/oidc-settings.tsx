import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useRouter } from "@tanstack/react-router";
import type { OidcSettings, UpdateOidcSettings } from "potoo-sdk";
import {
  getOidcSettingsOptions,
  getOidcSettingsQueryKey,
  updateOidcSettingsMutation,
} from "potoo-sdk/react-query";
import { errorMessage } from "../lib/api";
import { Button } from "./ui/button";
import { ErrorText } from "./error-text";
import { Input } from "./ui/input";
import { Checkbox } from "./ui/checkbox";

type TextField = "issuer" | "client_id" | "scopes" | "label" | "redirect_url";

/**
 * The single sign-on settings of the admin settings. A field set by an
 * OIDC_* environment variable shows, but cannot be changed here. The
 * client secret is never shown: it is replaced when one is typed.
 */
export function OidcSettingsForm() {
  const { data, error } = useQuery(getOidcSettingsOptions());
  // Above the form, which starts over from what the server saved (trimmed
  // values, a secret that is set or not): "Saved." outlives that. Before any
  // return: hooks run in the same order on every render.
  const [saved, setSaved] = useState(false);
  if (error) return <ErrorText>{errorMessage(error)}</ErrorText>;
  if (!data) return null;
  return <Form key={JSON.stringify(data)} settings={data} saved={saved} setSaved={setSaved} />;
}

function Form({
  settings,
  saved,
  setSaved,
}: {
  settings: OidcSettings;
  saved: boolean;
  setSaved: (saved: boolean) => void;
}) {
  const queryClient = useQueryClient();
  const router = useRouter();
  const update = useMutation({ ...updateOidcSettingsMutation(), meta: { inline: true } });
  const [values, setValues] = useState<Record<TextField, string>>({
    issuer: settings.issuer,
    client_id: settings.client_id,
    scopes: settings.scopes,
    label: settings.label,
    redirect_url: settings.redirect_url,
  });
  const [secret, setSecret] = useState("");
  const [removeSecret, setRemoveSecret] = useState(false);
  const [autoCreate, setAutoCreate] = useState(settings.auto_create);
  const [error, setError] = useState<string>();
  const locked = (field: string) => settings.locked.includes(field);
  const lockedNote = (field: string) =>
    locked(field) ? `Set by OIDC_${field.toUpperCase()}` : undefined;

  const field = (name: TextField, label: string, placeholder: string, hint?: string) => (
    <label className="flex flex-col gap-1 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <Input
        value={values[name]}
        placeholder={placeholder}
        disabled={locked(name)}
        onChange={(e) => setValues({ ...values, [name]: e.target.value })}
      />
      {(lockedNote(name) ?? hint) && (
        <span className="text-xs text-muted-foreground">{lockedNote(name) ?? hint}</span>
      )}
    </label>
  );

  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        setSaved(false);
        // Only what may change is sent; an empty secret field keeps the
        // secret, unless it is to be removed.
        const body: UpdateOidcSettings = {};
        for (const name of Object.keys(values) as TextField[]) {
          if (!locked(name)) body[name] = values[name];
        }
        if (!locked("auto_create")) body.auto_create = autoCreate;
        if (!locked("client_secret")) {
          if (removeSecret) body.client_secret = "";
          else if (secret) body.client_secret = secret;
        }
        try {
          await update.mutateAsync({ body });
          // Sent: the form may not start over (the saved settings can look
          // the same, e.g. a secret replaced by another), so clear it here.
          setSecret("");
          setRemoveSecret(false);
          setSaved(true);
          await queryClient.invalidateQueries({ queryKey: getOidcSettingsQueryKey() });
          // The login page's button comes from the session.
          await router.invalidate();
        } catch (err) {
          setError(errorMessage(err, "Failed"));
        }
      }}
    >
      <p className="text-sm text-soft-foreground">
        {settings.enabled
          ? "On: the login page offers to sign in with the provider."
          : "Off. Set an issuer and a client id to turn it on."}
      </p>
      {field("issuer", "Issuer URL", "https://id.example.com/realms/main")}
      {field("client_id", "Client id", "potoo")}
      <label className="flex flex-col gap-1 text-sm">
        <span className="text-muted-foreground">Client secret</span>
        <Input
          type="password"
          autoComplete="off"
          value={secret}
          placeholder={settings.client_secret_set ? "Set; type to replace it" : "None"}
          disabled={locked("client_secret") || removeSecret}
          onChange={(e) => setSecret(e.target.value)}
        />
        <span className="text-xs text-muted-foreground">
          {lockedNote("client_secret") ??
            "A new issuer removes the secret, unless you enter one with it."}
        </span>
      </label>
      {settings.client_secret_set && !locked("client_secret") && (
        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={removeSecret}
            onCheckedChange={(checked) => setRemoveSecret(checked === true)}
          />
          Remove the secret (a public client, with PKCE only)
        </label>
      )}
      {field("scopes", "Scopes", "openid email")}
      {field("label", "Button label", "Single sign-on", "The login button says “Sign in with …”.")}
      {field(
        "redirect_url",
        "Callback URL",
        settings.callback_url,
        "Leave empty to use the address Potoo is reached at.",
      )}
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          checked={autoCreate}
          disabled={locked("auto_create")}
          onCheckedChange={(checked) => setAutoCreate(checked === true)}
        />
        Create a regular user for a verified email that has no account
      </label>
      {lockedNote("auto_create") && (
        <span className="text-xs text-muted-foreground">{lockedNote("auto_create")}</span>
      )}
      <div className="rounded border border-border bg-muted p-3 text-xs">
        <p className="text-muted-foreground">Register this callback URL with the provider:</p>
        <code className="break-all">{settings.callback_url}</code>
      </div>
      <p className="text-xs text-muted-foreground">
        An existing user is matched by the verified email the provider gives, then stays linked to
        that provider account.
      </p>
      <ErrorText>{error}</ErrorText>
      {saved && <p className="text-sm text-success">Saved.</p>}
      <div>
        <Button type="submit" disabled={update.isPending}>
          Save
        </Button>
      </div>
    </form>
  );
}
