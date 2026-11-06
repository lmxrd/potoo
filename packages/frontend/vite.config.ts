/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

export default defineConfig({
  plugins: [tanstackRouter({ target: "react", autoCodeSplitting: true }), react(), tailwindcss()],
  // The unit tests (src/lib/*.test.ts) run in node, in UTC, so the local
  // times of the date ranges are the same on every machine.
  test: { environment: "node", env: { TZ: "UTC" } },
  // The tracker script is served from the root, like in production.
  server: { proxy: { "/api": "http://localhost:8000", "/tracker.js": "http://localhost:8000" } },
});
