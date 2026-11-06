import { useMutation } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { QRCodeSVG } from "qrcode.react";
import { useState } from "react";
import { Button } from "./ui/button";
import { ErrorText } from "./error-text";
import { Input } from "./ui/input";
import {
  confirmReplaceTotpMutation,
  disableTotpMutation,
  replaceTotpMutation,
  enableTotpMutation,
  regenerateRecoveryCodesMutation,
  setupTotpMutation,
} from "potoo-sdk/react-query";
import type { TotpSetup } from "potoo-sdk/types";
import { errorMessage } from "../lib/api";

/**
 * Two-factor authentication of the signed-in user: set up an authenticator
 * app (a QR code, or the secret to type), confirm it with a code, and keep
 * the recovery codes; later, new recovery codes, a new app (the old one works
 * until the new one is confirmed) or turning it off, each with a code.
 */
export function TotpSettings({ enabled, required }: { enabled: boolean; required: boolean }) {
  const router = useRouter();
  const [setup, setSetup] = useState<TotpSetup>();
  const [codes, setCodes] = useState<string[]>();
  const [code, setCode] = useState("");
  const [error, setError] = useState<string>();
  const start = useMutation({ ...setupTotpMutation(), meta: { inline: true } });
  const enable = useMutation({ ...enableTotpMutation(), meta: { inline: true } });
  const disable = useMutation({ ...disableTotpMutation(), meta: { inline: true } });
  const regenerate = useMutation({ ...regenerateRecoveryCodesMutation(), meta: { inline: true } });
  const replace = useMutation({ ...replaceTotpMutation(), meta: { inline: true } });
  const confirmReplace = useMutation({ ...confirmReplaceTotpMutation(), meta: { inline: true } });
  const [replacement, setReplacement] = useState<TotpSetup>();
  const [replaced, setReplaced] = useState(false);

  const run = async (action: () => Promise<unknown>) => {
    setError(undefined);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  if (codes) return <RecoveryCodeList codes={codes} onDone={() => setCodes(undefined)} />;

  const codeInput = (
    <Input
      required
      autoComplete="one-time-code"
      placeholder="Code from the app"
      value={code}
      onChange={(e) => setCode(e.target.value)}
    />
  );

  // Replacing the app: the new secret, until a code from the new app
  // confirms it. The old app keeps working until then.
  if (enabled && replacement) {
    return (
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            await confirmReplace.mutateAsync({ body: { code } });
            setReplacement(undefined);
            setCode("");
            setReplaced(true);
          });
        }}
      >
        <QrCode setup={replacement} />
        {codeInput}
        <ErrorText>{error}</ErrorText>
        <div className="flex gap-2">
          <Button type="submit">Replace</Button>
          <Button
            type="button"
            onClick={() => {
              setReplacement(undefined);
              setCode("");
            }}
          >
            Cancel
          </Button>
        </div>
      </form>
    );
  }

  if (enabled) {
    return (
      <form className="flex flex-col gap-3" onSubmit={(e) => e.preventDefault()}>
        <p className="text-sm text-success">
          On: a code from your authenticator app is asked for at sign-in.
        </p>
        {replaced && <p className="text-sm text-success">Authenticator app replaced.</p>}
        <p className="text-xs text-muted-foreground">
          {required
            ? "A code from the app, or a recovery code, to make new recovery codes or to move to a new app. It is required here: it cannot be turned off."
            : "A code from the app, or a recovery code, to make new recovery codes, to move to a new app, or to turn it off."}
        </p>
        {codeInput}
        <ErrorText>{error}</ErrorText>
        <div className="flex flex-wrap gap-2">
          <Button
            type="submit"
            onClick={() =>
              code &&
              run(async () => {
                const next = await replace.mutateAsync({ body: { code } });
                setCode("");
                setReplaced(false);
                setReplacement(next);
              })
            }
          >
            Replace app
          </Button>
          <Button
            type="submit"
            onClick={() =>
              code &&
              run(async () => {
                const result = await regenerate.mutateAsync({ body: { code } });
                setCode("");
                setCodes(result.codes);
              })
            }
          >
            New recovery codes
          </Button>
          {!required && (
            <Button
              type="submit"
              variant="destructive"
              onClick={() =>
                code &&
                run(async () => {
                  await disable.mutateAsync({ body: { code } });
                  setCode("");
                  await router.invalidate();
                })
              }
            >
              Turn off
            </Button>
          )}
        </div>
      </form>
    );
  }

  if (setup) {
    return (
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            const result = await enable.mutateAsync({ body: { code } });
            setSetup(undefined);
            setCode("");
            setCodes(result.codes);
            await router.invalidate();
          });
        }}
      >
        <QrCode setup={setup} />
        {codeInput}
        <ErrorText>{error}</ErrorText>
        <div className="flex gap-2">
          <Button type="submit">Turn on</Button>
          <Button type="button" onClick={() => setSetup(undefined)}>
            Cancel
          </Button>
        </div>
      </form>
    );
  }

  return (
    <div className="space-y-3">
      <p className="text-sm text-muted-foreground">
        Off. With it on, signing in also asks for a code from an authenticator app.
      </p>
      <ErrorText>{error}</ErrorText>
      <Button onClick={() => run(async () => setSetup(await start.mutateAsync({})))}>Set up</Button>
    </div>
  );
}

/** The QR code of a new secret, and the key to type instead. */
export function QrCode({ setup }: { setup: TotpSetup }) {
  return (
    <>
      <p className="text-sm">
        Scan this with your authenticator app, then enter the code it shows.
      </p>
      {/* Dark on white whatever the theme: what the scanners read best. */}
      <div className="self-start rounded bg-white p-3">
        <QRCodeSVG value={setup.uri} size={176} />
      </div>
      <p className="text-xs text-muted-foreground">
        Or type this key:{" "}
        <code className="break-all font-mono text-soft-foreground">{setup.secret}</code>
      </p>
    </>
  );
}

/** New recovery codes, shown once. */
export function RecoveryCodeList({
  codes,
  onDone,
  done = "Done",
}: {
  codes: string[];
  onDone: () => void;
  done?: string;
}) {
  return (
    <div className="space-y-3">
      <p className="text-sm">
        Your recovery codes. Each one signs you in once without the app. Keep them somewhere safe:
        they are not shown again.
      </p>
      <ul className="grid grid-cols-2 gap-1 rounded border border-border bg-muted p-3 font-mono text-sm">
        {codes.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ul>
      <div className="flex gap-2">
        <Button onClick={() => navigator.clipboard.writeText(codes.join("\n"))}>Copy</Button>
        <Button onClick={onDone}>{done}</Button>
      </div>
    </div>
  );
}
