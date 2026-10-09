import { assertEquals } from "@std/assert";
import { SqliteClient } from "@halvardm/nessie/clients/sqlite";
import { createApp } from "./app.ts";
import { all, openDb } from "./db.ts";

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
      json(
        { current_password: "correct horse battery", new_password: "x" },
        auth,
      ),
    )).status,
    400,
  );
  const change = await app.request(
    "/api/password",
    json({
      current_password: "correct horse battery",
      new_password: "new long pass!",
    }, auth),
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
  const users = await (await app.request("/api/users", { headers: auth }))
    .json();
  const regular = users.find((u: { email: string }) =>
    u.email === "u@example.com"
  );
  const admin = users.find((u: { email: string }) =>
    u.email === "a@example.com"
  );
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
    204,
  );
  assertEquals(
    (await app.request(
      "/api/login",
      json({ email: "u@example.com", password: "reset pass 123" }),
    )).status,
    204,
  );
});

Deno.test("multiple admins, roles and email change", async () => {
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const setup = await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );
  const auth = { cookie: setup.headers.get("set-cookie")!.split(";")[0] };

  // Creating a user with a role: a second admin, and a regular user.
  assertEquals(
    (await app.request(
      "/api/users",
      json(
        {
          email: "b@example.com",
          password: "another long pass",
          is_admin: true,
        },
        auth,
      ),
    )).status,
    201,
  );
  assertEquals(
    (await app.request(
      "/api/users",
      json({ email: "u@example.com", password: "another long pass" }, auth),
    )).status,
    201,
  );
  const users = await (await app.request("/api/users", { headers: auth }))
    .json();
  const admins = users.filter((u: { is_admin: number }) => u.is_admin === 1);
  assertEquals(admins.length, 2);

  // An admin promotes a regular user, and demotes another admin.
  const u = users.find((x: { email: string }) => x.email === "u@example.com");
  const b = users.find((x: { email: string }) => x.email === "b@example.com");
  const patch = (id: number, isAdmin: boolean) =>
    app.request(`/api/users/${id}`, {
      method: "PATCH",
      headers: { ...auth, "content-type": "application/json" },
      body: JSON.stringify({ is_admin: isAdmin }),
    });
  assertEquals((await patch(u.id, true)).status, 204);
  assertEquals((await patch(b.id, false)).status, 204);
  const after = await (await app.request("/api/users", { headers: auth }))
    .json();
  assertEquals(
    after.filter((x: { is_admin: number }) => x.is_admin === 1).length,
    2,
  );

  // The last admin cannot be demoted or deleted. Demoting the first
  // admin leaves u as the only admin, so the admin session from here on
  // is u's.
  assertEquals((await patch(1, false)).status, 204);
  const loginU = await app.request(
    "/api/login",
    json({ email: "u@example.com", password: "another long pass" }),
  );
  const authU = { cookie: loginU.headers.get("set-cookie")!.split(";")[0] };
  const patchU = (id: number, isAdmin: boolean) =>
    app.request(`/api/users/${id}`, {
      method: "PATCH",
      headers: { ...authU, "content-type": "application/json" },
      body: JSON.stringify({ is_admin: isAdmin }),
    });
  assertEquals((await patchU(u.id, false)).status, 409);
  assertEquals(
    (await app.request(`/api/users/${u.id}`, {
      method: "DELETE",
      headers: authU,
    }))
      .status,
    409,
  );

  // Admins can delete other admins and reset their passwords.
  assertEquals((await patchU(1, true)).status, 204);
  assertEquals(
    (await app.request(`/api/users/1`, { method: "DELETE", headers: authU }))
      .status,
    204,
  );
  assertEquals(
    (await app.request(
      `/api/users/${b.id}/password`,
      json({ new_password: "reset pass 123" }, authU),
    )).status,
    204,
  );
  assertEquals(
    (await app.request(
      "/api/login",
      json({ email: "b@example.com", password: "reset pass 123" }),
    )).status,
    204,
  );

  // Changing your own email: the new one logs in, the old one does not,
  // and a taken email is refused.
  assertEquals(
    (await app.request("/api/email", json({ email: "new@example.com" }, authU)))
      .status,
    204,
  );
  assertEquals(
    (await app.request(
      "/api/login",
      json({ email: "u@example.com", password: "another long pass" }),
    )).status,
    401,
  );
  assertEquals(
    (await app.request(
      "/api/login",
      json({ email: "new@example.com", password: "another long pass" }),
    )).status,
    204,
  );
  assertEquals(
    (await app.request("/api/email", json({ email: "b@example.com" }, authU)))
      .status,
    409,
  );
  assertEquals(
    (await app.request("/api/email", json({ email: "x@y.com" }))).status,
    401,
  );
});

Deno.test("CSV export with site and date selection", async () => {
  Deno.env.set("TRUST_PROXY", "1");
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const setup = await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );
  const auth = { cookie: setup.headers.get("set-cookie")!.split(";")[0] };
  const a = await (await app.request(
    "/api/sites",
    json({ hostname: "a.example.com" }, auth),
  )).json();
  const b = await (await app.request(
    "/api/sites",
    json({ hostname: "b.example.com" }, auth),
  )).json();

  const ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Mobile";
  const hit = (body: unknown, ip = "203.0.113.5") =>
    app.request("/api/collect", {
      method: "POST",
      body: JSON.stringify(body),
      headers: { "user-agent": ua, "x-forwarded-for": ip },
    });
  await hit({
    type: "pageview",
    host: "a.example.com",
    path: "/a?x=1",
    referrer: "https://ref.example.com/x",
  });
  await hit(
    { type: "click", host: "b.example.com", path: "/", name: 'signup"btn' },
    "203.0.113.6",
  );
  // A hit from the past: the API only records "now", so it goes in directly.
  await db.execute(
    `INSERT INTO hits (site_id, kind, path, name, referrer, country, device, visitor, created_at)
     VALUES (?, 'pageview', '/old', NULL, NULL, NULL, 'desktop', 'x', '2026-01-01T12:00:00.000Z')`,
    [a.id],
  );

  assertEquals((await app.request("/api/export.csv")).status, 401);

  const get = (query = "") =>
    app.request(`/api/export.csv${query}`, { headers: auth });

  // Every site, no dates: three rows with their site.
  const allRes = await get();
  assertEquals(allRes.status, 200);
  assertEquals(allRes.headers.get("content-type"), "text/csv; charset=utf-8");
  assertEquals(
    allRes.headers.get("content-disposition"),
    'attachment; filename="potoo-export.csv"',
  );
  const csv = await allRes.text();
  assertEquals(
    csv.split("\n")[0],
    "site,created_at,kind,path,name,referrer,country,device,browser,os",
  );
  assertEquals(csv.split("\n").length - 1, 4); // header + 3 hits
  assertEquals(csv.includes('"a.example.com"'), true);
  assertEquals(csv.includes('"b.example.com"'), true);
  assertEquals(csv.includes('"signup""btn"'), true);

  // One site selected.
  const onlyB = await (await get(`?sites=${b.id}`)).text();
  assertEquals(onlyB.includes('"b.example.com"'), true);
  assertEquals(onlyB.includes('"a.example.com"'), false);

  // The date range: from and to are inclusive days.
  const oldOnly = await (await get("?from=2026-01-01&to=2026-01-02")).text();
  assertEquals(oldOnly.split("\n").length - 1, 2);
  assertEquals(oldOnly.includes("/old"), true);
  const today = new Date().toISOString().slice(0, 10);
  const todayOnly = await (await get(`?from=${today}`)).text();
  assertEquals(todayOnly.includes("/old"), false);
  assertEquals(todayOnly.split("\n").length - 1, 3);

  // No sites, an unknown site, and a bad date.
  assertEquals((await get("?from=banana")).status, 400);
  assertEquals(
    (await (await get("?sites=999")).text()).split("\n").length - 1,
    1,
  );
});

Deno.test("browser and os are tracked", async () => {
  Deno.env.set("TRUST_PROXY", "1");
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const setup = await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );
  const auth = { cookie: setup.headers.get("set-cookie")!.split(";")[0] };
  const site = await (await app.request(
    "/api/sites",
    json({ hostname: "example.com" }, auth),
  )).json();

  const hit = (ua: string) =>
    app.request("/api/collect", {
      method: "POST",
      body: JSON.stringify({
        type: "pageview",
        host: "example.com",
        path: "/",
      }),
      headers: { "user-agent": ua, "x-forwarded-for": "203.0.113.7" },
    });
  // Edge contains "Chrome" and "Safari", so it must be matched first.
  await hit(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/130.0.0.0",
  );
  await hit(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15",
  );
  await hit(
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/131.0.6778.73 Mobile/15E148 Safari/604.1",
  );
  await hit(
    "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0",
  );

  const stats = await (await app.request(
    `/api/sites/${site.id}/stats?days=7`,
    { headers: auth },
  )).json();
  assertEquals(
    stats.browsers.map((b: { browser: string }) => b.browser).sort(),
    ["Chrome", "Edge", "Firefox", "Safari"],
  );
  assertEquals(
    stats.os.map((o: { os: string }) => o.os).sort(),
    ["Linux", "Windows", "iOS", "macOS"],
  );

  // The export carries the new columns too.
  const csv = await (await app.request(
    `/api/export.csv?sites=${site.id}`,
    { headers: auth },
  )).text();
  assertEquals(csv.includes('"Edge","Windows"'), true);
  assertEquals(csv.includes('"Safari","macOS"'), true);
});

Deno.test("the bar chart series is complete and calendar aligned", async () => {
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const setup = await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );
  const auth = { cookie: setup.headers.get("set-cookie")!.split(";")[0] };
  const site = await (await app.request(
    "/api/sites",
    json({ hostname: "example.com" }, auth),
  )).json();

  // Hits at 5, 2 and 0 hours ago: today if it is past midnight, else
  // yesterday; the last 24 hours include them either way.
  const day = new Date().toISOString().slice(0, 10);
  const midnight = Date.parse(day + "T00:00:00Z");
  const at = (hoursAgo: number) => new Date(Date.now() - hoursAgo * 3_600_000);
  const times = [at(5), at(2), at(0)];
  for (const t of times) {
    await db.execute(
      `INSERT INTO hits (site_id, kind, path, name, referrer, country, device, visitor, created_at)
       VALUES (?, 'pageview', '/x', NULL, NULL, NULL, 'desktop', ?, ?)`,
      [site.id, crypto.randomUUID().replaceAll("-", ""), t.toISOString()],
    );
  }
  const expectedToday = times.filter((t) => t.getTime() >= midnight).length;

  // The last 24 hours: 24 started hours, ending with the current one,
  // empty hours included.
  const rolling = await (await app.request(
    `/api/sites/${site.id}/stats?days=1`,
    { headers: auth },
  )).json();
  assertEquals(rolling.byDay.length, 24);
  assertEquals(rolling.pageviews, 3);
  const hourLabel = (ms: number) =>
    new Date(ms).toISOString().slice(0, 13) + ":00";
  assertEquals(rolling.byDay[23].day, hourLabel(Date.now()));
  assertEquals(
    rolling.byDay[0].day,
    hourLabel(Date.now() - 23 * 3_600_000),
  );
  const fiveHoursAgo = rolling.byDay.findIndex((d: { day: string }) =>
    d.day === hourLabel(Date.now() - 5 * 3_600_000)
  );
  assertEquals(rolling.byDay[fiveHoursAgo].pageviews, 1);

  // Today: this day's 24 hours, from midnight.
  const today = await (await app.request(
    `/api/sites/${site.id}/stats?days=today`,
    { headers: auth },
  )).json();
  assertEquals(today.byDay.length, 24);
  assertEquals(today.byDay[0].day, `${day}T00:00`);
  assertEquals(today.byDay[23].day, `${day}T23:00`);
  assertEquals(today.pageviews, expectedToday);

  const globalToday = await (await app.request("/api/stats?days=today", {
    headers: auth,
  })).json();
  assertEquals(globalToday.pageviews, expectedToday);
  assertEquals(globalToday.byDay.length, 24);

  // 7 days: one bar per calendar day, ending today.
  const week = await (await app.request(
    `/api/sites/${site.id}/stats?days=7`,
    { headers: auth },
  )).json();
  assertEquals(week.byDay.length, 7);
  assertEquals(week.byDay[6].day, day);
  assertEquals(
    week.byDay[0].day,
    new Date(Date.now() - 6 * 86_400_000).toISOString().slice(0, 10),
  );

  // 30 days: 30 bars.
  const month = await (await app.request(
    `/api/sites/${site.id}/stats?days=30`,
    { headers: auth },
  )).json();
  assertEquals(month.byDay.length, 30);

  // 90 days: one bar per calendar week, Mondays, 13 of them.
  const quarter = await (await app.request(
    `/api/sites/${site.id}/stats?days=90`,
    { headers: auth },
  )).json();
  assertEquals(quarter.byDay.length, 13);
  const allMondays = quarter.byDay.every((d: { day: string }) =>
    new Date(`${d.day}T00:00:00Z`).getUTCDay() === 1
  );
  assertEquals(allMondays, true);

  // A year: one bar per calendar month, 12 of them, ending this month.
  const year = await (await app.request(
    `/api/sites/${site.id}/stats?days=365`,
    { headers: auth },
  )).json();
  assertEquals(year.byDay.length, 12);
  assertEquals(
    year.byDay.every((d: { day: string }) => d.day.endsWith("-01")),
    true,
  );
  const now = new Date();
  assertEquals(
    year.byDay[11].day,
    `${now.getUTCFullYear()}-${
      String(now.getUTCMonth() + 1).padStart(2, "0")
    }-01`,
  );
});

Deno.test("date format setting", async () => {
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const setup = await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );
  const auth = { cookie: setup.headers.get("set-cookie")!.split(";")[0] };

  // The default.
  const before = await (await app.request("/api/session", { headers: auth }))
    .json();
  assertEquals(before.user.date_format, "YYYY-MM-DD");

  assertEquals(
    (await app.request(
      "/api/settings",
      json({ date_format: "DD/MM/YYYY" }, auth),
    )).status,
    204,
  );
  const after = await (await app.request("/api/session", { headers: auth }))
    .json();
  assertEquals(after.user.date_format, "DD/MM/YYYY");

  // Only the known formats are accepted.
  assertEquals(
    (await app.request(
      "/api/settings",
      json({ date_format: "DD-MM-YYYY" }, auth),
    )).status,
    400,
  );
  assertEquals(
    (await app.request("/api/settings", json({ date_format: "DD/MM/YYYY" })))
      .status,
    401,
  );
});

Deno.test("openDb applies and records migrations", async () => {
  const dir = await Deno.makeTempDir();
  const file = `${dir}/potoo.db`;

  // A fresh database: the migration runs once and is recorded.
  {
    await using db = await openDb(file);
    const applied = await all<{ name: string }>(
      db,
      "SELECT file_name AS name FROM nessie_migrations",
    );
    assertEquals(
      applied.map((m) => m.name),
      [
        "20261008090000_init_schema.ts",
        "20261009000000_allow_multiple_admins.ts",
        "20261009010000_user_date_format.ts",
        "20261009020000_browser_os.ts",
      ],
    );
  }

  // A user created between the two runs, as a database that predates
  // migrations would have: it must survive the migration on reopen.
  {
    await using legacy = new SqliteClient(file);
    await legacy.execute(
      "INSERT INTO users (email, password_hash, is_admin) VALUES ('old@example.com', 'x', 0)",
    );
  }

  {
    await using reopened = await openDb(file);
    const applied = await all<{ name: string }>(
      reopened,
      "SELECT file_name AS name FROM nessie_migrations",
    );
    assertEquals(
      applied.map((m) => m.name),
      [
        "20261008090000_init_schema.ts",
        "20261009000000_allow_multiple_admins.ts",
        "20261009010000_user_date_format.ts",
        "20261009020000_browser_os.ts",
      ],
    );
    const users = await all<{ email: string }>(
      reopened,
      "SELECT email FROM users",
    );
    assertEquals(users, [{ email: "old@example.com" }]);
  }

  await Deno.remove(dir, { recursive: true });
});

Deno.test("global stats across sites", async () => {
  Deno.env.set("TRUST_PROXY", "1");
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const setup = await app.request(
    "/api/setup",
    json({ email: "a@example.com", password: "correct horse battery" }),
  );
  const auth = { cookie: setup.headers.get("set-cookie")!.split(";")[0] };
  const a = await (await app.request(
    "/api/sites",
    json({ hostname: "a.example.com" }, auth),
  )).json();
  const b = await (await app.request(
    "/api/sites",
    json({ hostname: "b.example.com" }, auth),
  )).json();

  const ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Mobile";
  const hit = (host: string, path: string, ip: string) =>
    app.request("/api/collect", {
      method: "POST",
      body: JSON.stringify({ type: "pageview", host, path }),
      headers: { "user-agent": ua, "x-forwarded-for": ip },
    });
  await hit("a.example.com", "/a1", "203.0.113.1");
  await hit("a.example.com", "/a2", "203.0.113.2");
  await hit("b.example.com", "/b1", "203.0.113.1");

  assertEquals((await app.request("/api/stats")).status, 401);

  // Every site, without a `sites` param.
  const all = await (await app.request("/api/stats", { headers: auth })).json();
  assertEquals(all.pageviews, 3);
  // The same IP on two sites is two visitors: the hash includes the site,
  // so visitors cannot be linked across sites.
  assertEquals(all.visitors, 3);
  assertEquals(all.paths.length, 3);
  assertEquals(all.byDay.length, 7);

  // One site selected.
  const onlyA = await (await app.request(`/api/stats?sites=${a.id}`, {
    headers: auth,
  })).json();
  assertEquals(onlyA.pageviews, 2);
  assertEquals(onlyA.visitors, 2);
  assertEquals(
    onlyA.paths.map((p: { path: string }) => p.path).sort(),
    ["/a1", "/a2"],
  );

  // Multiple sites selected.
  const both = await (await app.request(`/api/stats?sites=${a.id},${b.id}`, {
    headers: auth,
  })).json();
  assertEquals(both.pageviews, 3);

  // Invalid input.
  assertEquals(
    (await app.request("/api/stats?sites=x", { headers: auth })).status,
    400,
  );
  assertEquals(
    (await app.request("/api/stats?days=999", { headers: auth })).status,
    400,
  );
});

Deno.test("stats with an explicit minute-precise range", async () => {
  await using db = await openDb(":memory:");
  const app = createApp(db, () => "NO", "frontend/dist");
  const admin = { email: "a@example.com", password: "correct horse battery" };
  const setup = await app.request("/api/setup", json(admin));
  const cookie = setup.headers.get("set-cookie")!.split(";")[0];
  const auth = { cookie };
  const site = await (await app.request(
    "/api/sites",
    json({ hostname: "example.com" }, auth),
  )).json();
  const insert = (path: string, visitor: string, at: string) =>
    db.execute(
      `INSERT INTO hits (site_id, kind, path, name, referrer, country, device, visitor, created_at)
       VALUES (?, 'pageview', ?, NULL, NULL, NULL, 'desktop', ?, ?)`,
      [site.id, path, visitor, at],
    );
  await insert("/a", "v1", "2026-01-01T12:10:00.000Z");
  await insert("/a", "v2", "2026-01-01T12:40:00.000Z");
  await insert("/b", "v3", "2026-01-02T09:15:00.000Z");
  await insert("/b", "v4", "2026-01-02T09:16:00.000Z");
  const get = (query: string) =>
    app.request(`/api/sites/${site.id}/stats?${query}`, { headers: auth });

  // Up to two days: hourly buckets, empty hours included.
  const hourly = await (await get("from=2026-01-01T12:00&to=2026-01-01T14:00"))
    .json();
  assertEquals(hourly.bucket, "hour");
  assertEquals(hourly.pageviews, 2);
  assertEquals(hourly.byDay.length, 3);
  assertEquals(hourly.byDay[0], {
    day: "2026-01-01T12:00",
    pageviews: 2,
    visitors: 2,
  });
  assertEquals(hourly.byDay[1].pageviews, 0);
  assertEquals(hourly.byDay[2].day, "2026-01-01T14:00");

  // The `to` minute is included.
  const clipped = await (await get("from=2026-01-01T12:00&to=2026-01-01T12:40"))
    .json();
  assertEquals(clipped.pageviews, 2);
  assertEquals(clipped.byDay.length, 1);

  // Beyond 48 hours, up to 35 days: daily buckets.
  const daily = await (await get(
    "from=2025-12-31T18:00&to=2026-01-03T09:16",
  )).json();
  assertEquals(daily.bucket, "day");
  assertEquals(daily.pageviews, 4);
  assertEquals(daily.byDay.map((d: { day: string }) => d.day), [
    "2025-12-31",
    "2026-01-01",
    "2026-01-02",
    "2026-01-03",
  ]);
  assertEquals(daily.byDay[1], {
    day: "2026-01-01",
    pageviews: 2,
    visitors: 2,
  });

  // Up to 180 days: weekly buckets, aligned to Mondays.
  const weekly = await (await get(
    "from=2026-01-01T00:00&to=2026-03-01T00:00",
  )).json();
  assertEquals(weekly.bucket, "week");
  assertEquals(weekly.byDay.length, 9);
  assertEquals(weekly.byDay[0].day, "2025-12-29");
  assertEquals(
    weekly.byDay.every((d: { day: string }) =>
      new Date(`${d.day}T00:00:00Z`).getUTCDay() === 1
    ),
    true,
  );
  // All four hits fall in the week of Dec 29.
  assertEquals(weekly.byDay[0].pageviews, 4);

  // Beyond that: monthly buckets.
  const monthly = await (await get(
    "from=2025-01-15T00:00&to=2026-03-01T00:00",
  )).json();
  assertEquals(monthly.bucket, "month");
  assertEquals(monthly.byDay.length, 15);
  assertEquals(monthly.byDay[0].day, "2025-01-01");
  assertEquals(monthly.byDay.at(-1).day, "2026-03-01");

  // The same range on the global stats route.
  const global = await (await app.request(
    "/api/stats?from=2026-01-01T12:00&to=2026-01-01T14:00",
    { headers: auth },
  )).json();
  assertEquals(global.pageviews, 2);
  assertEquals(global.bucket, "hour");

  // Both bounds are required, ordered, well-formed, and at most 730 days.
  assertEquals((await get("from=2026-01-01T12:00")).status, 400);
  assertEquals((await get("to=2026-01-01T12:00")).status, 400);
  assertEquals(
    (await get("from=2026-01-01T12:00&to=2026-01-01T12:00"))
      .status,
    400,
  );
  assertEquals(
    (await get("from=2026-01-01T12:00&to=2025-01-01T12:00"))
      .status,
    400,
  );
  assertEquals((await get("from=banana&to=2026-01-01T12:00")).status, 400);
  assertEquals(
    (await get("from=2024-01-01T00:00&to=2026-06-01T00:00"))
      .status,
    400,
  );
});
