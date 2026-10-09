import type { NessieConfig } from "@halvardm/nessie";
import { SqliteClient } from "@halvardm/nessie/clients/sqlite";

/**
 * The nessie CLI's config. Migrations live with the app, seeds are
 * development only: nothing in backend/ imports the seed folder, so
 * `deno compile` never includes it in the binary.
 */
const config: NessieConfig = {
  client: new SqliteClient(Deno.env.get("DATABASE_PATH") ?? "data/potoo.db"),
  migrationFolders: ["./backend/db/migrations"],
  seedFolders: ["./backend/db/seeds"],
};

export default config;
