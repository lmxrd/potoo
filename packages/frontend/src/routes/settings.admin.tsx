import { createFileRoute, Outlet, redirect } from "@tanstack/react-router";

/** Everything under here is for admins; anyone else is sent back to their account. */
export const Route = createFileRoute("/settings/admin")({
  beforeLoad: ({ context }) => {
    if (!context.session.user?.is_admin) throw redirect({ to: "/settings/account", replace: true });
  },
  component: () => <Outlet />,
});
