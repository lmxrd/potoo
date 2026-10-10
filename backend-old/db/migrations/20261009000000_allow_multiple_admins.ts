import type { Migration } from "@halvardm/nessie";

// Users can be admins in any number. The index of the initial schema
// enforced a single admin; this drops it. The `down` recreates it, which
// fails while more than one admin exists, as it should.
export default {
  async up({ client }) {
    await client.execute("DROP INDEX IF EXISTS users_single_admin");
  },
  async down({ client }) {
    await client.execute(
      `CREATE UNIQUE INDEX IF NOT EXISTS users_single_admin ON users (is_admin) WHERE is_admin = 1`,
    );
  },
} satisfies Migration;
