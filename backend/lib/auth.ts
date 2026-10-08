import type { Context, MiddlewareHandler } from "@hono/hono";
import { deleteCookie, getCookie, setCookie } from "@hono/hono/cookie";
import { type Db, one } from "../db.ts";

export type User = { id: number; email: string; is_admin: number };
export type AuthEnv = { Variables: { user: User } };

const COOKIE = "potoo_session";
const SESSION_DAYS = 30;
const ITERATIONS = 310_000;

const toHex = (b: ArrayBuffer | Uint8Array) =>
  Array.from(new Uint8Array(b), (x) => x.toString(16).padStart(2, "0")).join(
    "",
  );
const fromHex = (s: string) =>
  new Uint8Array(s.match(/../g)?.map((h) => parseInt(h, 16)) ?? []);

async function pbkdf2(password: string, salt: Uint8Array, iterations: number) {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(password),
    "PBKDF2",
    false,
    ["deriveBits"],
  );
  return crypto.subtle.deriveBits(
    { name: "PBKDF2", hash: "SHA-256", salt: salt as BufferSource, iterations },
    key,
    256,
  );
}

export async function hashPassword(password: string): Promise<string> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  return `pbkdf2$${ITERATIONS}$${toHex(salt)}$${
    toHex(await pbkdf2(password, salt, ITERATIONS))
  }`;
}

export async function verifyPassword(password: string, stored: string) {
  const [, iterations, salt, hash] = stored.split("$");
  const actual = toHex(
    await pbkdf2(password, fromHex(salt), Number(iterations)),
  );
  // Constant-time-ish comparison of equal-length hex strings.
  let diff = actual.length ^ hash.length;
  for (let i = 0; i < actual.length; i++) {
    diff |= actual.charCodeAt(i) ^ (hash.charCodeAt(i) || 0);
  }
  return diff === 0;
}

export async function sha256(input: string) {
  return toHex(
    await crypto.subtle.digest("SHA-256", new TextEncoder().encode(input)),
  );
}

export async function createSession(db: Db, c: Context, userId: number) {
  const token = toHex(crypto.getRandomValues(new Uint8Array(32)));
  const expires = new Date(Date.now() + SESSION_DAYS * 86_400_000);
  await db.execute(
    "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)",
    [await sha256(token), userId, expires.toISOString()],
  );
  setCookie(c, COOKIE, token, {
    httpOnly: true,
    sameSite: "Strict",
    secure: new URL(c.req.url).protocol === "https:" ||
      c.req.header("x-forwarded-proto") === "https",
    path: "/",
    expires,
  });
}

export async function destroySession(db: Db, c: Context) {
  const token = getCookie(c, COOKIE);
  if (token) {
    await db.execute("DELETE FROM sessions WHERE token_hash = ?", [
      await sha256(token),
    ]);
  }
  deleteCookie(c, COOKIE, { path: "/" });
}

export async function currentUser(db: Db, c: Context) {
  const token = getCookie(c, COOKIE);
  if (!token) return undefined;
  return one<User>(
    db,
    `SELECT u.id, u.email, u.is_admin FROM sessions s
     JOIN users u ON u.id = s.user_id
     WHERE s.token_hash = ? AND s.expires_at > ?`,
    [await sha256(token), new Date().toISOString()],
  );
}

export const requireUser =
  (db: Db): MiddlewareHandler<AuthEnv> => async (c, next) => {
    const user = await currentUser(db, c);
    if (!user) return c.json({ error: "Unauthorized" }, 401);
    c.set("user", user);
    await next();
  };

export const requireAdmin: MiddlewareHandler<AuthEnv> = async (c, next) => {
  if (!c.get("user").is_admin) return c.json({ error: "Forbidden" }, 403);
  await next();
};
