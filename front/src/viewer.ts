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
import type { GreasedLineMesh } from "@babylonjs/core/Meshes/GreasedLine/greasedLineMesh";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import { PointerDragBehavior } from "@babylonjs/core/Behaviors/Meshes/pointerDragBehavior";
import "@babylonjs/core/Culling/ray";
import type { GeometryView, MeshView, View } from "./decode";
import type { GeomGroup, Spot } from "./geometry";
import {
  DIM_OF,
  geomBounds,
  geomCorners,
  geomCurves,
  geomSurfaces,
  GEOM_GROUPS,
  SURFACE,
} from "./geometry";
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
import { DEFAULTS, NODE_SIZE, type GeomLayer, type Layer, type ViewPrefs } from "./theme";
import { ANCHOR, FAMILY, MATE, colourForId } from "./palette";
import type { SheetEdge } from "./kernel";
import {
  buildMesh,
  buildSurfaces,
  edgeSegments,
  meshEdges,
  type SurfaceData,
  type SurfaceOptions,
} from "./surface";

/** The two groups drawn from the same quadrangles. */
export type Group = "faces" | "blocks";

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
 * apart by size as well as by colour.
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
  centre: readonly [number, number, number],
  normal: readonly [number, number, number],
): number | null {
  const dot = (a: readonly number[], b: readonly number[]) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const gap = [0, 1, 2].map((k) => origin[k] - centre[k]);
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
 * The last two are over everything on purpose: a lit sheet runs *through*
 * the model and an extrusion preview is half inside the block it grows
 * from, and both are things being shown to you rather than things that are
 * there.
 */
export const PASS = { geometry: 0, blocking: 1, sheet: 2, aiming: 3 } as const;

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
function boxFaces(near: readonly Vector3[], far: readonly Vector3[]): Float32Array {
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
function boxOutline(near: readonly Vector3[], far: readonly Vector3[]): number[][] {
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
function glide(target: object, property: string, from: number | Vector3, to: number | Vector3) {
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
  for (const a of running?.getAnimations() ?? []) a.animation.setEasingFunction(ease);
}

function normalsFor(data: SurfaceData): number[] {
  const normals: number[] = [];
  VertexData.ComputeNormals(data.positions, Array.from(data.indices), normals);
  return normals;
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
  centre: [number, number, number] = [0, 0, 0];

  private nodes: Mesh[] = [];
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
  /** The edge that sheet was asked for, so a second click on it can put the
   *  sheet out again. View state, like the face selection. */
  selectedEdge: number | null = null;
  private litWire?: Mesh;
  /** One mesh per block, so that Babylon can sort them back to front. */
  private surfaces: Record<Group, { mesh: Mesh; data: SurfaceData }[]> = {
    faces: [],
    blocks: [],
  };
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
    centre: Vector3;
    normal: Vector3;
    corners: Vector3[];
    /** Whether the drag may go **behind** the face. True for a face of an
     *  open sheet, which has no inside; false for a face of a block,
     *  whose other side is the block. */
    bothWays: boolean;
  };
  private grownWire?: GreasedLineMesh;
  private grownBox?: Mesh;
  private grown: number | null = null;
  /** The geometry, if one has been loaded. Read-only here: nothing in the
   *  viewer edits it, and no operation moves it. */
  private geometry?: GeometryView;
  /** One mesh per geometric entity, so each can be coloured, faded and
   *  hidden as the one thing it is — and so the transparency sorter has
   *  something to sort. */
  private geomMeshes: Record<GeomGroup, Mesh[]> = { corners: [], curves: [], surfaces: [] };
  /** A working copy of the view: the drag previews against it, so the scene can
   *  follow the mouse without a round trip to the kernel. */
  private view?: View;
  private dropped: Drop[] = [];
  private prefs: ViewPrefs = structuredClone(DEFAULTS);
  /** What the user has picked, in order. The first is the anchor — a glue
   *  leaves the seam where it is — so the two are drawn differently. */
  private picked: Pick[] = [];
  readonly axes: AxisGizmo;
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
    const light = new HemisphericLight("hemi", new Vector3(0, 1, 0), this.scene);
    light.intensity = 0.9;
    // Not black underneath: surfaces are lit by this one light, and a face
    // turned away from it would otherwise be a silhouette with no colour left
    // to read — which defeats colouring by name.
    light.groundColor = new Color3(0.42, 0.42, 0.46);

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
    this.axes = new AxisGizmo(this.scene, this.camera);
    this.layOut();
    // Rendering leaves `activeCamera` set to whichever camera went last — the
    // corner gizmo. Everything that runs between frames then reads the wrong
    // one, and Babylon's own drag behaviour reads it twice: to decide whose
    // controls to suspend, and to build the ray the drag follows. So the
    // dragged node went the wrong way *and* the view orbited along with it.
    // Put it back to the main camera once the frame is done.
    this.scene.onAfterRenderObservable.add(() => {
      this.scene.activeCamera = this.panes[this.hovered];
    });
    // Babylon's own picking — which is what the drag behaviours run on — must
    // use the main camera, not whichever happens to be active. The gizmo does
    // its own pick, through its own camera.
    this.scene.cameraToUseForPointers = this.camera;
    // And it must only ever see the node handles.
    //
    // The surfaces are pickable, because a face is something you choose with
    // the mouse. But Babylon's pointer system returns the nearest pickable
    // mesh, so a node on the far side of the model became ungrabbable the
    // moment the faces were drawn — the face in front answered first, and
    // the drag behaviour, which checks that the pick is *its* mesh, never
    // started. Half the handles of every cube, silently.
    //
    // Faces are picked deliberately instead, by `pickSurface`, on a click that
    // did not land on a handle.
    // The blocking's pass keeps the depth the geometry's left behind, so
    // the two are ordered without either being declared always in front.
    // The two passes above it do clear, which is what puts them over
    // everything.
    this.scene.setRenderingAutoClearDepthStencil(PASS.blocking, false, false, false);
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
    const camera = new ArcRotateCamera(`cam-${pane}`, alpha, beta, 100, Vector3.Zero(), this.scene);
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
    return camera;
  }

  /** Split the window in four, or put it back. */
  setQuad(quad: boolean) {
    if (this.quad === quad) return;
    this.quad = quad;
    if (!quad) this.hovered = "3d";
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
      for (const behaviour of mesh.behaviors) {
        if (behaviour.name !== "PointerDrag") continue;
        const normal = (behaviour as PointerDragBehavior).options.dragPlaneNormal;
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
    // Babylon's own pointer system — which is what the node drag behaviours
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
    for (const behaviour of mesh.behaviors) {
      if (behaviour.name !== "PointerDrag") continue;
      (behaviour as PointerDragBehavior).options = normal ? { dragPlaneNormal: normal } : {};
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
   * it: the kernel says what exists, not what colour it is here.
   *
   * One entry point rather than one per control. Colour, transparency and
   * shrink are not independent of each other at the mesh level — a shrink
   * moves vertices, a colouring rewrites the colour buffer — and splitting
   * them into separate methods only invites two of them to disagree.
   */
  applyPrefs(prefs: ViewPrefs) {
    this.prefs = structuredClone(prefs);
    const theme = this.prefs.theme;
    this.scene.clearColor = Color3.FromHexString(theme.background).toColor4(1);
    this.paintNodes();
    // The wireframe's colours and width both come out of the rebuild: a
    // ribbon carries its colours in its material, not in a `color` on the
    // mesh.
    this.rebuildEdges();
    this.rebuildHighlight();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");
    this.rebuildGeometry();
    this.rebuildMesh();
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
      colouring: layer.colouring,
      uniform: group === "faces" ? this.prefs.theme.faces : this.prefs.theme.blocks,
      shrink: layer.shrink,
      // A block keeps all six of its own faces whatever this says: shrink an
      // open shell and you are looking into a box with a side missing.
      internal: group === "faces" ? this.prefs.faces.internal : true,
      highlight: new Map(this.picked.map((p, n) => [p.face, n === 0 ? ANCHOR : MATE])),
      classified: this.classifiedFaces(),
    };
  }

  /** The colour each classified face wears, by face name. */
  private classifiedFaces(): Map<number, string> {
    const out = new Map<number, string>();
    for (let q = 0; q < (this.view?.quadFaces.length ?? 0); q++) {
      const dim = this.view!.quadClassDim[q];
      if (dim < 0) continue;
      out.set(
        this.view!.quadFaces[q],
        this.classColour(dim, 0, this.prefs.theme.faces).toHexString(),
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
      slots.every((slot, i) => slot.data.positions.length === wanted[i].data.positions.length);

    if (sameShape) {
      slots.forEach((slot, i) => {
        const data = wanted[i].data;
        slot.data = data;
        slot.mesh.updateVerticesData(VertexBuffer.PositionKind, data.positions);
        slot.mesh.updateVerticesData(VertexBuffer.ColorKind, data.colors);
        slot.mesh.updateVerticesData(VertexBuffer.NormalKind, normalsFor(data));
        (slot.mesh.material as StandardMaterial).alpha = layer.alpha;
      });
      return;
    }

    for (const slot of slots) slot.mesh.dispose();
    this.surfaces[group] = wanted.map(({ block, data }) => ({
      mesh: this.surfaceMesh(group, block, data, layer),
      data,
    }));
  }

  private surfaceMesh(group: Group, block: number, data: SurfaceData, layer: Layer): Mesh {
    const mesh = new Mesh(`${group}-${block}`, this.scene);
    const vertices = new VertexData();
    vertices.positions = data.positions;
    vertices.indices = data.indices;
    vertices.colors = data.colors;
    vertices.normals = normalsFor(data);
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
    const corners = [0, 1, 2, 3].map((k) => this.point(this.view!.quads[quad * 4 + k]));
    const normal = quadNormal(corners);
    if (normal.length() < 1e-12) return false;
    const centre = corners
      .reduce((at, p) => at.addInPlace(p), new Vector3(0, 0, 0))
      .scale(0.25);
    this.growing = { pick, centre, normal: normal.normalize(), corners, bothWays };
    this.grown = null;
    this.showGrown();
    return true;
  }

  /** How far the pointer is asking for, and the preview to match. */
  aimExtrusion(x: number, y: number): number | null {
    if (!this.growing) return null;
    const { centre, normal, corners } = this.growing;
    const ray = this.scene.createPickingRay(x, y, Matrix.Identity(), this.cameraFor(x, y));
    const towards = ray.direction.normalizeToNew();
    const asked = heightAlong(
      [ray.origin.x, ray.origin.y, ray.origin.z],
      [towards.x, towards.y, towards.z],
      [centre.x, centre.y, centre.z],
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
      this.grownBox.updateVerticesData(VertexBuffer.NormalKind, boxNormals(positions));
      this.grownWire.setPoints(outline);
      return;
    }

    const mesh = new Mesh("grown", this.scene);
    const vertices = new VertexData();
    vertices.positions = positions;
    vertices.indices = Array.from(BOX_TRIANGLES);
    vertices.normals = boxNormals(positions);
    vertices.applyToMesh(mesh, true);

    const colour = Color3.FromHexString(MATE);
    const material = new StandardMaterial("grownMat", this.scene);
    material.diffuseColor = colour;
    material.emissiveColor = colour.scale(0.45);
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
      { color: colour, ...this.ribbon(1) },
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
      if (this.view.quadFaces[q] === pick.face && this.view.quadBlocks[q] === pick.block) {
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
    const handles = (m: { metadata?: { name?: number } }) =>
      picking === "blocks" && (m.metadata?.name ?? -1) >= 0;
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
    for (const group of ["corners", "curves", "surfaces"] as const) {
      const mine = new Set(this.geomMeshes[group].map((m) => m.name));
      if (!mine.size) continue;
      const hit = this.scene.pick(x, y, (m) => mine.has(m.name), false, this.cameraFor(x, y));
      if (!hit?.hit || !hit.pickedMesh) continue;
      const tag = Number(hit.pickedMesh.name.split("-").pop());
      if (Number.isFinite(tag)) return { dim: DIM_OF[group], tag };
    }
    return null;
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
      view: this.centre,
      geometry: this.geometry?.centre ?? null,
      mesh: this.meshView?.centre ?? null,
    };
  }

  /** What the geometry's three groups currently hold, for a test to read. */
  geomCount(group: GeomGroup): number {
    return this.geomMeshes[group].length;
  }

  /** The entity tags on screen in one group, in the order they are drawn. */
  geomTags(group: GeomGroup): number[] {
    return this.geomMeshes[group].map((m) => Number(m.name.split("-").pop()));
  }

  /** The colour and opacity one geometric entity is drawn in — everything
   *  there is to assert about a group's display without rendering. */
  geomLook(group: GeomGroup, tag: number): { colour: string; alpha: number } | null {
    const mesh = this.geomMeshes[group].find((m) => m.name === `geom-${group}-${tag}`);
    if (!mesh) return null;
    const material = mesh.material as StandardMaterial | null;
    // Where each group keeps the colour it was given: a line mesh has no
    // material of its own, a corner is unlit and so carries it as its own
    // light, and a surface is lit and carries it as what the light falls on.
    const colour =
      group === "curves"
        ? (mesh as LinesMesh).color
        : group === "corners"
          ? (material?.emissiveColor ?? Color3.Black())
          : (material?.diffuseColor ?? Color3.Black());
    return { colour: colour.toHexString().toLowerCase(), alpha: mesh.visibility };
  }

  private geomColour(group: GeomGroup, tag: number): Color3 {
    // Picked surfaces take the selection colour, whatever the colouring
    // says — the same convention as a picked face, so there is one thing
    // to learn rather than two.
    if (this.pickedGeom.some((s) => s.dim === DIM_OF[group] && s.tag === tag)) {
      return Color3.FromHexString(ANCHOR);
    }
    return this.entityColour(group, tag);
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
      colouring: layer.colouring,
    });
    const skin = new Mesh("mesh-skin", this.scene);
    const vertices = new VertexData();
    vertices.positions = data.positions;
    vertices.indices = data.indices;
    vertices.colors = data.colors;
    vertices.normals = normalsFor(data);
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

  /** What a geometric entity is drawn in: one colour for its group, or one
   *  per entity, as that group's own setting says. */
  private entityColour(group: GeomGroup, tag: number): Color3 {
    const layer: GeomLayer = this.prefs[group];
    return Color3.FromHexString(
      layer.colouring === "id" ? colourForId(tag, FAMILY[group]) : this.prefs.theme[group],
    );
  }

  /**
   * The colour of a cell of the **blocking**, by the *kind* of thing it is
   * classified on: a corner, a curve, a surface, or nothing.
   *
   * Three colours and a legend, not a hue per entity. What you read off a
   * `class` colouring is whether the blocking has taken hold of the
   * geometry, which is a question with four answers — and painting a
   * block face the colour of the very surface it is classified on, on top of that
   * surface, made two things nobody could tell apart.
   */
  private classColour(dim: number, tag: number, free: string): Color3 {
    void tag;
    const named = (["onCorner", "onCurve", "onSurface"] as const)[dim];
    return Color3.FromHexString(named ? this.prefs.theme[named] : free);
  }

  /**
   * The colour of one node handle: picked, then whatever the colouring
   * says.
   *
   * Being frozen is **not** in this list. It is drawn as a cube instead,
   * so that a frozen node can still be read for its id or for what it
   * classified on — the two things the colouring is there to show.
   */
  private nodeColour(index: number): Color3 {
    const plain = this.prefs.theme.nodes;
    if (!this.view) return Color3.FromHexString(plain);
    if (this.pickedNodes.includes(this.view.names[index])) return Color3.FromHexString(ANCHOR);
    switch (this.prefs.nodes.colouring) {
      case "class":
        return this.classColour(this.view.nodeClassDim[index], 0, plain);
      case "id":
        return Color3.FromHexString(colourForId(this.view.names[index], FAMILY.nodes));
      default:
        return Color3.FromHexString(plain);
    }
  }

  /**
   * Rebuild all three groups.
   *
   * Thrown away and remade rather than patched, unlike the blocking's
   * surfaces. There is no slider dragging a stream of these: a geometry has
   * no shrink, so the only things that change are a colour, an opacity and
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
      VertexData.ComputeNormals(surface.positions, Array.from(surface.indices), normals);
      vertices.normals = normals;
      vertices.applyToMesh(mesh, false);

      const colour = this.geomColour("surfaces", surface.tag);
      const material = new StandardMaterial(`geomSurfaceMat${surface.tag}`, this.scene);
      material.diffuseColor = colour;
      // A little of its own light, so a patch turned away from the lamp is
      // still the colour it was given rather than a silhouette.
      material.emissiveColor = colour.scale(0.35);
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
          new Vector3(curve.segments[at], curve.segments[at + 1], curve.segments[at + 2]),
          new Vector3(curve.segments[at + 3], curve.segments[at + 4], curve.segments[at + 5]),
        ]);
      }
      const mesh = CreateLineSystem(`geom-curves-${curve.tag}`, { lines }, this.scene);
      mesh.color = this.geomColour("curves", curve.tag);
      this.dressGeometry(mesh, "curves");
    }
  }

  private buildGeomCorners() {
    for (const corner of geomCorners(this.geometry!)) {
      // Unit diameter; `rescaleHandles` sizes it in pixels every frame, the
      // same way the node handles are sized.
      const mesh = CreateSphere(`geom-corners-${corner.tag}`, { diameter: 1, segments: 8 }, this.scene);
      mesh.position = new Vector3(...corner.at);
      const material = new StandardMaterial(`geomCornerMat${corner.tag}`, this.scene);
      material.emissiveColor = this.geomColour("corners", corner.tag);
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
    const lines = this.segments().map(([a, b]) => [a.x, a.y, a.z, b.x, b.y, b.z]);
    const colours = this.edgeColours().map(([c]) => new Color3(c.r, c.g, c.b));
    // Reused while the shape of the thing has not changed: a drag rebuilds
    // this on every frame, and rewriting the points of a ribbon that is
    // already there costs a fraction of building one.
    if (this.wire && this.wireSegments === lines.length) {
      this.wire.setPoints(lines);
      this.dressEdges(colours);
      return;
    }
    this.wire?.dispose(false, true);
    this.wire = CreateGreasedLine(
      "edges",
      { points: lines, updatable: true },
      {
        colors: colours,
        useColors: true,
        // One colour per segment: an edge wears what it is classified on,
        // and each segment of this ribbon **is** an edge.
        colorDistributionType: GreasedLineMeshColorDistributionType.COLOR_DISTRIBUTION_TYPE_SEGMENT,
        // The unlit material rather than the lit one the builder reaches
        // for by default: a wireframe takes no light, and this is the one
        // that can be handed new colours without being rebuilt.
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
   * Write the colours **and the width** onto a ribbon that is being kept.
   *
   * Both, because both are settings and a kept ribbon hears about a
   * setting only here: writing one and not the other is how a width that
   * was asked for went nowhere until the model's shape happened to change.
   */
  private dressEdges(colours: Color3[]) {
    const material = this.wire?.material as GreasedLineSimpleMaterial | undefined;
    if (!material) return;
    material.setColors(colours, false, true);
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
      resolution: new Vector2(engine.getRenderWidth(), engine.getRenderHeight()),
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
    ] as const) {
      const material = (mesh as GreasedLineMesh | undefined)?.greasedLineMaterial;
      if (!material) continue;
      const { width, resolution } = this.ribbon(share);
      material.width = width;
      material.resolution = resolution;
    }
  }

  attachTo(canvas: HTMLCanvasElement) {
    this.held = canvas;
    this.attachHovered();
    canvas.addEventListener("pointermove", (e) => this.aimAt(e.offsetX, e.offsetY));
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
    this.centre = view.centre;
    // A copy, because the drag writes into it. The incoming arrays are views
    // onto the kernel's bytes.
    this.view = {
      centre: view.centre,
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
      // colour free to say whatever the colouring is set to — a frozen
      // node still shows its id or its classification, which it could not
      // when being frozen was a colour of its own.
      //
      // Unit size; the per-frame pass below scales it to a fixed number
      // of pixels.
      const s =
        view.nodeFrozen[n] === 1
          ? CreateBox(`node${n}`, { size: FROZEN_SIDE }, this.scene)
          : CreateSphere(`node${n}`, { diameter: 1, segments: 8 }, this.scene);
      s.position = this.point(n);
      // The kernel's name, not this index: the index is only good until the
      // next edit reorders the view.
      s.metadata = { index: n, name: view.names[n] };
      const m = new StandardMaterial(`nodeMat${n}`, this.scene);
      m.emissiveColor = this.nodeColour(n);
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
   * A behaviour per node rather than one attached on selection: attaching on
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
              .filter((m) => m !== sphere && picked.includes(m.metadata.name as number))
              .map((mesh) => ({
                mesh,
                index: mesh.metadata.index as number,
                from: mesh.position.clone(),
              }))
          : [];
    });
    drag.onDragObservable.add(() => {
      const by = sphere.position.subtract(from);
      this.preview(index, sphere.position);
      for (const other of carried) {
        other.mesh.position.copyFrom(other.from.add(by));
        this.preview(other.index, other.mesh.position);
      }
    });
    drag.onDragEndObservable.add(() => {
      const p = sphere.position;
      // A press and release on the spot is a click, not a move. Without
      // this it is a command all the same — and now a command that would
      // pin the node to whatever it happens to be near.
      if (Vector3.Distance(from, p) < 1e-9) return;
      const name = sphere.metadata.name as number;
      const to: [number, number, number] = [
        p.x + this.centre[0],
        p.y + this.centre[1],
        p.z + this.centre[2],
      ];
      const also = carried.map((o) => o.mesh.metadata.name as number);
      carried = [];
      this.dropped.forEach((listener) => listener(name, to, also));
    });
  }

  /** Move a node in the working copy and redraw the edges that reach it. */
  private preview(index: number, to: Vector3) {
    if (!this.view) return;
    this.view.positions[index * 3] = to.x;
    this.view.positions[index * 3 + 1] = to.y;
    this.view.positions[index * 3 + 2] = to.z;
    // A drag moves vertices and changes nothing else, so this is the same
    // in-place path a slider takes.
    this.rebuildEdges();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");
  }

  private point(i: number): Vector3 {
    const p = this.view!.positions;
    return new Vector3(p[i * 3], p[i * 3 + 1], p[i * 3 + 2]);
  }

  /**
   * A colour per end of every edge.
   *
   * Classified, an edge wears the colour of the curve or surface it
   * classified on; unclassified, the wireframe's own. Both ends the same: an
   * edge is on one entity or none, and a gradient would suggest otherwise.
   */
  private edgeColours(): Color4[][] {
    const plain = this.prefs.theme.edges;
    const how = this.prefs.edges.colouring;
    const out: Color4[][] = [];
    for (let e = 0; e < (this.view?.edgeNames.length ?? 0); e++) {
      const colour =
        how === "class"
          ? this.classColour(this.view!.edgeClassDim[e], 0, plain)
          : how === "id"
            ? Color3.FromHexString(colourForId(this.view!.edgeNames[e], FAMILY.edges))
            : Color3.FromHexString(plain);
      out.push([colour.toColor4(1), colour.toColor4(1)]);
    }
    return out;
  }

  private segments(): Vector3[][] {
    const flat = edgeSegments(this.view!, this.prefs.edges.shrink);
    const out: Vector3[][] = [];
    for (let s = 0; s < flat.length; s += 6) {
      out.push([
        new Vector3(flat[s], flat[s + 1], flat[s + 2]),
        new Vector3(flat[s + 3], flat[s + 4], flat[s + 5]),
      ]);
    }
    return out;
  }

  /** Give every handle the colour its node has earned. */
  private paintNodes() {
    this.nodes.forEach((mesh, n) => {
      (mesh.material as StandardMaterial).emissiveColor = this.nodeColour(n);
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
  nodeState(name: number): { on: { dim: number; tag: number } | null; frozen: boolean } | null {
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

  /** The colour a node's handle is wearing, as `#rrggbb`. */
  nodeTint(name: number): string | null {
    const mesh = this.nodes.find((m) => m.metadata.name === name);
    const material = mesh?.material as StandardMaterial | undefined;
    return material ? material.emissiveColor.toHexString().toLowerCase() : null;
  }

  /** Screen coordinates -> the kernel's name for the node under them. */
  pickNode(x: number, y: number): number | null {
    // Explicitly the main camera: after a frame, `scene.activeCamera` is
    // whichever rendered last, which is the corner gizmo.
    const hit = this.scene.pick(
      x,
      y,
      (m) => (m.metadata?.name ?? -1) >= 0,
      false,
      this.cameraFor(x, y),
    );
    return hit?.hit && hit.pickedMesh ? (hit.pickedMesh.metadata.name as number) : null;
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
    const hit = this.scene.pick(x, y, (m) => m === this.wire, false, this.cameraFor(x, y));
    // A ribbon answers with **where** it was hit and not with which of its
    // triangles: its `intersects` is its own, written for lines, and it
    // fills in the point rather than a face. So the segment is the one
    // that point is nearest — which is the same question asked in the
    // same space, and exact to the width of the ribbon.
    if (!hit?.hit || !hit.pickedPoint) return null;
    return this.edgeNearest(hit.pickedPoint);
  }

  /** The name of the edge whose drawn segment passes nearest a point. */
  private edgeNearest(point: Vector3): number | null {
    if (!this.view) return null;
    const flat = edgeSegments(this.view, this.prefs.edges.shrink);
    let best: number | null = null;
    let nearest = Infinity;
    for (let e = 0; e < this.view.edgeNames.length; e++) {
      const at = e * 6;
      const from = new Vector3(flat[at], flat[at + 1], flat[at + 2]);
      const to = new Vector3(flat[at + 3], flat[at + 4], flat[at + 5]);
      const along = to.subtract(from);
      const length = along.lengthSquared();
      // A degenerate edge is a point, and the distance to it is the
      // distance to that point.
      const t = length === 0 ? 0 : Math.min(1, Math.max(0, Vector3.Dot(point.subtract(from), along) / length));
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
   * A second line mesh rather than per-vertex colours on the first: the
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
   * the join between the two colours *is* the plane. One edge would not be
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
      new Vector3(p[0] - this.centre[0], p[1] - this.centre[1], p[2] - this.centre[2]);
    return [this.toScreen(scene(edge.from)), this.toScreen(scene(edge.to))];
  }

  private rebuildAim() {
    // `true` for the material as well: a ribbon is rebuilt on every change
    // of selection, and the material does not go with the mesh by default.
    this.aimWire?.dispose(false, true);
    this.aimWire = undefined;
    if (this.aim === null || !this.lit.length) return;
    const t = this.aim;
    const points = this.lit.map((e) => [
      e.from[0] - this.centre[0],
      e.from[1] - this.centre[1],
      e.from[2] - this.centre[2],
      e.from[0] + (e.to[0] - e.from[0]) * t - this.centre[0],
      e.from[1] + (e.to[1] - e.from[1]) * t - this.centre[1],
      e.from[2] + (e.to[2] - e.from[2]) * t - this.centre[2],
    ]);
    this.aimWire = CreateGreasedLine(
      "aim",
      { points },
      { color: Color3.FromHexString(MATE), ...this.ribbon(AIM_SHARE) },
      this.scene,
    );
    this.aimWire.isPickable = false;
    this.aimWire.layerMask = MAIN_LAYER;
    this.aimWire.renderingGroupId = PASS.aiming;
  }

  private rebuildHighlight() {
    this.litWire?.dispose(false, true);
    this.litWire = undefined;
    this.rebuildAim();
    if (!this.view || !this.lit.length) return;

    const wanted = new Set(this.lit.map((e) => e.id));
    const flat = edgeSegments(this.view, this.prefs.edges.shrink);
    const lines: Vector3[][] = [];
    for (let e = 0; e < this.view.edgeNames.length; e++) {
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
    this.litWire.renderingGroupId = PASS.sheet;
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
    return (mesh.scaling.x * height) / (2 * Math.tan(camera.fov / 2) * distance);
  }

  /** How wide the wireframe's ribbon is drawn, in the render pixels its
   *  material speaks in. */
  get edgeRibbonWidth(): number {
    const material = this.wire?.material as GreasedLineSimpleMaterial | undefined;
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

  /** The centre of a drawn face, in **CSS pixels** — the inverse of
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
      mesh.position.x + this.centre[0],
      mesh.position.y + this.centre[1],
      mesh.position.z + this.centre[2],
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
    this.camera.lowerRadiusLimit = diagonal ? diagonal * CLOSEST_APPROACH : null;
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
    const [a, b] = [this.view.edges[at * 2] * 3, this.view.edges[at * 2 + 1] * 3];
    return [0, 1, 2].map(
      (k) => (this.view!.positions[a + k] + this.view!.positions[b + k]) / 2 + this.centre[k],
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
    if (!this.wire) return;
    const engine = this.scene.getEngine();
    const camera = this.panes[this.hovered];
    const height = engine.getRenderHeight() * camera.viewport.height;
    const ortho = PANE_AXIS[this.hovered] ? this.half[this.hovered] : null;
    // Half, because the threshold is a radius about the line. Taken at the
    // distance to what the camera is looking at: an exact answer would be
    // per edge, and this is a click tolerance.
    this.wire.intersectionThreshold =
      (ortho === null
        ? worldSizeForPixels(
            EDGE_PICK_PIXELS,
            Vector3.Distance(camera.target, camera.position),
            camera.fov,
            height,
          )
        : worldSizeForPixelsOrtho(EDGE_PICK_PIXELS, ortho, height)) / 2;
  }

  get nodeCount() {
    return this.nodes.length;
  }

  /**
   * What one of the two surfaces currently holds.
   *
   * How many quadrangles, how many distinct colours, how opaque, and where the
   * corners ended up — which is everything there is to assert about drawing,
   * colouring and shrinking without asking a GPU to render anything.
   */
  surface(group: Group): {
    quads: number;
    alpha: number;
    colours: number;
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
      colours: seen.size,
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
