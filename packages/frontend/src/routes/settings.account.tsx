import { createFileRoute } from "@tanstack/react-router";

import { AccountSection } from "../components/settings/account-page";
import { ApiKeysSection } from "../components/settings/api-keys-section";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";

export const Route = createFileRoute("/settings/account")({
  component: Account,
});

function Account() {
  const { session } = Route.useRouteContext();
  return (
    <div className="max-w-sm space-y-6">
      <AccountSection />
      <Card>
        <CardHeader>
          <CardTitle>API keys</CardTitle>
        </CardHeader>
        <CardContent>
          <ApiKeysSection isAdmin={session.user!.is_admin} />
        </CardContent>
      </Card>
    </div>
  );
}
