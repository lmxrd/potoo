import { useMutation, type QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  Link,
  Outlet,
  redirect,
  useNavigate,
} from "@tanstack/react-router";
import type { User } from "potoo-sdk";
import { getSessionOptions, logoutMutation } from "potoo-sdk/react-query";
import { ChevronDownIcon } from "lucide-react";
import { Button } from "../components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "../components/ui/dropdown-menu";
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
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="ml-auto font-normal text-soft-foreground">
          {user.email}
          <ChevronDownIcon aria-hidden />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuItem asChild>
          <Link to="/settings">Settings</Link>
        </DropdownMenuItem>
        {user.is_admin && (
          <>
            <DropdownMenuItem asChild>
              <Link to="/users">Admin settings</Link>
            </DropdownMenuItem>
            <DropdownMenuItem asChild>
              <Link to="/import">Import hits</Link>
            </DropdownMenuItem>
          </>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={async () => {
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
