import { SqliteClient } from "@stdx/database/drivers/sqlite";

export type Db = SqliteClient;
export type Param = string | number | null;

export async function openDb(path: string): Promise<Db> {
  const db = new SqliteClient(path);
  await db.execute("PRAGMA journal_mode = WAL");
  await db.execute("PRAGMA foreign_keys = ON");
  for (const statement of SCHEMA) await db.execute(statement);
  return db;
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

const SCHEMA = [
  `CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
  )`,
  // At most one admin, enforced by the database.
  `CREATE UNIQUE INDEX IF NOT EXISTS users_single_admin ON users (is_admin) WHERE is_admin = 1`,
  `CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL
  )`,
  // One site per hostname; subdomains are separate sites.
  `CREATE TABLE IF NOT EXISTS sites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hostname TEXT NOT NULL UNIQUE,
    honor_dnt INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
  )`,
  // Only coarse data: no IP, no full user agent, no cookies, no query strings.
  `CREATE TABLE IF NOT EXISTS hits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('pageview', 'click')),
    path TEXT NOT NULL,
    name TEXT,
    referrer TEXT,
    country TEXT,
    device TEXT NOT NULL,
    visitor TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
  )`,
  `CREATE INDEX IF NOT EXISTS hits_site_time ON hits (site_id, created_at)`,
  // Daily salts for visitor hashes. Old salts are deleted, so hashes cannot be linked across days.
  `CREATE TABLE IF NOT EXISTS salts (day TEXT PRIMARY KEY, salt TEXT NOT NULL)`,
];
