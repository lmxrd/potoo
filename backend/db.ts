import { type FileEntry, isMigrationFile, MigrationClient } from "@halvardm/nessie";
import { SqliteClient } from "@halvardm/nessie/clients/sqlite";

export type Db = SqliteClient;
export type Param = string | number | null;

const MIGRATIONS_DIR = new URL("./db/migrations/", import.meta.url);

export async function openDb(path: string): Promise<Db> {
  const db = new SqliteClient(path);
  await db.execute("PRAGMA journal_mode = WAL");
  await db.execute("PRAGMA foreign_keys = ON");
  const migrations = new MigrationClient(db, {
    migrationFiles: await listMigrations(),
    info: () => {}, // startup runs silently; errors still throw
  });
  await migrations.migrate();
  return db;
}

/** The migration files in the migrations folder, in the order they run. */
async function listMigrations(): Promise<FileEntry[]> {
  const files: FileEntry[] = [];
  try {
    for await (const entry of Deno.readDir(MIGRATIONS_DIR)) {
      if (entry.isFile && isMigrationFile(entry.name)) {
        files.push({ name: entry.name, path: new URL(entry.name, MIGRATIONS_DIR).href });
      }
    }
  } catch {
    // No migrations folder: nothing to run.
  }
  return files;
}

/** Run a query and return all rows as typed records. */
export async function all<T>(
  db: Db,
  sql: string,
  params: Param[] = [],
): Promise<T[]> {
  return (await db.query(sql, params).toRecords()) as T[];
}

export async function one<T>(
  db: Db,
  sql: string,
  params: Param[] = [],
): Promise<T | undefined> {
  return (await all<T>(db, sql, params))[0];
}
