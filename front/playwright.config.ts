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
export default defineConfig({
  testDir: "tests/e2e",
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
