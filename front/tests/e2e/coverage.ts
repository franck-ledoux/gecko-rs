/**
 * The browser half of the page's coverage.
 *
 * V8 counts what the page actually ran, and the bundle's source maps put
 * those counts back onto `src/*.ts`. So this measures the **gestures**: what
 * a click, a drag and a key press reach, which no unit test can speak for.
 *
 * Off unless `GECKO_COVERAGE` is set, and then the `page` fixture is wrapped
 * rather than every test being rewritten. `./coverage` sets it; `./check.sh`
 * does not, so the ordinary suite runs exactly as it did.
 *
 * Each worker is its own process, so each one *adds* to a shared cache and
 * the teardown merges them. That is monocart's own arrangement for running
 * in parallel, and the reason the report is generated there rather than here.
 */

import { test as base, expect } from "@playwright/test";
import { CoverageReport, type CoverageReportOptions } from "monocart-coverage-reports";

export const COLLECTING = !!process.env.GECKO_COVERAGE;

export const options: CoverageReportOptions = {
  name: "the page, in a browser",
  outputDir: "coverage/e2e",
  reports: [["lcovonly", { file: "lcov.info" }], "v8", "console-summary"],
  // Ours, and not what a toolchain wrote for us: the two wasm modules come
  // with their own glue, which nobody here can cover or should try to.
  sourceFilter: (path: string) =>
    path.search(/src\//) !== -1 && path.search(/src\/(pkg|lua)\//) === -1,
  // Every worker adds to the same cache; only the teardown clears it.
  cleanCache: false,
};

export const test = COLLECTING
  ? base.extend({
      page: async ({ page }, use) => {
        await page.coverage.startJSCoverage({ resetOnNavigation: false });
        await use(page);
        const entries = await page.coverage.stopJSCoverage();
        await new CoverageReport(options).add(entries);
      },
    })
  : base;

export { expect };
