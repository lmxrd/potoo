import { createApp } from "./app.ts";
import { openDb } from "./db.ts";

const db = await openDb(Deno.env.get("DATABASE_PATH") ?? "data/potoo.db");
const app = createApp(db, Deno.env.get("STATIC_DIR") ?? "frontend/dist");

Deno.serve({ port: Number(Deno.env.get("PORT") ?? 8000) }, app.fetch);
