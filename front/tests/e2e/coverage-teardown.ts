/**
 * Merge what every worker collected, once, at the end.
 *
 * A no-op when coverage was not asked for, so the same configuration serves
 * both runs.
 */

import { CoverageReport } from "monocart-coverage-reports";
import { COLLECTING, options } from "./coverage";

export default async function teardown() {
  if (!COLLECTING) return;
  const report = await new CoverageReport(options).generate();
  // Printed rather than asserted: the floor lives in `./coverage`, which
  // owns both numbers and knows what to do about them.
  if (report) console.log(`page lines: ${report.summary?.lines?.pct ?? "?"}%`);
}
