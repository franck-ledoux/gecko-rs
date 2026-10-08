/**
 * The development loop: `./dev`.
 *
 * The page is served by Vite, which forwards the mode probe and the socket to
 * a Python station behind it. That arrangement is the only reason a TypeScript
 * edit can reload the view **without losing the kernel's state** — the kernel
 * is on the far side of the proxy and never hears about the reload.
 *
 * It is also the only part of the loop with a mechanism in it rather than
 * convenience, so it is the part worth a test.
 */

import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";
import { Station } from "./station";

// Each test in this file owns a dev server on a fixed port, so they cannot
// overlap. Serial rather than a port per test: the constraint is that a dev
// server is a singleton, and saying so is clearer than working around it.
test.describe.configure({ mode: "serial" });

// Deliberately not 8731/5173: a developer may well have `./dev` running. And a
// pair per test rather than one pair shared — a dev server that has been asked
// to stop has not necessarily released its port, and the next test would
// happily latch onto it while it still points at a station that is gone.
const PORTS = [
  { station: 8799, vite: 5174 },
  { station: 8800, vite: 5175 },
  { station: 8801, vite: 5176 },
];

const FRONT = fileURLToPath(new URL("../..", import.meta.url));

/**
 * Start a dev server in front of a station, and wait until it is really
 * forwarding.
 *
 * Waiting for the index page is not enough: any leftover server answers that.
 * The mode probe only answers `station` once this server is up *and* reaching
 * the station behind it, which is the thing being tested.
 */
async function startVite(ports: { station: number; vite: number }): Promise<ChildProcess> {
  const vite = await serveOnly(ports);
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    try {
      const probe = await fetch(`http://localhost:${ports.vite}/mode`);
      if (probe.ok && (await probe.text()).trim() === "station") return vite;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  vite.kill();
  throw new Error(`no dev server forwarding to :${ports.station}`);
}

/**
 * The dev server alone, waiting only for itself.
 *
 * This is the arrangement `./dev` really produces for a moment: a dev server
 * up, the Python session still starting. The page is served, and the station
 * behind the proxy is not there yet.
 */
async function serveOnly(ports: { station: number; vite: number }): Promise<ChildProcess> {
  const vite = spawn(
    "npx",
    ["vite", "--port", String(ports.vite), "--strictPort", "--clearScreen", "false"],
    {
      cwd: FRONT,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, GECKO_PORT: String(ports.station) },
    },
  );
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    try {
      if ((await fetch(`http://localhost:${ports.vite}/`)).ok) return vite;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  vite.kill();
  throw new Error(`no dev server on :${ports.vite}`);
}

async function stop(vite: ChildProcess) {
  if (vite.exitCode !== null) return;
  const ended = new Promise((r) => vite.once("exit", r));
  vite.kill();
  await Promise.race([ended, new Promise((r) => setTimeout(r, 5000))]);
}

test("the dev server stands in front of a live Python session", async ({ page }) => {
  const ports = PORTS[0];
  const station = await Station.start(ports.station);
  const vite = await startVite(ports);
  try {
    await station.ask(JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }));

    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(`http://localhost:${ports.vite}/`);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });

    // Served by Vite, yet in station mode: the mode probe was forwarded.
    await expect(page.locator("[data-test=mode]")).toHaveText("station");
    await expect(page.locator("[data-test=blocks]")).toHaveText("1");

    // And the socket was forwarded too, in both directions.
    await page.locator("#cube").click();
    await expect(page.locator("[data-test=blocks]")).toHaveText("2");
    const { stats } = await station.ask<{ stats: Record<string, number> }>("stats");
    expect(stats.blocks).toBe(2);

    await station.ask(JSON.stringify({ op: "create_cube", origin: [4, 0, 0], size: 1 }));
    await expect(page.locator("[data-test=blocks]")).toHaveText("3");
    expect(errors).toEqual([]);
  } finally {
    await stop(vite);
    station.stop();
  }
});

test("a reload leaves the session untouched", async ({ page }) => {
  // The point of proxying rather than embedding: the page can be thrown away
  // and rebuilt — which is what a hot reload does — and the kernel does not
  // notice.
  const ports = PORTS[1];
  const station = await Station.start(ports.station);
  const vite = await startVite(ports);
  try {
    await station.ask(JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }));
    await page.goto(`http://localhost:${ports.vite}/`);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    await page.locator("#cube").click();
    await expect(page.locator("[data-test=blocks]")).toHaveText("2");

    await page.reload();
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    await expect(page.locator("[data-test=blocks]")).toHaveText("2");
    await expect(page.locator("[data-test=mode]")).toHaveText("station");
  } finally {
    await stop(vite);
    station.stop();
  }
});

test("a tab opened before the station waits for it", async ({ page }) => {
  // The order `./dev` used to produce, and the browser won the race every
  // time. The probe is forwarded, so with no station behind it the proxy
  // answers 5xx — which is not the same thing as "no station here". Taken for
  // one, the tab starts a kernel of its own and then looks entirely healthy
  // while ignoring everything typed in the Python console.
  const ports = PORTS[2];
  const vite = await serveOnly(ports);
  let station: Station | undefined;
  try {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(`http://localhost:${ports.vite}/`);

    await page.waitForSelector("body[data-waiting='1']", { timeout: 30_000 });
    // Waiting, not running: no kernel has been chosen.
    expect(await page.evaluate(() => document.body.dataset.ready)).toBeUndefined();
    expect(await page.evaluate(() => (window as any).__gecko)).toBeUndefined();

    station = await Station.start(ports.station);
    await station.ask(JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }));

    // No reload: the page picks the station up by itself.
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    await expect(page.locator("[data-test=mode]")).toHaveText("station");
    await expect(page.locator("[data-test=blocks]")).toHaveText("1");
    expect(await page.evaluate(() => document.body.dataset.waiting)).toBeUndefined();
    expect(errors).toEqual([]);
  } finally {
    await stop(vite);
    station?.stop();
  }
});
