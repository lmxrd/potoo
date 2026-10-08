import { SqliteClient } from "@stdx/database/drivers/sqlite";

export type Db = SqliteClient;

export async function openDb(path: string): Promise<Db> {
  const db = new SqliteClient(path);
  await db.execute("PRAGMA journal_mode = WAL");
  await db.execute(`
    CREATE TABLE IF NOT EXISTS events (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL,
      path TEXT,
      props TEXT,
      created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    )
  `);
  await db.execute(
    "CREATE INDEX IF NOT EXISTS events_created_at ON events (created_at)",
  );
  return db;
}
