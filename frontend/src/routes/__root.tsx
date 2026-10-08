import { createRootRoute, Link, Outlet, redirect, useNavigate } from "@tanstack/react-router";
import { api, type Session } from "../api";

export const Route = createRootRoute({
  // Every page goes through here: first-run → /setup, signed-out → /login.
  beforeLoad: async ({ location }) => {
    const session = await api<Session>("/session");
    const path = location.pathname;
    if (session.needsSetup && path !== "/setup") throw redirect({ to: "/setup" });
    if (!session.needsSetup && !session.user && path !== "/login") throw redirect({ to: "/login" });
    if (session.user && (path === "/login" || path === "/setup")) throw redirect({ to: "/" });
    return { session };
  },
  component: Root,
});

function Root() {
  const { session } = Route.useRouteContext();
  const navigate = useNavigate();
  const user = session.user;
  return (
    <div className="min-h-screen bg-slate-50 text-slate-900">
      <header className="border-b border-slate-200 bg-white">
        <nav className="mx-auto flex max-w-5xl items-center gap-6 px-4 py-3">
          <span className="font-semibold">Potoo</span>
          {user && (
            <>
              <Link
                to="/"
                className="text-slate-600 [&.active]:font-medium [&.active]:text-slate-900"
              >
                Sites
              </Link>
              {user.is_admin === 1 && (
                <Link
                  to="/users"
                  className="text-slate-600 [&.active]:font-medium [&.active]:text-slate-900"
                >
                  Users
                </Link>
              )}
              <span className="ml-auto text-sm text-slate-500">{user.email}</span>
              <button
                className="text-sm text-slate-600 hover:text-slate-900"
                onClick={async () => {
                  await api("/logout", { method: "POST", body: {} });
                  await navigate({ to: "/login", reloadDocument: true });
                }}
              >
                Sign out
              </button>
            </>
          )}
        </nav>
      </header>
      <main className="mx-auto max-w-5xl px-4 py-8">
        <Outlet />
      </main>
    </div>
  );
}
