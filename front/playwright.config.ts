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
  /**
   * One retry on a runner, none here.
   *
   * Not to make a flaky suite look green: a retried test is reported as
   * flaky, its trace from the first attempt is kept, and it is a thing to
   * go and fix. It is about what a single blip costs. On a shared runner
   * the suite is several times slower than it is here, apt falls over, a
   * worker stalls — and today every one of those costs a push, a wait of
   * twenty minutes and a re-run started by hand. Rerunning the one test
   * that fell over costs seconds.
   *
   * None locally, on purpose. A flake you can see in front of you is a
   * flake you fix.
   */
  retries: collecting || process.env.CI ? 1 : 0,
  /**
   * A minute a test, which is four times the slowest one here.
   *
   * It was thirty seconds, and thirty seconds is not the margin it sounds
   * like: the two slowest tests take nine on this machine, and both of them
   * spend it on round trips rather than on work — sixty wheel notches, one
   * message each, while a software-rendered browser draws a frame between
   * them. A shared runner is several times slower at exactly that, and one
   * of the two timed out there while passing everywhere else, with nothing
   * wrong in it.
   *
   * A passing suite takes no longer for this. Only a test that is genuinely
   * stuck waits, and that one was never going to be reported quickly.
   */
  timeout: collecting ? 120_000 : 60_000,
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
