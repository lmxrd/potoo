import { assertEquals } from "jsr:@std/assert@^1";
import { createApp } from "./app.ts";
import { openDb } from "./db.ts";

Deno.test("records events and reports stats", async () => {
  await using db = await openDb(":memory:");
  const app = createApp(db, "frontend/dist");

  const bad = await app.request("/api/events", {
    method: "POST",
    body: JSON.stringify({}),
  });
  assertEquals(bad.status, 400);

  const ok = await app.request("/api/events", {
    method: "POST",
    body: JSON.stringify({ name: "pageview", path: "/" }),
  });
  assertEquals(ok.status, 201);

  const stats = await (await app.request("/api/stats")).json();
  assertEquals(stats.total, 1);
  assertEquals(stats.byName[0], { name: "pageview", count: 1 });
});
