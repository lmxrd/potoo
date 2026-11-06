import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../ui/button";
import { ErrorText } from "../common/error-text";
import { Input } from "../ui/input";
import { loginTotpMutation } from "potoo-sdk/react-query";
import { errorMessage } from "../../lib/api";

/**
 * The second step of a password sign-in, for a user with two-factor
 * authentication on: a code from the authenticator app, or a recovery code.
 */
export function TotpForm({ onDone, onRestart }: { onDone: () => void; onRestart: () => void }) {
  const [code, setCode] = useState("");
  const [recovery, setRecovery] = useState(false);
  const [error, setError] = useState<string>();
  const login = useMutation({ ...loginTotpMutation(), meta: { inline: true } });
  return (
    <form
      className="flex max-w-sm flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        try {
          await login.mutateAsync({ body: { code } });
          onDone();
        } catch (err) {
          setError(errorMessage(err));
        }
      }}
    >
      <p className="text-sm text-muted-foreground">
        {recovery
          ? "Enter one of your recovery codes. Each one works once."
          : "Enter the code from your authenticator app."}
      </p>
      <Input
        // One field for both: the server tells a code from a recovery code.
        key={recovery ? "recovery" : "code"}
        autoFocus
        required
        autoComplete="one-time-code"
        inputMode={recovery ? "text" : "numeric"}
        placeholder={recovery ? "xxxxx-xxxxx" : "123456"}
        value={code}
        onChange={(e) => setCode(e.target.value)}
      />
      <ErrorText>{error}</ErrorText>
      <Button type="submit" disabled={login.isPending}>
        Verify
      </Button>
      <div className="flex justify-between text-xs">
        <Button
          type="button"
          variant="link"
          size="xs"
          className="px-0 text-muted-foreground"
          onClick={() => {
            setRecovery(!recovery);
            setCode("");
            setError(undefined);
          }}
        >
          {recovery ? "Use a code from the app" : "Use a recovery code"}
        </Button>
        <Button
          type="button"
          variant="link"
          size="xs"
          className="px-0 text-muted-foreground"
          onClick={onRestart}
        >
          Start over
        </Button>
      </div>
    </form>
  );
}
