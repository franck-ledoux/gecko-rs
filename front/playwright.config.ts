import { defineConfig, devices } from "@playwright/test";

/**
 * Software rendering (SwiftShader) rather than whatever GPU happens to be
 * present: slower, and reproducible on a laptop and on a runner with no GPU
 * alike, which matters more.
 *
 * `webServer` serves the **built** bundle, so the thing under test is the
 * thing that ships — the page, with the kernel and the interpreter inside
 * it. Nothing else is running: there is no server to have.
 */
/**
 * Collecting V8 coverage makes every page slower and every worker heavier:
 * the counters are read and the source maps resolved once per test. Eight
 * instrumented browsers at once simply queue, so a coverage run takes fewer
 * workers and gives each test longer — it is not a run anybody waits on.
 */
const collecting = !!process.env.GECKO_COVERAGE;

export default defineConfig({
  testDir: "tests/e2e",
  // One worker: two instrumented browsers reading counters and resolving
  // source maps over the same cache wedge each other, and a coverage run is
  // not a run anybody waits on. One retry, because a wedged browser is a
  // wedged browser and not a finding.
  workers: collecting ? 1 : undefined,
  retries: collecting ? 1 : 0,
  timeout: collecting ? 120_000 : 30_000,
  // Merges what each worker collected, and does nothing at all unless
  // `GECKO_COVERAGE` is set.
  globalTeardown: "./tests/e2e/coverage-teardown.ts",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? "github" : "list",
  webServer: {
    command: "npm run build && npm run preview",
    url: "http://localhost:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
  use: {
    baseURL: "http://localhost:4173",
    trace: "retain-on-failure",
    launchOptions: {
      args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
    },
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
  ],
});
