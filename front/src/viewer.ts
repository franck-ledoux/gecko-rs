/**
 * The scene.
 *
 * It owns no application state: it is a function of a decoded view. Ask it what
 * is on screen and it will tell you what it was last given — everything that
 * can be *decided* lives in the kernel.
 *
 * It takes an `AbstractEngine`, not a canvas, so the same class runs under
 * `NullEngine` in Node with no GPU. That is where the cheap tests live.
 */

import type { AbstractEngine } from "@babylonjs/core/Engines/abstractEngine";
import { Scene } from "@babylonjs/core/scene";
import { ArcRotateCamera } from "@babylonjs/core/Cameras/arcRotateCamera";
import { Camera } from "@babylonjs/core/Cameras/camera";
import { Viewport } from "@babylonjs/core/Maths/math.viewport";
import { HemisphericLight } from "@babylonjs/core/Lights/hemisphericLight";
import { DirectionalLight } from "@babylonjs/core/Lights/directionalLight";
import { Matrix, Vector2, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Animation } from "@babylonjs/core/Animations/animation";
import { CubicEase, EasingFunction } from "@babylonjs/core/Animations/easing";
import "@babylonjs/core/Animations/animatable";
// Not for a name: importing it is what installs the depth-peeling renderer.
// Without it the property below exists, accepts `true`, and leaves the
// scene rendering nothing at all.
import { Color3, Color4 } from "@babylonjs/core/Maths/math.color";
import { Mesh } from "@babylonjs/core/Meshes/mesh";
import { VertexData } from "@babylonjs/core/Meshes/mesh.vertexData";
import { VertexBuffer } from "@babylonjs/core/Buffers/buffer";
import type { LinesMesh } from "@babylonjs/core/Meshes/linesMesh";
import { CreateBox } from "@babylonjs/core/Meshes/Builders/boxBuilder";
import { CreateSphere } from "@babylonjs/core/Meshes/Builders/sphereBuilder";
import { CreateLineSystem } from "@babylonjs/core/Meshes/Builders/linesBuilder";
import { CreateGreasedLine } from "@babylonjs/core/Meshes/Builders/greasedLineBuilder";
import {
  GreasedLineMeshColorDistributionType,
  GreasedLineMeshMaterialType,
} from "@babylonjs/core/Materials/GreasedLine/greasedLineMaterialInterfaces";
import type { GreasedLineSimpleMaterial } from "@babylonjs/core/Materials/GreasedLine/greasedLineSimpleMaterial";
import { GreasedLineMaterialDefaults } from "@babylonjs/core/Materials/GreasedLine/greasedLineMaterialDefaults";
import type { GreasedLineMesh } from "@babylonjs/core/Meshes/GreasedLine/greasedLineMesh";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import { GlowLayer } from "@babylonjs/core/Layers/glowLayer";
import { RenderTargetTexture } from "@babylonjs/core/Materials/Textures/renderTargetTexture";
import { PointerDragBehavior } from "@babylonjs/core/Behaviors/Meshes/pointerDragBehavior";
import "@babylonjs/core/Culling/ray";
import type { GeometryView, MeshView, NodesView, View } from "./decode";
import type { GeomGroup, Spot } from "./geometry";
import {
  CORNER,
  CURVE,
  DIM_OF,
  geomBounds,
  geomCorners,
  geomCurves,
  geomSurfaces,
  GEOM_GROUPS,
  patchOutline,
  SURFACE,
} from "./geometry";
import { Occlusion } from "./occlusion";
import { AxisGizmo, MAIN_LAYER, orientationFor } from "./axes";
import type { Axis } from "./axes";
import {
  orthoExtents,
  orthoHalfFor,
  paneAt,
  paneRect,
  paneViewport,
  PANES,
  PANE_AXIS,
  worldSizeForPixelsOrtho,
  type Pane,
} from "./panes";
import {
  DEFAULTS,
  NODE_SIZE,
  type GeomLayer,
  type Layer,
  type ViewPrefs,
} from "./theme";
import { ANCHOR, FAMILY, MATE, colorForId } from "./palette";
import type { SheetEdge } from "./kernel";
import {
  buildMesh,
  buildSurfaces,
  buildSurfacesOf,
  edgeSegments,
  groupColor,
  meshEdges,
  type SurfaceData,
  type SurfaceOptions,
} from "./surface";

/** The two groups drawn from the same quadrangles. */
export type Group = "faces" | "blocks";

/** Both of them, for the few things that treat them alike. */
const GROUPS: readonly Group[] = ["faces", "blocks"];

/** What the mouse is aimed at: the block structure, or the geometry. */
export type Picking = "blocks" | "geometry";

/** One click on the surface. Both names, because the quadrangles are listed
 *  per (face, block) pair: gluing wants the face, removing wants the block,
 *  and a pick knows which block's side it landed on even when the face has
 *  one on each. */
export interface Pick {
  face: number;
  block: number;
}

/**
 * What a click at a pointer position would take.
 *
 * One shape for all four kinds, so that one function can answer the
 * question once — see `targetAt`. A halo is drawn from the same answer the
 * click acts on, because a halo promising something the click would not
 * take is worse than no halo at all.
 */
export type Target =
  | { kind: "node"; name: number }
  | { kind: "edge"; name: number }
  | { kind: "face"; face: number; block: number }
  | { kind: "spot"; spot: Spot };

/** Whether two answers are the same cell — so a pointer wandering across
 *  one face does not rebuild its halo sixty times a second. */
export function sameTarget(a: Target | null, b: Target | null): boolean {
  if (a === null || b === null) return a === b;
  if (a.kind !== b.kind) return false;
  if (a.kind === "face") {
    const other = b as { face: number; block: number };
    return a.face === other.face && a.block === other.block;
  }
  if (a.kind === "spot") {
    const other = (b as { spot: Spot }).spot;
    return a.spot.dim === other.dim && a.spot.tag === other.tag;
  }
  return a.name === (b as { name: number }).name;
}

/**
 * Give a greased line new points, **and widths to go with them**.
 *
 * `setPoints` keeps the width table the line was made with. Upstream,
 * `GreasedLineMesh._updateWidths` is empty — "intentionally left blank" —
 * so a line made with one segment and then given twenty has a width for
 * the first two points and nothing for the other forty: the buffer the
 * shader reads them from is the size it was, and what a GPU returns for a
 * read past its end is its own business. Headless Chromium happens to
 * return something drawable for a small overrun; Chrome and Safari on a
 * Mac return nothing, and the line is not drawn at all.
 *
 * That is why a halo along an edge lit up and one along a curve of B10 did
 * not: an edge is one segment, the size the line was made at, and a curve
 * of a real model is twenty. The fix is the table Babylon would have built
 * itself had the line been made with these points — one width per side of
 * every point — put in place before the buffers are made from it.
 *
 * The table is written to the private field because the public setter
 * updates the **old** buffer, which is the wrong size, before `setPoints`
 * makes the new one. A test reads the public getter back, so a Babylon
 * that renames the field fails a test rather than a picture.
 */
export function repoint(line: GreasedLineMesh, lines: number[][]) {
  let points = 0;
  for (const one of lines) points += one.length / 3;
  (line as unknown as { _widths: number[] })._widths = new Array(
    points * 2,
  ).fill(1);
  line.setPoints(lines);
}

/**
 * Throw away a greased line **without** taking every other one with it.
 *
 * A line of one plain color, drawn with the unlit material, is given a
 * colors texture it does not own: Babylon hands every such line the same
 * one-pixel texture, a static on `GreasedLineMaterialDefaults`. And the
 * material's `dispose` disposes whatever colors texture it holds — that
 * shared one included — without clearing the static, so every line still
 * using it, and every line made afterwards, is bound to a dead texture and
 * is not drawn again for the rest of the session.
 *
 * That one upstream behaviour was all three reports about the halo. Made
 * and thrown away per hover, it lit up once and never again; rebuilt when
 * a slider moved, it went for good; and laying a quadrangle throws its
 * provisional side away on every movement of the pointer, so after `⇧Q`
 * nothing plain-colored was drawn at all — the halo, and the sides of the
 * quadrangle being laid.
 *
 * So the shared texture is taken off the material before the material
 * goes. A line with colors of its own owns its texture and keeps the
 * ordinary path.
 */
export function disposeLine(line: GreasedLineMesh | undefined) {
  if (!line) return;
  const material = line.material as GreasedLineSimpleMaterial | null;
  line.dispose(false, false);
  if (!material) return;
  const shared = GreasedLineMaterialDefaults.EmptyColorsTexture;
  const held = material as unknown as { _colorsTexture: unknown };
  if (shared && held._colorsTexture === shared) held._colorsTexture = null;
  material.dispose();
}

/** Where a dragged node was let go, in model coordinates. */
export type Drop = (
  name: number,
  to: [number, number, number],
  /** The other picked nodes, which came along by the same offset. */
  carrying: number[],
) => void;

/** How many pixels across a node handle is by default; the settings panel
 *  can ask for another size. Kept here as well as in the preferences
 *  because the tests measure against it. */
export const HANDLE_PIXELS = NODE_SIZE;

/**
 * The side of the cube a **frozen** node is drawn as, against the diameter
 * of the ball every other node gets.
 *
 * Four fifths rather than one: a cube as wide as a ball's diameter reads
 * bigger than the ball, because the ball's silhouette is its widest circle
 * while the cube shows a corner. Four fifths puts the two at about the same
 * weight on screen, which is what makes the *shape* the thing you notice
 * rather than the size.
 */
export const FROZEN_SIDE = 0.8;

/**
 * And a geometric corner, which is bigger on purpose.
 *
 * A corner of the geometry and a node of the blocking are two different
 * things that will often sit on top of each other — the whole point of the
 * geometry being there is for the blocking to land on it — so they are told
 * apart by size as well as by color.
 */
export const CORNER_PIXELS = 15;

/** How near a line the pointer has to be to count as on it. Lines are one
 *  pixel wide and nobody can hit one. */
export const EDGE_PICK_PIXELS = 7;

/**
 * How thick a lit sheet is drawn, in CSS pixels.
 *
 * WebGL caps `lineWidth` at one pixel whatever you ask for, so a thick line
 * is not a line at all — it is a ribbon built as geometry. That is what
 * `CreateGreasedLine` makes.
 *
 * `sizeAttenuation` is what decides which kind of width this is, and it is
 * the opposite way round from what the name suggests: **off** means "width
 * units are scene units", so the ribbon shrinks into the distance like
 * anything else in the model, and **on** means a width in pixels of the
 * material's `resolution`. A sheet is something you are being shown, not
 * part of the model, so it wants the second — otherwise its thickness is a
 * reading of how far away the camera happens to be, and every recentring
 * changes it.
 *
 * Seven, which is [`EDGE_PICK_PIXELS`]: the lit sheet comes out exactly as
 * wide as the target you had to hit to light it.
 */
export const SHEET_PIXELS = 7;

/** The aiming line, drawn over the sheet, is half again as wide so it reads
 *  against it. */
export const AIM_SHARE = 1.6;

/**
 * The core of a halo along a line, as a share of the halo's own size.
 *
 * A ball and a thread, not two balls: a node's glow comes off a sphere the
 * full size, an edge's off a ribbon well under half of it. What spreads
 * them into the same soft light is the blur, and a thread blurred reads as
 * a line lit from inside while a slab blurred reads as a smear.
 */
export const HALO_CORE = 0.4;

/**
 * How far the blur carries the glow past its core, as a share of the size.
 *
 * The number that makes it a glow rather than a highlight. Babylon takes a
 * kernel in pixels and clamps it itself; this is what it is asked for.
 */
export const HALO_SPREAD = 2.4;

/** As wide as the blur is ever asked to be. Past this the light is spread
 *  so thin that a bigger halo reads as a fainter one. */
export const MAX_HALO_KERNEL = 32;

/** How long the picture has to be still before the halo blooms again, in
 *  milliseconds. Long enough not to flicker between two notches of the
 *  wheel, short enough to read as the glow simply being there. */
export const HALO_REST_MS = 150;

/** How bright the glow comes back. Over one, because the core is drawn at
 *  less than full alpha so the cell shows through it. */
export const HALO_INTENSITY = 1.3;

/** How solid a halo's core is. Short of opaque on purpose: the cell it is
 *  pointing at is inside the light, and a core that hid it would be
 *  answering the question with the answer out of sight. */
export const HALO_ALPHA = 0.55;

/** How long a recentring takes. Long enough to follow the model moving into
 *  place, short enough not to be waiting for it. */
export const FRAMING_MS = 450;

/** How much of the remaining distance one wheel notch covers. Proportional,
 *  so zooming feels the same at every scale. */
export const ZOOM_PER_NOTCH = 0.02;

/**
 * How close the camera may orbit, as a fraction of the model's diagonal.
 *
 * An orbit camera that reaches its own target has nothing left to orbit
 * around, and the view turns inside out. Stopping just short of the model is
 * what zooming in means for this kind of camera.
 */
export const CLOSEST_APPROACH = 0.35;

/**
 * Depth bias pushing the surfaces back, so the wireframe along their borders
 * wins the depth test instead of disappearing into them.
 *
 * Two numbers because `gl.polygonOffset` takes two, and only the second one
 * does the work here. The factor multiplies the polygon's depth *slope*,
 * which is nought for a face square to the camera — exactly the faces whose
 * edges go missing. The units are a constant, and that is what shifts them.
 */
export const SURFACE_DEPTH_BIAS = { factor: 1, units: 4 };

/**
 * Depth bias pulling the lit sheet towards the camera, so it wins against
 * the wireframe it runs along, now that the two are drawn in the same pass.
 *
 * A lit sheet's lines **are** edges of the model, drawn a second time in
 * another color, so the two sit at exactly the same depth and the test
 * between them decides nothing: the sheet comes out in pieces, whichever
 * way the arithmetic happens to fall along each line. Pulled forward by a
 * hair it wins wherever the two coincide, and still loses to anything
 * genuinely in front of it — which is the whole reason for giving it the
 * depth buffer at all.
 *
 * Twice what pushes the surfaces back, and the other way: the sheet has to
 * beat the wireframe, which the surfaces are already losing to.
 */
export const SHEET_DEPTH_BIAS = { factor: -1, units: -8 };

/** And the aiming mark a hair in front of the lit sheet, which it is drawn
 *  along and has to win against: the two are the same edges at the same
 *  depth, and a depth test between two things at the same depth settles
 *  nothing. Twice the sheet's bias rather than a nudge, so that what
 *  separates them is not of the same size as the slack in the comparison. */
export const AIM_DEPTH_BIAS = { factor: -1, units: -16 };

/**
 * The shortest extrusion the pointer can ask for, as a share of the face it
 * grows from.
 *
 * Never nought: a block of no thickness is not a block, and the kernel
 * refuses one. Dragging the wrong way should hold the preview at its
 * thinnest rather than make it vanish and leave nothing to let go of.
 */
export const THINNEST = 0.02;

/**
 * How far along an axis the pointer is aiming.
 *
 * The pointer gives a *ray* and the extrusion runs along a *line*; the
 * answer is the point of the line closest to that ray, which is the usual
 * closest-approach-of-two-lines and has a closed form. `towards` and
 * `normal` are unit vectors, so the determinant below is `1 - cos²` of the
 * angle between them.
 *
 * Nothing when the two are near enough parallel — looking straight down the
 * extrusion, where every distance is as plausible as every other and the
 * arithmetic says so by dividing by nothing.
 */
export function heightAlong(
  origin: readonly [number, number, number],
  towards: readonly [number, number, number],
  center: readonly [number, number, number],
  normal: readonly [number, number, number],
): number | null {
  const dot = (a: readonly number[], b: readonly number[]) =>
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const gap = [0, 1, 2].map((k) => origin[k] - center[k]);
  const slant = dot(towards, normal);
  const denominator = 1 - slant * slant;
  if (Math.abs(denominator) < 1e-6) return null;
  return (dot(normal, gap) - slant * dot(towards, gap)) / denominator;
}

/**
 * Which pass each thing is drawn in.
 *
 * Transparency is sorted **per mesh**, back to front by how far the mesh's
 * middle is from the camera, and that is a guess: a geometry surface and
 * the block face following it have their middles in almost the same place,
 * so which came first flipped as the view turned. Putting them in separate
 * passes settles it — the geometry first, the blocking over it — and the
 * passes share one depth buffer, so what is genuinely in front still wins.
 *
 * A lit sheet is drawn *with* the blocking rather than over it. It had a
 * pass of its own above everything once, on the grounds that it is a thing
 * being shown to you rather than a thing that is there — but it runs
 * **through** the model, and a sheet at the back of a hundred blocks then
 * reads exactly like one at the front. In the blocking's own pass it is
 * hidden by what is in front of it and tinted by the faces it lies behind,
 * which is what tells you where it runs.
 *
 * The mark a cut is aimed with goes the same way, and for the same reason:
 * it is drawn along the sheet's own edges, so a mark over everything says a
 * cut lands on the far side of the model as loudly as on the near side.
 *
 * The last pass alone is over everything, on purpose: an extrusion preview
 * is half inside the block it grows from, and is a thing being shown to you
 * rather than a thing that is there.
 */
export const PASS = { geometry: 0, blocking: 1, aiming: 2 } as const;

/**
 * Take the keyboard away from a camera.
 *
 * The view is moved with the pointer and with the keys this application
 * says move it — `F`, `X`, `Y`, `Z` — and with nothing else. Babylon hands
 * an arc-rotate camera the arrow keys of its own accord, which is a binding
 * the application never asked for and cannot see in its own table: the
 * arrows are how you step from one layer to the next, and every step turned
 * the view as well.
 */
function noKeyboard(camera: ArcRotateCamera) {
  camera.inputs.removeByType("ArcRotateCameraKeyboardMoveInput");
}

/** How solid the preview of an extrusion is drawn. Enough to read as a
 *  volume, little enough to see the model through. */
export const GROWN_ALPHA = 0.3;

/**
 * The six quadrangles of the preview box, as 24 vertices.
 *
 * Unshared corners, so each face gets a normal of its own and the box
 * reads as a box rather than as a balloon. Wound outward throughout: the
 * near ring runs anticlockwise seen from the far end, so the far face
 * keeps that order, the near face reverses it, and each side takes one
 * edge of the ring and the two above it.
 */
function boxFaces(
  near: readonly Vector3[],
  far: readonly Vector3[],
): Float32Array {
  const quads: Vector3[][] = [
    [far[0], far[1], far[2], far[3]],
    [near[0], near[3], near[2], near[1]],
  ];
  for (let k = 0; k < 4; k++) {
    const next = (k + 1) % 4;
    quads.push([near[k], near[next], far[next], far[k]]);
  }
  const out = new Float32Array(24 * 3);
  quads.forEach((quad, q) =>
    quad.forEach((p, k) => {
      const at = (q * 4 + k) * 3;
      out[at] = p.x;
      out[at + 1] = p.y;
      out[at + 2] = p.z;
    }),
  );
  return out;
}

/** Two triangles per quadrangle, and the quadrangles never change. */
const BOX_TRIANGLES = new Uint32Array(
  Array.from({ length: 6 }, (_, q) => {
    const base = q * 4;
    return [base, base + 1, base + 2, base, base + 2, base + 3];
  }).flat(),
);

function boxNormals(positions: Float32Array): number[] {
  const normals: number[] = [];
  VertexData.ComputeNormals(positions, Array.from(BOX_TRIANGLES), normals);
  return normals;
}

/** The twelve edges, six numbers each. */
function boxOutline(
  near: readonly Vector3[],
  far: readonly Vector3[],
): number[][] {
  const out: number[][] = [];
  for (let k = 0; k < 4; k++) {
    const next = (k + 1) % 4;
    out.push([...near[k].asArray(), ...near[next].asArray()]);
    out.push([...far[k].asArray(), ...far[next].asArray()]);
    out.push([...near[k].asArray(), ...far[k].asArray()]);
  }
  return out;
}

/** The axis an axis view looks down, as a vector. */
function planeNormal(axis: Axis): Vector3 {
  const sign = axis.startsWith("-") ? -1 : 1;
  const letter = axis[axis.length - 1];
  return new Vector3(
    letter === "x" ? sign : 0,
    letter === "y" ? sign : 0,
    letter === "z" ? sign : 0,
  );
}

/** Newell's normal of a quadrangle — the same arithmetic as the kernel's
 *  `quad_normal`, so the two agree about which way is out. */
function quadNormal(corners: readonly Vector3[]): Vector3 {
  const n = new Vector3(0, 0, 0);
  for (let k = 0; k < 4; k++) {
    const a = corners[k];
    const b = corners[(k + 1) % 4];
    n.x += (a.y - b.y) * (a.z + b.z);
    n.y += (a.z - b.z) * (a.x + b.x);
    n.z += (a.x - b.x) * (a.y + b.y);
  }
  return n;
}

/**
 * The world size that covers `pixels` on screen at this distance.
 *
 * Sizing handles in world units is wrong in both directions: tied to the
 * model's extent they balloon as blocks are added, tied to a block's edge they
 * vanish once the model is big enough to be framed from far away. A handle is
 * something you aim at, so what has to stay constant is how big it is *on
 * screen*.
 *
 * There is deliberately no ceiling on the result. Zooming in makes a handle
 * *smaller* in world units, never larger, so it can never swallow the block it
 * marks — a cap would only ever bind when far away, which is exactly where it
 * would undo the point.
 */
export function worldSizeForPixels(
  pixels: number,
  distance: number,
  fieldOfView: number,
  renderHeight: number,
): number {
  if (renderHeight <= 0) return 0;
  return (pixels * 2 * Math.tan(fieldOfView / 2) * distance) / renderHeight;
}

/**
 * Animate one property from where it is to where it should be.
 *
 * Eased in and out rather than linear: a constant-speed move starts and stops
 * dead, which reads as two jumps with a wait in between.
 */
function glide(
  target: object,
  property: string,
  from: number | Vector3,
  to: number | Vector3,
) {
  const frames = 30;
  const running = Animation.CreateAndStartAnimation(
    `frame-${property}`,
    target,
    property,
    (frames * 1000) / FRAMING_MS,
    frames,
    from,
    to,
    Animation.ANIMATIONLOOPMODE_CONSTANT,
  );
  const ease = new CubicEase();
  ease.setEasingMode(EasingFunction.EASINGMODE_EASEINOUT);
  for (const a of running?.getAnimations() ?? [])
    a.animation.setEasingFunction(ease);
}

/*
 * Render pixels to the CSS pixels a pointer event speaks — applied by
 * `nodeScreen`, `faceScreen` and `AxisGizmo.axisScreen`.
 *
 * `Vector3.Project` works in the canvas's backing store, which on a
 * high-density display is twice the size of the canvas itself. `scene.pick`
 * and `offsetX`/`offsetY` are both in CSS pixels. The two frames coincide at
 * a device pixel ratio of 1 — which is what a headless test runs at, so no
 * test here can see the difference — and are a factor of two apart on the
 * machine this is used on.
 */
export class Viewer {
  readonly scene: Scene;
  readonly camera: ArcRotateCamera;
  /** The f64 offset the kernel subtracted. The scene is never shifted by it —
   *  that would put the float32 world matrices back in the precision hole the
   *  rebasing just dug us out of. It is applied only when converting a scene
   *  point back to model coordinates. */
  center: [number, number, number] = [0, 0, 0];

  private nodes: Mesh[] = [];
  /**
   * Which groups are being looked at, as the two halves of a mask, and
   * whether the cells in no group at all are drawn.
   *
   * View state, like the theme: the kernel holds which cells are in which
   * group and knows nothing about what anybody is looking at. Everything
   * is shown until somebody says otherwise, so a model with no groups in
   * it behaves exactly as it did before there were any.
   */
  private shown = { lo: 0xffffffff, hi: 0xffffffff, ungrouped: true };
  /**
   * One layer of blocks, when the view has been narrowed to it, as the
   * names of everything in it.
   *
   * A filter of its own rather than one more group: a group is something
   * the model has and a script can name, and this is somewhere you are
   * standing. The two are independent — a layer of a group is a perfectly
   * good thing to be looking at — so both are asked, and a cell is drawn
   * only if both say yes.
   */
  private only: {
    nodes: Set<number>;
    edges: Set<number>;
    blocks: Set<number>;
  } | null = null;

  /** The blocking's wireframe: ribbons rather than GL lines, because a GL
   *  line is one pixel wide whatever anybody asks for. */
  private wire?: GreasedLineMesh;
  /** How many segments it holds, which is the edge count and what says
   *  whether the mesh can be reused rather than rebuilt. */
  private wireSegments = 0;
  /** The sheet the viewer has been asked to light up: the edge names, and
   *  where a cut would run from and to on each. */
  private lit: SheetEdge[] = [];
  /** Where a cut is being aimed, along the sheet's own direction. */
  private aim: number | null = null;
  private aimWire?: Mesh;
  private held?: HTMLCanvasElement;
  /** How many edges of the lit sheet are actually drawn — the kernel says
   *  how many there are, and the filters say how many of them you see. */
  litDrawn = 0;
  /** The edge that sheet was asked for, so a second click on it can put the
   *  sheet out again. View state, like the face selection. */
  selectedEdge: number | null = null;
  private litWire?: Mesh;
  /** One mesh per block, so that Babylon can sort them back to front.
   *
   *  The block's name travels with the slot: a drag redraws the blocks that
   *  the moved node is a corner of and no others, and that needs to be able
   *  to find them. */
  private surfaces: Record<
    Group,
    { block: number; mesh: Mesh; data: SurfaceData }[]
  > = {
    faces: [],
    blocks: [],
  };

  /** Which cells each node belongs to, by its place in the view's arrays.
   *
   *  Built from the view when the view changes, not when a node moves —
   *  which is the whole point: moving one node leaves the topology alone,
   *  so this is the same index for the length of a gesture.
   *
   *  Undefined until something asks, and dropped whenever the view is
   *  replaced. */
  private around?: {
    edges: Map<number, number[]>;
    blocks: Map<number, number[]>;
  };

  /** The wireframe's points, kept so that a drag can rewrite the few that
   *  moved instead of building the whole list again. */
  private wirePoints?: number[][];
  /** Which edge each of those entries is, in the same order. */
  private wireEdges?: number[];
  /** Which kind of thing the mouse is aimed at.
   *
   *  A mode, because the two overlap: a geometric surface and the block
   *  faces inside it are at the same pixels, and whichever answered first
   *  would make the other unreachable. Transient rather than remembered —
   *  a mode you left on last week and have forgotten is a mystery, not a
   *  preference. */
  private picking: Picking = "blocks";
  /** The geometric entities picked, by dimension and gmsh tag, in the
   *  order they were picked. View state, like the face selection.
   *
   *  Any of the three groups, not surfaces alone: a box is built round
   *  surfaces, but what you put a corner of it *on* is as likely to be a
   *  curve — the circle at the end of a cylinder, say. */
  private pickedGeom: Spot[] = [];
  /** The nodes picked, by the kernel's names for them, in the order they
   *  were picked.
   *
   *  A list, because the things you do with picked nodes are worth doing
   *  to four of them at once: putting the corners of a box on the circle
   *  they are meant to follow is one act, not four. */
  private pickedNodes: number[] = [];
  /** The mesh's skin as the kernel last sent it, and what is drawn of it:
   *  one surface and one wireframe. Apart from everything else because it
   *  arrives on a clock of its own — when somebody asks for a mesh — and
   *  is thrown away by the first edit after that. */
  private meshView?: MeshView;
  private meshSkin?: Mesh;
  private meshWire?: LinesMesh;
  /** The face being grown, while the pointer is down on it: where it is,
   *  which way is out, and how far the pointer has asked for so far. */
  private growing?: {
    pick: Pick;
    center: Vector3;
    normal: Vector3;
    corners: Vector3[];
    /** Whether the drag may go **behind** the face. True for a face of an
     *  open sheet, which has no inside; false for a face of a block,
     *  whose other side is the block. */
    bothWays: boolean;
  };
  /** A face being **slid** rather than grown: which nodes move, where each
   *  of them started, and the line they move along.
   *
   *  Its own state and not a flag on `growing`, because the two are
   *  different gestures that happen to be aimed the same way: one makes a
   *  block and previews a ghost of it, the other moves corners the model
   *  already has and previews by moving them. */
  private sliding?: {
    nodes: number[];
    from: Vector3[];
    center: Vector3;
    normal: Vector3;
    by: number;
  };
  /** The marks and sides shown while a quadrangle is being laid on the
   *  geometry, and the provisional sides to wherever the pointer is. */
  private laying?: { marks: Mesh[]; wire?: GreasedLineMesh };
  private layingAim?: GreasedLineMesh;
  private layingAimSides = 0;

  /** What the halo rings, and the meshes that draw it.
   *
   *  Rebuilt only when the answer changes — a pointer wandering across one
   *  face would otherwise throw two meshes away sixty times a second to
   *  draw the same ring. `sameTarget` is what decides. */
  private aimedTarget: Target | null = null;
  /** The halo's two meshes, **made once and kept**.
   *
   *  Shown, hidden and given new points — the idiom the grown box already
   *  uses — rather than made per hover: there is nothing to gain from a
   *  mesh and a material sixty times a second, and throwing a one-color
   *  line away is not harmless anyway (see `disposeLine`).
   *
   *  Keeping the line means giving it new points, and that has a trap of
   *  its own, which hid the curves of every real model: see `repoint`. */
  private haloBall?: Mesh;
  private haloLine?: GreasedLineMesh;
  /** The handle the ball is on, which it follows wherever it goes. */
  private haloFollows: Mesh | null = null;
  /** How many sides of ribbon the ring is in. Counted on the way in, like
   *  the laying mark's, rather than read back off the mesh: a greased line
   *  is free to hold its points however it likes. */
  private haloSides = 0;
  /** How big the halo's marks are asked to come out, in CSS pixels. Read by
   *  the per-pane sizing pass, which is the only place that knows which
   *  camera they are about to be seen through. */
  private haloPixels = 0;
  /** The bloom the halo is seen through. Made on the first halo and kept:
   *  an effect layer carries render targets, and building one per pointer
   *  movement would be a new framebuffer sixty times a second. */
  private glow?: GlowLayer;
  /** What the glow layer was last given, so it can be taken back. */
  private glowing: Mesh[] = [];
  /** When the picture last moved, and what it looked like then — for
   *  `quietGlow`, which keeps the bloom off while anything moves. */
  private stillSince = 0;
  private lastView = "";
  /** Whether the bloom's texture has to be drawn again: the halo moved to
   *  another cell, or was tuned, or the glow is coming back on. */
  private glowStale = true;
  /** How many times the halo's meshes have been made.
   *
   *  Not a statistic: it is what a test can hold on to to say that a
   *  preference change **tuned** the halo rather than building a new one,
   *  which is the difference between a glow that survives a slider and one
   *  that does not. */
  private haloBuilds = 0;
  private grownWire?: GreasedLineMesh;
  private grownBox?: Mesh;
  private grown: number | null = null;
  /** The geometry, if one has been loaded. Read-only here: nothing in the
   *  viewer edits it, and no operation moves it. */
  private geometry?: GeometryView;
  /** One mesh per geometric entity, so each can be colored, faded and
   *  hidden as the one thing it is — and so the transparency sorter has
   *  something to sort. */
  private geomMeshes: Record<GeomGroup, Mesh[]> = {
    corners: [],
    curves: [],
    surfaces: [],
  };
  /** A working copy of the view: the drag previews against it, so the scene can
   *  follow the mouse without a round trip to the kernel. */
  private view?: View;
  private dropped: Drop[] = [];
  private prefs: ViewPrefs = structuredClone(DEFAULTS);
  /** What the user has picked, in order. The first is the anchor — a glue
   *  leaves the seam where it is — so the two are drawn differently. */
  private picked: Pick[] = [];
  readonly axes: AxisGizmo;
  /** The ambient occlusion, switched on and off by the preferences. */
  private readonly occlusion: Occlusion;
  /** One camera per pane. The 3-D one is [`camera`](Viewer.camera); the
   *  other three look down an axis and are orthographic. */
  private readonly panes: Record<Pane, ArcRotateCamera>;
  /** Whether the window is split in four. */
  private quad = false;
  /** The pane the pointer is over. Everything a gesture does — which
   *  camera answers a click, which plane a drag runs in, how big a handle
   *  has to be — is about this one. */
  private hovered: Pane = "3d";
  /** How much of the world each orthographic pane shows, top to bottom.
   *  Its own, because each pane zooms on its own. */
  private half: Record<Pane, number> = { "3d": 1, x: 1, y: 1, z: 1 };

  constructor(engine: AbstractEngine) {
    this.scene = new Scene(engine);
    this.scene.clearColor = new Color4(0.09, 0.1, 0.12, 1);
    this.camera = new ArcRotateCamera(
      "cam",
      -Math.PI / 3,
      Math.PI / 3,
      4,
      Vector3.Zero(),
      this.scene,
    );
    noKeyboard(this.camera);
    // Moving the target must not re-derive the angles.
    //
    // `camera.target = …` goes through `setTarget`, which by default calls
    // `rebuildAnglesAndRadius`: it keeps the camera where it is and works out
    // new angles to the new target. That is the opposite of recentring, whose
    // whole point is that the direction you are looking from does not change.
    // This flag makes every assignment keep alpha, beta and radius.
    this.camera.overrideCloneAlphaBetaRadius = true;
    // Zoom by a percentage of the distance, not a fixed step. With a fixed
    // step every notch is the same number of world units, so far out it
    // barely moves and close in a single notch leaps straight through the
    // model and out the other side. `lowerRadiusLimit` is set when the model
    // is framed, since what counts as close depends on how big it is.
    this.camera.wheelDeltaPercentage = ZOOM_PER_NOTCH;
    this.camera.minZ = 0.001;
    this.camera.layerMask = MAIN_LAYER;
    const light = new HemisphericLight(
      "hemi",
      new Vector3(0, 1, 0),
      this.scene,
    );
    light.intensity = 0.9;
    // Not black underneath: surfaces are lit by this one light, and a face
    // turned away from it would otherwise be a silhouette with no color left
    // to read — which defeats coloring by name.
    light.groundColor = new Color3(0.42, 0.42, 0.46);
    // A second light, from over your left shoulder and fixed in the world.
    //
    // One hemispheric light shades a surface by which way it faces *up*, so
    // the four walls of a block come out the same color and the block reads
    // as a flat outline. A directional light breaks that: two faces meeting
    // at a corner take different amounts of it, which is what makes the
    // corner visible. Fixed rather than carried by the camera, because a
    // light that follows you lights every face the same from every side and
    // gives the shape back up. Dim: this is a depth cue laid over the
    // coloring, not a second coloring.
    const sun = new DirectionalLight(
      "sun",
      new Vector3(-0.55, -1, -0.4),
      this.scene,
    );
    sun.intensity = 0.45;
    // Colors here carry a meaning — a classification, a group, an id — and a
    // highlight is a white patch across one that says nothing.
    sun.specular = Color3.Black();

    this.panes = {
      "3d": this.camera,
      x: this.orthoCamera("x"),
      y: this.orthoCamera("y"),
      z: this.orthoCamera("z"),
    };

    // Per **camera**, not per frame. One sphere is shown in four views at
    // four zooms and cannot be the right number of pixels across in all
    // of them at once — but it can be resized between them, because each
    // camera's render happens after its own observable.
    this.scene.onBeforeCameraRenderObservable.add((camera) => {
      this.rescaleHandles(camera as ArcRotateCamera);
    });
    this.scene.onBeforeRenderObservable.add(() => this.fitPickTolerance());
    this.scene.onBeforeRenderObservable.add(() => this.followHalo());
    this.scene.onBeforeRenderObservable.add(() => this.quietGlow());
    this.occlusion = new Occlusion(this.scene, this.camera);
    // Per frame, because the surfaces are built afresh by every edit and a
    // list that still names a disposed mesh is a crash inside a frame. It
    // walks the meshes and stops at the comparison when nothing moved.
    this.scene.onBeforeRenderObservable.add(() => {
      if (this.occlusion.on) this.occlusion.shade(this.shadedMeshes());
    });
    this.axes = new AxisGizmo(this.scene, this.camera);
    this.layOut();
    // Rendering leaves `activeCamera` set to whichever camera went last — the
    // corner gizmo. Everything that runs between frames then reads the wrong
    // one, and Babylon's own drag behavior reads it twice: to decide whose
    // controls to suspend, and to build the ray the drag follows. So the
    // dragged node went the wrong way *and* the view orbited along with it.
    // Put it back to the main camera once the frame is done.
    this.scene.onAfterRenderObservable.add(() => {
      this.scene.activeCamera = this.panes[this.hovered];
    });
    // Babylon's own picking — which is what the drag behaviors run on — must
    // use the main camera, not whichever happens to be active. The gizmo does
    // its own pick, through its own camera.
    this.scene.cameraToUseForPointers = this.camera;
    // And it must only ever see the node handles.
    //
    // The surfaces are pickable, because a face is something you choose with
    // the mouse. But Babylon's pointer system returns the nearest pickable
    // mesh, so a node on the far side of the model became ungrabbable the
    // moment the faces were drawn — the face in front answered first, and
    // the drag behavior, which checks that the pick is *its* mesh, never
    // started. Half the handles of every cube, silently.
    //
    // Faces are picked deliberately instead, by `pickSurface`, on a click that
    // did not land on a handle.
    // The blocking's pass keeps the depth the geometry's left behind, so
    // the two are ordered without either being declared always in front.
    // The pass above it does clear, which is what puts an extrusion
    // preview over everything.
    this.scene.setRenderingAutoClearDepthStencil(
      PASS.blocking,
      false,
      false,
      false,
    );
    this.setPicking("blocks");
    this.applyPrefs(this.prefs);
  }

  /**
   * A camera looking straight down one axis, with no perspective.
   *
   * No perspective is the point of it: two things the same size are drawn
   * the same size wherever they are, which is what makes these views worth
   * having for lining anything up. The rotation is locked — a view down X
   * that can be turned off X is a third 3-D view — and the distance is
   * fixed, an orthographic camera's distance changing nothing but what it
   * clips.
   */
  private orthoCamera(pane: Pane): ArcRotateCamera {
    const { alpha, beta } = orientationFor(PANE_AXIS[pane]!);
    const camera = new ArcRotateCamera(
      `cam-${pane}`,
      alpha,
      beta,
      100,
      Vector3.Zero(),
      this.scene,
    );
    camera.mode = Camera.ORTHOGRAPHIC_CAMERA;
    camera.overrideCloneAlphaBetaRadius = true;
    camera.layerMask = MAIN_LAYER;
    camera.lowerAlphaLimit = alpha;
    camera.upperAlphaLimit = alpha;
    camera.lowerBetaLimit = beta;
    camera.upperBetaLimit = beta;
    camera.lowerRadiusLimit = 100;
    camera.upperRadiusLimit = 100;
    camera.minZ = 0.01;
    camera.maxZ = 1000;
    // Left-drag belongs to the model here — there is nothing to orbit — so
    // the only thing the camera takes is the right button, which pans.
    camera.panningSensibility = 200;
    noKeyboard(camera);
    return camera;
  }

  /** Split the window in four, or put it back. */
  setQuad(quad: boolean) {
    if (this.quad === quad) return;
    this.quad = quad;
    if (!quad) this.hovered = "3d";
    // Before the layout: splitting takes the shading away and joining
    // gives it back, and either way it is this that decides.
    this.refreshOcclusion();
    this.layOut();
    this.frameAll(true);
  }

  get split(): boolean {
    return this.quad;
  }

  /** The pane the pointer is over. */
  get aimedAtPane(): Pane {
    return this.hovered;
  }

  /** The plane a node is currently dragged in, or nothing when the drag
   *  is left to face the camera. */
  dragPlane(): [number, number, number] | null {
    for (const mesh of this.nodes) {
      for (const behavior of mesh.behaviors) {
        if (behavior.name !== "PointerDrag") continue;
        const normal = (behavior as PointerDragBehavior).options
          .dragPlaneNormal;
        return normal ? [normal.x, normal.y, normal.z] : null;
      }
    }
    return null;
  }

  /** What the pane under the pointer is showing — everything a test needs
   *  to say where a point of the model lands on screen. */
  paneCamera(): {
    pane: Pane;
    ortho: boolean;
    half: number;
    extents: [number, number, number, number];
    viewport: [number, number, number, number];
    target: [number, number, number];
  } {
    const camera = this.panes[this.hovered];
    const v = camera.viewport;
    return {
      pane: this.hovered,
      ortho: camera.mode === Camera.ORTHOGRAPHIC_CAMERA,
      half: this.half[this.hovered],
      extents: [
        camera.orthoLeft ?? 0,
        camera.orthoRight ?? 0,
        camera.orthoTop ?? 0,
        camera.orthoBottom ?? 0,
      ],
      viewport: [v.x, v.y, v.width, v.height],
      target: [camera.target.x, camera.target.y, camera.target.z],
    };
  }

  /**
   * Give each camera its share of the canvas, and say which are drawn.
   *
   * The corner triad goes in the 3-D pane rather than in the corner of the
   * window: it reports the 3-D camera's rotation, and in the bottom right
   * of a split window it would be sitting in the Z view saying something
   * about a different view entirely.
   */
  private layOut() {
    for (const pane of PANES) {
      const f = paneViewport(pane, this.quad);
      this.panes[pane].viewport = new Viewport(f.x, f.y, f.w, f.h);
    }
    const drawn: ArcRotateCamera[] = this.quad
      ? PANES.map((p) => this.panes[p])
      : [this.camera];
    this.scene.activeCameras = [...drawn, this.axes.camera];
    this.resize();
    this.attachHovered();
  }

  /** Point the camera controls at the pane the pointer is over, and only
   *  at that one: four cameras attached to one canvas all answer the same
   *  drag, and the view would turn in every pane at once. */
  private attachHovered() {
    for (const pane of PANES) this.panes[pane].detachControl();
    const camera = this.panes[this.hovered];
    if (this.held) camera.attachControl(this.held, true);
    // Babylon's own pointer system — which is what the node drag behaviors
    // run on — builds its ray through this one.
    this.scene.cameraToUseForPointers = camera;
  }

  /**
   * Note which pane the pointer is in, and set everything that follows
   * from it.
   *
   * Called on every pointer move over the canvas. The **drag plane** is
   * the part that matters: in an axis view a node is dragged in the plane
   * of the screen, which is what makes these views worth using for placing
   * one — there is no depth to get wrong. In the 3-D view it stays
   * camera-facing, as it always was.
   */
  aimAt(x: number, y: number) {
    const pane = this.paneFor(x, y);
    if (pane === this.hovered) return;
    this.hovered = pane;
    this.attachHovered();
    for (const mesh of this.nodes) this.setDragPlane(mesh);
  }

  /**
   * Which plane a node is dragged in, from the pane the pointer is over.
   *
   * In an axis view it is that axis's plane: two coordinates move and the
   * third cannot, which is what makes these views worth using for placing
   * a corner. In the 3-D view the plane faces the camera, as it always
   * has. Read at the start of each drag, so crossing into another pane
   * between drags is enough to change it.
   *
   * An orthographic camera locked to an axis already faces that plane, so
   * leaving this out would **look** the same today. Saying it makes the
   * constraint a property of the pane rather than a coincidence of where
   * the camera happens to be pointing — which is the thing that was
   * asked for, and the thing that would quietly stop being true if an
   * axis view ever gained a way to be turned.
   */
  private setDragPlane(mesh: Mesh) {
    const axis = PANE_AXIS[this.hovered];
    const normal = axis ? planeNormal(axis) : undefined;
    for (const behavior of mesh.behaviors) {
      if (behavior.name !== "PointerDrag") continue;
      (behavior as PointerDragBehavior).options = normal
        ? { dragPlaneNormal: normal }
        : {};
    }
  }

  /** The camera that answers for a point of the canvas. */
  private cameraFor(x: number, y: number): ArcRotateCamera {
    return this.panes[this.paneFor(x, y)];
  }

  /**
   * Which pane a point of the canvas is in.
   *
   * The canvas's CSS size when there is one, and the engine's otherwise —
   * which is what a headless test has, and what there is before anything
   * has been attached.
   */
  private paneFor(x: number, y: number): Pane {
    const engine = this.scene.getEngine();
    const css = engine.getHardwareScalingLevel();
    const width = this.held?.clientWidth || engine.getRenderWidth() * css;
    const height = this.held?.clientHeight || engine.getRenderHeight() * css;
    return paneAt(this.quad, x, y, width, height);
  }

  /**
   * How the scene looks. Not the kernel's business, so it never goes through
   * it: the kernel says what exists, not what color it is here.
   *
   * One entry point rather than one per control. Color, transparency and
   * shrink are not independent of each other at the mesh level — a shrink
   * moves vertices, a coloring rewrites the color buffer — and splitting
   * them into separate methods only invites two of them to disagree.
   */
  applyPrefs(prefs: ViewPrefs) {
    this.prefs = structuredClone(prefs);
    const theme = this.prefs.theme;
    this.scene.clearColor = Color3.FromHexString(theme.background).toColor4(1);
    this.paintNodes();
    // The wireframe's colors and width both come out of the rebuild: a
    // ribbon carries its colors in its material, not in a `color` on the
    // mesh.
    this.rebuildEdges();
    this.rebuildHighlight();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");
    this.rebuildGeometry();
    this.rebuildMesh();
    this.retuneHalo();
    this.refreshOcclusion();
  }

  /**
   * The occlusion, in step with the preferences and with the model.
   *
   * One call rather than three, and run after every rebuild: the shading is
   * a function of what is on screen and of how big it is, and both of those
   * change under it.
   */
  private refreshOcclusion() {
    // **Not while the window is split.** The shading is one camera's, and
    // in four views it is not merely absent from the three orthographic
    // panes — it is wrong in the 3-D one, because the pipeline shades the
    // whole canvas while that camera owns a quarter of it, and the
    // occlusion of the model lands in the empty space beside it.
    //
    // Neither way out works, and both were tried: attached to all four
    // cameras, each pane's output wipes the one before it and the window
    // goes black; told to respect the viewports, every pane draws itself
    // at a quarter size in a corner. So the shading waits for a single
    // view. The preference is untouched — the faces stay as the switch
    // left them, and the shading comes back when the window does.
    this.occlusion.set(this.prefs.ao && !this.quad);
    if (!this.occlusion.on) return;
    this.occlusion.tune(this.modelBounds());
    this.occlusion.shade(this.shadedMeshes());
  }

  /**
   * What the shading is computed from: the surfaces, and nothing with a
   * width of its own.
   *
   * Named one by one rather than taken as everything-but, so that a mesh
   * added here next year is left out until somebody decides it belongs —
   * which is the safe way round. Measured on a model of twenty-seven
   * blocks, letting the wireframe and the handles in raises the shaded
   * pixels by three quarters: a line lying along an edge of the model is
   * an occluder of the faces either side of it, and it does not belong to
   * the model any more than a label does.
   */
  private shadedMeshes(): Mesh[] {
    const shaded: Mesh[] = [];
    for (const group of GROUPS) {
      for (const slot of this.surfaces[group]) shaded.push(slot.mesh);
    }
    // The geometry's patches and the mesh's skin are surfaces too, and a
    // model classified onto a loaded geometry is exactly the picture this
    // is meant to help read.
    shaded.push(...this.geomMeshes.surfaces);
    if (this.meshSkin) shaded.push(this.meshSkin);
    return shaded;
  }

  /**
   * Look at these groups and no others.
   *
   * A cell is drawn when it is in **any** shown group — one cell can be
   * in several, and hiding one of them should not take the cell with it.
   */
  showGroups(lo: number, hi: number, ungrouped: boolean) {
    this.shown = { lo: lo >>> 0, hi: hi >>> 0, ungrouped };
    this.rebuildEdges();
    this.rebuildHighlight();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");
    this.paintNodes();
  }

  /** What is being looked at, for a test and for the drawer. */
  get groupsShown(): { lo: number; hi: number; ungrouped: boolean } {
    return { ...this.shown };
  }

  /**
   * How much of the model the group filter lets through, against how much
   * there is — counted in quadrangles, which is the one thing every block,
   * face and edge on screen is made of.
   *
   * Not the same question as what is drawn: the display settings hide
   * things too, and each of those has a switch of its own in plain sight.
   * This filter is the one that can empty the view with nothing to say so,
   * which is why the page asks it this and puts the answer on the screen.
   */
  get filtered(): { shown: number; all: number } {
    const view = this.view;
    if (!view) return { shown: 0, all: 0 };
    let shown = 0;
    for (let q = 0; q < view.quadFaces.length; q++) {
      if (this.inShown(view.quadGroupsLo[q], view.quadGroupsHi[q])) shown++;
    }
    return { shown, all: view.quadFaces.length };
  }

  /**
   * Narrow the view to one layer of blocks, or widen it again with `null`.
   *
   * The names come from the kernel, which is the only thing that knows
   * what a layer is made of. Everything here does is believe them.
   */
  showOnly(
    cells: { nodes: number[]; edges: number[]; blocks: number[] } | null,
  ) {
    this.only = cells && {
      nodes: new Set(cells.nodes),
      edges: new Set(cells.edges),
      blocks: new Set(cells.blocks),
    };
    this.rebuildEdges();
    this.rebuildHighlight();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");
    this.paintNodes();
  }

  /** What the shading is computed from, by name, for a test: the rule that
   *  keeps the wireframe and the handles out of it is worth stating
   *  exactly rather than inferring from how dark the picture came out. */
  get shadedNames(): string[] {
    return this.shadedMeshes().map((m) => m.name);
  }

  /** Whether the hollows are actually being shaded. Not the same as the
   *  preference: an engine that cannot do it says no. */
  get occluding(): boolean {
    return this.occlusion.on;
  }

  /** Whether the view is narrowed to a layer, for a test and for the page. */
  get narrowed(): boolean {
    return this.only !== null;
  }

  /**
   * Is a cell one of the ones being looked at?
   *
   * A cell is drawn when **any** block it bounds is drawn. `loose` says
   * it bounds a block in no group, which the mask cannot: an edge between
   * a block of the fluid and a block of nothing carries the fluid's bit,
   * and hiding the fluid used to take that edge off the block still on
   * screen. A cell with no groups at all is loose by definition.
   */
  private inShown(lo: number, hi: number, loose = false): boolean {
    if ((lo & this.shown.lo) !== 0 || (hi & this.shown.hi) !== 0) return true;
    return (loose || (lo === 0 && hi === 0)) && this.shown.ungrouped;
  }

  /** Which quadrangles of the view are in a group being looked at. */
  private quadsShown(): ReadonlySet<number> | undefined {
    const view = this.view;
    if (!view) return undefined;
    // Nothing filtered is the usual case, and it costs nothing to say so:
    // the surface builder then draws every quadrangle as it always did.
    const everyGroup =
      this.shown.lo === 0xffffffff &&
      this.shown.hi === 0xffffffff &&
      this.shown.ungrouped;
    if (everyGroup && !this.only) return undefined;
    const kept = new Set<number>();
    for (let q = 0; q < view.quadFaces.length; q++) {
      if (this.only && !this.only.blocks.has(view.quadBlocks[q])) continue;
      // A quadrangle is drawn with one block, so its own mask is the
      // whole answer: zero means that block is in no group.
      if (this.inShown(view.quadGroupsLo[q], view.quadGroupsHi[q])) kept.add(q);
    }
    return kept;
  }

  /** What the viewer is currently told to look like. */
  get display(): ViewPrefs {
    return structuredClone(this.prefs);
  }

  private layerOf(group: Group): Layer {
    return group === "faces" ? this.prefs.faces : this.prefs.blocks;
  }

  private surfaceOptions(group: Group): SurfaceOptions {
    const layer = this.layerOf(group);
    return {
      by: group === "faces" ? "face" : "block",
      coloring: layer.coloring,
      uniform:
        group === "faces" ? this.prefs.theme.faces : this.prefs.theme.blocks,
      shrink: layer.shrink,
      // A block keeps all six of its own faces whatever this says: shrink an
      // open shell and you are looking into a box with a side missing.
      internal: group === "faces" ? this.prefs.faces.internal : true,
      highlight: new Map(
        this.picked.map((p, n) => [p.face, n === 0 ? ANCHOR : MATE]),
      ),
      classified: this.classifiedFaces(),
      only: this.quadsShown(),
    };
  }

  /** The color each classified face wears, by face name. */
  private classifiedFaces(): Map<number, string> {
    const out = new Map<number, string>();
    for (let q = 0; q < (this.view?.quadFaces.length ?? 0); q++) {
      const dim = this.view!.quadClassDim[q];
      if (dim < 0) continue;
      out.set(
        this.view!.quadFaces[q],
        this.classColor(dim, 0, this.prefs.theme.faces).toHexString(),
      );
    }
    return out;
  }

  /**
   * Which faces are picked, in the order they were picked.
   *
   * View state, like the theme: the kernel is told about a selection once, as
   * a command, and never holds one. Two tabs on a station may legitimately
   * have different faces selected.
   */
  get selection(): Pick[] {
    return this.picked.map((p) => ({ ...p }));
  }

  select(picks: readonly Pick[]) {
    this.picked = picks.map((p) => ({ ...p }));
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");
  }

  /**
   * Screen coordinates -> the face and block under them.
   *
   * Babylon reports the triangle it hit; two triangles make a quadrangle, and
   * the quadrangle carries both names whichever group is on screen.
   */
  pickSurface(x: number, y: number): Pick | null {
    const hit = this.scene.pick(
      x,
      y,
      (m) => this.dataOf(m.name) !== undefined,
      false,
      this.cameraFor(x, y),
    );
    if (!hit?.hit || !hit.pickedMesh) return null;
    const data = this.dataOf(hit.pickedMesh.name);
    const quad = Math.floor(hit.faceId / 2);
    if (!data || quad < 0 || quad >= data.faces.length) return null;
    return { face: data.faces[quad], block: data.blocks[quad] };
  }

  /** The surface data behind a mesh, by the name it was given. */
  private dataOf(name: string): SurfaceData | undefined {
    for (const group of ["faces", "blocks"] as Group[]) {
      for (const slot of this.surfaces[group]) {
        if (slot.mesh.name === name) return slot.data;
      }
    }
    return undefined;
  }

  /**
   * What a click here would take. **The** resolution order, in one place.
   *
   * Smallest target first — a handle, then a line, then the surface behind
   * them — because each is the thing you had to aim at to hit it. Aimed at
   * the geometry, the block structure is not on offer at all: the two are
   * at the same pixels, and whichever answered first would hide the other.
   *
   * This used to be written three times over, in three `pointerdown`
   * listeners that each re-asked the questions the one before it had asked.
   * Three copies of an order is three chances for them to disagree, and a
   * halo drawn from a fourth copy would have been a promise the click did
   * not keep. So: one function, and everything that needs the answer asks
   * it for it.
   */
  targetAt(x: number, y: number): Target | null {
    if (this.aimedAtGeometry) {
      const spot = this.pickGeometry(x, y);
      return spot === null ? null : { kind: "spot", spot };
    }
    const node = this.pickNode(x, y);
    if (node !== null) return { kind: "node", name: node };
    const edge = this.pickEdge(x, y);
    if (edge !== null) return { kind: "edge", name: edge };
    const face = this.pickSurface(x, y);
    return face === null ? null : { kind: "face", ...face };
  }

  /** What the halo is ringing, for a test to read as state rather than as
   *  pixels — and for the readout to name. */
  get targeted(): Target | null {
    return this.aimedTarget;
  }

  /** How much halo is actually on screen: how many balls, and how many
   *  sides of ribbon. What says the ring reached the picture and not only
   *  the field holding the answer. */
  /**
   * The box the ring occupies, in scene coordinates, or nothing when there
   * is no ring.
   *
   * `haloDrawn` says how many pieces reached the screen; this says where
   * they are. A ring whose count is right and whose place is wrong is a
   * ring nobody sees, and a count alone cannot tell the two apart.
   */
  get haloBox(): { min: number[]; max: number[] } | null {
    const meshes = [this.haloBall, this.haloLine].filter(
      (m): m is Mesh | GreasedLineMesh => m !== undefined && m.isEnabled(),
    );
    if (!meshes.length) return null;
    const min = [Infinity, Infinity, Infinity];
    const max = [-Infinity, -Infinity, -Infinity];
    for (const mesh of meshes) {
      mesh.computeWorldMatrix(true);
      const box = mesh.getBoundingInfo().boundingBox;
      for (const axis of [0, 1, 2]) {
        min[axis] = Math.min(min[axis], box.minimumWorld.asArray()[axis]);
        max[axis] = Math.max(max[axis], box.maximumWorld.asArray()[axis]);
      }
    }
    return { min, max };
  }

  get haloDrawn(): {
    marks: number;
    sides: number;
    builds: number;
    widths: number;
    vertices: number;
    textured: boolean;
    glowing: boolean;
  } {
    const line = this.haloLine?.isEnabled() ? this.haloLine : undefined;
    return {
      marks: this.haloBall?.isEnabled() ? 1 : 0,
      sides: this.haloSides,
      builds: this.haloBuilds,
      // One width per vertex is what the shader reads; fewer is a line
      // drawn in part or not at all, depending on the GPU. Both numbers,
      // so a test can say they agree.
      widths: line?.widths.length ?? 0,
      vertices: line?.getTotalVertices() ?? 0,
      // Whether the texture the line reads its color from is still there.
      // A dead one is drawn as nothing, whatever the rest says.
      glowing: this.glow?.isEnabled ?? false,
      textured: line
        ? !!(
            line.material as GreasedLineSimpleMaterial | null
          )?.colorsTexture?.getInternalTexture()
        : false,
    };
  }

  /**
   * Ring whatever the pointer would take, or take the ring away.
   *
   * Two shapes and no more: a ball around anything that is a point, and a
   * ribbon along anything that is a line — which a face is too, by its
   * four sides, and a surface by the sides its triangles do not share. A
   * tint over the cell was the other option and is the wrong one: a halo
   * that covers the thing it is pointing at hides the very detail you were
   * squinting at when you needed it.
   *
   * Nothing here is pickable and nothing is added to the camera's limits.
   * A ring under the pointer is not part of the model, and letting it push
   * the limits about would make the view breathe as the mouse moved.
   */
  showHover(target: Target | null) {
    if (sameTarget(target, this.aimedTarget)) return;
    this.aimedTarget = target;
    this.haloSides = 0;

    const anchor = target === null ? null : this.haloAnchor(target);
    this.haloFollows = anchor;
    const at = anchor ? anchor.position.clone() : null;
    const lines = at || target === null ? [] : this.haloLines(target);

    const ball = this.ball();
    ball.setEnabled(at !== null);
    if (at) {
      ball.position = at;
      this.sizeBall(ball, at);
    }

    const line = this.line();
    line.setEnabled(lines.length > 0);
    if (lines.length) {
      repoint(line, lines);
      this.haloSides = lines.length;
    }

    // Whether it blooms is decided every frame, in `quietGlow`: on when
    // something is lit **and** nothing is moving. Another cell is another
    // picture to bloom.
    this.glowStale = true;
  }

  /**
   * The halo on screen, in step with the preferences.
   *
   * A size and a color are read when the halo is made, so a slider dragged
   * with one on screen would otherwise leave it as it was until the
   * pointer moved. **Tuned, not rebuilt**, as the wireframe's width is on
   * a resize. Rebuilding it here is what used to put the halo out for the
   * rest of the session at one drag of any slider — not, as was first
   * thought, the glow layer, but the shared colors texture a one-color
   * line takes down with it when it goes: see `disposeLine`.
   */
  private retuneHalo() {
    const color = Color3.FromHexString(this.prefs.theme.halo);
    // Read by the per-pane pass, which is what actually resizes the ball.
    this.haloPixels = this.prefs.halo.size;
    const ball = this.haloBall;
    if (ball) {
      (ball.material as StandardMaterial).emissiveColor = color;
      if (ball.isEnabled()) this.sizeBall(ball, ball.position);
    }
    const wire = this.haloLine;
    if (wire) {
      const material = wire.greasedLineMaterial as GreasedLineSimpleMaterial;
      material.color = color;
      material.width = this.ribbon(this.haloShare).width;
    }
    if (this.glow) this.glow.blurKernelSize = this.haloBlur;
    this.glowStale = true;
  }

  /** How wide the blur is asked to be, from the size. */
  private get haloBlur(): number {
    return Math.min(
      MAX_HALO_KERNEL,
      Math.round(this.prefs.halo.size * HALO_SPREAD),
    );
  }

  /** Where a halo around a point-like target goes, in scene coordinates —
   *  or nothing, when the target is not point-like. Taken off the mesh that
   *  draws the cell rather than worked out again, so the ring cannot land
   *  anywhere other than on what is on screen. */
  private haloAnchor(target: Target): Mesh | null {
    if (target.kind === "node") {
      return this.nodes.find((m) => m.metadata.name === target.name) ?? null;
    }
    if (target.kind === "spot" && target.spot.dim === CORNER) {
      return (
        this.geomMeshes.corners.find(
          (m) => m.name === `geom-corners-${target.spot.tag}`,
        ) ?? null
      );
    }
    return null;
  }

  /**
   * Keep the ball on the handle it rings, every frame.
   *
   * A handle being dragged moves under the ball — the drag moves the mesh,
   * and the kernel is only told at the drop — so a ball placed once stayed
   * where the node **was** until it was let go. It follows the mesh it was
   * put on instead, whatever moves it.
   */
  private followHalo() {
    const anchor = this.haloFollows;
    const ball = this.haloBall;
    if (!anchor || !ball?.isEnabled()) return;
    if (!ball.position.equals(anchor.position)) {
      ball.position.copyFrom(anchor.position);
      // A handle being dragged is something moving, like the view.
      this.stillSince = performance.now();
    }
  }

  /**
   * The bloom on only when the picture is still.
   *
   * The glow layer is not cheap: a blurred pass over its own target, every
   * frame it is on. Measured on a cube, with a halo lit, it took the frame
   * rate from 52 to 21 — and the wheel, the orbit and a drag are exactly
   * when a slow frame is felt, and when nobody is looking at a halo. On a
   * runner with no graphics card it was slow enough for sixty notches of
   * the wheel to run past a test's minute.
   *
   * So it goes off the moment any pane's camera moves, or the handle under
   * the ball does, and comes back once things have been still for
   * `HALO_REST_MS`. The halo itself — the ball, the thread — stays drawn
   * throughout; only the light around it waits.
   */
  private quietGlow() {
    const now = performance.now();
    // Rounded, so that the last few millionths an inertial camera sheds as
    // it comes to rest do not count as moving: compared exactly, a view
    // that has stopped for every eye kept the bloom off indefinitely.
    const view = PANES.map((pane) => {
      const c = this.panes[pane];
      const scale = Math.max(Math.abs(c.radius), 1e-9);
      const angle = (a: number) => a.toFixed(4);
      const length = (l: number) => (l / scale).toFixed(4);
      return [
        angle(c.alpha),
        angle(c.beta),
        c.radius.toPrecision(5),
        length(c.target.x),
        length(c.target.y),
        length(c.target.z),
        length(c.orthoTop ?? 0),
        length(c.orthoLeft ?? 0),
      ].join(",");
    }).join("|");
    if (view !== this.lastView) {
      this.lastView = view;
      this.stillSince = now;
    }
    if (!this.glow) return;
    const lit = this.glowing.some((mesh) => mesh.isEnabled());
    const on = lit && now - this.stillSince >= HALO_REST_MS;
    if (on && !this.glow.isEnabled) this.glowStale = true;
    if (this.glow.isEnabled !== on) this.glow.isEnabled = on;
    if (!on) return;
    // **Rendered once per still picture**, not every frame. A still view
    // with a still halo blooms the same way each time, and the blurred
    // pass is the whole of the glow's cost: drawn once and composited
    // after that, it costs a still view next to nothing. A split window
    // keeps rendering every frame — one texture for four cameras would
    // put the 3-D pane's light in all four.
    const texture = this.glow.mainTexture;
    const once = !this.quad;
    const rate = once ? RenderTargetTexture.REFRESHRATE_RENDER_ONCE : 1;
    if (texture.refreshRate !== rate) texture.refreshRate = rate;
    if (once && this.glowStale) texture.resetRefreshCounter();
    this.glowStale = false;
  }

  /** The sides a halo around a line-like or patch-like target runs along,
   *  six numbers each. Empty when there are none to draw — a cell a filter
   *  has taken off the screen has no sides on it. */
  private haloLines(target: Target): number[][] {
    if (target.kind === "edge") return this.haloEdge(target.name);
    if (target.kind === "face") return this.haloFace(target);
    if (target.kind === "spot" && target.spot.dim === CURVE) {
      const curve = this.geometry
        ? geomCurves(this.geometry).find((c) => c.tag === target.spot.tag)
        : undefined;
      if (!curve) return [];
      const out: number[][] = [];
      for (let s = 0; s < curve.segments.length; s += 6) {
        out.push([...curve.segments.slice(s, s + 6)]);
      }
      return out;
    }
    if (target.kind === "spot" && target.spot.dim === SURFACE) {
      const surface = this.geometry
        ? geomSurfaces(this.geometry).find((s) => s.tag === target.spot.tag)
        : undefined;
      return surface ? patchOutline(surface.positions, surface.indices) : [];
    }
    return [];
  }

  /** The drawn segment of one edge, shrink and all. Nothing when the edge
   *  is not among the ones on screen. */
  private haloEdge(name: number): number[][] {
    if (!this.view) return [];
    const flat = edgeSegments(this.view, this.prefs.edges.shrink);
    for (const e of this.edgesShown()) {
      if (this.view.edgeNames[e] !== name) continue;
      return [[...flat.slice(e * 6, e * 6 + 6)]];
    }
    return [];
  }

  /**
   * The four sides of a face, as the surface actually draws it.
   *
   * Off the built buffer rather than off the view, because the buffer is
   * where the shrink has been applied: a halo around an exploded model has
   * to be around the quadrangle you can see, not around where it would be
   * if nothing were exploded.
   *
   * Faces before blocks, which is the order `dataOf` scans in. The two draw
   * the same quadrangle at their own shrink, so with both layers on the ring
   * lands on the faces' copy — and with only one of them on, on that one.
   */
  private haloFace(pick: { face: number; block: number }): number[][] {
    for (const group of GROUPS) {
      for (const slot of this.surfaces[group]) {
        for (let q = 0; q < slot.data.faces.length; q++) {
          if (slot.data.faces[q] !== pick.face) continue;
          if (slot.data.blocks[q] !== pick.block) continue;
          const corner = (k: number) => {
            const v = q * 12 + k * 3;
            return [
              slot.data.positions[v],
              slot.data.positions[v + 1],
              slot.data.positions[v + 2],
            ];
          };
          // Round the perimeter: the four vertices are the quadrangle's
          // corners in the kernel's winding, so consecutive ones are a side
          // and the fourth closes it.
          return [0, 1, 2, 3].map((k) => [
            ...corner(k),
            ...corner((k + 1) % 4),
          ]);
        }
      }
    }
    return [];
  }

  /** The ball, made the first time one is wanted and kept thereafter. */
  private ball(): Mesh {
    if (this.haloBall) return this.haloBall;
    const ball = CreateSphere("halo", { diameter: 1, segments: 12 }, this.scene);
    const material = new StandardMaterial("haloMat", this.scene);
    material.emissiveColor = Color3.FromHexString(this.prefs.theme.halo);
    material.disableLighting = true;
    // Through, not over: a solid ball the size of this one would hide the
    // handle it is saying something about.
    material.alpha = HALO_ALPHA;
    ball.material = material;
    ball.isPickable = false;
    ball.layerMask = MAIN_LAYER;
    ball.renderingGroupId = PASS.aiming;
    ball.setEnabled(false);
    this.haloBall = ball;
    this.haloBuilds += 1;
    this.lightUp(ball);
    return ball;
  }

  /** The ball at the size the preference asks for, now.
   *
   *  Sized here as well as in the per-pane pass, so it is never drawn at a
   *  world unit for the one frame between being moved and being scaled. */
  private sizeBall(ball: Mesh, at: Vector3) {
    this.haloPixels = this.prefs.halo.size;
    ball.scaling.setAll(
      worldSizeForPixels(
        this.haloPixels,
        Vector3.Distance(at, this.camera.position),
        this.camera.fov,
        this.scene.getEngine().getRenderHeight(),
      ),
    );
  }

  /** The ribbon, likewise: one mesh, given new points each time.
   *
   *  `updatable`, because that is the whole point of keeping it. It starts
   *  on a segment of no length, which is never seen: it is disabled until
   *  there is something to run it along. */
  private line(): GreasedLineMesh {
    if (this.haloLine) return this.haloLine;
    const wire = CreateGreasedLine(
      "halo",
      { points: [[0, 0, 0, 0, 0, 0]], updatable: true },
      {
        color: Color3.FromHexString(this.prefs.theme.halo),
        materialType: GreasedLineMeshMaterialType.MATERIAL_TYPE_SIMPLE,
        ...this.ribbon(this.haloShare),
      },
      this.scene,
    ) as GreasedLineMesh;
    wire.isPickable = false;
    wire.layerMask = MAIN_LAYER;
    wire.renderingGroupId = PASS.aiming;
    const material = wire.material as GreasedLineSimpleMaterial;
    material.alpha = HALO_ALPHA;
    wire.setEnabled(false);
    this.haloLine = wire;
    this.haloBuilds += 1;
    this.lightUp(wire);
    return wire;
  }

  /** The ribbon's width as `ribbon` wants it: a share of `SHEET_PIXELS`. */
  private get haloShare(): number {
    return (this.prefs.halo.size * HALO_CORE) / SHEET_PIXELS;
  }

  /**
   * The bloom, made on demand and told what to carry.
   *
   * `includedOnlyMeshes` rather than letting it loose on the scene: every
   * surface in this model is emissive to some degree, and a glow layer with
   * no list blooms the lot — which is a different application.
   *
   * The ribbon is referenced to its own material because it has one: a
   * greased line is not a `StandardMaterial`, and the layer's own shader
   * would find no emissive color on it and render it black.
   */
  private lightUp(mesh: Mesh | GreasedLineMesh) {
    if (!this.glow) {
      this.glow = new GlowLayer("halo", this.scene, {
        // A quarter of the pixels of 512, for a light that is blurred
        // anyway: measured, it is a third of the glow's cost gone.
        mainTextureFixedSize: 256,
        blurKernelSize: 32,
        // The 3-D pane's camera and no other. Without it the layer is drawn
        // for every camera that renders — the corner triad included, which
        // then wore a small copy of the halo's light in its corner.
        camera: this.camera,
      });
      this.glow.intensity = HALO_INTENSITY;
    }
    const glow = this.glow;
    // Clamped: the kernel is in pixels and a very wide one spreads the
    // light so thin that a big halo comes out fainter than a small one,
    // which is not what a size slider should do.
    glow.blurKernelSize = this.haloBlur;
    this.glowing.push(mesh as Mesh);
    glow.addIncludedOnlyMesh(mesh as Mesh);
    // A greased line carries its color in a material of its own, which the
    // layer's shader knows nothing about: told to use that material, it
    // blooms the line's own color instead of rendering it black.
    if ("greasedLineMaterial" in mesh) {
      glow.referenceMeshToUseItsOwnMaterial(mesh as GreasedLineMesh);
    }
  }


  /**
   * Bring one of the two surfaces up to date with the view and the settings.
   *
   * The mesh is **kept** whenever the new data has the same shape as the old,
   * and only its buffers are rewritten. Dragging a shrink slider is a stream
   * of these — one per mouse move — and disposing two meshes and two
   * materials per move is what makes the model blink and the slider stutter.
   *
   * It is thrown away only when the shape really changes: a new view, a group
   * switched on or off, internal faces hidden. Those remove or add
   * quadrangles, and there is then nothing to patch.
   */
  private rebuildSurface(group: Group) {
    const layer = this.layerOf(group);
    const wanted =
      this.view && layer.visible && this.view.quads.length
        ? buildSurfaces(this.view, this.surfaceOptions(group))
        : [];

    // Same shape as what is already there: rewrite the buffers rather than
    // throwing the meshes away. Dragging a slider is a stream of these, and
    // meshes disposed and rebuilt per mouse-move make the model blink.
    const slots = this.surfaces[group];
    const sameShape =
      slots.length === wanted.length &&
      slots.every(
        (slot, i) =>
          slot.data.positions.length === wanted[i].data.positions.length,
      );

    if (sameShape) {
      slots.forEach((slot, i) => {
        const data = wanted[i].data;
        slot.block = wanted[i].block;
        slot.data = data;
        slot.mesh.updateVerticesData(VertexBuffer.PositionKind, data.positions);
        slot.mesh.updateVerticesData(VertexBuffer.ColorKind, data.colors);
        slot.mesh.updateVerticesData(VertexBuffer.NormalKind, data.normals);
        (slot.mesh.material as StandardMaterial).alpha = layer.alpha;
      });
      return;
    }

    for (const slot of slots) slot.mesh.dispose();
    this.surfaces[group] = wanted.map(({ block, data }) => ({
      block,
      mesh: this.surfaceMesh(group, block, data, layer),
      data,
    }));
  }

  private surfaceMesh(
    group: Group,
    block: number,
    data: SurfaceData,
    layer: Layer,
  ): Mesh {
    const mesh = new Mesh(`${group}-${block}`, this.scene);
    const vertices = new VertexData();
    vertices.positions = data.positions;
    vertices.indices = data.indices;
    vertices.colors = data.colors;
    vertices.normals = data.normals;
    // Updatable: everything above depends on it.
    vertices.applyToMesh(mesh, true);

    const material = new StandardMaterial(`${group}Mat${block}`, this.scene);
    material.specularColor = Color3.Black();
    // Both sides: a translucent shell you can only see the outside of is a
    // picture of its silhouette.
    material.backFaceCulling = false;
    material.alpha = layer.alpha;
    // And the inside of that shell before the outside, which is the one
    // ordering a mesh of its own cannot settle on its own.
    material.separateCullingPass = true;
    // Push the surface a hair away from the camera in depth only. The edges
    // run exactly along it — they are the same corners — so without this the
    // depth test decides between them arbitrarily, and at full opacity the
    // wireframe simply vanishes into the faces it borders.
    material.zOffset = SURFACE_DEPTH_BIAS.factor;
    material.zOffsetUnits = SURFACE_DEPTH_BIAS.units;
    mesh.material = material;
    // Pickable, because a face is something you choose with the mouse. The
    // node handles still win where they overlap: the scene's pointer
    // predicates only ever see those.
    mesh.isPickable = true;
    mesh.layerMask = MAIN_LAYER;
    mesh.renderingGroupId = PASS.blocking;
    return mesh;
  }

  /**
   * Begin growing a block out of a face.
   *
   * Reads the face's own corners from the view — not from the drawn
   * surface, whose corners have been pulled about by the shrink slider.
   * What is grown has to be the face the kernel holds, not the picture of
   * it.
   */
  beginExtrusion(pick: Pick, bothWays = false): boolean {
    const quad = this.quadOf(pick);
    if (quad === null || !this.view) return false;
    const corners = [0, 1, 2, 3].map((k) =>
      this.point(this.view!.quads[quad * 4 + k]),
    );
    const normal = quadNormal(corners);
    if (normal.length() < 1e-12) return false;
    const center = corners
      .reduce((at, p) => at.addInPlace(p), new Vector3(0, 0, 0))
      .scale(0.25);
    this.growing = {
      pick,
      center,
      normal: normal.normalize(),
      corners,
      bothWays,
    };
    this.grown = null;
    this.showGrown();
    return true;
  }

  /**
   * Begin sliding these faces along the first one's normal.
   *
   * A **translation**, which is why one normal serves them all: the nodes
   * a face shares with another selected face would otherwise have two
   * places to go, and a drag is one gesture with one direction in it.
   *
   * Previewed by moving the nodes in the working copy, exactly as dragging
   * a handle does — so what you see while you aim is the model with the
   * face where you are putting it, rather than a ghost of something else.
   */
  beginSlide(along: Pick, picks: readonly Pick[]): boolean {
    const view = this.view;
    if (!view) return false;
    const lead = this.quadOf(along);
    if (lead === null) return false;
    const corners = [0, 1, 2, 3].map((k) =>
      this.point(view.quads[lead * 4 + k]),
    );
    const normal = quadNormal(corners);
    if (normal.length() < 1e-12) return false;

    // Every node of every selected face, once. A node on two of them moves
    // with both, which is what makes a crease travel as one piece.
    const nodes: number[] = [];
    for (const pick of picks.some((p) => p.face === along.face)
      ? picks
      : [along]) {
      const quad = this.quadOf(pick);
      if (quad === null) continue;
      for (let k = 0; k < 4; k++) {
        const node = view.quads[quad * 4 + k];
        if (!nodes.includes(node)) nodes.push(node);
      }
    }
    if (!nodes.length) return false;

    this.sliding = {
      nodes,
      from: nodes.map((n) => this.point(n)),
      center: corners
        .reduce((at, p) => at.addInPlace(p), new Vector3(0, 0, 0))
        .scale(0.25),
      normal: normal.normalize(),
      by: 0,
    };
    return true;
  }

  /** How far the pointer is asking the faces to slide, and the model moved
   *  to match. Signed: a face of a block may be pushed in as well as out. */
  aimSlide(x: number, y: number): number | null {
    const slide = this.sliding;
    if (!slide) return null;
    const ray = this.scene.createPickingRay(
      x,
      y,
      Matrix.Identity(),
      this.cameraFor(x, y),
    );
    const towards = ray.direction.normalizeToNew();
    const asked = heightAlong(
      [ray.origin.x, ray.origin.y, ray.origin.z],
      [towards.x, towards.y, towards.z],
      [slide.center.x, slide.center.y, slide.center.z],
      [slide.normal.x, slide.normal.y, slide.normal.z],
    );
    // Looking straight down the line: hold what we had rather than jump to
    // whatever a near-singular division produced.
    if (asked === null) return slide.by;
    slide.by = asked;
    slide.nodes.forEach((node, i) => {
      this.place(node, slide.from[i].add(slide.normal.scale(asked)));
    });
    this.redrawAround(slide.nodes);
    return asked;
  }

  /**
   * Let go, and say what to tell the kernel: the nodes that moved, the first
   * one's new place, and the rest to be carried by the same offset.
   *
   * Nothing is put back. The working copy stays where the pointer left it,
   * and the kernel's answer arrives over the top of it — which is what makes
   * the model stop still rather than snap back and then forward again.
   */
  endSlide(): {
    node: number;
    to: [number, number, number];
    carrying: number[];
  } | null {
    const slide = this.sliding;
    this.sliding = undefined;
    if (!slide || !this.view) return null;
    const names = slide.nodes.map((n) => this.view!.names[n]);
    const at = slide.from[0].add(slide.normal.scale(slide.by));
    return {
      node: names[0],
      to: [at.x + this.center[0], at.y + this.center[1], at.z + this.center[2]],
      carrying: names.slice(1),
    };
  }

  /** Put the slid nodes back where they were, for a gesture given up on. */
  cancelSlide() {
    const slide = this.sliding;
    this.sliding = undefined;
    if (!slide) return;
    slide.nodes.forEach((node, i) => this.place(node, slide.from[i]));
    this.redrawAround(slide.nodes);
  }

  /** How far the pointer is asking for, and the preview to match. */
  aimExtrusion(x: number, y: number): number | null {
    if (!this.growing) return null;
    const { center, normal, corners } = this.growing;
    const ray = this.scene.createPickingRay(
      x,
      y,
      Matrix.Identity(),
      this.cameraFor(x, y),
    );
    const towards = ray.direction.normalizeToNew();
    const asked = heightAlong(
      [ray.origin.x, ray.origin.y, ray.origin.z],
      [towards.x, towards.y, towards.z],
      [center.x, center.y, center.z],
      [normal.x, normal.y, normal.z],
    );
    // Looking straight down the extrusion: hold what we had rather than
    // jump to whatever a near-singular division produced.
    if (asked === null) return this.grown;
    // Thinner than this is not a block anybody meant to make — but on an
    // open sheet of faces *either side* is a direction somebody may have
    // meant, so the sign is kept and only the thickness is held off zero.
    // Which way a free face is wound came from the triangles under it, and
    // nobody looking at the screen can tell.
    const thinnest = Vector3.Distance(corners[0], corners[2]) * THINNEST;
    this.grown =
      this.growing.bothWays && asked < 0
        ? Math.min(asked, -thinnest)
        : Math.max(asked, thinnest);
    this.showGrown();
    return this.grown;
  }

  /** The height the pointer last asked for, or nothing if none is being
   *  grown. */
  get grownBy(): number | null {
    return this.grown;
  }

  /** What is being grown, as the pair the kernel wants. */
  get growingFrom(): Pick | null {
    return this.growing ? { ...this.growing.pick } : null;
  }

  endExtrusion() {
    this.growing = undefined;
    this.grown = null;
    this.showGrown();
  }

  /**
   * The preview: the block that would be built, as a translucent solid
   * with its edges drawn over it.
   *
   * **Built once and then written to.** A drag is a stream of these — one
   * per pointer event, several per frame — and throwing two meshes and
   * two materials away each time is what made it flicker and lag. Only
   * eight corners move, so only the buffers are rewritten; the same
   * lesson as the shrink sliders.
   *
   * A solid rather than the bare wireframe it started as: what you are
   * judging is the volume you are about to add, and twelve lines in
   * mid-air read as twelve lines.
   */
  private showGrown() {
    if (!this.growing || this.grown === null) {
      this.grownBox?.dispose(false, true);
      this.grownWire?.dispose(false, true);
      this.grownBox = undefined;
      this.grownWire = undefined;
      return;
    }

    const { normal, corners } = this.growing;
    const height = this.grown;
    const far = corners.map((p) => p.add(normal.scale(height)));
    const positions = boxFaces(corners, far);
    const outline = boxOutline(corners, far);

    if (this.grownBox && this.grownWire) {
      this.grownBox.updateVerticesData(VertexBuffer.PositionKind, positions);
      this.grownBox.updateVerticesData(
        VertexBuffer.NormalKind,
        boxNormals(positions),
      );
      this.grownWire.setPoints(outline);
      return;
    }

    const mesh = new Mesh("grown", this.scene);
    const vertices = new VertexData();
    vertices.positions = positions;
    vertices.indices = Array.from(BOX_TRIANGLES);
    vertices.normals = boxNormals(positions);
    vertices.applyToMesh(mesh, true);

    const color = Color3.FromHexString(MATE);
    const material = new StandardMaterial("grownMat", this.scene);
    material.diffuseColor = color;
    material.emissiveColor = color.scale(0.45);
    material.specularColor = Color3.Black();
    material.backFaceCulling = false;
    material.separateCullingPass = true;
    material.alpha = GROWN_ALPHA;
    mesh.material = material;
    mesh.isPickable = false;
    mesh.layerMask = MAIN_LAYER;
    // Over the model, like the lit sheet: it is a thing being shown to
    // you rather than a thing that is there, and half of it is usually
    // behind the block it is growing from.
    mesh.renderingGroupId = PASS.aiming;
    this.grownBox = mesh;

    this.grownWire = CreateGreasedLine(
      "grown-edges",
      { points: outline, updatable: true },
      { color: color, ...this.ribbon(1) },
      this.scene,
    ) as GreasedLineMesh;
    this.grownWire.isPickable = false;
    this.grownWire.layerMask = MAIN_LAYER;
    this.grownWire.renderingGroupId = PASS.aiming;
  }

  /** Which quadrangle of the view a pick names. */
  private quadOf(pick: Pick): number | null {
    if (!this.view) return null;
    for (let q = 0; q < this.view.quadFaces.length; q++) {
      if (
        this.view.quadFaces[q] === pick.face &&
        this.view.quadBlocks[q] === pick.block
      ) {
        return q;
      }
    }
    return null;
  }

  /**
   * Aim the mouse at the blocks, or at the geometry.
   *
   * In geometry mode nothing of the block structure answers a click — not
   * the node handles either, which Babylon's own pointer system reaches
   * through the scene's predicates rather than through ours.
   */
  setPicking(picking: Picking) {
    this.picking = picking;
    const handles = (m: {
      metadata?: { name?: number };
      isVisible?: boolean;
    }) =>
      picking === "blocks" &&
      (m.metadata?.name ?? -1) >= 0 &&
      m.isVisible !== false;
    this.scene.pointerDownPredicate = handles;
    this.scene.pointerUpPredicate = handles;
    this.scene.pointerMovePredicate = handles;
  }

  get aimedAtGeometry(): boolean {
    return this.picking === "geometry";
  }

  /** The geometric entities picked. */
  get geomSelection(): Spot[] {
    return this.pickedGeom.map((spot) => ({ ...spot }));
  }

  /** Those of them that are surfaces, which is what a box is built round. */
  get surfaceSelection(): number[] {
    return this.pickedGeom.filter((s) => s.dim === SURFACE).map((s) => s.tag);
  }

  selectGeom(spots: readonly Spot[]) {
    this.pickedGeom = spots.map((spot) => ({ ...spot }));
    this.rebuildGeometry();
  }

  /**
   * Screen coordinates -> the geometric entity under them.
   *
   * Through a predicate of its own rather than by making the geometry
   * pickable: Babylon consults `isPickable` only when no predicate is
   * given, so the geometry can stay invisible to the pointer system —
   * which is what keeps it from stealing a click meant for a block — while
   * still being reachable deliberately, from here.
   *
   * Smallest target first — a corner, then a curve, then the surface
   * behind them — because each is the thing you had to aim at to hit it,
   * and a curve lies exactly on the surfaces it bounds.
   */
  pickGeometry(x: number, y: number): Spot | null {
    return this.pickOnGeometry(x, y)?.spot ?? null;
  }

  /**
   * The same, with the point the ray touched it at.
   *
   * Corners first, then curves, then surfaces — which is the preference the
   * tolerance is made of: a corner is a small ball and a curve a thin
   * ribbon, so a click only reaches one of them by being near it, and a
   * click that reaches two means the nearer kind.
   */
  pickOnGeometry(
    x: number,
    y: number,
  ): { spot: Spot; at: [number, number, number] } | null {
    for (const group of ["corners", "curves", "surfaces"] as const) {
      const mine = new Set(this.geomMeshes[group].map((m) => m.name));
      if (!mine.size) continue;
      const hit = this.scene.pick(
        x,
        y,
        (m) => mine.has(m.name),
        false,
        this.cameraFor(x, y),
      );
      if (!hit?.hit || !hit.pickedMesh || !hit.pickedPoint) continue;
      // A corner or a curve buried inside a solid blocking is not on the
      // screen, so it is not under the pointer either.
      if (this.behindTheShell(x, y, hit.distance)) continue;
      const tag = Number(hit.pickedMesh.name.split("-").pop());
      if (!Number.isFinite(tag)) continue;
      const p = hit.pickedPoint;
      return {
        spot: { dim: DIM_OF[group], tag },
        at: [p.x + this.center[0], p.y + this.center[1], p.z + this.center[2]],
      };
    }
    return null;
  }

  /**
   * Show the corners placed so far for a slab being laid, and the sides
   * between them.
   *
   * One point is a mark, two are a mark and a side, and four close the
   * quadrangle — so what is on screen says how many more clicks there are
   * without a word being written anywhere.
   */
  showLaying(points: readonly [number, number, number][]) {
    for (const mark of this.laying?.marks ?? []) mark.dispose();
    disposeLine(this.laying?.wire);
    this.laying = undefined;
    // A corner placed settles whatever was provisional about it.
    this.aimLaying([], null);
    if (!points.length) return;

    const here = points.map(
      (p) =>
        new Vector3(
          p[0] - this.center[0],
          p[1] - this.center[1],
          p[2] - this.center[2],
        ),
    );
    const marks = here.map((at, k) => {
      // A unit sphere, which is what the per-pane sizing pass expects — and
      // sized here as well as there, so that it is never drawn at a world
      // unit even for the one frame between being made and being scaled.
      const mark = CreateSphere(
        `laying${k}`,
        { diameter: 1, segments: 8 },
        this.scene,
      );
      mark.position = at;
      mark.scaling.setAll(
        worldSizeForPixels(
          2 * this.prefs.nodes.size,
          Vector3.Distance(at, this.camera.position),
          this.camera.fov,
          this.scene.getEngine().getRenderHeight(),
        ),
      );
      const material = new StandardMaterial(`layingMat${k}`, this.scene);
      material.emissiveColor = Color3.FromHexString(this.prefs.theme.corners);
      material.disableLighting = true;
      mark.material = material;
      mark.layerMask = MAIN_LAYER;
      mark.renderingGroupId = PASS.aiming;
      return mark;
    });

    let wire: GreasedLineMesh | undefined;
    if (here.length > 1) {
      // Closed once there are four: the last side is the one that makes it
      // a quadrangle rather than three sides of one.
      const ring = here.length === 4 ? [...here, here[0]] : here;
      const lines = ring
        .slice(1)
        .map((to, k) => [ring[k].x, ring[k].y, ring[k].z, to.x, to.y, to.z]);
      wire = CreateGreasedLine(
        "layingWire",
        { points: lines, updatable: false },
        {
          color: Color3.FromHexString(this.prefs.theme.corners),
          materialType: GreasedLineMeshMaterialType.MATERIAL_TYPE_SIMPLE,
          ...this.ribbon(2 / SHEET_PIXELS),
        },
        this.scene,
      ) as GreasedLineMesh;
      wire.layerMask = MAIN_LAYER;
      wire.renderingGroupId = PASS.aiming;
    }
    this.laying = { marks, wire };
    this.fitLimits();
  }

  /**
   * The sides that are not there yet: from the last corner placed to
   * wherever the pointer is, and — once three are down — on from there back
   * to the first.
   *
   * So the quadrangle is whole on screen before the fourth click rather
   * than after it, which is the difference between placing a corner and
   * finding out where it went. Drawn thinner than the sides that have been
   * settled, because it is a question and not an answer.
   *
   * `at` null takes it away. Nothing is added to the camera's limits from
   * here: a provisional point under the pointer is not part of the model,
   * and letting it push the limits about would make the view breathe as
   * the mouse moved.
   */
  aimLaying(
    points: readonly [number, number, number][],
    at: [number, number, number] | null,
  ) {
    disposeLine(this.layingAim);
    this.layingAim = undefined;
    this.layingAimSides = 0;
    if (at === null || !points.length || points.length > 3) return;

    const here = [...points, at, ...(points.length === 3 ? [points[0]] : [])];
    const lines = here
      .slice(points.length - 1)
      .map((p) => [
        p[0] - this.center[0],
        p[1] - this.center[1],
        p[2] - this.center[2],
      ]);
    const segments = lines
      .slice(1)
      .map((to, k) => [...lines[k], ...to]) as number[][];
    const wire = CreateGreasedLine(
      "layingAim",
      { points: segments, updatable: false },
      {
        color: Color3.FromHexString(this.prefs.theme.corners),
        materialType: GreasedLineMeshMaterialType.MATERIAL_TYPE_SIMPLE,
        ...this.ribbon(1 / SHEET_PIXELS),
      },
      this.scene,
    ) as GreasedLineMesh;
    wire.layerMask = MAIN_LAYER;
    wire.renderingGroupId = PASS.aiming;
    this.layingAim = wire;
    this.layingAimSides = segments.length;
  }

  /** How many provisional sides are drawn: one, or two once the quadrangle
   *  closes at the third corner. Zero when none is. */
  get aimedSides(): number {
    return this.layingAimSides;
  }

  /**
   * How big a mark for a corner being placed comes out on screen, in
   * pixels — the same arithmetic `handleAcross` undoes, and for the same
   * reason: to say that the sizing pass reached the picture.
   */
  layingAcross(): number | null {
    const mark = this.laying?.marks[0];
    if (!mark) return null;
    const camera = this.camera;
    const height = this.scene.getEngine().getRenderHeight();
    const distance = Vector3.Distance(mark.position, camera.position);
    return (
      (mark.scaling.x * height) / (2 * Math.tan(camera.fov / 2) * distance)
    );
  }

  /**
   * Show a geometry, replacing whatever was there.
   *
   * Asked for when the kernel's generation number says it has changed —
   * which is why this does the whole thing rather than patching: a new
   * geometry shares nothing with the old one, not even its node numbering —
   * and also when the scene's rebasing origin has moved, since the numbers
   * in the buffer are relative to it. In that second case it is the same
   * geometry, and the caller puts the selection back.
   */
  setGeometry(view: GeometryView) {
    this.geometry = view;
    this.pickedGeom = [];
    this.showHover(null);
    this.rebuildGeometry();
    // The geometry counts towards the model's extent, so how close the
    // camera may come has to be worked out again.
    this.fitLimits();
  }

  /**
   * The origin each of the three things on screen was drawn relative to.
   *
   * They have to be the same number. Everything is sent rebased on one
   * origin so that f32 coordinates stay small, and anything drawn against
   * a stale one sits an origin-shift away from the rest of the scene.
   */
  origins(): {
    view: readonly [number, number, number];
    geometry: readonly [number, number, number] | null;
    mesh: readonly [number, number, number] | null;
  } {
    return {
      view: this.center,
      geometry: this.geometry?.center ?? null,
      mesh: this.meshView?.center ?? null,
    };
  }

  /** How many handles are **drawn**, as against how many exist: hiding a
   *  group leaves its handles in the scene and takes them off the screen. */
  get nodesDrawn(): number {
    return this.nodes.filter((m) => m.isVisible).length;
  }

  /** What the geometry's three groups currently hold, for a test to read. */
  geomCount(group: GeomGroup): number {
    return this.geomMeshes[group].length;
  }

  /** The entity tags on screen in one group, in the order they are drawn. */
  geomTags(group: GeomGroup): number[] {
    return this.geomMeshes[group].map((m) => Number(m.name.split("-").pop()));
  }

  /** The color and opacity one geometric entity is drawn in — everything
   *  there is to assert about a group's display without rendering. */
  geomLook(
    group: GeomGroup,
    tag: number,
  ): { color: string; alpha: number } | null {
    const mesh = this.geomMeshes[group].find(
      (m) => m.name === `geom-${group}-${tag}`,
    );
    if (!mesh) return null;
    const material = mesh.material as StandardMaterial | null;
    // Where each group keeps the color it was given: a line mesh has no
    // material of its own, a corner is unlit and so carries it as its own
    // light, and a surface is lit and carries it as what the light falls on.
    const color =
      group === "curves"
        ? (mesh as LinesMesh).color
        : group === "corners"
          ? (material?.emissiveColor ?? Color3.Black())
          : (material?.diffuseColor ?? Color3.Black());
    return { color: color.toHexString().toLowerCase(), alpha: mesh.visibility };
  }

  private geomColor(group: GeomGroup, tag: number): Color3 {
    // Picked surfaces take the selection color, whatever the coloring
    // says — the same convention as a picked face, so there is one thing
    // to learn rather than two.
    if (this.pickedGeom.some((s) => s.dim === DIM_OF[group] && s.tag === tag)) {
      return Color3.FromHexString(ANCHOR);
    }
    return this.entityColor(group, tag);
  }

  /**
   * Show a mesh, or take the last one away.
   *
   * Thrown away and rebuilt rather than patched, unlike the blocking's own
   * surfaces: a mesh does not change a corner at a time — it is built
   * whole, looked at, and replaced by the next one.
   */
  setMesh(view: MeshView | null) {
    this.meshView = view?.blocks.length ? view : undefined;
    this.rebuildMesh();
  }

  /** How many quadrangles of skin are **drawn**, for a test to read. What
   *  the kernel holds is in its own stats; this is the picture. */
  get skinCount(): number {
    return this.meshSkin ? (this.meshView?.blocks.length ?? 0) : 0;
  }

  private rebuildMesh() {
    this.meshSkin?.dispose();
    this.meshSkin = undefined;
    this.meshWire?.dispose();
    this.meshWire = undefined;
    const view = this.meshView;
    const layer = this.prefs.mesh;
    if (!view || !layer.visible) return;

    const data = buildMesh(view, {
      uniform: this.prefs.theme.mesh,
      coloring: layer.coloring,
    });
    const skin = new Mesh("mesh-skin", this.scene);
    const vertices = new VertexData();
    vertices.positions = data.positions;
    vertices.indices = data.indices;
    vertices.colors = data.colors;
    vertices.normals = data.normals;
    vertices.applyToMesh(skin, false);
    const material = new StandardMaterial("meshMat", this.scene);
    material.specularColor = Color3.Black();
    material.backFaceCulling = false;
    material.alpha = layer.alpha;
    material.separateCullingPass = true;
    material.zOffset = SURFACE_DEPTH_BIAS.factor;
    material.zOffsetUnits = SURFACE_DEPTH_BIAS.units;
    skin.material = material;
    // Not pickable: a mesh is the answer rather than the thing being
    // worked on, and a click through it has to reach the blocking.
    skin.isPickable = false;
    skin.layerMask = MAIN_LAYER;
    skin.renderingGroupId = PASS.blocking;
    this.meshSkin = skin;

    // The element grid, which is the half of this that says what the
    // discretisation actually did.
    const flat = meshEdges(view);
    const lines: Vector3[][] = [];
    for (let at = 0; at < flat.length; at += 6) {
      lines.push([
        new Vector3(flat[at], flat[at + 1], flat[at + 2]),
        new Vector3(flat[at + 3], flat[at + 4], flat[at + 5]),
      ]);
    }
    if (lines.length) {
      this.meshWire = CreateLineSystem("mesh-edges", { lines }, this.scene);
      this.meshWire.color = Color3.FromHexString(this.prefs.theme.edges);
      this.meshWire.alpha = layer.alpha;
      this.meshWire.isPickable = false;
      this.meshWire.layerMask = MAIN_LAYER;
      this.meshWire.renderingGroupId = PASS.blocking;
    }
  }

  /** What a geometric entity is drawn in: one color for its group, or one
   *  per entity, as that group's own setting says. */
  private entityColor(group: GeomGroup, tag: number): Color3 {
    const layer: GeomLayer = this.prefs[group];
    return Color3.FromHexString(
      layer.coloring === "id"
        ? colorForId(tag, FAMILY[group])
        : this.prefs.theme[group],
    );
  }

  /**
   * The color of a cell of the **blocking**, by the *kind* of thing it is
   * classified on: a corner, a curve, a surface, or nothing.
   *
   * Three colors and a legend, not a hue per entity. What you read off a
   * `class` coloring is whether the blocking has taken hold of the
   * geometry, which is a question with four answers — and painting a
   * block face the color of the very surface it is classified on, on top of that
   * surface, made two things nobody could tell apart.
   */
  private classColor(dim: number, tag: number, free: string): Color3 {
    void tag;
    const named = (["onCorner", "onCurve", "onSurface"] as const)[dim];
    return Color3.FromHexString(named ? this.prefs.theme[named] : free);
  }

  /**
   * The color of one node handle: picked, then whatever the coloring
   * says.
   *
   * Being frozen is **not** in this list. It is drawn as a cube instead,
   * so that a frozen node can still be read for its id or for what it
   * classified on — the two things the coloring is there to show.
   */
  private nodeColor(index: number): Color3 {
    const plain = this.prefs.theme.nodes;
    if (!this.view) return Color3.FromHexString(plain);
    if (this.pickedNodes.includes(this.view.names[index]))
      return Color3.FromHexString(ANCHOR);
    switch (this.prefs.nodes.coloring) {
      case "class":
        return this.classColor(this.view.nodeClassDim[index], 0, plain);
      case "id":
        return Color3.FromHexString(
          colorForId(this.view.names[index], FAMILY.nodes),
        );
      case "group":
        return Color3.FromHexString(
          groupColor(
            this.view.nodeGroupsLo[index],
            this.view.nodeGroupsHi[index],
          ) ?? plain,
        );
      default:
        return Color3.FromHexString(plain);
    }
  }

  /**
   * Rebuild all three groups.
   *
   * Thrown away and remade rather than patched, unlike the blocking's
   * surfaces. There is no slider dragging a stream of these: a geometry has
   * no shrink, so the only things that change are a color, an opacity and
   * whether it is on at all — one click each, not one per mouse move.
   */
  private rebuildGeometry() {
    for (const group of GEOM_GROUPS) {
      for (const mesh of this.geomMeshes[group]) mesh.dispose(false, true);
      this.geomMeshes[group] = [];
    }
    if (!this.geometry) return;
    if (this.prefs.surfaces.visible) this.buildGeomSurfaces();
    if (this.prefs.curves.visible) this.buildGeomCurves();
    if (this.prefs.corners.visible) this.buildGeomCorners();
  }

  private buildGeomSurfaces() {
    for (const surface of geomSurfaces(this.geometry!)) {
      const mesh = new Mesh(`geom-surfaces-${surface.tag}`, this.scene);
      const vertices = new VertexData();
      vertices.positions = surface.positions;
      vertices.indices = surface.indices;
      const normals: number[] = [];
      // Averaged over the surface's own triangles and no others, because no
      // vertex is shared between surfaces: a faceted sphere reads as a
      // sphere, and the crease where two surfaces meet stays a crease.
      VertexData.ComputeNormals(
        surface.positions,
        Array.from(surface.indices),
        normals,
      );
      vertices.normals = normals;
      vertices.applyToMesh(mesh, false);

      const color = this.geomColor("surfaces", surface.tag);
      const material = new StandardMaterial(
        `geomSurfaceMat${surface.tag}`,
        this.scene,
      );
      material.diffuseColor = color;
      // A little of its own light, so a patch turned away from the lamp is
      // still the color it was given rather than a silhouette.
      material.emissiveColor = color.scale(0.35);
      material.specularColor = Color3.Black();
      material.backFaceCulling = false;
      material.separateCullingPass = true;
      // The curves run exactly along these surfaces, being made of the same
      // nodes. Same bias as the blocking's faces, for the same reason.
      material.zOffset = SURFACE_DEPTH_BIAS.factor;
      material.zOffsetUnits = SURFACE_DEPTH_BIAS.units;
      mesh.material = material;
      this.dressGeometry(mesh, "surfaces");
    }
  }

  private buildGeomCurves() {
    for (const curve of geomCurves(this.geometry!)) {
      const lines: Vector3[][] = [];
      for (let at = 0; at < curve.segments.length; at += 6) {
        lines.push([
          new Vector3(
            curve.segments[at],
            curve.segments[at + 1],
            curve.segments[at + 2],
          ),
          new Vector3(
            curve.segments[at + 3],
            curve.segments[at + 4],
            curve.segments[at + 5],
          ),
        ]);
      }
      const mesh = CreateLineSystem(
        `geom-curves-${curve.tag}`,
        { lines },
        this.scene,
      );
      mesh.color = this.geomColor("curves", curve.tag);
      this.dressGeometry(mesh, "curves");
    }
  }

  private buildGeomCorners() {
    for (const corner of geomCorners(this.geometry!)) {
      // Unit diameter; `rescaleHandles` sizes it in pixels every frame, the
      // same way the node handles are sized.
      const mesh = CreateSphere(
        `geom-corners-${corner.tag}`,
        { diameter: 1, segments: 8 },
        this.scene,
      );
      mesh.position = new Vector3(...corner.at);
      const material = new StandardMaterial(
        `geomCornerMat${corner.tag}`,
        this.scene,
      );
      material.emissiveColor = this.geomColor("corners", corner.tag);
      material.disableLighting = true;
      mesh.material = material;
      this.dressGeometry(mesh, "corners");
    }
  }

  /**
   * What every piece of geometry has in common.
   *
   * **Never pickable**, which is the important one. The geometry is what the
   * blocking is built against, not something you edit, and a triangle in
   * front of a block face that answered the click first would make that face
   * unselectable — the same trap the node handles fell into when surfaces
   * first became pickable.
   *
   * Opacity through `visibility` rather than the material's alpha, because a
   * `LinesMesh` has no material of its own to set it on.
   */
  private dressGeometry(mesh: Mesh | LinesMesh, group: GeomGroup) {
    mesh.visibility = this.prefs[group].alpha;
    mesh.isPickable = false;
    mesh.layerMask = MAIN_LAYER;
    mesh.renderingGroupId = PASS.geometry;
    this.geomMeshes[group].push(mesh as Mesh);
  }

  private rebuildEdges() {
    if (!this.view?.edges.length || !this.prefs.edges.visible) {
      this.wire?.dispose();
      this.wire = undefined;
      return;
    }
    const lines = this.segments().map(([a, b]) => [
      a.x,
      a.y,
      a.z,
      b.x,
      b.y,
      b.z,
    ]);
    const colors = this.edgeColors().map(([c]) => new Color3(c.r, c.g, c.b));
    // Kept, so that a drag can rewrite the entries that moved rather than
    // building this list again from every edge in the model.
    this.wirePoints = lines;
    this.wireEdges = this.edgesShown();
    // Reused while the shape of the thing has not changed: a drag rebuilds
    // this on every frame, and rewriting the points of a ribbon that is
    // already there costs a fraction of building one.
    if (this.wire && this.wireSegments === lines.length) {
      this.wire.setPoints(lines);
      this.dressEdges(colors);
      return;
    }
    this.wire?.dispose(false, true);
    this.wire = CreateGreasedLine(
      "edges",
      {
        points: lines,
        updatable: true,
        // Which color each vertex reads, said rather than left to the
        // ribbon. Left to it, it numbers them by **point** across all its
        // lines, so edge k read the colors of edges 2k and 2k + 1 off a
        // table of one per edge. And it reads entry i at i / width, the
        // boundary between two texels, which is only right as long as the
        // rounding goes one way. So: the four vertices of edge k (two per
        // end) all read the middle of entry k. Kept by `setPoints`, which is right
        // while the number of edges is the same, the only time it is used.
        colorPointers: lines.flatMap((_, k) => [k, k, k, k].map((i) => i + 0.5)),
      },
      {
        colors: colors,
        useColors: true,
        // One color per segment: an edge wears what it is classified on,
        // and each segment of this ribbon **is** an edge.
        colorDistributionType:
          GreasedLineMeshColorDistributionType.COLOR_DISTRIBUTION_TYPE_SEGMENT,
        // The unlit material rather than the lit one the builder reaches
        // for by default: a wireframe takes no light, and this is the one
        // that can be handed new colors without being rebuilt.
        materialType: GreasedLineMeshMaterialType.MATERIAL_TYPE_SIMPLE,
        ...this.ribbon(this.prefs.edges.width / SHEET_PIXELS),
      },
      this.scene,
    ) as GreasedLineMesh;
    this.wireSegments = lines.length;
    // Pickable, because an edge is what a sheet is named by. The handles
    // still win: the scene's pointer predicates only ever see those, and
    // this is reached by `pickEdge` on a click that missed one.
    this.wire.isPickable = true;
    this.wire.layerMask = MAIN_LAYER;
    this.wire.renderingGroupId = PASS.blocking;
  }

  /**
   * Write the colors **and the width** onto a ribbon that is being kept.
   *
   * Both, because both are settings and a kept ribbon hears about a
   * setting only here: writing one and not the other is how a width that
   * was asked for went nowhere until the model's shape happened to change.
   */
  private dressEdges(colors: Color3[]) {
    const material = this.wire?.material as
      GreasedLineSimpleMaterial | undefined;
    if (!material) return;
    material.setColors(colors, false, true);
    material.width = this.ribbon(this.prefs.edges.width / SHEET_PIXELS).width;
  }

  /**
   * The width and resolution a ribbon is built with.
   *
   * In **render** pixels, which is what the material's `resolution` is in,
   * and which on a high-density display is twice the CSS size — hence the
   * division, so seven means seven of the pixels a pointer speaks in.
   */
  private ribbon(share: number) {
    const engine = this.scene.getEngine();
    return {
      width: (SHEET_PIXELS * share) / engine.getHardwareScalingLevel(),
      sizeAttenuation: true,
      resolution: new Vector2(
        engine.getRenderWidth(),
        engine.getRenderHeight(),
      ),
    };
  }

  /**
   * Fit everything to the canvas's new shape.
   *
   * The orthographic panes keep their half-height and take a new width
   * from it, so a window made wider shows more to the sides rather than
   * stretching what was there.
   */
  resize() {
    const sized = this.scene.getEngine();
    const css = sized.getHardwareScalingLevel();
    const wide = (sized.getRenderWidth() || 1) * css;
    const tall = (sized.getRenderHeight() || 1) * css;
    for (const pane of PANES) {
      if (!PANE_AXIS[pane]) continue;
      const rect = paneRect(pane, this.quad, wide, tall);
      const extents = orthoExtents(this.half[pane], rect.width, rect.height);
      const camera = this.panes[pane];
      camera.orthoLeft = extents.left;
      camera.orthoRight = extents.right;
      camera.orthoTop = extents.top;
      camera.orthoBottom = extents.bottom;
    }
    // In the corner of the 3-D pane, not of the window: the triad reports
    // the 3-D camera's rotation, and in the bottom right of a split window
    // it would sit in the Z view saying something about another view.
    this.axes.resize(paneRect("3d", this.quad, wide, tall), wide, tall);
    // A ribbon's width is in pixels of the resolution it was built with, and
    // the material holds that resolution from the moment it was made. Resize
    // the window with a sheet lit and it would go on measuring itself
    // against the old one.
    for (const [mesh, share] of [
      [this.litWire, 1],
      [this.aimWire, AIM_SHARE],
      [this.haloLine, this.haloShare],
    ] as const) {
      const material = (mesh as GreasedLineMesh | undefined)
        ?.greasedLineMaterial;
      if (!material) continue;
      const { width, resolution } = this.ribbon(share);
      material.width = width;
      material.resolution = resolution;
    }
  }

  attachTo(canvas: HTMLCanvasElement) {
    this.held = canvas;
    this.attachHovered();
    canvas.addEventListener("pointermove", (e) =>
      this.aimAt(e.offsetX, e.offsetY),
    );
    // An orthographic camera's distance changes nothing but what it clips,
    // so the wheel has to move the extents instead. A share of what is
    // left, like the perspective pane's, so it feels the same at every
    // scale.
    canvas.addEventListener(
      "wheel",
      (e) => {
        if (!PANE_AXIS[this.hovered]) return;
        e.preventDefault();
        const by = Math.exp(Math.sign(e.deltaY) * ZOOM_PER_NOTCH * 4);
        this.half[this.hovered] = Math.max(1e-6, this.half[this.hovered] * by);
        this.resize();
      },
      { passive: false },
    );
  }

  /** Take the pointer away from the camera, and give it back.
   *
   *  Aiming a cut is a drag along an edge, and a drag is also how the view
   *  is turned. One of them has to stand aside. */
  suspendCamera() {
    for (const pane of PANES) this.panes[pane].detachControl();
  }

  resumeCamera() {
    this.attachHovered();
  }

  /** Called when a drag ends, with the node's new position in model space. */
  onDrop(listener: Drop) {
    this.dropped.push(listener);
  }

  setView(view: View) {
    this.center = view.center;
    // A ring around a cell of the blocking that has just been replaced is a
    // ring around nothing. The next pointer movement puts it back, over
    // whatever is there now.
    this.showHover(null);
    // The topology may be another one, so what each node touches has to be
    // worked out again. Dropped rather than rebuilt: the next drag asks.
    this.around = undefined;
    // A copy, because the drag writes into it. The incoming arrays are views
    // onto the kernel's bytes.
    this.view = {
      center: view.center,
      positions: Float32Array.from(view.positions),
      names: Uint32Array.from(view.names),
      edges: Uint32Array.from(view.edges),
      edgeNames: Uint32Array.from(view.edgeNames),
      quads: Uint32Array.from(view.quads),
      quadFaces: Uint32Array.from(view.quadFaces),
      quadBlocks: Uint32Array.from(view.quadBlocks),
      nodeClassDim: Int32Array.from(view.nodeClassDim),
      nodeClassTag: Int32Array.from(view.nodeClassTag),
      nodeFrozen: Uint32Array.from(view.nodeFrozen),
      edgeClassDim: Int32Array.from(view.edgeClassDim),
      edgeClassTag: Int32Array.from(view.edgeClassTag),
      quadClassDim: Int32Array.from(view.quadClassDim),
      quadClassTag: Int32Array.from(view.quadClassTag),
      nodeGroupsLo: Uint32Array.from(view.nodeGroupsLo),
      nodeGroupsHi: Uint32Array.from(view.nodeGroupsHi),
      edgeGroupsLo: Uint32Array.from(view.edgeGroupsLo),
      edgeGroupsHi: Uint32Array.from(view.edgeGroupsHi),
      quadGroupsLo: Uint32Array.from(view.quadGroupsLo),
      quadGroupsHi: Uint32Array.from(view.quadGroupsHi),
      nodeLoose: Uint32Array.from(view.nodeLoose),
      edgeLoose: Uint32Array.from(view.edgeLoose),
    };
    this.nodes.forEach((m) => m.dispose());
    this.nodes = [];
    this.rebuildEdges();
    this.rebuildHighlight();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");

    for (let n = 0; n < view.positions.length / 3; n++) {
      // A cube for a node that is frozen, a ball for one that can move.
      // The *shape* says whether it is out of play, which leaves its
      // color free to say whatever the coloring is set to — a frozen
      // node still shows its id or its classification, which it could not
      // when being frozen was a color of its own.
      //
      // Unit size; the per-frame pass below scales it to a fixed number
      // of pixels.
      const s =
        view.nodeFrozen[n] === 1
          ? CreateBox(`node${n}`, { size: FROZEN_SIDE }, this.scene)
          : CreateSphere(`node${n}`, { diameter: 1, segments: 8 }, this.scene);
      s.position = this.point(n);
      // The setting holds across a refresh: new handles arrive hidden when
      // the handles are hidden.
      s.isVisible = this.prefs.nodes.visible;
      // The kernel's name, not this index: the index is only good until the
      // next edit reorders the view.
      s.metadata = { index: n, name: view.names[n] };
      const m = new StandardMaterial(`nodeMat${n}`, this.scene);
      m.emissiveColor = this.nodeColor(n);
      m.disableLighting = true;
      s.material = m;
      s.layerMask = MAIN_LAYER;
      s.renderingGroupId = PASS.blocking;
      this.nodes.push(s);
      this.makeDraggable(s, n);
    }
    this.fitLimits();
  }

  /**
   * Drag and drop, one gesture.
   *
   * A behavior per node rather than one attached on selection: attaching on
   * the press that should *start* the drag misses that press, which turns the
   * gesture into click-then-drag. Cheap at this size; an instanced hit target
   * goes here when it is not.
   *
   * The kernel hears once, at the drop. Telling it on every mouse move would
   * be a network round trip per pixel in the station.
   */
  private makeDraggable(sphere: Mesh, index: number) {
    const drag = new PointerDragBehavior({});
    drag.useObjectOrientationForDragging = false;
    // Babylon eases the node towards the pointer, a fifth of the way each
    // frame, to damp jitter from a tracked controller. With a mouse it means
    // the node lands wherever the easing had got to rather than where you
    // pointed — and placing a vertex is exactly the case where that matters.
    drag.dragDeltaRatio = 1;
    // A frozen node is not draggable at all. Letting it be dragged and
    // having the kernel refuse would show it moving and then snapping
    // back, which looks like a bug rather than like a locked node.
    drag.enabled = this.view?.nodeFrozen[index] !== 1;
    sphere.addBehavior(drag);
    this.setDragPlane(sphere);
    let from = sphere.position.clone();
    /** The other picked handles and where each of them started, while a
     *  drag that takes them along is running. */
    let carried: { mesh: Mesh; index: number; from: Vector3 }[] = [];
    drag.onDragStartObservable.add(() => {
      from = sphere.position.clone();
      // Picked handles come along; one that is not picked moves alone,
      // and leaves whatever is picked where it is. Decided at the start of
      // the gesture, so the selection cannot change under it.
      const picked = this.pickedNodes;
      carried =
        picked.length > 1 && picked.includes(sphere.metadata.name as number)
          ? this.nodes
              .filter(
                (m) =>
                  m !== sphere && picked.includes(m.metadata.name as number),
              )
              .map((mesh) => ({
                mesh,
                index: mesh.metadata.index as number,
                from: mesh.position.clone(),
              }))
          : [];
    });
    drag.onDragObservable.add(() => {
      const by = sphere.position.subtract(from);
      const moved = [index];
      this.place(index, sphere.position);
      for (const other of carried) {
        other.mesh.position.copyFrom(other.from.add(by));
        this.place(other.index, other.mesh.position);
        moved.push(other.index);
      }
      // One redraw for the step, not one per handle: two nodes of the same
      // block would otherwise rebuild it twice.
      this.redrawAround(moved);
    });
    drag.onDragEndObservable.add(() => {
      const p = sphere.position;
      // A press and release on the spot is a click, not a move. Without
      // this it is a command all the same — and now a command that would
      // pin the node to whatever it happens to be near.
      if (Vector3.Distance(from, p) < 1e-9) return;
      const name = sphere.metadata.name as number;
      const to: [number, number, number] = [
        p.x + this.center[0],
        p.y + this.center[1],
        p.z + this.center[2],
      ];
      const also = carried.map((o) => o.mesh.metadata.name as number);
      carried = [];
      this.dropped.forEach((listener) => listener(name, to, also));
    });
  }

  /**
   * Take a new set of node places and classifications, and redraw what moved.
   *
   * What a drop does when the only thing the kernel changed is where a node
   * is. Everything else the viewer holds — the edges, the quadrangles, their
   * names, their colors — is still right, and rebuilding it from a whole view
   * is what used to make dropping a node at five hundred blocks a quarter of
   * a second's wait.
   *
   * Returns **false** when this cannot be believed, and then the caller asks
   * for the whole view: a different set of names, a different count, an
   * origin that has shifted under the quadrangles still held here, or a node
   * that has been frozen or thawed — a frozen handle is a cube and a thawed
   * one a ball, and swapping a mesh is not an update.
   */
  applyNodes(fresh: NodesView): boolean {
    const view = this.view;
    if (!view || fresh.names.length !== view.names.length) return false;
    for (let k = 0; k < 3; k++)
      if (fresh.center[k] !== this.center[k]) return false;
    for (let i = 0; i < fresh.names.length; i++) {
      if (fresh.names[i] !== view.names[i]) return false;
      if (fresh.frozen[i] !== view.nodeFrozen[i]) return false;
    }

    const moved: number[] = [];
    for (let i = 0; i < fresh.names.length; i++) {
      const p = i * 3;
      if (
        view.positions[p] !== fresh.positions[p] ||
        view.positions[p + 1] !== fresh.positions[p + 1] ||
        view.positions[p + 2] !== fresh.positions[p + 2]
      ) {
        view.positions[p] = fresh.positions[p];
        view.positions[p + 1] = fresh.positions[p + 1];
        view.positions[p + 2] = fresh.positions[p + 2];
        moved.push(i);
      }
    }

    // The classification may have settled onto the geometry on the way
    // past, and that is a color. Copied for every node rather than for the
    // ones that moved: it is one short array, and a node that was dragged
    // off a curve changes what its neighbors are beside.
    view.nodeClassDim.set(fresh.classDim);
    view.nodeClassTag.set(fresh.classTag);
    view.nodeGroupsLo.set(fresh.groupsLo);
    view.nodeGroupsHi.set(fresh.groupsHi);
    view.nodeLoose.set(fresh.loose);

    for (const i of moved) {
      const handle = this.nodes[i];
      if (handle) handle.position = this.point(i);
    }
    for (let i = 0; i < this.nodes.length; i++) {
      const material = this.nodes[i]?.material as StandardMaterial | undefined;
      if (material) material.emissiveColor = this.nodeColor(i);
    }
    this.redrawAround(moved);
    this.fitLimits();
    return true;
  }

  /** Move a node in the working copy, without redrawing anything yet.
   *
   *  The handle comes along. When a handle is being dragged it is already
   *  where this puts it — Babylon moved it, and the position written back
   *  is the one read off it — but a gesture that moves nodes *without*
   *  dragging their handles, as sliding a face does, would otherwise leave
   *  them behind, and the model would be right while the thing you grab
   *  was somewhere else. */
  private place(index: number, to: Vector3) {
    if (!this.view) return;
    this.view.positions[index * 3] = to.x;
    this.view.positions[index * 3 + 1] = to.y;
    this.view.positions[index * 3 + 2] = to.z;
    const handle = this.nodes[index];
    if (handle) handle.position.copyFrom(to);
  }

  /**
   * Redraw what these nodes touch, and nothing else.
   *
   * A drag used to rebuild the whole wireframe and every block's surface on
   * every pointer move — at five hundred blocks, a quarter of a second to
   * redraw two thousand edges and three thousand quadrangles of which six
   * and a dozen had moved. The topology has not changed, so what a node
   * moves is its own ring: the edges that reach it, and the blocks it is a
   * corner of.
   *
   * The blocks and not merely their faces, because a block's quadrangles are
   * shrunk towards its center and the center moves with the corner.
   */
  private redrawAround(nodes: number[]) {
    if (!this.view) return;
    const index = this.ring();
    const edges = new Set<number>();
    const blocks = new Set<number>();
    for (const node of nodes) {
      for (const e of index.edges.get(node) ?? []) edges.add(e);
      for (const b of index.blocks.get(node) ?? []) blocks.add(b);
    }
    this.redrawEdges(edges);
    this.redrawBlocks("faces", blocks);
    this.redrawBlocks("blocks", blocks);
  }

  /** Which edges and blocks each node belongs to. */
  private ring(): {
    edges: Map<number, number[]>;
    blocks: Map<number, number[]>;
  } {
    if (this.around) return this.around;
    const view = this.view!;
    const edges = new Map<number, number[]>();
    const blocks = new Map<number, number[]>();
    const add = (to: Map<number, number[]>, node: number, what: number) => {
      const already = to.get(node);
      if (already) {
        if (!already.includes(what)) already.push(what);
      } else {
        to.set(node, [what]);
      }
    };
    for (let e = 0; e < view.edgeNames.length; e++) {
      add(edges, view.edges[e * 2], e);
      add(edges, view.edges[e * 2 + 1], e);
    }
    for (let q = 0; q < view.quadBlocks.length; q++) {
      for (let k = 0; k < 4; k++)
        add(blocks, view.quads[q * 4 + k], view.quadBlocks[q]);
    }
    this.around = { edges, blocks };
    return this.around;
  }

  /** Rewrite the points of these edges, leaving the rest of the ribbon. */
  private redrawEdges(which: Set<number>) {
    if (!this.wire || !this.wirePoints || !this.wireEdges || !which.size)
      return;
    const flat = edgeSegments(this.view!, this.prefs.edges.shrink);
    let moved = false;
    this.wireEdges.forEach((e, i) => {
      if (!which.has(e)) return;
      const s = e * 6;
      const to = this.wirePoints![i];
      for (let k = 0; k < 6; k++) to[k] = flat[s + k];
      moved = true;
    });
    // The colors are untouched: a node that moves changes no edge's
    // classification, its group or its name.
    if (moved) this.wire.setPoints(this.wirePoints);
  }

  /** Rebuild the surfaces of these blocks, leaving their neighbours alone. */
  private redrawBlocks(group: Group, which: Set<number>) {
    const layer = this.layerOf(group);
    const slots = this.surfaces[group];
    if (!this.view || !layer.visible || !slots.length || !which.size) return;
    const built = buildSurfacesOf(this.view, this.surfaceOptions(group), which);
    for (const slot of slots) {
      const data = built.get(slot.block);
      if (!data) continue;
      // Only if the shape is the same, which it is: a move changes where
      // the corners are and not how many there are. Anything else is a
      // topological change, and those come back through the view.
      if (data.positions.length !== slot.data.positions.length) continue;
      slot.data = data;
      slot.mesh.updateVerticesData(VertexBuffer.PositionKind, data.positions);
      slot.mesh.updateVerticesData(VertexBuffer.NormalKind, data.normals);
    }
  }

  private point(i: number): Vector3 {
    const p = this.view!.positions;
    return new Vector3(p[i * 3], p[i * 3 + 1], p[i * 3 + 2]);
  }

  /**
   * A color per end of every edge.
   *
   * Classified, an edge wears the color of the curve or surface it
   * classified on; unclassified, the wireframe's own. Both ends the same: an
   * edge is on one entity or none, and a gradient would suggest otherwise.
   */
  private edgeColors(): Color4[][] {
    const plain = this.prefs.theme.edges;
    const how = this.prefs.edges.coloring;
    const out: Color4[][] = [];
    for (const e of this.edgesShown()) {
      const color =
        how === "class"
          ? this.classColor(this.view!.edgeClassDim[e], 0, plain)
          : how === "id"
            ? Color3.FromHexString(
                colorForId(this.view!.edgeNames[e], FAMILY.edges),
              )
            : how === "group"
              ? Color3.FromHexString(
                  groupColor(
                    this.view!.edgeGroupsLo[e],
                    this.view!.edgeGroupsHi[e],
                  ) ?? plain,
                )
              : Color3.FromHexString(plain);
      out.push([color.toColor4(1), color.toColor4(1)]);
    }
    return out;
  }

  /**
   * The names of the edges the filters let through, or `null` when they let
   * everything through.
   *
   * What a lit sheet and the marks along it are clipped to. A sheet runs
   * through the whole model and a filter shows part of it, so lighting one
   * used to draw its far half over empty space — a thread with nothing on
   * it, in a view whose whole point is that the rest is out of the way.
   */
  private drawnEdgeNames(): ReadonlySet<number> | null {
    const everyGroup =
      this.shown.lo === 0xffffffff &&
      this.shown.hi === 0xffffffff &&
      this.shown.ungrouped;
    if (everyGroup && !this.only) return null;
    return new Set(this.edgesShown().map((e) => this.view!.edgeNames[e]));
  }

  /** Which edges of the view are in a group being looked at, in order. */
  private edgesShown(): number[] {
    const out: number[] = [];
    for (let e = 0; e < (this.view?.edgeNames.length ?? 0); e++) {
      if (this.only && !this.only.edges.has(this.view!.edgeNames[e])) continue;
      const loose = this.view!.edgeLoose[e] === 1;
      if (
        this.inShown(
          this.view!.edgeGroupsLo[e],
          this.view!.edgeGroupsHi[e],
          loose,
        )
      ) {
        out.push(e);
      }
    }
    return out;
  }

  private segments(): Vector3[][] {
    const flat = edgeSegments(this.view!, this.prefs.edges.shrink);
    const out: Vector3[][] = [];
    for (const e of this.edgesShown()) {
      const s = e * 6;
      out.push([
        new Vector3(flat[s], flat[s + 1], flat[s + 2]),
        new Vector3(flat[s + 3], flat[s + 4], flat[s + 5]),
      ]);
    }
    return out;
  }

  /** Give every handle the color its node has earned, and show or hide
   *  the lot of them. */
  private paintNodes() {
    this.nodes.forEach((mesh, n) => {
      (mesh.material as StandardMaterial).emissiveColor = this.nodeColor(n);
      // Hidden means hidden from the pointer too. A handle you cannot see
      // but can still grab is worse than one you can see: the model moves
      // and nothing on screen says what you took hold of.
      mesh.isVisible =
        this.prefs.nodes.visible &&
        (!this.only || this.only.nodes.has(this.view?.names[n] ?? -1)) &&
        this.inShown(
          this.view?.nodeGroupsLo[n] ?? 0,
          this.view?.nodeGroupsHi[n] ?? 0,
          (this.view?.nodeLoose[n] ?? 0) === 1,
        );
    });
  }

  /** The nodes picked, by the kernel's names. */
  get selectedNodes(): number[] {
    return [...this.pickedNodes];
  }

  selectNodes(names: readonly number[]) {
    // Only those the view still has: a node named before a cut that
    // consumed it is a name nothing answers to, and a selection that
    // outlives its cells is a command refused later for no visible reason.
    const live = new Set(this.view?.names ?? []);
    this.pickedNodes = names.filter((n) => live.has(n));
    this.paintNodes();
  }

  /**
   * Every node whose handle falls inside a rectangle on screen, in CSS
   * pixels.
   *
   * Projected one by one rather than picked: a pick asks what is in front,
   * and a lasso means *everything in there*, including the handles on the
   * far side of the model. Which is what you want when the four corners
   * you are after are the four at the bottom.
   */
  nodesInRect(x0: number, y0: number, x1: number, y1: number): number[] {
    const [left, right] = x0 < x1 ? [x0, x1] : [x1, x0];
    const [top, bottom] = y0 < y1 ? [y0, y1] : [y1, y0];
    // Through the camera of the pane the band was **begun** in. The pointer
    // may wander into another pane while it is drawn — the rectangle is
    // anchored where it started, and so is what it means.
    const pane = this.paneFor(x0, y0);
    const out: number[] = [];
    for (const mesh of this.nodes) {
      // Hidden handles are not caught by a band either: what the band is
      // for is picking what you can see.
      if (!mesh.isVisible) continue;
      const at = this.toScreen(mesh.position, pane);
      if (!at) continue;
      if (at[0] >= left && at[0] <= right && at[1] >= top && at[1] <= bottom) {
        out.push(mesh.metadata.name as number);
      }
    }
    return out;
  }

  /** What a node is classified on, and whether it may move — for the
   *  readout and for the keys that act on a picked node. */
  nodeState(
    name: number,
  ): { on: { dim: number; tag: number } | null; frozen: boolean } | null {
    if (!this.view) return null;
    const at = this.view.names.indexOf(name);
    if (at < 0) return null;
    const dim = this.view.nodeClassDim[at];
    return {
      on: dim < 0 ? null : { dim, tag: this.view.nodeClassTag[at] },
      frozen: this.view.nodeFrozen[at] === 1,
    };
  }

  /**
   * How a node's handle is **drawn**, read off the mesh that is on screen
   * rather than off the flag it was built from — a cube has eight corners
   * and so twenty-four vertices, three to a corner, where the ball has one
   * per point of its lattice. Asking the mesh is what makes this worth
   * asking: the flag is already known, the shape is the thing that might
   * not have followed it.
   */
  nodeShape(name: number): "cube" | "ball" | null {
    const mesh = this.nodes.find((m) => m.metadata.name === name);
    if (!mesh) return null;
    return mesh.getTotalVertices() === 24 ? "cube" : "ball";
  }

  /** The color a node's handle is wearing, as `#rrggbb`. */
  nodeTint(name: number): string | null {
    const mesh = this.nodes.find((m) => m.metadata.name === name);
    const material = mesh?.material as StandardMaterial | undefined;
    return material ? material.emissiveColor.toHexString().toLowerCase() : null;
  }

  /**
   * How far along the ray through these coordinates the model's shell stops
   * it, or `Infinity` when nothing in the way is solid.
   *
   * Only what is drawn **opaque**. A translucent shell is one you can see
   * through, and what you can see you may pick: the rule follows the
   * picture rather than the model, which is why the test is made against
   * the meshes actually on screen. Shrinking the blocks opens gaps and the
   * ray goes through them; hiding a layer or filtering a group takes the
   * surface out of the scene and out of this with it. None of that has to
   * be asked about separately.
   *
   * The discretised mesh is not here, and not by omission: it carries its
   * own `isPickable = false`, which already says a click through it has to
   * reach the blocking.
   */
  private shellAt(x: number, y: number): number {
    const solid = new Set<number>();
    for (const group of ["faces", "blocks"] as const) {
      if (this.layerOf(group).alpha < 1) continue;
      for (const slot of this.surfaces[group]) solid.add(slot.mesh.uniqueId);
    }
    if (this.prefs.surfaces.alpha >= 1) {
      for (const mesh of this.geomMeshes.surfaces) solid.add(mesh.uniqueId);
    }
    if (!solid.size) return Infinity;
    const hit = this.scene.pick(
      x,
      y,
      (m) => solid.has(m.uniqueId) && m.isVisible,
      false,
      this.cameraFor(x, y),
    );
    return hit?.hit ? hit.distance : Infinity;
  }

  /**
   * How far proud of the shell a mark drawn **on** it can stand, in world
   * units at this distance.
   *
   * Handles and the wireframe are sized in pixels, so a handle on a front
   * corner sticks out towards the camera by a world amount that depends on
   * how far away it is and on which camera is looking — and an edge runs
   * exactly along the surface it borders, so without this slack the two
   * come back at the same distance and the depth test between them is a
   * coin toss. This undoes the sizing pass, as `handleAcross` does, for
   * both kinds of camera: the quad panes are orthographic, where a pixel
   * is the same size however far away it is.
   */
  private markReach(camera: ArcRotateCamera, distance: number): number {
    const height = this.scene.getEngine().getRenderHeight();
    const pixels = Math.max(this.prefs.nodes.size, this.prefs.edges.width, 1);
    const across =
      camera.mode === Camera.ORTHOGRAPHIC_CAMERA
        ? (camera.orthoTop ?? 0) - (camera.orthoBottom ?? 0)
        : 2 * Math.tan(camera.fov / 2) * distance;
    return (pixels * across) / height;
  }

  /** Whether a hit that far along the ray is behind the model's shell —
   *  something you are looking at the back of rather than at. */
  private behindTheShell(x: number, y: number, distance: number): boolean {
    const shell = this.shellAt(x, y);
    if (!Number.isFinite(shell)) return false;
    return distance > shell + this.markReach(this.cameraFor(x, y), shell);
  }

  /** Screen coordinates -> the kernel's name for the node under them. */
  pickNode(x: number, y: number): number | null {
    // Explicitly the main camera: after a frame, `scene.activeCamera` is
    // whichever rendered last, which is the corner gizmo.
    const hit = this.scene.pick(
      x,
      y,
      (m) => (m.metadata?.name ?? -1) >= 0 && m.isVisible,
      false,
      this.cameraFor(x, y),
    );
    if (!hit?.hit || !hit.pickedMesh) return null;
    // Not through a solid model: a handle you cannot see is not one you
    // were aiming at.
    if (this.behindTheShell(x, y, hit.distance)) return null;
    return hit.pickedMesh.metadata.name as number;
  }

  /**
   * The handle under these coordinates, preferring one that is **on the
   * geometry**.
   *
   * `pickNode` answers with the nearest, which is what grabbing one wants.
   * Laying a quadrangle against the model wants something else: a block
   * sitting on a surface has a node inside the material directly behind
   * each of the ones on it, and from most angles the two are a pixel apart
   * on screen. The hidden one is never the corner you were aiming at — a
   * node that follows nothing is not a place anybody lays a new face onto.
   *
   * So: the nearest among those that are classified, and the plain nearest
   * when none of them is.
   */
  pickHandle(x: number, y: number): number | null {
    const hits =
      this.scene.multiPick(
        x,
        y,
        (m) => (m.metadata?.name ?? -1) >= 0 && m.isVisible,
        this.cameraFor(x, y),
      ) ?? [];
    const names = hits
      .filter((hit) => hit.hit && hit.pickedMesh)
      .sort((a, b) => a.distance - b.distance)
      // The shell stops this one too. The rule above is about which of two
      // handles a pixel apart you meant; this is about whether either of
      // them is on the screen at all.
      .filter((hit) => !this.behindTheShell(x, y, hit.distance))
      .map((hit) => hit.pickedMesh!.metadata.name as number);
    return names.find((name) => this.nodeState(name)?.on) ?? names[0] ?? null;
  }

  /** The node with this name, in **CSS pixels** — the inverse of `pickNode`,
   *  in the frame that takes, and what lets a test aim a real mouse at a real
   *  handle. */
  nodeScreen(name: number): [number, number] | null {
    const mesh = this.nodes.find((m) => m.metadata.name === name);
    return mesh ? this.toScreen(mesh.position) : null;
  }

  /**
   * Screen coordinates -> the name of the edge under them.
   *
   * Babylon reports which line segment it hit, and the segments are the
   * edges in the order the view lists them.
   */
  pickEdge(x: number, y: number): number | null {
    if (!this.wire || !this.view) return null;
    const hit = this.scene.pick(
      x,
      y,
      (m) => m === this.wire,
      false,
      this.cameraFor(x, y),
    );
    // A ribbon answers with **where** it was hit and not with which of its
    // triangles: its `intersects` is its own, written for lines, and it
    // fills in the point rather than a face. So the segment is the one
    // that point is nearest — which is the same question asked in the
    // same space, and exact to the width of the ribbon.
    if (!hit?.hit || !hit.pickedPoint) return null;
    if (this.behindTheShell(x, y, hit.distance)) return null;
    return this.edgeNearest(hit.pickedPoint);
  }

  /** The name of the edge whose drawn segment passes nearest a point. */
  private edgeNearest(point: Vector3): number | null {
    if (!this.view) return null;
    const flat = edgeSegments(this.view, this.prefs.edges.shrink);
    let best: number | null = null;
    let nearest = Infinity;
    for (const e of this.edgesShown()) {
      const at = e * 6;
      const from = new Vector3(flat[at], flat[at + 1], flat[at + 2]);
      const to = new Vector3(flat[at + 3], flat[at + 4], flat[at + 5]);
      const along = to.subtract(from);
      const length = along.lengthSquared();
      // A degenerate edge is a point, and the distance to it is the
      // distance to that point.
      const t =
        length === 0
          ? 0
          : Math.min(
              1,
              Math.max(0, Vector3.Dot(point.subtract(from), along) / length),
            );
      const far = point.subtract(from.add(along.scale(t))).lengthSquared();
      if (far < nearest) {
        nearest = far;
        best = this.view.edgeNames[e];
      }
    }
    return best;
  }

  /**
   * Light up a set of edges by name.
   *
   * A second line mesh rather than per-vertex colors on the first: the
   * wireframe is rebuilt on every drag frame, and this changes only when a
   * selection does. It is drawn in a later rendering group, so it shows
   * **through** the model — which is the point, since a sheet runs through
   * it and where it comes out the far side is what you want to see.
   */
  showSheet(edges: readonly SheetEdge[]) {
    this.lit = edges.map((e) => ({ ...e }));
    this.aim = null;
    this.rebuildHighlight();
  }

  get highlighted(): number[] {
    return this.lit.map((e) => e.id);
  }

  /**
   * Show where a cut at `t` would land, on every edge of the sheet at once.
   *
   * Drawn as a second ribbon from each edge's `from` end up to the cut, so
   * the join between the two colors *is* the plane. One edge would not be
   * enough: what the parameter does to the rest of the sheet is the thing
   * worth seeing before committing to it.
   */
  aimCut(t: number | null) {
    this.aim = t;
    this.rebuildAim();
  }

  get aimedAt(): number | null {
    return this.aim;
  }

  /** The two ends of a sheet edge in CSS pixels, `from` first — what turns
   *  a pointer position into a parameter. */
  edgeEndsOnScreen(name: number): [[number, number], [number, number]] | null {
    const edge = this.lit.find((e) => e.id === name);
    if (!edge) return null;
    const scene = (p: readonly number[]) =>
      new Vector3(
        p[0] - this.center[0],
        p[1] - this.center[1],
        p[2] - this.center[2],
      );
    return [this.toScreen(scene(edge.from)), this.toScreen(scene(edge.to))];
  }

  private rebuildAim() {
    // `true` for the material as well: a ribbon is rebuilt on every change
    // of selection, and the material does not go with the mesh by default.
    this.aimWire?.dispose(false, true);
    this.aimWire = undefined;
    if (this.aim === null || !this.lit.length) return;
    const t = this.aim;
    // Only along the edges that are drawn: a mark on an edge a filter has
    // taken off the screen is a mark floating in the air.
    const drawn = this.drawnEdgeNames();
    const along = drawn ? this.lit.filter((e) => drawn.has(e.id)) : this.lit;
    if (!along.length) return;
    const points = along.map((e) => [
      e.from[0] - this.center[0],
      e.from[1] - this.center[1],
      e.from[2] - this.center[2],
      e.from[0] + (e.to[0] - e.from[0]) * t - this.center[0],
      e.from[1] + (e.to[1] - e.from[1]) * t - this.center[1],
      e.from[2] + (e.to[2] - e.from[2]) * t - this.center[2],
    ]);
    this.aimWire = CreateGreasedLine(
      "aim",
      { points },
      { color: Color3.FromHexString(MATE), ...this.ribbon(AIM_SHARE) },
      this.scene,
    );
    this.aimWire.isPickable = false;
    this.aimWire.layerMask = MAIN_LAYER;
    // With the blocking, like the sheet it is drawn along: the mark runs
    // through the model with the sheet, and over everything it says a cut
    // lands at the back as loudly as at the front.
    this.aimWire.renderingGroupId = PASS.blocking;
    const aim = this.aimWire.material as GreasedLineSimpleMaterial;
    aim.zOffset = AIM_DEPTH_BIAS.factor;
    aim.zOffsetUnits = AIM_DEPTH_BIAS.units;
  }

  private rebuildHighlight() {
    this.litWire?.dispose(false, true);
    this.litWire = undefined;
    this.litDrawn = 0;
    this.rebuildAim();
    if (!this.view || !this.lit.length) return;

    const wanted = new Set(this.lit.map((e) => e.id));
    const flat = edgeSegments(this.view, this.prefs.edges.shrink);
    const lines: Vector3[][] = [];
    // Over the edges the wireframe draws, by the same reckoning: a sheet is
    // lit where the model it runs through is on screen, and nowhere else.
    for (const e of this.edgesShown()) {
      if (!wanted.has(this.view.edgeNames[e])) continue;
      const at = e * 6;
      lines.push([
        new Vector3(flat[at], flat[at + 1], flat[at + 2]),
        new Vector3(flat[at + 3], flat[at + 4], flat[at + 5]),
      ]);
    }
    if (!lines.length) return;
    this.litWire = CreateGreasedLine(
      "sheet",
      { points: lines.map(([a, b]) => [a.x, a.y, a.z, b.x, b.y, b.z]) },
      { color: Color3.FromHexString(ANCHOR), ...this.ribbon(1) },
      this.scene,
    );
    this.litWire.isPickable = false;
    this.litWire.layerMask = MAIN_LAYER;
    this.litWire.renderingGroupId = PASS.blocking;
    this.litDrawn = lines.length;
    const lit = this.litWire.material as GreasedLineSimpleMaterial;
    lit.zOffset = SHEET_DEPTH_BIAS.factor;
    lit.zOffsetUnits = SHEET_DEPTH_BIAS.units;
  }

  /**
   * How many pixels across a node's handle actually covers.
   *
   * Worked out rather than measured off the screen: the sizing pass puts
   * the handle at a world size that comes to the asked-for number of
   * pixels at its distance, and this undoes that arithmetic. What it is
   * for is saying that the setting reached the picture.
   */
  handleAcross(name: number): number | null {
    const mesh = this.nodes.find((m) => m.metadata.name === name);
    if (!mesh) return null;
    const camera = this.camera;
    const height = this.scene.getEngine().getRenderHeight();
    const distance = Vector3.Distance(mesh.position, camera.position);
    return (
      (mesh.scaling.x * height) / (2 * Math.tan(camera.fov / 2) * distance)
    );
  }

  /** How wide the wireframe's ribbon is drawn, in the render pixels its
   *  material speaks in. */
  get edgeRibbonWidth(): number {
    const material = this.wire?.material as
      GreasedLineSimpleMaterial | undefined;
    return material?.width ?? 0;
  }

  /** The midpoint of an edge, in **CSS pixels** — the inverse of `pickEdge`,
   *  and what lets a test aim a real mouse at a real line. */
  edgeScreen(name: number): [number, number] | null {
    if (!this.view) return null;
    const at = this.view.edgeNames.indexOf(name);
    if (at < 0) return null;
    const flat = edgeSegments(this.view, this.prefs.edges.shrink);
    const mid = new Vector3(
      (flat[at * 6] + flat[at * 6 + 3]) / 2,
      (flat[at * 6 + 1] + flat[at * 6 + 4]) / 2,
      (flat[at * 6 + 2] + flat[at * 6 + 5]) / 2,
    );
    return this.toScreen(mid);
  }

  /** The center of a drawn face, in **CSS pixels** — the inverse of
   *  `pickSurface`, and what lets a test aim a real mouse at a real face. */
  faceScreen(name: number): [number, number] | null {
    for (const group of ["faces", "blocks"] as Group[]) {
      for (const { data } of this.surfaces[group]) {
        const quad = data.faces.indexOf(name);
        if (quad < 0) continue;
        const mid = new Vector3(0, 0, 0);
        for (let k = 0; k < 4; k++) {
          const v = (quad * 4 + k) * 3;
          mid.addInPlaceFromFloats(
            data.positions[v] / 4,
            data.positions[v + 1] / 4,
            data.positions[v + 2] / 4,
          );
        }
        return this.toScreen(mid);
      }
    }
    return null;
  }

  /**
   * A point in the scene, in CSS pixels.
   *
   * The main camera's own matrices, not `scene.getTransformMatrix()`: with
   * two active cameras that holds whichever rendered last, which is the
   * corner gizmo.
   */
  private toScreen(at: Vector3, pane: Pane = this.hovered): [number, number] {
    const engine = this.scene.getEngine();
    const camera = this.panes[pane];
    const width = engine.getRenderWidth();
    const height = engine.getRenderHeight();
    const pixels = camera.viewport.toGlobal(width, height);
    // A Babylon viewport measures y from the bottom and `Project` wants it
    // from the top. A full-screen viewport hides the difference because
    // the offset is nought either way; a quarter of the window does not.
    const topDown = new Viewport(
      pixels.x,
      height - pixels.y - pixels.height,
      pixels.width,
      pixels.height,
    );
    const projected = Vector3.Project(
      at,
      Matrix.Identity(),
      camera.getViewMatrix().multiply(camera.getProjectionMatrix()),
      topDown,
    );
    const css = engine.getHardwareScalingLevel();
    return [projected.x * css, projected.y * css];
  }

  /** Model-space position of a node, as the scene currently shows it. */
  nodePosition(name: number): [number, number, number] | null {
    const mesh = this.nodes.find((m) => m.metadata.name === name);
    if (!mesh) return null;
    return [
      mesh.position.x + this.center[0],
      mesh.position.y + this.center[1],
      mesh.position.z + this.center[2],
    ];
  }

  /**
   * How near and how far the camera may orbit, from the size of the model.
   *
   * Set when the view changes rather than when it is framed. A station tab is
   * told the kernel moved and redraws **without** reframing — that is the
   * whole point of not yanking the camera about under someone — so limits
   * that only a reframe set would never be set at all for a model built from
   * Python.
   */
  private fitLimits() {
    const diagonal = this.modelBounds();
    // One node has no diagonal, and a limit of zero is worse than none.
    this.camera.lowerRadiusLimit = diagonal
      ? diagonal * CLOSEST_APPROACH
      : null;
    this.camera.upperRadiusLimit = diagonal ? diagonal * 60 : null;
  }

  /**
   * Everything on screen, blocking and geometry together.
   *
   * Both, because framing is "show me what there is" and a geometry with no
   * blocks on it yet is still what there is. The blocking's part comes from
   * the handles, which sit on every node; the geometry's from its own nodes
   * rather than from its meshes, which have no handles to read.
   */
  private bounds(): { lo: Vector3; hi: Vector3 } | null {
    let lo = new Vector3(Infinity, Infinity, Infinity);
    let hi = new Vector3(-Infinity, -Infinity, -Infinity);
    let any = false;
    for (const m of this.nodes) {
      lo = Vector3.Minimize(lo, m.position);
      hi = Vector3.Maximize(hi, m.position);
      any = true;
    }
    const box = this.geometry && geomBounds(this.geometry);
    if (box) {
      lo = Vector3.Minimize(lo, new Vector3(...box.lo));
      hi = Vector3.Maximize(hi, new Vector3(...box.hi));
      any = true;
    }
    return any ? { lo, hi } : null;
  }

  private modelBounds(): number {
    const box = this.bounds();
    return box ? box.hi.subtract(box.lo).length() : 0;
  }

  /**
   * Bring the whole model into view.
   *
   * **The direction is left alone.** Where the model is and how far away it
   * has to be come from its own bounds; which way you are looking at it from
   * is yours, and the triad is what changes that. Recentring that also swung
   * the view back to some default angle meant re-aiming after every use.
   *
   * It glides rather than jumps, unless asked otherwise: a camera that
   * teleports leaves you working out which way round the model ended up —
   * the same reason the triad swings.
   */
  frameAll(instant = false) {
    const box = this.bounds();
    if (!box) return;
    const target = box.lo.add(box.hi).scale(0.5);
    const diagonal = box.hi.subtract(box.lo).length();
    const radius = diagonal * 1.8 || 4;

    // The orthographic panes always jump. What a glide is for is keeping
    // track of which way round the model ended up, and a view locked to an
    // axis has no way round to lose.
    for (const pane of PANES) {
      if (!PANE_AXIS[pane]) continue;
      this.panes[pane].target = target.clone();
      this.half[pane] = orthoHalfFor(diagonal);
    }
    this.resize();

    // A gesture leaves inertia running, and it would go on nudging the camera
    // while the animation moves it, ending somewhere neither of them meant.
    this.camera.inertialAlphaOffset = 0;
    this.camera.inertialBetaOffset = 0;
    this.camera.inertialRadiusOffset = 0;
    // And a second recentring replaces the first rather than racing it.
    this.scene.stopAnimation(this.camera);

    if (instant) {
      this.camera.target = target;
      this.camera.radius = radius;
      return;
    }
    glide(this.camera, "target", this.camera.target.clone(), target);
    glide(this.camera, "radius", this.camera.radius, radius);
  }

  /** Whether the camera is still being animated somewhere. */
  get cameraMoving(): boolean {
    return this.scene.getAllAnimatablesByTarget(this.camera).length > 0;
  }

  /** The kernel's names for the edges on screen. */
  edgeNames(): number[] {
    return this.view ? Array.from(this.view.edgeNames) : [];
  }

  /** An edge's midpoint, in model coordinates. */
  edgeMidpoint(name: number): [number, number, number] | null {
    if (!this.view) return null;
    const at = this.view.edgeNames.indexOf(name);
    if (at < 0) return null;
    const [a, b] = [
      this.view.edges[at * 2] * 3,
      this.view.edges[at * 2 + 1] * 3,
    ];
    return [0, 1, 2].map(
      (k) =>
        (this.view!.positions[a + k] + this.view!.positions[b + k]) / 2 +
        this.center[k],
    ) as [number, number, number];
  }

  /** The kernel's names for the nodes on screen. */
  names(): number[] {
    return this.nodes.map((m) => m.metadata.name as number);
  }

  /** Diameter of the node handles, as the scene currently draws them. */
  get nodeSize(): number {
    return this.nodes[0]?.scaling.x ?? 0;
  }

  /**
   * Keep every handle the same size on screen.
   *
   * A loop per frame over the node meshes. Negligible at this scale; when
   * there are thousands of nodes this becomes a thin-instance buffer updated
   * once, not a loop.
   */
  /**
   * Keep every handle the same size on screen, **in the pane about to be
   * drawn**.
   *
   * Called once per camera rather than once per frame. One sphere is one
   * mesh shown in four views at four zooms, and sizing it for whichever
   * pane the pointer happened to be in left it the size of a block in a
   * pane zoomed in further — which is what it looked like.
   *
   * A loop per camera over the node meshes. Negligible at this scale; when
   * there are thousands of nodes this becomes a thin-instance buffer, one
   * per pane.
   */
  private rescaleHandles(camera: ArcRotateCamera) {
    const pane = PANES.find((p) => this.panes[p] === camera);
    if (!pane) return;
    const engine = this.scene.getEngine();
    // The pane's own height, not the canvas's: in a split window a pane is
    // half as tall, and a handle sized against the whole canvas would come
    // out half the size it was asked for.
    const height = engine.getRenderHeight() * camera.viewport.height;
    const ortho = PANE_AXIS[pane] ? this.half[pane] : null;
    for (const [meshes, pixels] of [
      [this.nodes, this.prefs.nodes.size],
      [this.geomMeshes.corners, CORNER_PIXELS],
      // Twice a handle: big enough to read as "this one is being placed",
      // and sized in pixels like everything else rather than in world
      // units, which on a twenty-unit model drew them the size of a block.
      [this.laying?.marks ?? [], 2 * this.prefs.nodes.size],
      // The halo's ball, for the same reason and by the same arithmetic:
      // it is a ring around a handle, so it has to be a ring around it in
      // every pane, at whatever each one is zoomed to.
      [this.haloBall?.isEnabled() ? [this.haloBall] : [], this.haloPixels],
    ] as const) {
      for (const mesh of meshes) {
        const wanted =
          ortho === null
            ? worldSizeForPixels(
                pixels,
                Vector3.Distance(mesh.position, camera.position),
                camera.fov,
                height,
              )
            : worldSizeForPixelsOrtho(pixels, ortho, height);
        mesh.scaling.setAll(wanted);
      }
    }
  }

  /**
   * How near a line the pointer has to be, in the pane it is in.
   *
   * This one really is per frame rather than per camera: it is about
   * where the mouse is, and the mouse is in one pane.
   */
  private fitPickTolerance() {
    const engine = this.scene.getEngine();
    const camera = this.panes[this.hovered];
    const height = engine.getRenderHeight() * camera.viewport.height;
    const ortho = PANE_AXIS[this.hovered] ? this.half[this.hovered] : null;
    // Half, because the threshold is a radius about the line. Taken at the
    // distance to what the camera is looking at: an exact answer would be
    // per edge, and this is a click tolerance.
    const reach =
      (ortho === null
        ? worldSizeForPixels(
            EDGE_PICK_PIXELS,
            Vector3.Distance(camera.target, camera.position),
            camera.fov,
            height,
          )
        : worldSizeForPixelsOrtho(EDGE_PICK_PIXELS, ortho, height)) / 2;
    if (this.wire) this.wire.intersectionThreshold = reach;
    // **The geometry's curves on the same terms.** They are what a
    // classification is made on, and they were left on Babylon's own
    // default: a tenth of a **world unit**, which on a model a hundred
    // units across is a target thinner than the line drawn for it. A curve
    // you can see and cannot hit is the whole reason a halo was asked for
    // here.
    for (const mesh of this.geomMeshes.curves) {
      (mesh as unknown as LinesMesh).intersectionThreshold = reach;
    }
  }

  get nodeCount() {
    return this.nodes.length;
  }

  /**
   * What one of the two surfaces currently holds.
   *
   * How many quadrangles, how many distinct colors, how opaque, and where the
   * corners ended up — which is everything there is to assert about drawing,
   * coloring and shrinking without asking a GPU to render anything.
   */
  surface(group: Group): {
    quads: number;
    alpha: number;
    colors: number;
    positions: Float32Array;
    faces: Uint32Array;
    blocks: Uint32Array;
  } | null {
    const slots = this.surfaces[group];
    if (!slots.length) return null;
    // The whole group, gathered back together: a test asks about the
    // surface, not about how many meshes it happens to be drawn in.
    const seen = new Set<string>();
    const positions: number[] = [];
    const faces: number[] = [];
    const blocks: number[] = [];
    let quads = 0;
    for (const { data } of slots) {
      for (let v = 0; v < data.colors.length; v += 4) {
        seen.add(data.colors.slice(v, v + 3).join(","));
      }
      quads += data.indices.length / 6;
      positions.push(...data.positions);
      faces.push(...data.faces);
      blocks.push(...data.blocks);
    }
    return {
      quads,
      alpha: (slots[0].mesh.material as StandardMaterial).alpha,
      colors: seen.size,
      positions: new Float32Array(positions),
      faces: new Uint32Array(faces),
      blocks: new Uint32Array(blocks),
    };
  }

  /** How many meshes a group is drawn in — one per block. */
  meshCount(group: Group): number {
    return this.surfaces[group].length;
  }

  get edgeCount() {
    return this.wire ? this.wireSegments : 0;
  }

  render() {
    this.scene.render();
  }

  dispose() {
    this.scene.dispose();
  }
}
