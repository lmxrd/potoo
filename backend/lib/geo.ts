import { type CountryResponse, Reader } from "maxmind";

/**
 * Country lookup from a local MaxMind-format country database (GeoLite2 or
 * DB-IP "country lite"). Without a database file, country is simply unknown.
 */
export async function openGeo(path: string) {
  let reader: Reader<CountryResponse> | undefined;
  try {
    reader = new Reader<CountryResponse>(
      await Deno.readFile(path) as never,
    );
  } catch {
    console.warn(`GeoIP database not found at ${path}; country will be empty`);
  }
  return (ip: string): string | null => {
    try {
      return reader?.get(ip)?.country?.iso_code ?? null;
    } catch {
      return null;
    }
  };
}
export type GeoLookup = Awaited<ReturnType<typeof openGeo>>;
