import { useState } from "react";
import { api } from "../api";
import { Button, ErrorText, Input } from "../components";

export function CredentialsForm({
  endpoint,
  submit,
  onDone,
}: {
  endpoint: string;
  submit: string;
  onDone: () => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string>();
  return (
    <form
      className="flex max-w-sm flex-col gap-3"
      onSubmit={async (e) => {
        e.preventDefault();
        setError(undefined);
        try {
          await api(endpoint, { body: { email, password } });
          onDone();
        } catch (err) {
          setError(err instanceof Error ? err.message : "Something went wrong");
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
