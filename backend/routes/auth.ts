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
import { clientIp } from "../lib/visitor.ts";

const Password = z.string().min(10).max(200);
const Credentials = z.object({
  email: z.email().max(254).transform((e) => e.toLowerCase()),
  password: Password,
});

const hasUsers = async (db: Db) =>
  (await one<{ n: number }>(db, "SELECT COUNT(*) AS n FROM users"))!.n > 0;

// In-memory throttle for failed logins: after MAX_FAILURES within WINDOW_MS
// the IP + email pair is locked out until the oldest failure expires.
// It resets when the server restarts, which keeps the no-storage promise.
const WINDOW_MS = 15 * 60_000;
const MAX_FAILURES = 5;

export function authRoutes(db: Db) {
  const app = new Hono<AuthEnv>();
  const failures = new Map<string, number[]>();

  const recent = (key: string) => {
    const now = Date.now();
    const left = (failures.get(key) ?? []).filter((t) => now - t < WINDOW_MS);
    if (left.length) failures.set(key, left);
    else failures.delete(key);
    return left;
  };

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
    const key = parsed.success ? `${clientIp(c)}|${parsed.data.email}` : null;
    if (key && recent(key).length >= MAX_FAILURES) {
      const retryAfter = Math.ceil(
        (recent(key)[0] + WINDOW_MS - Date.now()) / 1000,
      );
      return c.json({ error: "Too many attempts, try again later" }, 429, {
        "retry-after": String(retryAfter),
      });
    }
    // Same work and same response whether or not the user exists.
    const ok = parsed.success &&
      await verifyPassword(
        parsed.data.password,
        row?.password_hash ?? "pbkdf2$310000$00$00",
      ) && row;
    if (!ok) {
      if (key) {
        const left = recent(key);
        left.push(Date.now());
        failures.set(key, left);
        if (failures.size > 10_000) failures.clear();
      }
      return c.json({ error: "Invalid email or password" }, 401);
    }
    if (key) failures.delete(key);
    await createSession(db, c, row!.id);
    return c.body(null, 204);
  });

  app.post("/logout", async (c) => {
    await destroySession(db, c);
    return c.body(null, 204);
  });

  // Change the signed-in user's own password.
  app.post("/password", requireUser(db), async (c) => {
    const parsed = z.object({
      current_password: z.string().max(200),
      new_password: Password,
    }).safeParse(await c.req.json().catch(() => null));
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const row = await one<{ password_hash: string }>(
      db,
      "SELECT password_hash FROM users WHERE id = ?",
      [c.get("user").id],
    );
    if (
      !row ||
      !(await verifyPassword(parsed.data.current_password, row.password_hash))
    ) {
      return c.json({ error: "Invalid password" }, 401);
    }
    await db.execute("UPDATE users SET password_hash = ? WHERE id = ?", [
      await hashPassword(parsed.data.new_password),
      c.get("user").id,
    ]);
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
  // Admin sets a new password for a regular user. The admin account is
  // excluded here, like deletion: admins change their own via /password.
  users.post("/:id/password", async (c) => {
    const parsed = z.object({ new_password: Password }).safeParse(
      await c.req.json().catch(() => null),
    );
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const res = await db.execute(
      "UPDATE users SET password_hash = ? WHERE id = ? AND is_admin = 0",
      [await hashPassword(parsed.data.new_password), Number(c.req.param("id"))],
    );
    return res.affectedRows
      ? c.body(null, 204)
      : c.json({ error: "Not found" }, 404);
  });
  app.route("/users", users);

  return app;
}
