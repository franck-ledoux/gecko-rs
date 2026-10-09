/**
 * Static mode: the page carries its own kernel. No server, no Python.
 */

import { expect, test } from "@playwright/test";

const CUBE = [
  [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
  [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1],
];

async function boot(page: import("@playwright/test").Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await page.goto("/");
  await page.waitForSelector("body[data-ready='1']", { timeout: 30_000 });
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

  // The panel is generated from the theme's keys, so there is one input per
  // colour and no markup to keep in step.
  await expect(page.locator("#swatches input[type=color]")).toHaveCount(5);
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

test("a surface never swallows the node under it", async ({ page }) => {
  // The surfaces sit exactly where the handles are. One that answered a pick
  // would be a node that cannot be dragged — with everything still looking
  // perfectly normal.
  const errors = await boot(page);
  await page.locator("#cube").click();
  await page.waitForFunction(() => (window as any).__gecko.counts().nodes === 8);
  await page.locator("[data-test=blocks-visible]").check();
  await page.locator("[data-test=faces-alpha]").fill("1");
  await page.evaluate(() => (window as any).__gecko.settled());

  const name = (await page.evaluate(() => (window as any).__gecko.names()))[0] as number;
  const at = (await page.evaluate(
    (n) => (window as any).__gecko.nodeScreen(n),
    name,
  )) as [number, number];
  expect(await page.evaluate((p) => (window as any).__gecko.pickNode(p[0], p[1]), at)).toBe(
    name,
  );
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
