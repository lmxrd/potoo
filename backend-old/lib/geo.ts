import { Buffer } from "node:buffer";
import { type CountryResponse, Reader } from "maxmind";

/**
 * Country lookup from a local MaxMind-format country database. The database
 * is downloaded from DB-IP (country lite, CC BY 4.0) when it is missing or
 * older than a week, and can be refreshed with the returned `refresh`. Look
 * ups themselves are always local. Set `GEOIP_URL` to another URL, or to an
 * empty string to never download anything.
 */
export const GEO_REFRESH_MS = 7 * 86_400_000;

const defaultUrl = () =>
  `https://download.db-ip.com/free/dbip-country-lite-${
    new Date().toISOString().slice(0, 7)
  }.mmdb.gz`;

/** Country lookup: the country code of an IP, or null when unknown. */
export type GeoLookup = (ip: string) => string | null;
/** What `openGeo` returns: the lookup, plus a way to refresh the database. */
export type GeoHandle = GeoLookup & { refresh: () => Promise<boolean> };

export async function openGeo(
  path: string,
  opts: { url?: string | null; fetcher?: typeof fetch } = {},
): Promise<GeoHandle> {
  const envUrl = Deno.env.get("GEOIP_URL");
  const url = opts.url !== undefined
    ? opts.url
    : envUrl === ""
    ? null
    : envUrl ?? defaultUrl();
  const fetcher = opts.fetcher ?? fetch;

  let reader: Reader<CountryResponse> | undefined;

  const openFile = async () => {
    try {
      // maxmind requires a Node Buffer, not a plain Uint8Array.
      reader = new Reader<CountryResponse>(
        Buffer.from(await Deno.readFile(path)) as never,
      );
      return true;
    } catch {
      return false;
    }
  };

  /** Download, validate and install a fresh database. Returns success. */
  const refresh = async (): Promise<boolean> => {
    if (!url) return false;
    try {
      const res = await fetcher(url, { signal: AbortSignal.timeout(60_000) });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const bytes = await gunzip(new Uint8Array(await res.arrayBuffer()));
      // Only replace the file once the new database actually opens.
      reader = new Reader<CountryResponse>(Buffer.from(bytes) as never);
      try {
        await Deno.remove(path);
      } catch { /* not present yet */ }
      const dir = path.replace(/[/\\][^/\\]+$/, "");
      if (dir && dir !== path) {
        await Deno.mkdir(dir, { recursive: true }).catch(() => {});
      }
      await Deno.writeFile(path, bytes, { create: true });
      return true;
    } catch (err) {
      console.warn(
        `GeoIP database download failed: ${
          err instanceof Error ? err.message : err
        }`,
      );
      return false;
    }
  };

  const loaded = await openFile();
  if (!loaded || await isStale(path)) {
    const fresh = await refresh();
    if (!fresh && !loaded) {
      console.warn(
        `GeoIP database not found at ${path}; country will be empty`,
      );
    }
  }

  const lookup: GeoLookup = (ip: string): string | null => {
    try {
      return reader?.get(ip)?.country?.iso_code ?? null;
    } catch {
      return null;
    }
  };
  return Object.assign(lookup, { refresh });
}

/**
 * Whether the database file is older than a week. A missing file counts as
 * stale, so a fresh install downloads on first boot.
 */
export async function isStale(
  path: string,
  maxAgeMs = GEO_REFRESH_MS,
): Promise<boolean> {
  try {
    const { mtime } = await Deno.stat(path);
    return mtime === null || Date.now() - mtime.getTime() > maxAgeMs;
  } catch {
    return true;
  }
}

async function gunzip(bytes: Uint8Array): Promise<Uint8Array> {
  const stream = new Blob([bytes as never]).stream().pipeThrough(
    new DecompressionStream("gzip"),
  );
  return new Uint8Array(await new Response(stream).arrayBuffer());
}
