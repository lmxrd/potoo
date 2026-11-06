/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { heyApiPlugin } from "@hey-api/vite-plugin";
import { config } from "../sdk/openapi-ts.config.ts";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    heyApiPlugin({ config: config }),
    // File-based routing: generates src/routeTree.gen.ts from src/routes/.
    // Must run before the react plugin.
    tanstackRouter({
      target: "react",
      autoCodeSplitting: true,
      quoteStyle: "double",
      semicolons: true,
    }),
    react(),
    tailwindcss(),
  ],
  // The unit tests (src/lib/*.test.ts) run in node, in UTC, so the local
  // times of the date ranges are the same on every machine.
  test: { environment: "node", env: { TZ: "UTC" } },
  server: {
    proxy: {
      // Same-origin cookies in development: the SPA talks to /api on the
      // vite origin and vite forwards it to the backend.
      "/api": { target: "http://localhost:8000", ws: true },
      // The tracker script is served from the root, like in production.
      "/tracker.js": "http://localhost:8000",
    },
  },
});
