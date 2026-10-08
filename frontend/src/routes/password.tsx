import { useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { api } from "../api";
import { Button, Card, ErrorText, Input } from "../components";

export const Route = createFileRoute("/password")({
  component: ChangePassword,
});

function ChangePassword() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  return (
    <div className="max-w-sm space-y-6">
      <h1 className="text-xl font-semibold">Change password</h1>
      <Card>
        <form
          className="flex flex-col gap-3"
          onSubmit={async (e) => {
            e.preventDefault();
            setError(undefined);
            setSaved(false);
            try {
              await api("/password", {
                body: { current_password: current, new_password: next },
              });
              setCurrent("");
              setNext("");
              setSaved(true);
            } catch (err) {
              setError(err instanceof Error ? err.message : "Something went wrong");
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
          {saved && <p className="text-sm text-emerald-600">Password updated.</p>}
          <Button type="submit">Update password</Button>
        </form>
      </Card>
    </div>
  );
}
