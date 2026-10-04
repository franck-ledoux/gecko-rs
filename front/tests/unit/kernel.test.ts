/**
 * The static mode's whole stack, minus the GPU, in Node.
 *
 * `NullEngine` gives a real Babylon scene graph with no rendering context, and
 * the kernel is the same wasm binary the browser loads. So decode → scene →
 * command → scene runs here in milliseconds, and Playwright is left to test
 * only what genuinely needs a browser.
 */

import { beforeAll, describe, expect, it } from "vitest";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { NullEngine } from "@babylonjs/core/Engines/nullEngine";
import { decodeView } from "../../src/decode";
import { initKernel, WasmKernel } from "../../src/kernel-wasm";
import { Viewer } from "../../src/viewer";

const CUBE = [
  [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
  [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1],
];

beforeAll(async () => {
  await initKernel(readFileSync(new URL("../../src/pkg/gecko_bg.wasm", import.meta.url)));
});

describe("the binary transport", () => {
  it("decodes the buffer the kernel encodes", async () => {
    const kernel = new WasmKernel();
    await kernel.apply({ op: "add_block", corners: CUBE });
    const view = decodeView(await kernel.view());

    expect(view.positions.length / 3).toBe(8);
    expect(view.edges.length / 2).toBe(12);
    expect(view.centre).toEqual([0.5, 0.5, 0.5]);
    // Rebased on the model centre: the unit cube sits in [-0.5, 0.5].
    expect([...view.positions].every((x) => Math.abs(x) <= 0.5 + 1e-6)).toBe(true);
    // Every edge addresses a node that exists.
    expect([...view.edges].every((i) => i < view.positions.length / 3)).toBe(true);
  });

  it("refuses a buffer it cannot trust", async () => {
    expect(() => decodeView(new Uint8Array(8))).toThrow(/shorter than a header/);
    expect(() => decodeView(new Uint8Array(64))).toThrow(/bad magic/);

    const kernel = new WasmKernel();
    await kernel.apply({ op: "add_block", corners: CUBE });
    const good = await kernel.view();
    expect(() => decodeView(good.slice(0, good.length - 8))).toThrow(/declares/);
  });

  it("keeps precision for a model far from the origin", async () => {
    const far = CUBE.map((p) => [p[0] + 1e6, p[1] + 2e6, p[2] - 5e5]);
    const kernel = new WasmKernel();
    await kernel.apply({ op: "add_block", corners: far });
    const view = decodeView(await kernel.view());

    expect([...view.positions].every((x) => Math.abs(x) <= 0.5 + 1e-6)).toBe(true);
    // Recompose f64 centre + f32 offset and land back on the cube.
    for (let i = 0; i < view.positions.length; i += 3) {
      const back = [0, 1, 2].map((k) => view.centre[k] + view.positions[i + k]);
      expect(Math.abs(back[0] - 1e6)).toBeLessThan(1.001);
      expect(Math.abs(back[1] - 2e6)).toBeLessThan(1.001);
    }
  });
});

describe("the static kernel", () => {
  it("reports what it did, and refuses what it cannot do", async () => {
    const kernel = new WasmKernel();
    expect(kernel.mode).toBe("static");
    expect(await kernel.stats()).toEqual({ nodes: 0, edges: 0, faces: 0, blocks: 0 });

    const reply = await kernel.apply({ op: "add_block", corners: CUBE });
    expect(reply.ok).toBe(true);
    expect(reply.viewChanged).toBe(true);
    expect(await kernel.stats()).toEqual({ nodes: 8, edges: 12, faces: 6, blocks: 1 });

    const bad = await kernel.apply({ op: "nonesuch" });
    expect(bad.ok).toBe(false);
    expect(bad.viewChanged).toBe(false);
    // A refused command must leave the session alone.
    expect(await kernel.stats()).toEqual({ nodes: 8, edges: 12, faces: 6, blocks: 1 });
  });
});

describe("the scene is a function of the view", () => {
  it("draws one sphere per node and one segment per edge", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();

    await kernel.apply({ op: "add_block", corners: CUBE });
    viewer.setView(decodeView(await kernel.view()));
    expect(viewer.nodeCount).toBe(8);
    expect(viewer.edgeCount).toBe(12);

    // A second block, standing apart: the counts follow the kernel, not a
    // guess made here.
    await kernel.apply({ op: "add_block", corners: CUBE.map((p) => [p[0] + 3, p[1], p[2]]) });
    viewer.setView(decodeView(await kernel.view()));
    expect(viewer.nodeCount).toBe(16);
    expect(viewer.edgeCount).toBe(24);
    expect(viewer.centre[0]).toBeCloseTo(2, 9);
  });

  it("frames the model from its bounds, not from where the camera was", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    await kernel.apply({ op: "add_block", corners: CUBE });
    viewer.setView(decodeView(await kernel.view()));

    viewer.camera.alpha = 1.234;
    viewer.camera.radius = 99;
    viewer.frameAll();
    // Reproducible: a screenshot test would be worthless otherwise.
    expect(viewer.camera.alpha).toBeCloseTo(-Math.PI / 3, 9);
    expect(viewer.camera.radius).toBeCloseTo(Math.sqrt(3) * 1.8, 6);
  });
});

describe("the contract the other mode has to meet", () => {
  it("records what the static kernel produces, for pytest to replay", async () => {
    // Both modes are transports onto one `gecko_cmd::Session`. If that stops
    // being true, these bytes stop matching — and `python/test_modes.py`
    // replays this very sequence through PyO3 and says so.
    const commands = [
      { op: "add_block", corners: CUBE },
      { op: "add_block", corners: CUBE.map((p) => [p[0] + 3, p[1], p[2]]) },
      { op: "add_block", corners: CUBE.map((p) => [p[0], p[1] + 3, p[2] - 1]) },
    ];
    const kernel = new WasmKernel();
    for (const command of commands) {
      expect((await kernel.apply(command)).ok).toBe(true);
    }
    const bytes = await kernel.view();
    const stats = await kernel.stats();
    expect(stats.blocks).toBe(3);

    mkdirSync(new URL("../../artifacts/", import.meta.url), { recursive: true });
    writeFileSync(
      new URL("../../artifacts/static-session.json", import.meta.url),
      JSON.stringify({ commands, stats, length: bytes.length, fnv1a: fnv1a(bytes) }, null, 1),
    );
  });
});

/** A digest cheap to recompute in any language. */
function fnv1a(data: Uint8Array): number {
  let h = 0x811c9dc5 >>> 0;
  for (const byte of data) {
    h = (h ^ byte) >>> 0;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}
