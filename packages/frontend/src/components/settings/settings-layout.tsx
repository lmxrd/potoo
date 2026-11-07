import { Link, Outlet, useRouteContext } from "@tanstack/react-router";

const link =
  "whitespace-nowrap rounded px-3 py-1.5 text-sm text-soft-foreground hover:text-foreground md:block";
const active = "bg-muted font-medium text-foreground";

/**
 * The settings shell: a side menu (a row of links on phones) with the user's
 * own pages and, for an admin, the admin pages. The selected page fills the
 * rest.
 */
export function SettingsLayout() {
  const { session } = useRouteContext({ from: "__root__" });
  const user = session.user;
  return (
    <div className="space-y-6 md:grid md:grid-cols-[200px_minmax(0,1fr)] md:gap-8 md:space-y-0">
      <aside className="space-y-3">
        <h1 className="text-xl font-semibold">Settings</h1>
        <nav aria-label="Settings" className="flex gap-1 overflow-x-auto md:flex-col md:gap-0.5">
          <Link to="/settings/account" className={link} activeProps={{ className: active }}>
            Account
          </Link>
          <Link to="/settings/appearance" className={link} activeProps={{ className: active }}>
            Appearance
          </Link>
          <Link to="/settings/password" className={link} activeProps={{ className: active }}>
            {/* No passwords with single sign-on only; two-factor is there either way. */}
            {session.password_login ? "Password & two-factor" : "Two-factor"}
          </Link>
          {user?.is_admin && (
            <>
              <p className="px-3 pt-3 text-xs text-muted-foreground max-md:hidden">Admin</p>
              <Link to="/settings/admin/users" className={link} activeProps={{ className: active }}>
                Users
              </Link>
              <Link to="/settings/admin/sso" className={link} activeProps={{ className: active }}>
                Single sign-on
              </Link>
            </>
          )}
        </nav>
      </aside>
      <div className="min-w-0">
        <Outlet />
      </div>
    </div>
  );
}
