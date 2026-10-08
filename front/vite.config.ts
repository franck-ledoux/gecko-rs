import { defineConfig } from "vite";

/**
 * When `GECKO_PORT` is set, the dev server stands in front of a running
 * station: it serves the TypeScript with hot reload and forwards the two
 * things the page asks of its origin — the mode probe and the socket — to the
 * Python process.
 *
 * That is what lets a TypeScript edit reload the page **without losing the
 * kernel's state**: the kernel is in Python, on the other side of the proxy,
 * and knows nothing about the reload.
 */
const station = process.env.GECKO_PORT;

export default defineConfig({
  // Relative, so the same build works at a site root, in a subdirectory, and
  // served from the station's own little server.
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
    proxy: station
      ? {
          "/mode": { target: `http://127.0.0.1:${station}` },
          "/ws": { target: `ws://127.0.0.1:${station}`, ws: true },
        }
      : undefined,
  },
  build: { target: "es2022", outDir: "dist", sourcemap: true },
});
