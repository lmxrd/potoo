# potoo-e2e

End-to-end smoke tests: Playwright drives Chromium through the built image,
on a fresh database. They cover the flows that must never break: first
setup, signing out and in, two-factor authentication, and a page view that
reaches the dashboard. Everything else is tested closer to the code.

```sh
mise run test:e2e
```

builds the image, starts it with an empty volume on `127.0.0.1:8090`
(`E2E_PORT`), runs the tests and removes the container (`scripts/e2e.sh`;
`CONTAINER=docker` to use Docker instead of podman). CI runs them against
the image it builds.
