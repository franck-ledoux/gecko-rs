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
  /**
   * Two, and one while collecting coverage.
   *
   * Playwright's own default is half the cores, which on a sixteen-core
   * machine is eight browsers — and each of these is a software-rendered
   * browser pulling two wasm modules from one `vite preview`. Past a couple,
   * workers stop getting slower and start getting **stuck**: eight of them
   * left eight tests waiting the entire fifteen minutes of a run for pages
   * that normally load in a second, while their neighbours passed in two.
   * Four left three or four stuck. One is reliable.
   *
   * Fewer is not slower overall here, and CI runs on two cores where this
   * changes nothing. Collecting coverage goes down to one and gets a retry:
   * reading counters and resolving source maps is heavier still.
   */
  workers: collecting ? 1 : 2,
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
