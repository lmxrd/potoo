import { useState } from "react";
import { createRootRoute, Link, Outlet, redirect, useNavigate } from "@tanstack/react-router";
import { api, type Session } from "../api";
import { Button, ErrorText, Input } from "../components";

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
                <>
                  <Link
                    to="/users"
                    className="text-slate-600 [&.active]:font-medium [&.active]:text-slate-900"
                  >
                    Users
                  </Link>
                  <AddSite />
                </>
              )}
              <Link
                to="/password"
                className="text-slate-600 [&.active]:font-medium [&.active]:text-slate-900"
              >
                Password
              </Link>
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

/** The admin's "add site" button in the nav bar, with its popup form. */
function AddSite() {
  const [open, setOpen] = useState(false);
  const [hostname, setHostname] = useState("");
  const [error, setError] = useState<string>();
  const navigate = useNavigate();

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(undefined);
    try {
      const site = await api<{ id: number }>("/sites", { body: { hostname } });
      setOpen(false);
      setHostname("");
      // Go to the new site, where its tracking snippet is.
      await navigate({
        to: "/sites/$siteId",
        params: { siteId: String(site.id) },
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed");
    }
  };

  return (
    <div className="relative">
      <Button
        className="!px-2 !py-1 !text-xs"
        onClick={() => {
          setOpen((o) => !o);
          setError(undefined);
        }}
      >
        + Add site
      </Button>
      {open && (
        <>
          {/* Clicking anywhere else closes the popup. */}
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <form
            className="absolute right-0 top-full z-20 mt-2 w-72 space-y-2 rounded-lg border border-slate-200 bg-white p-4 shadow-lg"
            onSubmit={submit}
          >
            <Input
              autoFocus
              placeholder="blog.example.com"
              required
              value={hostname}
              onChange={(e) => setHostname(e.target.value)}
            />
            <ErrorText>{error}</ErrorText>
            <Button type="submit" className="w-full">
              Add site
            </Button>
            <p className="text-xs text-slate-500">Each subdomain is its own site.</p>
          </form>
        </>
      )}
    </div>
  );
}
