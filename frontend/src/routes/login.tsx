import { createFileRoute } from "@tanstack/react-router";
import { CredentialsForm } from "./-credentials-form";

export const Route = createFileRoute("/login")({ component: Login });

function Login() {
  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">Sign in</h1>
      <CredentialsForm endpoint="/login" submit="Sign in" onDone={() => location.assign("/")} />
    </div>
  );
}
