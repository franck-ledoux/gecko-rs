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
async function unfold(page: import("@playwright/test").Page, id: string, handle = "> summary") {
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
  // blocks, the geometry's corners, curves and surfaces, the mesh, and the
  // three that say what a cell follows. Being frozen is not among them —
  // that is a shape, not a colour.
  await expect(page.locator("#swatches input[type=color]")).toHaveCount(12);
  expect(await page.evaluate(() => (window as any).__gecko.theme())).toMatchObject({
    background: "#15171b",
  });

  await openSection(page, "colours");
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
  await openSection(page, "colours");
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
  await openGroup(page, "faces");
  await page.locator("[data-test=faces-id]").check();
  expect(await surface("faces")).toMatchObject({ quads: 6, colours: 6 });

  // Both at once, each with its own colouring and its own transparency.
  await page.locator("[data-test=blocks-visible]").check();
  await openGroup(page, "blocks");
  await page.locator("[data-test=blocks-uniform]").check();
  expect(await surface("blocks")).toMatchObject({ quads: 6, colours: 1 });
  expect(await surface("faces")).toMatchObject({ colours: 6 });

  await openGroup(page, "faces");
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
  await openGroup(page, "blocks");
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
  await openGroup(page, "blocks");
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
  await page.locator("[data-test=faces-id]").check();
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
  await openGroup(page, "faces");
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

  // Both picked, and the panel says what gluing them would do.
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(2);
  await expect(page.locator("#status")).toContainText("will be welded where they meet");
  await expect(page.locator("#glue")).toBeEnabled();

  await page.locator("#glue").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

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
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);
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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);

  /**
   * The distinct x every **node** sits at.
   *
   * The nodes, not the quadrangles: faces are drawn shrunk towards their own
   * centres, so no corner of the drawing is where a node is.
   */
  const columns = () =>
    page.evaluate(() => {
      const g = (window as any).__gecko;
      const seen = new Set<number>();
      for (const name of g.names()) seen.add(Math.round(g.nodePosition(name)[0] * 1e4) / 1e4);
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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

  // One column where there were two, half way between them: the faces met
  // in the middle, and neither block was dragged onto the other.
  expect(await columns()).toEqual([0, 1.25, 2.5]);
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
  await expect(page.locator("#display")).toHaveAttribute("aria-expanded", "true");

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
  await openGroup(page, "faces");
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
  await openGroup(page, "faces");
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

test("everything on screen is drawn against one origin, even when it moves", async ({ page }) => {
  // The scene is sent rebased on a single origin so that f32 coordinates
  // stay small, and that origin **moves** when the model walks away from
  // it. Everything then has to be re-sent against the new one: the
  // geometry's own buffer has not changed, so nothing but this makes the
  // page fetch it again, and a geometry still rebased on where the middle
  // of the scene used to be is drawn an origin-shift away from the blocks.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

  // A geometry a thousand kilometres away from that block: only its node
  // coordinates move, so it is the same square and still one surface.
  const far = SQUARE_MSH.split("$EndNodes")
    .map((part, at) =>
      at === 0
        ? part.replace(/^([1-4]) (\S+)/gm, (_, id: string, x: string) => `${id} ${Number(x) + 1e6}`)
        : part,
    )
    .join("$EndNodes");
  await openSquare(page, far);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveText("1");

  // Both on the session's origin, wherever its hysteresis has left it.
  const between = (await page.evaluate(() => (window as any).__gecko.origins())) as {
    view: number[];
    geometry: number[] | null;
  };
  expect(between.geometry).toEqual(between.view);

  // Something picked on the geometry, to be looked for again afterwards: a
  // re-centring fetches the geometry again, and a selection dropped over a
  // change of coordinates is a click lost for no reason anybody can see.
  await page.evaluate(() => (window as any).__gecko.selectGeom([{ dim: 2, tag: 1 }]));

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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 0);

  const after = (await page.evaluate(() => (window as any).__gecko.origins())) as {
    view: number[];
    geometry: number[] | null;
  };
  expect(after.view[0]).toBeGreaterThan(999_999);
  expect(after.geometry, "the geometry is drawn against a stale origin").toEqual(after.view);
  expect(
    await page.evaluate(() => (window as any).__gecko.geomSelection()),
    "the selection survived the re-centring",
  ).toEqual([{ dim: 2, tag: 1 }]);
  expect(errors).toEqual([]);
});

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
  await openGroup(page, "curves");
  await page.locator('[data-test="curves-uniform"]').check();
  const uniform = await look("curves", 11);
  expect((await look("curves", 12))!.colour).toBe(uniform!.colour);
  await page.locator('[data-test="curves-id"]').check();
  expect((await look("curves", 12))!.colour).not.toBe((await look("curves", 11))!.colour);

  // And opacity, per group.
  await openGroup(page, "surfaces");
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
  for (const id of ["picking", "box"]) {
    await expect(page.locator(`#${id}`)).toBeDisabled();
  }
  await openCube(page);
  for (const id of ["picking", "box"]) {
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
  // Over a face of the block, which sits **inside** the geometry: the ray
  // there meets a face of the cube and a surface of the model both, which
  // is what lets one point mean two things in the two modes. Not the middle
  // of the view — this geometry is three units deep and the block sits in a
  // corner of it, so the middle of the view is thin air.
  //
  // And not the face's own centre either: a click asks for a node, then an
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
        const p: [number, number] = [middle[0] + dx * away, middle[1] + dy * away];
        const free = g.pickNode(p[0], p[1]) === null && g.pickEdge(p[0], p[1]) === null;
        if (free && g.pickSurface(p[0], p[1])?.face === face) return p;
      }
    }
    return null;
  })) as [number, number] | null;
  expect(at, "somewhere on a face of the block that a click would take").not.toBeNull();
  const middle: [number, number] = [box.x + at![0], box.y + at![1]];

  // Blocks first: the click takes a face of the cube, not the geometry
  // around it.
  await page.mouse.click(...middle);
  await page.waitForFunction(() => (window as any).__gecko.selection().length === 1);
  expect(await page.evaluate(() => (window as any).__gecko.surfaceSelection())).toEqual([]);

  // Which mode you are in is said twice over, because a toggle that looks
  // the same either way is one you click to find out about.
  await expect(page.locator("[data-test=aim]")).toHaveText("picking blocks");
  await expect(page.locator("#picking .as-blocks")).toBeVisible();
  await expect(page.locator("#picking .as-geometry")).toBeHidden();

  // Switched over, and the same click takes a surface instead.
  await page.keyboard.press("m");
  await expect(page.locator("[data-test=aim]")).toHaveText("picking geometry");
  await expect(page.locator("#picking .as-geometry")).toBeVisible();
  await expect(page.locator("#picking .as-blocks")).toBeHidden();
  expect(await page.evaluate(() => (window as any).__gecko.picking())).toBe("geometry");
  await expect(page.locator("#picking")).toHaveAttribute("aria-pressed", "true");
  // The face stays picked through the change of mode: a handful of cells
  // and the entity to put them on are chosen in different modes and used
  // together. `Esc` is how you let go.
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(1);

  await page.mouse.click(...middle);
  await page.waitForFunction(() => (window as any).__gecko.surfaceSelection().length === 1);
  const [tag] = (await page.evaluate(() =>
    (window as any).__gecko.surfaceSelection(),
  )) as number[];
  await expect(page.locator("#status")).toContainText(`surface ${tag} selected`);
  // And it is drawn in the selection colour, like a picked face.
  expect(
    (await page.evaluate((t) => (window as any).__gecko.geomLook("surfaces", t), tag)) as {
      colour: string;
    },
  ).toMatchObject({ colour: "#ffcc33" });
  // Nothing of the blocking answered the click: the face selection is the
  // one from before the change of mode, not a second one.
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(1);

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

test("a box that cannot be built says why and builds nothing", async ({ page }) => {
  const errors = await boot(page);
  await openCube(page);
  await page.keyboard.press("m");
  await page.evaluate(() => (window as any).__gecko.selectGeom([{ dim: 2, tag: 1 }]));
  await page.keyboard.press("b");
  await expect(page.locator("#status")).toContainText("flat");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  expect(errors).toEqual([]);
});

test("each group of cells is its own box, not nested inside the last", async ({ page }) => {
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
    (window as any).__gecko.apply({ op: "box_geometry", surfaces: [], margin: 0 }),
  );
  await page.evaluate(() => (window as any).__gecko.refresh());
  await page.evaluate(() => (window as any).__gecko.settled());
}

test("the blocking is put on the geometry and wears what it follows", async ({ page }) => {
  const errors = await boot(page);
  await boxedAndPinned(page);
  await expect(page.locator("[data-test=pinned]")).toHaveText("0");

  await page.keyboard.press("k");
  await expect(page.locator("#status")).toContainText("8 nodes, 12 edges and 6 faces");
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");

  // Every corner of the block is on a corner of the model, and wears its
  // colour rather than the wireframe's.
  const state = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    return g.names().map((n: number) => g.nodeState(n));
  })) as { on: { dim: number; tag: number } | null; frozen: boolean }[];
  expect(state.every((s) => s.on?.dim === 0)).toBe(true);
  expect(state.every((s) => !s.frozen)).toBe(true);
  expect(errors).toEqual([]);
});

test("a frozen node is a cube, and still wears what the colouring says", async ({ page }) => {
  // Freezing used to paint the handle grey, which hid the very thing the
  // colouring is for. Now it changes the handle's shape, and the colour is
  // left to say what the node is on.
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.keyboard.press("k");
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");

  const name = (await page.evaluate(() => (window as any).__gecko.names()[0])) as number;
  const other = (await page.evaluate(() => (window as any).__gecko.names()[1])) as number;
  const shape = (n: number) =>
    page.evaluate((who) => (window as any).__gecko.nodeShape(who), n);
  const tint = (n: number) => page.evaluate((who) => (window as any).__gecko.nodeTint(who), n);

  // Every corner of the block is on a corner of the model, so under the
  // `follows` colouring every handle wears the corner colour — set from
  // the drawer, the way somebody would.
  await openDrawer(page);
  await openGroup(page, "nodes");
  await page.locator('[data-test="nodes-class"]').check();
  const theme = (await page.evaluate(() => (window as any).__gecko.theme())) as {
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
  await page.waitForFunction((n) => (window as any).__gecko.nodeState(n).frozen, name);

  // A cube where it was a ball, and the colour unchanged — while the node
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
  // a press and release on a handle, which the drag behaviour has already
  // decided was not a drag.
  const name = (await page.evaluate(() => (window as any).__gecko.names()[0])) as number;
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
  await page.waitForFunction((n) => (window as any).__gecko.nodeState(n).frozen, name);
  await page.keyboard.press("l");
  await page.waitForFunction((n) => !(window as any).__gecko.nodeState(n).frozen, name);

  // And off the model, which is something you say rather than something
  // that happens to you by dragging.
  await page.keyboard.press("u");
  await page.waitForFunction((n) => (window as any).__gecko.nodeState(n).on === null, name);
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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.evaluate(() => (window as any).__gecko.settled());

  expect(await page.evaluate(() => (window as any).__gecko.split())).toBe(false);
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
  expect(await page.evaluate(() => (window as any).__gecko.paneCamera().ortho)).toBe(false);
  await page.mouse.move(box.x + box.width - 20, box.y + 20);
  expect(await page.evaluate(() => (window as any).__gecko.paneCamera().ortho)).toBe(true);

  // Remembered, like every other thing about how it looks.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(await page.evaluate(() => (window as any).__gecko.split())).toBe(true);
  expect(errors).toEqual([]);
});

test("dragging a node in an axis view keeps it in the plane of the screen", async ({ page }) => {
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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recentre"));
  await page.evaluate(() => (window as any).__gecko.settled());

  const box = (await page.locator("#view").boundingBox())!;
  // Into the Z pane — the bottom right — which looks down Z, so Z is what
  // must not move.
  await page.mouse.move(box.x + box.width * 0.75, box.y + box.height * 0.75);
  expect(await page.evaluate(() => (window as any).__gecko.pane())).toBe("z");
  expect(await page.evaluate(() => (window as any).__gecko.dragPlane())).toEqual([0, 0, 1]);
  // And in the 3-D pane there is no plane to name: it faces the camera,
  // wherever that is.
  await page.mouse.move(box.x + box.width * 0.25, box.y + box.height * 0.3);
  expect(await page.evaluate(() => (window as any).__gecko.dragPlane())).toBeNull();
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

test("a face is picked in an axis view, through that view's own camera", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recentre"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // The middle of the Y pane, which is looking straight at the model.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + box.width * 0.25, box.y + box.height * 0.75);
  await page.waitForFunction(() => (window as any).__gecko.selection().length === 1);
  expect(errors).toEqual([]);
});

test("the help card says what the classification colours mean", async ({ page }) => {
  // A colour code nobody can look up is a colour code nobody reads. It is
  // generated from the theme, so it cannot come to disagree with the
  // screen — the same reason the keys beside it are generated from the
  // table they are matched against.
  const errors = await boot(page);
  await page.keyboard.press("h");
  await expect(page.locator("#help-legend .key")).toHaveCount(4);
  await expect(page.locator("#help-legend")).toContainText("on a corner");
  await expect(page.locator("#help-legend")).toContainText("on a curve");
  await expect(page.locator("#help-legend")).toContainText("on a surface");
  // And the one thing about a node that is **not** a colour is said in
  // words, beside them.
  await expect(page.locator("[data-test=legend-note]")).toContainText("frozen");
  await expect(page.locator("[data-test=legend-note]")).toContainText("cube");

  // The swatch really is the colour the scene uses.
  const first = await page
    .locator("#help-legend .key i")
    .first()
    .evaluate((e) => (e as HTMLElement).style.background);
  const theme = (await page.evaluate(() => (window as any).__gecko.theme())) as {
    onCorner: string;
  };
  // The browser hands a style back as `rgb(…)` whatever it was set with.
  const rgb = (hex: string) =>
    `rgb(${[1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16)).join(",")})`;
  expect(first.replace(/\s/g, "")).toBe(rgb(theme.onCorner));
  expect(errors).toEqual([]);
});

test("nodes, edges and faces each take three colourings", async ({ page }) => {
  const errors = await boot(page);
  await openCube(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await openDrawer(page);

  // Three for each of the three, and only two for blocks — a block is
  // classified on nothing, a surface mesh having no volumes to put it on.
  for (const group of ["nodes", "edges", "faces"]) {
    for (const mode of ["uniform", "id", "class"]) {
      await expect(page.locator(`[data-test="${group}-${mode}"]`)).toHaveCount(1);
    }
  }
  await expect(page.locator('[data-test="blocks-class"]')).toHaveCount(0);

  // And choosing one is remembered, like every other thing about how it
  // looks.
  await openGroup(page, "nodes");
  await page.locator('[data-test="nodes-class"]').check();
  expect(await page.evaluate(() => (window as any).__gecko.prefs().nodes.colouring)).toBe("class");
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(await page.evaluate(() => (window as any).__gecko.prefs().nodes.colouring)).toBe("class");
  expect(errors).toEqual([]);
});

test("an edge is picked and its sheet cut from an axis view", async ({ page }) => {
  // Everything works in every pane or the split is half a feature. Each
  // pick goes through the camera of the pane the pointer is in; it was
  // the edges that were still asking the 3-D one, so clicking a line in
  // a side view did nothing at all.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recentre"));
  await page.evaluate(() => (window as any).__gecko.settled());

  // Into the Y pane, bottom left, and ask it where an edge is.
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.move(box.x + box.width * 0.25, box.y + box.height * 0.75);
  expect(await page.evaluate(() => (window as any).__gecko.pane())).toBe("y");

  const edge = (await page.evaluate(() => edgeNear([0.5, 0, 0]))) as number;
  const at = (await page.evaluate((e) => (window as any).__gecko.edgeScreen(e), edge)) as [
    number,
    number,
  ];
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(() => (window as any).__gecko.sheet().length === 4);

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

test("a change can be taken back, and the button says when there is one", async ({ page }) => {
  const errors = await boot(page);
  // Nothing has happened, so there is nothing to take back.
  await expect(page.locator("#undo")).toBeDisabled();

  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await expect(page.locator("#undo")).toBeEnabled();

  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);

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

test("a cut is taken back whole, and the names come back with it", async ({ page }) => {
  // The point of keeping states rather than replaying commands: the cells
  // answer to the same numbers afterwards. A model rebuilt from a
  // description of itself would renumber them, and every name this page is
  // holding — the lit sheet, the selected node — would come to mean
  // something else.
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

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
  await page.evaluate((e) => (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }), edge);
  await page.evaluate(() => (window as any).__gecko.refresh());
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");

  await page.keyboard.press("ControlOrMeta+z");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  expect(await dump()).toEqual(before);

  // And the same cut again, on the same edge, which still names it.
  await page.evaluate((e) => (window as any).__gecko.apply({ op: "split_sheet", edge: e, at: 0.5 }), edge);
  await page.evaluate(() => (window as any).__gecko.refresh());
  await expect(page.locator("[data-test=blocks]")).toHaveText("2");
  expect(errors).toEqual([]);
});

test("how far back it goes is a setting, and it is remembered", async ({ page }) => {
  const errors = await boot(page);
  await openParams(page);
  await expect(page.locator("[data-test=history-steps]")).toHaveText("5");

  // One step, and then two changes: only the second can be taken back.
  await page.locator("[data-test=history]").fill("1");
  await page.locator("[data-test=history]").dispatchEvent("input");
  await expect(page.locator("[data-test=history-steps]")).toHaveText("1");
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);

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
  expect(await open("sec-colours")).toBe(false);
  expect(await open("grp-faces")).toBe(false);

  // It fits: no scrolling, which is the whole complaint this answers.
  const fits = await page
    .locator("#drawer")
    .evaluate((d) => d.scrollHeight <= d.clientHeight + 1);
  expect(fits, "the folded drawer still scrolls").toBe(true);

  // A folded group still says what it is: what you opened it to find out.
  // Faces start on their classification, so that is what it says.
  await expect(page.locator("#grp-faces .digest")).toHaveText("classification · 35%");
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

test("the two panels are one at a time, and the cog has its own key", async ({ page }) => {
  // They are drawn in the same place: two sheets of glass on top of each
  // other is a bug that looks like a rendering fault.
  const errors = await boot(page);
  await page.locator("#display").click();
  await expect(page.locator("#drawer")).toBeVisible();

  await page.locator("#settings").click();
  await expect(page.locator("#params")).toBeVisible();
  await expect(page.locator("#drawer")).toBeHidden();
  await expect(page.locator("#settings")).toHaveAttribute("aria-expanded", "true");

  await page.keyboard.press("Tab");
  await expect(page.locator("#drawer")).toBeVisible();
  await expect(page.locator("#params")).toBeHidden();

  // And the comma is the cog, as it is everywhere else.
  await page.keyboard.press(",");
  await expect(page.locator("#params")).toBeVisible();
  await expect(page.locator("#drawer")).toBeHidden();
  expect(errors).toEqual([]);
});

test("a fresh window starts on the classification, with a box that hugs", async ({ page }) => {
  // What somebody sees the first time: putting the blocking onto the model
  // is what the session is for, so nodes, edges and faces say what they
  // are classified on before they say anything else. Blocks keep their own
  // colouring — a block is classified on nothing.
  const errors = await boot(page);
  const prefs = (await page.evaluate(() => (window as any).__gecko.prefs())) as {
    nodes: { colouring: string };
    edges: { colouring: string };
    faces: { colouring: string };
    blocks: { colouring: string };
    box: { margin: number };
  };
  expect(prefs.nodes.colouring).toBe("class");
  expect(prefs.edges.colouring).toBe("class");
  expect(prefs.faces.colouring).toBe("class");
  expect(prefs.blocks.colouring).toBe("id");
  // And a bounding box that hugs the model: two percent of each half-side.
  expect(prefs.box.margin).toBeCloseTo(0.02, 12);
  await openParams(page);
  await expect(page.locator("[data-test=box-margin-value]")).toHaveText("2%");
  expect(errors).toEqual([]);
});

test("how big the handles and the edges are drawn is a setting", async ({ page }) => {
  // Both in pixels on the screen rather than in model units: what they are
  // for is being easy to see and easy to hit, and that is a question about
  // the screen. A blocking with a lot in it wants thin, a projector wants
  // thick.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
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
  const narrow = (await page.evaluate(() => (window as any).__gecko.edgeRibbonWidth())) as number;
  await page.locator("[data-test=edge-width]").fill("6");
  await page.locator("[data-test=edge-width]").dispatchEvent("input");
  await expect(page.locator("[data-test=edge-width-value]")).toHaveText("6px");
  expect(await page.evaluate(() => (window as any).__gecko.prefs().edges.width)).toBe(6);
  const wide = (await page.evaluate(() => (window as any).__gecko.edgeRibbonWidth())) as number;
  // Drawn at it, not merely remembered. Measured as a ratio rather than in
  // pixels, so the assertion does not depend on how many device pixels the
  // runner's screen puts in one of the pointer's.
  expect(wide / narrow).toBeCloseTo(3, 1);

  // And both are remembered, like every other thing about how it looks.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(await page.evaluate(() => (window as any).__gecko.prefs().nodes.size)).toBe(20);
  expect(await page.evaluate(() => (window as any).__gecko.prefs().edges.width)).toBe(6);
  expect(errors).toEqual([]);
});

test("the box margin in the settings panel is the one the box is built to", async ({ page }) => {
  // The kernel's constant is what you get when nobody says; this is
  // somebody saying, and it has to reach the command.
  const errors = await boot(page);
  await openCube(page); // a 2 × 1 × 3 box of a geometry
  await openParams(page);

  /** How wide the blocking is, on each axis. Rebasing-proof: a difference
   *  of two positions does not care where the origin went. */
  const extent = async () => {
    const names = (await page.evaluate(() => (window as any).__gecko.names())) as number[];
    const at = await Promise.all(
      names.map((n) => page.evaluate((m) => (window as any).__gecko.nodePosition(m), n)),
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
    (n) => Math.abs(n.at[0] - 1) < 1e-6 && Math.abs(n.at[1]) < 1e-6 && Math.abs(n.at[2]) < 1e-6,
  )!;
  const middle: [number, number] = [(a.on[0] + b.on[0]) / 2, (a.on[1] + b.on[1]) / 2];

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
  const from: [number, number] = [box.x + Math.min(...xs) - pad, box.y + Math.min(...ys) - pad];
  const to: [number, number] = [box.x + Math.max(...xs) + pad, box.y + Math.max(...ys) + pad];

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
    ((await page.evaluate(() => (window as any).__gecko.selectedNodes())) as number[]).sort(),
  ).toEqual(bottom.map((n) => n.name).sort());
  await expect(page.locator("#status")).toContainText("4 nodes — 0 on the geometry");
  // Still not enough: there is nothing to put them on yet.
  await expect(page.locator("#attach")).toBeDisabled();

  // Curve 201 runs along y = z = 0. Picked through the viewer rather than
  // by aiming at it, which the test above does.
  await page.evaluate(() => (window as any).__gecko.selectGeom([{ dim: 1, tag: 201 }]));
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
  expect(await page.evaluate(() => (window as any).__gecko.selectedNodes())).toEqual([]);
  await expect(page.locator("#attach")).toBeDisabled();

  // One step back puts all four where they were.
  await page.keyboard.press("ControlOrMeta+z");
  await expect(page.locator("[data-test=pinned]")).toHaveText("0");
  expect(errors).toEqual([]);
});

test("freezing and unclassifying take the whole selection", async ({ page }) => {
  const errors = await boot(page);
  await boxedAndPinned(page);
  await page.keyboard.press("k");
  await expect(page.locator("[data-test=pinned]")).toHaveText("8");

  const names = (await page.evaluate(() => (window as any).__gecko.names())) as number[];
  await page.evaluate((n) => (window as any).__gecko.selectNodes(n), names.slice(0, 3));
  await page.evaluate(() => (window as any).__gecko.showVerbs());

  await page.keyboard.press("l");
  await expect(page.locator("#status")).toContainText("frozen");
  expect(
    await page.evaluate(() => (window as any).__gecko.stats().classified.frozen),
  ).toBe(3);

  // Thawed only when every one of them is frozen, so a mixed selection has
  // an obvious answer.
  await page.keyboard.press("l");
  expect(
    await page.evaluate(() => (window as any).__gecko.stats().classified.frozen),
  ).toBe(0);

  await page.keyboard.press("u");
  await expect(page.locator("[data-test=pinned]")).toHaveText("5");
  expect(errors).toEqual([]);
});

test("the band is opened with W as well, and works in an axis view", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.keyboard.press("v");
  await page.evaluate(() => (window as any).__gecko.run("recentre"));
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
  const pair = seen.filter((n) => Math.hypot(n.on[0] - first.on[0], n.on[1] - first.on[1]) < 3);
  expect(pair).toHaveLength(2);

  const pad = 10;
  await page.keyboard.down("w");
  await page.mouse.move(box.x + first.on[0] - pad, box.y + first.on[1] - pad);
  await page.mouse.down();
  await page.mouse.move(box.x + first.on[0] + pad, box.y + first.on[1] + pad, { steps: 4 });
  await expect(page.locator("#lasso")).toBeVisible();
  await page.mouse.up();
  await page.keyboard.up("w");

  expect(
    ((await page.evaluate(() => (window as any).__gecko.selectedNodes())) as number[]).sort(),
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
    ((await page.evaluate(() => (window as any).__gecko.selectedNodes())) as number[]).sort(),
  ).toEqual(inside.sort());
  expect(errors).toEqual([]);
});

test("dragging one picked handle takes the others with it", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
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
  await page.mouse.move(box.x + grab.on[0] + 100, box.y + grab.on[1], { steps: 8 });
  await page.mouse.up();
  await page.waitForFunction(() => (window as any).__gecko.message().includes("moved"));

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
    for (const k of [0, 1, 2]) expect(moved(node.name)[k]).toBeCloseTo(by[k], 5);
  }
  for (const node of before.filter((n) => !picked.some((p) => p.name === n.name))) {
    expect(Math.hypot(...moved(node.name))).toBeLessThan(1e-9);
  }

  // One gesture, one command, one step back.
  await expect(page.locator("#status")).toContainText("3 nodes moved");
  await page.keyboard.press("ControlOrMeta+z");
  await page.waitForFunction(() => (window as any).__gecko.message().includes("undone"));
  const back = await handles(page);
  for (const node of before) {
    const now = back.find((n) => n.name === node.name)!.at;
    for (const k of [0, 1, 2]) expect(now[k]).toBeCloseTo(node.at[k], 6);
  }
  expect(errors).toEqual([]);
});

test("dragging a handle that is not picked moves it alone", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
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
  await page.mouse.move(box.x + grab.on[0] + 80, box.y + grab.on[1], { steps: 8 });
  await page.mouse.up();
  await page.waitForFunction(() => (window as any).__gecko.message().includes("moved"));

  const after = await handles(page);
  for (const node of before) {
    const now = after.find((n) => n.name === node.name)!.at;
    const went = Math.hypot(...[0, 1, 2].map((k) => now[k] - node.at[k]));
    if (node.name === grab.name) expect(went).toBeGreaterThan(0.1);
    else expect(went).toBeLessThan(1e-9);
  }
  expect(errors).toEqual([]);
});

test("a blocking is discretised and the skin of it is drawn", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(0);

  // Three segments a side, set in the settings panel before asking.
  await openParams(page);
  await page.locator("[data-test=segments]").fill("3");
  await page.locator("[data-test=segments]").dispatchEvent("input");
  await expect(page.locator("[data-test=segments-value]")).toHaveText("3");

  await page.locator("#mesh").click();
  await expect(page.locator("#status")).toContainText("64 nodes, 27 hexahedra");
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("27");
  // Six faces of nine quadrangles: the skin, and nothing inside it.
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(54);

  // A mesh of a blocking you have since edited is a lie, and a lie that
  // looks right — so it goes when the blocking moves.
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(0);
  expect(errors).toEqual([]);
});

test("a script is opened like a geometry, and replayed from nothing", async ({ page }) => {
  // The same button: "open" is the same question whether the file is a
  // model to work on or a session to play back. And a script describes a
  // session from an empty one, so what is there goes first.
  const errors = await boot(page);

  // Something to throw away, so the reset is doing work.
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
  await expect(page.locator("[data-test=geom-surfaces]")).toHaveCount(0);

  // The console is where a script talks, so it is open and it said so.
  await expect(page.locator("#console")).toBeVisible();
  await expect(page.locator("[data-test=console-out]")).toContainText("emptied first");
  await expect(page.locator("[data-test=console-out]")).toContainText("replayed");
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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

  // Three nodes, picked through the same door a click goes through.
  const names = (await page.evaluate(() => (window as any).__gecko.names())) as number[];
  for (const n of names.slice(0, 3)) {
    await page.evaluate((who) => (window as any).__gecko.pickNodeByName(who), n);
  }
  expect(await page.evaluate(() => (window as any).__gecko.selectedNodes())).toEqual(
    names.slice(0, 3),
  );

  // Then a face, which is a different selection — so "the last one" is a
  // question nothing but the order of the clicks can answer.
  const face = (await page.evaluate(() => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    g.choose({ face: s.faces[0], block: s.blocks[0] });
    return s.faces[0];
  })) as number;
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toHaveLength(1);

  // ⇧Esc takes back the face, and leaves the three nodes alone.
  await page.keyboard.press("Shift+Escape");
  expect(await page.evaluate(() => (window as any).__gecko.selection())).toEqual([]);
  expect(await page.evaluate(() => (window as any).__gecko.selectedNodes())).toEqual(
    names.slice(0, 3),
  );
  void face;

  // Again, and it is the last node that goes — one at a time, newest first.
  await page.keyboard.press("Shift+Escape");
  expect(await page.evaluate(() => (window as any).__gecko.selectedNodes())).toEqual(
    names.slice(0, 2),
  );

  // Esc still lets go of everything.
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => (window as any).__gecko.selectedNodes())).toEqual([]);

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
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
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
  expect(await page.evaluate(() => (window as any).__gecko.stats().mesh.hexes)).toBeUndefined();

  // And once more, to say the door swings both ways rather than once.
  await page.locator("#mesh").click();
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("1000");
  expect(errors).toEqual([]);
});

test("the lit sheet is made finer and coarser", async ({ page }) => {
  const errors = await boot(page);
  await page.evaluate(EDGE_NEAR);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);

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
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText(String(9 * 10 * 10));
  expect(errors).toEqual([]);
});

test("the mesh is written out as VTK", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
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
  expect(saved.suggestedFilename()).toMatch(/^gecko-\d{4}-\d{2}-\d{2}-\d{6}\.vtk$/);

  const file = await readFile(await saved.path(), "utf8");
  expect(file.split("\n")[0]).toBe("# vtk DataFile Version 2.0");
  expect(file).toContain("DATASET UNSTRUCTURED_GRID");
  expect(file).toContain("POINTS 27 double");
  expect(file).toContain("CELLS 8 72");
  expect(file).toContain("SCALARS block int 1");
  expect(errors).toEqual([]);
});

test("blocks are put in a group, which colours and filters what is drawn", async ({ page }) => {
  // Groups are of blocks; a face, an edge or a node is in a group when a
  // block it bounds is. Nothing here touches the geometry: a group is a
  // part of the model somebody named.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);
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

  // Both back, and the colouring by group gives the group's own colour to
  // its cells and the plain one to the rest.
  await page.locator("[data-test=group-shown]").check();
  await page.locator("[data-test=ungrouped-visible]").check();
  await expect.poll(async () => (await drawn()).nodes).toBe(16);
  await openGroup(page, "nodes");
  await page.locator('[data-test="nodes-group"]').check();
  const tints = await page.evaluate(() => {
    const g = (window as any).__gecko;
    return g.names().map((n: number) => g.nodeTint(n));
  });
  expect(new Set(tints as string[]).size).toBe(2);

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

test("the handles can be put away, and the pointer lets them be", async ({ page }) => {
  // Edges, faces and blocks could each be hidden; the handles could not,
  // on the grounds that they are what you grab. A hundred blocks is a
  // thousand dots over the model, and looking at the shape of a blocking
  // is a thing you do as much as working on it.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await openDrawer(page);
  await openGroup(page, "nodes");

  const name = (await page.evaluate(() => (window as any).__gecko.names()[0])) as number;
  const at = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    name,
  )) as [number, number];
  const box = (await page.locator("#view").boundingBox())!;

  // There to begin with, and a click on it picks it.
  expect(await page.evaluate((p) => (window as any).__gecko.pickNode(p[0], p[1]), at)).toBe(name);

  await page.locator('[data-test="nodes-visible"]').uncheck();
  await expect(page.locator("#grp-nodes .digest")).toHaveText("hidden");
  expect(await page.evaluate(() => (window as any).__gecko.nodeShape(0))).not.toBeNull();

  // Gone from the picture, and gone from the pointer with it: a handle you
  // cannot see but can still grab moves the model with nothing on screen
  // to say what you took hold of.
  expect(await page.evaluate((p) => (window as any).__gecko.pickNode(p[0], p[1]), at)).toBeNull();
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  expect(await page.evaluate(() => (window as any).__gecko.selectedNodes())).toEqual([]);

  // Nor caught by a band drawn over them.
  await page.keyboard.down("Shift");
  await page.mouse.move(box.x + 10, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 10, box.y + box.height - 10, { steps: 8 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  expect(await page.evaluate(() => (window as any).__gecko.selectedNodes())).toEqual([]);

  // And the setting is remembered, like every other thing about how it looks.
  await page.reload();
  await page.waitForSelector("body[data-ready='1']");
  expect(await page.evaluate(() => (window as any).__gecko.prefs().nodes.visible)).toBe(false);
  expect(errors).toEqual([]);
});

test("the mesh has a band of its own, and can be put away", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.keyboard.press("e");
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("1000");

  // One band, and its controls are in it — no fold inside the fold. A mesh
  // has no groups to tell apart, so there was nothing for the inner one to
  // separate.
  await openSection(page, "mesh");
  expect(await page.locator("#mesh-layer details").count()).toBe(0);
  await expect(page.locator('[data-test="mesh-uniform"]')).toBeVisible();
  await expect(page.locator('[data-test="mesh-alpha"]')).toBeVisible();
  await page.locator("[data-test=mesh-visible]").uncheck();
  // Still meshed — the kernel holds it — but nothing of it is drawn.
  expect(await page.evaluate(() => (window as any).__gecko.skinCount())).toBe(0);
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("1000");
  expect(errors).toEqual([]);
});

/** Open the console and wait for the interpreter to be there. */
async function openConsole(page: import("@playwright/test").Page) {
  if (!(await page.evaluate(() => document.body.classList.contains("console-open")))) {
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

  await lua(page, "for i = 1, 3 do gecko.create_cube{ origin = {i * 1.5, 0, 0} } end");
  await expect(page.locator("[data-test=blocks]")).toHaveText("3");
  // And the scene drew them, rather than the readout merely saying so.
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 24);

  // A value comes back, and a table is opened out rather than printed as
  // an address.
  await lua(page, "return gecko.stats()");
  await expect(page.locator("[data-test=console-out]")).toContainText("blocks = 3");
  // Whole, and in name order: the table has nine fields, and a console
  // that printed them in hash order would hide one of them behind an
  // ellipsis as soon as a tenth was added.
  await expect(page.locator("[data-test=console-out]")).toContainText(
    /blocks = 3,.*\bedges = .*\bfaces = .*\bnodes = .*\bpatches = /s,
  );

  // `print` goes to the scrollback.
  await lua(page, 'print("nodes:", #gecko.nodes())');
  await expect(page.locator("[data-test=console-out]")).toContainText("nodes:\t24");
  expect(errors).toEqual([]);
});

test("what the mouse does is written in the console as Lua", async ({ page }) => {
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
  const shown = (await page.locator("[data-test=console-out]").textContent()) ?? "";
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
  await lua(page, "gecko.create_cube() gecko.remove_block(99) gecko.create_cube()");
  await expect(page.locator("#console-out .bad").last()).toContainText("99");
  await expect(page.locator("[data-test=blocks]")).toHaveText("1");
  expect(errors).toEqual([]);
});

test("a file handed to the page can then be opened by a script", async ({ page }) => {
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
  expect(await page.evaluate(() => (window as any).__gecko.stats().geometry.surfaces)).toBe(1);

  // Even written with a folder in front of it: there is no folder here for
  // it to be wrong about.
  await lua(page, 'gecko.clear_geometry() gecko.load_geometry("data/square.msh")');
  expect(await page.evaluate(() => (window as any).__gecko.stats().geometry.surfaces)).toBe(1);
  expect(errors).toEqual([]);
});

test("a script asking this page for a file is told why there is none", async ({ page }) => {
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
  expect(saved.suggestedFilename()).toMatch(/^gecko-\d{4}-\d{2}-\d{2}-\d{6}\.lua$/);

  const file = await readFile(await saved.path(), "utf8");
  expect(file).toContain("-- Written by gecko");
  expect(file).toContain("gecko.create_cube{ origin = {0, 0, 0}, size = 1 }");
  expect(file).toContain("gecko.mesh{ segments = 2 }");
  expect(errors).toEqual([]);
});

test("the page's Lua and the terminal's agree, byte for byte", async ({ page }) => {
  // The same script runs in two hosts: one holds the session, the other
  // reaches it through JavaScript. Everything above those four primitives
  // is the same code, and this is what says so — the script computes a
  // digest of the VTK it produces and asserts it against a number the
  // Rust suite pinned by running the very same file.
  const errors = await boot(page);
  await openConsole(page);

  const script = await readFile(
    new URL("../../../crates/gecko-lua/tests/scripts/agree.lua", import.meta.url),
    "utf8",
  );
  await lua(page, script);

  // Its own assertions did the work; what matters here is that none of
  // them fired.
  await expect(page.locator("[data-test=console-out]")).toContainText("digest");
  expect(await page.locator("#console-out .bad").count(), "the hosts disagree").toBe(0);
  // Both digests were printed, and the model the script leaves behind is
  // the smoothed grid of its second half.
  await expect(page.locator("[data-test=console-out]")).toContainText("smoothed:");
  await expect(page.locator("[data-test=mesh-hexes]")).toHaveText("216");
  expect(errors).toEqual([]);
});

test("holding shift lights the chord, and ⇧Del takes it out", async ({ page }) => {
  // The one gesture both chord operations share: hold ⇧ with a face
  // selected and the thread through it lights up, so what is about to go
  // is on screen before it goes.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);

  // Glued through the same door a script uses: what is under test is the
  // keyboard, not the mouse, and the mouse has its own tests.
  const seam = await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const [first, second] = facingPair();
    g.apply({ op: "glue_faces", first, second });
    await g.refresh();
    return first;
  });
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);

  // Selected from outside the pointer, with the block the surface lists it
  // under — the pair a click would have given.
  await page.evaluate((face) => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    const at = [...s.faces.keys()].find((q: number) => s.faces[q] === face)!;
    g.choose({ face, block: s.blocks[at] });
  }, seam);
  await expect(page.locator("#status")).toContainText("hold ⇧ for its chord");
  expect(await page.evaluate(() => (window as any).__gecko.sheet())).toEqual([]);

  await page.keyboard.down("Shift");
  await expect(page.locator("#status")).toContainText("2 blocks");
  // Three faces along a chord of two blocks — two ends and the seam — four
  // edges each.
  expect(await page.evaluate(() => (window as any).__gecko.sheet().length)).toBe(12);

  await page.keyboard.press("Delete");
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 0);
  await page.keyboard.up("Shift");
  await expect(page.locator("[data-test=blocks]")).toHaveText("0");
  expect(errors).toEqual([]);
});

test("⇧P wraps that chord in an o-grid", async ({ page }) => {
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 16);

  const seam = await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const [first, second] = facingPair();
    g.apply({ op: "glue_faces", first, second });
    await g.refresh();
    return first;
  });
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 12);
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
  expect(await page.evaluate(() => (window as any).__gecko.stats().patches)).toBe(1);

  // Raised through the page's own door, as the drag does it.
  await page.evaluate(async () => {
    const g = (window as any).__gecko;
    const s = g.surface("faces");
    g.apply({ op: "extrude_patch", face: s.faces[0], height: 0.5 });
    await g.refresh();
  });
  await page.waitForFunction(() => (window as any).__gecko.stats().blocks === 2);
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

test("a face of a patch is dragged either way, and the block follows", async ({ page }) => {
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
  const face = (await page.evaluate(() => (window as any).__gecko.surface("faces").faces[0])) as number;
  const at = (await page.evaluate((f) => (window as any).__gecko.faceScreen(f), face)) as [
    number,
    number,
  ];
  const box = (await page.locator("#view").boundingBox())!;
  await page.mouse.click(box.x + at[0], box.y + at[1]);
  await page.waitForFunction(() => (window as any).__gecko.selection().length === 1);

  // Down the screen, which on this camera is the side the face is *not*
  // wound towards: the direction the old clamp refused to go.
  await page.mouse.move(box.x + at[0], box.y + at[1]);
  await page.mouse.down();
  for (const step of [20, 60, 110]) {
    await page.mouse.move(box.x + at[0], box.y + at[1] + step);
  }
  const asked = (await page.evaluate(() => (window as any).__gecko.grownBy())) as number;
  expect(asked).toBeLessThan(-0.2);
  await page.mouse.up();

  await page.waitForFunction(() => (window as any).__gecko.stats().blocks === 1);
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
  test(`the help card is read without scrolling at ${width}×${height}`, async ({ page }) => {
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
      return { wants: card.scrollHeight, has: card.clientHeight, window: window.innerHeight };
    });
    // Not scrolling, first — the plain reading of the complaint.
    expect(
      fits.wants,
      `the card wants ${fits.wants}px and has ${fits.has}`,
    ).toBeLessThanOrEqual(fits.has);

    // And with room to spare, which the card's own height cannot show
    // because it shrinks to its contents: the measure has to be against
    // the window. The same words come out taller on another machine's
    // font engine — the first cut of this fitted here and wanted
    // twenty-five pixels more on the runner — so the margin is the thing
    // to assert. `56` is the breathing room the card leaves round itself;
    // the rest is for whatever sets the type differently.
    expect(
      fits.wants,
      `the card's ${fits.wants}px in a ${fits.window}px window`,
    ).toBeLessThanOrEqual(fits.window - 56 - 24);
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
  expect(await page.evaluate(() => (window as any).__gecko.version())).toBe(shown);
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
       local key = string.format("%.3f/%.3f/%.3f", f.centre[1], f.centre[2], f.centre[3])
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
  await page.waitForFunction(() => (window as any).__gecko.stats().blocks === 8);

  // All the way to the middle, once, so where the node lands says that the
  // slider was the number sent.
  await openParams(page);
  await page.locator("[data-test=smooth-rounds]").fill("1");
  await page.locator("[data-test=smooth-share]").fill("1");
  await expect(page.locator("[data-test=smooth-share-value]")).toHaveText("100%");

  // Out of the slider first: anything typed into a field is typing, which
  // is exactly the rule the keyboard is supposed to follow.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press("r");
  await expect(page.locator("#status")).toContainText("smoothed 1 time by 100%");

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
