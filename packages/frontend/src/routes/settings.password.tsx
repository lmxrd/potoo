import { createFileRoute } from "@tanstack/react-router";

import { PasswordSection } from "../components/settings/account-page";
import { TwoFactorSection } from "../components/settings/two-factor-section";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";

export const Route = createFileRoute("/settings/password")({
  component: PasswordSettings,
});

/** The password (not with single sign-on only) and two-factor sign-in (always). */
function PasswordSettings() {
  const { session } = Route.useRouteContext();
  const user = session.user!;
  return (
    <div className="max-w-sm space-y-6">
      {session.password_login && <PasswordSection />}
      <Card>
        <CardHeader>
          <CardTitle>Two-factor authentication</CardTitle>
        </CardHeader>
        <CardContent>
          <TwoFactorSection enabled={user.totp_enabled} required={session.totp_required} />
        </CardContent>
      </Card>
    </div>
  );
}
