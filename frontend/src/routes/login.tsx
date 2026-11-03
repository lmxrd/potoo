import { createFileRoute } from "@tanstack/react-router";
import { CredentialsForm } from "../components/credentials-form";
import { ErrorText } from "../components/error-text";

export const Route = createFileRoute("/login")({
  // A failed single sign-on comes back here with a code saying why.
  validateSearch: (search): { error?: string } => ({
    error: typeof search.error === "string" ? search.error : undefined,
  }),
  component: Login,
});

/**
 * What the codes of `/api/auth/oidc/callback` mean. The codes are listed in
 * the backend's `Failure` (src/oidc.rs) too: change both.
 */
const SSO_ERRORS: Record<string, string> = {
  unavailable: "Single sign-on is not available right now.",
  state:
    "The sign-in expired, was started in another browser, or was replaced by a newer one in another tab. Try again.",
  provider: "The provider did not sign you in.",
  token: "The provider's answer could not be verified.",
  email: "The provider did not give a verified email address.",
  unknown_user: "There is no account for your email address. Ask an admin to create one.",
  misconfigured:
    "Single sign-on is set up for another address (http or https) than this one. Ask an admin to check its callback URL.",
  linked:
    "Your email address belongs to a user who signs in with another account at the provider. Ask an admin.",
};

function Login() {
  const { session } = Route.useRouteContext();
  const { error } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">Sign in</h1>
      {error && <ErrorText>{SSO_ERRORS[error] ?? "Single sign-on failed."}</ErrorText>}
      {session.oidc && (
        <div className="flex max-w-sm flex-col gap-3">
          {/* A navigation, not a request: the browser goes to the provider. */}
          <a
            href="/api/auth/oidc/login"
            className="rounded bg-accent px-3 py-2 text-center text-sm font-medium text-accent-ink hover:bg-accent-hover"
          >
            Sign in with {session.oidc.label}
          </a>
          <p className="text-center text-xs text-muted">or with your password</p>
        </div>
      )}
      <CredentialsForm
        kind="login"
        submit="Sign in"
        // A password attempt is a new try: the single sign-on error is old.
        onSubmit={() => error && navigate({ search: {}, replace: true })}
        onDone={() => location.assign("/")}
      />
    </div>
  );
}
