import { createFileRoute } from "@tanstack/react-router";

import { SsoTab } from "../components/admin/sso-tab";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";

export const Route = createFileRoute("/settings/admin/sso")({
  component: () => (
    <Card>
      <CardHeader>
        <CardTitle>Single sign-on</CardTitle>
      </CardHeader>
      <CardContent>
        <SsoTab />
      </CardContent>
    </Card>
  ),
});
