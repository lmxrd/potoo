import { assert, assertEquals } from "@std/assert";
import { isStale, openGeo } from "./geo.ts";

const touch = (path: string, ageMs: number) =>
  Deno.utime(path, ageMs / 1000, ageMs / 1000);

Deno.test("isStale", async () => {
  const dir = await Deno.makeTempDir();
  const file = `${dir}/country.mmdb`;

  // A missing file is stale, so a fresh install downloads on first boot.
  assertEquals(await isStale(file), true);

  await Deno.writeFile(file, new Uint8Array());
  // A file from now is fresh, a file from a week and a day ago is not.
  assert(!(await isStale(file)));
  await touch(file, Date.now() - (8 * 86_400_000));
  assert(await isStale(file));
  // The threshold is configurable.
  assert(!(await isStale(file, 10 * 86_400_000)));

  await Deno.remove(dir, { recursive: true });
});

Deno.test("openGeo without downloading", async () => {
  const dir = await Deno.makeTempDir();
  const file = `${dir}/country.mmdb`;
  const url: string | null = null;

  // No file and no download: the lookup stays empty instead of crashing.
  let fetches = 0;
  const fetcher: typeof fetch = (() => {
    fetches++;
    throw new Error("must not be called");
  }) as never;
  const missing = await openGeo(file, { url, fetcher });
  assertEquals(missing("8.8.8.8"), null);
  assertEquals(fetches, 0);
  assertEquals(await missing.refresh(), false);

  // A failed download leaves any existing file alone and keeps the lookup.
  await Deno.writeFile(file, new TextEncoder().encode("not an mmdb"));
  await touch(file, Date.now() - (30 * 86_400_000)); // stale
  const failing = await openGeo(file, {
    url: "https://example.com/db.mmdb.gz",
    fetcher: () => Promise.resolve(new Response("garbage")), // not gzip
  });
  assertEquals(failing("8.8.8.8"), null);
  assertEquals(
    new TextDecoder().decode(await Deno.readFile(file)),
    "not an mmdb",
  );

  await Deno.remove(dir, { recursive: true });
});
