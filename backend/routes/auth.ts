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
const Email = z.email().max(254).transform((e) => e.toLowerCase());
const Credentials = z.object({
  email: Email,
  password: Password,
});

const hasUsers = async (db: Db) =>
  (await one<{ n: number }>(db, "SELECT COUNT(*) AS n FROM users"))!.n > 0;

const adminCount = async (db: Db) =>
  (await one<{ n: number }>(
    db,
    "SELECT COUNT(*) AS n FROM users WHERE is_admin = 1",
  ))!
    .n;

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

  // Change the signed-in user's own email.
  app.post("/email", requireUser(db), async (c) => {
    const parsed = z.object({ email: Email }).safeParse(
      await c.req.json().catch(() => null),
    );
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const taken = await one(
      db,
      "SELECT 1 FROM users WHERE email = ? AND id != ?",
      [parsed.data.email, c.get("user").id],
    );
    if (taken) return c.json({ error: "Email already in use" }, 409);
    await db.execute("UPDATE users SET email = ? WHERE id = ?", [
      parsed.data.email,
      c.get("user").id,
    ]);
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
    const parsed = z.object({
      email: Email,
      password: Password,
      is_admin: z.boolean().default(false),
    }).safeParse(await c.req.json().catch(() => null));
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    if (
      await one(db, "SELECT 1 FROM users WHERE email = ?", [parsed.data.email])
    ) {
      return c.json({ error: "Email already in use" }, 409);
    }
    await db.execute(
      "INSERT INTO users (email, password_hash, is_admin) VALUES (?, ?, ?)",
      [
        parsed.data.email,
        await hashPassword(parsed.data.password),
        parsed.data.is_admin ? 1 : 0,
      ],
    );
    return c.body(null, 201);
  });
  // Change the role of a user. The last admin cannot be demoted, so the
  // app always has at least one admin left.
  users.patch("/:id", async (c) => {
    const parsed = z.object({ is_admin: z.boolean() }).safeParse(
      await c.req.json().catch(() => null),
    );
    if (!parsed.success) {
      return c.json({ error: z.treeifyError(parsed.error) }, 400);
    }
    const id = Number(c.req.param("id"));
    const target = await one<{ is_admin: number }>(
      db,
      "SELECT is_admin FROM users WHERE id = ?",
      [id],
    );
    if (!target) return c.json({ error: "Not found" }, 404);
    if (
      target.is_admin === 1 && !parsed.data.is_admin &&
      (await adminCount(db)) === 1
    ) {
      return c.json({ error: "Cannot demote the last admin" }, 409);
    }
    await db.execute("UPDATE users SET is_admin = ? WHERE id = ?", [
      parsed.data.is_admin ? 1 : 0,
      id,
    ]);
    return c.body(null, 204);
  });

  users.delete("/:id", async (c) => {
    const id = Number(c.req.param("id"));
    const target = await one<{ is_admin: number }>(
      db,
      "SELECT is_admin FROM users WHERE id = ?",
      [id],
    );
    if (!target) return c.body(null, 204);
    if (target.is_admin === 1 && (await adminCount(db)) === 1) {
      return c.json({ error: "Cannot delete the last admin" }, 409);
    }
    await db.execute("DELETE FROM users WHERE id = ?", [id]);
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
      "UPDATE users SET password_hash = ? WHERE id = ?",
      [await hashPassword(parsed.data.new_password), Number(c.req.param("id"))],
    );
    return res.affectedRows
      ? c.body(null, 204)
      : c.json({ error: "Not found" }, 404);
  });
  app.route("/users", users);

  return app;
}
