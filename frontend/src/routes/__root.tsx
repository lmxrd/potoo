import { useState } from "react";
import { useMutation, type QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  Link,
  Outlet,
  redirect,
  useNavigate,
} from "@tanstack/react-router";
import type { User } from "../client";
import { getSessionOptions, logoutMutation } from "../client/@tanstack/react-query.gen";
import { applyTheme, parseTheme } from "../lib/theme";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  // Every page goes through here: first-run → /setup, signed-out → /login.
  beforeLoad: async ({ location, context }) => {
    // Always fresh: the session changes with settings and sign-ins.
    const session = await context.queryClient.fetchQuery(getSessionOptions());
    // The theme applies before anything renders, so there is no flash of
    // the default palette.
    applyTheme(parseTheme(session.user?.theme));
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
    <div className="min-h-screen bg-bg text-ink">
      <header className="border-b border-line bg-card">
        <nav className="mx-auto flex max-w-5xl items-center gap-6 px-4 py-3">
          <Link to="/" className="font-semibold hover:text-soft-ink">
            Potoo
          </Link>
          {user && (
            <>
              <Link
                to="/sites"
                className="text-soft-ink [&.active]:font-medium [&.active]:text-ink"
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
  const logout = useMutation(logoutMutation());
  const close = () => setOpen(false);
  const item = "block w-full px-4 py-2 text-left text-sm text-soft-ink hover:bg-subtle";
  return (
    <div className="relative ml-auto">
      <button
        className="flex items-center gap-1 text-sm text-soft-ink hover:text-ink"
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
          <div className="absolute right-0 top-full z-20 mt-2 w-48 overflow-hidden rounded-lg border border-line bg-card py-1 shadow-lg">
            <Link to="/settings" className={item} onClick={close}>
              Settings
            </Link>
            {user.is_admin && (
              <>
                <Link to="/users" className={item} onClick={close}>
                  Admin settings
                </Link>
                <Link to="/import" className={item} onClick={close}>
                  Import hits
                </Link>
              </>
            )}
            <hr className="my-1 border-line" />
            <button
              className={item}
              onClick={async () => {
                close();
                await logout.mutateAsync({});
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
