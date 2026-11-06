import { createFileRoute } from "@tanstack/react-router";

import { AppearanceSettings } from "../components/settings/account-page";

export const Route = createFileRoute("/settings/appearance")({
  component: () => (
    <div className="max-w-sm space-y-6">
      <AppearanceSettings />
    </div>
  ),
});
