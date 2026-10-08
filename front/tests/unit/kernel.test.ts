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
import { Vector3 } from "@babylonjs/core/Maths/math.vector";
import { decodeView } from "../../src/decode";
import { initKernel, WasmKernel } from "../../src/kernel-wasm";
import { HANDLE_PIXELS, Viewer, worldSizeForPixels } from "../../src/viewer";
import {
  GIZMO_LAYER,
  MAIN_LAYER,
  orientationFor,
  shortestTurn,
  type Axis,
} from "../../src/axes";
import { PRESETS } from "../../src/theme";
import { readProbe } from "../../src/mode";
import { Color3 } from "@babylonjs/core/Maths/math.color";
import type { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";

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
    expect(view.names.length).toBe(8);
    expect(new Set(view.names).size).toBe(8);
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
    // The rebasing origin has not followed the new cube. It is there so f32
    // coordinates stay small, not to keep the model centred, and moving it
    // would slide everything already on screen.
    expect(viewer.centre[0]).toBeCloseTo(0.5, 9);
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
    const kernel = new WasmKernel();
    const commands: Record<string, unknown>[] = [
      { op: "create_cube", origin: [0, 0, 0], size: 1 },
      { op: "create_cube", origin: [3, 0, 0], size: 2 },
      { op: "add_block", corners: CUBE.map((p) => [p[0], p[1] + 3, p[2] - 1]) },
    ];
    for (const command of commands) {
      expect((await kernel.apply(command)).ok).toBe(true);
    }
    // Move a node too, naming it the way a client does — out of the view.
    const name = decodeView(await kernel.view()).names[2];
    const move = { op: "move_node", node: name, to: [0.25, -0.5, 1.75] };
    expect((await kernel.apply(move)).ok).toBe(true);
    commands.push(move);
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

describe("creating a cube and moving a node", () => {
  it("creates the same block the eight corners would", async () => {
    const byCube = new WasmKernel();
    const byCorners = new WasmKernel();
    await byCube.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    await byCorners.apply({ op: "add_block", corners: CUBE });
    expect(await byCube.view()).toEqual(await byCorners.view());
  });

  it("refuses a cube with no size, and changes nothing", async () => {
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const before = await kernel.view();

    const reply = await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 0 });
    expect(reply.ok).toBe(false);
    expect(reply.message).toMatch(/strictly positive/);
    expect(await kernel.view()).toEqual(before);
  });

  it("moves the node the view named, and only that one", async () => {
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const before = decodeView(await kernel.view());
    const name = before.names[5];

    expect((await kernel.apply({ op: "move_node", node: name, to: [7, 8, 9] })).ok).toBe(true);
    const after = decodeView(await kernel.view());

    // The names are the thing that survives; the order need not.
    expect([...after.names].sort()).toEqual([...before.names].sort());
    const where = (v: typeof after, n: number) => {
      const i = [...v.names].indexOf(n);
      return [0, 1, 2].map((k) => v.centre[k] + v.positions[i * 3 + k]);
    };
    expect(where(after, name)).toEqual([7, 8, 9]);
    for (const other of before.names) {
      if (other === name) continue;
      expect(where(after, other)).toEqual(where(before, other));
    }
  });

  it("refuses to move a node that does not exist", async () => {
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const before = await kernel.view();
    const reply = await kernel.apply({ op: "move_node", node: 4242, to: [1, 2, 3] });
    expect(reply.ok).toBe(false);
    expect(await kernel.view()).toEqual(before);
  });
});

describe("the scene carries the kernel's names", () => {
  it("gives every sphere a name, and finds it back from the screen", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const view = decodeView(await kernel.view());
    viewer.setView(view);
    viewer.frameAll();
    viewer.render();

    expect([...viewer.names()].sort()).toEqual([...view.names].sort());

    // A real Babylon ray cast: aim at a node and get its name back. No GPU
    // needed, so the whole pick path is testable here.
    const name = view.names[3];
    const at = viewer.nodeScreen(name)!;
    expect(viewer.pickNode(...at)).toBe(name);
    expect(viewer.pickNode(2, 2)).toBeNull();
  });

  it("reports a node's position in model coordinates", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    // Far from the origin, so the rebasing has to be undone correctly.
    await kernel.apply({ op: "create_cube", origin: [1000, 0, 0], size: 1 });
    const view = decodeView(await kernel.view());
    viewer.setView(view);

    const corners = Array.from(view.names).map((n) => viewer.nodePosition(n)!);
    expect(corners.every((p) => p[0] === 1000 || p[0] === 1001)).toBe(true);
  });
});

describe("the orientation gizmo", () => {
  it("puts the camera on the axis you clicked", () => {
    // A pure function, so the six cases are checkable without a scene. The
    // camera sits *on* the named axis, so you are looking at the plane
    // opposite: +x shows you YZ.
    const direction = (axis: Axis) => {
      const { alpha, beta } = orientationFor(axis);
      return [
        Math.cos(alpha) * Math.sin(beta),
        Math.cos(beta),
        Math.sin(alpha) * Math.sin(beta),
        // `+ 0` normalises negative zero, which is not worth asserting about.
      ].map((v) => Math.round(v * 1000) / 1000 + 0);
    };
    expect(direction("+x")).toEqual([1, 0, 0]);
    expect(direction("-x")).toEqual([-1, 0, 0]);
    expect(direction("+y")).toEqual([0, 1, -0.001]);
    expect(direction("-y")).toEqual([0, -1, -0.001]);
    expect(direction("+z")).toEqual([0, 0, 1]);
    expect(direction("-z")).toEqual([0, 0, -1]);

    // Looking straight down Y is a singularity for an arc-rotate camera, so
    // the pole is approached but never reached.
    for (const axis of ["+y", "-y"] as Axis[]) {
      const { beta } = orientationFor(axis);
      expect(beta).toBeGreaterThan(0);
      expect(beta).toBeLessThan(Math.PI);
    }
  });

  it("turns the short way round", () => {
    // Azimuth is periodic: 3.1 and -3.1 are a tenth of a turn apart, and also
    // six radians apart. Animating to the raw value spins the model for no
    // reason.
    expect(shortestTurn(3.1, -3.1)).toBeCloseTo(3.1831, 3);
    expect(shortestTurn(-3.1, 3.1)).toBeCloseTo(-3.1831, 3);
    expect(shortestTurn(0, 0)).toBe(0);
    expect(shortestTurn(0.2, 0.5)).toBe(0.5);
    // Whatever it returns is the same angle.
    for (const [from, to] of [[3.1, -3.1], [-6, 1], [10, -10]]) {
      const turned = shortestTurn(from, to);
      expect(Math.abs(Math.sin(turned) - Math.sin(to))).toBeLessThan(1e-9);
      expect(Math.abs(turned - from)).toBeLessThanOrEqual(Math.PI + 1e-9);
    }
  });

  it("lives on its own layer, so neither camera draws the other's meshes", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(await kernel.view()));

    // Two cameras, and the pointer picking is pinned to the main one — the
    // drag behaviours run on that, and must not see the corner.
    expect(viewer.scene.activeCameras).toHaveLength(2);
    expect(viewer.scene.cameraToUseForPointers).toBe(viewer.camera);
    expect(viewer.camera.layerMask).toBe(MAIN_LAYER);
    expect(viewer.axes.camera.layerMask).toBe(GIZMO_LAYER);

    // Every mesh belongs to exactly one side. A mesh created without a layer
    // mask renders in both, which is the failure this guards.
    for (const mesh of viewer.scene.meshes) {
      const sides = [MAIN_LAYER, GIZMO_LAYER].filter((l) => mesh.layerMask & l);
      expect(sides, `${mesh.name} renders in ${sides.length} viewports`).toHaveLength(1);
    }
  });

  it("has the animation machinery it needs to swing the view", async () => {
    // `Animation.CreateAndStartAnimation` calls `scene.beginDirectAnimation`,
    // which only exists if the animatable side-effect module was imported.
    // Importing the `Animation` class alone compiles, ships, and throws.
    const viewer = new Viewer(new NullEngine());
    expect(typeof viewer.scene.beginDirectAnimation).toBe("function");
    expect(() => viewer.axes.lookDown("+z")).not.toThrow();
  });

  it("can be hidden, and snaps the camera when asked", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(await kernel.view()));

    expect(viewer.axes.visible).toBe(true);
    viewer.axes.setVisible(false);
    expect(viewer.axes.visible).toBe(false);
    // Hidden means unpickable: a triad you cannot see must not be in the way.
    viewer.render();
    expect(viewer.axes.pick(10, 10)).toBeNull();

    viewer.axes.setVisible(true);
    viewer.axes.snapTo("+x");
    expect(viewer.camera.alpha).toBeCloseTo(0, 9);
    expect(viewer.camera.beta).toBeCloseTo(Math.PI / 2, 9);
  });

  it("finds an axis back from the corner it was drawn in", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(await kernel.view()));
    viewer.frameAll();
    viewer.render();

    // Project through the gizmo's own camera, then pick it back. Using the
    // scene's transform would aim into the middle of the model instead.
    for (const axis of ["+x", "+y", "+z"] as Axis[]) {
      const at = viewer.axes.axisScreen(axis);
      expect(at, `no screen position for ${axis}`).not.toBeNull();
      expect(viewer.axes.pick(...at!)).toBe(axis);
    }
  });
});

describe("the theme", () => {
  it("reaches the scene", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(await kernel.view()));

    viewer.applyTheme(PRESETS.light);
    expect(viewer.scene.clearColor.toHexString()).toBe(
      Color3.FromHexString(PRESETS.light.background).toColor4(1).toHexString(),
    );
    const anyNode = viewer.scene.getMeshByName("node0")!;
    expect((anyNode.material as StandardMaterial).emissiveColor.toHexString()).toBe(
      Color3.FromHexString(PRESETS.light.nodes).toHexString(),
    );

    // And a theme set before there is anything to colour still applies to what
    // arrives afterwards.
    const fresh = new Viewer(new NullEngine());
    fresh.applyTheme(PRESETS.light);
    fresh.setView(decodeView(await kernel.view()));
    expect(
      (fresh.scene.getMeshByName("node0")!.material as StandardMaterial).emissiveColor.toHexString(),
    ).toBe(Color3.FromHexString(PRESETS.light.nodes).toHexString());
  });
});

describe("node handles", () => {
  async function sceneWith(cubes: number) {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    for (let i = 0; i < cubes; i++) {
      await kernel.apply({ op: "create_cube", origin: [i * 1.5, 0, 0], size: 1 });
    }
    viewer.setView(decodeView(await kernel.view()));
    viewer.frameAll();
    viewer.render();
    return viewer;
  }

  /** How many pixels across the first handle actually covers. */
  function apparent(viewer: Viewer): number {
    const mesh = viewer.scene.getMeshByName("node0")!;
    const distance = Vector3.Distance(mesh.position, viewer.camera.position);
    const height = viewer.scene.getEngine().getRenderHeight();
    return (viewer.nodeSize * height) / (2 * Math.tan(viewer.camera.fov / 2) * distance);
  }

  it("covers the same number of pixels however many blocks there are", async () => {
    // The bug this guards: sizing handles off the model's bounding box made
    // them grow with the model while the blocks they mark did not, until a
    // corner marker was half the width of its own cube. Sizing them off a
    // block's edge instead swaps that for handles too small to aim at once
    // the model is framed from far away. What has to stay fixed is the only
    // thing the user sees: how big they are on screen.
    for (const count of [1, 2, 5, 20]) {
      expect(apparent(await sceneWith(count))).toBeCloseTo(HANDLE_PIXELS, 6);
    }
  });

  it("covers the same number of pixels however big the blocks are", async () => {
    const viewer = new Viewer(new NullEngine());
    for (const size of [0.01, 1, 1000]) {
      const kernel = new WasmKernel();
      await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size });
      viewer.setView(decodeView(await kernel.view()));
      viewer.frameAll();
      viewer.render();
      expect(apparent(viewer)).toBeCloseTo(HANDLE_PIXELS, 6);
      // In the model's own units it does scale — that is the point.
      expect(viewer.nodeSize).toBeGreaterThan(0);
    }
  });

  it("converts pixels to world units proportionally to distance", () => {
    // A pure function, so the relation is checkable without a scene.
    const near = worldSizeForPixels(10, 5, 0.8, 600);
    expect(worldSizeForPixels(10, 10, 0.8, 600)).toBeCloseTo(near * 2, 12);
    expect(worldSizeForPixels(20, 5, 0.8, 600)).toBeCloseTo(near * 2, 12);
    expect(worldSizeForPixels(10, 5, 0.8, 1200)).toBeCloseTo(near / 2, 12);
    // A canvas with no height has no pixels to cover.
    expect(worldSizeForPixels(10, 5, 0.8, 0)).toBe(0);
  });
});

describe("which camera is the active one", () => {
  it("is the main camera once a frame is over", async () => {
    // Rendering leaves it set to whichever camera went last, which is the
    // corner gizmo. Anything running between frames reads it — including
    // Babylon's drag behaviour, which uses it both to suspend the right
    // camera's controls and to build the ray the drag follows.
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    await kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(await kernel.view()));

    viewer.render();
    expect(viewer.scene.activeCamera).toBe(viewer.camera);
    viewer.render();
    expect(viewer.scene.activeCamera).toBe(viewer.camera);
  });
});

describe("reading the mode probe", () => {
  it("knows a station when it hears one", () => {
    expect(readProbe(200, "station")).toBe("station");
    // The station's own server adds no newline; a proxy might.
    expect(readProbe(200, "station\n")).toBe("station");
  });

  it("knows a static host", () => {
    // No such route: the deployed site, which carries its own kernel.
    expect(readProbe(404, "not found")).toBe("static");
    // A host that serves index.html for anything, which is why the body is
    // read at all rather than just the code.
    expect(readProbe(200, "<!doctype html>")).toBe("static");
  });

  it("waits when a station is promised and missing", () => {
    // This is the one. Vite forwards `mode` to the station, so with no station
    // behind it the answer is a 502/500 from the proxy — not "no station
    // here". Read as static, the page starts a kernel of its own and then
    // ignores every command typed in Python, while looking perfectly alive.
    expect(readProbe(500, "")).toBe("waiting");
    expect(readProbe(502, "Error: connect ECONNREFUSED")).toBe("waiting");
    // And a fetch that threw, which this reports as status 0.
    expect(readProbe(0, "")).toBe("waiting");
  });
});
