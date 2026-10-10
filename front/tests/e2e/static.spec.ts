/**
 * Static mode: the page carries its own kernel. No server, no Python.
 */

import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

/**
 * The two faces looking at each other: the closest pair with one from each
 * block. Evaluated in the page, and said that way rather than by coordinate,
 * so no test quietly depends on where the button happens to put a cube.
 */
declare function facingPair(): number[];
declare function edgeNear(want: number[]): number;

const FACING_PAIR = `
window.facingPair = () => {
  const s = window.__gecko.surface("faces");
  const centre = (q) => {
    const at = [0, 0, 0];
    for (let k = 0; k < 4; k++)
      for (let axis = 0; axis < 3; axis++) at[axis] += s.positions[(q * 4 + k) * 3 + axis] / 4;
    return at;
  };
  let best = [0, 0];
  let shortest = Infinity;
  for (let a = 0; a < s.quads; a++)
    for (let b = 0; b < s.quads; b++) {
      if (s.blocks[a] === s.blocks[b]) continue;
      const [p, q] = [centre(a), centre(b)];
      const gap = Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]);
      if (gap < shortest) { shortest = gap; best = [s.faces[a], s.faces[b]]; }
    }
  return best;
};`;


const CUBE = [
  [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
  [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1],
];

/**
 * Open the settings drawer.
 *
 * It is shut unless asked for — the window belongs to the model — so a test
 * that reaches for a setting has to open it first, exactly as a reader does.
 */
async function openDrawer(page: import("@playwright/test").Page) {
  if (!(await page.evaluate(() => (window as any).__gecko.chrome().drawer))) {
    await page.locator("#settings").click();
  }
  await expect(page.locator("#drawer")).toBeVisible();
}

async function boot(page: import("@playwright/test").Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await page.goto("/");
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
  await page.evaluate(FACING_PAIR);
  return errors;
}

test("boots with its own kernel and nothing else", async ({ page }) => {
  const errors = await boot(page);
  await expect(page.locator("[data-test=mode]")).toHaveText("static");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  expect(errors, "no console errors, no wasm panic").toEqual([]);
});

test("creating a cube reaches the kernel and comes back to the scene", async ({ page }) => {
  await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.revision() > 1);

  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("[data-test=nodes]")).toHaveText("8");
  await expect(page.locator("[data-test=edges]")).toHaveText("12");
  await expect(page.locator("[data-test=faces]")).toHaveText("6");

  // And the scene really drew it, rather than the panel merely saying so.
  expect(await page.evaluate(() => (window as any).__gecko.counts())).toEqual({
    nodes: 8,
    edges: 12,
  });

  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
});

test("a refused command is reported and changes nothing", async ({ page }) => {
  await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.revision() > 1);

  const reply = await page.evaluate(
    () => (window as any).__gecko.apply({ op: "nonesuch" }) as Promise<{ ok: boolean }>,
  );
  expect(reply.ok).toBe(false);
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
});

test("the model is rebased on its own centre", async ({ page }) => {
  await boot(page);
  await page.evaluate(
    (cube) => (window as any).__gecko.apply({ op: "add_block", corners: cube }),
    CUBE.map((p) => [p[0] + 1e6, p[1], p[2]]),
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  const centre = await page.evaluate(() => (window as any).__gecko.centre());
  expect(centre[0]).toBeCloseTo(1e6 + 0.5, 3);
});

test("dragging a node with the mouse moves it in the kernel", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

  // Screen coordinates are read off the camera's transform matrix, which the
  // *next* render updates. Reading them before the reframe has been drawn
  // aims the mouse with a stale matrix — which is right often enough to look
  // like it works.
  await page.evaluate(() => (window as any).__gecko.settled());
  const name = (await page.evaluate(() => (window as any).__gecko.names()))[4] as number;
  const before = await page.evaluate(
    (n) => (window as any).__gecko.nodePosition(n),
    name,
  );
  const at = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    name,
  )) as [number, number];

  // A real gesture: press on the sphere, move, release. One command is sent,
  // at the drop.
  const camera = await page.evaluate(() => (window as any).__gecko.camera());

  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  for (let i = 1; i <= 8; i++) {
    await page.mouse.move(box.x + at[0] + i * 10, box.y + at[1] - i * 6);
  }
  await page.mouse.up();
  await page.waitForFunction(() => (window as any).__gecko.message().includes("moved"));

  // Dragging a node is not orbiting. Babylon suspends the controls of
  // `scene.activeCamera`, which with two cameras is whichever rendered last —
  // so the view used to turn along with the node.
  //
  // Where the camera *is*, not everything the camera reports: `closest` is
  // derived from the size of the model, and a node that moved made the model
  // a different size. That one is supposed to follow.
  const stayed = await page.evaluate(() => (window as any).__gecko.camera());
  expect(stayed.alpha).toBe(camera.alpha);
  expect(stayed.beta).toBe(camera.beta);
  expect(stayed.radius).toBe(camera.radius);
  expect(stayed.closest).not.toBe(camera.closest);

  // And it went where the mouse went. The same confusion fed the ray the drag
  // follows, so the node used to travel along a plane belonging to the corner
  // gizmo instead of to the view.
  const landed = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    name,
  )) as [number, number];
  // Within a pixel or two: the kernel holds f64 and the view is drawn from
  // f32, and nothing else should be between the pointer and where it landed.
  expect(Math.abs(landed[0] - (at[0] + 80))).toBeLessThan(2);
  expect(Math.abs(landed[1] - (at[1] - 48))).toBeLessThan(2);

  // The scene is redrawn from the kernel after the drop, so reading it back
  // is reading what the kernel holds.
  const after = await page.evaluate(
    (n) => (window as any).__gecko.nodePosition(n),
    name,
  );
  expect(after).not.toEqual(before);
  // Only that node moved: the block still has eight of them and twelve edges.
  expect(await page.evaluate(() => (window as any).__gecko.counts())).toEqual({
    nodes: 8,
    edges: 12,
  });
  expect(errors).toEqual([]);
});

test("colours can be changed, and are remembered", async ({ page }) => {
  const errors = await boot(page);
  await openDrawer(page);

  // The panel is generated from the theme's keys, so there is one input per
  // colour and no markup to keep in step: background, nodes, edges, faces,
  // blocks, and the geometry's corners, curves and surfaces.
  await expect(page.locator("#swatches input[type=color]")).toHaveCount(8);
  expect(await page.evaluate(() => (window as any).__gecko.theme())).toMatchObject({
    background: "#15171b",
  });

  await page.locator("#preset-light").click();
  const light = await page.evaluate(() => (window as any).__gecko.theme());
  expect(light.background).toBe("#f4f5f7");

  await page.locator("[data-test=colour-nodes]").fill("#ff00aa");
  await page.waitForFunction(
    () => (window as any).__gecko.theme().nodes === "#ff00aa",
  );

  // A viewing preference belongs to the viewer, so it survives a reload —
  // and it never went near the kernel.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
  const kept = await page.evaluate(() => (window as any).__gecko.theme());
  expect(kept).toMatchObject({ background: "#f4f5f7", nodes: "#ff00aa" });
  expect(errors).toEqual([]);
});

test("the triad turns the view, and can be hidden", async ({ page }) => {
  await boot(page);
  await openDrawer(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  expect(await page.evaluate(() => (window as any).__gecko.axesVisible())).toBe(true);

  // Aim a real mouse at the +X handle in the corner.
  const at = (await page.evaluate(
    () => (window as any).__gecko.axisScreen("+x"),
  )) as [number, number];
  expect(at, "the +x handle should be on screen").not.toBeNull();

  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + at[0], box.y + at[1]);

  // It swings rather than snapping, so wait for it to arrive: on +X the
  // camera looks at the YZ plane.
  await page.waitForFunction(
    () => {
      const c = (window as any).__gecko.camera();
      return Math.abs(c.alpha) < 0.01 && Math.abs(c.beta - Math.PI / 2) < 0.01;
    },
    undefined,
    { timeout: 5000 },
  );

  // Hidden means out of the way, not merely invisible.
  await page.locator("#show-axes").uncheck();
  expect(await page.evaluate(() => (window as any).__gecko.axesVisible())).toBe(false);
  expect(
    await page.evaluate(
      (p) => (window as any).__gecko.pickAxis(p[0], p[1]),
      at,
    ),
  ).toBeNull();

  // And that choice is remembered too.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
  expect(await page.evaluate(() => (window as any).__gecko.axesVisible())).toBe(false);
});

test("faces and blocks are drawn, coloured and faded from the panel", async ({ page }) => {
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

  const surface = (group: string) =>
    page.evaluate((g) => (window as any).__gecko.surface(g), group);

  // Faces by default; blocks are the same six quadrangles read the other way,
  // so showing them too is a choice rather than something to default to.
  expect(await surface("faces")).toMatchObject({ quads: 6, colours: 1 });
  expect(await surface("blocks")).toBeNull();

  // One colour per face. Six faces, six colours — and the kernel was not
  // asked anything: these come off the names already in the view buffer.
  await page.locator("[data-test=faces-id]").check();
  expect(await surface("faces")).toMatchObject({ quads: 6, colours: 6 });

  // Both at once, each with its own colouring and its own transparency.
  await page.locator("[data-test=blocks-visible]").check();
  await page.locator("[data-test=blocks-uniform]").check();
  expect(await surface("blocks")).toMatchObject({ quads: 6, colours: 1 });
  expect(await surface("faces")).toMatchObject({ colours: 6 });

  await page.locator("[data-test=faces-alpha]").fill("0.2");
  await page.waitForFunction(
    () => (window as any).__gecko.surface("faces").alpha < 0.21,
  );
  // One slider per group, so the other one does not move with it.
  expect((await surface("blocks")).alpha).toBeCloseTo(0.55, 6);
  expect(errors).toEqual([]);
});

test("each kind of cell shrinks towards its own centre", async ({ page }) => {
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("[data-test=blocks-visible]").check();
  // Blocks come up slightly shrunk, because that is what makes them legible
  // over the faces they are drawn from. Start both at full size, so what the
  // numbers below say about shrinking is about shrinking.
  await page.locator("[data-test=blocks-shrink]").fill("1");
  await page.waitForFunction(() => (window as any).__gecko.display().blocks.shrink === 1);

  const extent = async (group: string) =>
    page.evaluate((g) => {
      const p = (window as any).__gecko.surface(g).positions as number[];
      let lo = Infinity;
      let hi = -Infinity;
      for (let i = 0; i < p.length; i += 3) {
        lo = Math.min(lo, p[i]);
        hi = Math.max(hi, p[i]);
      }
      return hi - lo;
    }, group);

  // The unit cube, as wide as it is.
  expect(await extent("faces")).toBeCloseTo(1, 5);
  expect(await extent("blocks")).toBeCloseTo(1, 5);

  // A block pulls towards one point, so the whole thing narrows.
  await page.locator("[data-test=blocks-shrink]").fill("0.5");
  await page.waitForFunction(() => {
    const p = (window as any).__gecko.surface("blocks").positions as number[];
    return Math.max(...p.map(Math.abs)) < 0.3;
  });
  expect(await extent("blocks")).toBeCloseTo(0.5, 5);
  // And the faces, which were not asked to move, did not.
  expect(await extent("faces")).toBeCloseTo(1, 5);

  // A face pulls towards its own centre, which is in its own plane: the cube
  // keeps its width while every face shrinks inside it.
  await page.locator("[data-test=faces-shrink]").fill("0.5");
  await page.waitForFunction(
    () => (window as any).__gecko.display().faces.shrink === 0.5,
  );
  expect(await extent("faces")).toBeCloseTo(1, 5);
  expect(errors).toEqual([]);
});

test("the display settings survive a reload", async ({ page }) => {
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("[data-test=blocks-visible]").check();
  await page.locator("[data-test=faces-id]").check();
  await page.locator("[data-test=edges-shrink]").fill("0.8");
  await page.waitForFunction(
    () => (window as any).__gecko.display().edges.shrink === 0.8,
  );

  await page.reload();
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });

  // Still a viewing preference, so it is still the browser's business and
  // never went near the kernel.
  const kept = await page.evaluate(() => (window as any).__gecko.display());
  expect(kept.blocks.visible).toBe(true);
  expect(kept.faces.colouring).toBe("id");
  expect(kept.edges.shrink).toBe(0.8);
  // And the controls came back showing it.
  await expect(page.locator("[data-test=blocks-visible]")).toBeChecked();
  await expect(page.locator("[data-test=faces-id]")).toBeChecked();
  expect(errors).toEqual([]);
});

test("every handle stays grabbable through an opaque surface", async ({ page }) => {
  // All eight, not a node picked for being likely: Babylon's pointer system
  // returns the nearest pickable mesh, so when the surfaces joined that
  // contest it was exactly the handles **behind** one that stopped working —
  // and which ones those are depends on where the camera happens to be.
  //
  // A real drag each time. Asking `pickNode` would prove nothing: it passes
  // a predicate of its own, and so never travels the path that matters.
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("[data-test=blocks-visible]").check();
  await page.locator("[data-test=faces-alpha]").fill("1");
  await page.evaluate(() => (window as any).__gecko.settled());

  const names = (await page.evaluate(() => (window as any).__gecko.names())) as number[];
  expect(names).toHaveLength(8);
  const box = (await page.locator("#view").boundingBox())!;

  for (const name of names) {
    const before = (await page.evaluate(
      (n) => (window as any).__gecko.nodePosition(n),
      name,
    )) as number[];
    const at = (await page.evaluate(
      (n) => (window as any).__gecko.nodeScreen(n),
      name,
    )) as [number, number];

    await page.mouse.move(box.x + at[0], box.y + at[1]);
    await page.mouse.down();
    for (let i = 1; i <= 4; i++) {
      await page.mouse.move(box.x + at[0] + i * 8, box.y + at[1] - i * 5);
    }
    await page.mouse.up();

    await page
      .waitForFunction(
        ([n, was]) => {
          const p = (window as any).__gecko.nodePosition(n) as number[];
          return p.some((x, k) => Math.abs(x - (was as number[])[k]) > 1e-4);
        },
        [name, before] as const,
        { timeout: 5000 },
      )
      .catch(() => {
        throw new Error(`handle ${name} could not be dragged`);
      });
    await page.evaluate(() => (window as any).__gecko.settled());
  }

  // Dragged, never selected: a handle is a handle even with a face over it.
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);
  expect(errors).toEqual([]);
});

test("zooming in stops at the model instead of going through it", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const camera = () => page.evaluate(() => (window as any).__gecko.camera());
  const framed = await camera();
  expect(framed.closest).toBeGreaterThan(0);
  expect(framed.closest).toBeLessThan(framed.radius);

  // A real wheel, hard, far more than anyone would turn it.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 60; i++) await page.mouse.wheel(0, -120);
  await page.evaluate(() => (window as any).__gecko.settled());
  await page.waitForTimeout(500);

  const close = await camera();
  // It got closer, and it stopped short. With a fixed step per notch the
  // radius sails past the target and the view turns inside out.
  expect(close.radius).toBeLessThan(framed.radius);
  expect(close.radius).toBeGreaterThanOrEqual(close.closest - 1e-6);
  expect(errors).toEqual([]);
});

test("two faces are picked with the mouse and glued", async ({ page }) => {
  const errors = await boot(page);
  // Two cubes side by side, sharing the plane x = 1.5 — the button spaces
  // them, so they are glued across the gap rather than at a touching face.
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Nothing to glue until two faces are chosen.
  await expect(page.locator("#glue")).toBeDisabled();

  const facing = (await page.evaluate(() => facingPair())) as number[];
  expect(facing[0]).not.toBe(facing[1]);

  const box = (await page.locator("#view").boundingBox())!;
  for (const face of facing) {
    const at = (await page.evaluate((f) => (window as any).__gecko.faceScreen(f), face)) as
      | [number, number]
      | null;
    expect(at, `face ${face} should be on screen`).not.toBeNull();
    await page.mouse.click(box.x + at![0], box.y + at![1]);
  }

  // Both picked, in order, and the panel says which one the other comes to.
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(2);
  await expect(page.locator("#status")).toContainText("will be glued onto");
  await expect(page.locator("#glue")).toBeEnabled();

  await page.locator("#glue").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

  // Four pairs of nodes became four nodes: the blocks are one object now.
  expect(await page.evaluate(() => (window as any).__gecko.counts())).toEqual({
    nodes: 12,
    edges: 20,
  });
  await expect(page.locator("[data-test=faces]")).toHaveText("11");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  // And the selection is gone: those two names mean something else now.
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);
  await expect(page.locator("#glue")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("dragging from a face turns the view instead of selecting it", async ({ page }) => {
  // Orbiting starts wherever the pointer happens to be, which is usually on
  // the model. A press-move-release must not also count as a pick.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const face = (await page.evaluate(() => (window as any).__gecko.surface("faces").faces[0])) as number;
  const at = (await page.evaluate((f) => (window as any).__gecko.faceScreen(f), face)) as [
    number,
    number,
  ];
  const box = (await page.locator("#view").boundingBox())!;
  const before = await page.evaluate(() => (window as any).__gecko.camera());

  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  for (let i = 1; i <= 6; i++) await page.mouse.move(box.x + at[0] + i * 12, box.y + at[1]);
  await page.mouse.up();

  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);
  const after = await page.evaluate(() => (window as any).__gecko.camera());
  expect(after.alpha).not.toBeCloseTo(before.alpha, 3);
  expect(errors).toEqual([]);
});

test("gluing leaves the camera exactly where it was", async ({ page }) => {
  // A glue welds two blocks you are already looking at. Reframing on it was
  // a conflation: the kernel's `viewChanged` says the bytes are stale, which
  // is a different question from whether the camera should move.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Put the camera somewhere deliberate, so "unchanged" means something.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 60, box.y + box.height / 2 + 30);
  await page.mouse.up();
  await page.mouse.wheel(0, -240);
  // Babylon keeps applying inertia after a gesture, so "unchanged" means
  // nothing unless the drift is stopped first. Waiting for it to decay is a
  // flake under load; stopping it is not.
  await page.evaluate(() => (window as any).__gecko.stopCamera());
  await page.evaluate(() => (window as any).__gecko.settled());
  const before = await page.evaluate(() => (window as any).__gecko.camera());

  const facing = (await page.evaluate(() => facingPair())) as number[];
  await page.evaluate(
    (f) => (window as any).__gecko.choose({ face: f[0], block: 0 }),
    facing,
  );
  await page.evaluate(
    (f) => (window as any).__gecko.choose({ face: f[1], block: 1 }),
    facing,
  );
  await page.locator("#glue").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

  const after = await page.evaluate(() => (window as any).__gecko.camera());
  expect(after.alpha).toBe(before.alpha);
  expect(after.beta).toBe(before.beta);
  expect(after.radius).toBe(before.radius);
  expect(errors).toEqual([]);
});

test("recentring shows everything without changing the direction", async ({ page }) => {
  const errors = await boot(page);
  for (let i = 0; i < 3; i++) {
    await page.locator("#cube").click();
    await page.waitForFunction((n) => (window as any).__gecko.counts().nodes === n, (i + 1) * 8);
  }
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 };

  // Look at it from somewhere deliberate — not the angle it starts at, or
  // "unchanged" would be indistinguishable from "reset to the default".
  await page.mouse.move(centre.x, centre.y);
  await page.mouse.down();
  await page.mouse.move(centre.x + 110, centre.y + 40);
  await page.mouse.up();
  // Then lose the model by zooming right in.
  for (let i = 0; i < 40; i++) await page.mouse.wheel(0, -120);
  await page.evaluate(() => (window as any).__gecko.stopCamera());
  await page.evaluate(() => (window as any).__gecko.settled());
  const before = await page.evaluate(() => (window as any).__gecko.camera());
  expect(Math.abs(before.alpha + Math.PI / 3)).toBeGreaterThan(0.05);

  const offScreen = async () =>
    page.evaluate(() => {
      const g = (window as any).__gecko;
      const r = (document.querySelector("#view") as HTMLCanvasElement).getBoundingClientRect();
      return (g.names() as number[]).filter((n) => {
        const at = g.nodeScreen(n) as [number, number] | null;
        return !at || at[0] < 0 || at[1] < 0 || at[0] > r.width || at[1] > r.height;
      }).length;
    });
  expect(await offScreen(), "the zoom should have lost some of the model").toBeGreaterThan(0);

  // Record every frame's radius while it moves. Sampling at one chosen
  // moment instead would be a bet on how fast the machine is — and on a slow
  // one the eased start has barely left the ground.
  await page.evaluate(() => {
    const w = window as any;
    w.__radii = [];
    const tick = () => {
      w.__radii.push(w.__gecko.camera().radius);
      if (w.__radii.length < 200) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.locator("#recentre").click();
  await page.evaluate(() => (window as any).__gecko.settled());
  await expect(page.locator("#status")).toHaveText("view recentred");

  // It glided: a cut goes straight from one radius to the other and leaves
  // nothing in between, however the frames happen to fall.
  const after = await page.evaluate(() => (window as any).__gecko.camera());
  const between = (await page.evaluate(() => (window as any).__radii)) as number[];
  const partWay = between.filter((r) => r > before.radius + 1e-6 && r < after.radius - 1e-6);
  // One is the whole claim: it did not get there in a single step. A cut
  // leaves exactly none however the frames fall, so this still fails on one
  // — and asking for more was a bet on the frame rate that a CI runner
  // drawing seven frames in 450 ms, most of them at the eased ends, lost.
  expect(partWay.length, `radii seen: ${between.join(", ")}`).toBeGreaterThanOrEqual(1);

  // Everything in view, and looked at from exactly where it was.
  expect(await offScreen()).toBe(0);
  expect(after.alpha).toBeCloseTo(before.alpha, 9);
  expect(after.beta).toBeCloseTo(before.beta, 9);
  expect(after.radius).toBeGreaterThan(before.radius);
  expect(errors).toEqual([]);
});

test("a block is picked and removed", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  await page.evaluate(() => (window as any).__gecko.settled());

  await expect(page.locator("#remove")).toBeDisabled();

  // One click is enough: a pick knows which block's side it landed on.
  const face = (await page.evaluate(() => facingPair()))[0] as number;
  const at = (await page.evaluate((f) => (window as any).__gecko.faceScreen(f), face)) as [
    number,
    number,
  ];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + at[0], box.y + at[1]);

  await expect(page.locator("#remove")).toBeEnabled();
  await expect(page.locator("#status")).toContainText("block");

  await page.locator("#remove").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("[data-test=faces]")).toHaveText("6");
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);
  await expect(page.locator("#remove")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("the toolbar and the keyboard are the same thing", async ({ page }) => {
  // One definition of each action, reached two ways. A button whose
  // shortcut did something subtly different would be the worst of both.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

  await page.keyboard.press("c");
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);

  // And the keys that are not actions are left alone.
  await page.keyboard.press("q");
  expect(await page.evaluate(() => (window as any).__gecko.counts().nodes)).toBe(16);
  expect(errors).toEqual([]);
});

test("the interface folds away, and remembers that it did", async ({ page }) => {
  const errors = await boot(page);
  // Shut to begin with: the window belongs to the model.
  expect(await page.evaluate(() => (window as any).__gecko.chrome())).toMatchObject({
    drawer: false,
    hidden: false,
  });
  await expect(page.locator("#drawer")).toBeHidden();

  await page.keyboard.press("Tab");
  await expect(page.locator("#drawer")).toBeVisible();
  await expect(page.locator("#settings")).toHaveAttribute("aria-expanded", "true");

  // Everything away, toolbar and readout included.
  await page.keyboard.press("Shift+Tab");
  await expect(page.locator("#bar")).toBeHidden();
  await expect(page.locator("#readout")).toBeHidden();
  await page.keyboard.press("Shift+Tab");
  await expect(page.locator("#bar")).toBeVisible();

  await page.reload();
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
  await expect(page.locator("#drawer")).toBeVisible();
  expect(errors).toEqual([]);
});

test("the help card lists the keys, and any key dismisses it", async ({ page }) => {
  const errors = await boot(page);
  await expect(page.locator("#help")).toBeHidden();

  await page.keyboard.press("h");
  await expect(page.locator("#help")).toBeVisible();
  // Written from the same table the keys are read from, so it cannot lie.
  await expect(page.locator("#help-card")).toContainText("create a cube");
  await expect(page.locator("#help-card kbd").first()).toHaveText("C");

  // Any key, not a particular one: a card you have to guess your way out of
  // is a trap. And the key that dismissed it does nothing else.
  await page.keyboard.press("c");
  await expect(page.locator("#help")).toBeHidden();
  expect(await page.evaluate(() => (window as any).__gecko.counts().nodes)).toBe(0);
  expect(errors).toEqual([]);
});

test("typing in a field is typing, not a shortcut", async ({ page }) => {
  // The colour inputs and the sliders take characters and arrow keys. A
  // slider that jumped to a new view instead of nudging would be unusable.
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("[data-test=faces-alpha]").focus();
  await page.keyboard.press("c");
  expect(await page.evaluate(() => (window as any).__gecko.counts().nodes)).toBe(0);

  // Tab from inside the drawer walks its fields rather than shutting it.
  await page.keyboard.press("Tab");
  await expect(page.locator("#drawer")).toBeVisible();
  expect(errors).toEqual([]);
});

/** Width and height out of a PNG's header, which is all this needs to read. */
function pngSize(bytes: Buffer): { width: number; height: number } {
  expect(bytes.subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
  expect(bytes.subarray(12, 16).toString()).toBe("IHDR");
  return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

test("a picture is saved, at the size the drawer asks for", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const canvas = await page.evaluate(() => {
    const c = document.querySelector("#view") as HTMLCanvasElement;
    return { width: c.width, height: c.height };
  });

  const download = page.waitForEvent("download");
  await page.keyboard.press("s");
  const saved = await download;

  // Named by the moment, so the files sort the way they were taken.
  expect(saved.suggestedFilename()).toMatch(/^gecko-\d{4}-\d{2}-\d{2}-\d{6}\.png$/);
  const where = await saved.path();
  const size = pngSize(await readFile(where));
  // Twice the canvas by default: a figure in a paper wants more pixels than
  // a window has.
  expect(size).toEqual({ width: canvas.width * 2, height: canvas.height * 2 });

  await expect(page.locator("#status")).toHaveText("picture saved");
  expect(errors).toEqual([]);
});

test("a picture can be saved with nothing behind it", async ({ page }) => {
  // For dropping a figure onto a page whose colour you do not know yet.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const cornerAlpha = (transparent: boolean) =>
    page.evaluate(async (t) => {
      const url = await (window as any).__gecko.capture({ scale: 1, transparent: t });
      const image = new Image();
      await new Promise((done) => {
        image.onload = done;
        image.src = url;
      });
      const scratch = document.createElement("canvas");
      scratch.width = image.width;
      scratch.height = image.height;
      const flat = scratch.getContext("2d")!;
      flat.drawImage(image, 0, 0);
      return flat.getImageData(1, 1, 1, 1).data[3];
    }, transparent);

  expect(await cornerAlpha(false)).toBe(255);
  expect(await cornerAlpha(true)).toBe(0);
  // And the scene is left as it was found, whatever the capture did to it.
  expect(await page.evaluate(() => (window as any).__gecko.theme().background)).toBe("#15171b");
  expect(errors).toEqual([]);
});

/** The name of an edge whose midpoint is nearest this model-space point. */
const EDGE_NEAR = `
window.edgeNear = (want) => {
  const g = window.__gecko;
  const names = g.edgeNames();
  let best = null, shortest = Infinity;
  for (const name of names) {
    const mid = g.edgeMidpoint(name);
    const d = Math.hypot(mid[0] - want[0], mid[1] - want[1], mid[2] - want[2]);
    if (d < shortest) { shortest = d; best = name; }
  }
  return best;
};`;

test("clicking an edge lights up its sheet", async ({ page }) => {
  // Nothing is cut yet. The sheet is what a cut would touch, shown before
  // there is a cut — because the one thing that cannot be checked after the
  // fact is whether the sheet was the right set of edges.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  const at = (await page.evaluate((e) => (window as any).__gecko.edgeScreen(e), edge)) as [
    number,
    number,
  ];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + at[0], box.y + at[1]);

  // Four parallel edges on a lone cube, and the one clicked is among them.
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  const lit = (await page.evaluate(() => (window as any).__gecko.sheet())) as number[];
  expect(lit).toHaveLength(4);
  expect(lit).toContain(edge);
  await expect(page.locator("#status")).toContainText("4 edges");

  // Clicking it again puts it out.
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 0);
  expect(errors).toEqual([]);
});

/**
 * How many pixels on the canvas are the lit-sheet yellow.
 *
 * A ribbon's area is its length times its width, and only its length is
 * supposed to change with distance — which is what makes a count of pixels
 * enough to tell the two apart.
 */
async function litPixels(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const flat = document.createElement("canvas");
    flat.width = view.width;
    flat.height = view.height;
    flat.getContext("2d")!.drawImage(view, 0, 0);
    const pixels = flat.getContext("2d")!.getImageData(0, 0, view.width, view.height).data;
    let lit = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
      if (r > 200 && g > 150 && g < 230 && b < 120) lit++;
    }
    return lit;
  });
}

test("a lit sheet is the same thickness however far away it is", async ({ page }) => {
  // The sheet is something you are being shown, not part of the model, so
  // its width belongs to the screen. Drawn in scene units instead it reads
  // as a measure of where the camera happens to be — and every recentring
  // moves the camera, which is how this was noticed.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

  // Creating a cube glides the camera into place, and that glide writes to
  // `radius` every frame. Setting a distance while it runs is setting a
  // value the animation is about to overwrite — which is how this test read
  // the same distance twice and still looked like it worked.
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 4);

  const at = async (radius: number) => {
    await page.evaluate((r) => (window as any).__gecko.setRadius(r), radius);
    await page.evaluate(() => (window as any).__gecko.settled());
    expect(await page.evaluate(() => (window as any).__gecko.camera().radius)).toBeCloseTo(
      radius,
      3,
    );
    return litPixels(page);
  };
  const near = await at(6);
  const far = await at(12);

  // Twice as far away halves the sheet's *length* on screen and nothing
  // else, so the pixels it covers halve. A width in scene units would halve
  // too, and the count would come down by four — which is the whole of the
  // difference between the two, and why the number to assert is a ratio.
  expect(near, `near ${near}, far ${far}`).toBeGreaterThan(400);
  expect(near / far, `near ${near}, far ${far}`).toBeGreaterThan(1.6);
  expect(near / far, `near ${near}, far ${far}`).toBeLessThan(2.6);
  expect(errors).toEqual([]);
});

test("a sheet runs on through a glued block", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  await page.evaluate(() => (window as any).__gecko.settled());

  const facing = (await page.evaluate(() => facingPair())) as number[];
  await page.evaluate((f) => (window as any).__gecko.choose({ face: f[0], block: 0 }), facing);
  await page.evaluate((f) => (window as any).__gecko.choose({ face: f[1], block: 1 }), facing);
  await page.locator("#glue").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);
  await page.evaluate(() => (window as any).__gecko.settled());

  // An edge lying in the seam, parallel to it: its sheet crosses into the
  // second block. Four and four, sharing the two that lie in the seam.
  const seam = (await page.evaluate(() => edgeNear([1, 0.5, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), seam);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  expect(await page.evaluate(() => (window as any).__gecko.sheet())).toHaveLength(6);

  // And one that does not lie in the seam stays in the block it started in.
  const away = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), away);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 4);

  // Esc clears it, as it clears a face selection.
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 0);
  expect(errors).toEqual([]);
});

test("dragging along an edge cuts the whole sheet", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  const at = (await page.evaluate((e) => (window as any).__gecko.edgeScreen(e), edge)) as [
    number,
    number,
  ];
  const box = (await page.locator("#view").boundingBox())!;
  const camera = await page.evaluate(() => (window as any).__gecko.camera());

  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  // Pressing lights the sheet; nothing is cut yet.
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 4);
  expect(await page.evaluate(() => (window as any).__gecko.counts().nodes)).toBe(8);

  for (let i = 1; i <= 6; i++) await page.mouse.move(box.x + at[0], box.y + at[1] - i * 12);
  // Aimed, and said, and still nothing cut.
  const aimed = (await page.evaluate(() => (window as any).__gecko.aimedAt())) as number;
  expect(aimed).toBeGreaterThan(0);
  expect(aimed).toBeLessThan(1);
  await expect(page.locator("#status")).toContainText("let go to cut");
  expect(await page.evaluate(() => (window as any).__gecko.counts().nodes)).toBe(8);

  // Aiming is not orbiting: the pointer was taken off the camera.
  const during = await page.evaluate(() => (window as any).__gecko.camera());
  expect(during.alpha).toBe(camera.alpha);
  expect(during.beta).toBe(camera.beta);

  await page.mouse.up();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

  // One cube became two blocks, sharing a face.
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("[data-test=faces]")).toHaveText("11");
  expect(await page.evaluate(() => (window as any).__gecko.counts())).toEqual({
    nodes: 12,
    edges: 20,
  });
  // The sheet is gone with the edge it was named by.
  expect(await page.evaluate(() => (window as any).__gecko.sheet())).toEqual([]);
  expect(await page.evaluate(() => (window as any).__gecko.aimedAt())).toBeNull();
  expect(errors).toEqual([]);
});

test("pressing an edge without moving selects it rather than cutting", async ({ page }) => {
  // A cut cannot be undone, and a click is cheap to make by accident.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  const at = (await page.evaluate((e) => (window as any).__gecko.edgeScreen(e), edge)) as [
    number,
    number,
  ];
  const box = (await page.locator("#view").boundingBox())!;

  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 4);
  expect(await page.evaluate(() => (window as any).__gecko.counts().nodes)).toBe(8);

  // And clicking it again puts the sheet out, still without cutting.
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 0);
  expect(await page.evaluate(() => (window as any).__gecko.counts().nodes)).toBe(8);
  expect(errors).toEqual([]);
});

test("pillowing lays a layer and leaves the surface where it was", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  // The highest face, chosen the way the eye would.
  const top = (await page.evaluate(() => {
    const s = (window as any).__gecko.surface("faces");
    let best = 0;
    let highest = -Infinity;
    for (let q = 0; q < s.quads; q++) {
      let z = 0;
      for (let k = 0; k < 4; k++) z += s.positions[(q * 4 + k) * 3 + 2] / 4;
      if (z > highest) {
        highest = z;
        best = q;
      }
    }
    return { face: s.faces[best], block: s.blocks[best] };
  })) as { face: number; block: number };

  const before = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return (g.names() as number[]).map((n) => g.nodePosition(n)[2]).sort();
  })) as number[];

  await expect(page.locator("#pillow")).toBeDisabled();
  await page.evaluate((t) => (window as any).__gecko.choose(t), top);
  await expect(page.locator("#pillow")).toBeEnabled();
  await page.locator("#pillow").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("[data-test=faces]")).toHaveText("11");

  // The whole point: the model's surface has not moved. Every height that
  // was there is still there, with one new layer between them.
  const after = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return (g.names() as number[]).map((n) => g.nodePosition(n)[2]);
  })) as number[];
  for (const z of before) {
    expect(after.some((w) => Math.abs(w - z) < 1e-6), `height ${z} went missing`).toBe(true);
  }
  expect(new Set(after.map((z) => z.toFixed(6))).size).toBe(3);
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);
  expect(errors).toEqual([]);
});

test("the selection grows as far as you like", async ({ page }) => {
  // Two is what a glue wants; a pillow takes as many as are picked, so the
  // third click no longer throws the first two away.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const faces = (await page.evaluate(() => {
    const s = (window as any).__gecko.surface("faces");
    return [0, 1, 2].map((q) => ({ face: s.faces[q], block: s.blocks[q] }));
  })) as { face: number; block: number }[];

  for (const f of faces) await page.evaluate((t) => (window as any).__gecko.choose(t), f);
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(3);
  // Three is too many to glue and fine to pillow.
  await expect(page.locator("#glue")).toBeDisabled();
  await expect(page.locator("#pillow")).toBeEnabled();
  // All three lit, which is what tells you what the layer will cover.
  expect(await page.evaluate(() => (window as any).__gecko.surface("faces").colours)).toBe(3);

  // Clicking one again takes it out rather than starting over.
  await page.evaluate((t) => (window as any).__gecko.choose(t), faces[0]);
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(2);
  await expect(page.locator("#glue")).toBeEnabled();
  expect(errors).toEqual([]);
});

/** The colour in the middle of the canvas, as three numbers. */
async function centrePixel(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const flat = document.createElement("canvas");
    flat.width = view.width;
    flat.height = view.height;
    flat.getContext("2d")!.drawImage(view, 0, 0);
    const at = flat
      .getContext("2d")!
      .getImageData(Math.floor(view.width / 2), Math.floor(view.height / 2), 1, 1).data;
    return [at[0], at[1], at[2]] as [number, number, number];
  });
}

const apart = (a: number[], b: number[]) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);

test("the nearer of two translucent blocks is the one you see", async ({ page }) => {
  // Which of them is nearer has to follow the camera. It cannot when every
  // cell is in one mesh: Babylon sorts transparent *meshes* back to front
  // and leaves the triangles inside one in the order they were built, so
  // the same cell wins the blend from either side.
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("[data-test=faces-id]").check();
  await page.locator("[data-test=faces-alpha]").fill("0.6");

  // One cube, seen from +x: this is what that cube looks like on its own.
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.snapTo("+x"));
  await page.evaluate(() => (window as any).__gecko.settled());
  const alone = await centrePixel(page);

  // A second cube at a greater x, so it stands between that one and the
  // camera. Along the row they cover each other exactly.
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  await page.evaluate(() => (window as any).__gecko.snapTo("+x"));
  await page.evaluate(() => (window as any).__gecko.settled());
  const behindIt = await centrePixel(page);

  // From the far side the first cube is the near one again.
  await page.evaluate(() => (window as any).__gecko.snapTo("-x"));
  await page.evaluate(() => (window as any).__gecko.settled());
  const inFront = await centrePixel(page);

  // No threshold to pick: whichever cube is nearer dominates, so the view
  // that has the first one in front must land closer to how it looks alone.
  expect(
    apart(inFront, alone),
    `alone ${alone}, behind ${behindIt}, in front ${inFront}`,
  ).toBeLessThan(apart(behindIt, alone));
  expect(errors).toEqual([]);
});

test("a lit sheet is what the remove button takes away", async ({ page }) => {
  // The bin acts on what is selected: faces mean a block, a lit sheet means
  // the whole layer. One button rather than a ninth icon for a verb that is
  // the same verb.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Cut it in two, so there is a sheet worth removing.
  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  const at = (await page.evaluate((e) => (window as any).__gecko.edgeScreen(e), edge)) as [
    number,
    number,
  ];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  for (let i = 1; i <= 6; i++) await page.mouse.move(box.x + at[0], box.y + at[1] - i * 10);
  await page.mouse.up();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

  // Light the sheet across the upper slab and take it out.
  await expect(page.locator("#remove")).toBeDisabled();
  // The sheet across the upper slab, told apart by its size: four edges
  // stay inside one slab, and the ones that run along the cut go through
  // both. Picking the highest edge alone would catch one of those.
  const across = (await page.evaluate(async () => {
    const g = (window as any).__gecko;
    let best: number | null = null;
    let highest = -Infinity;
    for (const n of g.edgeNames() as number[]) {
      await g.pickEdgeByName(n);
      if (g.sheet().length === 4 && g.edgeMidpoint(n)[2] > highest) {
        highest = g.edgeMidpoint(n)[2];
        best = n;
      }
    }
    return best;
  })) as number;
  await page.keyboard.press("Escape");
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), across);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  await expect(page.locator("#remove")).toBeEnabled();

  await page.locator("#remove").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

  // One block left, and nothing still lit.
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  expect(await page.evaluate(() => (window as any).__gecko.sheet())).toEqual([]);
  await expect(page.locator("#remove")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("clicking the background lets go of everything", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());
  const box = (await page.locator("#view").boundingBox())!;

  // A face: clicking away clears it.
  const face = (await page.evaluate(() => {
    const s = (window as any).__gecko.surface("faces");
    return { face: s.faces[0], block: s.blocks[0] };
  })) as { face: number; block: number };
  await page.evaluate((f) => (window as any).__gecko.choose(f), face);
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(1);
  await page.mouse.click(box.x + 20, box.y + box.height - 20);
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);

  // And a lit sheet, which used to stay lit because only the faces were
  // being let go of.
  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  await page.mouse.click(box.x + 20, box.y + box.height - 20);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 0);
  expect(await page.evaluate(() => (window as any).__gecko.selectedEdge())).toBeNull();
  await expect(page.locator("#remove")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("a lit sheet says how many blocks it would take", async ({ page }) => {
  // The edges alone do not say that, and taking out more than you meant to
  // looks like a hole in the grid.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await expect(page.locator("#status")).toContainText("across 1 block");
  expect(errors).toEqual([]);
});

/**
 * The square again, as a file: two triangles on one surface, four sides each
 * a curve of its own, four corners each a point element. Physical tag 0 and
 * the entity tag carrying the structure, which is how gmsh writes a model
 * with no physical groups declared.
 */
const SQUARE_MSH = `$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
4
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
$EndNodes
$Elements
10
1 15 2 0 21 1
2 15 2 0 22 2
3 15 2 0 23 3
4 15 2 0 24 4
5 1 2 0 11 1 2
6 1 2 0 12 2 3
7 1 2 0 13 3 4
8 1 2 0 14 4 1
9 2 2 0 1 1 2 3
10 2 2 0 1 1 3 4
$EndElements
`;

async function openSquare(page: import("@playwright/test").Page, msh = SQUARE_MSH) {
  await page.locator("#msh").setInputFiles({
    name: "square.msh",
    mimeType: "text/plain",
    buffer: Buffer.from(msh),
  });
}

test("a geometry is opened from a file and drawn in its three groups", async ({ page }) => {
  const errors = await boot(page);
  await openSquare(page);

  await expect(page.locator("#status")).toContainText("square.msh loaded");
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await expect(page.locator("[data-test=geom-curves]")).toHaveText("4");
  await expect(page.locator("[data-test=geom-corners]")).toHaveText("4");

  // And the scene really drew it, rather than the readout merely saying so.
  const count = (group: string) =>
    page.evaluate((g) => (window as any).__gecko.geomCount(g), group);
  expect(await count("surfaces")).toBe(1);
  expect(await count("curves")).toBe(4);
  expect(await count("corners")).toBe(4);
  expect(errors).toEqual([]);
});

test("each geometry group is hidden, coloured and faded on its own", async ({ page }) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-curves]")).toHaveText("4");
  await openDrawer(page);

  const count = (group: string) =>
    page.evaluate((g) => (window as any).__gecko.geomCount(g), group);
  const look = (group: string, tag: number) =>
    page.evaluate(([g, t]) => (window as any).__gecko.geomLook(g, t), [group, tag] as const);

  // Hidden from the drawer, and the other two groups stay.
  await page.locator('[data-test="curves-visible"]').uncheck();
  expect(await count("curves")).toBe(0);
  expect(await count("surfaces")).toBe(1);
  await page.locator('[data-test="curves-visible"]').check();
  expect(await count("curves")).toBe(4);

  // One colour for the lot, then one per tag.
  await page.locator('[data-test="curves-uniform"]').check();
  const uniform = await look("curves", 11);
  expect((await look("curves", 12))!.colour).toBe(uniform!.colour);
  await page.locator('[data-test="curves-id"]').check();
  expect((await look("curves", 12))!.colour).not.toBe((await look("curves", 11))!.colour);

  // And opacity, per group.
  await page.locator('[data-test="surfaces-alpha"]').fill("0.2");
  expect((await look("surfaces", 1))!.alpha).toBeCloseTo(0.2, 2);
  expect((await look("curves", 11))!.alpha).toBeCloseTo(1, 2);
  expect(errors).toEqual([]);
});

test("the geometry groups have keys of their own, and they are remembered", async ({ page }) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");

  const count = (group: string) =>
    page.evaluate((g) => (window as any).__gecko.geomCount(g), group);
  await page.keyboard.press("Digit4");
  expect(await count("surfaces")).toBe(0);
  await expect(page.locator("#status")).toContainText("geometric surfaces hidden");
  await page.keyboard.press("Digit5");
  expect(await count("curves")).toBe(0);
  await page.keyboard.press("Digit6");
  expect(await count("corners")).toBe(0);

  // Reloaded, the settings come back — and the geometry does not, because
  // it lives in the kernel and a static page's kernel goes with the tab.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(await page.evaluate(() => (window as any).__gecko.prefs().surfaces.visible)).toBe(false);
  expect(errors).toEqual([]);
});

test("a file that is not a gmsh mesh is refused and says why", async ({ page }) => {
  const errors = await boot(page);
  await openSquare(page, "hello, I am not a mesh\n");
  await expect(page.locator("#status")).toContainText("$MeshFormat");
  expect(await page.evaluate(() => (window as any).__gecko.geomCount("surfaces"))).toBe(0);

  // A modern gmsh writes 4.1 by default, so this is the message most likely
  // to be read in anger: it has to say what to do about it.
  await openSquare(page, SQUARE_MSH.replace("2.2 0 8", "4.1 0 8"));
  await expect(page.locator("#status")).toContainText("-format msh2");
  expect(errors).toEqual([]);
});

test("framing takes in the geometry as well as the blocks", async ({ page }) => {
  const errors = await boot(page);
  // The square sits at the origin; put a cube well away from it.
  await page.evaluate(() =>
    (window as any).__gecko.apply({ op: "create_cube", origin: [20, 0, 0], size: 1 }),
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await openSquare(page);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Far enough back to hold both, which is more than either alone needs.
  const radius = (await page.evaluate(() => (window as any).__gecko.camera().radius)) as number;
  expect(radius).toBeGreaterThan(20);
  expect(errors).toEqual([]);
});

/**
 * Where the picked face is on screen, and which way **out of the model** is
 * on screen from there.
 *
 * Read from the face opposite it: on a cube the line between the two face
 * centres is the normal, so the direction from the far one to the picked one
 * is outward, whatever angle the camera happens to be at. Guessing instead —
 * "up the screen is up the model" — is wrong here, because the scene's up is
 * Y and the model's is Z.
 */
const OUTWARD = `
window.outward = (picked) => {
  const s = window.__gecko.surface("faces");
  const centre = (q) => {
    const at = [0, 0, 0];
    for (let k = 0; k < 4; k++)
      for (let axis = 0; axis < 3; axis++) at[axis] += s.positions[(q * 4 + k) * 3 + axis] / 4;
    return at;
  };
  let mine = 0;
  for (let q = 0; q < s.quads; q++) if (s.faces[q] === picked) mine = q;
  let far = mine;
  const gap = (q) => {
    const [a, b] = [centre(q), centre(mine)];
    return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
  };
  for (let q = 0; q < s.quads; q++) if (gap(q) > gap(far)) far = q;
  const here = window.__gecko.faceScreen(picked);
  const behind = window.__gecko.faceScreen(s.faces[far]);
  const length = Math.hypot(here[0] - behind[0], here[1] - behind[1]) || 1;
  return { at: here, dir: [(here[0] - behind[0]) / length, (here[1] - behind[1]) / length] };
};`;

declare function outward(face: number): { at: [number, number]; dir: [number, number] };

/** The model's longest side, which an extrusion along a cube's face normal
 *  lengthens by exactly the height asked for. */
const SPAN = `
window.span = () => {
  const g = window.__gecko;
  const points = g.names().map((n) => g.nodePosition(n));
  return Math.max(...[0, 1, 2].map((k) => {
    const all = points.map((p) => p[k]);
    return Math.max(...all) - Math.min(...all);
  }));
};`;

declare function span(): number;

test("a selected face is dragged out into a new block", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(OUTWARD);
  await page.evaluate(SPAN);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Clicked first, which is what arms the gesture: an unselected face is
  // how you turn the view, and a face fills the screen. Which face it is
  // comes from the click rather than from this test's idea of the angle.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await page.waitForFunction(() => (window as any).__gecko.selection().length === 1);
  const picked = (await page.evaluate(() => (window as any).__gecko.selection()[0])) as {
    face: number;
  };
  const out = (await page.evaluate((f) => outward(f), picked.face)) as {
    at: [number, number];
    dir: [number, number];
  };

  const before = (await page.evaluate(() => (window as any).__gecko.camera())) as {
    alpha: number;
    beta: number;
  };
  await page.mouse.move(box.x + out.at[0], box.y + out.at[1]);
  await page.mouse.down();
  for (const step of [20, 50, 90]) {
    await page.mouse.move(box.x + out.at[0] + out.dir[0] * step, box.y + out.at[1] + out.dir[1] * step);
  }
  // The preview says how far before anything is built.
  const asked = (await page.evaluate(() => (window as any).__gecko.grownBy())) as number;
  expect(asked).toBeGreaterThan(0.2);
  await expect(page.locator("#status")).toContainText("let go to build");
  await page.mouse.up();

  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("[data-test=nodes]")).toHaveText("12");
  // Welded, not merely standing on it: eleven faces, not twelve.
  await expect(page.locator("[data-test=faces]")).toHaveText("11");

  // And it grew by what the preview promised, along the face's own normal —
  // which for a cube lengthens exactly one side of the model.
  expect((await page.evaluate(() => span())) as number).toBeCloseTo(1 + asked, 3);

  // The view did not turn while the face was being dragged.
  const after = (await page.evaluate(() => (window as any).__gecko.camera())) as {
    alpha: number;
    beta: number;
  };
  expect(after.alpha).toBeCloseTo(before.alpha, 6);
  expect(after.beta).toBeCloseTo(before.beta, 6);
  expect(errors).toEqual([]);
});

test("pressing a selected face without moving lets go of it", async ({ page }) => {
  // The gesture has to leave the plain click alone: press and release on
  // the spot is how a selection is undone, extrusion or no extrusion.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  const middle: [number, number] = [box.x + box.width / 2, box.y + box.height / 2];
  await page.mouse.click(...middle);
  await page.waitForFunction(() => (window as any).__gecko.selection().length === 1);

  await page.mouse.click(...middle);
  await page.waitForFunction(() => (window as any).__gecko.selection().length === 0);
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  expect(errors).toEqual([]);
});

/** A 2×1×3 box's surface, each of its six faces an entity of its own. */
const CUBE_MSH = `$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
8
1 0 0 0
2 2 0 0
3 2 1 0
4 0 1 0
5 0 0 3
6 2 0 3
7 2 1 3
8 0 1 3
$EndNodes
$Elements
12
1 2 2 0 1 1 2 3
2 2 2 0 1 1 3 4
3 2 2 0 2 5 6 7
4 2 2 0 2 5 7 8
5 2 2 0 3 1 2 6
6 2 2 0 3 1 6 5
7 2 2 0 4 4 3 7
8 2 2 0 4 4 7 8
9 2 2 0 5 1 4 8
10 2 2 0 5 1 8 5
11 2 2 0 6 2 3 7
12 2 2 0 6 2 7 6
$EndElements
`;

async function openCube(page: import("@playwright/test").Page) {
  await page.locator("#msh").setInputFiles({
    name: "cube.msh",
    mimeType: "text/plain",
    buffer: Buffer.from(CUBE_MSH),
  });
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("6");
  await page.evaluate(() => (window as any).__gecko.settled());
}

test("the geometry buttons wait for a geometry", async ({ page }) => {
  const errors = await boot(page);
  for (const id of ["picking", "box", "box-inertia"]) {
    await expect(page.locator(`#${id}`)).toBeDisabled();
  }
  await openCube(page);
  for (const id of ["picking", "box", "box-inertia"]) {
    await expect(page.locator(`#${id}`)).toBeEnabled();
  }
  expect(errors).toEqual([]);
});

test("aiming at the geometry picks surfaces, and the blocks stay out of it", async ({ page }) => {
  const errors = await boot(page);
  await openCube(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  const middle: [number, number] = [box.x + box.width / 2, box.y + box.height / 2];

  // Blocks first: a click in the middle takes a face of the cube, not the
  // geometry in front of it.
  await page.mouse.click(...middle);
  await page.waitForFunction(() => (window as any).__gecko.selection().length === 1);
  expect(await page.evaluate(() => (window as any).__gecko.surfaceSelection())).toEqual([]);

  // Switched over, and the same click takes a surface instead.
  await page.keyboard.press("m");
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe("geometry");
  await expect(page.locator("#picking")).toHaveAttribute("aria-pressed", "true");
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);

  await page.mouse.click(...middle);
  await page.waitForFunction(() => (window as any).__gecko.surfaceSelection().length === 1);
  const [tag] = (await page.evaluate(() =>
    (window as any).__gecko.surfaceSelection(),
  )) as number[];
  await expect(page.locator("#status")).toContainText("1 surface selected");
  // And it is drawn in the selection colour, like a picked face.
  expect(
    (await page.evaluate((t) => (window as any).__gecko.geomLook("surfaces", t), tag)) as {
      colour: string;
    },
  ).toMatchObject({ colour: "#ffcc33" });
  // Nothing of the blocking answered the click.
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);

  // Clicking it again puts it out, and the background lets go of the lot.
  await page.mouse.click(...middle);
  await page.waitForFunction(() => (window as any).__gecko.surfaceSelection().length === 0);
  expect(errors).toEqual([]);
});

test("a box is built round the geometry, or round what is selected", async ({ page }) => {
  const errors = await boot(page);
  await openCube(page);

  // Nothing selected: the whole geometry, with the kernel's own margin.
  await page.locator("#box").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("#status")).toContainText("the whole geometry");
  await expect(page.locator("#status")).toContainText("square to the world");

  // The shape's own axes, from the second button.
  await page.locator("#box-inertia").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("#status")).toContainText("its own axes");

  // And one round a selection, which has to be two surfaces at least or
  // the box has no thickness.
  await page.keyboard.press("m");
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.selectSurfaces([1, 2]);
  });
  await page.keyboard.press("b");
  await expect(page.locator("[data-test=blocks]")).toHaveText("3");
  await expect(page.locator("#status")).toContainText("2 surfaces");
  expect(errors).toEqual([]);
});

test("a box that cannot be built says why and builds nothing", async ({ page }) => {
  const errors = await boot(page);
  await openCube(page);
  await page.keyboard.press("m");
  await page.evaluate(() => (window as any).__gecko.selectSurfaces([1]));
  await page.keyboard.press("b");
  await expect(page.locator("#status")).toContainText("flat");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  expect(errors).toEqual([]);
});

test("each group of cells is its own box, not nested inside the last", async ({ page }) => {
  // An unclosed `<fieldset>` is not a parse error — it is a fieldset that
  // swallows whatever comes after it. The three boxes came out one inside
  // the next, which reads as though edges were part of blocks were part of
  // faces, and they are nothing of the sort.
  const errors = await boot(page);
  await openDrawer(page);

  await expect(page.locator("#layers > fieldset")).toHaveCount(3);
  expect(await page.locator("#layers fieldset fieldset").count()).toBe(0);
  await expect(page.locator("#geometry > fieldset")).toHaveCount(3);
  expect(await page.locator("#geometry fieldset fieldset").count()).toBe(0);

  // Side by side, so they all start at the same place on screen.
  const lefts = await page
    .locator("#layers > fieldset")
    .evaluateAll((all) => all.map((f) => f.getBoundingClientRect().left));
  expect(new Set(lefts).size).toBe(1);
  expect(errors).toEqual([]);
});
