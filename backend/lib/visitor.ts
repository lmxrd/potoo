import type { Context } from "@hono/hono";
import { getConnInfo } from "@hono/hono/deno";
import { type Db, one } from "../db.ts";
import { sha256 } from "./auth.ts";

export type Device = "desktop" | "mobile" | "tablet";

const BOT =
  /bot|crawl|spider|slurp|headless|lighthouse|curl|wget|python-requests|preview/i;

export function parseDevice(ua: string): Device | "bot" {
  if (!ua || BOT.test(ua)) return "bot";
  if (/ipad|tablet|playbook|silk|(android(?!.*mobile))/i.test(ua)) {
    return "tablet";
  }
  if (/mobi|iphone|ipod|android|windows phone/i.test(ua)) return "mobile";
  return "desktop";
}

/**
 * The coarse browser family, derived from the user agent. The order
 * matters: Edge, Opera and iOS Chrome all contain "Chrome" and "Safari".
 */
export function parseBrowser(ua: string): string {
  if (/edg\//i.test(ua)) return "Edge";
  if (/opr\/|opera/i.test(ua)) return "Opera";
  if (/crios|chrome\//i.test(ua)) return "Chrome";
  if (/firefox\/|fxios/i.test(ua)) return "Firefox";
  if (/safari\//i.test(ua)) return "Safari";
  return "Other";
}

/** The coarse operating system, derived from the user agent. */
export function parseOs(ua: string): string {
  if (/windows/i.test(ua)) return "Windows";
  if (/iphone|ipod/i.test(ua)) return "iOS";
  if (/ipad/i.test(ua)) return "iPadOS";
  if (/android/i.test(ua)) return "Android";
  if (/macintosh|mac os x/i.test(ua)) return "macOS";
  if (/linux|x11/i.test(ua)) return "Linux";
  return "Other";
}

export function clientIp(c: Context): string {
  if (Deno.env.get("TRUST_PROXY") === "1") {
    const fwd = c.req.header("x-forwarded-for")?.split(",")[0].trim();
    if (fwd) return fwd;
  }
  try {
    return getConnInfo(c).remote.address ?? "";
  } catch {
    return "";
  }
}

/** Salt that is rotated daily; previous days' salts are deleted. */
async function dailySalt(db: Db): Promise<string> {
  const day = new Date().toISOString().slice(0, 10);
  const existing = await one<{ salt: string }>(
    db,
    "SELECT salt FROM salts WHERE day = ?",
    [day],
  );
  if (existing) return existing.salt;
  const fresh = crypto.randomUUID() + crypto.randomUUID();
  await db.execute("INSERT OR IGNORE INTO salts (day, salt) VALUES (?, ?)", [
    day,
    fresh,
  ]);
  await db.execute("DELETE FROM salts WHERE day < ?", [day]);
  return (await one<{ salt: string }>(
    db,
    "SELECT salt FROM salts WHERE day = ?",
    [day],
  ))!.salt;
}

/** Pseudonymous, per-day, per-site visitor id. The IP and UA are never stored. */
export async function visitorHash(
  db: Db,
  siteId: number,
  ip: string,
  ua: string,
) {
  return (await sha256(`${await dailySalt(db)}|${siteId}|${ip}|${ua}`)).slice(
    0,
    32,
  );
}
