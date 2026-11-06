import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "./ui/button";
import { ErrorText } from "./error-text";
import { Input } from "./ui/input";
import { loginMutation, setupMutation } from "potoo-sdk/react-query";
import { errorMessage } from "../lib/api";

export function CredentialsForm({
  kind,
  submit,
  onSubmit,
  onDone,
  onTotp,
}: {
  kind: "login" | "setup";
  submit: string;
  onSubmit?: () => void;
  onDone: () => void;
  /** The password was right, and two-factor authentication is next. */
  onTotp?: (step: "totp" | "totp-setup") => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string>();
  const login = useMutation(loginMutation());
  const setup = useMutation(setupMutation());
  return (
    <form
      className="flex max-w-sm flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        onSubmit?.();
        try {
          if (kind === "login") {
            const result = await login.mutateAsync({ body: { email, password } });
            if (result?.totp_required) return onTotp?.("totp");
            if (result?.totp_setup_required) return onTotp?.("totp-setup");
          } else {
            await setup.mutateAsync({ body: { email, password } });
          }
          onDone();
        } catch (err) {
          setError(errorMessage(err));
        }
      }}
    >
      <Input
        type="email"
        placeholder="Email"
        required
        value={email}
        onChange={(e) => setEmail(e.target.value)}
      />
      <Input
        type="password"
        placeholder="Password (min. 10 characters)"
        required
        minLength={10}
        value={password}
        onChange={(e) => setPassword(e.target.value)}
      />
      <ErrorText>{error}</ErrorText>
      <Button type="submit">{submit}</Button>
    </form>
  );
}
