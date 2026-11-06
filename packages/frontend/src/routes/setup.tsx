import { createFileRoute } from "@tanstack/react-router";
import { CredentialsForm } from "../components/credentials-form";

export const Route = createFileRoute("/setup")({ component: Setup });

function Setup() {
  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">Create the admin account</h1>
      <p className="text-sm text-soft-ink">
        This is the first start. The admin can add sites and create all other users.
      </p>
      <CredentialsForm kind="setup" submit="Create admin" onDone={() => location.assign("/")} />
    </div>
  );
}
