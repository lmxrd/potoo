import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "./button";
import { ErrorText } from "./error-text";
import { Input } from "./input";
import { QrCode, RecoveryCodeList } from "./totp-settings";
import { loginTotpEnableMutation, loginTotpSetupMutation } from "potoo-sdk/react-query";
import type { TotpSetup } from "potoo-sdk/types";
import { errorMessage } from "../lib/api";

/**
 * Signing in where two-factor authentication is required (TOTP_REQUIRED)
 * by a user who has not set it up: the app first, then the session.
 */
export function TotpEnroll({ onDone, onRestart }: { onDone: () => void; onRestart: () => void }) {
  const [setup, setSetup] = useState<TotpSetup>();
  const [codes, setCodes] = useState<string[]>();
  const [code, setCode] = useState("");
  const [error, setError] = useState<string>();
  const start = useMutation(loginTotpSetupMutation());
  const enable = useMutation(loginTotpEnableMutation());

  if (codes) {
    return (
      <div className="max-w-sm">
        <RecoveryCodeList codes={codes} onDone={onDone} done="Continue" />
      </div>
    );
  }

  if (!setup) {
    return (
      <div className="flex max-w-sm flex-col gap-3">
        <p className="text-sm text-muted">
          Two-factor authentication is required here. Set up an authenticator app to continue.
        </p>
        <ErrorText>{error}</ErrorText>
        <Button
          onClick={async () => {
            setError(undefined);
            try {
              setSetup(await start.mutateAsync({}));
            } catch (err) {
              setError(errorMessage(err));
            }
          }}
        >
          Set up
        </Button>
        <button
          type="button"
          className="self-end text-xs text-muted underline hover:text-soft-ink"
          onClick={onRestart}
        >
          Start over
        </button>
      </div>
    );
  }

  return (
    <form
      className="flex max-w-sm flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        try {
          setCodes((await enable.mutateAsync({ body: { code } })).codes);
        } catch (err) {
          setError(errorMessage(err));
        }
      }}
    >
      <QrCode setup={setup} />
      <Input
        required
        autoFocus
        autoComplete="one-time-code"
        inputMode="numeric"
        placeholder="Code from the app"
        value={code}
        onChange={(e) => setCode(e.target.value)}
      />
      <ErrorText>{error}</ErrorText>
      <Button type="submit" disabled={enable.isPending}>
        Turn on and sign in
      </Button>
    </form>
  );
}
