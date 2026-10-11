import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "./button";
import { ErrorText } from "./error-text";
import { Input } from "./input";
import { loginMutation, setupMutation } from "../client/@tanstack/react-query.gen";
import { errorMessage } from "../lib/api";

export function CredentialsForm({
  kind,
  submit,
  onDone,
}: {
  kind: "login" | "setup";
  submit: string;
  onDone: () => void;
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
        try {
          await (kind === "login" ? login : setup).mutateAsync({ body: { email, password } });
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
