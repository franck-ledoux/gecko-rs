/**
 * Static mode: the page carries its own kernel. No server, no Python.
 */

import { expect, test } from "@playwright/test";

/**
 * The two faces looking at each other: the closest pair with one from each
 * block. Evaluated in the page, and said that way rather than by coordinate,
 * so no test quietly depends on where the button happens to put a cube.
 */
declare function facingPair(): number[];

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

test("every handle stays grabbable through an opaque surface", async ({ page }) => {
  // All eight, not a node picked for being likely: Babylon's pointer system
  // returns the nearest pickable mesh, so when the surfaces joined that
  // contest it was exactly the handles **behind** one that stopped working —
  // and which ones those are depends on where the camera happens to be.
  //
  // A real drag each time. Asking `pickNode` would prove nothing: it passes
  // a predicate of its own, and so never travels the path that matters.
  const errors = await boot(page);
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
  // Two, not a dozen: a cut leaves exactly none however the frames fall, so
  // a low bar still has teeth, and a slow runner drawing a handful of frames
  // in 450 ms does not make this a lottery.
  expect(partWay.length, `radii seen: ${between.join(", ")}`).toBeGreaterThanOrEqual(2);

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
