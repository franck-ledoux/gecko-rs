/**
 * Station mode: the kernel is in a Python process, and the page is a view of
 * it.
 *
 * This is the mode's reason to exist, so the test asks it directly: a command
 * typed on the Python side must reach the browser **without the browser having
 * asked for anything**. And the link is two-way — a click in the page must be
 * visible to the same Python session, because there is only one of them.
 */

import { expect, test } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const CUBE = [
  [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
  [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1],
];

const PYTHON = fileURLToPath(new URL("../../../.venv/bin/python", import.meta.url));
const DRIVER = fileURLToPath(new URL("../station.py", import.meta.url));

/** A station, and a line-by-line conversation with it. */
class Station {
  private lines: string[] = [];
  private waiting: Array<(line: string) => void> = [];

  private constructor(
    readonly process: ChildProcessWithoutNullStreams,
    readonly url: string,
  ) {}

  static async start(): Promise<Station> {
    const child = spawn(PYTHON, [DRIVER], { stdio: ["pipe", "pipe", "pipe"] });
    let stderr = "";
    child.stderr.on("data", (d) => (stderr += d));
    const station = new Station(child, await Station.firstLine(child, () => stderr));
    createInterface({ input: child.stdout }).on("line", (line) => station.receive(line));
    return station;
  }

  private static firstLine(
    child: ChildProcessWithoutNullStreams,
    stderr: () => string,
  ): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = createInterface({ input: child.stdout });
      reader.once("line", (line) => {
        reader.close();
        resolve(line.trim());
      });
      child.once("exit", (code) =>
        reject(new Error(`the station exited with ${code}: ${stderr()}`)),
      );
    });
  }

  private receive(line: string) {
    const next = this.waiting.shift();
    if (next) next(line);
    else this.lines.push(line);
  }

  async ask<T>(request: string): Promise<T> {
    const reply = new Promise<string>((resolve) => {
      const buffered = this.lines.shift();
      if (buffered) resolve(buffered);
      else this.waiting.push(resolve);
    });
    this.process.stdin.write(`${request}\n`);
    return JSON.parse(await reply) as T;
  }

  stop() {
    this.process.stdin.end("quit\n");
    this.process.kill();
  }
}

test("a command typed in Python reaches the browser unasked", async ({ page }) => {
  const station = await Station.start();
  try {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(station.url);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });

    // The page found a station and connected to it, rather than falling back
    // to a kernel of its own.
    await expect(page.locator("[data-test=mode]")).toHaveText("station");
    await expect(page.locator("[data-test=blocks]")).toHaveText("0");

    // Python moves the kernel. Nothing is clicked, nothing is reloaded.
    const reply = await station.ask<{ ok: boolean; message: string }>(
      JSON.stringify({ op: "add_block", corners: CUBE }),
    );
    expect(reply.ok, reply.message).toBe(true);

    await expect(page.locator("[data-test=blocks]")).toHaveText("1");
    await expect(page.locator("[data-test=nodes]")).toHaveText("8");
    await expect(page.locator("#status")).toHaveText("modifié depuis Python");
    expect(await page.evaluate(() => (window as any).__gecko.counts())).toEqual({
      nodes: 8,
      edges: 12,
    });
    expect(errors).toEqual([]);
  } finally {
    station.stop();
  }
});

test("a click in the browser is visible to the same Python session", async ({ page }) => {
  const station = await Station.start();
  try {
    await page.goto(station.url);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });

    await page.locator("#add").click();
    await expect(page.locator("[data-test=blocks]")).toHaveText("1");

    // One kernel, two clients: Python must see what the mouse did.
    const { stats } = await station.ask<{ stats: Record<string, number> }>("stats");
    expect(stats).toEqual({ nodes: 8, edges: 12, faces: 6, blocks: 1 });
  } finally {
    station.stop();
  }
});

test("two tabs on one station see each other's work", async ({ browser }) => {
  const station = await Station.start();
  const first = await browser.newPage();
  const second = await browser.newPage();
  try {
    for (const page of [first, second]) {
      await page.goto(station.url);
      await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    }

    await first.locator("#add").click();
    await expect(first.locator("[data-test=blocks]")).toHaveText("1");
    // The second tab asked for nothing, and must still be told.
    await expect(second.locator("[data-test=blocks]")).toHaveText("1");
  } finally {
    await first.close();
    await second.close();
    station.stop();
  }
});
