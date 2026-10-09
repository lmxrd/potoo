import type { Seed } from "@halvardm/nessie";
import { faker } from "@faker-js/faker";

/**
 * A year of demo data. Development only: the seed folder is never
 * imported by the app, so `deno compile` leaves it out of the binary.
 *
 * Wipes every hit — not the sites or users — and inserts pageviews,
 * referrers, countries, devices, browsers, operating systems and clicks
 * over the past year, for every site in the database; a demo site is
 * created when there is none. Deterministic: seeding twice gives the
 * same data.
 *
 *    deno task seed
 */
export default {
  async run({ client }) {
    faker.seed(42);

    let sites =
      (await client.query("SELECT id, hostname FROM sites").toRecords()) as {
        id: number;
        hostname: string;
      }[];
    if (!sites.length) {
      await client.execute(
        "INSERT INTO sites (hostname) VALUES ('blog.example.com')",
        [],
      );
      sites = (await client.query(
        "SELECT id, hostname FROM sites WHERE hostname = 'blog.example.com'",
      ).toRecords()) as typeof sites;
    }

    // The demo data replaces whatever hits are in the database.
    await client.execute("DELETE FROM hits", []);

    const now = Date.now();
    const midnight = Date.UTC(
      new Date(now).getUTCFullYear(),
      new Date(now).getUTCMonth(),
      new Date(now).getUTCDate(),
    );
    let inserted = 0;
    for (const site of sites) {
      const base = 40 + faker.number.int(80);
      for (let i = 0; i < YEAR; i++) {
        const start = midnight - (YEAR - 1 - i) * DAY;
        // Traffic grows over the year; today ends now, not in the future.
        const trend = 0.55 + (0.9 * i) / (YEAR - 1);
        const hits = dayHits(
          site.id,
          start,
          Math.min(start + DAY, now),
          base,
          trend,
        );
        await insert(client, hits);
        inserted += hits.length;
      }
    }

    console.log(
      `Seeded ${inserted.toLocaleString()} hits over the past year into ${
        sites.map((s) => s.hostname).join(", ")
      }.`,
    );
  },
} satisfies Seed;

const DAY = 86_400_000;
const YEAR = 365;

const PATHS: [string, number][] = [
  ["/", 0.3],
  ["/docs/quickstart", 0.15],
  ["/pricing", 0.15],
  ["/blog/self-hosting", 0.12],
  ["/blog/why-potoo", 0.1],
  ["/changelog", 0.1],
  ["/about", 0.08],
];
const REFERRERS: [string, number][] = [
  ["https://news.ycombinator.com", 0.2],
  ["https://www.google.com", 0.35],
  ["https://duckduckgo.com", 0.15],
  ["https://www.reddit.com", 0.15],
  ["https://lobste.rs", 0.15],
];
const COUNTRIES: [string, number][] = [
  ["NO", 0.12],
  ["US", 0.28],
  ["DE", 0.12],
  ["GB", 0.1],
  ["FR", 0.08],
  ["NL", 0.06],
  ["SE", 0.06],
  ["IN", 0.08],
  ["JP", 0.05],
  ["BR", 0.05],
];
const BROWSERS: [string, number][] = [
  ["Chrome", 0.64],
  ["Safari", 0.2],
  ["Firefox", 0.09],
  ["Edge", 0.07],
];
const OSES: [string, number][] = [
  ["Windows", 0.48],
  ["iOS", 0.2],
  ["macOS", 0.13],
  ["Android", 0.13],
  ["Linux", 0.06],
];
const DEVICES: [string, number][] = [
  ["desktop", 0.62],
  ["mobile", 0.33],
  ["tablet", 0.05],
];
const CLICKS: [string, number][] = [
  ["signup", 0.3],
  ["docs", 0.3],
  ["demo", 0.25],
  ["download", 0.15],
];

const pick = (options: [string, number][]) =>
  faker.helpers.weightedArrayElement(
    options.map(([value, weight]) => ({ value, weight })),
  );

// One row of the hits table: site_id, kind, path, name, referrer,
// country, device, browser, os, visitor, created_at.
type Hit = [
  number,
  string,
  string,
  string | null,
  string | null,
  string | null,
  string,
  string,
  string,
  string,
  string,
];

/** One site's pageviews for one day: a few visitors, one or more each. */
function dayHits(
  siteId: number,
  start: number,
  end: number,
  base: number,
  trend: number,
): Hit[] {
  const weekend = new Date(start).getUTCDay() % 6 === 0 ? 0.55 : 1;
  const volume = Math.round(
    base * weekend * trend * faker.number.float({ min: 0.8, max: 1.2 }),
  );
  const at = () =>
    new Date(start + faker.number.int({ min: 0, max: end - start - 1 }))
      .toISOString();

  const hits: Hit[] = [];
  for (let v = 0; v < Math.round(volume * 0.6); v++) {
    // The same visitor hash ties one person's views of one day together.
    const visitor = faker.string.hexadecimal({ length: 32 }).toLowerCase();
    const device = pick(DEVICES);
    const browser = pick(BROWSERS);
    const os = pick(OSES);
    const country = pick(COUNTRIES);
    for (let i = 0; i < faker.number.int({ min: 1, max: 3 }); i++) {
      hits.push([
        siteId,
        "pageview",
        pick(PATHS),
        null,
        i === 0 && faker.number.float() < 0.35 ? pick(REFERRERS) : null,
        country,
        device,
        browser,
        os,
        visitor,
        at(),
      ]);
    }
    // Some visitors click a tracked element.
    if (faker.number.float() < 0.15) {
      hits.push([
        siteId,
        "click",
        "/",
        pick(CLICKS),
        null,
        country,
        device,
        browser,
        os,
        visitor,
        at(),
      ]);
    }
  }
  return hits;
}

/** Insert hits in chunks, within SQLite's parameter limit. */
/** The client a seed runs on: the type the Seed type itself passes to run. */
type SeedClient = Parameters<Seed["run"]>[0]["client"];

/** Insert hits in chunks, within SQLite's parameter limit. */
async function insert(
  client: SeedClient,
  hits: Hit[],
) {
  const CHUNK = 80; // 11 columns x 80 rows stay under the 999 limit
  for (let i = 0; i < hits.length; i += CHUNK) {
    const chunk = hits.slice(i, i + CHUNK);
    await client.execute(
      `INSERT INTO hits
         (site_id, kind, path, name, referrer, country, device, browser, os, visitor, created_at)
       VALUES ${
        chunk.map(() => "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)").join(", ")
      }`,
      chunk.flat(),
    );
  }
}
