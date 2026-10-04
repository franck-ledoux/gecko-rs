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
      JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }),
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

    await page.locator("#cube").click();
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

    await first.locator("#cube").click();
    await expect(first.locator("[data-test=blocks]")).toHaveText("1");
    // The second tab asked for nothing, and must still be told.
    await expect(second.locator("[data-test=blocks]")).toHaveText("1");
  } finally {
    await first.close();
    await second.close();
    station.stop();
  }
});

test("a node dragged in the browser lands in the Python session", async ({ page }) => {
  const station = await Station.start();
  try {
    await station.ask(JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }));
    await page.goto(station.url);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    await expect(page.locator("[data-test=nodes]")).toHaveText("8");

    // Screen coordinates are read off the camera's transform matrix, which the
    // *next* render updates. Reading them before the reframe has been drawn
    // aims the mouse with a stale matrix — which is right often enough to look
    // like it works.
    await page.evaluate(() => (window as any).__gecko.settled());
    const name = (await page.evaluate(() => (window as any).__gecko.names()))[2] as number;
    const at = (await page.evaluate(
      (n) => (window as any).__gecko.nodeScreen(n),
      name,
    )) as [number, number];

    const box = (await page.locator("#view").boundingBox())!;
    await page.mouse.move(box.x + at[0], box.y + at[1]);
    await page.mouse.down();
    for (let i = 1; i <= 8; i++) {
      await page.mouse.move(box.x + at[0] + i * 9, box.y + at[1] - i * 7);
    }
    await page.mouse.up();
    await page.waitForFunction(() => (window as any).__gecko.message().includes("moved"));

    // The drag happened in a browser; the node lives in a Python process. They
    // have to agree on where it ended up — to the precision the wire carries,
    // not to the last bit: the view is f32 by design and the kernel is the
    // authority in f64, so a round trip through the buffer is lossy on purpose.
    const shown = (await page.evaluate(
      (n) => (window as any).__gecko.nodePosition(n),
      name,
    )) as number[];
    const { nodes } = await station.ask<{ nodes: { id: number; pos: number[] }[] }>("nodes");
    const held = nodes.find((n) => n.id === name)!;
    expect(held.pos).toHaveLength(3);
    for (let k = 0; k < 3; k++) {
      expect(held.pos[k]).toBeCloseTo(shown[k], 5);
    }
    // And it really moved: the node did not start at the origin corner.
    expect(Math.hypot(...held.pos)).toBeGreaterThan(0.1);
  } finally {
    station.stop();
  }
});

test("a node moved from Python moves in the browser", async ({ page }) => {
  const station = await Station.start();
  try {
    await station.ask(JSON.stringify({ op: "create_cube", origin: [0, 0, 0], size: 1 }));
    await page.goto(station.url);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });

    const name = (await page.evaluate(() => (window as any).__gecko.names()))[0] as number;
    const before = await page.evaluate(
      (n) => (window as any).__gecko.nodePosition(n),
      name,
    );

    await station.ask(JSON.stringify({ op: "move_node", node: name, to: [5, 6, 7] }));

    // Nothing was clicked and nothing reloaded.
    await expect(page.locator("#status")).toHaveText("modifié depuis Python");
    await page.waitForFunction(
      (n) => {
        const p = (window as any).__gecko.nodePosition(n);
        return p && p[0] === 5 && p[1] === 6 && p[2] === 7;
      },
      name,
      { timeout: 10_000 },
    );
    expect(before).not.toEqual([5, 6, 7]);
  } finally {
    station.stop();
  }
});
