import { defineConfig } from "vite";

/**
 * When `GECKO_PORT` is set, the dev server stands in front of a running
 * `gecko`: it serves the TypeScript with hot reload and forwards the one
 * thing the page asks of its origin — the files endpoint — to the binary.
 *
 * That is what lets a TypeScript edit reload the page while scripts can
 * still read and write real files.
 */
const files = process.env.GECKO_PORT;

export default defineConfig({
  // Relative, so the same build works at a site root, in a subdirectory, and
  // served from the binary's own little server.
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
    proxy: files ? { "/file": { target: `http://127.0.0.1:${files}` } } : undefined,
  },
  build: { target: "es2022", outDir: "dist", sourcemap: true },
});
