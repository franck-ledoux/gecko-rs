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
import { decodeGeometry, decodeMesh, decodeView } from "../../src/decode";
import { initKernel, WasmKernel } from "../../src/kernel-wasm";
import {
  CLOSEST_APPROACH,
  HANDLE_PIXELS,
  heightAlong,
  Viewer,
  worldSizeForPixels,
  ZOOM_PER_NOTCH,
} from "../../src/viewer";
import {
  GIZMO_LAYER,
  MAIN_LAYER,
  orientationFor,
  shortestTurn,
  type Axis,
} from "../../src/axes";
import { DEFAULTS, PRESETS } from "../../src/theme";
import { colorForId, FAMILY, hexToRgb, hslToHex, UNNAMED } from "../../src/palette";
import { buildSurface, edgeSegments, meshEdges, quadsToDraw } from "../../src/surface";
import { geomBounds, geomCorners, geomCurves, geomSurfaces, GEOM_GROUPS } from "../../src/geometry";
import {
  orthoExtents,
  paneAt,
  paneRect,
  paneViewport,
  worldSizeForPixelsOrtho,
} from "../../src/panes";
import { isLight, loadPrefs, MAX_BOX_MARGIN, SHOT_SCALES } from "../../src/theme";
import { actionFor, ACTIONS, helpRows, SHORTCUTS } from "../../src/shortcuts";
import { dataUrlToBlob, shotName } from "../../src/capture";
import type { View } from "../../src/decode";
import { Color3 } from "@babylonjs/core/Maths/math.color";
import type { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";

const CUBE = [
  [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
  [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1],
];

beforeAll(async () => {
  await initKernel(readFileSync(new URL("../../src/pkg/gecko_bg.wasm", import.meta.url)));
});

describe("the mesh buffer", () => {
  it("carries the skin, and nothing inside it", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });

    // Empty until something is meshed, and a buffer rather than a special
    // case for the caller.
    expect(decodeMesh(kernel.mesh()).quads.length).toBe(0);

    kernel.apply({ op: "mesh", segments: 3 });
    const view = decodeMesh(kernel.mesh());
    // Six faces of nine, and the 56 nodes on the outside of a 4 × 4 × 4
    // lattice rather than the 64 of the whole of it.
    expect(view.quads.length / 4).toBe(54);
    expect(view.positions.length / 3).toBe(56);
    expect(view.blocks.length).toBe(54);
    expect([...view.blocks].every((b) => b === 0)).toBe(true);

    // Every corner of the skin is on the surface of the cube.
    for (let q = 0; q < view.quads.length; q++) {
      const node = view.quads[q];
      const at = [0, 1, 2].map((k) => view.positions[node * 3 + k] + view.center[k]);
      expect(at.some((x) => Math.abs(x) < 1e-6 || Math.abs(x - 1) < 1e-6)).toBe(true);
    }

    // And it goes when the blocking changes under it.
    kernel.apply({ op: "create_cube", origin: [3, 0, 0], size: 1 });
    expect(decodeMesh(kernel.mesh()).quads.length).toBe(0);
  });

  it("draws one line per side of the skin, however many quadrangles share it", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    kernel.apply({ op: "mesh", segments: 2 });
    const view = decodeMesh(kernel.mesh());

    // A 2 × 2 × 2 mesh has 24 quadrangles of skin; their sides are the 48
    // edges of the cube's own surface grid, not 96.
    expect(view.quads.length / 4).toBe(24);
    expect(meshEdges(view).length / 6).toBe(48);
  });
});

describe("the binary transport", () => {
  it("decodes the buffer the kernel encodes", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: CUBE });
    const view = decodeView(kernel.view());

    expect(view.positions.length / 3).toBe(8);
    expect(view.names.length).toBe(8);
    expect(new Set(view.names).size).toBe(8);
    expect(view.edges.length / 2).toBe(12);
    expect(view.center).toEqual([0.5, 0.5, 0.5]);
    // Rebased on the model center: the unit cube sits in [-0.5, 0.5].
    expect([...view.positions].every((x) => Math.abs(x) <= 0.5 + 1e-6)).toBe(true);
    // Every edge addresses a node that exists.
    expect([...view.edges].every((i) => i < view.positions.length / 3)).toBe(true);
  });

  it("refuses a buffer it cannot trust", async () => {
    expect(() => decodeView(new Uint8Array(8))).toThrow(/shorter than a header/);
    expect(() => decodeView(new Uint8Array(64))).toThrow(/bad magic/);

    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: CUBE });
    const good = kernel.view();
    expect(() => decodeView(good.slice(0, good.length - 8))).toThrow(/declares/);
  });

  it("keeps precision for a model far from the origin", async () => {
    const far = CUBE.map((p) => [p[0] + 1e6, p[1] + 2e6, p[2] - 5e5]);
    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: far });
    const view = decodeView(kernel.view());

    expect([...view.positions].every((x) => Math.abs(x) <= 0.5 + 1e-6)).toBe(true);
    // Recompose f64 center + f32 offset and land back on the cube.
    for (let i = 0; i < view.positions.length; i += 3) {
      const back = [0, 1, 2].map((k) => view.center[k] + view.positions[i + k]);
      expect(Math.abs(back[0] - 1e6)).toBeLessThan(1.001);
      expect(Math.abs(back[1] - 2e6)).toBeLessThan(1.001);
    }
  });
});

describe("the static kernel", () => {
  it("reports what it did, and refuses what it cannot do", async () => {
    const kernel = new WasmKernel();
    expect(kernel.stats()).toEqual({
      nodes: 0,
      edges: 0,
      faces: 0,
      blocks: 0,
      patches: 0,
      geometry: { generation: 0 },
      history: { steps: 5, back: 0 },
      mesh: { generation: 0 },
      classified: { nodes: 0, edges: 0, faces: 0, frozen: 0 },
    });

    const reply = kernel.apply({ op: "add_block", corners: CUBE });
    expect(reply.ok).toBe(true);
    expect(reply.viewChanged).toBe(true);
    expect(kernel.stats()).toEqual({
      nodes: 8,
      edges: 12,
      faces: 6,
      blocks: 1,
      patches: 0,
      geometry: { generation: 0 },
      history: { steps: 5, back: 1 },
      mesh: { generation: 0 },
      classified: { nodes: 0, edges: 0, faces: 0, frozen: 0 },
    });

    const bad = kernel.apply({ op: "nonesuch" });
    expect(bad.ok).toBe(false);
    expect(bad.viewChanged).toBe(false);
    // A refused command must leave the session alone.
    expect(kernel.stats()).toEqual({
      nodes: 8,
      edges: 12,
      faces: 6,
      blocks: 1,
      patches: 0,
      geometry: { generation: 0 },
      history: { steps: 5, back: 1 },
      mesh: { generation: 0 },
      classified: { nodes: 0, edges: 0, faces: 0, frozen: 0 },
    });
  });
});

describe("the scene is a function of the view", () => {
  it("draws one sphere per node and one segment per edge", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();

    kernel.apply({ op: "add_block", corners: CUBE });
    viewer.setView(decodeView(kernel.view()));
    expect(viewer.nodeCount).toBe(8);
    expect(viewer.edgeCount).toBe(12);

    // A second block, standing apart: the counts follow the kernel, not a
    // guess made here.
    kernel.apply({ op: "add_block", corners: CUBE.map((p) => [p[0] + 3, p[1], p[2]]) });
    viewer.setView(decodeView(kernel.view()));
    expect(viewer.nodeCount).toBe(16);
    expect(viewer.edgeCount).toBe(24);
    // The rebasing origin has not followed the new cube. It is there so f32
    // coordinates stay small, not to keep the model centerd, and moving it
    // would slide everything already on screen.
    expect(viewer.center[0]).toBeCloseTo(0.5, 9);
  });

  it("frames the model from its bounds, and leaves the direction alone", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: CUBE });
    viewer.setView(decodeView(kernel.view()));

    // Looking at it from somewhere deliberate, and from too far away.
    viewer.camera.alpha = 1.234;
    viewer.camera.beta = 0.789;
    viewer.camera.radius = 99;
    viewer.frameAll(true);

    // Where it is and how far away come from the model. Which way round you
    // are looking at it from is yours — recentring is not a point of view.
    expect(viewer.camera.radius).toBeCloseTo(Math.sqrt(3) * 1.8, 6);
    expect(viewer.camera.target.x).toBeCloseTo(0, 6);
    expect(viewer.camera.target.y).toBeCloseTo(0, 6);
    expect(viewer.camera.target.z).toBeCloseTo(0, 6);
    expect(viewer.camera.alpha).toBeCloseTo(1.234, 9);
    expect(viewer.camera.beta).toBeCloseTo(0.789, 9);
  });

  it("moves the target without Babylon re-deriving the angles from it", async () => {
    // `camera.target = …` normally keeps the camera where it is and works
    // out new angles to the new target, which would turn every recentring
    // into a change of direction. This is the flag that stops it, asserted
    // rather than trusted.
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [7, -3, 5], size: 2 });
    viewer.setView(decodeView(kernel.view()));

    viewer.camera.alpha = 0.5;
    viewer.camera.beta = 1.1;
    viewer.camera.target = new Vector3(4, 4, 4);
    expect(viewer.camera.alpha).toBeCloseTo(0.5, 9);
    expect(viewer.camera.beta).toBeCloseTo(1.1, 9);
  });

  it("glides rather than jumping, unless told to jump", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: CUBE });
    viewer.setView(decodeView(kernel.view()));
    viewer.camera.radius = 99;

    viewer.frameAll();
    // Not there yet: an animation is running and the value is still the old
    // one until it has had a frame.
    expect(viewer.camera.radius).toBe(99);
    expect(viewer.scene.getAllAnimatablesByTarget(viewer.camera)).not.toHaveLength(0);

    // And asking for a jump really jumps, with nothing left running.
    viewer.frameAll(true);
    expect(viewer.camera.radius).toBeCloseTo(Math.sqrt(3) * 1.8, 6);
    expect(viewer.scene.getAllAnimatablesByTarget(viewer.camera)).toHaveLength(0);
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
      expect((kernel.apply(command)).ok).toBe(true);
    }
    // Move a node too, naming it the way a client does — out of the view.
    const name = decodeView(kernel.view()).names[2];
    const move = { op: "move_node", node: name, to: [0.25, -0.5, 1.75] };
    expect((kernel.apply(move)).ok).toBe(true);
    commands.push(move);

    // And a glue, because it is the first operation that walks an orbit and
    // orders what it finds. Non-determinism there would show up here and
    // nowhere else — which is exactly how an earlier prototype's HashSet was
    // caught.
    const view = decodeView(kernel.view());
    const center = (q: number) => {
      const at = [0, 0, 0];
      for (let k = 0; k < 4; k++)
        for (let axis = 0; axis < 3; axis++)
          at[axis] += view.positions[view.quads[q * 4 + k] * 3 + axis] / 4;
      return at;
    };
    let pair: [number, number] = [0, 0];
    let shortest = Infinity;
    for (let a = 0; a < view.quadFaces.length; a++)
      for (let b = 0; b < view.quadFaces.length; b++) {
        if (view.quadBlocks[a] === view.quadBlocks[b]) continue;
        const [p, q] = [center(a), center(b)];
        const gap = Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]);
        if (gap < shortest) {
          shortest = gap;
          pair = [view.quadFaces[a], view.quadFaces[b]];
        }
      }
    const glue = { op: "glue_faces", first: pair[0], second: pair[1] };
    expect((kernel.apply(glue)).ok).toBe(true);
    commands.push(glue);

    // And a removal, which walks an orbit, unsews, and hands darts back to
    // the pool. Whether the pool hands them out again in the same order is
    // exactly the kind of thing only this test would notice.
    const standing = decodeView(kernel.view());
    // The block that was not part of the glue: the one whose faces are all
    // still on the boundary.
    const glued = new Set(
      [...standing.quadFaces.keys()]
        .filter((q) => standing.quadFaces.filter((f) => f === standing.quadFaces[q]).length > 1)
        .map((q) => standing.quadBlocks[q]),
    );
    const lonely = [...standing.quadBlocks].find((b) => !glued.has(b))!;
    const remove = { op: "remove_block", block: lonely };
    expect((kernel.apply(remove)).ok).toBe(true);
    commands.push(remove);

    // And a cut taken back. An undo puts a copy of the whole structure
    // back, down to the dart numbers the names are derived from — so a
    // façade that rebuilt the model instead of restoring it would arrive
    // here with different bytes, and only this test would see it.
    const cut = {
      op: "split_sheet",
      edge: decodeView(kernel.view()).edgeNames[0],
      at: 0.5,
    };
    expect((kernel.apply(cut)).ok).toBe(true);
    commands.push(cut);
    expect((kernel.stats()).blocks).toBe(4);

    const undo = { op: "undo" };
    expect((kernel.apply(undo)).ok).toBe(true);
    commands.push(undo);

    // And a mesh, which is the one command whose answer is a hundred
    // times the size of the question. Both façades have to agree about
    // the count, and `test_modes.py` compares the stats.
    const meshed = { op: "mesh", segments: 2 };
    expect((kernel.apply(meshed)).ok).toBe(true);
    commands.push(meshed);

    const bytes = kernel.view();
    const stats = kernel.stats();
    expect(stats.blocks).toBe(2);

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
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const before = kernel.view();

    const reply = kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 0 });
    expect(reply.ok).toBe(false);
    expect(reply.message).toMatch(/strictly positive/);
    expect(kernel.view()).toEqual(before);
  });

  it("moves the node the view named, and only that one", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const before = decodeView(kernel.view());
    const name = before.names[5];

    expect((kernel.apply({ op: "move_node", node: name, to: [7, 8, 9] })).ok).toBe(true);
    const after = decodeView(kernel.view());

    // The names are the thing that survives; the order need not.
    expect([...after.names].sort()).toEqual([...before.names].sort());
    const where = (v: typeof after, n: number) => {
      const i = [...v.names].indexOf(n);
      return [0, 1, 2].map((k) => v.center[k] + v.positions[i * 3 + k]);
    };
    expect(where(after, name)).toEqual([7, 8, 9]);
    for (const other of before.names) {
      if (other === name) continue;
      expect(where(after, other)).toEqual(where(before, other));
    }
  });

  it("refuses to move a node that does not exist", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const before = kernel.view();
    const reply = kernel.apply({ op: "move_node", node: 4242, to: [1, 2, 3] });
    expect(reply.ok).toBe(false);
    expect(kernel.view()).toEqual(before);
  });
});

describe("the scene carries the kernel's names", () => {
  it("gives every sphere a name, and finds it back from the screen", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const view = decodeView(kernel.view());
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
    kernel.apply({ op: "create_cube", origin: [1000, 0, 0], size: 1 });
    const view = decodeView(kernel.view());
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
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(kernel.view()));

    // Two cameras, and the pointer picking is pinned to the main one — the
    // drag behaviors run on that, and must not see the corner.
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
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(kernel.view()));

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
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(kernel.view()));
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
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(kernel.view()));

    viewer.applyPrefs({ ...DEFAULTS, theme: PRESETS.light });
    expect(viewer.scene.clearColor.toHexString()).toBe(
      Color3.FromHexString(PRESETS.light.background).toColor4(1).toHexString(),
    );
    const anyNode = viewer.scene.getMeshByName("node0")!;
    expect((anyNode.material as StandardMaterial).emissiveColor.toHexString()).toBe(
      Color3.FromHexString(PRESETS.light.nodes).toHexString(),
    );

    // And a theme set before there is anything to color still applies to what
    // arrives afterwards.
    const fresh = new Viewer(new NullEngine());
    fresh.applyPrefs({ ...DEFAULTS, theme: PRESETS.light });
    fresh.setView(decodeView(kernel.view()));
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
      kernel.apply({ op: "create_cube", origin: [i * 1.5, 0, 0], size: 1 });
    }
    viewer.setView(decodeView(kernel.view()));
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
      kernel.apply({ op: "create_cube", origin: [0, 0, 0], size });
      viewer.setView(decodeView(kernel.view()));
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
    // Babylon's drag behavior, which uses it both to suspend the right
    // camera's controls and to build the ray the drag follows.
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.setView(decodeView(kernel.view()));

    viewer.render();
    expect(viewer.scene.activeCamera).toBe(viewer.camera);
    viewer.render();
    expect(viewer.scene.activeCamera).toBe(viewer.camera);
  });
});

describe("a color per name", () => {
  it("gives a name the same color for ever", () => {
    // The whole reason not to color by position in a list: an edit reorders
    // that list, and the model would repaint itself while you watched.
    expect(colorForId(7, 2)).toBe(colorForId(7, 2));
    expect(colorForId(7, 2)).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("does not depend on how many cells there are", () => {
    // `hue = name / total` is the obvious formula and the wrong one: adding
    // one block would recolor every other block in the model.
    const before = [1, 2, 3].map((n) => colorForId(n, 3));
    const after = [1, 2, 3].map((n) => colorForId(n, 3));
    expect(after).toEqual(before);
    // And it is still spread out: three names, three colors.
    expect(new Set(before).size).toBe(3);
  });

  it("keeps a face and a block of the same number apart", () => {
    expect(colorForId(4, 2)).not.toBe(colorForId(4, 3));
  });

  it("has something to say about a cell with no name yet", () => {
    expect(colorForId(UNNAMED, 2)).toBe(colorForId(UNNAMED, 3));
  });

  it("round-trips through hex the way Babylon reads it", () => {
    expect(hexToRgb("#000000")).toEqual([0, 0, 0]);
    expect(hexToRgb("#ffffff")).toEqual([1, 1, 1]);
    const [r, g, b] = hexToRgb(hslToHex(0, 1, 0.5));
    expect([r, g, b]).toEqual([1, 0, 0]);
  });
});

/** A view with two coplanar quadrangles sharing one face name: what the
 *  kernel will ship once blocks can be joined, built by hand because today
 *  it cannot yet produce one. */
function sharedFaceView(): View {
  return {
    center: [0, 0, 0],
    positions: new Float32Array([0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0]),
    names: new Uint32Array([0, 1, 2, 3]),
    nodeClassDim: new Int32Array([-1, -1, -1, -1]),
    nodeClassTag: new Int32Array([0, 0, 0, 0]),
    nodeFrozen: new Uint32Array([0, 0, 0, 0]),
    edges: new Uint32Array([0, 1]),
    edgeNames: new Uint32Array([5]),
    edgeClassDim: new Int32Array([-1]),
    edgeClassTag: new Int32Array([0]),
    quadClassDim: new Int32Array([-1, -1]),
    quadClassTag: new Int32Array([0, 0]),
    // The same four corners twice: one face, seen from each of two blocks.
    quads: new Uint32Array([0, 1, 2, 3, 0, 3, 2, 1]),
    quadFaces: new Uint32Array([9, 9]),
    quadBlocks: new Uint32Array([100, 200]),
    nodeGroupsLo: new Uint32Array([0, 0, 0, 0]),
    nodeGroupsHi: new Uint32Array([0, 0, 0, 0]),
    nodeLoose: new Uint32Array([0, 0, 0, 0]),
    edgeGroupsLo: new Uint32Array([0]),
    edgeGroupsHi: new Uint32Array([0]),
    edgeLoose: new Uint32Array([0]),
    quadGroupsLo: new Uint32Array([0, 0]),
    quadGroupsHi: new Uint32Array([0, 0]),
  };
}

describe("choosing which quadrangles to draw", () => {
  it("draws every one of them for blocks", () => {
    // Each block keeps a closed surface of its own, so a shared face is drawn
    // once per block. Dropping one would open a shell the moment it shrinks.
    expect(quadsToDraw(sharedFaceView(), "block", true)).toEqual([0, 1]);
    expect(quadsToDraw(sharedFaceView(), "block", false)).toEqual([0, 1]);
  });

  it("draws a shared face once, and only when asked", () => {
    // It arrives twice and it is still one face.
    expect(quadsToDraw(sharedFaceView(), "face", true)).toEqual([0]);
    // Internal, so hidden when internal faces are off.
    expect(quadsToDraw(sharedFaceView(), "face", false)).toEqual([]);
  });

  it("keeps a face on the boundary either way", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const view = decodeView(kernel.view());
    // A free-standing block has six faces and none of them is shared, so
    // hiding internal faces hides nothing.
    expect(quadsToDraw(view, "face", true)).toHaveLength(6);
    expect(quadsToDraw(view, "face", false)).toHaveLength(6);
  });
});

describe("building a surface", () => {
  const cube = async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    return decodeView(kernel.view());
  };

  const options = (over: Partial<Parameters<typeof buildSurface>[1]> = {}) => ({
    by: "face" as const,
    coloring: "uniform" as const,
    uniform: "#808080",
    shrink: 1,
    internal: true,
    ...over,
  });

  it("splits a vertex per quadrangle", async () => {
    // Corners cannot be shared: each one carries its quadrangle's color and
    // is pulled towards its quadrangle's center.
    const data = buildSurface(await cube(), options());
    expect(data.positions.length / 3).toBe(6 * 4);
    expect(data.indices.length).toBe(6 * 6);
    expect(data.colors.length / 4).toBe(6 * 4);
  });

  it("uses one color, or one per name", async () => {
    const view = await cube();
    const distinct = (d: ReturnType<typeof buildSurface>) => {
      const seen = new Set<string>();
      for (let v = 0; v < d.colors.length; v += 4) seen.add(d.colors.slice(v, v + 3).join(","));
      return seen.size;
    };
    expect(distinct(buildSurface(view, options()))).toBe(1);
    expect(distinct(buildSurface(view, options({ coloring: "id" })))).toBe(6);
    // Six faces, one block: by block it is one color whichever mode.
    expect(distinct(buildSurface(view, options({ by: "block", coloring: "id" })))).toBe(1);
  });

  it("leaves the geometry alone at a shrink of 1", async () => {
    const view = await cube();
    const data = buildSurface(view, options());
    // Every corner of the unit cube, rebased, is at ±0.5 on each axis.
    for (const x of data.positions) expect(Math.abs(x)).toBeCloseTo(0.5, 6);
  });

  it("pulls a face towards its own center, and a block towards the block's", async () => {
    const view = await cube();
    const faces = buildSurface(view, options({ shrink: 0.5 }));
    const blocks = buildSurface(view, options({ by: "block", shrink: 0.5 }));

    // A face shrinks in its own plane: the axis it is flat along does not
    // move, the other two halve. So one coordinate per corner stays at 0.5.
    for (let v = 0; v < faces.positions.length; v += 3) {
      const p = [faces.positions[v], faces.positions[v + 1], faces.positions[v + 2]];
      expect(p.filter((x) => Math.abs(Math.abs(x) - 0.5) < 1e-6)).toHaveLength(1);
      expect(p.filter((x) => Math.abs(Math.abs(x) - 0.25) < 1e-6)).toHaveLength(2);
    }
    // A block shrinks towards one point, so every coordinate halves.
    for (const x of blocks.positions) expect(Math.abs(x)).toBeCloseTo(0.25, 6);
  });

  it("collapses to nothing at a shrink of 0, and never moves the center", async () => {
    const view = await cube();
    const blocks = buildSurface(view, options({ by: "block", shrink: 0 }));
    for (const x of blocks.positions) expect(x).toBeCloseTo(0, 6);
  });

  it("has nothing to draw when there is nothing to draw", async () => {
    const empty = buildSurface(await cube(), options({ internal: false, by: "face" }));
    expect(empty.indices.length).toBe(6 * 6); // a free block is all boundary
    const shared = buildSurface(sharedFaceView(), options({ internal: false }));
    expect(shared.indices.length).toBe(0);
  });
});

describe("shrinking the edges", () => {
  it("pulls each one towards its own midpoint", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    const view = decodeView(kernel.view());

    const whole = edgeSegments(view, 1);
    const half = edgeSegments(view, 0.5);
    expect(whole.length).toBe(12 * 6);

    for (let s = 0; s < whole.length; s += 6) {
      for (let axis = 0; axis < 3; axis++) {
        const mid = (whole[s + axis] + whole[s + 3 + axis]) / 2;
        // The midpoint is where it was, and both ends are half as far from it.
        expect((half[s + axis] + half[s + 3 + axis]) / 2).toBeCloseTo(mid, 6);
        expect(half[s + axis] - mid).toBeCloseTo((whole[s + axis] - mid) / 2, 6);
      }
    }
  });
});

describe("the surfaces in the scene", () => {
  const show = async (prefs: Parameters<Viewer["applyPrefs"]>[0]) => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.applyPrefs(prefs);
    viewer.setView(decodeView(kernel.view()));
    return viewer;
  };

  it("draws the faces and leaves the blocks out by default", async () => {
    const viewer = await show(DEFAULTS);
    expect(viewer.surface("faces")?.quads).toBe(6);
    expect(viewer.surface("blocks")).toBeNull();
  });

  it("draws both at once when both are asked for", async () => {
    // Two colorings of one shape, which is the point of having both.
    const viewer = await show({
      ...DEFAULTS,
      faces: { ...DEFAULTS.faces, visible: true, coloring: "id" },
      blocks: { ...DEFAULTS.blocks, visible: true, coloring: "uniform" },
    });
    expect(viewer.surface("faces")?.colors).toBe(6);
    expect(viewer.surface("blocks")?.colors).toBe(1);
  });

  it("puts the transparency on the material, not in the colors", async () => {
    const viewer = await show({
      ...DEFAULTS,
      faces: { ...DEFAULTS.faces, alpha: 0.25 },
    });
    expect(viewer.surface("faces")?.alpha).toBeCloseTo(0.25, 9);
  });

  it("lets a surface answer a pick, because a face is chosen with the mouse", async () => {
    // This was the other way round until faces could be glued. The handles
    // still win where the two overlap — a sphere is centerd on the surface,
    // so its near half is in front — and the end-to-end test "a surface never
    // swallows the node under it" is what holds that down.
    const viewer = await show({
      ...DEFAULTS,
      blocks: { ...DEFAULTS.blocks, visible: true },
    });
    for (const group of ["faces", "blocks"] as const) {
      expect(viewer.meshCount(group)).toBeGreaterThan(0);
      for (let i = 0; i < viewer.meshCount(group); i++) {
        expect(viewer.scene.getMeshByName(`${group}-${i}`)?.isPickable ?? true).toBe(true);
      }
    }
  });

  it("draws one mesh per block, so that they can be sorted", async () => {
    // Babylon sorts transparent meshes back to front and does nothing about
    // the triangles inside one. A single mesh for the lot left it nothing
    // to sort, and a block behind another could be drawn over it.
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    for (const x of [0, 2, 4]) {
      kernel.apply({ op: "create_cube", origin: [x, 0, 0], size: 1 });
    }
    viewer.applyPrefs({ ...DEFAULTS, blocks: { ...DEFAULTS.blocks, visible: true } });
    viewer.setView(decodeView(kernel.view()));

    expect(viewer.meshCount("faces")).toBe(3);
    expect(viewer.meshCount("blocks")).toBe(3);
    // And the group still reports as one surface, which is what it is.
    expect(viewer.surface("faces")!.quads).toBe(18);
  });

  it("knows which face each drawn quadrangle belongs to, on both meshes", async () => {
    // Picking has to answer "which face" even when what is on screen is the
    // blocks, so both meshes carry both names.
    const viewer = await show({
      ...DEFAULTS,
      blocks: { ...DEFAULTS.blocks, visible: true },
    });
    const faces = viewer.surface("faces")!;
    const blocks = viewer.surface("blocks")!;
    expect(new Set(faces.faces).size).toBe(6);
    expect(new Set(blocks.faces).size).toBe(6);
    expect(new Set(blocks.blocks).size).toBe(1);
    // The same six faces, drawn twice.
    expect([...faces.faces].sort()).toEqual([...blocks.faces].sort());
  });

  it("follows a node being dragged", async () => {
    const viewer = await show(DEFAULTS);
    const before = viewer.surface("faces")!.positions.slice();
    const node = viewer.scene.getMeshByName("node0")!;
    node.position.x += 3;
    // What the drag behavior calls on every mouse move.
    (viewer as unknown as { preview: (i: number, to: Vector3) => void }).preview(
      0,
      node.position,
    );
    const after = viewer.surface("faces")!.positions;
    expect(after.length).toBe(before.length);
    expect(Array.from(after)).not.toEqual(Array.from(before));
  });

  it("takes a setting changed after the view arrived", async () => {
    const viewer = await show(DEFAULTS);
    expect(viewer.surface("blocks")).toBeNull();
    viewer.applyPrefs({
      ...DEFAULTS,
      blocks: { ...DEFAULTS.blocks, visible: true, shrink: 0.5 },
    });
    expect(viewer.surface("blocks")?.quads).toBe(6);
    for (const x of viewer.surface("blocks")!.positions) {
      expect(Math.abs(x)).toBeCloseTo(0.25, 6);
    }
  });
});

describe("preferences that outlive a version", () => {
  const store = new Map<string, string>();
  beforeAll(() => {
    (globalThis as unknown as { localStorage: Storage }).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
      key: () => null,
      length: 0,
    } as Storage;
  });

  it("fills in what an older version never wrote", () => {
    // Exactly what every stored copy held the day faces and blocks arrived.
    store.set("gecko.view", JSON.stringify({ theme: { background: "#000000" }, axes: false }));
    const prefs = loadPrefs();
    expect(prefs.theme.background).toBe("#000000");
    expect(prefs.axes).toBe(false);
    expect(prefs.faces).toEqual(DEFAULTS.faces);
    expect(prefs.blocks).toEqual(DEFAULTS.blocks);
  });

  it("clamps what someone else wrote", () => {
    store.set("gecko.view", JSON.stringify({ faces: { alpha: 40, shrink: -2 } }));
    const prefs = loadPrefs();
    expect(prefs.faces.alpha).toBe(1);
    expect(prefs.faces.shrink).toBe(0);
  });

  it("clamps the two shares to a ceiling of their own", () => {
    // Not fractions of one, so the usual clamp is the wrong one: a snap of
    // 80% of the model would take every node to the nearest anything.
    store.set("gecko.view", JSON.stringify({ box: { margin: 9, snap: -1 } }));
    const prefs = loadPrefs();
    expect(prefs.box.margin).toBe(MAX_BOX_MARGIN);
    expect(prefs.box.snap).toBe(0);
  });

  it("keeps the folds that were open, and ignores a name it does not know", () => {
    store.set("gecko.view", JSON.stringify({ panels: ["sec-colors", "grp-faces", 7] }));
    // Not checked against a list: a fold added later would need a migration
    // for nothing, and an id nobody recognizes is simply not on screen.
    expect(loadPrefs().panels).toEqual(["sec-colors", "grp-faces"]);
    store.set("gecko.view", JSON.stringify({ panels: "all of them" }));
    expect(loadPrefs().panels).toEqual(DEFAULTS.panels);
  });

  it("survives nonsense in storage", () => {
    store.set("gecko.view", "{ not json");
    expect(loadPrefs()).toEqual(DEFAULTS);
  });
});

describe("changing a setting without rebuilding the scene", () => {
  const shown = async (blocks = false) => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.applyPrefs({
      ...DEFAULTS,
      blocks: { ...DEFAULTS.blocks, visible: blocks },
    });
    viewer.setView(decodeView(kernel.view()));
    return viewer;
  };

  /** Identity, not just presence: a mesh thrown away and rebuilt has a new
   *  one, and that is the thing being tested. */
  const idOf = (viewer: Viewer, name: string) =>
    viewer.scene.getMeshByName(name)?.uniqueId ?? null;

  it("keeps the same mesh when only the shrink moves", async () => {
    // Dragging a slider is one of these per mouse move. Disposing two meshes
    // and two materials each time is what makes the model blink.
    const viewer = await shown();
    const before = idOf(viewer, "faces");
    const positions = viewer.surface("faces")!.positions.slice();

    for (const shrink of [0.9, 0.8, 0.7, 0.6]) {
      viewer.applyPrefs({ ...DEFAULTS, faces: { ...DEFAULTS.faces, shrink } });
    }
    expect(idOf(viewer, "faces")).toBe(before);
    // Same mesh, and it really did move.
    expect(Array.from(viewer.surface("faces")!.positions)).not.toEqual(
      Array.from(positions),
    );
  });

  it("keeps the same mesh when only the transparency or the coloring changes", async () => {
    const viewer = await shown();
    const before = idOf(viewer, "faces");
    viewer.applyPrefs({
      ...DEFAULTS,
      faces: { ...DEFAULTS.faces, alpha: 0.1, coloring: "id" },
    });
    expect(idOf(viewer, "faces")).toBe(before);
    expect(viewer.surface("faces")!.alpha).toBeCloseTo(0.1, 9);
    expect(viewer.surface("faces")!.colors).toBe(6);
  });

  it("keeps the edges when only their shrink moves", async () => {
    const viewer = await shown();
    const before = idOf(viewer, "edges");
    viewer.applyPrefs({ ...DEFAULTS, edges: { visible: true, shrink: 0.5, coloring: "uniform", width: DEFAULTS.edges.width } });
    expect(idOf(viewer, "edges")).toBe(before);
    expect(viewer.edgeCount).toBe(12);
  });

  it("puts the wireframe away when it is switched off", async () => {
    // Its own switch, rather than a shrink of nearly nothing standing in
    // for one: a wireframe pulled to a tenth is still a wireframe, just a
    // dotted one, and there was no way to ask for none at all.
    const viewer = await shown();
    viewer.applyPrefs({ ...DEFAULTS, edges: { visible: false, shrink: 1, coloring: "uniform", width: DEFAULTS.edges.width } });
    expect(viewer.edgeCount).toBe(0);
    expect(viewer.scene.getMeshByName("edges")).toBeNull();

    viewer.applyPrefs({ ...DEFAULTS, edges: { visible: true, shrink: 1, coloring: "uniform", width: DEFAULTS.edges.width } });
    expect(viewer.edgeCount).toBe(12);
  });

  it("still throws the mesh away when the shape really changes", async () => {
    // Hiding is hiding, and a different number of quadrangles is a different
    // mesh. The point is not to never rebuild, it is to never rebuild for
    // nothing.
    const viewer = await shown();
    viewer.applyPrefs({ ...DEFAULTS, faces: { ...DEFAULTS.faces, visible: false } });
    expect(idOf(viewer, "faces")).toBeNull();
    expect(viewer.surface("faces")).toBeNull();
  });
});

describe("how close the camera may get", () => {
  it("zooms by a share of the distance rather than a fixed step", async () => {
    // A fixed step is the same number of world units however near you are,
    // so one notch close in leaps through the model and out the far side.
    const viewer = new Viewer(new NullEngine());
    expect(viewer.camera.wheelDeltaPercentage).toBe(ZOOM_PER_NOTCH);
    expect(viewer.camera.wheelDeltaPercentage).toBeGreaterThan(0);
  });

  it("stops short of the model rather than passing through it", async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    // No frameAll: a station tab is told the kernel moved and redraws
    // without reframing, and the limits still have to be right.
    viewer.setView(decodeView(kernel.view()));

    const diagonal = Math.sqrt(3);
    expect(viewer.camera.lowerRadiusLimit).toBeCloseTo(diagonal * CLOSEST_APPROACH, 5);
    // Room to zoom in, and never as far as the center it orbits.
    expect(viewer.camera.lowerRadiusLimit!).toBeGreaterThan(0);
    expect(viewer.camera.lowerRadiusLimit!).toBeLessThan(viewer.camera.radius);
  });

  it("leaves the limits off when there is nothing to scale them to", async () => {
    // One node has no diagonal; a limit of zero would be worse than none.
    const viewer = new Viewer(new NullEngine());
    viewer.setView({
      center: [0, 0, 0],
      positions: new Float32Array([0, 0, 0]),
      names: new Uint32Array([1]),
      nodeClassDim: new Int32Array([-1]),
      nodeClassTag: new Int32Array([0]),
      nodeFrozen: new Uint32Array([0]),
      edgeClassDim: new Int32Array([]),
      edgeClassTag: new Int32Array([]),
      quadClassDim: new Int32Array([]),
      quadClassTag: new Int32Array([]),
      edges: new Uint32Array(0),
      edgeNames: new Uint32Array(0),
      quads: new Uint32Array(0),
      quadFaces: new Uint32Array(0),
      quadBlocks: new Uint32Array(0),
      nodeGroupsLo: new Uint32Array([0]),
      nodeGroupsHi: new Uint32Array([0]),
      nodeLoose: new Uint32Array([0]),
      edgeGroupsLo: new Uint32Array(0),
      edgeGroupsHi: new Uint32Array(0),
      edgeLoose: new Uint32Array(0),
      quadGroupsLo: new Uint32Array(0),
      quadGroupsHi: new Uint32Array(0),
    });
    viewer.frameAll();
    expect(viewer.camera.lowerRadiusLimit).toBeNull();
    expect(viewer.camera.upperRadiusLimit).toBeNull();
    expect(viewer.camera.radius).toBe(4);
  });
});

describe("picking faces to glue", () => {
  const twoCubes = async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    kernel.apply({ op: "create_cube", origin: [1, 0, 0], size: 1 });
    viewer.applyPrefs(DEFAULTS);
    viewer.setView(decodeView(kernel.view()));
    return { viewer, kernel };
  };

  it("paints the two picked faces apart from each other", async () => {
    // Which is which is not decoration: the first one's position is what
    // survives the glue.
    const { viewer } = await twoCubes();
    const surface = viewer.surface("faces")!;
    const picks = [...surface.faces].map((face, i) => ({
      face,
      block: surface.blocks[i],
    }));
    expect(viewer.surface("faces")!.colors).toBe(1);

    viewer.select([picks[0]]);
    expect(viewer.selection).toEqual([picks[0]]);
    expect(viewer.surface("faces")!.colors).toBe(2);

    viewer.select([picks[0], picks[1]]);
    // The rest, the anchor, and the one coming to meet it.
    expect(viewer.surface("faces")!.colors).toBe(3);

    viewer.select([]);
    expect(viewer.surface("faces")!.colors).toBe(1);
  });

  it("shows the selection on the blocks as well as the faces", async () => {
    const { viewer } = await twoCubes();
    viewer.applyPrefs({ ...DEFAULTS, blocks: { ...DEFAULTS.blocks, visible: true } });
    const surface = viewer.surface("faces")!;
    viewer.select([{ face: surface.faces[0], block: surface.blocks[0] }]);
    // Keyed by face, so one pick lights up wherever that face is drawn.
    expect(viewer.surface("blocks")!.colors).toBeGreaterThan(1);
  });

  it("glues, and the model becomes one object", async () => {
    const { viewer, kernel } = await twoCubes();
    const view = decodeView(kernel.view());

    // The two faces that meet at x = 1, found the way a mouse would: by
    // where they are.
    const centerX = (quad: number) => {
      let x = 0;
      for (let k = 0; k < 4; k++) x += view.positions[view.quads[quad * 4 + k] * 3] / 4;
      return x + view.center[0];
    };
    const meeting = [...view.quadFaces.keys()].filter((q) => Math.abs(centerX(q) - 1) < 1e-6);
    expect(meeting).toHaveLength(2);

    const reply = kernel.apply({
      op: "glue_faces",
      first: view.quadFaces[meeting[0]],
      second: view.quadFaces[meeting[1]],
    });
    expect(reply.ok, reply.message).toBe(true);

    const after = decodeView(kernel.view());
    expect(after.positions.length / 3).toBe(12);
    expect(after.edges.length / 2).toBe(20);
    expect(after.quads.length / 4).toBe(12);

    // And the viewer can now hide something: one face has a block on each
    // side, which until this operation existed never happened.
    viewer.setView(after);
    expect(viewer.surface("faces")!.quads).toBe(11);
    viewer.applyPrefs({ ...DEFAULTS, faces: { ...DEFAULTS.faces, internal: false } });
    expect(viewer.surface("faces")!.quads).toBe(10);
  });
});

describe("the frame a screen position is reported in", () => {
  /** The same scene, on an engine that reports a backing store twice the
   *  size of its canvas — what a retina display gives you. `NullEngine`
   *  ignores `setHardwareScalingLevel`, so the two numbers that matter are
   *  stubbed directly rather than asked for. */
  const viewerOn = async (retina: boolean) => {
    const engine = new NullEngine();
    if (retina) {
      const [w, h] = [engine.getRenderWidth(), engine.getRenderHeight()];
      Object.assign(engine, {
        getRenderWidth: () => w * 2,
        getRenderHeight: () => h * 2,
        getHardwareScalingLevel: () => 0.5,
      });
    }
    const viewer = new Viewer(engine);
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.applyPrefs(DEFAULTS);
    viewer.setView(decodeView(kernel.view()));
    viewer.frameAll();
    viewer.render();
    return viewer;
  };

  const reported = (viewer: Viewer) => ({
    node: viewer.nodeScreen(viewer.names()[0])!,
    face: viewer.faceScreen(viewer.surface("faces")!.faces[0])!,
    axis: viewer.axes.axisScreen("+x")!,
  });

  it("is CSS pixels, not the backing store", async () => {
    // `scene.pick` and a pointer event's `offsetX` are both in CSS pixels,
    // and `Vector3.Project` works in the backing store. The two coincide at a
    // device pixel ratio of 1 — which is what this runner and Playwright both
    // use, so nothing else here can tell them apart — and are a factor of two
    // apart on the machine this is used on, where aiming at what these report
    // would miss by half the window.
    const plain = reported(await viewerOn(false));
    const retina = reported(await viewerOn(true));

    for (const key of ["node", "face", "axis"] as const) {
      for (let k = 0; k < 2; k++) {
        expect(
          retina[key][k],
          `${key}[${k}] moved when only the backing store changed`,
        ).toBeCloseTo(plain[key][k], 6);
      }
    }
    // And the scene really was twice the size, or the above proves nothing.
    expect((await viewerOn(true)).scene.getEngine().getRenderWidth()).toBe(
      (await viewerOn(false)).scene.getEngine().getRenderWidth() * 2,
    );
  });
});

describe("the keyboard", () => {
  const press = (over: Partial<Parameters<typeof actionFor>[0]>) =>
    actionFor({
      key: "",
      code: "",
      shiftKey: false,
      ctrlKey: false,
      metaKey: false,
      altKey: false,
      ...over,
    });

  it("has a key for every action, and no key for two", () => {
    // Every action reachable, which is what makes the help card the whole
    // story. The other half — every action implemented — is the compiler's,
    // through the `Record<Action, …>` the toolbar and the keys share.
    for (const action of ACTIONS) {
      expect(
        SHORTCUTS.some((s) => s.action === action),
        `${action} has no key`,
      ).toBe(true);
    }

    const seen = new Set<string>();
    for (const shortcut of SHORTCUTS) {
      const token =
        `${shortcut.mod ? "mod+" : ""}${shortcut.shift ? "shift+" : ""}` +
        `${shortcut.key ?? shortcut.code}`;
      expect(seen.has(token), `${token} is bound twice`).toBe(false);
      seen.add(token);
    }
  });

  it("reads a letter by its character", () => {
    expect(press({ key: "c", code: "KeyC" })).toBe("cube");
    expect(press({ key: "C", code: "KeyC" })).toBe("cube");
    expect(press({ key: "x", code: "KeyX" })).toBe("axis-x");
    expect(press({ key: "x", code: "KeyX", shiftKey: true })).toBe("axis--x");
  });

  it("reads a digit by its position", () => {
    // On a French keyboard the digit row is shifted: the key printed `1`
    // produces `&`. Matching the character would make the help card a lie
    // there, while the physical key under the printed digit is the same one.
    expect(press({ key: "&", code: "Digit1" })).toBe("faces");
    expect(press({ key: "1", code: "Digit1" })).toBe("faces");
    expect(press({ key: "é", code: "Digit2" })).toBe("blocks");
  });

  it("leaves the browser's own shortcuts alone", () => {
    for (const held of ["ctrlKey", "metaKey", "altKey"] as const) {
      expect(press({ key: "s", code: "KeyS", [held]: true })).toBeNull();
    }
    expect(press({ key: "s", code: "KeyS" })).toBe("screenshot");
  });

  it("has a key for each of the two panels", () => {
    // They are two different questions — how it is drawn, and what the
    // operations do — so they are two panels and two keys.
    expect(press({ key: "Tab", code: "Tab" })).toBe("drawer");
    expect(press({ key: ",", code: "Comma" })).toBe("params");
  });

  it("takes ⌘Z and ctrl Z, and leaves bare Z to the view", () => {
    // The one shortcut with a modifier, because it is the one everybody
    // already knows. Bare Z is a view down the Z axis and must stay one.
    expect(press({ key: "z", code: "KeyZ", metaKey: true })).toBe("undo");
    expect(press({ key: "z", code: "KeyZ", ctrlKey: true })).toBe("undo");
    expect(press({ key: "Z", code: "KeyZ", metaKey: true })).toBe("undo");
    expect(press({ key: "z", code: "KeyZ" })).toBe("axis-z");
    // Alt is how several keyboards type the characters above the digits, so
    // it is left out of the table altogether — ⌥⌘Z included.
    expect(press({ key: "z", code: "KeyZ", metaKey: true, altKey: true })).toBeNull();
    // And a modifier on a key that does not ask for one is the browser's.
    expect(press({ key: "c", code: "KeyC", metaKey: true })).toBeNull();
  });

  it("leaves typing alone", () => {
    // The color inputs and the sliders both take characters and arrow keys.
    // A slider that jumped to a new view instead of nudging would be unusable.
    for (const tagName of ["INPUT", "TEXTAREA", "SELECT"]) {
      expect(press({ key: "c", code: "KeyC", target: { tagName } })).toBeNull();
    }
    expect(press({ key: "c", code: "KeyC", target: { isContentEditable: true } })).toBeNull();
    expect(press({ key: "c", code: "KeyC", target: { tagName: "CANVAS" } })).toBe("cube");
  });

  it("folds a key with nothing to add onto the row above it", () => {
    const rows = helpRows();
    const remove = rows.find((r) => r.does.includes("remove"))!;
    expect(remove.keys).toEqual(["Del", "⌫"]);
    expect(rows.every((r) => r.does !== "")).toBe(true);
  });

});

describe("saving a picture", () => {
  it("names it by the moment it was taken", () => {
    expect(shotName(new Date(2026, 9, 9, 18, 30, 12))).toBe("gecko-2026-10-09-183012.png");
    // Padded, so the names sort the way the pictures were taken.
    expect(shotName(new Date(2026, 0, 2, 3, 4, 5))).toBe("gecko-2026-01-02-030405.png");
  });

  it("turns a data url back into its bytes", () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47]);
    const url = `data:image/png;base64,${Buffer.from(png).toString("base64")}`;
    const blob = dataUrlToBlob(url);
    expect(blob.type).toBe("image/png");
    expect(blob.size).toBe(4);
  });
});

describe("the interface's own colors", () => {
  it("follow the scene rather than fighting it", () => {
    expect(isLight(PRESETS.light.background)).toBe(true);
    expect(isLight(PRESETS.dark.background)).toBe(false);
    // Weighted the way the eye is: green carries most of the luminance, so a
    // saturated green reads as light and a saturated blue does not.
    expect(isLight("#00ff00")).toBe(true);
    expect(isLight("#0000ff")).toBe(false);
  });

  it("remembers whether the drawer was open, and how a picture comes out", () => {
    const store = new Map<string, string>();
    (globalThis as unknown as { localStorage: Storage }).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
      key: () => null,
      length: 0,
    } as Storage;

    store.set("gecko.view", JSON.stringify({ drawer: true, shot: { scale: 4 } }));
    const prefs = loadPrefs();
    expect(prefs.drawer).toBe(true);
    expect(prefs.chrome).toBe(true);
    expect(prefs.shot.scale).toBe(4);

    // A size nobody offered is a gigapixel the browser would refuse to make.
    store.set("gecko.view", JSON.stringify({ shot: { scale: 40 } }));
    expect(loadPrefs().shot.scale).toBe(DEFAULTS.shot.scale);
    expect(SHOT_SCALES).toContain(DEFAULTS.shot.scale);
  });
});

describe("lighting up a sheet", () => {
  const cube = async () => {
    const viewer = new Viewer(new NullEngine());
    const kernel = new WasmKernel();
    kernel.apply({ op: "create_cube", origin: [0, 0, 0], size: 1 });
    viewer.applyPrefs(DEFAULTS);
    viewer.setView(decodeView(kernel.view()));
    return { viewer, kernel };
  };

  it("draws the lit edges as their own thing, and takes it away again", async () => {
    // A ribbon, not a line: WebGL caps `lineWidth` at one pixel whatever is
    // asked of it, so a thick line has to be geometry. Built headless here
    // as well, which is the cheap way to find out it still builds.
    const { viewer } = await cube();
    expect(viewer.scene.getMeshByName("sheet")).toBeNull();

    const names = viewer.edgeNames().slice(0, 4);
    viewer.showSheet(names.map((id) => ({ id, from: [0, 0, 0], to: [1, 0, 0] }) as const));
    expect(viewer.highlighted).toEqual(names);
    expect(viewer.scene.getMeshByName("sheet")).not.toBeNull();

    viewer.showSheet([]);
    expect(viewer.scene.getMeshByName("sheet")).toBeNull();
  });

  it("keeps the lit edges when the view is redrawn under it", async () => {
    // A node dragged, a setting changed: the sheet is still the sheet.
    const { viewer, kernel } = await cube();
    const names = viewer.edgeNames().slice(0, 2);
    viewer.showSheet(names.map((id) => ({ id, from: [0, 0, 0], to: [1, 0, 0] }) as const));

    viewer.setView(decodeView(kernel.view()));
    expect(viewer.highlighted).toEqual(names);
    expect(viewer.scene.getMeshByName("sheet")).not.toBeNull();

    viewer.applyPrefs({ ...DEFAULTS, edges: { visible: true, shrink: 0.4, coloring: "uniform", width: DEFAULTS.edges.width } });
    expect(viewer.scene.getMeshByName("sheet")).not.toBeNull();
  });

  it("names an edge from where it is, and finds where it is from its name", async () => {
    const { viewer } = await cube();
    for (const name of viewer.edgeNames()) {
      const mid = viewer.edgeMidpoint(name)!;
      // The unit cube: every edge midpoint has one coordinate at a half and
      // the other two at a corner.
      expect(mid.filter((x) => Math.abs(x - 0.5) < 1e-6).length).toBeGreaterThanOrEqual(1);
    }
    expect(viewer.edgeMidpoint(999_999)).toBeNull();
  });
});

/**
 * A unit square: two triangles on one surface, four sides each a curve, four
 * corners each a point element. Written out rather than loaded, because what
 * each line means is the point of the tests below.
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

async function withGeometry() {
  const kernel = new WasmKernel();
  const reply = kernel.apply({ op: "load_geometry", msh: SQUARE_MSH, name: "square.msh" });
  expect(reply.ok, reply.message).toBe(true);
  return kernel;
}

describe("the geometry's own buffer", () => {
  it("decodes what the kernel encodes", async () => {
    const kernel = await withGeometry();
    const geometry = decodeGeometry(kernel.geometry());

    expect(geometry.positions.length / 3).toBe(4);
    expect(geometry.triangles.length / 3).toBe(2);
    expect(Array.from(geometry.triangleTags)).toEqual([1, 1]);
    expect(Array.from(geometry.segmentTags)).toEqual([11, 12, 13, 14]);
    expect(Array.from(geometry.cornerTags)).toEqual([21, 22, 23, 24]);
    // Rebased like everything else: the square's own center is the origin.
    expect(geometry.center).toEqual([0.5, 0.5, 0]);
    expect(Array.from(geometry.positions.slice(0, 3))).toEqual([-0.5, -0.5, 0]);
  });

  it("is only asked for when the kernel says it changed", async () => {
    // The whole reason for a generation number rather than a flag on an
    // outcome: it is just as right for a change this client did not make.
    const kernel = new WasmKernel();
    expect((kernel.stats()).geometry.generation).toBe(0);
    expect((kernel.stats()).geometry.surfaces).toBeUndefined();

    kernel.apply({ op: "load_geometry", msh: SQUARE_MSH, name: "square.msh" });
    expect((kernel.stats()).geometry).toEqual({
      generation: 1,
      corners: 4,
      curves: 4,
      surfaces: 1,
      segments: 4,
      triangles: 2,
    });

    kernel.apply({ op: "clear_geometry" });
    expect((kernel.stats()).geometry.generation).toBe(2);
  });

  it("hands back something decodable even when there is no geometry", async () => {
    const geometry = decodeGeometry(await new WasmKernel().geometry());
    expect(geometry.positions.length).toBe(0);
    expect(geometry.triangles.length).toBe(0);
  });

  it("refuses a buffer whose header and body disagree", async () => {
    const kernel = await withGeometry();
    const bytes = kernel.geometry();
    expect(() => decodeGeometry(bytes.slice(0, bytes.length - 4))).toThrow(/bytes/);
    const wrong = new Uint8Array(bytes);
    wrong[4] = 9;
    expect(() => decodeGeometry(wrong)).toThrow(/version 9/);
  });
});

describe("a geometry made into meshes", () => {
  it("gathers triangles into one surface per tag, sharing their nodes", async () => {
    const kernel = await withGeometry();
    const view = decodeGeometry(kernel.geometry());
    const surfaces = geomSurfaces(view);

    expect(surfaces).toHaveLength(1);
    expect(surfaces[0].tag).toBe(1);
    expect(surfaces[0].indices.length / 3).toBe(2);
    // Four vertices for two triangles, not six: the two share an edge, and
    // sharing is what makes the normals average across a surface — a
    // faceted sphere reading as a sphere rather than as its facets.
    expect(surfaces[0].positions.length / 3).toBe(4);
  });

  it("keeps each curve and each corner its own thing", async () => {
    const kernel = await withGeometry();
    const view = decodeGeometry(kernel.geometry());

    const curves = geomCurves(view);
    expect(curves.map((c) => c.tag)).toEqual([11, 12, 13, 14]);
    expect(curves.every((c) => c.segments.length === 6)).toBe(true);
    expect(geomCorners(view).map((c) => c.tag)).toEqual([21, 22, 23, 24]);
  });

  it("reports the extent the camera has to frame", async () => {
    const kernel = await withGeometry();
    const box = geomBounds(decodeGeometry(kernel.geometry()))!;
    expect(box.lo).toEqual([-0.5, -0.5, 0]);
    expect(box.hi).toEqual([0.5, 0.5, 0]);
    expect(geomBounds(decodeGeometry(await new WasmKernel().geometry()))).toBeNull();
  });
});

describe("the geometry in the scene", () => {
  async function scene() {
    const kernel = await withGeometry();
    const viewer = new Viewer(new NullEngine());
    viewer.setView(decodeView(kernel.view()));
    viewer.setGeometry(decodeGeometry(kernel.geometry()));
    return viewer;
  }

  it("draws one mesh per entity, so each can be colored and faded alone", async () => {
    const viewer = await scene();
    expect(viewer.geomCount("surfaces")).toBe(1);
    expect(viewer.geomCount("curves")).toBe(4);
    expect(viewer.geomCount("corners")).toBe(4);
    expect(viewer.geomTags("curves")).toEqual([11, 12, 13, 14]);
  });

  it("hides each group on its own", async () => {
    const viewer = await scene();
    for (const group of GEOM_GROUPS) {
      const prefs = structuredClone(DEFAULTS);
      prefs[group].visible = false;
      viewer.applyPrefs(prefs);
      expect(viewer.geomCount(group), `${group} hidden`).toBe(0);
      // And the other two are untouched.
      for (const other of GEOM_GROUPS) {
        if (other !== group) expect(viewer.geomCount(other), `${other} still shown`).toBeGreaterThan(0);
      }
    }
  });

  it("colors a group one color, or one color per tag", async () => {
    const viewer = await scene();
    const prefs = structuredClone(DEFAULTS);
    prefs.curves.coloring = "uniform";
    prefs.theme.curves = "#123456";
    viewer.applyPrefs(prefs);
    expect(viewer.geomLook("curves", 11)!.color).toBe("#123456");
    expect(viewer.geomLook("curves", 12)!.color).toBe("#123456");

    prefs.curves.coloring = "id";
    viewer.applyPrefs(prefs);
    const eleven = viewer.geomLook("curves", 11)!.color;
    expect(eleven).not.toBe(viewer.geomLook("curves", 12)!.color);
    expect(eleven).toBe(colorForId(11, FAMILY.curves).toLowerCase());
    // And not the color a *face* of the blocking numbered 11 would be:
    // two different things with the same number must not look alike.
    expect(eleven).not.toBe(colorForId(11, FAMILY.faces).toLowerCase());
  });

  it("gives all three groups the color they were asked for", async () => {
    // Each keeps it somewhere different — a line has no material, a corner
    // is unlit and a surface is lit — so "the color it is drawn in" is
    // three different fields and worth asserting on each.
    const viewer = await scene();
    const prefs = structuredClone(DEFAULTS);
    for (const group of GEOM_GROUPS) {
      prefs[group].coloring = "uniform";
      prefs.theme[group] = "#abcdef";
    }
    viewer.applyPrefs(prefs);
    expect(viewer.geomLook("surfaces", 1)!.color).toBe("#abcdef");
    expect(viewer.geomLook("curves", 11)!.color).toBe("#abcdef");
    expect(viewer.geomLook("corners", 21)!.color).toBe("#abcdef");

    for (const group of GEOM_GROUPS) prefs[group].coloring = "id";
    viewer.applyPrefs(prefs);
    expect(viewer.geomLook("surfaces", 1)!.color).toBe(
      colorForId(1, FAMILY.surfaces).toLowerCase(),
    );
    expect(viewer.geomLook("corners", 21)!.color).toBe(
      colorForId(21, FAMILY.corners).toLowerCase(),
    );
  });

  it("fades a group without touching the others", async () => {
    const viewer = await scene();
    const prefs = structuredClone(DEFAULTS);
    prefs.surfaces.alpha = 0.25;
    prefs.curves.alpha = 1;
    viewer.applyPrefs(prefs);
    expect(viewer.geomLook("surfaces", 1)!.alpha).toBeCloseTo(0.25);
    expect(viewer.geomLook("curves", 11)!.alpha).toBeCloseTo(1);
  });

  it("is never pickable, so it cannot steal a click meant for a block", async () => {
    // The trap the node handles fell into when surfaces first became
    // pickable: a triangle in front of a face answered the click, and the
    // face behind it became unselectable.
    const viewer = await scene();
    for (const group of GEOM_GROUPS) {
      for (const tag of viewer.geomTags(group)) {
        const mesh = viewer.scene.getMeshByName(`geom-${group}-${tag}`)!;
        expect(mesh.isPickable, `geom-${group}-${tag}`).toBe(false);
      }
    }
  });

  it("counts towards what the camera frames", async () => {
    // A geometry with no blocks on it yet is still what there is to see.
    const kernel = await withGeometry();
    const viewer = new Viewer(new NullEngine());
    viewer.setView(decodeView(kernel.view()));
    const empty = viewer.camera.radius;
    viewer.frameAll(true);
    expect(viewer.camera.radius, "nothing to frame yet").toBe(empty);

    viewer.setGeometry(decodeGeometry(kernel.geometry()));
    viewer.frameAll(true);
    expect(viewer.camera.radius).toBeGreaterThan(1);
  });
});

describe("aiming an extrusion", () => {
  const Z: [number, number, number] = [0, 0, 1];
  const ORIGIN: [number, number, number] = [0, 0, 0];

  it("reads the height off the point of the axis nearest the ray", () => {
    // Looking along −y, the ray is a horizontal line at its own height, so
    // the nearest point of the z axis is at exactly that height.
    expect(heightAlong([0, 5, 3], [0, -1, 0], ORIGIN, Z)).toBeCloseTo(3);
    expect(heightAlong([0, 5, 0], [0, -1, 0], ORIGIN, Z)).toBeCloseTo(0);
    expect(heightAlong([0, 5, -2], [0, -1, 0], ORIGIN, Z)).toBeCloseTo(-2);
    // Measured from the face, not from the world: the axis starts where the
    // face is, so the same ray reads lower from a face higher up.
    expect(heightAlong([0, 5, 3], [0, -1, 0], [0, 0, 1], Z)).toBeCloseTo(2);
  });

  it("gives up when the ray runs down the extrusion", () => {
    // Looking straight along it, every distance is as plausible as every
    // other, and the closed form says so by dividing by nothing.
    expect(heightAlong([0, 0, 9], [0, 0, -1], ORIGIN, Z)).toBeNull();
    expect(heightAlong([1, 1, 9], [0, 0, 1], ORIGIN, Z)).toBeNull();
    // A hair off parallel is still an answer.
    const slant = Math.hypot(0.01, 1);
    expect(heightAlong([0, 1, 9], [0, -0.01 / slant, -1 / slant], ORIGIN, Z)).not.toBeNull();
  });
});

describe("growing a block out of a face", () => {
  async function cubeScene() {
    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: CUBE });
    const viewer = new Viewer(new NullEngine());
    viewer.setView(decodeView(kernel.view()));
    return { kernel, viewer };
  }

  it("takes a picked face and nothing else", async () => {
    const { viewer } = await cubeScene();
    const pick = viewer.surface("faces")!;
    const face = { face: pick.faces[0], block: pick.blocks[0] };
    expect(viewer.beginExtrusion(face)).toBe(true);
    expect(viewer.growingFrom).toEqual(face);
    // Nothing is asked for until the pointer moves: a press that has not
    // moved is a click, and a preview before the first movement would be a
    // block you have to undo by letting go of a button you already have.
    expect(viewer.grownBy).toBeNull();

    viewer.endExtrusion();
    expect(viewer.growingFrom).toBeNull();
    expect(viewer.beginExtrusion({ face: 999, block: 999 })).toBe(false);
  });

  it("keeps the preview's meshes while the pointer moves", async () => {
    // A drag is a stream of these, several per frame. Throwing two meshes
    // and two materials away each time is what made the preview flicker
    // and lag — the same lesson as the shrink sliders, which is why the
    // assertion is the same one: the mesh you end with is the mesh you
    // started with, and only its buffers changed.
    const { viewer } = await cubeScene();
    const id = () => viewer.scene.getMeshByName("grown")?.uniqueId ?? null;
    const pick = viewer.surface("faces")!;
    viewer.beginExtrusion({ face: pick.faces[0], block: pick.blocks[0] });
    expect(id(), "nothing drawn before the first movement").toBeNull();

    viewer.aimExtrusion(200, 120);
    const first = id();
    expect(first).not.toBeNull();
    const corners = () =>
      Array.from(viewer.scene.getMeshByName("grown")!.getVerticesData("position")!);
    const before = corners();

    for (const [x, y] of [
      [210, 110],
      [230, 90],
      [260, 60],
    ]) {
      viewer.aimExtrusion(x, y);
      expect(id(), "the mesh was thrown away and rebuilt").toBe(first);
    }
    // Rewritten, not merely kept: a preview that never moves is no better
    // than one that flickers.
    expect(corners()).not.toEqual(before);

    viewer.endExtrusion();
    expect(id()).toBeNull();
    expect(viewer.scene.getMeshByName("grown-edges")).toBeNull();
  });

  it("never asks for a block of no thickness", async () => {
    // Dragging the wrong way holds the preview at its thinnest rather than
    // letting it vanish: the kernel refuses a height of nought, and there
    // would be nothing on screen to tell you why.
    const { viewer } = await cubeScene();
    const pick = viewer.surface("faces")!;
    viewer.beginExtrusion({ face: pick.faces[0], block: pick.blocks[0] });
    for (const [x, y] of [
      [10, 10],
      [400, 300],
      [0, 0],
      [511, 255],
    ]) {
      const height = viewer.aimExtrusion(x, y);
      if (height !== null) expect(height).toBeGreaterThan(0);
    }
  });
});

describe("wearing its classification", () => {
  async function pinned() {
    const kernel = new WasmKernel();
    kernel.apply({ op: "load_geometry", msh: SQUARE_MSH, name: "square.msh" });
    kernel.apply({ op: "add_block", corners: CUBE });
    const viewer = new Viewer(new NullEngine());
    viewer.setView(decodeView(kernel.view()));
    viewer.setGeometry(decodeGeometry(kernel.geometry()));
    return { kernel, viewer };
  }

  const handle = (viewer: Viewer, at: number) =>
    (viewer.scene.getMeshByName(`node${at}`)!.material as StandardMaterial).emissiveColor
      .toHexString()
      .toLowerCase();

  async function redraw(kernel: WasmKernel, viewer: Viewer) {
    viewer.setView(decodeView(kernel.view()));
  }

  it("says a node's classification in the coloring that asks for it", async () => {
    // Classification coloring is a mode, not an override — it is where
    // nodes, edges and faces start, and `uniform` turns it off. Before it
    // was a mode it was on always, which painted a block face the color
    // of the very surface it is attached to and drew it on top of that
    // surface.
    const { kernel, viewer } = await pinned();
    const names = viewer.names();
    kernel.apply({ op: "classify_node", nodes: [names[0]], on: { dim: 1, tag: 11 } });
    await redraw(kernel, viewer);

    const plain = structuredClone(DEFAULTS);
    plain.nodes.coloring = "uniform";
    viewer.applyPrefs(plain);
    expect(handle(viewer, 0)).toBe(DEFAULTS.theme.nodes.toLowerCase());

    const prefs = structuredClone(DEFAULTS);
    prefs.nodes.coloring = "class";
    viewer.applyPrefs(prefs);
    expect(handle(viewer, 0)).toBe(DEFAULTS.theme.onCurve.toLowerCase());
    // One color per *kind* of entity, not per entity: a second
    // node on another curve reads the same.
    kernel.apply({ op: "classify_node", nodes: [names[1]], on: { dim: 1, tag: 12 } });
    await redraw(kernel, viewer);
    expect(handle(viewer, 1)).toBe(DEFAULTS.theme.onCurve.toLowerCase());
    // And a node on nothing keeps the group's own color, which is what
    // makes the coloring readable as an answer to "has this taken hold".
    expect(handle(viewer, 2)).toBe(DEFAULTS.theme.nodes.toLowerCase());

    // On a surface it is a third color again.
    kernel.apply({ op: "classify_node", nodes: [names[2]], on: { dim: 2, tag: 1 } });
    await redraw(kernel, viewer);
    expect(handle(viewer, 2)).toBe(DEFAULTS.theme.onSurface.toLowerCase());
  });

  it("colors a node by its own name when asked for that instead", async () => {
    const { viewer } = await pinned();
    const prefs = structuredClone(DEFAULTS);
    prefs.nodes.coloring = "id";
    viewer.applyPrefs(prefs);
    const names = viewer.names();
    expect(handle(viewer, 0)).toBe(colorForId(names[0], FAMILY.nodes).toLowerCase());
    expect(handle(viewer, 0)).not.toBe(handle(viewer, 1));
  });

  it("draws a frozen node as a cube, and leaves its color to the coloring", async () => {
    // A frozen node is a cell out of play, which is a thing about the cell
    // and not about its classification — so it is said with the *shape* of
    // the handle. Said with its color, as it once was, it hid the two
    // things the coloring is for: the node's id and its classification.
    const { kernel, viewer } = await pinned();
    const names = viewer.names();
    kernel.apply({ op: "classify_node", nodes: [names[0]], on: { dim: 2, tag: 1 } });
    kernel.apply({ op: "freeze_node", nodes: [names[0]], frozen: true });
    await redraw(kernel, viewer);

    // The shape says frozen, read off the mesh on screen.
    expect(viewer.nodeShape(names[0])).toBe("cube");
    expect(viewer.nodeShape(names[1])).toBe("ball");
    expect(viewer.nodeState(names[0])).toEqual({ on: { dim: 2, tag: 1 }, frozen: true });

    // And the color still says its classification, which is a surface.
    viewer.applyPrefs({ ...DEFAULTS, nodes: { ...DEFAULTS.nodes, coloring: "class" } });
    expect(handle(viewer, 0)).toBe(DEFAULTS.theme.onSurface.toLowerCase());
    // Or its id, or nothing at all, as the setting says — the frozen node
    // is treated like any other in each case.
    viewer.applyPrefs({ ...DEFAULTS, nodes: { ...DEFAULTS.nodes, coloring: "id" } });
    expect(handle(viewer, 0)).toBe(colorForId(names[0], FAMILY.nodes).toLowerCase());
    viewer.applyPrefs({ ...DEFAULTS, nodes: { ...DEFAULTS.nodes, coloring: "uniform" } });
    expect(handle(viewer, 0)).toBe(DEFAULTS.theme.nodes.toLowerCase());
  });

  it("will not let a frozen node be dragged at all", async () => {
    // The kernel refuses the move either way; without this the handle
    // would visibly go and come back, which reads as a bug rather than as
    // a locked node.
    const { kernel, viewer } = await pinned();
    const names = viewer.names();
    const dragging = () =>
      viewer.scene
        .getMeshByName("node0")!
        .behaviors.filter((b) => b.name === "PointerDrag")
        .map((b) => (b as unknown as { enabled: boolean }).enabled);

    expect(dragging()).toEqual([true]);
    kernel.apply({ op: "freeze_node", nodes: [names[0]], frozen: true });
    await redraw(kernel, viewer);
    expect(dragging()).toEqual([false]);
  });

  it("marks the picked node, over everything else it might be", async () => {
    const { kernel, viewer } = await pinned();
    const names = viewer.names();
    kernel.apply({ op: "freeze_node", nodes: [names[0]], frozen: true });
    await redraw(kernel, viewer);

    viewer.selectNodes([names[0]]);
    expect(viewer.selectedNodes).toEqual([names[0]]);
    expect(handle(viewer, 0)).toBe("#ffcc33");
    viewer.selectNodes([]);
    expect(handle(viewer, 0)).toBe(DEFAULTS.theme.nodes.toLowerCase());

    // Several at once, which is what the fast way onto a geometry needs:
    // pick the four corners, pick the curve, say so once.
    viewer.selectNodes([names[0], names[1], 9999]);
    expect(viewer.selectedNodes).toEqual([names[0], names[1]]);
    expect(handle(viewer, 0)).toBe("#ffcc33");
    expect(handle(viewer, 1)).toBe("#ffcc33");
  });

  it("colors the wireframe edge by edge", async () => {
    // One mesh for the lot, so this is a color per **segment** of the
    // ribbon rather than a color per mesh — one segment being one edge,
    // which is on one entity or on none.
    const { kernel, viewer } = await pinned();
    const names = viewer.names();
    const prefs = structuredClone(DEFAULTS);
    prefs.edges.coloring = "class";
    viewer.applyPrefs(prefs);
    const colors = () => {
      const material = viewer.scene.getMeshByName("edges")!
        .material as unknown as { colors: { r: number; g: number; b: number }[] | null };
      return (material.colors ?? []).map((c) => [c.r, c.g, c.b]);
    };
    const plain = colors();
    expect(plain).toHaveLength(12);

    // Two corners of the cube on one curve puts every edge between them
    // on that curve — and both of them in one command, which is how the
    // interface sends it.
    kernel.apply({
      op: "classify_node",
      nodes: [names[0], names[1]],
      on: { dim: 1, tag: 11 },
    });
    await redraw(kernel, viewer);
    expect(colors()).not.toEqual(plain);
  });
});

describe("four views of one scene", () => {
  it("lays the panes out in the order they are named", () => {
    // 3D and X across the top, Y and Z beneath — and Babylon measures a
    // viewport's y from the bottom, which is the one thing a full-screen
    // viewport lets you get away with not knowing.
    expect(paneViewport("3d", true)).toEqual({ x: 0, y: 0.5, w: 0.5, h: 0.5 });
    expect(paneViewport("x", true)).toEqual({ x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
    expect(paneViewport("y", true)).toEqual({ x: 0, y: 0, w: 0.5, h: 0.5 });
    expect(paneViewport("z", true)).toEqual({ x: 0.5, y: 0, w: 0.5, h: 0.5 });
    // Unsplit, the 3-D view has the lot.
    expect(paneViewport("3d", false)).toEqual({ x: 0, y: 0, w: 1, h: 1 });

    // The same rectangles counted from the top left, which is the frame a
    // pointer speaks in.
    expect(paneRect("3d", true, 800, 600)).toEqual({ x: 0, y: 0, width: 400, height: 300 });
    expect(paneRect("z", true, 800, 600)).toEqual({ x: 400, y: 300, width: 400, height: 300 });
    expect(paneRect("3d", false, 800, 600)).toEqual({ x: 0, y: 0, width: 800, height: 600 });
  });

  it("says which pane a point is in", () => {
    for (const [x, y, pane] of [
      [10, 10, "3d"],
      [790, 10, "x"],
      [10, 590, "y"],
      [790, 590, "z"],
      // The lines themselves belong to the pane below and to the right,
      // which is what a strict comparison gives and is as good as either.
      [400, 300, "z"],
      [399, 299, "3d"],
    ] as const) {
      expect(paneAt(true, x, y, 800, 600), `${x},${y}`).toBe(pane);
    }
    // Unsplit, every point is the 3-D view's.
    expect(paneAt(false, 790, 590, 800, 600)).toBe("3d");
  });

  it("gives an orthographic pane the shape of its own rectangle", () => {
    // The height is what was asked for and the width follows the pane, so
    // a model in a wide pane is not stretched to fit it.
    const wide = orthoExtents(2, 400, 200);
    expect([wide.top, wide.bottom]).toEqual([2, -2]);
    expect([wide.left, wide.right]).toEqual([-4, 4]);
    const tall = orthoExtents(2, 200, 400);
    expect([tall.left, tall.right]).toEqual([-1, 1]);
    // A pane of no height — every pane but one, unsplit — would divide by
    // nothing.
    expect(orthoExtents(2, 0, 0).right).toBe(2);
  });

  it("sizes a handle in an orthographic pane without a distance", () => {
    // Which is the whole difference from the perspective case: everything
    // is drawn at one scale, so how far away it is does not come into it.
    expect(worldSizeForPixelsOrtho(10, 5, 100)).toBeCloseTo(1);
    expect(worldSizeForPixelsOrtho(10, 5, 200)).toBeCloseTo(0.5);
    expect(worldSizeForPixelsOrtho(10, 5, 0)).toBe(0);
  });

  it("draws four cameras when split and one when not", async () => {
    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: CUBE });
    const viewer = new Viewer(new NullEngine());
    viewer.setView(decodeView(kernel.view()));

    // One view and the corner triad.
    expect(viewer.scene.activeCameras).toHaveLength(2);
    expect(viewer.split).toBe(false);

    viewer.setQuad(true);
    expect(viewer.split).toBe(true);
    expect(viewer.scene.activeCameras).toHaveLength(5);

    viewer.setQuad(false);
    expect(viewer.scene.activeCameras).toHaveLength(2);
  });

  it("locks an axis view to its axis, and frames it on the model", async () => {
    // A view down X that can be turned off X is a third 3-D view.
    const kernel = new WasmKernel();
    kernel.apply({ op: "add_block", corners: CUBE });
    const viewer = new Viewer(new NullEngine());
    viewer.setView(decodeView(kernel.view()));
    viewer.setQuad(true);

    viewer.aimAt(10, 10);
    expect(viewer.aimedAtPane).toBe("3d");
    expect(viewer.paneCamera().ortho).toBe(false);

    const engine = viewer.scene.getEngine();
    const w = engine.getRenderWidth();
    viewer.aimAt(w - 10, 10);
    expect(viewer.aimedAtPane).toBe("x");
    const camera = viewer.paneCamera();
    expect(camera.ortho).toBe(true);
    // Big enough to hold a unit cube, which has a diagonal of √3.
    expect(camera.half).toBeGreaterThan(Math.sqrt(3) / 2);
    expect(camera.extents[2]).toBeCloseTo(camera.half);
    expect(camera.extents[3]).toBeCloseTo(-camera.half);
  });
});

describe("which gecko this is", () => {
  it("comes from the crate that was built, not from a string in the page", () => {
    // One source, so the number in the window cannot disagree with the
    // release it came from.
    const kernel = new WasmKernel();
    expect(kernel.version()).toMatch(/^\d+\.\d+\.\d+/);
  });
});

describe("the chord through a face", () => {
  /** Every face the kernel knows, the way a script reads them. */
  interface Face {
    id: number;
    block: number;
    center: [number, number, number];
  }
  const faces = (kernel: WasmKernel): Face[] =>
    JSON.parse(kernel.direct.query("faces", "")) as Face[];

  /** Three unit cubes stacked along z and glued, as the kernel sees them. */
  const slabs = () => {
    const kernel = new WasmKernel();
    for (const z of [0, 1, 2]) kernel.apply({ op: "create_cube", origin: [0, 0, z], size: 1 });
    for (const z of [1, 2]) {
      const pair = faces(kernel)
        .filter((f) => Math.abs(f.center[2] - z) < 1e-9)
        .map((f) => f.id);
      expect(pair).toHaveLength(2);
      kernel.apply({ op: "glue_faces", first: pair[0], second: pair[1] });
    }
    return kernel;
  };

  const flatAt = (kernel: WasmKernel, z: number) => {
    const face = faces(kernel).find((f) => Math.abs(f.center[2] - z) < 1e-9);
    expect(face).toBeDefined();
    return { face: face!.id, block: face!.block };
  };

  it("comes back as something the sheet drawing can light", () => {
    const kernel = slabs();
    const { face, block } = flatAt(kernel, 0);
    const chord = kernel.chord(face, block);
    expect(chord.ok).toBe(true);
    if (!chord.ok) return;

    // Three blocks, and the outline of each face it crosses: four faces
    // along the chord — two ends and two seams — four edges each.
    expect(chord.blocks).toBe(3);
    expect(chord.edges).toHaveLength(16);
    for (const edge of chord.edges) {
      expect(edge.from).toHaveLength(3);
      expect(edge.to).toHaveLength(3);
    }

    // And it draws, headless, with the same call a sheet uses.
    const viewer = new Viewer(new NullEngine());
    viewer.applyPrefs(DEFAULTS);
    viewer.setView(decodeView(kernel.view()));
    viewer.showSheet(chord.edges);
    expect(viewer.scene.getMeshByName("sheet")).not.toBeNull();
    viewer.showSheet([]);
    expect(viewer.scene.getMeshByName("sheet")).toBeNull();
  });

  it("says so rather than throwing when there is no such face", () => {
    const kernel = slabs();
    const chord = kernel.chord(4242, 0);
    expect(chord.ok).toBe(false);
    if (chord.ok) return;
    expect(chord.error).toContain("no face");
  });

  it("is the same thread from either side of an internal face", () => {
    // A face with a block on each side is listed once per block, and the
    // chord through it does not depend on which side was clicked.
    const kernel = slabs();
    const both = faces(kernel).filter((f) => Math.abs(f.center[2] - 1) < 1e-9);
    expect(both).toHaveLength(2);
    const answers = both.map((f) => {
      const chord = kernel.chord(f.id, f.block);
      return chord.ok ? chord.blocks : -1;
    });
    expect(answers).toEqual([3, 3]);
  });
});

describe("the classification colors are told apart", () => {
  // The complaint this answers: the three were violets of one hue, and a
  // corner, a curve and a surface looked the same. Measured rather than
  // judged, so that "harmonising" them back into one violet fails here
  // instead of on somebody's screen.
  //
  // The distance is Euclidean in **OKLab**, which is built so that a step
  // of the same size looks the same size wherever it is taken — unlike
  // hex arithmetic, where two colors can be far apart in numbers and
  // identical to look at. Scaled by 100, so the numbers read like ΔE.
  const oklab = (hex: string): [number, number, number] => {
    const channel = (at: number) => {
      const c = parseInt(hex.slice(at, at + 2), 16) / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    const [r, g, b] = [1, 3, 5].map(channel);
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    return [
      0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
      1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
      0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
    ];
  };
  const apart = (a: string, b: string) => {
    const [x, y] = [oklab(a), oklab(b)];
    return 100 * Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2]);
  };

  for (const which of ["dark", "light"] as const) {
    it(`keeps a corner, a curve and a surface apart in the ${which} theme`, () => {
      const t = PRESETS[which];
      const three = [t.onCorner, t.onCurve, t.onSurface];
      for (const [a, b] of [
        [three[0], three[1]],
        [three[1], three[2]],
        [three[0], three[2]],
      ]) {
        expect(apart(a, b), `${a} and ${b}`).toBeGreaterThan(18);
      }

      // And from what a cell wears when it is classified on **nothing**,
      // which is the fourth answer the same legend gives, and from the
      // yellow of a picked cell, which has to beat all four.
      for (const mine of three) {
        for (const other of [t.nodes, t.edges, t.faces, "#ffcc33"]) {
          expect(apart(mine, other), `${mine} and ${other}`).toBeGreaterThan(15);
        }
      }
    });
  }
});
