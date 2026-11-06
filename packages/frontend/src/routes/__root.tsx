import { useMutation } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  Link,
  Outlet,
  redirect,
  useNavigate,
} from "@tanstack/react-router";
import type { User } from "potoo-sdk";
import { getAuthStatusOptions, getMeOptions, logoutMutation } from "potoo-sdk/react-query";
import { ChevronDownIcon } from "lucide-react";
import { Button } from "../components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "../components/ui/dropdown-menu";
import { applyTheme, parseTheme } from "../lib/themes";
import type { RouterContext } from "../router";

export const Route = createRootRouteWithContext<RouterContext>()({
  // Every page goes through here: first-run → /setup, signed-out → /login.
  beforeLoad: async ({ location, context }) => {
    // Always fresh: both change with settings and sign-ins. The signed-in
    // user is null while nobody is (`/api/auth/me` answers 401).
    const status = await context.queryClient.fetchQuery(getAuthStatusOptions());
    const user = status.bootstrap_required
      ? null
      : await context.queryClient.fetchQuery(getMeOptions()).catch(() => null);
    const session = { ...status, user };
    // The theme applies before anything renders, so there is no flash of
    // the default palette.
    applyTheme(parseTheme(session.user?.theme));
    const path = location.pathname;
    if (session.bootstrap_required && path !== "/setup") throw redirect({ to: "/setup" });
    if (!session.bootstrap_required && !session.user && path !== "/login")
      throw redirect({ to: "/login" });
    if (session.user && (path === "/login" || path === "/setup")) throw redirect({ to: "/" });
    return { session };
  },
  component: Root,
});

function Root() {
  const { session } = Route.useRouteContext();
  const user = session.user;
  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="border-b border-border bg-card">
        <nav className="mx-auto flex max-w-5xl items-center gap-6 px-4 py-3">
          <Link to="/" className="font-semibold hover:text-soft-foreground">
            Potoo
          </Link>
          {user && (
            <>
              <Link
                to="/sites"
                className="text-soft-foreground [&.active]:font-medium [&.active]:text-foreground"
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

/** The user's email, with its menu. */
function UserMenu({ user }: { user: User }) {
  const navigate = useNavigate();
  const logout = useMutation(logoutMutation());
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button variant="ghost" size="sm" className="ml-auto font-normal text-soft-foreground" />
        }
      >
        {user.email}
        <ChevronDownIcon aria-hidden />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuItem render={<Link to="/settings" />}>Settings</DropdownMenuItem>
        {user.is_admin && (
          <>
            <DropdownMenuItem render={<Link to="/users" />}>Admin settings</DropdownMenuItem>
            <DropdownMenuItem render={<Link to="/import" />}>Import hits</DropdownMenuItem>
          </>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onClick={async () => {
            await logout.mutateAsync({});
            await navigate({ to: "/login", reloadDocument: true });
          }}
        >
          Sign out
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
