import type { Migration } from "@halvardm/nessie";

// Users pick how dates are shown to them. The stored format is one of the
// formats the backend validates, and defaults to YYYY-MM-DD.
export default {
  async up({ client }) {
    await client.execute(
      "ALTER TABLE users ADD COLUMN date_format TEXT NOT NULL DEFAULT 'YYYY-MM-DD'",
    );
  },
  async down({ client }) {
    await client.execute("ALTER TABLE users DROP COLUMN date_format");
  },
} satisfies Migration;
