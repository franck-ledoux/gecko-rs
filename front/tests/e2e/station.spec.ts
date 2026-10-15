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
import { Station } from "./station";


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
    await expect(page.locator("#status")).toHaveText("changed from Python");
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
    const { stats } = await station.ask<{ stats: Record<string, unknown> }>("stats");
    expect(stats).toEqual({
      nodes: 8,
      edges: 12,
      faces: 6,
      blocks: 1,
      geometry: { generation: 0 },
      history: { steps: 5, back: 1 },
      mesh: { generation: 0 },
      classified: { nodes: 0, edges: 0, faces: 0, frozen: 0 },
    });
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
    await expect(page.locator("#status")).toHaveText("changed from Python");
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

/** The square, as one JSON-safe line. */
const SQUARE_MSH = [
  "$MeshFormat",
  "2.2 0 8",
  "$EndMeshFormat",
  "$Nodes",
  "4",
  "1 0 0 0",
  "2 1 0 0",
  "3 1 1 0",
  "4 0 1 0",
  "$EndNodes",
  "$Elements",
  "10",
  "1 15 2 0 21 1",
  "2 15 2 0 22 2",
  "3 15 2 0 23 3",
  "4 15 2 0 24 4",
  "5 1 2 0 11 1 2",
  "6 1 2 0 12 2 3",
  "7 1 2 0 13 3 4",
  "8 1 2 0 14 4 1",
  "9 2 2 0 1 1 2 3",
  "10 2 2 0 1 1 3 4",
  "$EndElements",
].join("\n");

test("a geometry loaded from Python appears in a browser that never asked", async ({ page }) => {
  // The generation number earning its keep: this tab is told only that
  // *something* changed, and works out for itself that the something was
  // the geometry and that it has to fetch the big buffer again.
  const station = await Station.start();
  try {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto(station.url);
    await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
    expect(await page.evaluate(() => (window as any).__gecko.geomCount("surfaces"))).toBe(0);

    const reply = await station.ask<{ ok: boolean; message: string }>(
      JSON.stringify({ op: "load_geometry", msh: SQUARE_MSH, name: "square.msh" }),
    );
    expect(reply.ok, reply.message).toBe(true);

    await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
    await expect(page.locator("[data-test=geom-curves]")).toHaveText("4");
    await page.waitForFunction(() => (window as any).__gecko.geomCount("corners") === 4);
    expect(errors).toEqual([]);
  } finally {
    station.stop();
  }
});
