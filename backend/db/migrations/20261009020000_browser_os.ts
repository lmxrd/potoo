import type { Migration } from "@halvardm/nessie";

// The coarse browser and operating system of a hit, derived from the user
// agent at collect time, like the device class. The user agent itself is
// never stored.
export default {
  async up({ client }) {
    await client.execute(
      "ALTER TABLE hits ADD COLUMN browser TEXT NOT NULL DEFAULT 'Other'",
    );
    await client.execute(
      "ALTER TABLE hits ADD COLUMN os TEXT NOT NULL DEFAULT 'Other'",
    );
  },
  async down({ client }) {
    await client.execute("ALTER TABLE hits DROP COLUMN browser");
    await client.execute("ALTER TABLE hits DROP COLUMN os");
  },
} satisfies Migration;
