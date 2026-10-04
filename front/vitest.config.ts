import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // NullEngine needs no DOM, so these run in plain Node: no jsdom, no
    // browser, no GPU. That is what keeps them in the sub-second range.
    environment: "node",
    include: ["tests/unit/**/*.test.ts"],
  },
});
