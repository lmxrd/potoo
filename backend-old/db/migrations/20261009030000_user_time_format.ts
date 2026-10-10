import type { Migration } from "@halvardm/nessie";

// Users pick how times are shown to them: 24-hour by default, or 12-hour
// with AM/PM.
export default {
  async up({ client }) {
    await client.execute(
      "ALTER TABLE users ADD COLUMN time_format TEXT NOT NULL DEFAULT '24h'",
    );
  },
  async down({ client }) {
    await client.execute("ALTER TABLE users DROP COLUMN time_format");
  },
} satisfies Migration;
