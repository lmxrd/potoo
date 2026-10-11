import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

export default defineConfig({
  plugins: [tanstackRouter({ target: "react", autoCodeSplitting: true }), react(), tailwindcss()],
  // The tracker script is served from the root, like in production.
  server: { proxy: { "/api": "http://localhost:8000", "/tracker.js": "http://localhost:8000" } },
});
