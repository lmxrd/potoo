import { Hono } from "@hono/hono";
import { z } from "@zod/zod";
import { type Db, one } from "../db.ts";
import {
  type AuthEnv,
  createSession,
  currentUser,
  destroySession,
  hashPassword,
  requireAdmin,
  requireUser,
  verifyPassword,
} from "../lib/auth.ts";

const Credentials = z.object({
  email: z.email().max(254).transform((e) => e.toLowerCase()),
  password: z.string().min(10).max(200),
});

const hasUsers = async (db: Db) =>
  (await one<{ n: number }>(db, "SELECT COUNT(*) AS n FROM users"))!.n > 0;

export function authRoutes(db: Db) {
  const app = new Hono<AuthEnv>();

  // Session + first-run state. The frontend uses this to route every page.
  app.get("/session", async (c) => {
    const needsSetup = !(await hasUsers(db));
    const user = needsSetup ? undefined : await currentUser(db, c);
    return c.json({ needsSetup, user: user ?? null });
  });

  // Creates the admin. Only possible while there are no users at all.
  app.post("/setup", async (c) => {
    const parsed = Credentials.safeParse(await c.req.json().catch(() => null));
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const res = await db.execute(
      `INSERT INTO users (email, password_hash, is_admin)
       SELECT ?, ?, 1 WHERE NOT EXISTS (SELECT 1 FROM users)`,
      [parsed.data.email, await hashPassword(parsed.data.password)],
    );
    if (!res.affectedRows) return c.json({ error: "Already set up" }, 409);
    await createSession(db, c, Number(res.lastInsertId));
    return c.body(null, 201);
  });

  app.post("/login", async (c) => {
    const parsed = Credentials.safeParse(await c.req.json().catch(() => null));
    const row = parsed.success
      ? await one<{ id: number; password_hash: string }>(
        db,
        "SELECT id, password_hash FROM users WHERE email = ?",
        [parsed.data.email],
      )
      : undefined;
    // Same work and same response whether or not the user exists.
    const ok = parsed.success &&
      await verifyPassword(
        parsed.data.password,
        row?.password_hash ?? "pbkdf2$310000$00$00",
      ) && row;
    if (!ok) return c.json({ error: "Invalid email or password" }, 401);
    await createSession(db, c, row!.id);
    return c.body(null, 204);
  });

  app.post("/logout", async (c) => {
    await destroySession(db, c);
    return c.body(null, 204);
  });

  // Users: admin only. Everyone created here is a regular (non-admin) user.
  const users = new Hono<AuthEnv>();
  users.use(requireUser(db), requireAdmin);
  users.get("/", async (c) =>
    c.json(
      await db.query(
        "SELECT id, email, is_admin, created_at FROM users ORDER BY id",
      ).toRecords(),
    ));
  users.post("/", async (c) => {
    const parsed = Credentials.safeParse(await c.req.json().catch(() => null));
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    if (
      await one(db, "SELECT 1 FROM users WHERE email = ?", [parsed.data.email])
    ) {
      return c.json({ error: "Email already in use" }, 409);
    }
    await db.execute(
      "INSERT INTO users (email, password_hash, is_admin) VALUES (?, ?, 0)",
      [parsed.data.email, await hashPassword(parsed.data.password)],
    );
    return c.body(null, 201);
  });
  users.delete("/:id", async (c) => {
    const id = Number(c.req.param("id"));
    await db.execute("DELETE FROM users WHERE id = ? AND is_admin = 0", [id]);
    return c.body(null, 204);
  });
  app.route("/users", users);

  return app;
}
