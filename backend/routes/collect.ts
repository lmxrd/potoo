import type { Context } from "@hono/hono";
import { Hono } from "@hono/hono";
import { cors } from "@hono/hono/cors";
import { type Db, one } from "../db.ts";
import { clientIp, parseDevice, visitorHash } from "../lib/visitor.ts";
import type { GeoLookup } from "../lib/geo.ts";
import { TRACKER_JS } from "../lib/tracker.ts";

const GIF = Uint8Array.from(
  atob("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7"),
  (ch) => ch.charCodeAt(0),
);

/** Paths may contain personal data in their query/fragment, so those are dropped. */
function cleanPath(raw: unknown): string {
  const s = typeof raw === "string" ? raw : "/";
  const path = s.split(/[?#]/)[0].slice(0, 500);
  return path.startsWith("/") ? path : `/${path}`;
}

/** Only the referring hostname is kept, and never the site itself. */
function cleanReferrer(raw: unknown, host: string): string | null {
  if (typeof raw !== "string" || !raw) return null;
  try {
    const h = new URL(raw).host.toLowerCase();
    return h === host ? null : h.slice(0, 253);
  } catch {
    return null;
  }
}

function hostOf(raw: unknown): string | undefined {
  if (typeof raw !== "string" || !raw) return undefined;
  try {
    return new URL(raw.includes("://") ? raw : `https://${raw}`).host
      .toLowerCase();
  } catch {
    return undefined;
  }
}

type Hit = {
  host?: string;
  kind: "pageview" | "click";
  path?: unknown;
  name?: unknown;
  referrer?: unknown;
};

export function collectRoutes(db: Db, geo: GeoLookup) {
  const app = new Hono();
  app.use("*", cors({ origin: "*", allowMethods: ["GET", "POST"] }));

  async function record(c: Context, hit: Hit) {
    const host = hit.host ?? hostOf(c.req.header("origin"));
    if (!host) return;
    const site = await one<{ id: number; honor_dnt: number }>(
      db,
      "SELECT id, honor_dnt FROM sites WHERE hostname = ?",
      [host],
    );
    if (!site) return;
    if (
      site.honor_dnt &&
      (c.req.header("dnt") === "1" || c.req.header("sec-gpc") === "1")
    ) return;

    const ua = c.req.header("user-agent") ?? "";
    const device = parseDevice(ua);
    if (device === "bot") return;
    const ip = clientIp(c);

    await db.execute(
      `INSERT INTO hits (site_id, kind, path, name, referrer, country, device, visitor)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
      [
        site.id,
        hit.kind,
        cleanPath(hit.path),
        hit.kind === "click" && typeof hit.name === "string"
          ? hit.name.split(/[?#]/)[0].slice(0, 200)
          : null,
        hit.kind === "pageview" ? cleanReferrer(hit.referrer, host) : null,
        ip ? geo(ip) : null,
        device,
        await visitorHash(db, site.id, ip, ua),
      ],
    );
  }

  // JS tracker. Sent as text/plain by sendBeacon so no CORS preflight is needed.
  app.post("/collect", async (c) => {
    const body = await c.req.text();
    let data: Record<string, unknown> = {};
    try {
      data = JSON.parse(body);
    } catch { /* ignore malformed payloads */ }
    if (data.type === "pageview" || data.type === "click") {
      await record(c, {
        host: hostOf(data.host),
        kind: data.type,
        path: data.path,
        name: data.name,
        referrer: data.referrer,
      });
    }
    return c.body(null, 204);
  });

  // No-JS fallback: <img src=".../api/collect.gif?h=example.com&p=/page">.
  // Without `p`/`h`, the Referer header is used when the browser sends it.
  app.get("/collect.gif", async (c) => {
    const ref = c.req.header("referer");
    let refPath: string | undefined;
    try {
      refPath = ref ? new URL(ref).pathname : undefined;
    } catch { /* ignore */ }
    await record(c, {
      host: hostOf(c.req.query("h")) ?? hostOf(ref),
      kind: "pageview",
      path: c.req.query("p") ?? refPath,
      referrer: c.req.query("r"),
    });
    return c.body(GIF, 200, {
      "content-type": "image/gif",
      "cache-control": "no-store",
    });
  });

  app.get("/tracker.js", (c) =>
    c.body(TRACKER_JS, 200, {
      "content-type": "text/javascript; charset=utf-8",
      "cache-control": "public, max-age=3600",
    }));

  return app;
}
