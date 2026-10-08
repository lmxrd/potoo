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
