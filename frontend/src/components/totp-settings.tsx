import { useMutation } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { QRCodeSVG } from "qrcode.react";
import { useState } from "react";
import { Button } from "./button";
import { ErrorText } from "./error-text";
import { Input } from "./input";
import {
  disableTotpMutation,
  enableTotpMutation,
  regenerateRecoveryCodesMutation,
  setupTotpMutation,
} from "../client/@tanstack/react-query.gen";
import type { TotpSetup } from "../client/types.gen";
import { errorMessage } from "../lib/api";

/**
 * Two-factor authentication of the signed-in user: set up an authenticator
 * app (a QR code, or the secret to type), confirm it with a code, and keep
 * the recovery codes; later, new recovery codes or turning it off, each with
 * a code.
 */
export function TotpSettings({ enabled }: { enabled: boolean }) {
  const router = useRouter();
  const [setup, setSetup] = useState<TotpSetup>();
  const [codes, setCodes] = useState<string[]>();
  const [code, setCode] = useState("");
  const [error, setError] = useState<string>();
  const start = useMutation(setupTotpMutation());
  const enable = useMutation(enableTotpMutation());
  const disable = useMutation(disableTotpMutation());
  const regenerate = useMutation(regenerateRecoveryCodesMutation());

  const run = async (action: () => Promise<unknown>) => {
    setError(undefined);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  if (codes) {
    return (
      <div className="space-y-3">
        <p className="text-sm">
          Your recovery codes. Each one signs you in once without the app. Keep them somewhere safe:
          they are not shown again.
        </p>
        <ul className="grid grid-cols-2 gap-1 rounded border border-line bg-subtle p-3 font-mono text-sm">
          {codes.map((c) => (
            <li key={c}>{c}</li>
          ))}
        </ul>
        <div className="flex gap-2">
          <Button onClick={() => navigator.clipboard.writeText(codes.join("\n"))}>Copy</Button>
          <Button onClick={() => setCodes(undefined)}>Done</Button>
        </div>
      </div>
    );
  }

  const codeInput = (
    <Input
      required
      autoComplete="one-time-code"
      placeholder="Code from the app"
      value={code}
      onChange={(e) => setCode(e.target.value)}
    />
  );

  if (enabled) {
    return (
      <form className="flex flex-col gap-3" onSubmit={(e) => e.preventDefault()}>
        <p className="text-sm text-ok">
          On: a code from your authenticator app is asked for at sign-in.
        </p>
        <p className="text-xs text-muted">
          A code from the app, or a recovery code, to make new recovery codes or to turn it off.
        </p>
        {codeInput}
        <ErrorText>{error}</ErrorText>
        <div className="flex gap-2">
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
          <Button
            type="submit"
            variant="danger"
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
        <p className="text-sm">
          Scan this with your authenticator app, then enter the code it shows.
        </p>
        {/* Dark on white whatever the theme: what the scanners read best. */}
        <div className="self-start rounded bg-white p-3">
          <QRCodeSVG value={setup.uri} size={176} />
        </div>
        <p className="text-xs text-muted">
          Or type this key:{" "}
          <code className="break-all font-mono text-soft-ink">{setup.secret}</code>
        </p>
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
      <p className="text-sm text-muted">
        Off. With it on, signing in with your password also asks for a code from an authenticator
        app. (Single sign-on leaves this to your provider.)
      </p>
      <ErrorText>{error}</ErrorText>
      <Button onClick={() => run(async () => setSetup(await start.mutateAsync({})))}>Set up</Button>
    </div>
  );
}
