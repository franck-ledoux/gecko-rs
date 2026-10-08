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

// Deliberately not 8731/5173: a developer may well have `./dev` running.
const STATION_PORT = 8799;
const VITE_PORT = 5174;
const VITE_URL = `http://localhost:${VITE_PORT}/`;

const FRONT = fileURLToPath(new URL("../..", import.meta.url));

async function startVite(stationPort: number): Promise<ChildProcess> {
  const vite = spawn(
    "npx",
    ["vite", "--port", String(VITE_PORT), "--strictPort", "--clearScreen", "false"],
    { cwd: FRONT, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, GECKO_PORT: String(stationPort) } },
  );
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    try {
      const probe = await fetch(VITE_URL);
      if (probe.ok) return vite;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  vite.kill();
  throw new Error("the dev server never came up");
}

test("the dev server stands in front of a live Python session", async ({ page }) => {
  const station = await Station.start(STATION_PORT);
  const vite = await startVite(STATION_PORT);
  try {
    await station.ask(JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }));

    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(VITE_URL);
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
    vite.kill();
    station.stop();
  }
});

test("a reload leaves the session untouched", async ({ page }) => {
  // The point of proxying rather than embedding: the page can be thrown away
  // and rebuilt — which is what a hot reload does — and the kernel does not
  // notice.
  const station = await Station.start(STATION_PORT + 1);
  const vite = await startVite(STATION_PORT + 1);
  try {
    await station.ask(JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }));
    await page.goto(VITE_URL);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    await page.locator("#cube").click();
    await expect(page.locator("[data-test=blocks]")).toHaveText("2");

    await page.reload();
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    await expect(page.locator("[data-test=blocks]")).toHaveText("2");
    await expect(page.locator("[data-test=mode]")).toHaveText("station");
  } finally {
    vite.kill();
    station.stop();
  }
});
