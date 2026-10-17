import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // NullEngine needs no DOM, so these run in plain Node: no jsdom, no
    // browser, no GPU. That is what keeps them in the sub-second range.
    environment: "node",
    include: ["tests/unit/**/*.test.ts"],
    // Half of the page's coverage — `./coverage` merges it with what the
    // browser suite reaches, because neither alone is the page. Collected
    // only when asked for: the provider instruments every module, and these
    // tests are quick for a reason.
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "lcov"],
      reportsDirectory: "coverage/unit",
      include: ["src/**/*.ts"],
      // `pkg` and `lua` are wasm-bindgen's and emscripten's output: machine
      // written, and not ours to cover.
      exclude: ["src/pkg/**", "src/lua/**"],
    },
  },
});
