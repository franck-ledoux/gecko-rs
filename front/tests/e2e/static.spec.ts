/**
 * The page, end to end: it carries its own kernel and its own interpreter,
 * and needs no server for either.
 */

import { expect, test } from "./coverage";
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
  const center = (q) => {
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
      const [p, q] = [center(a), center(b)];
      const gap = Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]);
      if (gap < shortest) { shortest = gap; best = [s.faces[a], s.faces[b]]; }
    }
  return best;
};`;

const CUBE = [
  [0, 0, 0],
  [1, 0, 0],
  [1, 1, 0],
  [0, 1, 0],
  [0, 0, 1],
  [1, 0, 1],
  [1, 1, 1],
  [0, 1, 1],
];

/**
 * Open the settings drawer.
 *
 * It is shut unless asked for — the window belongs to the model — so a test
 * that reaches for a setting has to open it first, exactly as a reader does.
 */
async function openDrawer(page: import("@playwright/test").Page) {
  if (!(await page.evaluate(() => (window as any).__gecko.chrome().drawer))) {
    await page.locator("#display").click();
  }
  await expect(page.locator("#drawer")).toBeVisible();
}

/** The other panel: what the operations do, as against how they look. */
async function openParams(page: import("@playwright/test").Page) {
  if (!(await page.evaluate(() => (window as any).__gecko.chrome().params))) {
    await page.locator("#settings").click();
  }
  await expect(page.locator("#params")).toBeVisible();
}

/** Unfold part of the display drawer, which mostly starts folded: a section
 *  by its name, a group of cells by `grp-…`. */
async function unfold(
  page: import("@playwright/test").Page,
  id: string,
  handle = "> summary",
) {
  await openDrawer(page);
  const fold = page.locator(`#${id}`);
  if (!(await fold.evaluate((d) => (d as HTMLDetailsElement).open))) {
    await fold.locator(handle).click();
  }
  await expect(fold).toHaveAttribute("open", "");
}

/** One group of cells or of the geometry. Clicked on its name, which is the
 *  one part of the line that is neither the box nor the digest. */
const openGroup = (page: import("@playwright/test").Page, group: string) =>
  unfold(page, `grp-${group}`, "summary .name");

const openSection = (page: import("@playwright/test").Page, name: string) =>
  unfold(page, `sec-${name}`);

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
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  expect(errors, "no console errors, no wasm panic").toEqual([]);
});

test("creating a cube reaches the kernel and comes back to the scene", async ({
  page,
}) => {
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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
});

test("a refused command is reported and changes nothing", async ({ page }) => {
  await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.revision() > 1);

  const reply = await page.evaluate(
    () =>
      (window as any).__gecko.apply({ op: "nonesuch" }) as Promise<{
        ok: boolean;
      }>,
  );
  expect(reply.ok).toBe(false);
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
});

test("the model is rebased on its own center", async ({ page }) => {
  await boot(page);
  await page.evaluate(
    (cube) => (window as any).__gecko.apply({ op: "add_block", corners: cube }),
    CUBE.map((p) => [p[0] + 1e6, p[1], p[2]]),
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  const center = await page.evaluate(() => (window as any).__gecko.center());
  expect(center[0]).toBeCloseTo(1e6 + 0.5, 3);
});

test("dragging a node with the mouse moves it in the kernel", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  // Screen coordinates are read off the camera's transform matrix, which the
  // *next* render updates. Reading them before the reframe has been drawn
  // aims the mouse with a stale matrix — which is right often enough to look
  // like it works.
  await page.evaluate(() => (window as any).__gecko.settled());
  const name = (
    await page.evaluate(() => (window as any).__gecko.names())
  )[4] as number;
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
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("moved"),
  );

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

test("colors can be changed, and are remembered", async ({ page }) => {
  const errors = await boot(page);
  await openDrawer(page);

  // The panel is generated from the theme's keys, so there is one input per
  // color and no markup to keep in step: background, nodes, edges, faces,
  // blocks, the geometry's corners, curves and surfaces, the mesh, the
  // three that say what a cell follows, and the halo. Being frozen is not
  // among them — that is a shape, not a color.
  await expect(page.locator("#swatches input[type=color]")).toHaveCount(13);
  expect(
    await page.evaluate(() => (window as any).__gecko.theme()),
  ).toMatchObject({
    background: "#15171b",
  });

  await openSection(page, "colors");
  await page.locator("#preset-light").click();
  const light = await page.evaluate(() => (window as any).__gecko.theme());
  expect(light.background).toBe("#f4f5f7");

  await page.locator("[data-test=color-nodes]").fill("#ff00aa");
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
  await openSection(page, "colors");
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  expect(await page.evaluate(() => (window as any).__gecko.axesVisible())).toBe(
    true,
  );

  // Aim a real mouse at the +X handle in the corner.
  const at = (await page.evaluate(() =>
    (window as any).__gecko.axisScreen("+x"),
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
  expect(await page.evaluate(() => (window as any).__gecko.axesVisible())).toBe(
    false,
  );
  expect(
    await page.evaluate(
      (p) => (window as any).__gecko.pickAxis(p[0], p[1]),
      at,
    ),
  ).toBeNull();

  // And that choice is remembered too.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
  expect(await page.evaluate(() => (window as any).__gecko.axesVisible())).toBe(
    false,
  );
});

test("faces and blocks are drawn, colored and faded from the panel", async ({
  page,
}) => {
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  const surface = (group: string) =>
    page.evaluate((g) => (window as any).__gecko.surface(g), group);

  // Faces by default; blocks are the same six quadrangles read the other way,
  // so showing them too is a choice rather than something to default to.
  expect(await surface("faces")).toMatchObject({ quads: 6, colors: 1 });
  expect(await surface("blocks")).toBeNull();

  // One color per face. Six faces, six colors — and the kernel was not
  // asked anything: these come off the names already in the view buffer.
  await openGroup(page, "faces");
  await page.locator("[data-test=faces-mode]").selectOption("id");
  expect(await surface("faces")).toMatchObject({ quads: 6, colors: 6 });

  // Both at once, each with its own coloring and its own transparency.
  await page.locator("[data-test=blocks-visible]").check();
  await openGroup(page, "blocks");
  await page.locator("[data-test=blocks-mode]").selectOption("uniform");
  expect(await surface("blocks")).toMatchObject({ quads: 6, colors: 1 });
  expect(await surface("faces")).toMatchObject({ colors: 6 });

  await openGroup(page, "faces");
  await page.locator("[data-test=faces-alpha]").fill("0.2");
  await page.waitForFunction(
    () => (window as any).__gecko.surface("faces").alpha < 0.21,
  );
  // One slider per group, so the other one does not move with it.
  expect((await surface("blocks")).alpha).toBeCloseTo(0.55, 6);
  expect(errors).toEqual([]);
});

test("each kind of cell shrinks towards its own center", async ({ page }) => {
  const errors = await boot(page);
  await openDrawer(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("[data-test=blocks-visible]").check();
  // Blocks come up slightly shrunk, because that is what makes them legible
  // over the faces they are drawn from. Start both at full size, so what the
  // numbers below say about shrinking is about shrinking.
  await openGroup(page, "blocks");
  await page.locator("[data-test=blocks-shrink]").fill("1");
  await page.waitForFunction(
    () => (window as any).__gecko.display().blocks.shrink === 1,
  );

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
  await openGroup(page, "blocks");
  await page.locator("[data-test=blocks-shrink]").fill("0.5");
  await page.waitForFunction(() => {
    const p = (window as any).__gecko.surface("blocks").positions as number[];
    return Math.max(...p.map(Math.abs)) < 0.3;
  });
  expect(await extent("blocks")).toBeCloseTo(0.5, 5);
  // And the faces, which were not asked to move, did not.
  expect(await extent("faces")).toBeCloseTo(1, 5);

  // A face pulls towards its own center, which is in its own plane: the cube
  // keeps its width while every face shrinks inside it.
  await openGroup(page, "faces");
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
  await openGroup(page, "faces");
  await page.locator("[data-test=faces-mode]").selectOption("id");
  await openGroup(page, "edges");
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
  expect(kept.faces.coloring).toBe("id");
  expect(kept.edges.shrink).toBe(0.8);
  // And the controls came back showing it.
  await expect(page.locator("[data-test=blocks-visible]")).toBeChecked();
  await expect(page.locator("[data-test=faces-mode]")).toHaveValue("id");
  expect(errors).toEqual([]);
});

test("every handle stays grabbable through an opaque surface", async ({
  page,
}) => {
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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("[data-test=blocks-visible]").check();
  await openGroup(page, "faces");
  await page.locator("[data-test=faces-alpha]").fill("1");
  await page.evaluate(() => (window as any).__gecko.settled());

  const names = (await page.evaluate(() =>
    (window as any).__gecko.names(),
  )) as number[];
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
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toEqual([]);
  expect(errors).toEqual([]);
});

test("zooming in stops at the model instead of going through it", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // Nothing to glue until two faces are chosen.
  await expect(page.locator("#glue")).toBeDisabled();

  const facing = (await page.evaluate(() => facingPair())) as number[];
  expect(facing[0]).not.toBe(facing[1]);

  const box = (await page.locator("#view").boundingBox())!;
  for (const face of facing) {
    const at = (await page.evaluate(
      (f) => (window as any).__gecko.faceScreen(f),
      face,
    )) as [number, number] | null;
    expect(at, `face ${face} should be on screen`).not.toBeNull();
    await page.mouse.click(box.x + at![0], box.y + at![1]);
  }

  // Both picked, and the panel says what gluing them would do.
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toHaveLength(2);
  await expect(page.locator("#status")).toContainText(
    "will be welded where they meet",
  );
  await expect(page.locator("#glue")).toBeEnabled();

  await page.locator("#glue").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

  // **Which** two faces a click lands on is a question about the camera:
  // the faces looking at each other are in the middle of the model, and
  // whatever is in front of them is what the pointer hits first. So this
  // test is about the gesture, and where a weld lands is the subject of the
  // next one, which names its faces outright.

  // Four pairs of nodes became four nodes: the blocks are one object now.
  expect(await page.evaluate(() => (window as any).__gecko.counts())).toEqual({
    nodes: 12,
    edges: 20,
  });
  await expect(page.locator("[data-test=faces]")).toHaveText("11");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  // And the selection is gone: those two names mean something else now.
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toEqual([]);
  await expect(page.locator("#glue")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("a glue across a gap meets in the middle", async ({ page }) => {
  // Where the seam lands, through the page and nothing stubbed. The faces
  // are named rather than clicked: the two that face each other are buried
  // in the middle of the model, and a click would be aiming at them past
  // whatever is in front.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );

  /**
   * The distinct x every **node** sits at.
   *
   * The nodes, not the quadrangles: faces are drawn shrunk towards their own
   * centers, so no corner of the drawing is where a node is.
   */
  const columns = () =>
    page.evaluate(() => {
      const g = (window as any).__gecko;
      const seen = new Set<number>();
      for (const name of g.names())
        seen.add(Math.round(g.nodePosition(name)[0] * 1e4) / 1e4);
      return [...seen].sort((a: number, b: number) => a - b);
    });

  // The button puts each cube 1.5 along, so the two faces about to be glued
  // are half a unit apart: a glue across a space, not at a touching face.
  expect(await columns()).toEqual([0, 1, 1.5, 2.5]);

  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const [first, second] = facingPair();
    g.apply({ op: "glue_faces", first, second });
    await g.refresh();
  });
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

  // One column where there were two, half way between them: the faces met
  // in the middle, and neither block was dragged onto the other.
  expect(await columns()).toEqual([0, 1.25, 2.5]);
  expect(errors).toEqual([]);
});

test("dragging from a face turns the view instead of selecting it", async ({
  page,
}) => {
  // Orbiting starts wherever the pointer happens to be, which is usually on
  // the model. A press-move-release must not also count as a pick.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const face = (await page.evaluate(
    () => (window as any).__gecko.surface("faces").faces[0],
  )) as number;
  const at = (await page.evaluate(
    (f) => (window as any).__gecko.faceScreen(f),
    face,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;
  const before = await page.evaluate(() => (window as any).__gecko.camera());

  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  for (let i = 1; i <= 6; i++)
    await page.mouse.move(box.x + at[0] + i * 12, box.y + at[1]);
  await page.mouse.up();

  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toEqual([]);
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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // Put the camera somewhere deliberate, so "unchanged" means something.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(
    box.x + box.width / 2 + 60,
    box.y + box.height / 2 + 30,
  );
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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

  const after = await page.evaluate(() => (window as any).__gecko.camera());
  expect(after.alpha).toBe(before.alpha);
  expect(after.beta).toBe(before.beta);
  expect(after.radius).toBe(before.radius);
  expect(errors).toEqual([]);
});

test("recentring shows everything without changing the direction", async ({
  page,
}) => {
  const errors = await boot(page);
  for (let i = 0; i < 3; i++) {
    await page.locator("#cube").click();
    await page.waitForFunction(
      (n) => (window as any).__gecko.counts().nodes === n,
      (i + 1) * 8,
    );
  }
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  const center = { x: box.x + box.width / 2, y: box.y + box.height / 2 };

  // Look at it from somewhere deliberate — not the angle it starts at, or
  // "unchanged" would be indistinguishable from "reset to the default".
  await page.mouse.move(center.x, center.y);
  await page.mouse.down();
  await page.mouse.move(center.x + 110, center.y + 40);
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
      const r = (
        document.querySelector("#view") as HTMLCanvasElement
      ).getBoundingClientRect();
      return (g.names() as number[]).filter((n) => {
        const at = g.nodeScreen(n) as [number, number] | null;
        return (
          !at || at[0] < 0 || at[1] < 0 || at[0] > r.width || at[1] > r.height
        );
      }).length;
    });
  expect(
    await offScreen(),
    "the zoom should have lost some of the model",
  ).toBeGreaterThan(0);

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
  await page.locator("#recenter").click();
  await page.evaluate(() => (window as any).__gecko.settled());
  await expect(page.locator("#status")).toHaveText("view recenterd");

  // It glided: a cut goes straight from one radius to the other and leaves
  // nothing in between, however the frames happen to fall.
  const after = await page.evaluate(() => (window as any).__gecko.camera());
  const between = (await page.evaluate(
    () => (window as any).__radii,
  )) as number[];
  const partWay = between.filter(
    (r) => r > before.radius + 1e-6 && r < after.radius - 1e-6,
  );
  // One is the whole claim: it did not get there in a single step. A cut
  // leaves exactly none however the frames fall, so this still fails on one
  // — and asking for more was a bet on the frame rate that a CI runner
  // drawing seven frames in 450 ms, most of them at the eased ends, lost.
  expect(
    partWay.length,
    `radii seen: ${between.join(", ")}`,
  ).toBeGreaterThanOrEqual(1);

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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  await expect(page.locator("#remove")).toBeDisabled();
  const box = (await page.locator("#view").boundingBox())!;

  // A face of a block is a face: Del does not take the block behind it.
  const face = (await page.evaluate(() => facingPair()))[0] as number;
  const at = (await page.evaluate(
    (f) => (window as any).__gecko.faceScreen(f),
    face,
  )) as [number, number];
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await expect(page.locator("#status")).toContainText(`face ${face}`);
  await page.keyboard.press("Delete");
  await expect(page.locator("#status")).toContainText("pick the block");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await page.keyboard.press("Escape");

  // The block is picked on the blocks layer.
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.run("faces");
    g.run("blocks");
  });
  await page.evaluate(() => (window as any).__gecko.settled());
  const block = (await page.evaluate(
    () => (window as any).__gecko.surface("blocks").blocks[0],
  )) as number;
  const middle = (await page.evaluate(
    (b) => (window as any).__gecko.blockScreen(b),
    block,
  )) as [number, number];
  await page.mouse.click(box.x + middle[0], box.y + middle[1]);
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedBlocks()),
  ).toEqual([block]);
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toEqual([]);
  await expect(page.locator("#status")).toContainText(`block ${block} selected`);
  await expect(page.locator("#remove")).toBeEnabled();

  // Drawn in the color of a pick, every face of it and nothing else.
  const tinted = (await page.evaluate((b) => {
    const s = (window as any).__gecko.surface("blocks");
    const out: [number, boolean][] = [];
    for (let q = 0; q < s.quads; q++) {
      const [r, g, bl] = [s.tints[q * 3], s.tints[q * 3 + 1], s.tints[q * 3 + 2]];
      out.push([
        s.blocks[q],
        Math.abs(r - 1) < 0.01 && Math.abs(g - 0.8) < 0.01 && Math.abs(bl - 0.2) < 0.01,
      ]);
    }
    return out;
  }, block)) as [number, boolean][];
  expect(tinted.length).toBe(12);
  for (const [b, picked] of tinted) expect(picked).toBe(b === block);

  await page.locator("#remove").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("[data-test=faces]")).toHaveText("6");
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedBlocks()),
  ).toEqual([]);
  await expect(page.locator("#remove")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("blocks picked are taken away in one step", async ({ page }) => {
  const errors = await boot(page);
  for (const n of [8, 16, 24]) {
    await page.locator("#cube").click();
    await page.waitForFunction(
      (k) => (window as any).__gecko.counts().nodes === k,
      n,
    );
  }
  const blocks = (await page.evaluate(() => [
    ...new Set((window as any).__gecko.surface("faces").blocks),
  ])) as number[];
  expect(blocks.length).toBe(3);
  await page.evaluate((bs) => {
    const g = (window as any).__gecko;
    g.chooseBlock(bs[0]);
    g.chooseBlock(bs[2]);
  }, blocks);
  await expect(page.locator("#status")).toContainText("2 blocks selected");
  await page.keyboard.press("Delete");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await page.keyboard.press("Control+z");
  await expect(page.locator("[data-test=blocks]")).toHaveText("3");
  expect(errors).toEqual([]);
});

test("blocks picked are pillowed round, with or without their outer faces", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate(
    (e) => (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }),
    edge,
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  const one = () =>
    page.evaluate(() => {
      const g = (window as any).__gecko;
      g.chooseBlock(g.surface("faces").blocks[0]);
    });

  // Outer faces in, by default: the layer goes round all six of its faces.
  await one();
  await page.keyboard.press("p");
  await expect(page.locator("[data-test=blocks]")).toHaveText("8");
  await expect(page.locator("#status")).toContainText(
    "pillowed round 1 block, along 6 faces",
  );
  await page.keyboard.press("Control+z");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");

  // Left out: only the face between the two.
  await page.evaluate(() => document.body.classList.add("params-open"));
  await page.locator("[data-test=pillow-outer]").uncheck();
  // A key pressed in a field is typed into it, not a shortcut.
  await page.evaluate(() => (document.activeElement as HTMLElement).blur());
  await one();
  await page.keyboard.press("p");
  await expect(page.locator("[data-test=blocks]")).toHaveText("3");
  await expect(page.locator("#status")).toContainText(
    "pillowed round 1 block, along 1 face —",
  );
  expect(errors).toEqual([]);
});

test("the toolbar and the keyboard are the same thing", async ({ page }) => {
  // One definition of each action, reached two ways. A button whose
  // shortcut did something subtly different would be the worst of both.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  await page.keyboard.press("c");
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );

  // And the keys that are not actions are left alone.
  await page.keyboard.press("q");
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().nodes),
  ).toBe(16);
  expect(errors).toEqual([]);
});

test("the interface folds away, and remembers that it did", async ({
  page,
}) => {
  const errors = await boot(page);
  // Shut to begin with: the window belongs to the model.
  expect(
    await page.evaluate(() => (window as any).__gecko.chrome()),
  ).toMatchObject({
    drawer: false,
    hidden: false,
  });
  await expect(page.locator("#drawer")).toBeHidden();

  await page.keyboard.press("Tab");
  await expect(page.locator("#drawer")).toBeVisible();
  await expect(page.locator("#display")).toHaveAttribute(
    "aria-expanded",
    "true",
  );

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

test("the help card lists the keys, and any key dismisses it", async ({
  page,
}) => {
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
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().nodes),
  ).toBe(0);
  expect(errors).toEqual([]);
});

test("typing in a field is typing, not a shortcut", async ({ page }) => {
  // The color inputs and the sliders take characters and arrow keys. A
  // slider that jumped to a new view instead of nudging would be unusable.
  const errors = await boot(page);
  await openGroup(page, "faces");
  await page.locator("[data-test=faces-alpha]").focus();
  await page.keyboard.press("c");
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().nodes),
  ).toBe(0);

  // Tab from inside the drawer walks its fields rather than shutting it.
  await page.keyboard.press("Tab");
  await expect(page.locator("#drawer")).toBeVisible();
  expect(errors).toEqual([]);
});

/** Width and height out of a PNG's header, which is all this needs to read. */
function pngSize(bytes: Buffer): { width: number; height: number } {
  expect(bytes.subarray(0, 8)).toEqual(
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  );
  expect(bytes.subarray(12, 16).toString()).toBe("IHDR");
  return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

test("a picture is saved, at the size the drawer asks for", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const canvas = await page.evaluate(() => {
    const c = document.querySelector("#view") as HTMLCanvasElement;
    return { width: c.width, height: c.height };
  });

  const download = page.waitForEvent("download");
  await page.keyboard.press("s");
  const saved = await download;

  // Named by the moment, so the files sort the way they were taken.
  expect(saved.suggestedFilename()).toMatch(
    /^gecko-\d{4}-\d{2}-\d{2}-\d{6}\.png$/,
  );
  const where = await saved.path();
  const size = pngSize(await readFile(where));
  // Twice the canvas by default: a figure in a paper wants more pixels than
  // a window has.
  expect(size).toEqual({ width: canvas.width * 2, height: canvas.height * 2 });

  await expect(page.locator("#status")).toHaveText("picture saved");
  expect(errors).toEqual([]);
});

test("a picture can be saved with nothing behind it", async ({ page }) => {
  // For dropping a figure onto a page whose color you do not know yet.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const cornerAlpha = (transparent: boolean) =>
    page.evaluate(async (t) => {
      const url = await (window as any).__gecko.capture({
        scale: 1,
        transparent: t,
      });
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
  expect(
    await page.evaluate(() => (window as any).__gecko.theme().background),
  ).toBe("#15171b");
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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  const at = (await page.evaluate(
    (e) => (window as any).__gecko.edgeScreen(e),
    edge,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + at[0], box.y + at[1]);

  // Four parallel edges on a lone cube, and the one clicked is among them.
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  const lit = (await page.evaluate(() =>
    (window as any).__gecko.sheet(),
  )) as number[];
  expect(lit).toHaveLength(4);
  expect(lit).toContain(edge);
  await expect(page.locator("#status")).toContainText("4 edges");

  // Clicking it again puts it out.
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 0,
  );
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
    const pixels = flat
      .getContext("2d")!
      .getImageData(0, 0, view.width, view.height).data;
    let lit = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
      if (r > 200 && g > 150 && g < 230 && b < 120) lit++;
    }
    return lit;
  });
}

test("a lit sheet is the same thickness however far away it is", async ({
  page,
}) => {
  // The sheet is something you are being shown, not part of the model, so
  // its width belongs to the screen. Drawn in scene units instead it reads
  // as a measure of where the camera happens to be — and every recentring
  // moves the camera, which is how this was noticed.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  // Creating a cube glides the camera into place, and that glide writes to
  // `radius` every frame. Setting a distance while it runs is setting a
  // value the animation is about to overwrite — which is how this test read
  // the same distance twice and still looked like it worked.
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 4,
  );

  const at = async (radius: number) => {
    await page.evaluate((r) => (window as any).__gecko.setRadius(r), radius);
    await page.evaluate(() => (window as any).__gecko.settled());
    expect(
      await page.evaluate(() => (window as any).__gecko.camera().radius),
    ).toBeCloseTo(radius, 3);
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

/** Cut the model in half along `axis`, once, wherever the first edge runs. */
async function cutInHalf(
  page: import("@playwright/test").Page,
  at: [number, number, number],
) {
  const edge = (await page.evaluate((w) => edgeNear(w), at)) as number;
  await page.evaluate(async (e) => {
    const g = (window as any).__gecko;
    await g.apply({ op: "split_sheet", edge: e, at: 0.5 });
    await g.refresh();
  }, edge);
}

test("a lit sheet is hidden by the model it runs through", async ({ page }) => {
  // It used to be drawn over everything, on the grounds that a lit sheet is
  // a thing being shown to you rather than a thing that is there. But it
  // runs *through* the model, and one at the back of a hundred blocks then
  // reads exactly like one at the front.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  // In two each way, so the sheet has edges with blocks in front of them.
  await cutInHalf(page, [0.5, 0, 0]);
  await cutInHalf(page, [0, 0.5, 0]);
  await cutInHalf(page, [0, 0, 0.5]);
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 27,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([0, 0.25, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  await page.evaluate(() => (window as any).__gecko.settled());
  const through = await litPixels(page);
  expect(through, "the sheet is drawn at all").toBeGreaterThan(100);

  // The same sheet with the faces taken away: nothing in front of it now,
  // so every edge of it comes out at its own color. Drawn over everything,
  // the two counts would be the same number.
  await page.keyboard.press("1");
  await page.waitForFunction(
    () => !(window as any).__gecko.prefs().faces.visible,
  );
  await page.evaluate(() => (window as any).__gecko.settled());
  const bare = await litPixels(page);

  expect(bare, `through ${through}, bare ${bare}`).toBeGreaterThan(
    through * 1.1,
  );
  expect(errors).toEqual([]);
});

test("N takes the lit sheet away and leaves the gap it was in", async ({
  page,
}) => {
  // Del closes behind itself, which is what you want nine times in ten.
  // This is the other one: the two walls stay where they are, so the model
  // keeps its shape and gains a slot.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await cutInHalf(page, [0.5, 0, 0]);
  await cutInHalf(page, [0.75, 0, 0]);
  await expect(page.locator("[data-test=blocks]")).toHaveText("3");
  await page.evaluate(() => (window as any).__gecko.settled());

  // The middle layer, the thin one between x = 0.5 and x = 0.75.
  const edge = (await page.evaluate(() => edgeNear([0.625, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 4,
  );

  await page.keyboard.press("n");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  // Sixteen, not twelve: the two that are left do not meet.
  await expect(page.locator("[data-test=nodes]")).toHaveText("16");
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("gap left open"),
  );
  expect(errors).toEqual([]);
});

/** A unit cube cut into three each way, the way somebody cuts one: halve
 *  it, then halve the half. Twenty-seven blocks, sixty-four nodes. */
async function cutGrid(page: import("@playwright/test").Page) {
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  for (const at of [
    [0.5, 0, 0],
    [0, 0.5, 0],
    [0, 0, 0.5],
    [0.25, 0, 0],
    [0, 0.25, 0],
    [0, 0, 0.25],
  ] as [number, number, number][]) {
    await cutInHalf(page, at);
  }
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 64,
  );
  await page.evaluate(() => (window as any).__gecko.settled());
}

test("J stands you in one layer, and the arrows walk the model a layer at a time", async ({
  page,
}) => {
  // The complaint this answers: a face in the middle of a hundred blocks
  // cannot be clicked on through the ninety-nine in front of it.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await cutGrid(page);

  const edge = (await page.evaluate(() => edgeNear([0.125, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 16,
  );

  const drawn = () =>
    page.evaluate(() => ({
      nodes: (window as any).__gecko.nodesDrawn(),
      edges: (window as any).__gecko.counts().edges,
    }));
  expect(await drawn()).toEqual({ nodes: 64, edges: 144 });

  // One layer of nine blocks: sixteen corners on each of its two faces.
  await page.keyboard.press("j");
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("the layer through"),
  );
  expect(await drawn()).toEqual({ nodes: 32, edges: 64 });

  // Along the model, a layer at a time, and off the end of it. Each step
  // has to recognise the layer it came from — which is the whole of what
  // naming a layer by the least of its edges is for.
  const seen: string[] = [];
  for (let step = 0; step < 3; step++) {
    await page.keyboard.press("ArrowRight");
    await page.evaluate(() => (window as any).__gecko.settled());
    seen.push(
      (await page.evaluate(() => (window as any).__gecko.message())) as string,
    );
    expect(await drawn(), `after ${step + 1} steps`).toEqual({
      nodes: 32,
      edges: 64,
    });
  }
  expect(seen[2], `${seen.join(" | ")}`).toBe("no layer that way");
  // And the three it went through are three different layers, not two of
  // them twice: a step that forgot where it came from would bounce.
  expect(new Set([seen[0], seen[1]]).size).toBe(2);

  // Back the other way, and out.
  await page.keyboard.press("ArrowLeft");
  await page.evaluate(() => (window as any).__gecko.settled());
  expect(await drawn()).toEqual({ nodes: 32, edges: 64 });

  await page.keyboard.press("Escape");
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("whole model"),
  );
  expect(await drawn()).toEqual({ nodes: 64, edges: 144 });
  expect(errors).toEqual([]);
});

test("a lit sheet stops at the edge of what is on screen", async ({ page }) => {
  // A sheet runs through the whole model, and a filter shows part of it.
  // Lit over the part that is hidden, it is a thread floating in a view
  // whose whole point is that the rest is out of the way.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await cutGrid(page);

  // Stand in the first layer across x.
  const along = (await page.evaluate(() => edgeNear([0.125, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), along);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 16,
  );
  await page.keyboard.press("j");
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("the layer through"),
  );

  // And light a sheet that runs across it and out through the other two.
  // Sixteen edges, at the four values of x by the four of z; the layer is
  // two of those four values of x, so half of them are on screen.
  const across = (await page.evaluate(() => edgeNear([0, 0.125, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), across);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 16,
  );
  await page.evaluate(() => (window as any).__gecko.settled());
  expect(await page.evaluate(() => (window as any).__gecko.litDrawn())).toBe(8);

  // Out again, and the rest of it comes back.
  await page.keyboard.press("Escape");
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("whole model"),
  );
  await page.evaluate(() => (window as any).__gecko.settled());
  expect(await page.evaluate(() => (window as any).__gecko.litDrawn())).toBe(
    16,
  );
  expect(errors).toEqual([]);
});

test("the arrow keys do not turn the view", async ({ page }) => {
  // Babylon hands an arc-rotate camera the arrow keys of its own accord.
  // Nothing here asked for that, the help card does not print it, and the
  // arrows are how you step from one layer to the next — so every step
  // turned the view as well. The pointer moves the view, and the keys that
  // say they do.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // On the canvas, because that is where the camera listens: a key sent to
  // the page at large never reaches it, and a test that did that would pass
  // whatever the camera is wired to.
  await page.locator("#view").focus();
  const where = () => page.evaluate(() => (window as any).__gecko.camera());
  const before = await where();
  for (const key of ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]) {
    // Held across a few frames, which is what a real press is. The
    // camera's own keyboard input moves the view a little on each frame it
    // is held and nothing at all on a press that spans none, so a test
    // that tapped the key would pass whatever the camera is wired to.
    await page.keyboard.down(key);
    await page.evaluate(() => (window as any).__gecko.settled());
    await page.evaluate(() => (window as any).__gecko.settled());
    await page.keyboard.up(key);
  }
  expect(await where()).toEqual(before);
  expect(errors).toEqual([]);
});

test("editing from inside a layer reaches the kernel, and the layer gives way", async ({
  page,
}) => {
  // A place to stand, not a mode to be in: everything still works in here.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await cutGrid(page);

  const edge = (await page.evaluate(() => edgeNear([0.125, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 16,
  );
  await page.keyboard.press("j");
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("the layer through"),
  );

  // The layer you are standing in, taken away from inside it.
  await page.keyboard.press("n");
  await expect(page.locator("[data-test=blocks]")).toHaveText("18");
  // And nowhere to stand any more, so the whole model comes back rather
  // than a filter made of names nothing answers to showing nothing at all.
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("the layer is gone"),
  );
  expect(await page.evaluate(() => (window as any).__gecko.nodesDrawn())).toBe(
    48,
  );
  expect(errors).toEqual([]);
});

test("a sheet runs on through a glued block", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // An edge lying in the seam, parallel to it: its sheet crosses into the
  // second block. Four and four, sharing the two that lie in the seam.
  const seam = (await page.evaluate(() => edgeNear([1, 0.5, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), seam);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  expect(
    await page.evaluate(() => (window as any).__gecko.sheet()),
  ).toHaveLength(6);

  // And one that does not lie in the seam stays in the block it started in.
  const away = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), away);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 4,
  );

  // Esc clears it, as it clears a face selection.
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 0,
  );
  expect(errors).toEqual([]);
});

test("dragging along an edge cuts the whole sheet", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  const at = (await page.evaluate(
    (e) => (window as any).__gecko.edgeScreen(e),
    edge,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;
  const camera = await page.evaluate(() => (window as any).__gecko.camera());

  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  // Pressing lights the sheet; nothing is cut yet.
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 4,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().nodes),
  ).toBe(8);

  for (let i = 1; i <= 6; i++)
    await page.mouse.move(box.x + at[0], box.y + at[1] - i * 12);
  // Aimed, and said, and still nothing cut.
  const aimed = (await page.evaluate(() =>
    (window as any).__gecko.aimedAt(),
  )) as number;
  expect(aimed).toBeGreaterThan(0);
  expect(aimed).toBeLessThan(1);
  await expect(page.locator("#status")).toContainText("let go to cut");
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().nodes),
  ).toBe(8);

  // Aiming is not orbiting: the pointer was taken off the camera.
  const during = await page.evaluate(() => (window as any).__gecko.camera());
  expect(during.alpha).toBe(camera.alpha);
  expect(during.beta).toBe(camera.beta);

  await page.mouse.up();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

  // One cube became two blocks, sharing a face.
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("[data-test=faces]")).toHaveText("11");
  expect(await page.evaluate(() => (window as any).__gecko.counts())).toEqual({
    nodes: 12,
    edges: 20,
  });
  // The sheet is gone with the edge it was named by.
  expect(await page.evaluate(() => (window as any).__gecko.sheet())).toEqual(
    [],
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.aimedAt()),
  ).toBeNull();
  expect(errors).toEqual([]);
});

test("pressing an edge without moving selects it rather than cutting", async ({
  page,
}) => {
  // A cut cannot be undone, and a click is cheap to make by accident.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  const at = (await page.evaluate(
    (e) => (window as any).__gecko.edgeScreen(e),
    edge,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;

  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 4,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().nodes),
  ).toBe(8);

  // And clicking it again puts the sheet out, still without cutting.
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 0,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().nodes),
  ).toBe(8);
  expect(errors).toEqual([]);
});

test("pillowing lays a layer and leaves the surface where it was", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("[data-test=faces]")).toHaveText("11");

  // The whole point: the model's surface has not moved. Every height that
  // was there is still there, with one new layer between them.
  const after = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return (g.names() as number[]).map((n) => g.nodePosition(n)[2]);
  })) as number[];
  for (const z of before) {
    expect(
      after.some((w) => Math.abs(w - z) < 1e-6),
      `height ${z} went missing`,
    ).toBe(true);
  }
  expect(new Set(after.map((z) => z.toFixed(6))).size).toBe(3);
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toEqual([]);
  expect(errors).toEqual([]);
});

test("the selection grows as far as you like", async ({ page }) => {
  // Two is what a glue wants; a pillow takes as many as are picked, so the
  // third click no longer throws the first two away.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const faces = (await page.evaluate(() => {
    const s = (window as any).__gecko.surface("faces");
    return [0, 1, 2].map((q) => ({ face: s.faces[q], block: s.blocks[q] }));
  })) as { face: number; block: number }[];

  for (const f of faces)
    await page.evaluate((t) => (window as any).__gecko.choose(t), f);
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toHaveLength(3);
  // Three is too many to glue and fine to pillow.
  await expect(page.locator("#glue")).toBeDisabled();
  await expect(page.locator("#pillow")).toBeEnabled();
  // All three lit, which is what tells you what the layer will cover.
  expect(
    await page.evaluate(() => (window as any).__gecko.surface("faces").colors),
  ).toBe(3);

  // Clicking one again takes it out rather than starting over.
  await page.evaluate((t) => (window as any).__gecko.choose(t), faces[0]);
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toHaveLength(2);
  await expect(page.locator("#glue")).toBeEnabled();
  expect(errors).toEqual([]);
});

test("a refused pillowing selects the pillow surface that would pass", async ({
  page,
}) => {
  // One face out of four on a wall: a node of its edge would have to leave
  // the wall. Nothing is done, and the faces of the block behind it that
  // would pass are selected instead, for P to take.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  const msh = await readFile(
    "../crates/gecko-lua/tests/fixtures/cube.msh",
    "utf8",
  );
  await page.evaluate(
    (m) => (window as any).__gecko.loadGeometry("cube.msh", m),
    msh,
  );
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.apply({ op: "box_geometry", surfaces: [], margin: 0 });
    g.apply({ op: "classify", tolerance: 0.02 });
  });
  await page.evaluate(() => (window as any).__gecko.refresh());
  for (const want of [
    [0.5, 0, 0],
    [0, 0, 0.5],
  ]) {
    const edge = (await page.evaluate((w) => edgeNear(w), want)) as number;
    const cut = await page.evaluate(
      (e) =>
        (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }),
      edge,
    );
    expect(cut.ok, cut.message).toBe(true);
    await page.evaluate(() => (window as any).__gecko.refresh());
  }
  await expect(page.locator("[data-test=blocks]")).toHaveText("4");

  // The face of the wall y = 0 nearest the corner at the origin.
  const corner = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    const c = g.center();
    let best = 0;
    let shortest = Infinity;
    for (let q = 0; q < s.quads; q++) {
      const at = [0, 0, 0];
      for (let k = 0; k < 4; k++)
        for (let a = 0; a < 3; a++)
          at[a] += s.positions[(q * 4 + k) * 3 + a] / 4;
      const d = Math.hypot(
        at[0] + c[0] - 0.25,
        at[1] + c[1],
        at[2] + c[2] - 0.25,
      );
      if (d < shortest) {
        shortest = d;
        best = q;
      }
    }
    return { face: s.faces[best], block: s.blocks[best] };
  })) as { face: number; block: number };

  await page.evaluate((t) => (window as any).__gecko.choose(t), corner);
  await page.locator("#pillow").click();
  await expect(page.locator("#status")).toContainText("cannot be pulled in");
  await expect(page.locator("#status")).toContainText("P pillows them");
  await expect(page.locator("[data-test=blocks]")).toHaveText("4");
  const offered = (await page.evaluate(() =>
    (window as any).__gecko.selection(),
  )) as { face: number; block: number }[];
  expect(offered).toHaveLength(3);
  expect(offered).toContainEqual(corner);
  for (const f of offered) expect(f.block).toBe(corner.block);

  // And taking it is one more P.
  await page.locator("#pillow").click();
  await expect(page.locator("#status")).toContainText("pillowed 3 faces");
  expect(errors).toEqual([]);
});

test("every edge is drawn in the color of what it is on", async ({ page }) => {
  // A block resting on the cube, cut in half: the edges round the cube's
  // corners are on its curves, the ones the cut made across its walls are
  // on its surfaces. The ribbon numbers its colors by point, so a table of
  // one color per edge drew edge k in the colors of edges 2k and 2k + 1.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  const msh = await readFile(
    "../crates/gecko-lua/tests/fixtures/cube.msh",
    "utf8",
  );
  await page.evaluate(
    (m) => (window as any).__gecko.loadGeometry("cube.msh", m),
    msh,
  );
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.apply({ op: "box_geometry", surfaces: [], margin: 0 });
    g.apply({ op: "classify", tolerance: 0.02 });
  });
  await page.evaluate(() => (window as any).__gecko.refresh());
  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate(
    (e) =>
      (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }),
    edge,
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await page.evaluate(() => (window as any).__gecko.settled());
  expect(
    await page.evaluate(() => (window as any).__gecko.edgeNames().length),
  ).toBe(20);
  // Nothing in front of any edge, and nothing drawn along it: the faces
  // would tint the ones behind them, the model's own curves lie on top of
  // the edges on them.
  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    for (const what of ["faces", "blocks", "surfaces", "curves"])
      if (g.prefs()[what].visible) await g.run(what);
  });
  await page.waitForFunction(() => {
    const p = (window as any).__gecko.prefs();
    return ["faces", "blocks", "surfaces", "curves"].every(
      (w) => !p[w].visible,
    );
  });
  await page.evaluate(() => (window as any).__gecko.settled());

  const wrong = (await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const theme = g.prefs().theme;
    const hex = (h: string) =>
      [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
    const looks = {
      curve: hex(theme.onCurve),
      surface: hex(theme.onSurface),
      plain: hex(theme.edges),
    };
    const shot = await g.capture({ scale: 1, transparent: false });
    const img = await createImageBitmap(await (await fetch(shot)).blob());
    const canvas = new OffscreenCanvas(img.width, img.height);
    const cx = canvas.getContext("2d")!;
    cx.drawImage(img, 0, 0);
    const k = img.width / document.querySelector("#view")!.clientWidth;
    const far = (p: ArrayLike<number>, q: number[]) =>
      Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]);
    const out: string[] = [];
    let seen = 0;
    for (const e of g.edgeNames() as number[]) {
      // On two of the cube's walls is on a curve, on one is on a surface.
      const walls = (g.edgeMidpoint(e) as number[]).filter(
        (v) => Math.abs(v) < 1e-6 || Math.abs(v - 1) < 1e-6,
      ).length;
      const want = walls === 2 ? "curve" : "surface";
      const [x, y] = g.edgeScreen(e);
      seen++;
      // The color nearest any pixel round the middle.
      let best = "";
      let closest = Infinity;
      for (let dx = -2; dx <= 2; dx++)
        for (let dy = -2; dy <= 2; dy++) {
          const d = cx.getImageData(
            Math.round((x + dx) * k),
            Math.round((y + dy) * k),
            1,
            1,
          ).data;
          for (const [name, c] of Object.entries(looks)) {
            const gap = far(d, c);
            if (gap < closest) {
              closest = gap;
              best = name;
            }
          }
        }
      if (best !== want) out.push(`edge ${e}: ${best}, not ${want}`);
    }
    return { out, seen };
  })) as { out: string[]; seen: number };
  expect(wrong.out).toEqual([]);
  expect(wrong.seen).toBe(20);
  expect(errors).toEqual([]);
});

test("a face between two blocks is taken from the block behind it", async ({
  page,
}) => {
  // Eight blocks in two layers, internal faces shown and shrunk, so the
  // faces between the layers can be clicked through the gaps. A click on
  // one is a face of the block behind it, as on the outside: the layer
  // below seen from above, the layer above seen from below. It used to be
  // whichever block the view listed first, so a pillow of faces clicked
  // from one side could be refused for taking two.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  for (const want of [
    [0, 0.5, 0],
    [0.5, 0, 0],
    [0, 0, 0.5],
  ]) {
    const edge = (await page.evaluate((w) => edgeNear(w), want)) as number;
    await page.evaluate(
      (e) =>
        (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }),
      edge,
    );
    await page.evaluate(() => (window as any).__gecko.refresh());
  }
  await expect(page.locator("[data-test=blocks]")).toHaveText("8");
  await openDrawer(page);
  await openGroup(page, "faces");
  if (
    !(await page.evaluate(() => (window as any).__gecko.prefs().faces.internal))
  )
    await page.evaluate(() => (window as any).__gecko.run("internal"));
  await page.locator("[data-test=faces-shrink]").fill("0.5");
  await page.waitForFunction(
    () => (window as any).__gecko.display().faces.shrink === 0.5,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // The lower layer: the blocks with a face on the floor, y = 0.
  const lower = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    const c = g.center();
    const out: number[] = [];
    for (let q = 0; q < s.quads; q++)
      if (
        [0, 1, 2, 3].every(
          (k) => Math.abs(s.positions[(q * 4 + k) * 3 + 1] + c[1]) < 1e-6,
        )
      )
        out.push(s.blocks[q]);
    return out;
  })) as number[];
  expect(lower).toHaveLength(4);

  /** Click every face between the layers a click can reach, and check
   *  each is taken from the layer on the far side of it. */
  const check = async () => {
    const { taken, beta } = (await page.evaluate(() => {
      const g = (window as any).__gecko;
      const s = g.surface("faces");
      const c = g.center();
      const taken: { face: number; block: number }[] = [];
      for (let q = 0; q < s.quads; q++) {
        const between = [0, 1, 2, 3].every(
          (k) => Math.abs(s.positions[(q * 4 + k) * 3 + 1] + c[1] - 0.5) < 1e-6,
        );
        if (!between) continue;
        const [x, y] = g.faceScreen(s.faces[q]);
        const t = g.target(x, y);
        if (t?.kind === "face" && t.face === s.faces[q])
          taken.push({ face: t.face, block: t.block });
      }
      return { taken, beta: g.camera().beta };
    })) as { taken: { face: number; block: number }[]; beta: number };
    expect(taken.length, "a face between the layers in reach").toBeGreaterThan(
      0,
    );
    const looksDown = beta < Math.PI / 2;
    for (const t of taken)
      expect(
        lower.includes(t.block),
        `face ${t.face} taken from block ${t.block}, looking ${looksDown ? "down" : "up"}`,
      ).toBe(looksDown);
  };

  // From above, as the camera starts.
  await check();

  // And from below: dragged up, the camera goes under the model — a little
  // under, so the faces between the layers are not hidden behind the floor.
  const box = (await page.locator("#view").boundingBox())!;
  const x = box.x + box.width - 40;
  let y = box.y + box.height - 60;
  await page.mouse.move(x, y);
  await page.mouse.down();
  for (let i = 0; i < 100; i++) {
    const beta = (await page.evaluate(
      () => (window as any).__gecko.camera().beta,
    )) as number;
    if (beta > Math.PI / 2 + 0.3) break;
    y -= 5;
    await page.mouse.move(x, y);
  }
  await page.mouse.up();
  await page.evaluate(() => (window as any).__gecko.settled());
  expect(
    await page.evaluate(() => (window as any).__gecko.camera().beta),
  ).toBeGreaterThan(Math.PI / 2);
  await check();
  expect(errors).toEqual([]);
});

/** The color in the middle of the canvas, as three numbers. */
async function centerPixel(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const flat = document.createElement("canvas");
    flat.width = view.width;
    flat.height = view.height;
    flat.getContext("2d")!.drawImage(view, 0, 0);
    const at = flat
      .getContext("2d")!
      .getImageData(
        Math.floor(view.width / 2),
        Math.floor(view.height / 2),
        1,
        1,
      ).data;
    return [at[0], at[1], at[2]] as [number, number, number];
  });
}

const apart = (a: number[], b: number[]) =>
  Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);

test("the nearer of two translucent blocks is the one you see", async ({
  page,
}) => {
  // Which of them is nearer has to follow the camera. It cannot when every
  // cell is in one mesh: Babylon sorts transparent *meshes* back to front
  // and leaves the triangles inside one in the order they were built, so
  // the same cell wins the blend from either side.
  const errors = await boot(page);
  await openGroup(page, "faces");
  await page.locator("[data-test=faces-mode]").selectOption("id");
  await page.locator("[data-test=faces-alpha]").fill("0.6");

  // One cube: what it looks like on its own, from each side. **Both
  // sides**, because in this mode a face's colour comes from its name, so
  // the +x face and the −x face of one cube are two different colours —
  // and a view of the cube from one side is no baseline for a view of it
  // from the other.
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  const alone = async (axis: "+x" | "-x") => {
    await page.evaluate((a) => (window as any).__gecko.snapTo(a), axis);
    await page.evaluate(() => (window as any).__gecko.settled());
    return centerPixel(page);
  };
  const alonePlus = await alone("+x");
  const aloneMinus = await alone("-x");

  // A second cube at a greater x, so it stands between that one and a
  // camera at +x. Along the row they cover each other exactly.
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  const bothPlus = await alone("+x");
  const bothMinus = await alone("-x");

  // No threshold to pick: whichever cube is nearer dominates. Seen from
  // −x the first cube is the near one, so that view stays near how the
  // first cube looks alone from −x; seen from +x the second cube is in
  // front of it, so that view leaves how the first looks alone from +x.
  expect(
    apart(bothMinus, aloneMinus),
    `alone ${alonePlus} / ${aloneMinus}, both ${bothPlus} / ${bothMinus}`,
  ).toBeLessThan(apart(bothPlus, alonePlus));
  expect(errors).toEqual([]);
});

test("a lit sheet is what the remove button takes away", async ({ page }) => {
  // The bin acts on what is selected: faces mean a block, a lit sheet means
  // the whole layer. One button rather than a ninth icon for a verb that is
  // the same verb.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // Cut it in two, so there is a sheet worth removing.
  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  const at = (await page.evaluate(
    (e) => (window as any).__gecko.edgeScreen(e),
    edge,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  for (let i = 1; i <= 6; i++)
    await page.mouse.move(box.x + at[0], box.y + at[1] - i * 10);
  await page.mouse.up();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  // One block left, and nothing still lit.
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  expect(await page.evaluate(() => (window as any).__gecko.sheet())).toEqual(
    [],
  );
  await expect(page.locator("#remove")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("clicking the background lets go of everything", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());
  const box = (await page.locator("#view").boundingBox())!;

  // A face: clicking away clears it.
  const face = (await page.evaluate(() => {
    const s = (window as any).__gecko.surface("faces");
    return { face: s.faces[0], block: s.blocks[0] };
  })) as { face: number; block: number };
  await page.evaluate((f) => (window as any).__gecko.choose(f), face);
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toHaveLength(1);
  await page.mouse.click(box.x + 20, box.y + box.height - 20);
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toEqual([]);

  // And a lit sheet, which used to stay lit because only the faces were
  // being let go of.
  const edge = (await page.evaluate(() => edgeNear([1, 0.5, 1]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  await page.mouse.click(box.x + 20, box.y + box.height - 20);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 0,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedEdge()),
  ).toBeNull();
  await expect(page.locator("#remove")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("a lit sheet says how many blocks it would take", async ({ page }) => {
  // The edges alone do not say that, and taking out more than you meant to
  // looks like a hole in the grid.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
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

async function openSquare(
  page: import("@playwright/test").Page,
  msh = SQUARE_MSH,
) {
  await page.locator("#msh").setInputFiles({
    name: "square.msh",
    mimeType: "text/plain",
    buffer: Buffer.from(msh),
  });
}

test("everything on screen is drawn against one origin, even when it moves", async ({
  page,
}) => {
  // The scene is sent rebased on a single origin so that f32 coordinates
  // stay small, and that origin **moves** when the model walks away from
  // it. Everything then has to be re-sent against the new one: the
  // geometry's own buffer has not changed, so nothing but this makes the
  // page fetch it again, and a geometry still rebased on where the middle
  // of the scene used to be is drawn an origin-shift away from the blocks.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  // A geometry a thousand kilometers away from that block: only its node
  // coordinates move, so it is the same square and still one surface.
  const far = SQUARE_MSH.split("$EndNodes")
    .map((part, at) =>
      at === 0
        ? part.replace(
            /^([1-4]) (\S+)/gm,
            (_, id: string, x: string) => `${id} ${Number(x) + 1e6}`,
          )
        : part,
    )
    .join("$EndNodes");
  await openSquare(page, far);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");

  // Both on the session's origin, wherever its hysteresis has left it.
  const between = (await page.evaluate(() =>
    (window as any).__gecko.origins(),
  )) as {
    view: number[];
    geometry: number[] | null;
  };
  expect(between.geometry).toEqual(between.view);

  // Something picked on the geometry, to be looked for again afterwards: a
  // re-centring fetches the geometry again, and a selection dropped over a
  // change of coordinates is a click lost for no reason anybody can see.
  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([{ dim: 2, tag: 1 }]),
  );

  // The block goes, and now the model *is* the geometry: the origin has to
  // follow it out there, which is the moment the two could part company.
  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    // Off the faces the viewer is drawing: blocks are not drawn by
    // default, and the block a face belongs to is listed beside it.
    const block = g.surface("faces").blocks[0];
    g.apply({ op: "remove_block", block });
    await g.refresh();
  });
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 0,
  );

  const after = (await page.evaluate(() =>
    (window as any).__gecko.origins(),
  )) as {
    view: number[];
    geometry: number[] | null;
  };
  expect(after.view[0]).toBeGreaterThan(999_999);
  expect(
    after.geometry,
    "the geometry is drawn against a stale origin",
  ).toEqual(after.view);
  expect(
    await page.evaluate(() => (window as any).__gecko.geomSelection()),
    "the selection survived the re-centring",
  ).toEqual([{ dim: 2, tag: 1 }]);
  expect(errors).toEqual([]);
});

test("a geometry is opened from a file and drawn in its three groups", async ({
  page,
}) => {
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

test("each geometry group is hidden, colored and faded on its own", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-curves]")).toHaveText("4");
  await openDrawer(page);

  const count = (group: string) =>
    page.evaluate((g) => (window as any).__gecko.geomCount(g), group);
  const look = (group: string, tag: number) =>
    page.evaluate(([g, t]) => (window as any).__gecko.geomLook(g, t), [
      group,
      tag,
    ] as const);

  // Hidden from the drawer, and the other two groups stay.
  await page.locator('[data-test="curves-visible"]').uncheck();
  expect(await count("curves")).toBe(0);
  expect(await count("surfaces")).toBe(1);
  await page.locator('[data-test="curves-visible"]').check();
  expect(await count("curves")).toBe(4);

  // One color for the lot, then one per tag.
  await openGroup(page, "curves");
  await page.locator('[data-test="curves-mode"]').selectOption("uniform");
  const uniform = await look("curves", 11);
  expect((await look("curves", 12))!.color).toBe(uniform!.color);
  await page.locator('[data-test="curves-mode"]').selectOption("id");
  expect((await look("curves", 12))!.color).not.toBe(
    (await look("curves", 11))!.color,
  );

  // And opacity, per group.
  await openGroup(page, "surfaces");
  await page.locator('[data-test="surfaces-alpha"]').fill("0.2");
  expect((await look("surfaces", 1))!.alpha).toBeCloseTo(0.2, 2);
  expect((await look("curves", 11))!.alpha).toBeCloseTo(1, 2);
  expect(errors).toEqual([]);
});

test("the geometry groups have keys of their own, and they are remembered", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");

  const count = (group: string) =>
    page.evaluate((g) => (window as any).__gecko.geomCount(g), group);
  await page.keyboard.press("Digit4");
  expect(await count("surfaces")).toBe(0);
  await expect(page.locator("#status")).toContainText(
    "geometric surfaces hidden",
  );
  await page.keyboard.press("Digit5");
  expect(await count("curves")).toBe(0);
  await page.keyboard.press("Digit6");
  expect(await count("corners")).toBe(0);

  // Reloaded, the settings come back — and the geometry does not, because
  // it lives in the kernel and a static page's kernel goes with the tab.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().surfaces.visible),
  ).toBe(false);
  expect(errors).toEqual([]);
});

test("a file that is not a gmsh mesh is refused and says why", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page, "hello, I am not a mesh\n");
  await expect(page.locator("#status")).toContainText("$MeshFormat");
  expect(
    await page.evaluate(() => (window as any).__gecko.geomCount("surfaces")),
  ).toBe(0);

  // A modern gmsh writes 4.1 by default, so this is the message most likely
  // to be read in anger: it has to say what to do about it.
  await openSquare(page, SQUARE_MSH.replace("2.2 0 8", "4.1 0 8"));
  await expect(page.locator("#status")).toContainText("-format msh2");
  expect(errors).toEqual([]);
});

test("framing takes in the geometry as well as the blocks", async ({
  page,
}) => {
  const errors = await boot(page);
  // The square sits at the origin; put a cube well away from it.
  await page.evaluate(() =>
    (window as any).__gecko.apply({
      op: "create_cube",
      origin: [20, 0, 0],
      size: 1,
    }),
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await openSquare(page);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Far enough back to hold both, which is more than either alone needs.
  const radius = (await page.evaluate(
    () => (window as any).__gecko.camera().radius,
  )) as number;
  expect(radius).toBeGreaterThan(20);
  expect(errors).toEqual([]);
});

/**
 * Where the picked face is on screen, and which way **out of the model** is
 * on screen from there.
 *
 * Read from the face opposite it: on a cube the line between the two face
 * centers is the normal, so the direction from the far one to the picked one
 * is outward, whatever angle the camera happens to be at. Guessing instead —
 * "up the screen is up the model" — is wrong here, because the scene's up is
 * Y and the model's is Z.
 */
const OUTWARD = `
window.outward = (picked) => {
  const s = window.__gecko.surface("faces");
  const center = (q) => {
    const at = [0, 0, 0];
    for (let k = 0; k < 4; k++)
      for (let axis = 0; axis < 3; axis++) at[axis] += s.positions[(q * 4 + k) * 3 + axis] / 4;
    return at;
  };
  let mine = 0;
  for (let q = 0; q < s.quads; q++) if (s.faces[q] === picked) mine = q;
  let far = mine;
  const gap = (q) => {
    const [a, b] = [center(q), center(mine)];
    return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
  };
  for (let q = 0; q < s.quads; q++) if (gap(q) > gap(far)) far = q;
  const here = window.__gecko.faceScreen(picked);
  const behind = window.__gecko.faceScreen(s.faces[far]);
  const length = Math.hypot(here[0] - behind[0], here[1] - behind[1]) || 1;
  return { at: here, dir: [(here[0] - behind[0]) / length, (here[1] - behind[1]) / length] };
};`;

declare function outward(face: number): {
  at: [number, number];
  dir: [number, number];
};

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
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // Clicked first, which is what arms the gesture: an unselected face is
  // how you turn the view, and a face fills the screen. Which face it is
  // comes from the click rather than from this test's idea of the angle.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await page.waitForFunction(
    () => (window as any).__gecko.selection().length === 1,
  );
  const picked = (await page.evaluate(
    () => (window as any).__gecko.selection()[0],
  )) as {
    face: number;
  };
  const out = (await page.evaluate((f) => outward(f), picked.face)) as {
    at: [number, number];
    dir: [number, number];
  };

  const before = (await page.evaluate(() =>
    (window as any).__gecko.camera(),
  )) as {
    alpha: number;
    beta: number;
  };
  await page.mouse.move(box.x + out.at[0], box.y + out.at[1]);
  await page.mouse.down();
  for (const step of [20, 50, 90]) {
    await page.mouse.move(
      box.x + out.at[0] + out.dir[0] * step,
      box.y + out.at[1] + out.dir[1] * step,
    );
  }
  // The preview says how far before anything is built.
  const asked = (await page.evaluate(() =>
    (window as any).__gecko.grownBy(),
  )) as number;
  expect(asked).toBeGreaterThan(0.2);
  await expect(page.locator("#status")).toContainText("let go to build");
  await page.mouse.up();

  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("[data-test=nodes]")).toHaveText("12");
  // Welded, not merely standing on it: eleven faces, not twelve.
  await expect(page.locator("[data-test=faces]")).toHaveText("11");

  // And it grew by what the preview promised, along the face's own normal —
  // which for a cube lengthens exactly one side of the model.
  expect((await page.evaluate(() => span())) as number).toBeCloseTo(
    1 + asked,
    3,
  );

  // The view did not turn while the face was being dragged.
  const after = (await page.evaluate(() =>
    (window as any).__gecko.camera(),
  )) as {
    alpha: number;
    beta: number;
  };
  expect(after.alpha).toBeCloseTo(before.alpha, 6);
  expect(after.beta).toBeCloseTo(before.beta, 6);
  expect(errors).toEqual([]);
});

test("pressing a selected face without moving lets go of it", async ({
  page,
}) => {
  // The gesture has to leave the plain click alone: press and release on
  // the spot is how a selection is undone, extrusion or no extrusion.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  const middle: [number, number] = [
    box.x + box.width / 2,
    box.y + box.height / 2,
  ];
  await page.mouse.click(...middle);
  await page.waitForFunction(
    () => (window as any).__gecko.selection().length === 1,
  );

  await page.mouse.click(...middle);
  await page.waitForFunction(
    () => (window as any).__gecko.selection().length === 0,
  );
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
  for (const id of ["picking", "box"]) {
    await expect(page.locator(`#${id}`)).toBeDisabled();
  }
  await openCube(page);
  for (const id of ["picking", "box"]) {
    await expect(page.locator(`#${id}`)).toBeEnabled();
  }
  expect(errors).toEqual([]);
});

test("aiming at the geometry picks surfaces, and the blocks stay out of it", async ({
  page,
}) => {
  const errors = await boot(page);
  await openCube(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  // Over a face of the block, which sits **inside** the geometry: the ray
  // there meets a face of the cube and a surface of the model both, which
  // is what lets one point mean two things in the two modes. Not the middle
  // of the view — this geometry is three units deep and the block sits in a
  // corner of it, so the middle of the view is thin air.
  //
  // And not the face's own center either: a click asks for a node, then an
  // edge, then a face, and the far edges of a cube project into the middle
  // of the faces in front of them. So the point is chosen by asking the
  // page what is there — somewhere on the face that no handle and no line
  // has a claim on.
  const at = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    const face = g.surface("faces").faces[0];
    const middle = g.faceScreen(face) as [number, number];
    for (const away of [0, 6, 10, 14, 18]) {
      for (const [dx, dy] of [
        [0, 0],
        [1, 0],
        [0, 1],
        [-1, 0],
        [0, -1],
        [1, 1],
        [-1, -1],
        [1, -1],
        [-1, 1],
      ]) {
        const p: [number, number] = [
          middle[0] + dx * away,
          middle[1] + dy * away,
        ];
        const free =
          g.pickNode(p[0], p[1]) === null && g.pickEdge(p[0], p[1]) === null;
        if (free && g.pickSurface(p[0], p[1])?.face === face) return p;
      }
    }
    return null;
  })) as [number, number] | null;
  expect(
    at,
    "somewhere on a face of the block that a click would take",
  ).not.toBeNull();
  const middle: [number, number] = [box.x + at![0], box.y + at![1]];

  // Blocks first: the click takes a face of the cube, not the geometry
  // around it.
  await page.mouse.click(...middle);
  await page.waitForFunction(
    () => (window as any).__gecko.selection().length === 1,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.surfaceSelection()),
  ).toEqual([]);

  // Which mode you are in is said twice over, because a toggle that looks
  // the same either way is one you click to find out about. Contains
  // rather than equals: the pointer is still over the face it just
  // clicked, so the line also carries that face's name.
  await expect(page.locator("[data-test=aim]")).toContainText("picking blocks");
  await expect(page.locator("#picking .as-blocks")).toBeVisible();
  await expect(page.locator("#picking .as-geometry")).toBeHidden();

  // Switched over, and the same click takes a surface instead.
  await page.keyboard.press("m");
  await expect(page.locator("[data-test=aim]")).toContainText(
    "picking geometry",
  );
  await expect(page.locator("#picking .as-geometry")).toBeVisible();
  await expect(page.locator("#picking .as-blocks")).toBeHidden();
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe(
    "geometry",
  );
  await expect(page.locator("#picking")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  // The face stays picked through the change of mode: a handful of cells
  // and the entity to put them on are chosen in different modes and used
  // together. `Esc` is how you let go.
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toHaveLength(1);

  await page.mouse.click(...middle);
  await page.waitForFunction(
    () => (window as any).__gecko.surfaceSelection().length === 1,
  );
  const [tag] = (await page.evaluate(() =>
    (window as any).__gecko.surfaceSelection(),
  )) as number[];
  await expect(page.locator("#status")).toContainText(
    `surface ${tag} selected`,
  );
  // And it is drawn in the selection color, like a picked face.
  expect(
    (await page.evaluate(
      (t) => (window as any).__gecko.geomLook("surfaces", t),
      tag,
    )) as {
      color: string;
    },
  ).toMatchObject({ color: "#ffcc33" });
  // Nothing of the blocking answered the click: the face selection is the
  // one from before the change of mode, not a second one.
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toHaveLength(1);

  // Clicking it again puts it out, and the background lets go of the lot.
  await page.mouse.click(...middle);
  await page.waitForFunction(
    () => (window as any).__gecko.surfaceSelection().length === 0,
  );
  expect(errors).toEqual([]);
});

test("a box is built round the geometry, or round what is selected", async ({
  page,
}) => {
  const errors = await boot(page);
  await openCube(page);

  // Nothing selected: the whole geometry, with the kernel's own margin.
  await page.locator("#box").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("#status")).toContainText("the whole geometry");

  // And one round a selection, which has to be two surfaces at least or
  // the box has no thickness.
  await page.keyboard.press("m");
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.selectGeom([
      { dim: 2, tag: 1 },
      { dim: 2, tag: 2 },
    ]);
  });
  await page.keyboard.press("b");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await expect(page.locator("#status")).toContainText("2 surfaces");
  expect(errors).toEqual([]);
});

test("a box that cannot be built says why and builds nothing", async ({
  page,
}) => {
  const errors = await boot(page);
  await openCube(page);
  await page.keyboard.press("m");
  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([{ dim: 2, tag: 1 }]),
  );
  await page.keyboard.press("b");
  await expect(page.locator("#status")).toContainText("flat");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  expect(errors).toEqual([]);
});

test("each group of cells is its own box, not nested inside the last", async ({
  page,
}) => {
  // An unclosed element is not a parse error — it is an element that
  // swallows whatever comes after it. The boxes came out one inside the
  // next, which reads as though edges were part of blocks were part of
  // faces, and they are nothing of the sort.
  const errors = await boot(page);
  await openDrawer(page);

  // Blocks, faces, edges and nodes.
  await expect(page.locator("#layers > .layer")).toHaveCount(4);
  expect(await page.locator("#layers .layer .layer").count()).toBe(0);
  await expect(page.locator("#geometry > .layer")).toHaveCount(3);
  expect(await page.locator("#geometry .layer .layer").count()).toBe(0);

  // Side by side, so they all start at the same place on screen.
  const lefts = await page
    .locator("#layers > .layer")
    .evaluateAll((all) => all.map((f) => f.getBoundingClientRect().left));
  expect(new Set(lefts).size).toBe(1);
  expect(errors).toEqual([]);
});

/** The unit cube's boundary, every entity of it named: eight corners
 *  (101..108), twelve curves (201..212), six surfaces (1..6). */
const BOX_MSH = `$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
8
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
5 0 0 1
6 1 0 1
7 1 1 1
8 0 1 1
$EndNodes
$Elements
32
1 15 2 0 101 1
2 15 2 0 102 2
3 15 2 0 103 3
4 15 2 0 104 4
5 15 2 0 105 5
6 15 2 0 106 6
7 15 2 0 107 7
8 15 2 0 108 8
9 1 2 0 201 1 2
10 1 2 0 202 2 3
11 1 2 0 203 3 4
12 1 2 0 204 4 1
13 1 2 0 205 5 6
14 1 2 0 206 6 7
15 1 2 0 207 7 8
16 1 2 0 208 8 5
17 1 2 0 209 1 5
18 1 2 0 210 2 6
19 1 2 0 211 3 7
20 1 2 0 212 4 8
21 2 2 0 1 1 2 3
22 2 2 0 1 1 3 4
23 2 2 0 2 5 6 7
24 2 2 0 2 5 7 8
25 2 2 0 3 1 2 6
26 2 2 0 3 1 6 5
27 2 2 0 4 2 3 7
28 2 2 0 4 2 7 6
29 2 2 0 5 3 4 8
30 2 2 0 5 3 8 7
31 2 2 0 6 4 1 5
32 2 2 0 6 4 5 8
$EndElements
`;

async function boxedAndPinned(page: import("@playwright/test").Page) {
  await page.locator("#msh").setInputFiles({
    name: "box.msh",
    mimeType: "text/plain",
    buffer: Buffer.from(BOX_MSH),
  });
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("6");
  // A box resting exactly on it, so every corner is already where it
  // belongs and the tolerance has nothing to forgive.
  await page.evaluate(() =>
    (window as any).__gecko.apply({
      op: "box_geometry",
      surfaces: [],
      margin: 0,
    }),
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await page.evaluate(() => (window as any).__gecko.settled());
}

test("the blocking is put on the geometry and wears what it follows", async ({
  page,
}) => {
  const errors = await boot(page);
  await boxedAndPinned(page);
  await expect(page.locator("[data-test=pinned]")).toHaveText("0");

  await page.keyboard.press("k");
  await expect(page.locator("#status")).toContainText(
    "8 nodes, 12 edges and 6 faces",
  );
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");

  // Every corner of the block is on a corner of the model, and wears its
  // color rather than the wireframe's.
  const state = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return g.names().map((n: number) => g.nodeState(n));
  })) as { on: { dim: number; tag: number } | null; frozen: boolean }[];
  expect(state.every((s) => s.on?.dim === 0)).toBe(true);
  expect(state.every((s) => !s.frozen)).toBe(true);
  expect(errors).toEqual([]);
});

test("a frozen node is a cube, and still wears what the coloring says", async ({
  page,
}) => {
  // Freezing used to paint the handle gray, which hid the very thing the
  // coloring is for. Now it changes the handle's shape, and the color is
  // left to say what the node is on.
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.keyboard.press("k");
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");

  const name = (await page.evaluate(
    () => (window as any).__gecko.names()[0],
  )) as number;
  const other = (await page.evaluate(
    () => (window as any).__gecko.names()[1],
  )) as number;
  const shape = (n: number) =>
    page.evaluate((who) => (window as any).__gecko.nodeShape(who), n);
  const tint = (n: number) =>
    page.evaluate((who) => (window as any).__gecko.nodeTint(who), n);

  // Every corner of the block is on a corner of the model, so under the
  // `follows` coloring every handle wears the corner color — set from
  // the drawer, the way somebody would.
  await openDrawer(page);
  await openGroup(page, "nodes");
  await page.locator('[data-test="nodes-mode"]').selectOption("class");
  const theme = (await page.evaluate(() =>
    (window as any).__gecko.theme(),
  )) as {
    onCorner: string;
  };
  expect(await shape(name)).toBe("ball");
  expect(await tint(name)).toBe(theme.onCorner.toLowerCase());

  // Frozen through the kernel, since what is under test is the drawing.
  await page.evaluate(async (who) => {
    const g = (window as any).__gecko;
    g.apply({ op: "freeze_node", nodes: [who], frozen: true });
    await g.refresh();
  }, name);
  await page.waitForFunction(
    (n) => (window as any).__gecko.nodeState(n).frozen,
    name,
  );

  // A cube where it was a ball, and the color unchanged — while the node
  // beside it is still a ball.
  expect(await shape(name)).toBe("cube");
  expect(await shape(other)).toBe("ball");
  expect(await tint(name)).toBe(theme.onCorner.toLowerCase());
  expect(errors).toEqual([]);
});

test("a node is picked, locked, and taken off the model", async ({ page }) => {
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.keyboard.press("k");
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");

  // There was no such thing as a picked node until freezing needed one:
  // a press and release on a handle, which the drag behavior has already
  // decided was not a drag.
  const name = (await page.evaluate(
    () => (window as any).__gecko.names()[0],
  )) as number;
  const at = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    name,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(
    (n) => (window as any).__gecko.selectedNodes().includes(n),
    name,
  );
  await expect(page.locator("#status")).toContainText("on corner");

  // Locked, and it stays locked until it is let go.
  await page.keyboard.press("l");
  await expect(page.locator("#status")).toContainText("frozen");
  await page.waitForFunction(
    (n) => (window as any).__gecko.nodeState(n).frozen,
    name,
  );
  await page.keyboard.press("l");
  await page.waitForFunction(
    (n) => !(window as any).__gecko.nodeState(n).frozen,
    name,
  );

  // And off the model, which is something you say rather than something
  // that happens to you by dragging.
  await page.keyboard.press("u");
  await page.waitForFunction(
    (n) => (window as any).__gecko.nodeState(n).on === null,
    name,
  );
  await expect(page.locator("[data-test=pinned]")).toHaveText("7");
  expect(errors).toEqual([]);
});

test("the classify button waits for a geometry", async ({ page }) => {
  const errors = await boot(page);
  await expect(page.locator("#classify")).toBeDisabled();
  await boxedAndPinned(page);
  await expect(page.locator("#classify")).toBeEnabled();
  await page.locator("#classify").click();
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");
  expect(errors).toEqual([]);
});

test("the window splits into four views of the one scene", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  expect(await page.evaluate(() => (window as any).__gecko.split())).toBe(
    false,
  );
  await expect(page.locator("#panes")).toBeHidden();

  await page.keyboard.press("v");
  await expect(page.locator("#panes")).toBeVisible();
  await expect(page.locator("#panes span")).toHaveText(["3D", "X", "Y", "Z"]);
  expect(await page.evaluate(() => (window as any).__gecko.split())).toBe(true);

  // Which pane the pointer is in is what everything else follows from.
  const box = (await page.locator("#view").boundingBox())!;
  const corner = async (x: number, y: number) => {
    await page.mouse.move(box.x + x, box.y + y);
    return page.evaluate(() => (window as any).__gecko.pane());
  };
  expect(await corner(box.width * 0.25, box.height * 0.3)).toBe("3d");
  expect(await corner(box.width - 20, 20)).toBe("x");
  expect(await corner(20, box.height - 20)).toBe("y");
  expect(await corner(box.width - 20, box.height - 20)).toBe("z");

  // The three axis views have no perspective, which is what makes them
  // worth having; the 3-D one does. Aimed clear of the toolbar, which is
  // over the top left of that pane and takes the pointer events itself.
  await page.mouse.move(box.x + box.width * 0.25, box.y + box.height * 0.3);
  expect(
    await page.evaluate(() => (window as any).__gecko.paneCamera().ortho),
  ).toBe(false);
  await page.mouse.move(box.x + box.width - 20, box.y + 20);
  expect(
    await page.evaluate(() => (window as any).__gecko.paneCamera().ortho),
  ).toBe(true);

  // Remembered, like every other thing about how it looks.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(await page.evaluate(() => (window as any).__gecko.split())).toBe(true);
  expect(errors).toEqual([]);
});

test("dragging a node in an axis view keeps it in the plane of the screen", async ({
  page,
}) => {
  // The reason for the split: two coordinates move and the third cannot,
  // so placing a corner is a matter of looking at it rather than of
  // guessing how far away it ended up.
  //
  // Both halves are asserted, because the outcome alone would hold for
  // the wrong reason: an orthographic camera locked to an axis already
  // faces that plane, so a drag left to face the camera would keep the
  // third coordinate too. What is checked below is that the plane is the
  // **pane's**, said rather than inherited.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  // Into the Z pane — the bottom right — which looks down Z, so Z is what
  // must not move.
  await page.mouse.move(box.x + box.width * 0.75, box.y + box.height * 0.75);
  expect(await page.evaluate(() => (window as any).__gecko.pane())).toBe("z");
  expect(
    await page.evaluate(() => (window as any).__gecko.dragPlane()),
  ).toEqual([0, 0, 1]);
  // And in the 3-D pane there is no plane to name: it faces the camera,
  // wherever that is.
  await page.mouse.move(box.x + box.width * 0.25, box.y + box.height * 0.3);
  expect(
    await page.evaluate(() => (window as any).__gecko.dragPlane()),
  ).toBeNull();
  await page.mouse.move(box.x + box.width * 0.75, box.y + box.height * 0.75);

  // Where the nodes are before, and where one of them is on screen. Which
  // one the view answers for at that pixel is its business — looking down
  // Z, the nearest of a column of them wins — so what moved is worked out
  // afterwards rather than named in advance.
  const before = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return (g.names() as number[]).map((n) => ({ n, at: g.nodePosition(n) }));
  })) as { n: number; at: [number, number, number] }[];
  const aim = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    before[0].n,
  )) as [number, number];

  await page.mouse.move(box.x + aim[0], box.y + aim[1]);
  await page.mouse.down();
  await page.mouse.move(box.x + aim[0] + 35, box.y + aim[1] - 25, { steps: 4 });
  await page.mouse.up();
  await page.waitForFunction(() => (window as any).__gecko.revision() > 1);

  const after = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return (g.names() as number[]).map((n) => ({ n, at: g.nodePosition(n) }));
  })) as { n: number; at: [number, number, number] }[];

  const moved = before.filter((b, i) => {
    const a = after[i].at;
    return Math.hypot(a[0] - b.at[0], a[1] - b.at[1], a[2] - b.at[2]) > 1e-9;
  });
  expect(moved, "exactly one node should have moved").toHaveLength(1);
  const was = moved[0].at;
  const now = after.find((a) => a.n === moved[0].n)!.at;
  expect(now[2], "z moved in a view looking down Z").toBeCloseTo(was[2], 6);
  expect(Math.hypot(now[0] - was[0], now[1] - was[1])).toBeGreaterThan(0.05);
  expect(errors).toEqual([]);
});

test("a face is picked in an axis view, through that view's own camera", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // The middle of the Y pane, which is looking straight at the model.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + box.width * 0.25, box.y + box.height * 0.75);
  await page.waitForFunction(
    () => (window as any).__gecko.selection().length === 1,
  );
  expect(errors).toEqual([]);
});

test("the help card says what the classification colors mean", async ({
  page,
}) => {
  // A color code nobody can look up is a color code nobody reads. It is
  // generated from the theme, so it cannot come to disagree with the
  // screen — the same reason the keys beside it are generated from the
  // table they are matched against.
  const errors = await boot(page);
  await page.keyboard.press("h");
  await expect(page.locator("#help-legend .key")).toHaveCount(4);
  await expect(page.locator("#help-legend")).toContainText("on a corner");
  await expect(page.locator("#help-legend")).toContainText("on a curve");
  await expect(page.locator("#help-legend")).toContainText("on a surface");
  // And the one thing about a node that is **not** a color is said in
  // words, beside them.
  await expect(page.locator("[data-test=legend-note]")).toContainText("frozen");
  await expect(page.locator("[data-test=legend-note]")).toContainText("cube");

  // The swatch really is the color the scene uses.
  const first = await page
    .locator("#help-legend .key i")
    .first()
    .evaluate((e) => (e as HTMLElement).style.background);
  const theme = (await page.evaluate(() =>
    (window as any).__gecko.theme(),
  )) as {
    onCorner: string;
  };
  // The browser hands a style back as `rgb(…)` whatever it was set with.
  const rgb = (hex: string) =>
    `rgb(${[1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16)).join(",")})`;
  expect(first.replace(/\s/g, "")).toBe(rgb(theme.onCorner));
  expect(errors).toEqual([]);
});

test("nodes, edges and faces each take three colorings", async ({ page }) => {
  const errors = await boot(page);
  await openCube(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await openDrawer(page);

  // Four for each of the three — one color, its id, its classification,
  // its group — and three for blocks: a block is classified on nothing, a
  // surface mesh having no volumes to put it on.
  const offered = (group: string) =>
    page
      .locator(`[data-test="${group}-mode"] option`)
      .evaluateAll((all) => all.map((o) => (o as HTMLOptionElement).value));
  for (const group of ["nodes", "edges", "faces"]) {
    expect(await offered(group)).toEqual(["uniform", "id", "class", "group"]);
  }
  expect(await offered("blocks")).toEqual(["uniform", "id", "group"]);
  // And the geometry's own groups, which are classified on nothing and in
  // no group: a geometry is the thing a blocking is put on, not part of
  // it.
  expect(await offered("curves")).toEqual(["uniform", "id"]);

  // And choosing one is remembered, like every other thing about how it
  // looks.
  await openGroup(page, "nodes");
  await page.locator('[data-test="nodes-mode"]').selectOption("class");
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().nodes.coloring),
  ).toBe("class");
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().nodes.coloring),
  ).toBe("class");
  expect(errors).toEqual([]);
});

test("an edge is picked and its sheet cut from an axis view", async ({
  page,
}) => {
  // Everything works in every pane or the split is half a feature. Each
  // pick goes through the camera of the pane the pointer is in; it was
  // the edges that were still asking the 3-D one, so clicking a line in
  // a side view did nothing at all.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // Into the Y pane, bottom left, and ask it where an edge is.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + box.width * 0.25, box.y + box.height * 0.75);
  expect(await page.evaluate(() => (window as any).__gecko.pane())).toBe("y");

  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  const at = (await page.evaluate(
    (e) => (window as any).__gecko.edgeScreen(e),
    edge,
  )) as [number, number];
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 4,
  );

  // And dragging along it there cuts, as it does in the 3-D view.
  const ends = (await page.evaluate(
    (e) => (window as any).__gecko.edgeEndsOnScreen(e),
    edge,
  )) as [[number, number], [number, number]];
  const along = (t: number) =>
    [
      box.x + ends[0][0] + (ends[1][0] - ends[0][0]) * t,
      box.y + ends[0][1] + (ends[1][1] - ends[0][1]) * t,
    ] as const;
  await page.mouse.move(...along(0.05));
  await page.mouse.down();
  await page.mouse.move(...along(0.5), { steps: 5 });
  await page.mouse.up();

  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  expect(errors).toEqual([]);
});

test("a change can be taken back, and the button says when there is one", async ({
  page,
}) => {
  const errors = await boot(page);
  // Nothing has happened, so there is nothing to take back.
  await expect(page.locator("#undo")).toBeDisabled();

  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await expect(page.locator("#undo")).toBeEnabled();

  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );

  // The keyboard, with the modifier everybody already knows. Bare Z is a
  // view down the Z axis and has to stay one.
  await page.keyboard.press("ControlOrMeta+z");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("#status")).toContainText("undone");

  await page.locator("#undo").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  // Back at the beginning, and the button has nothing left to offer.
  await expect(page.locator("#undo")).toBeDisabled();
  expect(errors).toEqual([]);
});

test("a cut is taken back whole, and the names come back with it", async ({
  page,
}) => {
  // The point of keeping states rather than replaying commands: the cells
  // answer to the same numbers afterwards. A model rebuilt from a
  // description of itself would renumber them, and every name this page is
  // holding — the lit sheet, the selected node — would come to mean
  // something else.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  // Every node's name, and where the scene has it.
  const dump = () =>
    page.evaluate(() => {
      const g = (window as any).__gecko;
      return (g.names() as number[])
        .sort((a, b) => a - b)
        .map((n) => `${n} ${g.nodePosition(n)}`);
    });
  const before = await dump();
  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate(
    (e) =>
      (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }),
    edge,
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");

  await page.keyboard.press("ControlOrMeta+z");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  expect(await dump()).toEqual(before);

  // And the same cut again, on the same edge, which still names it.
  await page.evaluate(
    (e) =>
      (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }),
    edge,
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  expect(errors).toEqual([]);
});

test("how far back it goes is a setting, and it is remembered", async ({
  page,
}) => {
  const errors = await boot(page);
  await openParams(page);
  await expect(page.locator("[data-test=history-steps]")).toHaveText("5");

  // One step, and then two changes: only the second can be taken back.
  await page.locator("[data-test=history]").fill("1");
  await page.locator("[data-test=history]").dispatchEvent("input");
  await expect(page.locator("[data-test=history-steps]")).toHaveText("1");
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );

  await page.locator("#undo").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("#undo")).toBeDisabled();

  // And the setting outlives the page, like every other preference.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  await openParams(page);
  await expect(page.locator("[data-test=history-steps]")).toHaveText("1");
  expect(errors).toEqual([]);
});

test("the drawer folds, and what is open is remembered", async ({ page }) => {
  // Seven groups of controls unfolded at once is a drawer you scroll rather
  // than read. Folded, the whole of it fits in the window.
  const errors = await boot(page);
  await openDrawer(page);

  const open = (id: string) =>
    page.locator(`#${id}`).evaluate((d) => (d as HTMLDetailsElement).open);
  expect(await open("sec-cells")).toBe(true);
  expect(await open("sec-geometry")).toBe(true);
  expect(await open("sec-colors")).toBe(false);
  expect(await open("grp-faces")).toBe(false);

  // It fits: no scrolling, which is the whole complaint this answers.
  const fits = await page
    .locator("#drawer")
    .evaluate((d) => d.scrollHeight <= d.clientHeight + 1);
  expect(fits, "the folded drawer still scrolls").toBe(true);

  // A folded group still says what it is: what you opened it to find out.
  // Faces start on their classification, so that is what it says.
  await expect(page.locator("#grp-faces .digest")).toHaveText(
    "classification · 35%",
  );
  // And the longest of those words still fits on the line: the drawer is
  // 246 pixels wide and the word is the longest thing that goes there.
  const spills = await page
    .locator("#grp-faces")
    .evaluate((row) => row.scrollWidth > row.clientWidth + 1);
  expect(spills, "the folded line spills over the drawer's width").toBe(false);
  await expect(page.locator("#grp-blocks .digest")).toHaveText("hidden");

  // Its show box is on that line, and toggling it does not unfold anything.
  await page.locator("[data-test=blocks-visible]").check();
  expect(await open("grp-blocks")).toBe(false);
  await expect(page.locator("#grp-blocks .digest")).toHaveText("id · 55%");

  // One group of a band at a time: opening the next closes the last.
  await openGroup(page, "faces");
  await openGroup(page, "edges");
  expect(await open("grp-faces")).toBe(false);

  // And the folds outlive the page, like every other preference. Written
  // when the browser says a fold moved — `toggle` is a queued task, not
  // part of the click — so the preference is waited for rather than
  // assumed to be there already.
  await expect
    .poll(() => page.evaluate(() => (window as any).__gecko.prefs().panels))
    .toContain("grp-edges");
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  await openDrawer(page);
  expect(await open("grp-edges")).toBe(true);
  expect(await open("grp-faces")).toBe(false);
  expect(errors).toEqual([]);
});

test("the two panels are one at a time, and the cog has its own key", async ({
  page,
}) => {
  // They are drawn in the same place: two sheets of glass on top of each
  // other is a bug that looks like a rendering fault.
  const errors = await boot(page);
  await page.locator("#display").click();
  await expect(page.locator("#drawer")).toBeVisible();

  await page.locator("#settings").click();
  await expect(page.locator("#params")).toBeVisible();
  await expect(page.locator("#drawer")).toBeHidden();
  await expect(page.locator("#settings")).toHaveAttribute(
    "aria-expanded",
    "true",
  );

  await page.keyboard.press("Tab");
  await expect(page.locator("#drawer")).toBeVisible();
  await expect(page.locator("#params")).toBeHidden();

  // And the comma is the cog, as it is everywhere else.
  await page.keyboard.press(",");
  await expect(page.locator("#params")).toBeVisible();
  await expect(page.locator("#drawer")).toBeHidden();
  expect(errors).toEqual([]);
});

test("a fresh window starts on the classification, with a box that hugs", async ({
  page,
}) => {
  // What somebody sees the first time: putting the blocking onto the model
  // is what the session is for, so nodes, edges and faces say what they
  // are classified on before they say anything else. Blocks keep their own
  // coloring — a block is classified on nothing.
  const errors = await boot(page);
  const prefs = (await page.evaluate(() =>
    (window as any).__gecko.prefs(),
  )) as {
    nodes: { coloring: string };
    edges: { coloring: string };
    faces: { coloring: string };
    blocks: { coloring: string };
    box: { margin: number };
  };
  expect(prefs.nodes.coloring).toBe("class");
  expect(prefs.edges.coloring).toBe("class");
  expect(prefs.faces.coloring).toBe("class");
  expect(prefs.blocks.coloring).toBe("id");
  // And a bounding box that hugs the model: two percent of each half-side.
  expect(prefs.box.margin).toBeCloseTo(0.02, 12);
  await openParams(page);
  await expect(page.locator("[data-test=box-margin-value]")).toHaveText("2%");
  expect(errors).toEqual([]);
});

test("how big the handles and the edges are drawn is a setting", async ({
  page,
}) => {
  // Both in pixels on the screen rather than in model units: what they are
  // for is being easy to see and easy to hit, and that is a question about
  // the screen. A blocking with a lot in it wants thin, a projector wants
  // thick.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await openParams(page);

  // What a handle measures on screen, in CSS pixels: the sizing pass puts
  // every one of them at the asked-for size whatever the zoom, so two
  // corners of one at a known size are that many pixels apart.
  // Waited for rather than read straight away: the handles are resized in
  // the pass before a camera renders, so the answer is right one frame
  // after the setting changes, not in the same breath as it.
  const drawnAt = (px: number) =>
    page.waitForFunction(
      (want) => {
        const g = (window as any).__gecko;
        return Math.abs(g.handleAcross(g.names()[0]) - want) < 0.5;
      },
      px,
      { timeout: 5000 },
    );
  await expect(page.locator("[data-test=node-size-value]")).toHaveText("11px");
  await drawnAt(11);

  await page.locator("[data-test=node-size]").fill("20");
  await page.locator("[data-test=node-size]").dispatchEvent("input");
  await expect(page.locator("[data-test=node-size-value]")).toHaveText("20px");
  await drawnAt(20);

  // The edges are a ribbon, so their width is a number they can be given —
  // a GL line is one pixel whatever anybody asks for, which is why they
  // are no longer GL lines.
  await expect(page.locator("[data-test=edge-width-value]")).toHaveText("2px");
  const narrow = (await page.evaluate(() =>
    (window as any).__gecko.edgeRibbonWidth(),
  )) as number;
  await page.locator("[data-test=edge-width]").fill("6");
  await page.locator("[data-test=edge-width]").dispatchEvent("input");
  await expect(page.locator("[data-test=edge-width-value]")).toHaveText("6px");
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().edges.width),
  ).toBe(6);
  const wide = (await page.evaluate(() =>
    (window as any).__gecko.edgeRibbonWidth(),
  )) as number;
  // Drawn at it, not merely remembered. Measured as a ratio rather than in
  // pixels, so the assertion does not depend on how many device pixels the
  // runner's screen puts in one of the pointer's.
  expect(wide / narrow).toBeCloseTo(3, 1);

  // And both are remembered, like every other thing about how it looks.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().nodes.size),
  ).toBe(20);
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().edges.width),
  ).toBe(6);
  expect(errors).toEqual([]);
});

test("the box margin in the settings panel is the one the box is built to", async ({
  page,
}) => {
  // The kernel's constant is what you get when nobody says; this is
  // somebody saying, and it has to reach the command.
  const errors = await boot(page);
  await openCube(page); // a 2 × 1 × 3 box of a geometry
  await openParams(page);

  /** How wide the blocking is, on each axis. Rebasing-proof: a difference
   *  of two positions does not care where the origin went. */
  const extent = async () => {
    const names = (await page.evaluate(() =>
      (window as any).__gecko.names(),
    )) as number[];
    const at = await Promise.all(
      names.map((n) =>
        page.evaluate((m) => (window as any).__gecko.nodePosition(m), n),
      ),
    );
    return [0, 1, 2].map((axis) => {
      const all = (at as number[][]).map((p) => p[axis]);
      return Math.max(...all) - Math.min(...all);
    });
  };

  await page.locator("[data-test=box-margin]").fill("0");
  await page.locator("[data-test=box-margin]").dispatchEvent("input");
  await expect(page.locator("[data-test=box-margin-value]")).toHaveText("0%");
  await page.locator("#box").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  for (const [axis, want] of [2, 1, 3].entries()) {
    expect((await extent())[axis]).toBeCloseTo(want, 5);
  }

  // Half a half-side of room all round: every extent grows by half.
  await page.keyboard.press("ControlOrMeta+z");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  await page.locator("[data-test=box-margin]").fill("0.5");
  await page.locator("[data-test=box-margin]").dispatchEvent("input");
  await expect(page.locator("[data-test=box-margin-value]")).toHaveText("50%");
  await page.locator("#box").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  for (const [axis, want] of [3, 1.5, 4.5].entries()) {
    expect((await extent())[axis]).toBeCloseTo(want, 5);
  }
  expect(errors).toEqual([]);
});

/** Every node's name, where the kernel has it, and where it is on screen. */
async function handles(page: import("@playwright/test").Page) {
  return (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return (g.names() as number[]).map((name) => ({
      name,
      at: g.nodePosition(name) as [number, number, number],
      on: g.nodeScreen(name) as [number, number],
    }));
  })) as { name: number; at: [number, number, number]; on: [number, number] }[];
}

test("a curve is picked, not the surfaces it bounds", async ({ page }) => {
  // Smallest target first, as everywhere else: a curve lies exactly on the
  // surfaces it bounds, so whichever answered first would hide the other —
  // and it is the curve you had to aim at.
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.evaluate(() => (window as any).__gecko.snapTo("+z"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // The box rests exactly on the geometry, so the midpoint between two
  // handles that share an edge is a point on the curve between them.
  const nodes = await handles(page);
  const a = nodes.find((n) => n.at.every((v) => Math.abs(v) < 1e-6))!;
  const b = nodes.find(
    (n) =>
      Math.abs(n.at[0] - 1) < 1e-6 &&
      Math.abs(n.at[1]) < 1e-6 &&
      Math.abs(n.at[2]) < 1e-6,
  )!;
  const middle: [number, number] = [
    (a.on[0] + b.on[0]) / 2,
    (a.on[1] + b.on[1]) / 2,
  ];

  const spot = await page.evaluate(
    (at) => (window as any).__gecko.pickGeometry(at[0], at[1]),
    middle,
  );
  expect(spot).toEqual({ dim: 1, tag: 201 });
  expect(errors).toEqual([]);
});

test("four corners go onto one curve in one gesture", async ({ page }) => {
  // The fast way onto a model: catch the handles in a rubber band, pick
  // the entity they belong to, say so once.
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.evaluate(() => (window as any).__gecko.snapTo("+x"));
  await page.evaluate(() => (window as any).__gecko.settled());
  await expect(page.locator("#attach")).toBeDisabled();

  // The four at z = 0 — two of them behind the other two from here, which
  // is the point: a band takes everything inside it, not what is in front.
  const nodes = await handles(page);
  const bottom = nodes.filter((n) => Math.abs(n.at[2]) < 1e-6);
  expect(bottom).toHaveLength(4);
  const box = (await page.locator("#view").boundingBox())!;
  const xs = bottom.map((n) => n.on[0]);
  const ys = bottom.map((n) => n.on[1]);
  const pad = 12;
  const from: [number, number] = [
    box.x + Math.min(...xs) - pad,
    box.y + Math.min(...ys) - pad,
  ];
  const to: [number, number] = [
    box.x + Math.max(...xs) + pad,
    box.y + Math.max(...ys) + pad,
  ];

  await page.keyboard.down("Shift");
  await page.mouse.move(...from);
  await page.mouse.down();
  await page.mouse.move(from[0] + 5, to[1]);
  await expect(page.locator("#lasso")).toBeVisible();
  await page.mouse.move(...to, { steps: 5 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  await expect(page.locator("#lasso")).toBeHidden();

  expect(
    (
      (await page.evaluate(() =>
        (window as any).__gecko.selectedNodes(),
      )) as number[]
    ).sort(),
  ).toEqual(bottom.map((n) => n.name).sort());
  await expect(page.locator("#status")).toContainText(
    "4 nodes — 0 on the geometry",
  );
  // Still not enough: there is nothing to put them on yet.
  await expect(page.locator("#attach")).toBeDisabled();

  // Curve 201 runs along y = z = 0. Picked through the viewer rather than
  // by aiming at it, which the test above does.
  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([{ dim: 1, tag: 201 }]),
  );
  await page.evaluate(() => (window as any).__gecko.showVerbs());
  await expect(page.locator("#attach")).toBeEnabled();

  await page.keyboard.press("a");
  await expect(page.locator("#status")).toContainText("curve 201");
  await expect(page.locator("[data-test=pinned]")).toHaveText("4");

  // And each of them was moved onto it, which is the half of this that
  // changes the model.
  const after = await handles(page);
  for (const node of bottom) {
    const now = after.find((n) => n.name === node.name)!;
    expect(Math.abs(now.at[1])).toBeLessThan(1e-6);
    expect(Math.abs(now.at[2])).toBeLessThan(1e-6);
  }
  // Both selections let go: the next four corners are the next gesture.
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedNodes()),
  ).toEqual([]);
  await expect(page.locator("#attach")).toBeDisabled();

  // One step back puts all four where they were.
  await page.keyboard.press("ControlOrMeta+z");
  await expect(page.locator("[data-test=pinned]")).toHaveText("0");
  expect(errors).toEqual([]);
});

test("an edge goes on two curves, and a face on two surfaces", async ({
  page,
}) => {
  // A cell spanning several entities is told so: the entities are picked
  // together and taken as one.
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.evaluate(() => (window as any).__gecko.apply({ op: "classify" }));
  await page.evaluate(() => (window as any).__gecko.refresh());

  // The bottom-front edge, from (0,0,0) to (1,0,0): on curve 201 now.
  const edge = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return (g.edgeNames() as number[]).find((n) => {
      const m = g.edgeMidpoint(n) as number[];
      return (
        Math.abs(m[0] - 0.5) < 1e-6 &&
        Math.abs(m[1]) < 1e-6 &&
        Math.abs(m[2]) < 1e-6
      );
    });
  })) as number;
  expect(edge).toBeDefined();
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  // An edge and nothing to put it on is not enough.
  await expect(page.locator("#attach")).toBeDisabled();

  // Two curves and a surface do not go together.
  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([
      { dim: 1, tag: 201 },
      { dim: 2, tag: 1 },
    ]),
  );
  await page.evaluate(() => (window as any).__gecko.showVerbs());
  await expect(page.locator("#attach")).toBeDisabled();

  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([
      { dim: 1, tag: 201 },
      { dim: 1, tag: 202 },
    ]),
  );
  await page.evaluate(() => (window as any).__gecko.showVerbs());
  await expect(page.locator("#attach")).toBeEnabled();
  await page.keyboard.press("a");
  await expect(page.locator("#status")).toContainText(
    `edge ${edge} put on side 1 (curves 201, 202)`,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedEdge()),
  ).toBeNull();

  // The bottom face, across surfaces 1 and 3.
  const bottom = (await page.evaluate(() => {
    // Drawn positions are shifted by the scene's origin, so the face is
    // the lowest one rather than the one at a given height.
    const s = (window as any).__gecko.surface("faces");
    let best = -1;
    let at = Infinity;
    for (let q = 0; q < s.quads; q++) {
      let z = 0;
      for (let k = 0; k < 4; k++) z += s.positions[(q * 4 + k) * 3 + 2] / 4;
      if (z < at) {
        at = z;
        best = q;
      }
    }
    return { face: s.faces[best], block: s.blocks[best] };
  })) as { face: number; block: number };
  await page.evaluate((p) => (window as any).__gecko.choose(p), bottom);
  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([
      { dim: 2, tag: 1 },
      { dim: 2, tag: 3 },
    ]),
  );
  await page.evaluate(() => (window as any).__gecko.showVerbs());
  await page.keyboard.press("a");
  await expect(page.locator("#status")).toContainText(
    `face ${bottom.face} put on patch 2 (surfaces 1, 3)`,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  // The top face may not: its edges are on curves neither surface holds.
  const top = (await page.evaluate(() => {
    // Drawn positions are shifted by the scene's origin, so the face is
    // the highest one rather than the one at a given height.
    const s = (window as any).__gecko.surface("faces");
    let best = -1;
    let at = -Infinity;
    for (let q = 0; q < s.quads; q++) {
      let z = 0;
      for (let k = 0; k < 4; k++) z += s.positions[(q * 4 + k) * 3 + 2] / 4;
      if (z > at) {
        at = z;
        best = q;
      }
    }
    return { face: s.faces[best], block: s.blocks[best] };
  })) as { face: number; block: number };
  await page.evaluate((p) => (window as any).__gecko.choose(p), top);
  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([
      { dim: 2, tag: 1 },
      { dim: 2, tag: 3 },
    ]),
  );
  await page.evaluate(() => (window as any).__gecko.showVerbs());
  await page.keyboard.press("a");
  await expect(page.locator("#status")).toContainText("cannot stay on");
  expect(errors).toEqual([]);
});

test("freezing and unclassifying take the whole selection", async ({
  page,
}) => {
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.keyboard.press("k");
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");

  const names = (await page.evaluate(() =>
    (window as any).__gecko.names(),
  )) as number[];
  await page.evaluate(
    (n) => (window as any).__gecko.selectNodes(n),
    names.slice(0, 3),
  );
  await page.evaluate(() => (window as any).__gecko.showVerbs());

  await page.keyboard.press("l");
  await expect(page.locator("#status")).toContainText("frozen");
  expect(
    await page.evaluate(
      () => (window as any).__gecko.stats().classified.frozen,
    ),
  ).toBe(3);

  // Thawed only when every one of them is frozen, so a mixed selection has
  // an obvious answer.
  await page.keyboard.press("l");
  expect(
    await page.evaluate(
      () => (window as any).__gecko.stats().classified.frozen,
    ),
  ).toBe(0);

  await page.keyboard.press("u");
  await expect(page.locator("[data-test=pinned]")).toHaveText("5");
  expect(errors).toEqual([]);
});

test("⌥ slides a selected face instead of growing a block from it", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // Pick the face the pointer is over, and remember where its corners are.
  const box = (await page.locator("#view").boundingBox())!;
  const middle: [number, number] = [box.width / 2, box.height / 2];
  await page.mouse.click(box.x + middle[0], box.y + middle[1]);
  const picked = (await page.evaluate(() =>
    (window as any).__gecko.selection(),
  )) as { face: number; block: number }[];
  expect(picked).toHaveLength(1);

  const before = (await page.evaluate(() =>
    (window as any).__gecko.counts(),
  )) as {
    nodes: number;
  };
  // By the names the kernel hands out, not by the first eight numbers: a
  // name is the kernel's to choose, and this test is about four corners of
  // eight moving.
  const named = (await page.evaluate(() =>
    (window as any).__gecko.names(),
  )) as number[];
  expect(named).toHaveLength(8);
  const places = async () =>
    (await page.evaluate(
      (names) =>
        names.map((n: number) => (window as any).__gecko.nodePosition(n)),
      named,
    )) as number[][];
  const was = await places();

  // ⌥ and drag: the face moves, and no block is built.
  await page.keyboard.down("Alt");
  await page.mouse.move(box.x + middle[0], box.y + middle[1]);
  await page.mouse.down();
  await page.mouse.move(box.x + middle[0] + 60, box.y + middle[1] - 60, {
    steps: 6,
  });
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await page.evaluate(() => (window as any).__gecko.settled());

  const after = (await page.evaluate(() =>
    (window as any).__gecko.counts(),
  )) as {
    nodes: number;
  };
  expect(after.nodes).toBe(before.nodes);
  expect(
    await page.evaluate(() => (window as any).__gecko.stats().blocks),
  ).toBe(1);
  await expect(page.locator("#status")).toContainText("moved");

  // Four corners moved and four stayed: a face slid, and the block it is
  // on was stretched rather than carried.
  const now = await places();
  const stirred = was.filter(
    (p, i) =>
      p &&
      now[i] &&
      p.some((x: number, k: number) => Math.abs(x - now[i][k]) > 1e-6),
  );
  expect(stirred).toHaveLength(4);

  expect(errors).toEqual([]);
});

test("⇧Q lays a quadrangle on four corners clicked on the geometry", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // Nothing laid yet, and the mode says what it wants.
  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  await expect(page.locator("#status")).toContainText("four corners");

  // Four clicks spread over the square. Each one says how many are left,
  // which is how a reader knows the mode is still running.
  const box = (await page.locator("#view").boundingBox())!;
  const at: [number, number][] = [
    [box.width * 0.4, box.height * 0.4],
    [box.width * 0.6, box.height * 0.4],
    [box.width * 0.6, box.height * 0.6],
    [box.width * 0.4, box.height * 0.6],
  ];
  for (let k = 0; k < 3; k++) {
    await page.mouse.click(box.x + at[k][0], box.y + at[k][1]);
    await expect(page.locator("#status")).toContainText(`${k + 1} of 4`);
  }
  await page.mouse.click(box.x + at[3][0], box.y + at[3][1]);
  await page.evaluate(() => (window as any).__gecko.settled());
  await expect(page.locator("#status")).toContainText("laid on");

  // One quadrangle and no block: four corners, all four on the geometry.
  // The height comes later, once, for the whole patch.
  const stats = (await page.evaluate(() =>
    (window as any).__gecko.stats(),
  )) as {
    blocks: number;
    patches: number;
    nodes: number;
    classified: { nodes: number; faces: number };
  };
  expect(stats.blocks).toBe(0);
  expect(stats.patches).toBe(1);
  expect(stats.nodes).toBe(4);
  expect(stats.classified.nodes).toBe(4);
  // Nobody picked a surface, and the quadrangle is on one anyway: all four
  // of its corners are on this one, so the settling that runs after every
  // command carries that up through its sides to the face. Saying which
  // surface is for when the settling cannot tell — corners on curves of
  // two different surfaces, or on nothing.
  expect(stats.classified.faces).toBe(1);

  expect(errors).toEqual([]);
});

test("a surface picked first is what the quadrangle is laid on", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() =>
    (window as any).__gecko.selectGeom([{ dim: 2, tag: 1 }]),
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  await expect(page.locator("#status")).toContainText("surface 1");

  const box = (await page.locator("#view").boundingBox())!;
  for (const [x, y] of [
    [0.4, 0.4],
    [0.6, 0.4],
    [0.6, 0.6],
    [0.4, 0.6],
  ]) {
    await page.mouse.click(box.x + box.width * x, box.y + box.height * y);
  }
  await page.evaluate(() => (window as any).__gecko.settled());

  await expect(page.locator("#status")).toContainText("laid on surface 1");
  expect(
    await page.evaluate(() => (window as any).__gecko.stats().classified.faces),
  ).toBe(1);

  expect(errors).toEqual([]);
});

test("a quadrangle laid on a neighbor's corners is welded to it", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  const lay = async (corners: [number, number][]) => {
    await page.evaluate(() => (window as any).__gecko.run("lay-face"));
    for (const [x, y] of corners) await page.mouse.click(box.x + x, box.y + y);
    await page.evaluate(() => (window as any).__gecko.settled());
  };
  const view = (x: number, y: number): [number, number] => [
    box.width * x,
    box.height * y,
  ];

  // A strip across the middle of the square, kept well inside it so that
  // every click has geometry under it.
  await lay([
    view(0.45, 0.45),
    view(0.52, 0.45),
    view(0.52, 0.55),
    view(0.45, 0.55),
  ]);
  expect(await page.evaluate(() => (window as any).__gecko.stats().nodes)).toBe(
    4,
  );

  // Where its four corners actually ended up, rather than where they were
  // aimed at.
  const nodes = await handles(page);
  expect(nodes).toHaveLength(4);
  const byX = [...nodes].sort((a, b) => a.on[0] - b.on[0]);
  const [farA, farB] = byX.slice(2).sort((a, b) => a.on[1] - b.on[1]);
  const middle = (which: typeof byX) => [
    (which[0].on[0] + which[1].on[0]) / 2,
    (which[0].on[1] + which[1].on[1]) / 2,
  ];
  const [nx, ny] = middle(byX.slice(0, 2));
  const [fx, fy] = middle(byX.slice(2));
  const step: [number, number] = [fx - nx, fy - ny];
  const beyond = (p: { on: [number, number] }): [number, number] => [
    p.on[0] + step[0],
    p.on[1] + step[1],
  ];

  // The next one along, its first two corners clicked on this one's
  // handles — and clicked round **the same way**, which is the way that
  // cannot be welded as it stands: two adjacent faces of a sheet run
  // opposite ways along the side they share. A sheet has no inside, so the
  // quadrangle is turned to agree with its neighbour rather than refused.
  await lay([farA.on, farB.on, beyond(farB), beyond(farA)]);

  await expect(page.locator("#status")).toContainText("welded along 1 side");
  const stats = (await page.evaluate(() =>
    (window as any).__gecko.stats(),
  )) as { patches: number; nodes: number; faces: number };
  // Eight corners less the two that met, in one patch: the strip a single
  // drag raises into blocks of one height.
  expect(stats.faces).toBe(2);
  expect(stats.nodes).toBe(6);
  expect(stats.patches).toBe(1);

  expect(errors).toEqual([]);
});

test("laying reaches the handle on the geometry, not the one behind it", async ({
  page,
}) => {
  // The trap that made a hand-laid series of thin blocks unusable: a block
  // resting on a surface has a node inside the material right behind each
  // of the ones on it, a fraction of a pixel away on screen, and a click
  // meaning the corner reached the hidden one instead — putting the next
  // quadrangle a thickness off the model, classified on nothing.
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  // From +z, where the slab's material is: the inner handles are in front.
  await page.evaluate(() => (window as any).__gecko.snapTo("+z"));
  await page.evaluate(() => (window as any).__gecko.settled());

  await openConsole(page);
  // Thin, which is when the trap is worst: the thinner the block, the
  // nearer the hidden handle is to the one on the surface on screen, until
  // they are the same pixel and a click cannot tell them apart.
  await lua(
    page,
    "gecko.add_slab({ {0,0,0}, {1,0,0}, {1,1,0}, {0,1,0} }, { thickness = 0.002, on = " +
      "{ { dim = 2, tag = 1 }, { dim = 2, tag = 1 }, " +
      "{ dim = 2, tag = 1 }, { dim = 2, tag = 1 } } })",
  );
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await page.locator("#console-open").click();
  await page.evaluate(() => (window as any).__gecko.settled());

  // Aim between an inner handle and the one on the surface behind it. They
  // are all but the same pixel, which is the whole trouble, so both are
  // under the pointer — and `pickNode` answering with the inner one is what
  // says so.
  const nodes = await handles(page);
  const inner = nodes.find((n) => n.at[2] > 1e-6);
  expect(inner, "a handle inside the material").toBeTruthy();
  const outer = nodes.find(
    (n) =>
      Math.abs(n.at[2]) < 1e-9 &&
      Math.hypot(n.at[0] - inner!.at[0], n.at[1] - inner!.at[1]) < 1e-9,
  );
  expect(outer, "the one on the surface under it").toBeTruthy();
  const x = (inner!.on[0] + outer!.on[0]) / 2;
  const y = (inner!.on[1] + outer!.on[1]) / 2;

  const nearest = await page.evaluate(
    ([x, y]) => (window as any).__gecko.pickNode(x, y),
    [x, y],
  );
  const laying = await page.evaluate(
    ([x, y]) => (window as any).__gecko.pickHandle(x, y),
    [x, y],
  );
  expect(nearest).toBe(inner!.name);
  // What a click while laying reaches is the one that is on the geometry.
  expect(laying).toBe(outer!.name);
  const state = await page.evaluate(
    (name) => (window as any).__gecko.nodeState(name),
    laying,
  );
  expect(state?.on).toEqual({ dim: 2, tag: 1 });
  const at = await page.evaluate(
    (name) => (window as any).__gecko.nodePosition(name),
    laying,
  );
  expect(Math.abs(at[2])).toBeLessThan(1e-9);

  expect(errors).toEqual([]);
});

test("what is on screen while a quadrangle is laid: marks, sides to the pointer, and the geometry under the pointer", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // Aimed at the blocks to start with, which is the mode the laying has to
  // hand back.
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe(
    "blocks",
  );
  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  // For the duration the pointer is on the geometry and nothing of the
  // block structure answers it.
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe(
    "geometry",
  );

  let box = (await page.locator("#view").boundingBox())!;
  const at = (x: number, y: number): [number, number] => [
    box.x + box.width * x,
    box.y + box.height * y,
  ];

  // Nothing provisional before a corner is down, whatever the pointer does.
  await page.mouse.move(...at(0.45, 0.45));
  expect(await page.evaluate(() => (window as any).__gecko.aimedSides())).toBe(
    0,
  );

  // One corner, and a mark no bigger than two handles — in pixels, which is
  // the only size a reader cares about.
  await page.mouse.click(...at(0.45, 0.45));
  const across = (await page.evaluate(() =>
    (window as any).__gecko.layingAcross(),
  )) as number;
  const handle = (await page.evaluate(
    () => (window as any).__gecko.prefs().nodes.size,
  )) as number;
  expect(across).toBeGreaterThan(0);
  expect(across).toBeCloseTo(2 * handle, 0);

  // And it stays that size when the camera moves, which is what the
  // per-pane sizing pass is for: a mark fixed in world units grows into a
  // block as you zoom in.
  // And it is still that size after the window changes shape, which is what
  // the per-pane sizing pass is for: a mark fixed in world units is a
  // different number of pixels the moment the viewport is not the height it
  // was made for.
  await page.setViewportSize({ width: 1000, height: 420 });
  await page.evaluate(() => (window as any).__gecko.settled());
  box = (await page.locator("#view").boundingBox())!;
  const resized = (await page.evaluate(() =>
    (window as any).__gecko.layingAcross(),
  )) as number;
  expect(resized).toBeCloseTo(2 * handle, 0);

  // And from there a side follows the pointer.
  await page.mouse.move(...at(0.52, 0.45));
  expect(await page.evaluate(() => (window as any).__gecko.aimedSides())).toBe(
    1,
  );

  // At the third, two: on to the pointer and back to the first, so the
  // quadrangle is whole before the click that closes it.
  await page.mouse.click(...at(0.52, 0.45));
  await page.mouse.click(...at(0.52, 0.55));
  await page.mouse.move(...at(0.45, 0.55));
  expect(await page.evaluate(() => (window as any).__gecko.aimedSides())).toBe(
    2,
  );

  // The fourth click lays it, and nothing provisional is left over.
  await page.mouse.click(...at(0.45, 0.55));
  await page.evaluate(() => (window as any).__gecko.settled());
  await expect(page.locator("#status")).toContainText("laid on");
  expect(await page.evaluate(() => (window as any).__gecko.aimedSides())).toBe(
    0,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.layingAcross()),
  ).toBeNull();
  // And the pointer is back where it was.
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe(
    "blocks",
  );

  expect(errors).toEqual([]);
});

test("a quadrangle being laid is given up on with Esc, and walked back with ⌫", async ({
  page,
}) => {
  const errors = await boot(page);
  await openSquare(page);
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  await page.mouse.click(box.x + box.width * 0.45, box.y + box.height * 0.45);
  await page.mouse.click(box.x + box.width * 0.55, box.y + box.height * 0.45);
  await expect(page.locator("#status")).toContainText("2 of 4");

  // Back one.
  await page.keyboard.press("Backspace");
  await expect(page.locator("#status")).toContainText("1 of 4");

  // And give up: nothing was put in the model, so nothing comes out of it.
  await page.keyboard.press("Escape");
  await expect(page.locator("#status")).toContainText("given up");
  expect(await page.evaluate(() => (window as any).__gecko.stats().nodes)).toBe(
    0,
  );

  expect(errors).toEqual([]);
});

test("the band is opened with W as well, and works in an axis view", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // Into the Y pane, bottom left, and read the handles as **it** sees them.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + box.width * 0.25, box.y + box.height * 0.75);
  expect(await page.evaluate(() => (window as any).__gecko.pane())).toBe("y");

  const seen = await handles(page);
  // Seen along an axis the cube's corners fall in pairs, one behind the
  // other. A band round one of them has to catch both — which is only true
  // if the handles are projected through this pane's own camera.
  const first = seen[0];
  const pair = seen.filter(
    (n) => Math.hypot(n.on[0] - first.on[0], n.on[1] - first.on[1]) < 3,
  );
  expect(pair).toHaveLength(2);

  const pad = 10;
  await page.keyboard.down("w");
  await page.mouse.move(box.x + first.on[0] - pad, box.y + first.on[1] - pad);
  await page.mouse.down();
  await page.mouse.move(box.x + first.on[0] + pad, box.y + first.on[1] + pad, {
    steps: 4,
  });
  await expect(page.locator("#lasso")).toBeVisible();
  await page.mouse.up();
  await page.keyboard.up("w");

  expect(
    (
      (await page.evaluate(() =>
        (window as any).__gecko.selectedNodes(),
      )) as number[]
    ).sort(),
  ).toEqual(pair.map((n) => n.name).sort());

  // A band that wanders out of the pane it was begun in still means what
  // it meant there: the rectangle is anchored where it started, and so is
  // the camera it is read through.
  await page.keyboard.press("Escape");
  const corner: [number, number] = [first.on[0] - pad, first.on[1] + pad];
  const away: [number, number] = [first.on[0] + 3 * pad, box.height * 0.2];
  await page.keyboard.down("w");
  await page.mouse.move(box.x + corner[0], box.y + corner[1]);
  await page.mouse.down();
  // Up and out of the Y pane, into the 3-D one above it.
  await page.mouse.move(box.x + away[0], box.y + away[1], { steps: 8 });
  await page.mouse.up();
  await page.keyboard.up("w");
  expect(await page.evaluate(() => (window as any).__gecko.pane())).toBe("3d");

  // What the **Y** pane has inside that rectangle, which is what the band
  // was drawn over. Read with the pointer back in it.
  await page.mouse.move(box.x + box.width * 0.25, box.y + box.height * 0.75);
  const inside = (await handles(page))
    .filter(
      (n) =>
        n.on[0] >= Math.min(corner[0], away[0]) &&
        n.on[0] <= Math.max(corner[0], away[0]) &&
        n.on[1] >= Math.min(corner[1], away[1]) &&
        n.on[1] <= Math.max(corner[1], away[1]),
    )
    .map((n) => n.name);
  expect(inside.length).toBeGreaterThan(0);
  expect(
    (
      (await page.evaluate(() =>
        (window as any).__gecko.selectedNodes(),
      )) as number[]
    ).sort(),
  ).toEqual(inside.sort());
  expect(errors).toEqual([]);
});

test("dragging one picked handle takes the others with it", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const before = await handles(page);
  const picked = before.slice(0, 3);
  await page.evaluate(
    (names) => (window as any).__gecko.selectNodes(names),
    picked.map((n) => n.name),
  );

  // A real drag on one of them, by a hundred pixels.
  const box = (await page.locator("#view").boundingBox())!;
  const grab = picked[0];
  await page.mouse.move(box.x + grab.on[0], box.y + grab.on[1]);
  await page.mouse.down();
  await page.mouse.move(box.x + grab.on[0] + 100, box.y + grab.on[1], {
    steps: 8,
  });
  await page.mouse.up();
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("moved"),
  );

  const after = await handles(page);
  const moved = (name: number) => {
    const was = before.find((n) => n.name === name)!.at;
    const now = after.find((n) => n.name === name)!.at;
    return [0, 1, 2].map((k) => now[k] - was[k]);
  };
  const by = moved(grab.name);
  expect(Math.hypot(...by)).toBeGreaterThan(0.1);
  // The other two picked handles went exactly as far, and the five that
  // were not picked did not move at all.
  for (const node of picked.slice(1)) {
    for (const k of [0, 1, 2])
      expect(moved(node.name)[k]).toBeCloseTo(by[k], 5);
  }
  for (const node of before.filter(
    (n) => !picked.some((p) => p.name === n.name),
  )) {
    expect(Math.hypot(...moved(node.name))).toBeLessThan(1e-9);
  }

  // One gesture, one command, one step back.
  await expect(page.locator("#status")).toContainText("3 nodes moved");
  await page.keyboard.press("ControlOrMeta+z");
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("undone"),
  );
  const back = await handles(page);
  for (const node of before) {
    const now = back.find((n) => n.name === node.name)!.at;
    for (const k of [0, 1, 2]) expect(now[k]).toBeCloseTo(node.at[k], 6);
  }
  expect(errors).toEqual([]);
});

test("dragging a handle that is not picked moves it alone", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const before = await handles(page);
  await page.evaluate(
    (names) => (window as any).__gecko.selectNodes(names),
    before.slice(0, 3).map((n) => n.name),
  );

  const box = (await page.locator("#view").boundingBox())!;
  const grab = before[5];
  await page.mouse.move(box.x + grab.on[0], box.y + grab.on[1]);
  await page.mouse.down();
  await page.mouse.move(box.x + grab.on[0] + 80, box.y + grab.on[1], {
    steps: 8,
  });
  await page.mouse.up();
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("moved"),
  );

  const after = await handles(page);
  for (const node of before) {
    const now = after.find((n) => n.name === node.name)!.at;
    const went = Math.hypot(...[0, 1, 2].map((k) => now[k] - node.at[k]));
    if (node.name === grab.name) expect(went).toBeGreaterThan(0.1);
    else expect(went).toBeLessThan(1e-9);
  }
  expect(errors).toEqual([]);
});

test("a blocking is discretised and the skin of it is drawn", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(
    0,
  );

  // Three segments a side, set in the settings panel before asking.
  await openParams(page);
  await page.locator("[data-test=segments]").fill("3");
  await page.locator("[data-test=segments]").dispatchEvent("input");
  await expect(page.locator("[data-test=segments-value]")).toHaveText("3");

  await page.locator("#mesh").click();
  await expect(page.locator("#status")).toContainText("64 nodes, 27 hexahedra");
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("27");
  // Six faces of nine quadrangles: the skin, and nothing inside it.
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(
    54,
  );

  // A mesh of a blocking you have since edited is a lie, and a lie that
  // looks right — so it goes when the blocking moves.
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(
    0,
  );
  expect(errors).toEqual([]);
});

test("a script is opened like a geometry, and replayed from nothing", async ({
  page,
}) => {
  // The same button: "open" is the same question whether the file is a
  // model to work on or a session to play back. And a script describes a
  // session from an empty one, so what is there goes first.
  const errors = await boot(page);

  // Something to throw away, so the reset is doing work.
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");

  await page.locator("#msh").setInputFiles({
    name: "two.lua",
    mimeType: "text/plain",
    buffer: Buffer.from(
      "gecko.create_cube{ origin = {0, 0, 0} }\n" +
        "gecko.create_cube{ origin = {2, 0, 0} }\n" +
        "print('replayed')\n",
    ),
  });

  // Two blocks, not three: the cube that was there went with the reset,
  // and so did the geometry.
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveCount(0);

  // The console is where a script talks, so it is open and it said so.
  await expect(page.locator("#console")).toBeVisible();
  await expect(page.locator("[data-test=console-out]")).toContainText(
    "emptied first",
  );
  await expect(page.locator("[data-test=console-out]")).toContainText(
    "replayed",
  );
  await expect(page.locator("#status")).toContainText("2 blocks");

  // One step of undo takes the whole replay back — the reset included, so
  // what was there before it comes back.
  await page.locator("#undo").click();
  await page.locator("#undo").click();
  await page.locator("#undo").click();
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  expect(errors).toEqual([]);
});

test("⇧Esc drops the last thing picked, Esc the lot", async ({ page }) => {
  // One wrong click in a selection that took a while to build should cost
  // that click, not the selection.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  // Three nodes, picked through the same door a click goes through.
  const names = (await page.evaluate(() =>
    (window as any).__gecko.names(),
  )) as number[];
  for (const n of names.slice(0, 3)) {
    await page.evaluate(
      (who) => (window as any).__gecko.pickNodeByName(who),
      n,
    );
  }
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedNodes()),
  ).toEqual(names.slice(0, 3));

  // Then a face, which is a different selection — so "the last one" is a
  // question nothing but the order of the clicks can answer.
  const face = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    g.choose({ face: s.faces[0], block: s.blocks[0] });
    return s.faces[0];
  })) as number;
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toHaveLength(1);

  // ⇧Esc takes back the face, and leaves the three nodes alone.
  await page.keyboard.press("Shift+Escape");
  expect(
    await page.evaluate(() => (window as any).__gecko.selection()),
  ).toEqual([]);
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedNodes()),
  ).toEqual(names.slice(0, 3));
  void face;

  // Again, and it is the last node that goes — one at a time, newest first.
  await page.keyboard.press("Shift+Escape");
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedNodes()),
  ).toEqual(names.slice(0, 2));

  // Esc still lets go of everything.
  await page.keyboard.press("Escape");
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedNodes()),
  ).toEqual([]);

  // And with nothing picked it says so rather than doing something.
  await page.keyboard.press("Shift+Escape");
  await expect(page.locator("#status")).toContainText("nothing picked");
  expect(errors).toEqual([]);
});

test("the mesh button takes the mesh away again", async ({ page }) => {
  // It used to be a one-way door: once meshed, the only thing that got rid
  // of the mesh was editing the blocking, which is not what you press when
  // all you want is to see the blocks again.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await expect(page.locator("#mesh")).toHaveAttribute("aria-pressed", "false");

  await page.locator("#mesh").click();
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("1000");
  await expect(page.locator("#mesh")).toHaveAttribute("aria-pressed", "true");
  await page.waitForFunction(() => (window as any).__gecko.skinCount() > 0);

  // Again, and it is gone — out of the kernel, not merely hidden, which is
  // what the box in the display panel is for.
  await page.locator("#mesh").click();
  await expect(page.locator("#status")).toContainText("mesh cleared");
  await expect(page.locator("#mesh")).toHaveAttribute("aria-pressed", "false");
  await page.waitForFunction(() => (window as any).__gecko.skinCount() === 0);
  expect(
    await page.evaluate(() => (window as any).__gecko.stats().mesh.hexes),
  ).toBeUndefined();

  // And once more, to say the door swings both ways rather than once.
  await page.locator("#mesh").click();
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("1000");
  expect(errors).toEqual([]);
});

test("the lit sheet is made finer and coarser", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), edge);
  await expect(page.locator("#status")).toContainText("10 segments");

  await page.keyboard.press("=");
  await expect(page.locator("#status")).toContainText("11 segments");
  await page.keyboard.press("-");
  await page.keyboard.press("-");
  await expect(page.locator("#status")).toContainText("9 segments");

  // And the mesh is built to it: nine one way, the default ten the others.
  await page.keyboard.press("e");
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText(
    String(9 * 10 * 10),
  );
  expect(errors).toEqual([]);
});

test("the mesh is written out as VTK", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await openParams(page);
  await page.locator("[data-test=segments]").fill("2");
  await page.locator("[data-test=segments]").dispatchEvent("input");

  // Out of the slider first: a key pressed into a field is typing, which
  // is the whole point of that rule.
  await page.evaluate(() => (document.activeElement as HTMLElement)?.blur());

  // Nothing has been meshed yet: the key meshes first, so the file is of
  // the blocking as it now stands.
  const download = page.waitForEvent("download");
  await page.keyboard.press("Shift+E");
  const saved = await download;
  expect(saved.suggestedFilename()).toMatch(
    /^gecko-\d{4}-\d{2}-\d{2}-\d{6}\.vtk$/,
  );

  const file = await readFile(await saved.path(), "utf8");
  expect(file.split("\n")[0]).toBe("# vtk DataFile Version 2.0");
  expect(file).toContain("DATASET UNSTRUCTURED_GRID");
  expect(file).toContain("POINTS 27 double");
  expect(file).toContain("CELLS 8 72");
  expect(file).toContain("SCALARS block int 1");
  expect(errors).toEqual([]);
});

test("blocks are put in a group, which colors and filters what is drawn", async ({
  page,
}) => {
  // Groups are of blocks; a face, an edge or a node is in a group when a
  // block it bounds is. Nothing here touches the geometry: a group is a
  // part of the model somebody named.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );
  await openDrawer(page);
  await openSection(page, "groups");
  await expect(page.locator("#groups")).toContainText("No groups yet");

  // One block picked — a click names the face and the block it was on —
  // and a group made from it.
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    g.choose({ face: s.faces[0], block: s.blocks[0] });
  });
  await page.locator("[data-test=group-new]").fill("fluid");
  await page.locator("[data-test=group-make]").click();
  await expect(page.locator("[data-test=group-name]")).toHaveValue("fluid");
  await expect(page.locator("[data-test=group-blocks]")).toHaveText("1");
  await expect(page.locator("#status")).toContainText("fluid");

  // Everything is still drawn: a group is made shown.
  // What is **drawn**, not what exists: hiding a group leaves the handles
  // in the scene and takes them off the screen.
  const drawn = () =>
    page.evaluate(() => ({
      nodes: (window as any).__gecko.nodesDrawn(),
      edges: (window as any).__gecko.counts().edges,
      quads: (window as any).__gecko.surface("faces")?.faces.length ?? 0,
    }));
  const whole = await drawn();
  expect(whole.nodes).toBe(16);

  // Hide it and the half of the model that is in it goes — the cells of
  // the other block stay, which is what says the filter is per cell and
  // not per scene.
  await page.locator("[data-test=group-shown]").uncheck();
  await expect
    .poll(async () => (await drawn()).nodes)
    .toBeLessThan(whole.nodes);
  const left = await drawn();
  expect(left.nodes).toBe(8);
  expect(left.edges).toBe(12);
  expect(left.quads).toBe(6);

  // Hide the blocks in no group as well and nothing is left.
  await page.locator("[data-test=ungrouped-visible]").uncheck();
  await expect.poll(async () => (await drawn()).nodes).toBe(0);

  // Both back, and the coloring by group gives the group's own color to
  // its cells and the plain one to the rest.
  await page.locator("[data-test=group-shown]").check();
  await page.locator("[data-test=ungrouped-visible]").check();
  await expect.poll(async () => (await drawn()).nodes).toBe(16);
  await openGroup(page, "nodes");
  await page.locator('[data-test="nodes-mode"]').selectOption("group");
  const tints = await page.evaluate(() => {
    const g = (window as any).__gecko;
    return g.names().map((n: number) => g.nodeTint(n));
  });
  expect(new Set(tints as string[]).size).toBe(2);

  // Whether it reaches the mesh is said on its own row, in words, and it
  // goes both ways: the command that names a group is create-or-find, and
  // finding one used to ignore what it was told about the mesh.
  await expect(page.locator("[data-test=group-written]")).toBeChecked();
  await page.locator("[data-test=group-written]").uncheck();
  await expect(page.locator("[data-test=group-written]")).not.toBeChecked();
  await expect(page.locator("#status")).toContainText(
    "not written into the mesh",
  );
  await page.locator("[data-test=group-written]").check();
  await expect(page.locator("[data-test=group-written]")).toBeChecked();

  // Renamed in place, which is the one thing anybody does to a group
  // twice.
  await page.locator("[data-test=group-name]").fill("water");
  await page.locator("[data-test=group-name]").press("Enter");
  await expect(page.locator("#status")).toContainText("water");

  // And dropped: the group goes, the blocks do not.
  await page.locator("[data-test=group-drop]").click();
  await expect(page.locator("#groups")).toContainText("No groups yet");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  expect(errors).toEqual([]);
});

test("hiding a group leaves the blocks beside it whole", async ({ page }) => {
  // The bug this answers: an edge between a grouped block and an
  // ungrouped one carries the group's bit, so hiding the group took the
  // edge away from the block still on screen — and left it looking like a
  // box with a side missing. A cell is drawn when **any** block it bounds
  // is drawn.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );

  // Glued, so the two blocks share a face — four nodes and four edges
  // belong to both.
  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const [first, second] = facingPair();
    g.apply({ op: "glue_faces", first, second });
    await g.refresh();
  });
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

  // One of the two in a group.
  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const blocks = [
      ...new Set(Array.from(g.surface("faces").blocks as number[])),
    ];
    g.apply({ op: "group", name: "fluid", blocks: [blocks[0]] });
    await g.refresh();
  });
  await openDrawer(page);
  await openSection(page, "groups");
  await page.locator("[data-test=group-shown]").uncheck();

  // What is left is one whole cube: eight handles, twelve edges, six
  // faces — the shared ones among them.
  await expect
    .poll(async () => page.evaluate(() => (window as any).__gecko.nodesDrawn()))
    .toBe(8);
  expect(
    await page.evaluate(() => (window as any).__gecko.counts().edges),
  ).toBe(12);
  expect(
    await page.evaluate(
      () => (window as any).__gecko.surface("faces").faces.length,
    ),
  ).toBe(6);

  // And a sheet lit through both of them stops where the picture does. Six
  // edges across the two blocks, of which four bound the one still on
  // screen — the two in the seam among them, since they bound it too.
  await page.evaluate(EDGE_NEAR);
  const seam = (await page.evaluate(() => edgeNear([1, 0.5, 0]))) as number;
  await page.evaluate((e) => (window as any).__gecko.pickEdgeByName(e), seam);
  await page.waitForFunction(
    () => (window as any).__gecko.sheet().length === 6,
  );
  await page.evaluate(() => (window as any).__gecko.settled());
  expect(await page.evaluate(() => (window as any).__gecko.litDrawn())).toBe(4);
  expect(errors).toEqual([]);
});

test("the handles can be put away, and the pointer lets them be", async ({
  page,
}) => {
  // Edges, faces and blocks could each be hidden; the handles could not,
  // on the grounds that they are what you grab. A hundred blocks is a
  // thousand dots over the model, and looking at the shape of a blocking
  // is a thing you do as much as working on it.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await openDrawer(page);
  await openGroup(page, "nodes");

  const name = (await page.evaluate(
    () => (window as any).__gecko.names()[0],
  )) as number;
  const at = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    name,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;

  // There to begin with, and a click on it picks it.
  expect(
    await page.evaluate(
      (p) => (window as any).__gecko.pickNode(p[0], p[1]),
      at,
    ),
  ).toBe(name);

  await page.locator('[data-test="nodes-visible"]').uncheck();
  await expect(page.locator("#grp-nodes .digest")).toHaveText("hidden");
  expect(
    await page.evaluate(() => (window as any).__gecko.nodeShape(0)),
  ).not.toBeNull();

  // Gone from the picture, and gone from the pointer with it: a handle you
  // cannot see but can still grab moves the model with nothing on screen
  // to say what you took hold of.
  expect(
    await page.evaluate(
      (p) => (window as any).__gecko.pickNode(p[0], p[1]),
      at,
    ),
  ).toBeNull();
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedNodes()),
  ).toEqual([]);

  // Nor caught by a band drawn over them.
  await page.keyboard.down("Shift");
  await page.mouse.move(box.x + 10, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 10, box.y + box.height - 10, {
    steps: 8,
  });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  expect(
    await page.evaluate(() => (window as any).__gecko.selectedNodes()),
  ).toEqual([]);

  // And the setting is remembered, like every other thing about how it looks.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().nodes.visible),
  ).toBe(false);
  expect(errors).toEqual([]);
});

test("the mesh has a band of its own, and can be put away", async ({
  page,
}) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.keyboard.press("e");
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("1000");

  // One band, and its controls are in it — no fold inside the fold. A mesh
  // has no groups to tell apart, so there was nothing for the inner one to
  // separate.
  await openSection(page, "mesh");
  expect(await page.locator("#mesh-layer details").count()).toBe(0);
  await expect(page.locator('[data-test="mesh-mode"]')).toBeVisible();
  await expect(page.locator('[data-test="mesh-alpha"]')).toBeVisible();
  await page.locator("[data-test=mesh-visible]").uncheck();
  // Still meshed — the kernel holds it — but nothing of it is drawn.
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(
    0,
  );
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("1000");
  expect(errors).toEqual([]);
});

/** Open the console and wait for the interpreter to be there. */
async function openConsole(page: import("@playwright/test").Page) {
  if (
    !(await page.evaluate(() =>
      document.body.classList.contains("console-open"),
    ))
  ) {
    await page.locator("#console-open").click();
  }
  await expect(page.locator("#console")).toBeVisible();
  await expect(page.locator("[data-test=console-out]")).toContainText("ready", {
    timeout: 30_000,
  });
}

/** Type a chunk and run it. */
async function lua(page: import("@playwright/test").Page, source: string) {
  await page.locator("[data-test=console-in]").fill(source);
  await page.locator("[data-test=console-in]").press("Enter");
}

test("the console runs Lua, and the scene catches up", async ({ page }) => {
  // The whole point of having a language: a loop and a conditional
  // driving the kernel, from the page, with no server anywhere.
  const errors = await boot(page);
  await openConsole(page);

  await lua(
    page,
    "for i = 1, 3 do gecko.create_cube{ origin = {i * 1.5, 0, 0} } end",
  );
  await expect(page.locator("[data-test=blocks]")).toHaveText("3");
  // And the scene drew them, rather than the readout merely saying so.
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 24,
  );

  // A value comes back, and a table is opened out rather than printed as
  // an address.
  await lua(page, "return gecko.stats()");
  await expect(page.locator("[data-test=console-out]")).toContainText(
    "blocks = 3",
  );
  // Whole, and in name order: the table has nine fields, and a console
  // that printed them in hash order would hide one of them behind an
  // ellipsis as soon as a tenth was added.
  await expect(page.locator("[data-test=console-out]")).toContainText(
    /blocks = 3,.*\bedges = .*\bfaces = .*\bnodes = .*\bpatches = /s,
  );

  // `print` goes to the scrollback.
  await lua(page, 'print("nodes:", #gecko.nodes())');
  await expect(page.locator("[data-test=console-out]")).toContainText(
    "nodes:\t24",
  );
  expect(errors).toEqual([]);
});

test("what the mouse does is written in the console as Lua", async ({
  page,
}) => {
  // The session's history, kept by the kernel so that both doors — the
  // mouse and the console — write into the same one, in order.
  const errors = await boot(page);
  await openConsole(page);

  await page.locator("#cube").click();
  await expect(page.locator("[data-test=console-out]")).toContainText(
    "gecko.create_cube{ origin = {0, 0, 0}, size = 1 }",
  );

  await lua(page, "gecko.create_cube{ origin = {3, 0, 0} }");
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");

  // Both of them are in the script, in the order they happened.
  await lua(page, "return gecko.script()");
  const shown =
    (await page.locator("[data-test=console-out]").textContent()) ?? "";
  expect(shown, "the two commands are not in order in the script").toMatch(
    /size = 1 \}\s*gecko\.create_cube\{ origin = \{3, 0, 0\}/,
  );

  // An undo takes the line back off: the script is a recipe for where the
  // session now is, not a diary of what anybody tried.
  //
  // Out of the console's line first — a key pressed into a field is
  // typing, console or not, and ⌘Z in a text box undoes the text.
  await page.evaluate(() => (document.activeElement as HTMLElement)?.blur());
  await page.keyboard.press("ControlOrMeta+z");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await lua(page, 'print(select(2, gecko.script():gsub("create_cube", "")))');
  await expect(page.locator("[data-test=console-out]")).toContainText("\n1");
  expect(errors).toEqual([]);
});

test("a mistake in a script is reported, not swallowed", async ({ page }) => {
  const errors = await boot(page);
  await openConsole(page);

  await lua(page, "this is not lua");
  await expect(page.locator("#console-out .bad")).toContainText("syntax error");

  // A refusal from the kernel arrives the same way, and stops the chunk
  // where it went wrong rather than letting it carry on.
  await lua(
    page,
    "gecko.create_cube() gecko.remove_block(99) gecko.create_cube()",
  );
  await expect(page.locator("#console-out .bad").last()).toContainText("99");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  expect(errors).toEqual([]);
});

test("a file handed to the page can then be opened by a script", async ({
  page,
}) => {
  // The answer to "how do I open a file from Lua on the published page":
  // give the page the file first, and open it by name afterwards. There
  // is no path to give a browser, and the picker that could ask for one
  // is asynchronous — Lua asks in the middle of a loop and cannot wait.
  const errors = await boot(page);
  await openConsole(page);

  // Chosen with the toolbar's own button, which is what a reader does.
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");

  // And from now on a script can ask for it by name, and can find out
  // what the page is holding.
  await lua(page, 'print(table.concat(gecko.files(), " "))');
  await expect(page.locator("#console-out").last()).toContainText("square.msh");

  await lua(page, 'gecko.clear_geometry() gecko.load_geometry("square.msh")');
  expect(
    await page.evaluate(
      () => (window as any).__gecko.stats().geometry.surfaces,
    ),
  ).toBe(1);

  // Even written with a folder in front of it: there is no folder here for
  // it to be wrong about.
  await lua(
    page,
    'gecko.clear_geometry() gecko.load_geometry("data/square.msh")',
  );
  expect(
    await page.evaluate(
      () => (window as any).__gecko.stats().geometry.surfaces,
    ),
  ).toBe(1);
  expect(errors).toEqual([]);
});

test("a script asking this page for a file is told why there is none", async ({
  page,
}) => {
  // The suite serves the page as a static host does, so this is the case a
  // reader meets on the published page — and the first thing anyone tries
  // then is a longer path. The message has to say that no path will do.
  const errors = await boot(page);
  await openConsole(page);

  await lua(page, 'gecko.load_geometry("cylinder.msh")');
  const said = page.locator("#console-out .bad").last();
  await expect(said).toContainText("cylinder.msh");
  await expect(said).toContainText("A page has no paths");
  await expect(said).toContainText("drop the file on the window");
  await expect(said).toContainText("gecko.files()");
  expect(errors).toEqual([]);
});

test("the session is saved as a script", async ({ page }) => {
  const errors = await boot(page);
  await openConsole(page);
  await lua(page, "gecko.create_cube() gecko.mesh{ segments = 2 }");
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("8");

  const download = page.waitForEvent("download");
  await page.locator("#console-save").click();
  const saved = await download;
  expect(saved.suggestedFilename()).toMatch(
    /^gecko-\d{4}-\d{2}-\d{2}-\d{6}\.lua$/,
  );

  const file = await readFile(await saved.path(), "utf8");
  expect(file).toContain("-- Written by gecko");
  expect(file).toContain("gecko.create_cube{ origin = {0, 0, 0}, size = 1 }");
  expect(file).toContain("gecko.mesh{ segments = 2 }");
  expect(errors).toEqual([]);
});

test("the page's Lua and the terminal's agree, byte for byte", async ({
  page,
}) => {
  // The same script runs in two hosts: one holds the session, the other
  // reaches it through JavaScript. Everything above those four primitives
  // is the same code, and this is what says so — the script computes a
  // digest of the VTK it produces and asserts it against a number the
  // Rust suite pinned by running the very same file.
  const errors = await boot(page);
  await openConsole(page);

  const script = await readFile(
    new URL(
      "../../../crates/gecko-lua/tests/scripts/agree.lua",
      import.meta.url,
    ),
    "utf8",
  );
  await lua(page, script);

  // Its own assertions did the work; what matters here is that none of
  // them fired.
  await expect(page.locator("[data-test=console-out]")).toContainText("digest");
  expect(
    await page.locator("#console-out .bad").count(),
    "the hosts disagree",
  ).toBe(0);
  // Both digests were printed, and the model the script leaves behind is
  // the smoothed grid of its second half.
  await expect(page.locator("[data-test=console-out]")).toContainText(
    "smoothed:",
  );
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("216");
  expect(errors).toEqual([]);
});

test("holding shift lights the chord, and ⇧Del takes it out", async ({
  page,
}) => {
  // The one gesture both chord operations share: hold ⇧ with a face
  // selected and the thread through it lights up, so what is about to go
  // is on screen before it goes.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );

  // Glued through the same door a script uses: what is under test is the
  // keyboard, not the mouse, and the mouse has its own tests.
  const seam = await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const [first, second] = facingPair();
    g.apply({ op: "glue_faces", first, second });
    await g.refresh();
    return first;
  });
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );

  // Selected from outside the pointer, with the block the surface lists it
  // under — the pair a click would have given.
  await page.evaluate((face) => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    const at = [...s.faces.keys()].find((q: number) => s.faces[q] === face)!;
    g.choose({ face, block: s.blocks[at] });
  }, seam);
  await expect(page.locator("#status")).toContainText("hold ⇧ for its chord");
  expect(await page.evaluate(() => (window as any).__gecko.sheet())).toEqual(
    [],
  );

  await page.keyboard.down("Shift");
  await expect(page.locator("#status")).toContainText("2 blocks");
  // Three faces along a chord of two blocks — two ends and the seam — four
  // edges each.
  expect(
    await page.evaluate(() => (window as any).__gecko.sheet().length),
  ).toBe(12);

  await page.keyboard.press("Delete");
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 0,
  );
  await page.keyboard.up("Shift");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  expect(errors).toEqual([]);
});

test("⇧P wraps that chord in an o-grid", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 16,
  );

  const seam = await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const [first, second] = facingPair();
    g.apply({ op: "glue_faces", first, second });
    await g.refresh();
    return first;
  });
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 12,
  );
  await page.evaluate((face) => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    const at = [...s.faces.keys()].find((q: number) => s.faces[q] === face)!;
    g.choose({ face, block: s.blocks[at] });
  }, seam);

  await page.keyboard.press("Shift+P");
  // Five blocks where each of the two was, and the message says so.
  await expect(page.locator("[data-test=blocks]")).toHaveText("10");
  await expect(page.locator("#status")).toContainText("o-grid along 2 blocks");
  expect(errors).toEqual([]);
});

test("Del takes away a face of a patch, not a block", async ({ page }) => {
  // One key, two meanings, and the kernel's own answer about which: a face
  // of an open sheet is a face, and a face of a block is how you point at
  // the block.
  const errors = await boot(page);
  await openSquare(page);
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.selectGeom([{ dim: 2, tag: 1 }]);
    g.showVerbs();
  });
  await page.keyboard.press("q");
  await page.waitForFunction(() => (window as any).__gecko.stats().faces === 1);

  // Picked the way a click picks, and the status line says what it is.
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    g.choose({ face: s.faces[0], block: s.blocks[0] });
  });
  await expect(page.locator("#status")).toContainText("of a patch");

  await page.keyboard.press("Delete");
  await page.waitForFunction(() => (window as any).__gecko.stats().faces === 0);
  const stats = await page.evaluate(() => (window as any).__gecko.stats());
  expect(stats.nodes).toBe(0);
  expect(stats.patches).toBe(0);
  expect(errors).toEqual([]);
});

test("dragging a face of a patch raises the whole sheet", async ({ page }) => {
  // The same gesture that grows a block out of a block's face. On an open
  // sheet it takes all of it, because half a patch cannot be raised.
  const errors = await boot(page);
  await openSquare(page);
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.selectGeom([{ dim: 2, tag: 1 }]);
    g.showVerbs();
  });
  await page.keyboard.press("q");
  await page.waitForFunction(() => (window as any).__gecko.stats().faces === 1);

  // Cut in two, so there is a component to take all of.
  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const edges = g.edgeNames();
    g.apply({ op: "split_sheet", edge: edges[0], at: 0.5 });
    await g.refresh();
  });
  await page.waitForFunction(() => (window as any).__gecko.stats().faces === 2);
  expect(
    await page.evaluate(() => (window as any).__gecko.stats().patches),
  ).toBe(1);

  // Raised through the page's own door, as the drag does it.
  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    g.apply({ op: "extrude_patch", face: s.faces[0], height: 0.5 });
    await g.refresh();
  });
  await page.waitForFunction(
    () => (window as any).__gecko.stats().blocks === 2,
  );
  const stats = await page.evaluate(() => (window as any).__gecko.stats());
  expect(stats.patches).toBe(0);
  expect(stats.nodes).toBe(12);
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  expect(errors).toEqual([]);
});

test("a quadrangle is laid on the picked surface with Q", async ({ page }) => {
  // The beginning of a surface blocking, through the page: pick a surface
  // of the geometry, press one key, and a face comes back already
  // classified — corners on corners, sides on curves, face on the surface.
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");

  // Selected the way a pick selects, without aiming a mouse at a triangle.
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.selectGeom([{ dim: 2, tag: 1 }]);
    g.showVerbs();
  });

  await page.keyboard.press("q");
  await page.waitForFunction(() => (window as any).__gecko.stats().faces === 1);

  const stats = await page.evaluate(() => (window as any).__gecko.stats());
  expect(stats.nodes).toBe(4);
  expect(stats.blocks).toBe(0);
  expect(stats.patches).toBe(1);
  expect(stats.classified.nodes).toBe(4);
  expect(stats.classified.edges).toBe(4);
  expect(stats.classified.faces).toBe(1);
  await expect(page.locator("#status")).toContainText("laid on surface 1");
  // And the readout says what there is now: one patch, no blocks.
  await expect(page.locator("[data-test=patches]")).toHaveText("1");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");

  expect(errors).toEqual([]);
});

test("a face of a patch is dragged either way, and the block follows", async ({
  page,
}) => {
  // Which way a free face is wound came from the triangles under it, and
  // nobody looking at the screen can tell — so dragging it towards you has
  // to build a block towards you, rather than a sliver the other way.
  const errors = await boot(page);
  await openSquare(page);
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.selectGeom([{ dim: 2, tag: 1 }]);
    g.showVerbs();
  });
  await page.keyboard.press("q");
  await page.waitForFunction(() => (window as any).__gecko.stats().faces === 1);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Selected by clicking it, which is what arms the drag.
  const face = (await page.evaluate(
    () => (window as any).__gecko.surface("faces").faces[0],
  )) as number;
  const at = (await page.evaluate(
    (f) => (window as any).__gecko.faceScreen(f),
    face,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(
    () => (window as any).__gecko.selection().length === 1,
  );

  // Down the screen, which on this camera is the side the face is *not*
  // wound towards: the direction the old clamp refused to go.
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  for (const step of [20, 60, 110]) {
    await page.mouse.move(box.x + at[0], box.y + at[1] + step);
  }
  const asked = (await page.evaluate(() =>
    (window as any).__gecko.grownBy(),
  )) as number;
  expect(asked).toBeLessThan(-0.2);
  await page.mouse.up();

  await page.waitForFunction(
    () => (window as any).__gecko.stats().blocks === 1,
  );
  const stats = await page.evaluate(() => (window as any).__gecko.stats());
  expect(stats.nodes).toBe(8);
  expect(stats.patches).toBe(0);

  // The far corners are where the drag asked for: below the face.
  const z = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return g.names().map((n: number) => g.nodePosition(n)[2]);
  })) as number[];
  expect(z.filter((v) => Math.abs(v) < 1e-6)).toHaveLength(4);
  for (const low of z.filter((v) => Math.abs(v) > 1e-6)) {
    expect(low).toBeCloseTo(asked, 6);
  }
  expect(errors).toEqual([]);
});

for (const [width, height] of [
  [1280, 800],
  [1100, 760],
  [900, 700],
] as const) {
  test(`the help card is read without scrolling at ${width}×${height}`, async ({
    page,
  }) => {
    // The complaint this answers: the card had grown past the bottom of
    // the window, and half of what it says is the half nobody reads. Three
    // ordinary window sizes, and the measurement is the plain one — what
    // the card wants to be is no taller than what it is.
    await page.setViewportSize({ width, height });
    const errors = await boot(page);
    await page.keyboard.press("h");
    await expect(page.locator("#help-card")).toBeVisible();

    const fits = await page.evaluate(() => {
      const card = document.querySelector("#help-card") as HTMLElement;
      return {
        wants: card.scrollHeight,
        has: card.clientHeight,
        window: window.innerHeight,
      };
    });
    // Not scrolling, first — the plain reading of the complaint.
    expect(
      fits.wants,
      `the card wants ${fits.wants}px and has ${fits.has}`,
    ).toBeLessThanOrEqual(fits.has);

    // And with room to spare, which the card's own height cannot show
    // because it shrinks to its contents: the measure has to be against
    // the window. The same words come out taller on another machine's
    // font engine, so the margin is the thing to assert. `56` is the
    // breathing room the card leaves round itself; the rest is for
    // whatever sets the type differently.
    //
    // That cushion was 24 pixels and it was not enough. A card fitted to
    // three pixels here wanted 654 on the runner where this machine said
    // 617 — a sixteenth taller, not a rounding — and the release stopped
    // on it. Doubled, which covers that measured difference twice over.
    expect(
      fits.wants,
      `the card's ${fits.wants}px in a ${fits.window}px window`,
    ).toBeLessThanOrEqual(fits.window - 56 - 48);
    expect(errors).toEqual([]);
  });
}

test("the window says which gecko it is", async ({ page }) => {
  const errors = await boot(page);

  // In the readout, where it is always visible.
  const shown = await page.locator("[data-test=version]").textContent();
  expect(shown).toMatch(/^\d+\.\d+\.\d+/);
  await expect(page.locator("#who")).toContainText("gecko-rs");

  // And on the help card, which is where anybody looks for it.
  await page.keyboard.press("h");
  await expect(page.locator("#help-who")).toContainText("gecko-rs");
  await expect(page.locator("#help-who")).toContainText(shown!);

  // One source: the kernel's own, so it cannot disagree with the release
  // it was built from.
  expect(await page.evaluate(() => (window as any).__gecko.version())).toBe(
    shown,
  );
  expect(errors).toEqual([]);
});

test("R smooths the blocking, by what the sliders say", async ({ page }) => {
  // The kernel's own smoothing is proven byte for byte by the agreement
  // script; what this covers is the key and the two sliders, which are
  // what a reader actually touches.
  const errors = await boot(page);
  await openConsole(page);

  // Eight cubes glued into a grid, which has one node inside it — the only
  // kind of node a smoother is allowed to move when nothing is classified.
  await lua(
    page,
    `for i = 0, 1 do for j = 0, 1 do for k = 0, 1 do
       gecko.create_cube{ origin = {i, j, k} }
     end end end
     local seen, both = {}, {}
     for _, f in ipairs(gecko.faces()) do
       local key = string.format("%.3f/%.3f/%.3f", f.center[1], f.center[2], f.center[3])
       if seen[key] then both[#both + 1] = { seen[key], f.id } else seen[key] = f.id end
     end
     for _, pair in ipairs(both) do gecko.glue_faces(pair[1], pair[2]) end
     for _, n in ipairs(gecko.nodes()) do
       if math.abs(n.pos[1] - 1) < 1e-9 and math.abs(n.pos[2] - 1) < 1e-9
         and math.abs(n.pos[3] - 1) < 1e-9 then
         gecko.move_node(n.id, {1.4, 1.3, 1.2})
       end
     end`,
  );
  await page.waitForFunction(
    () => (window as any).__gecko.stats().blocks === 8,
  );

  // All the way to the middle, once, so where the node lands says that the
  // slider was the number sent.
  await openParams(page);
  await page.locator("[data-test=smooth-rounds]").fill("1");
  await page.locator("[data-test=smooth-share]").fill("1");
  await expect(page.locator("[data-test=smooth-share-value]")).toHaveText(
    "100%",
  );

  // Out of the slider first: anything typed into a field is typing, which
  // is exactly the rule the keyboard is supposed to follow.
  await page.evaluate(() =>
    (document.activeElement as HTMLElement | null)?.blur(),
  );
  await page.keyboard.press("r");
  await expect(page.locator("#status")).toContainText(
    "smoothed 1 time by 100%",
  );

  // And the node went back to the middle of its ring, which for this grid
  // is where it started.
  const back = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return g
      .names()
      .map((n: number) => g.nodePosition(n))
      .find((p: number[]) => p.every((x) => Math.abs(x - 1) < 1e-6));
  })) as number[] | undefined;
  expect(back, "the node should be back at (1, 1, 1)").toBeDefined();
  expect(errors).toEqual([]);
});

/**
 * Three blocks in an L, which is the smallest thing with a corner that
 * shades: two walls meeting at 270 degrees. A cube on its own is convex all
 * over and ambient occlusion has nothing to say about it.
 */
async function anLShape(page: import("@playwright/test").Page) {
  for (const origin of [
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0],
  ]) {
    await page.evaluate(
      (at) =>
        (window as any).__gecko.apply({
          op: "create_cube",
          origin: at,
          size: 1,
        }),
      origin,
    );
  }
  await page.evaluate(() => (window as any).__gecko.refresh());
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());
}

/** The drawer, open at the fold the shading's switch lives in. */
async function openShading(page: import("@playwright/test").Page) {
  await openDrawer(page);
  await page
    .locator("#sec-colors")
    .evaluate((d) => ((d as HTMLDetailsElement).open = true));
  await expect(page.locator("#show-ao")).toBeVisible();
}

/**
 * What switching the shading on does to the picture, with the opacity held
 * still either side so that the shading is the only difference.
 */
async function shadingDiff(page: import("@playwright/test").Page) {
  return (await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const frame = () =>
      new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const grab = () => {
      const flat = document.createElement("canvas");
      flat.width = view.width;
      flat.height = view.height;
      flat.getContext("2d")!.drawImage(view, 0, 0);
      return flat.getContext("2d")!.getImageData(0, 0, view.width, view.height)
        .data;
    };
    /**
     * A picture that has stopped changing.
     *
     * Switching the shading on builds a post-process pipeline, and the
     * frames before its shaders are compiled come out of the combine black.
     * Reading one of those would measure the compile rather than the
     * shading, so this waits for two frames that agree.
     */
    const read = async () => {
      let last = grab();
      for (let i = 0; i < 60; i++) {
        await frame();
        const now = grab();
        let moved = 0;
        for (let p = 0; p < now.length; p += 4)
          moved += Math.abs(now[p] - last[p]);
        last = now;
        if (moved / (now.length / 4) < 0.5) return now;
      }
      return last;
    };
    const box = document.getElementById("show-ao") as HTMLInputElement;
    if (box.checked) box.click();
    // Opaque before the first reading, so that turning the shading on moves
    // nothing but the shading: it opens the faces too, and a comparison
    // across that would only be measuring the transparency.
    const slider = document.querySelector(
      'input[data-test="faces-alpha"]',
    ) as HTMLInputElement;
    slider.value = "1";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    const plain = await read();
    box.click();
    const shaded = await read();

    let lit = 0;
    let darkened = 0;
    let untouched = 0;
    let deepest = 0;
    for (let i = 0; i < plain.length; i += 4) {
      const before = (plain[i] + plain[i + 1] + plain[i + 2]) / 3;
      // The background, which has no surface in it to shade.
      if (before <= 30) continue;
      lit++;
      const drop = before - (shaded[i] + shaded[i + 1] + shaded[i + 2]) / 3;
      if (drop > 8) darkened++;
      if (Math.abs(drop) <= 2) untouched++;
      if (drop > deepest) deepest = drop;
    }
    return { lit, darkened, untouched, deepest, on: g.occluding() };
  })) as {
    lit: number;
    darkened: number;
    untouched: number;
    deepest: number;
    on: boolean;
  };
}

test("shading the hollows darkens the corner and leaves the open faces alone", async ({
  page,
}) => {
  // Two bugs met here, and this one test holds both down.
  //
  // The surfaces' normals come off the winding of the kernel's quads, which
  // points **into** the block. Nothing had ever read them — a two-sided
  // material flips a back-facing normal before lighting it — so the mistake
  // sat there until the occlusion read them out of a geometry buffer, where
  // nothing flips anything, pointed its hemisphere into the solid and found
  // the solid everywhere. The whole model went dark by a quarter.
  //
  // And the wireframe and the handles must stay out of that buffer. They
  // are geometry like anything else, and a line lying along an edge of the
  // model occludes the faces either side of it: soot along every edge, a
  // ring round every handle. Both faults darken nearly every lit pixel,
  // which is what `untouched` is here to refuse.
  const errors = await boot(page);
  await anLShape(page);

  const diff = await shadingDiff(page);
  const where = JSON.stringify(diff);
  expect(diff.on, where).toBe(true);
  // It does something: the inside of the corner goes down a long way.
  expect(diff.deepest, where).toBeGreaterThan(20);
  expect(diff.darkened, where).toBeGreaterThan(500);
  // And it does not do it everywhere. Most of what is on screen is flat
  // wall facing open air, which is exactly what ambient occlusion leaves
  // alone.
  expect(diff.untouched / diff.lit, where).toBeGreaterThan(0.5);
  expect(errors).toEqual([]);
});

test("the shading opens the faces, and closing it hands the transparency back", async ({
  page,
}) => {
  // The switch is two things at once because one is useless without the
  // other: there is nothing to shade on a surface you can see through. It
  // has to be a round trip, though — a setting that quietly keeps your
  // faces opaque after you turned it off is a setting nobody trusts.
  const errors = await boot(page);
  await anLShape(page);
  await openShading(page);

  const alpha = () =>
    page.evaluate(
      () => (window as any).__gecko.display().faces.alpha,
    ) as Promise<number>;
  const before = await alpha();
  expect(before).toBeLessThan(1);

  await page.locator("#show-ao").click();
  await expect(page.locator("#status")).toContainText("hollows shaded");
  expect(await alpha()).toBe(1);
  expect(await page.evaluate(() => (window as any).__gecko.occluding())).toBe(
    true,
  );

  await page.locator("#show-ao").click();
  await expect(page.locator("#status")).toContainText("hollows plain");
  expect(await alpha()).toBe(before);
  expect(await page.evaluate(() => (window as any).__gecko.occluding())).toBe(
    false,
  );
  expect(errors).toEqual([]);
});

test("the triad is still drawn while the hollows are shaded", async ({
  page,
}) => {
  // The pipeline is given the geometry buffer rather than left to take a
  // prepass, which is scene-wide: it rode over the corner camera's own
  // viewport and left the corner empty.
  const errors = await boot(page);
  await anLShape(page);
  await openShading(page);
  await page.locator("#show-ao").click();
  await page.evaluate(() => (window as any).__gecko.settled());

  // The X arm is red and nothing else in the scene is.
  const red = await page.evaluate(() => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const flat = document.createElement("canvas");
    flat.width = view.width;
    flat.height = view.height;
    flat.getContext("2d")!.drawImage(view, 0, 0);
    const p = flat
      .getContext("2d")!
      .getImageData(0, 0, view.width, view.height).data;
    let n = 0;
    for (let i = 0; i < p.length; i += 4) {
      if (p[i] > 150 && p[i + 1] < 90 && p[i + 2] < 90) n++;
    }
    return n;
  });
  expect(red).toBeGreaterThan(20);
  expect(errors).toEqual([]);
});

test("the wireframe and the handles are kept out of the shading", async ({
  page,
}) => {
  // The occlusion is computed from a buffer of its own, and what goes into
  // that buffer is what counts as *the model*. A greased line lying along an
  // edge is not the model — it is a drawing of one — and left in, it shades
  // the faces either side of it: on a grid of twenty-seven blocks it raises
  // the shaded pixels by three quarters, soot along every edge.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await cutGrid(page);
  await openShading(page);
  await page.locator("#show-ao").click();
  expect(await page.evaluate(() => (window as any).__gecko.occluding())).toBe(
    true,
  );

  const shaded = (await page.evaluate(() =>
    (window as any).__gecko.shadedNames(),
  )) as string[];
  // Everything in it is a surface, one mesh per block.
  expect(shaded.length).toBeGreaterThan(20);
  expect(
    shaded.every((name) => /^(faces|blocks)-\d+$/.test(name)),
    shaded.join(" "),
  ).toBe(true);
  // And the two things that would spoil it are not.
  expect(shaded).not.toContain("edges");
  expect(shaded.filter((name) => name.startsWith("node"))).toEqual([]);
  expect(errors).toEqual([]);
});

test("two walls of a block that face different ways are drawn different shades", async ({
  page,
}) => {
  // Under a single hemispheric light a surface is shaded by how far it
  // faces *up*, and the four walls of a block all face sideways — so they
  // came out the same color exactly, and a block read as a flat outline
  // with a lid. The second light comes from one side, which is what makes
  // a corner between two walls visible at all.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const shades = (await page.evaluate(() => {
    const el = (key: string) =>
      document.getElementById(key) ??
      document.querySelector(`[data-test="${key}"]`);
    // The faces alone: a wireframe and a handle have colors of their own,
    // and this is counting the colors of the surfaces.
    for (const key of ["edges-visible", "nodes-visible"]) {
      const box = el(key) as HTMLInputElement;
      if (box.checked) box.click();
    }
    const slider = el("faces-alpha") as HTMLInputElement;
    slider.value = "1";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    return new Promise<number[]>((resolve) =>
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const view = document.querySelector("#view") as HTMLCanvasElement;
          const flat = document.createElement("canvas");
          flat.width = view.width;
          flat.height = view.height;
          flat.getContext("2d")!.drawImage(view, 0, 0);
          const p = flat
            .getContext("2d")!
            .getImageData(0, 0, view.width, view.height).data;
          const tally = new Map<number, number>();
          let lit = 0;
          for (let i = 0; i < p.length; i += 4) {
            const lum = (p[i] + p[i + 1] + p[i + 2]) / 3;
            if (lum <= 30) continue;
            lit++;
            const bucket = Math.round(lum / 4);
            tally.set(bucket, (tally.get(bucket) ?? 0) + 1);
          }
          // A flat face is one exact color, so each one is a bucket of its
          // own; what is left over is the anti-aliasing along the edges,
          // which never adds up to a fiftieth of the model.
          resolve(
            [...tally.entries()]
              .filter(([, n]) => n > lit / 50)
              .map(([bucket]) => bucket * 4)
              .sort((a, b) => a - b),
          );
        }),
      ),
    );
  })) as number[];

  // Three faces of the cube are on screen, and all three are told apart:
  // the lid, and two walls that no longer match.
  expect(shades.length, shades.join(" ")).toBeGreaterThanOrEqual(3);
  expect(errors).toEqual([]);
});

test("the shading does nothing at all where you can see through the model", async ({
  page,
}) => {
  // The switch opens the faces, but the opacity slider is still yours, and
  // turning it back down while the shading is on used to put pitch black
  // patches across half the model. One pixel of the buffer holds one depth
  // and one normal; a translucent face puts several surfaces on that pixel
  // and what lands there is whichever was drawn last — for blended
  // surfaces, not even the nearest. Half the model then read its shading
  // off a face pointing away from the camera, and a hemisphere aimed into
  // the solid finds the solid.
  const errors = await boot(page);
  await anLShape(page);
  await openShading(page);
  await page.locator("#show-ao").click();
  expect(await page.evaluate(() => (window as any).__gecko.occluding())).toBe(
    true,
  );

  const diff = await translucentDiff(page);
  const where = JSON.stringify(diff);
  // Seen through, the picture is the same whether the shading is on or off.
  expect(diff.untouched / diff.lit, where).toBeGreaterThan(0.95);
  expect(diff.blackened, where).toBe(0);
  expect(errors).toEqual([]);
});

/**
 * What the shading does to a picture whose faces are **not** opaque: it
 * should do nothing, and the measure of nothing is that no lit pixel goes
 * dark.
 */
async function translucentDiff(page: import("@playwright/test").Page) {
  return (await page.evaluate(async () => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const frame = () =>
      new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const grab = () => {
      const flat = document.createElement("canvas");
      flat.width = view.width;
      flat.height = view.height;
      flat.getContext("2d")!.drawImage(view, 0, 0);
      return flat.getContext("2d")!.getImageData(0, 0, view.width, view.height)
        .data;
    };
    const read = async () => {
      let last = grab();
      for (let i = 0; i < 60; i++) {
        await frame();
        const now = grab();
        let moved = 0;
        for (let p = 0; p < now.length; p += 4)
          moved += Math.abs(now[p] - last[p]);
        last = now;
        if (moved / (now.length / 4) < 0.5) return now;
      }
      return last;
    };
    const slider = document.querySelector(
      'input[data-test="faces-alpha"]',
    ) as HTMLInputElement;
    const box = document.getElementById("show-ao") as HTMLInputElement;
    // Half open, with the shading on — the state somebody reaches by pulling
    // the slider back down after switching it on.
    slider.value = "0.5";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    const shaded = await read();
    box.click();
    slider.value = "0.5";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    const plain = await read();

    let lit = 0;
    let untouched = 0;
    let blackened = 0;
    for (let i = 0; i < plain.length; i += 4) {
      const before = (plain[i] + plain[i + 1] + plain[i + 2]) / 3;
      if (before <= 30) continue;
      lit++;
      const after = (shaded[i] + shaded[i + 1] + shaded[i + 2]) / 3;
      if (Math.abs(before - after) <= 2) untouched++;
      // A lit pixel that went out altogether: the fault this refuses.
      if (before > 50 && after < 12) blackened++;
    }
    return { lit, untouched, blackened };
  })) as { lit: number; untouched: number; blackened: number };
}

/** The pixels the aiming mark covers — the pink one, as against the sheet's
 *  gold. */
async function aimPixels(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const flat = document.createElement("canvas");
    flat.width = view.width;
    flat.height = view.height;
    flat.getContext("2d")!.drawImage(view, 0, 0);
    const p = flat
      .getContext("2d")!
      .getImageData(0, 0, view.width, view.height).data;
    let n = 0;
    for (let i = 0; i < p.length; i += 4) {
      if (
        p[i] > 190 &&
        p[i + 1] > 40 &&
        p[i + 1] < 130 &&
        p[i + 2] > 60 &&
        p[i + 2] < 170
      )
        n++;
    }
    return n;
  });
}

test("the mark a cut is aimed with is hidden by the model too", async ({
  page,
}) => {
  // The sheet stopped being drawn over everything, and the mark along it
  // did not: the red showing where the cut lands came out in front of the
  // blocks it runs behind, so a cut at the back read as loudly as one at
  // the front — on every edge of the sheet at once, which is a lot of red
  // in the wrong place.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  // In two each way, so the sheet has edges with blocks in front of them.
  await cutInHalf(page, [0.5, 0, 0]);
  await cutInHalf(page, [0, 0.5, 0]);
  await cutInHalf(page, [0, 0, 0.5]);
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 27,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const edge = (await page.evaluate(() => edgeNear([0, 0.25, 0]))) as number;
  const at = (await page.evaluate(
    (e) => (window as any).__gecko.edgeScreen(e),
    edge,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  await page.waitForFunction(() => (window as any).__gecko.sheet().length > 0);
  for (let i = 1; i <= 5; i++)
    await page.mouse.move(box.x + at[0], box.y + at[1] - i * 10);
  await page.waitForFunction(() => (window as any).__gecko.aimedAt() !== null);
  await page.evaluate(() => (window as any).__gecko.settled());
  const through = await aimPixels(page);
  expect(through, "the mark is drawn at all").toBeGreaterThan(100);

  // The faces away, with the pointer still down so the aim is untouched:
  // nothing in front of the mark now. Drawn over everything, the two
  // counts would be the same number.
  await page.evaluate(() => (window as any).__gecko.run("faces"));
  await page.waitForFunction(
    () => !(window as any).__gecko.prefs().faces.visible,
  );
  await page.evaluate(() => (window as any).__gecko.settled());
  const bare = await aimPixels(page);
  await page.mouse.up();

  expect(bare, `through ${through}, bare ${bare}`).toBeGreaterThan(
    through * 1.1,
  );
  expect(errors).toEqual([]);
});

test("the hollows wait for a single view, and four views are all drawn", async ({
  page,
}) => {
  // The shading belongs to one camera, and a camera drawing into a corner
  // of the canvas is more than the pipeline can follow: in four views it
  // was not merely missing from the three orthographic panes, it was
  // *wrong* in the 3-D one — the occlusion of the model landed in the
  // empty space beside it, because the pipeline shades the whole canvas
  // while that camera owns a quarter of it.
  //
  // Both ways out were tried and measured. Attached to all four cameras,
  // each pane's output wipes the one before it and the window goes black —
  // which is what the second half of this test refuses. Told to respect
  // the viewports, every pane draws itself at a quarter size in a corner.
  // So the shading waits, and the preference is left alone.
  const errors = await boot(page);
  await anLShape(page);
  await openShading(page);
  await page.locator("#show-ao").click();
  expect(await page.evaluate(() => (window as any).__gecko.occluding())).toBe(
    true,
  );

  // Through the action rather than the key: the switch was just clicked,
  // so the keyboard belongs to the drawer.
  await page.evaluate(() => (window as any).__gecko.run("quad"));
  await page.waitForFunction(() => (window as any).__gecko.prefs().quad);
  await page.evaluate(() => (window as any).__gecko.settled());

  // Nothing is shaded...
  expect(await page.evaluate(() => (window as any).__gecko.occluding())).toBe(
    false,
  );
  await expect(page.locator("#status")).toContainText("hollows plain here");
  // ...and nothing about what you asked for has been thrown away: the
  // faces are still opaque and the switch is still on.
  expect(await page.evaluate(() => (window as any).__gecko.prefs().ao)).toBe(
    true,
  );
  expect(
    await page.evaluate(() => (window as any).__gecko.display().faces.alpha),
  ).toBe(1);

  // And all four panes have something in them.
  const quadrants = (await page.evaluate(() => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const flat = document.createElement("canvas");
    flat.width = view.width;
    flat.height = view.height;
    flat.getContext("2d")!.drawImage(view, 0, 0);
    const p = flat
      .getContext("2d")!
      .getImageData(0, 0, view.width, view.height).data;
    const lit = [0, 0, 0, 0];
    for (let y = 0; y < view.height; y++) {
      for (let x = 0; x < view.width; x++) {
        const i = (y * view.width + x) * 4;
        if ((p[i] + p[i + 1] + p[i + 2]) / 3 <= 40) continue;
        lit[(y < view.height / 2 ? 0 : 2) + (x < view.width / 2 ? 0 : 1)]++;
      }
    }
    return lit;
  })) as number[];
  expect(Math.min(...quadrants), quadrants.join(" ")).toBeGreaterThan(1000);

  // Back to one view, and it comes back by itself: it was waiting, not
  // switched off.
  await page.evaluate(() => (window as any).__gecko.run("quad"));
  await page.waitForFunction(() => !(window as any).__gecko.prefs().quad);
  expect(await page.evaluate(() => (window as any).__gecko.occluding())).toBe(
    true,
  );
  await expect(page.locator("#status")).toContainText("hollows shaded");
  expect(errors).toEqual([]);
});

/** The picture once it has stopped changing, as a count of its pixels by
 *  how dark they are. Switching the shading on builds a pipeline, and the
 *  frames before its shaders are compiled come out of the combine black. */
async function darkPixels(page: import("@playwright/test").Page) {
  return (await page.evaluate(async () => {
    const view = document.querySelector("#view") as HTMLCanvasElement;
    const frame = () =>
      new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const grab = () => {
      const flat = document.createElement("canvas");
      flat.width = view.width;
      flat.height = view.height;
      flat.getContext("2d")!.drawImage(view, 0, 0);
      return flat.getContext("2d")!.getImageData(0, 0, view.width, view.height)
        .data;
    };
    let last = grab();
    for (let i = 0; i < 60; i++) {
      await frame();
      const now = grab();
      let moved = 0;
      for (let p = 0; p < now.length; p += 4)
        moved += Math.abs(now[p] - last[p]);
      last = now;
      if (moved / (now.length / 4) < 0.5) break;
    }
    let blacker = 0;
    let lit = 0;
    for (let i = 0; i < last.length; i += 4) {
      const lum = (last[i] + last[i + 1] + last[i + 2]) / 3;
      // The background is around 23. Below ten is blacker than the window
      // itself, which nothing in a shaded model should be.
      if (lum < 10) blacker++;
      if (lum > 40) lit++;
    }
    return { blacker, lit };
  })) as { blacker: number; lit: number };
}

test("a block that lends its faces to its neighbours is still shaded the right way out", async ({
  page,
}) => {
  // Reported with this script: one face of the boundary came out pitch
  // black under the shading.
  //
  // A face shared by two blocks is drawn once, under one of them, and a
  // block can end up contributing a single face to what is drawn — its
  // neighbours having claimed the other five. The normals were turned
  // outward by asking the drawn quadrangles which way was out, and a lone
  // flat quadrangle cannot answer: its own middle lies in its own plane,
  // so the question came out exactly zero and the kernel's winding, which
  // points into the block, was kept. A hemisphere aimed into the solid
  // finds the solid.
  //
  // The block's own center answers it, and the surface builder has that
  // from all six of its faces whether they are drawn or not.
  //
  // The script is a fixture rather than a literal here, and it chooses
  // every cell by where it is: it was recorded by name, and the names
  // moved when the block structure stopped being a combinatorial map. The
  // Rust suite runs the same file and pins the shape it builds.
  const errors = await boot(page);
  await page.locator("#msh").setInputFiles({
    name: "boundary.lua",
    mimeType: "text/plain",
    buffer: await readFile(
      new URL(
        "../../../crates/gecko-lua/tests/scripts/boundary.lua",
        import.meta.url,
      ),
    ),
  });
  await expect(page.locator("[data-test=blocks]")).toHaveText("21");
  await page.evaluate(() => (window as any).__gecko.settled());

  // Through the element rather than the drawer: replaying a script opens
  // the console over it, and this test is not about where the switch is.
  await page.evaluate(() =>
    (document.getElementById("show-ao") as HTMLInputElement).click(),
  );
  await page.waitForFunction(() => (window as any).__gecko.occluding());

  const shaded = await darkPixels(page);
  expect(shaded.lit, JSON.stringify(shaded)).toBeGreaterThan(10000);
  // Not one pixel blacker than the window it is drawn in.
  expect(shaded.blacker, JSON.stringify(shaded)).toBe(0);
  expect(errors).toEqual([]);
});

test("a group filter says so on the screen, and dies with the session", async ({
  page,
}) => {
  // The filter is the one setting that can empty the view entirely, and the
  // checkboxes that work it are in a drawer that is shut by default. So it
  // admits to what it is doing while it is on, and it does not outlive the
  // session: it names the groups of one model, and the page always opens
  // without a model.
  const errors = await boot(page);
  const drawn = () =>
    page.evaluate(() => (window as any).__gecko.nodesDrawn());

  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  expect(await drawn()).toBe(8);
  // Nothing held back, nothing said.
  await expect(page.locator("[data-test=filtered]")).toHaveText("");

  await openDrawer(page);
  await openSection(page, "groups");
  await page.locator("[data-test=ungrouped-visible]").uncheck();
  await expect.poll(drawn).toBe(0);

  // The readout still counts the block the kernel holds — which is exactly
  // the trap — so the screen has to carry the reason next to it.
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  await expect(page.locator("[data-test=filtered]")).toHaveText(
    "nothing drawn — the group filter is hiding blocks in no group",
  );

  // And it is gone on the next visit. The block made after the reload is
  // drawn, where before this it would have arrived invisible and stayed
  // that way for every session on this origin.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await expect.poll(drawn).toBe(8);
  await expect(page.locator("[data-test=filtered]")).toHaveText("");

  // Not merely ignored on the way in: the stored copy is scrubbed, so an
  // older version's list cannot come back if one is ever read again.
  expect(
    await page.evaluate(
      () => JSON.parse(localStorage.getItem("gecko.view") ?? "{}").hiddenGroups,
    ),
  ).toEqual([]);
  expect(errors).toEqual([]);
});

test("a hidden group's name does not follow the browser to the next model", async ({
  page,
}) => {
  // `hiddenGroups` holds names, and a name belongs to a model. Stored per
  // origin, a `fluid` hidden in one model used to hide a different model's
  // `fluid` on sight, in a session nobody had touched the filter in.
  const errors = await boot(page);
  const drawn = () =>
    page.evaluate(() => (window as any).__gecko.nodesDrawn());

  const twoCubesInAGroup = async () => {
    await page.locator("#cube").click();
    await page.waitForFunction(
      () => (window as any).__gecko.counts().nodes === 8,
    );
    await page.locator("#cube").click();
    await page.waitForFunction(
      () => (window as any).__gecko.counts().nodes === 16,
    );
    await openDrawer(page);
    await openSection(page, "groups");
    await page.evaluate(() => {
      const g = (window as any).__gecko;
      const s = g.surface("faces");
      g.choose({ face: s.faces[0], block: s.blocks[0] });
    });
    await page.locator("[data-test=group-new]").fill("fluid");
    await page.locator("[data-test=group-make]").click();
    await expect(page.locator("[data-test=group-name]")).toHaveValue("fluid");
  };

  await twoCubesInAGroup();
  await page.locator("[data-test=group-shown]").uncheck();
  await expect.poll(drawn).toBe(8);
  await expect(page.locator("[data-test=filtered]")).toHaveText(
    "group filter: 1 group hidden",
  );

  // A new session, a new model, the same name — and all of it drawn.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
  await twoCubesInAGroup();
  await expect(page.locator("[data-test=group-shown]")).toBeChecked();
  await expect.poll(drawn).toBe(16);
  await expect(page.locator("[data-test=filtered]")).toHaveText("");
  expect(errors).toEqual([]);
});

test("a solid model hides what is behind it from the pointer", async ({
  page,
}) => {
  // Picking follows the picture. Through a shell you can see through, the
  // far side is fair game — being able to see it is what that means. Once
  // the faces go opaque the far side is not on the screen any more, and a
  // click that still reached it was reaching through the model.
  const errors = await boot(page);
  // The drawer first, and open for the whole test: it takes the canvas
  // narrower, and a probe is a screen position. Both halves have to be
  // measured in the same picture.
  await openDrawer(page);
  await openSection(page, "cells");
  await openGroup(page, "faces");
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );

  const settle = () => page.evaluate(() => (window as any).__gecko.settled());
  const opacity = async (alpha: string) => {
    await page.locator("[data-test=faces-alpha]").fill(alpha);
    await settle();
  };

  /** The edges that answer a pick aimed at their own midpoint. */
  const edges = () =>
    page.evaluate(() => {
      const g = (window as any).__gecko;
      return (g.edgeNames() as number[]).filter((name) => {
        const at = g.edgeScreen(name) as [number, number] | null;
        return at !== null && g.pickEdge(at[0], at[1]) === name;
      });
    });

  // From where the view opens, which is a corner of the cube: three of the
  // twelve edges are behind the model.
  const clear = await edges();
  await opacity("1");
  const solid = await edges();
  expect(
    solid.length,
    "an opaque shell takes edges away from the pointer",
  ).toBeLessThan(clear.length);
  expect(
    clear,
    "and takes none that were not reachable before",
  ).toEqual(expect.arrayContaining(solid));

  // Now squarely down one axis, where the near face of the cube sits
  // between the camera and all four of the far corners, and each of those
  // still has a pixel of its own to be aimed at.
  await page.evaluate(() => (window as any).__gecko.snapTo("+x"));
  await settle();

  /** The corners that answer a pick aimed at their own handle, by the side
   *  of the cube they are on. */
  const corners = () =>
    page.evaluate(() => {
      const g = (window as any).__gecko;
      const out = { near: [] as number[], far: [] as number[] };
      for (const name of g.names() as number[]) {
        const at = g.nodePosition(name) as [number, number, number];
        const screen = g.nodeScreen(name) as [number, number] | null;
        if (!screen) continue;
        if (g.pickNode(screen[0], screen[1]) === name) {
          (at[0] > 0.5 ? out.near : out.far).push(name);
        }
      }
      return out;
    });

  const behind = await corners();
  expect(behind.far, "nothing behind a solid model answers").toEqual([]);
  expect(
    behind.near,
    "and the corners on the near side are still there to grab",
  ).toHaveLength(4);

  // The control: it is the opacity doing it and not the angle. Hand the
  // transparency back and the far four answer again.
  await opacity("0.35");
  const through = await corners();
  expect(
    through.far.length,
    "a shell you can see through is one you can pick through",
  ).toBe(4);
  expect(through.near).toHaveLength(4);
  expect(errors).toEqual([]);
});

/** What the halo is ringing, how much ring reached the screen, and the
 *  readout's own words for it. Three answers from one place, because the
 *  claim under test is that they agree. */
async function ringed(page: import("@playwright/test").Page) {
  return {
    ...(await page.evaluate(() => ({
      hovered: (window as any).__gecko.hovered(),
      halo: (window as any).__gecko.halo(),
    }))),
    aim: await page.locator("[data-test=aim]").textContent(),
  };
}

test("a halo rings the cell a click would take, and the readout names it", async ({
  page,
}) => {
  // Picking is the hard part of putting a blocking onto a model: a corner of
  // a block and a corner of the geometry are a pixel apart from most
  // angles. So the cell a click would take is ringed before the click, and
  // named — a ring says where, and classifying onto curve 7 rather than
  // curve 8 needs the number.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  const look = async (at: [number, number]) => {
    await page.mouse.move(box.x + at[0], box.y + at[1]);
    // The resolution is put off to the frame that would show it, so the
    // answer is one frame away however fast the mouse was moved.
    await page.evaluate(() => (window as any).__gecko.settled());
    return ringed(page);
  };

  // A handle: a ball, and no ribbon.
  const node = (
    await page.evaluate(() => (window as any).__gecko.names())
  )[0] as number;
  const onNode = await look(
    (await page.evaluate(
      (n) => (window as any).__gecko.nodeScreen(n),
      node,
    )) as [number, number],
  );
  expect(onNode.hovered).toEqual({ kind: "node", name: node });
  expect(onNode.halo).toMatchObject({ marks: 1, sides: 0 });
  expect(onNode.aim).toBe(`picking blocks — node ${node}`);

  // An edge: one side of ribbon, along the segment the wireframe draws.
  const edge = (
    await page.evaluate(() => (window as any).__gecko.edgeNames())
  )[0] as number;
  const onEdge = await look(
    (await page.evaluate(
      (e) => (window as any).__gecko.edgeScreen(e),
      edge,
    )) as [number, number],
  );
  expect(onEdge.hovered).toEqual({ kind: "edge", name: edge });
  expect(onEdge.halo).toMatchObject({ marks: 0, sides: 1 });
  expect(onEdge.aim).toBe(`picking blocks — edge ${edge}`);

  // A face: four sides, round the quadrangle as it is drawn. Aimed at a
  // face whose own center answers for it — from an oblique camera the
  // center of a far face lands on a near one, which is the picture being
  // right rather than the test being wrong.
  const face = await page.evaluate(() => {
    const g = (window as any).__gecko;
    for (const name of g.surface("faces").faces as number[]) {
      const at = g.faceScreen(name) as [number, number] | null;
      if (!at) continue;
      const hit = g.pickSurface(at[0], at[1]) as { face: number } | null;
      if (hit && hit.face === name) return { name, at };
    }
    return null;
  });
  expect(face, "some face answers for its own center").not.toBeNull();
  const onFace = await look(face!.at);
  expect(onFace.hovered).toMatchObject({ kind: "face", face: face!.name });
  expect(onFace.halo).toMatchObject({ marks: 0, sides: 4 });
  expect(onFace.aim).toContain(`face ${face!.name} of block `);

  // And nothing over nothing. The corner of the canvas is background, and
  // the readout goes back to saying only which mode it is in.
  const onNothing = await look([3, 3]);
  expect(onNothing.hovered).toBeNull();
  expect(onNothing.halo).toMatchObject({ marks: 0, sides: 0 });
  expect(onNothing.aim).toBe("picking blocks");

  // **Two meshes, for the whole session.** A ball and a thread, made the
  // first time each is wanted and shown, hidden and given new points from
  // then on. Four kinds of cell have been pointed at by now, twice over
  // for some, and the count has to have stopped at two.
  //
  // And the thread, given a face's four sides after an edge's one, has a
  // width for every vertex. Babylon keeps the width table a line was made
  // with, and the vertices past it are drawn or not as the GPU pleases —
  // here, yes; on a Mac, no. That was every curve of a real model.
  const onFaceAgain = await look(face!.at);
  expect(onFaceAgain.halo.vertices).toBe(16);
  expect(onFaceAgain.halo.widths).toBe(onFaceAgain.halo.vertices);
  await look([3, 3]);
  const kept = await ringed(page);
  expect(
    kept.halo.builds,
    "the halo's meshes are made once and reused",
  ).toBeLessThanOrEqual(2);
  expect(errors).toEqual([]);
});

test("what the halo rings is what the click takes", async ({ page }) => {
  // The claim the whole feature rests on. The halo is drawn from the same
  // function the click is decided by — `targetAt` — and this is what says
  // so: for every position probed, the ring names a cell and the click at
  // that very pixel takes *that* cell. Two resolution orders that agree
  // today and drift tomorrow would fail here.
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  await page.evaluate(() => (window as any).__gecko.settled());
  await page.evaluate(() => (window as any).__gecko.run("picking"));
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe(
    "geometry",
  );

  const box = (await page.locator("#view").boundingBox())!;
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());

  // A grid over the square, which catches its surface, its four curves and
  // its four corners. Clicking in geometry mode toggles an entity, so every
  // probe is undone by a second click and the next one starts from nothing.
  let probed = 0;
  // Five by five. Each probe is a move, a click and a click back, with a
  // frame waited for after each — seven by seven was a minute's work on a
  // slow runner, and the probes that matter most are the three below.
  for (let u = 0.2; u <= 0.8; u += 0.15) {
    for (let v = 0.2; v <= 0.8; v += 0.15) {
      const at: [number, number] = [box.width * u, box.height * v];
      await page.mouse.move(box.x + at[0], box.y + at[1]);
      await settle();
      const { hovered } = await ringed(page);
      if (hovered === null) continue;
      await page.mouse.click(box.x + at[0], box.y + at[1]);
      await settle();
      const picked = (await page.evaluate(() =>
        (window as any).__gecko.geomSelection(),
      )) as { dim: number; tag: number }[];
      expect(
        picked,
        `a click where ${JSON.stringify(hovered)} was ringed takes it`,
      ).toEqual([hovered.spot]);
      probed += 1;
      // Back to nothing picked, for the next probe.
      await page.mouse.click(box.x + at[0], box.y + at[1]);
      await settle();
    }
  }
  // The square covers the middle of the window and not all of it, so a
  // handful of the twenty-five land on it — which is enough: the probes
  // below make sure of one of each kind.
  expect(probed, "the grid found something to ring").toBeGreaterThanOrEqual(3);

  // And one of **each** kind, found rather than hoped for. A coarse grid
  // lands on a surface everywhere and on a corner or a curve only by luck,
  // now that those are targets a few pixels across rather than a tenth of
  // the model — so they are swept for.
  const each = (await page.evaluate(
    ([w, h]) => {
      const g = (window as any).__gecko;
      const found: Record<number, [number, number]> = {};
      // Every pixel is a pick three deep, so the sweep stops the moment it
      // has one of each — a full sweep at this pitch is two hundred
      // thousand picks, and on a slow runner that alone ran past the
      // test's minute and starved the worker beside it.
      for (let x = 20; x < w - 20 && Object.keys(found).length < 3; x += 3) {
        for (let y = 20; y < h - 20; y += 3) {
          const t = g.target(x, y);
          if (t && t.kind === "spot" && found[t.spot.dim] === undefined) {
            found[t.spot.dim] = [x, y];
          }
        }
      }
      return found;
    },
    [box.width, box.height],
  )) as Record<string, [number, number]>;
  expect(
    Object.keys(each).map(Number).sort(),
    "a corner, a curve and a surface are all there to be pointed at",
  ).toEqual([0, 1, 2]);

  for (const [dim, at] of Object.entries(each)) {
    await page.mouse.move(box.x + at[0], box.y + at[1]);
    await settle();
    const { hovered } = await ringed(page);
    expect(hovered, `something is ringed at the ${dim}-dimensional probe`)
      .not.toBeNull();
    await page.mouse.click(box.x + at[0], box.y + at[1]);
    await settle();
    expect(
      await page.evaluate(() => (window as any).__gecko.geomSelection()),
      `a click takes the ${dim}-dimensional cell that was ringed`,
    ).toEqual([hovered!.spot]);
    await page.mouse.click(box.x + at[0], box.y + at[1]);
    await settle();
  }
  expect(errors).toEqual([]);
});

test("no halo on what the pointer cannot reach, or when it is turned off", async ({
  page,
}) => {
  // The halo is drawn from the pick, so everything the pick already refuses
  // it refuses too — nothing behind an opaque model, which is the rule the
  // picking itself was taught. Said out loud here because it is the kind of
  // thing that comes back: a halo lighting up over a corner you cannot see
  // is an offer the click would not keep.
  const errors = await boot(page);
  await openDrawer(page);
  await openSection(page, "cells");
  await openGroup(page, "faces");
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());
  await page.evaluate(() => (window as any).__gecko.snapTo("+x"));
  await settle();

  const box = (await page.locator("#view").boundingBox())!;
  /** Which of the cube's corners the halo will ring, by the side they are
   *  on: squarely down one axis the near face sits between the camera and
   *  all four far corners. */
  const ringable = async () => {
    const out = { near: [] as number[], far: [] as number[] };
    const corners = (await page.evaluate(() => {
      const g = (window as any).__gecko;
      return (g.names() as number[]).map((name) => ({
        name,
        at: g.nodeScreen(name) as [number, number] | null,
        x: (g.nodePosition(name) as [number, number, number])[0],
      }));
    })) as { name: number; at: [number, number] | null; x: number }[];
    for (const corner of corners) {
      if (!corner.at) continue;
      await page.mouse.move(box.x + corner.at[0], box.y + corner.at[1]);
      await settle();
      const { hovered } = await ringed(page);
      if (hovered?.kind === "node" && hovered.name === corner.name) {
        (corner.x > 0.5 ? out.near : out.far).push(corner.name);
      }
    }
    return out;
  };

  // Transparent: the far side is on the screen, so it is fair game.
  await page.locator("[data-test=faces-alpha]").fill("0.35");
  await settle();
  const through = await ringable();
  expect(through.far, "a shell you can see through rings through").toHaveLength(
    4,
  );

  // Opaque: it is not on the screen any more, and the halo says so by not
  // being there.
  await page.locator("[data-test=faces-alpha]").fill("1");
  await settle();
  const solid = await ringable();
  expect(solid.far, "nothing behind a solid model is ringed").toEqual([]);
  expect(
    solid.near,
    "and the corners on the near side still are",
  ).toHaveLength(4);

  // Turned off: no ring anywhere, and the readout back to the mode alone.
  // Taken away at once rather than at the next movement.
  await page.locator("[data-test=faces-alpha]").fill("0.35");
  await settle();
  const at = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return g.nodeScreen((g.names() as number[])[0]);
  })) as [number, number];
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await settle();
  expect((await ringed(page)).halo).toMatchObject({ marks: 1, sides: 0 });

  await openSection(page, "colors");
  await page.locator("[data-test=show-halo]").uncheck();
  await settle();
  const off = await ringed(page);
  expect(off.hovered).toBeNull();
  expect(off.halo).toMatchObject({ marks: 0, sides: 0 });
  expect(off.aim).toBe("picking blocks");
  expect(errors).toEqual([]);
});

test("laying a quadrangle rings the corner the next click would take", async ({
  page,
}) => {
  // Four clicks, and each one may land on a point of the geometry, on one of
  // its curves, or on a handle of a quadrangle already laid — which is how
  // one comes to be welded to the next. Which of those the click would take
  // is the whole question, and laying asks it differently from a plain
  // pick: it prefers the handle that is **on** the geometry to the one a
  // fraction of a pixel in front of it. So the ring is drawn from laying's
  // own answer, and this is what says it is.
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());
  await settle();

  const box = (await page.locator("#view").boundingBox())!;
  const corners: [number, number][] = [
    [box.width * 0.4, box.height * 0.4],
    [box.width * 0.6, box.height * 0.4],
    [box.width * 0.6, box.height * 0.6],
    [box.width * 0.4, box.height * 0.6],
  ];

  // One quadrangle first, so there are handles to reuse.
  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  for (const at of corners) {
    await page.mouse.click(box.x + at[0], box.y + at[1]);
  }
  await settle();
  await expect(page.locator("#status")).toContainText("laid on");
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 4,
  );

  // Laying again: now a handle of the quadrangle just laid is a corner to
  // take, and the ring says which one.
  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  await expect(page.locator("#status")).toContainText("four corners");

  const node = (
    await page.evaluate(() => (window as any).__gecko.names())
  )[0] as number;
  const at = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    node,
  )) as [number, number];
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await settle();
  const onHandle = await ringed(page);
  expect(
    onHandle.hovered,
    "a handle to reuse is ringed, not the surface behind it",
  ).toEqual({ kind: "node", name: node });
  expect(onHandle.halo).toMatchObject({ marks: 1, sides: 0 });

  // And away from every handle, the geometry answers — which is what the
  // click would take there. Found rather than guessed: where the square
  // falls on the screen is the camera's business, and a coordinate written
  // down here would be a test that passes until the framing changes.
  const empty = (await page.evaluate(([w, h]) => {
    const g = (window as any).__gecko;
    for (let u = 0.2; u <= 0.8; u += 0.05) {
      for (let v = 0.2; v <= 0.8; v += 0.05) {
        const x = w * u;
        const y = h * v;
        if (g.pickHandle(x, y) !== null) continue;
        if (g.pickGeometry(x, y) === null) continue;
        return [x, y];
      }
    }
    return null;
  }, [box.width, box.height])) as [number, number] | null;
  expect(empty, "the square has a spot with no handle on it").not.toBeNull();
  await page.mouse.move(box.x + empty![0], box.y + empty![1]);
  await settle();
  const onGeometry = await ringed(page);
  expect(onGeometry.hovered).toMatchObject({ kind: "spot" });
  expect(onGeometry.aim).toContain("picking geometry — ");

  // Given up, so the test leaves the page in the mode it found it in.
  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  await expect(page.locator("#status")).toContainText("given up");
  expect(errors).toEqual([]);
});

test("the halo's size and color are settings, and changing one keeps it alive", async ({
  page,
}) => {
  // Two controls, and one rule underneath them that is worth more than
  // either: a preference change **tunes** the halo on screen rather than
  // making a new one. Rebuilding it there threw a one-color line away,
  // which takes down the colors texture every such line shares, and the
  // halo was then gone for the rest of the session — from one drag of any
  // slider, including sliders that have nothing to do with it.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  await page.evaluate(() => (window as any).__gecko.snapTo("+z"));
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());
  await settle();

  // The panel first, and open for the whole test: opening it takes the
  // pointer off the canvas, which puts the halo out on the way — and the
  // halo is the thing being watched.
  await openParams(page);
  await settle();

  // Squarely down an axis, the middle of the window is the near face.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await settle();
  const lit = await ringed(page);
  expect(lit.hovered).toMatchObject({ kind: "face" });
  expect(lit.halo.sides).toBe(4);
  const built = lit.halo.builds as number;

  // `fill` rather than a drag: it sets the slider without taking the
  // pointer anywhere, so the halo under it is undisturbed.
  await page.locator("[data-test=halo-size]").fill("36");
  await settle();
  await expect(page.locator("[data-test=halo-size-value]")).toHaveText("36px");
  expect(
    await page.evaluate(() => (window as any).__gecko.prefs().halo.size),
  ).toBe(36);

  // **The same halo**, wider. Not a new one — that is the whole claim.
  const wider = await ringed(page);
  expect(wider.hovered).toEqual(lit.hovered);
  expect(wider.halo.sides).toBe(4);
  expect(
    wider.halo.builds,
    "a size change tunes the halo it has rather than making another",
  ).toBe(built);

  // A slider that is nothing to do with the halo, which is how this was
  // found: the handles slider used to take the glow with it.
  await page.locator("[data-test=node-size]").fill("14");
  await settle();
  const after = await ringed(page);
  expect(after.hovered).toEqual(lit.hovered);
  expect(after.halo.builds).toBe(built);

  // And the color, which is themed like every other: one swatch, light and
  // dark, and it reaches the halo already on screen.
  await page.evaluate(() => {
    const g = (window as any).__gecko;
    g.setTheme({ ...g.theme(), halo: "#ff9500" });
  });
  await settle();
  expect(await page.evaluate(() => (window as any).__gecko.theme().halo)).toBe(
    "#ff9500",
  );
  const painted = await ringed(page);
  expect(painted.halo.builds).toBe(built);
  expect(painted.hovered).toEqual(lit.hovered);
  expect(errors).toEqual([]);
});

/** The same square, a hundred times across. Only the coordinates change,
 *  so anything that behaves differently on the two is behaving differently
 *  because of the model's size — which is the thing under test. */
const BIG_SQUARE_MSH = SQUARE_MSH.replace(
  /^(\d+) (-?[\d.]+) (-?[\d.]+) (-?[\d.]+)$/gm,
  (_, n, x, y, z) =>
    `${n} ${Number(x) * 100} ${Number(y) * 100} ${Number(z) * 100}`,
);

test("a curve is as easy to point at on a big model as on a small one", async ({
  page,
}) => {
  // Classifying is done onto curves, so a curve has to be reachable with a
  // pointer. Babylon gives a line mesh a pick tolerance of a tenth of a
  // **world unit**, which is not a tolerance at all: on a square one unit
  // across it swallowed the surface behind it, and on a real model a
  // hundred units across it was thinner than the line drawn for it — a
  // curve you can see and cannot hit. In pixels, like every other target
  // in this application, it is the same either way.
  const errors = await boot(page);
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());

  /** How many pixels of a coarse grid reach a curve. */
  const reachable = async () => {
    const box = (await page.locator("#view").boundingBox())!;
    return page.evaluate(
      ([w, h]) => {
        const g = (window as any).__gecko;
        let seen = 0;
        // Coarse on purpose: a curve is a target seven pixels across, so
        // a six-pixel pitch still lands on every one of them, at a quarter
        // of the picks a finer grid costs.
        for (let x = 20; x < w - 20; x += 6) {
          for (let y = 20; y < h - 20; y += 6) {
            const t = g.target(x, y);
            if (t && t.kind === "spot" && t.spot.dim === 1) seen += 1;
          }
        }
        return seen;
      },
      [box.width, box.height],
    );
  };

  const count = async (msh: string) => {
    await page.locator("#msh").setInputFiles({
      name: "square.msh",
      mimeType: "text/plain",
      buffer: Buffer.from(msh),
    });
    await expect(page.locator("[data-test=geom-curves]")).toHaveText("4");
    await page.evaluate(() => (window as any).__gecko.run("recenter"));
    await settle();
    if (
      (await page.evaluate(() => (window as any).__gecko.picking())) !==
      "geometry"
    ) {
      await page.evaluate(() => (window as any).__gecko.run("picking"));
      await settle();
    }
    return reachable();
  };

  const small = await count(SQUARE_MSH);
  const big = await count(BIG_SQUARE_MSH);
  expect(small, "the curves of a small model can be pointed at").toBeGreaterThan(
    100,
  );
  expect(big, "and so can the curves of a model a hundred times across").toBeGreaterThan(
    100,
  );
  // Framed the same way, the two pictures are the same picture, so the same
  // pixels reach a curve in both. Loose, because the framing of a square is
  // not identical to the pixel — the claim is that the model's size does
  // not decide it.
  expect(Math.abs(big - small) / small).toBeLessThan(0.25);
  expect(errors).toEqual([]);
});

test("laying a quadrangle does not put out every plain-colored line", async ({
  page,
}) => {
  // Babylon gives every one-color greased line the same one-pixel colors
  // texture, and its material's `dispose` disposes that shared texture
  // without letting go of it. Laying a quadrangle throws its provisional
  // side away on every movement of the pointer, so after `⇧Q` nothing
  // plain-colored was drawn again: not the halo, not even the sides of the
  // quadrangle being laid. This is the same upstream behaviour that made a
  // halo light up once and never again, and a slider put it out for good.
  const errors = await boot(page);
  await openSquare(page);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");
  await page.evaluate(() => (window as any).__gecko.run("recenter"));
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());
  await settle();
  const box = (await page.locator("#view").boundingBox())!;
  // Aimed at the geometry, which is what a curve answers to.
  await page.evaluate(() => (window as any).__gecko.run("picking"));
  await settle();

  /** A pixel on a curve, found rather than guessed. */
  const curve = (await page.evaluate(
    ([w, h]) => {
      const g = (window as any).__gecko;
      for (let x = 20; x < w - 20; x += 2)
        for (let y = 20; y < h - 20; y += 2) {
          const t = g.target(x, y);
          if (t && t.kind === "spot" && t.spot.dim === 1) return [x, y];
        }
      return null;
    },
    [box.width, box.height],
  )) as [number, number] | null;

  // Lay a quadrangle the way a hand does: the pointer travels between the
  // clicks, and every step of it remakes the provisional side.
  await page.evaluate(() => (window as any).__gecko.run("lay-face"));
  const at: [number, number][] = [
    [box.width * 0.4, box.height * 0.4],
    [box.width * 0.6, box.height * 0.4],
    [box.width * 0.6, box.height * 0.6],
    [box.width * 0.4, box.height * 0.6],
  ];
  for (const [x, y] of at) {
    for (let i = 1; i <= 4; i++) {
      await page.mouse.move(box.x + x - 12 + i * 3, box.y + y);
    }
    await page.mouse.click(box.x + x, box.y + y);
  }
  await settle();
  await expect(page.locator("#status")).toContainText("laid on");

  // Then a curve. Laying hands the mouse back to the mode it found, which
  // was the geometry.
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe(
    "geometry",
  );
  expect(curve, "the square has a curve to point at").not.toBeNull();
  await page.mouse.move(box.x + curve![0], box.y + curve![1]);
  await settle();
  const lit = await ringed(page);
  expect(lit.hovered).toMatchObject({ kind: "spot", spot: { dim: 1 } });
  expect(lit.halo.sides).toBeGreaterThan(0);
  expect(
    lit.halo.textured,
    "the halo's color texture outlived the provisional sides thrown away",
  ).toBe(true);
  expect(errors).toEqual([]);
});

test("the halo stays on a handle while it is dragged", async ({ page }) => {
  // The drag moves the handle and tells the kernel only at the drop. The
  // halo was placed once, when the pointer arrived, so it stayed where the
  // node **was** until it was let go — a light around an empty spot, with
  // the thing it was about somewhere else.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());
  await settle();
  const name = (
    await page.evaluate(() => (window as any).__gecko.names())
  )[4] as number;
  const at = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    name,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;

  /** How far the middle of the halo is from where the node is now. */
  const apart = () =>
    page.evaluate((n) => {
      const g = (window as any).__gecko;
      const ring = g.haloBox() as { min: number[]; max: number[] } | null;
      if (!ring) return null;
      const center = g.center() as number[];
      const node = g.nodePosition(n) as number[];
      return Math.hypot(
        ...[0, 1, 2].map(
          (k) => (ring.min[k] + ring.max[k]) / 2 + center[k] - node[k],
        ),
      );
    }, name);

  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await settle();
  expect(await page.evaluate(() => (window as any).__gecko.hovered())).toEqual({
    kind: "node",
    name,
  });
  expect(await apart(), "the halo starts on the handle").toBeLessThan(1e-3);
  const start = await page.evaluate(
    (n) => (window as any).__gecko.nodePosition(n),
    name,
  );

  // Pressed and moving, and **not yet let go**.
  await page.mouse.down();
  for (let i = 1; i <= 8; i++) {
    await page.mouse.move(box.x + at[0] + i * 10, box.y + at[1] - i * 6);
  }
  await settle();
  const moved = await page.evaluate(
    (n) => (window as any).__gecko.nodePosition(n),
    name,
  );
  expect(moved, "the handle is under way").not.toEqual(start);
  expect(
    await apart(),
    "and the halo is on it, not where it started",
  ).toBeLessThan(1e-3);

  await page.mouse.up();
  await page.waitForFunction(() =>
    (window as any).__gecko.message().includes("moved"),
  );
  expect(errors).toEqual([]);
});

test("the halo blooms only while the picture is still", async ({ page }) => {
  // The bloom is a blurred pass every frame it is on, and it took a cube
  // from 52 frames a second to 21 — felt exactly when the view is moving,
  // and when nobody is reading a halo. So it waits for stillness; the ball
  // and the thread stay drawn throughout.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(
    () => (window as any).__gecko.counts().nodes === 8,
  );
  const settle = () => page.evaluate(() => (window as any).__gecko.settled());
  await settle();
  const box = (await page.locator("#view").boundingBox())!;
  /** The halo's state, two frames on — after the per-frame decision. */
  const halo = () =>
    page.evaluate(
      () =>
        new Promise((done) =>
          requestAnimationFrame(() =>
            requestAnimationFrame(() => done((window as any).__gecko.halo())),
          ),
        ),
    ) as Promise<{ sides: number; glowing: boolean }>;

  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await settle();
  await page.waitForTimeout(400);
  const still = await halo();
  expect(still.sides, "a halo is on the face").toBe(4);
  expect(still.glowing, "and, the view being still, it blooms").toBe(true);

  // The wheel moves the camera: the light goes, the halo stays.
  await page.mouse.wheel(0, -240);
  const moving = await halo();
  expect(moving.sides, "the halo is still drawn").toBe(4);
  expect(moving.glowing, "but not lit while the view moves").toBe(false);

  // And back once the view has come to rest. Polled rather than waited
  // for: the wheel's zoom glides on for most of a second after the last
  // notch, and the camera's own "settled" does not wait for that glide.
  await expect
    .poll(async () => (await halo()).glowing, {
      message: "lit again once it is still",
      timeout: 5000,
    })
    .toBe(true);
  expect(errors).toEqual([]);
});
