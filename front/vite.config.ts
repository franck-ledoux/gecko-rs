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
  build: {
    target: "es2022",
    outDir: "dist",
    sourcemap: true,
    // Unminified for a coverage run, and only then. V8 counts byte ranges
    // in what the browser loaded: minified, those ranges map back onto a
    // *fraction* of each source line, so the browser's numbers and the
    // headless ones would be shares of two different denominators and
    // could not be merged. Unminified, both measure the same lines.
    //
    // The same caveat as the kernel's: this is the same source, built
    // another way.
    minify: process.env.GECKO_COVERAGE ? false : "esbuild",
  },
});
