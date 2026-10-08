import { Hono } from "@hono/hono";
import { z } from "@zod/zod";
import type { Db } from "../db.ts";

const EventInput = z.object({
  name: z.string().min(1).max(100),
  path: z.string().max(2000).optional(),
  props: z.record(z.string(), z.unknown()).optional(),
});

const StatsQuery = z.object({
  days: z.coerce.number().int().min(1).max(365).default(7),
});

export function eventsRoutes(db: Db) {
  return new Hono()
    .post("/events", async (c) => {
      const parsed = EventInput.safeParse(await c.req.json().catch(() => null));
      if (!parsed.success) {
        return c.json({ error: z.treeifyError(parsed.error) }, 400);
      }
      const { name, path, props } = parsed.data;
      await db.execute(
        "INSERT INTO events (name, path, props) VALUES (?, ?, ?)",
        [name, path ?? null, props ? JSON.stringify(props) : null],
      );
      return c.body(null, 201);
    })
    .get("/stats", async (c) => {
      const parsed = StatsQuery.safeParse(c.req.query());
      if (!parsed.success) {
        return c.json({ error: z.treeifyError(parsed.error) }, 400);
      }
      const since = `-${parsed.data.days} days`;
      const total = (await db.query(
        "SELECT COUNT(*) AS count FROM events WHERE created_at >= datetime('now', ?)",
        [since],
      ).toRecords()) as { count: number }[];
      const byName = (await db.query(
        `SELECT name, COUNT(*) AS count FROM events
         WHERE created_at >= datetime('now', ?)
         GROUP BY name ORDER BY count DESC LIMIT 20`,
        [since],
      ).toRecords()) as { name: string; count: number }[];
      const byDay = (await db.query(
        `SELECT date(created_at) AS day, COUNT(*) AS count FROM events
         WHERE created_at >= datetime('now', ?)
         GROUP BY day ORDER BY day`,
        [since],
      ).toRecords()) as { day: string; count: number }[];
      return c.json({ total: total[0]?.count ?? 0, byName, byDay });
    });
}
