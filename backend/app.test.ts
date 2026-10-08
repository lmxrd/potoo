import { assertEquals } from "jsr:@std/assert@^1";
import { createApp } from "./app.ts";
import { openDb } from "./db.ts";

const json = (body: unknown, headers: HeadersInit = {}) => ({
  method: "POST",
  body: JSON.stringify(body),
  headers: { "content-type": "application/json", ...headers },
});

Deno.test("setup, users, sites, collection and stats", async () => {
  Deno.env.set("TRUST_PROXY", "1");
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const admin = { email: "a@example.com", password: "correct horse battery" };

  assertEquals(
    (await (await app.request("/api/session")).json()).needsSetup,
    true,
  );
  const setup = await app.request("/api/setup", json(admin));
  assertEquals(setup.status, 201);
  const cookie = setup.headers.get("set-cookie")!.split(";")[0];
  assertEquals((await app.request("/api/setup", json(admin))).status, 409);

  const auth = { cookie };
  assertEquals(
    (await app.request(
      "/api/users",
      json(
        { email: "u@example.com", password: "another long pass" },
        auth,
      ),
    )).status,
    201,
  );
  assertEquals((await app.request("/api/sites")).status, 401);

  const site = await (await app.request(
    "/api/sites",
    json({ hostname: "Blog.Example.com" }, auth),
  )).json();
  assertEquals(site.hostname, "blog.example.com");

  const ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Mobile";
  const hit = (body: unknown) =>
    app.request("/api/collect", {
      method: "POST",
      body: JSON.stringify(body),
      headers: { "user-agent": ua, "x-forwarded-for": "203.0.113.5" },
    });
  await hit({
    type: "pageview",
    host: "blog.example.com",
    path: "/a?secret=1",
  });
  await hit({
    type: "click",
    host: "blog.example.com",
    path: "/a",
    name: "signup",
  });
  await hit({ type: "pageview", host: "unknown.com", path: "/" });

  const stats = await (await app.request(`/api/sites/${site.id}/stats`, {
    headers: auth,
  })).json();
  assertEquals(stats.pageviews, 1);
  assertEquals(stats.paths[0].path, "/a");
  assertEquals(stats.countries[0].country, "NO");
  assertEquals(stats.devices[0].device, "mobile");
  assertEquals(stats.clicks[0], { name: "signup", clicks: 1 });
  assertEquals(stats.site.honor_dnt, 1);

  // The admin can toggle DNT honoring for a site.
  const patch = await app.request(`/api/sites/${site.id}`, {
    method: "PATCH",
    headers: { ...auth, "content-type": "application/json" },
    body: JSON.stringify({ honor_dnt: false }),
  });
  assertEquals(patch.status, 204);
  const patched = await (await app.request(`/api/sites/${site.id}/stats`, {
    headers: auth,
  })).json();
  assertEquals(patched.site.honor_dnt, 0);
});

Deno.test("login rate limiting", async () => {
  Deno.env.set("TRUST_PROXY", "1");
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );

  const attempt = (ip: string, password: string) =>
    app.request(
      "/api/login",
      json(
        { email: "a@example.com", password },
        { "x-forwarded-for": ip },
      ),
    );

  for (let i = 0; i < 5; i++) {
    assertEquals((await attempt("203.0.113.9", "wrong password!")).status, 401);
  }
  // Locked out even with the correct password, and tells the client when to retry.
  const locked = await attempt("203.0.113.9", "correct horse battery");
  assertEquals(locked.status, 429);
  assertEquals(Number(locked.headers.get("retry-after")) > 0, true);
  // A different IP is not locked out.
  assertEquals(
    (await attempt("203.0.113.10", "correct horse battery")).status,
    204,
  );
});

Deno.test("password change and admin reset", async () => {
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const setup = await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );
  const auth = { cookie: setup.headers.get("set-cookie")!.split(";")[0] };
  await app.request(
    "/api/users",
    json({ email: "u@example.com", password: "another long pass" }, auth),
  );

  // Requires signing in.
  assertEquals(
    (await app.request("/api/password", json({}))).status,
    401,
  );
  // Wrong current password.
  assertEquals(
    (await app.request(
      "/api/password",
      json({ current_password: "nope", new_password: "new long pass!" }, auth),
    )).status,
    401,
  );
  // New password must pass the same rules as everywhere else.
  assertEquals(
    (await app.request(
      "/api/password",
      json({ current_password: "correct horse battery", new_password: "x" }, auth),
    )).status,
    400,
  );
  const change = await app.request(
    "/api/password",
    json({ current_password: "correct horse battery", new_password: "new long pass!" }, auth),
  );
  assertEquals(change.status, 204);
  // The existing session survives the change; the old password does not.
  assertEquals(
    (await app.request("/api/session", { headers: auth })).status,
    200,
  );
  assertEquals(
    (await app.request(
      "/api/login",
      json({ email: "a@example.com", password: "correct horse battery" }),
    )).status,
    401,
  );
  assertEquals(
    (await app.request(
      "/api/login",
      json({ email: "a@example.com", password: "new long pass!" }),
    )).status,
    204,
  );

  // Admin resets a regular user's password; the admin cannot be reset here.
  const users = await (await app.request("/api/users", { headers: auth })).json();
  const regular = users.find((u: { email: string }) =>
    u.email === "u@example.com"
  );
  const admin = users.find((u: { email: string }) => u.email === "a@example.com");
  assertEquals(
    (await app.request(
      `/api/users/${regular.id}/password`,
      json({ new_password: "reset pass 123" }, auth),
    )).status,
    204,
  );
  assertEquals(
    (await app.request(
      `/api/users/${admin.id}/password`,
      json({ new_password: "reset pass 123" }, auth),
    )).status,
    404,
  );
  assertEquals(
    (await app.request(
      "/api/login",
      json({ email: "u@example.com", password: "reset pass 123" }),
    )).status,
    204,
  );
});
