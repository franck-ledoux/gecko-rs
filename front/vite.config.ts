import { defineConfig } from "vite";

export default defineConfig({
  // Relative, so the same build works at a site root, in a subdirectory, and
  // served from the station's own little server.
  base: "./",
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", sourcemap: true },
});
