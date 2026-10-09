import { useState } from "react";
import { createRootRoute, Link, Outlet, redirect, useNavigate } from "@tanstack/react-router";
import { api, type Session, type User } from "../api";

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
  const user = session.user;
  return (
    <div className="min-h-screen bg-slate-50 text-slate-900">
      <header className="border-b border-slate-200 bg-white">
        <nav className="mx-auto flex max-w-5xl items-center gap-6 px-4 py-3">
          <Link to="/" className="font-semibold hover:text-slate-700">
            Potoo
          </Link>
          {user && (
            <>
              <Link
                to="/sites"
                className="text-slate-600 [&.active]:font-medium [&.active]:text-slate-900"
              >
                Sites
              </Link>
              <UserMenu user={user} />
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

/** The user's email, with its popover menu. */
function UserMenu({ user }: { user: User }) {
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const close = () => setOpen(false);
  const item = "block w-full px-4 py-2 text-left text-sm text-slate-700 hover:bg-slate-100";
  return (
    <div className="relative ml-auto">
      <button
        className="flex items-center gap-1 text-sm text-slate-600 hover:text-slate-900"
        onClick={() => setOpen((o) => !o)}
      >
        {user.email}
        <svg
          width="10"
          height="10"
          viewBox="0 0 10 10"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
        >
          <path d="M2 3.5 5 6.5 8 3.5" />
        </svg>
      </button>
      {open && (
        <>
          {/* Clicking anywhere else closes the popover. */}
          <div className="fixed inset-0 z-10" onClick={close} />
          <div className="absolute right-0 top-full z-20 mt-2 w-48 overflow-hidden rounded-lg border border-slate-200 bg-white py-1 shadow-lg">
            <Link to="/settings" className={item} onClick={close}>
              Settings
            </Link>
            {user.is_admin === 1 && (
              <Link to="/users" className={item} onClick={close}>
                Admin settings
              </Link>
            )}
            <hr className="my-1 border-slate-200" />
            <button
              className={item}
              onClick={async () => {
                close();
                await api("/logout", { method: "POST", body: {} });
                await navigate({ to: "/login", reloadDocument: true });
              }}
            >
              Sign out
            </button>
          </div>
        </>
      )}
    </div>
  );
}
