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
import { CreateSphere } from "@babylonjs/core/Meshes/Builders/sphereBuilder";
import { CreateLineSystem } from "@babylonjs/core/Meshes/Builders/linesBuilder";
import { CreateGreasedLine } from "@babylonjs/core/Meshes/Builders/greasedLineBuilder";
import type { GreasedLineMesh } from "@babylonjs/core/Meshes/GreasedLine/greasedLineMesh";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import { PointerDragBehavior } from "@babylonjs/core/Behaviors/Meshes/pointerDragBehavior";
import "@babylonjs/core/Culling/ray";
import type { GeometryView, View } from "./decode";
import type { GeomGroup } from "./geometry";
import { geomBounds, geomCorners, geomCurves, geomSurfaces, GEOM_GROUPS } from "./geometry";
import { AxisGizmo, MAIN_LAYER } from "./axes";
import { DEFAULTS, type GeomLayer, type Layer, type ViewPrefs } from "./theme";
import { ANCHOR, FAMILY, MATE, colourForId } from "./palette";
import type { SheetEdge } from "./kernel";
import {
  buildSurfaces,
  edgeSegments,
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
export type Drop = (name: number, to: [number, number, number]) => void;

/** How many pixels across a node handle should be, whatever the zoom. */
export const HANDLE_PIXELS = 11;

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
  private wire?: LinesMesh;
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
  /** The geometric surfaces picked, by gmsh tag, in the order they were
   *  picked. View state, like the face selection. */
  private pickedSurfaces: number[] = [];
  /** The face being grown, while the pointer is down on it: where it is,
   *  which way is out, and how far the pointer has asked for so far. */
  private growing?: { pick: Pick; centre: Vector3; normal: Vector3; corners: Vector3[] };
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

    this.scene.onBeforeRenderObservable.add(() => this.rescaleHandles());
    this.axes = new AxisGizmo(this.scene, this.camera);
    this.scene.activeCameras = [this.camera, this.axes.camera];
    // Rendering leaves `activeCamera` set to whichever camera went last — the
    // corner gizmo. Everything that runs between frames then reads the wrong
    // one, and Babylon's own drag behaviour reads it twice: to decide whose
    // controls to suspend, and to build the ray the drag follows. So the
    // dragged node went the wrong way *and* the view orbited along with it.
    // Put it back to the main camera once the frame is done.
    this.scene.onAfterRenderObservable.add(() => {
      this.scene.activeCamera = this.camera;
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
    this.setPicking("blocks");
    this.applyPrefs(this.prefs);
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
    const nodes = Color3.FromHexString(theme.nodes);
    for (const mesh of this.nodes) {
      (mesh.material as StandardMaterial).emissiveColor = nodes;
    }
    if (this.wire) this.wire.color = Color3.FromHexString(theme.edges);
    this.rebuildEdges();
    this.rebuildHighlight();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");
    this.rebuildGeometry();
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
    };
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
      this.camera,
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
  beginExtrusion(pick: Pick): boolean {
    const quad = this.quadOf(pick);
    if (quad === null || !this.view) return false;
    const corners = [0, 1, 2, 3].map((k) => this.point(this.view!.quads[quad * 4 + k]));
    const normal = quadNormal(corners);
    if (normal.length() < 1e-12) return false;
    const centre = corners
      .reduce((at, p) => at.addInPlace(p), new Vector3(0, 0, 0))
      .scale(0.25);
    this.growing = { pick, centre, normal: normal.normalize(), corners };
    this.grown = null;
    this.showGrown();
    return true;
  }

  /** How far the pointer is asking for, and the preview to match. */
  aimExtrusion(x: number, y: number): number | null {
    if (!this.growing) return null;
    const { centre, normal, corners } = this.growing;
    const ray = this.scene.createPickingRay(x, y, Matrix.Identity(), this.camera);
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
    const across = Vector3.Distance(corners[0], corners[2]);
    this.grown = Math.max(asked, across * THINNEST);
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
    mesh.renderingGroupId = 1;
    this.grownBox = mesh;

    this.grownWire = CreateGreasedLine(
      "grown-edges",
      { points: outline, updatable: true },
      { color: colour, ...this.ribbon(1) },
      this.scene,
    ) as GreasedLineMesh;
    this.grownWire.isPickable = false;
    this.grownWire.layerMask = MAIN_LAYER;
    this.grownWire.renderingGroupId = 2;
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

  /** The geometric surfaces picked, by tag. */
  get surfaceSelection(): number[] {
    return [...this.pickedSurfaces];
  }

  selectSurfaces(tags: readonly number[]) {
    this.pickedSurfaces = [...tags];
    this.rebuildGeometry();
  }

  /**
   * Screen coordinates -> the geometric surface under them.
   *
   * Through a predicate of its own rather than by making the geometry
   * pickable: Babylon consults `isPickable` only when no predicate is
   * given, so the geometry can stay invisible to the pointer system —
   * which is what keeps it from stealing a click meant for a block — while
   * still being reachable deliberately, from here.
   */
  pickGeometry(x: number, y: number): number | null {
    const mine = new Set(this.geomMeshes.surfaces.map((m) => m.name));
    const hit = this.scene.pick(x, y, (m) => mine.has(m.name), false, this.camera);
    if (!hit?.hit || !hit.pickedMesh) return null;
    const tag = Number(hit.pickedMesh.name.split("-").pop());
    return Number.isFinite(tag) ? tag : null;
  }

  /**
   * Show a geometry, replacing whatever was there.
   *
   * Asked for only when the kernel's generation number says it has changed,
   * which is why this does the whole thing rather than patching: a new
   * geometry shares nothing with the old one, not even its node numbering.
   */
  setGeometry(view: GeometryView) {
    this.geometry = view;
    this.pickedSurfaces = [];
    this.rebuildGeometry();
    // The geometry counts towards the model's extent, so how close the
    // camera may come has to be worked out again.
    this.fitLimits();
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
    if (group === "surfaces" && this.pickedSurfaces.includes(tag)) {
      return Color3.FromHexString(ANCHOR);
    }
    const layer: GeomLayer = this.prefs[group];
    return Color3.FromHexString(
      layer.colouring === "id" ? colourForId(tag, FAMILY[group]) : this.prefs.theme[group],
    );
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
    this.geomMeshes[group].push(mesh as Mesh);
  }

  private rebuildEdges() {
    if (!this.view?.edges.length || !this.prefs.edges.visible) {
      this.wire?.dispose();
      this.wire = undefined;
      return;
    }
    const lines = this.segments();
    const reusable =
      this.wire && (this.wire.getIndices()?.length ?? 0) === lines.length * 2
        ? this.wire
        : undefined;
    if (this.wire && !reusable) this.wire.dispose();
    this.wire = CreateLineSystem(
      "edges",
      { lines, updatable: true, instance: reusable },
      this.scene,
    );
    this.wire.color = Color3.FromHexString(this.prefs.theme.edges);
    // Pickable, because an edge is what a sheet is named by. The handles
    // still win: the scene's pointer predicates only ever see those, and
    // this is reached by `pickEdge` on a click that missed one.
    this.wire.isPickable = true;
    this.wire.layerMask = MAIN_LAYER;
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

  /** Keep the corner viewport square when the canvas changes shape. */
  resize() {
    this.axes.resize();
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
    this.camera.attachControl(canvas, true);
  }

  /** Take the pointer away from the camera, and give it back.
   *
   *  Aiming a cut is a drag along an edge, and a drag is also how the view
   *  is turned. One of them has to stand aside. */
  suspendCamera() {
    this.camera.detachControl();
  }

  resumeCamera() {
    if (this.held) this.camera.attachControl(this.held, true);
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
    };
    this.nodes.forEach((m) => m.dispose());
    this.nodes = [];
    this.rebuildEdges();
    this.rebuildHighlight();
    this.rebuildSurface("faces");
    this.rebuildSurface("blocks");

    for (let n = 0; n < view.positions.length / 3; n++) {
      // Unit diameter; the per-frame pass below scales it to a fixed number
      // of pixels.
      const s = CreateSphere(`node${n}`, { diameter: 1, segments: 8 }, this.scene);
      s.position = this.point(n);
      // The kernel's name, not this index: the index is only good until the
      // next edit reorders the view.
      s.metadata = { index: n, name: view.names[n] };
      const m = new StandardMaterial(`nodeMat${n}`, this.scene);
      m.emissiveColor = Color3.FromHexString(this.prefs.theme.nodes);
      m.disableLighting = true;
      s.material = m;
      s.layerMask = MAIN_LAYER;
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
    sphere.addBehavior(drag);
    drag.onDragObservable.add(() => this.preview(index, sphere.position));
    drag.onDragEndObservable.add(() => {
      const p = sphere.position;
      const name = sphere.metadata.name as number;
      const to: [number, number, number] = [
        p.x + this.centre[0],
        p.y + this.centre[1],
        p.z + this.centre[2],
      ];
      this.dropped.forEach((listener) => listener(name, to));
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

  /** Screen coordinates -> the kernel's name for the node under them. */
  pickNode(x: number, y: number): number | null {
    // Explicitly the main camera: after a frame, `scene.activeCamera` is
    // whichever rendered last, which is the corner gizmo.
    const hit = this.scene.pick(x, y, (m) => (m.metadata?.name ?? -1) >= 0, false, this.camera);
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
    const hit = this.scene.pick(x, y, (m) => m === this.wire, false, this.camera);
    if (!hit?.hit || hit.faceId < 0) return null;
    return hit.faceId < this.view.edgeNames.length ? this.view.edgeNames[hit.faceId] : null;
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
    this.aimWire.renderingGroupId = 2;
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
    this.litWire.renderingGroupId = 1;
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
  private toScreen(at: Vector3): [number, number] {
    const engine = this.scene.getEngine();
    const projected = Vector3.Project(
      at,
      Matrix.Identity(),
      this.camera.getViewMatrix().multiply(this.camera.getProjectionMatrix()),
      this.camera.viewport.toGlobal(engine.getRenderWidth(), engine.getRenderHeight()),
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
    const radius = box.hi.subtract(box.lo).length() * 1.8 || 4;

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
  private rescaleHandles() {
    const engine = this.scene.getEngine();
    const height = engine.getRenderHeight();
    if (this.wire) {
      // Half, because the threshold is a radius about the line. Taken at the
      // distance to what the camera is looking at: an exact answer would be
      // per edge, and this is a click tolerance.
      this.wire.intersectionThreshold =
        worldSizeForPixels(
          EDGE_PICK_PIXELS,
          Vector3.Distance(this.camera.target, this.camera.position),
          this.camera.fov,
          height,
        ) / 2;
    }
    for (const [meshes, pixels] of [
      [this.nodes, HANDLE_PIXELS],
      [this.geomMeshes.corners, CORNER_PIXELS],
    ] as const) {
      for (const mesh of meshes) {
        const distance = Vector3.Distance(mesh.position, this.camera.position);
        mesh.scaling.setAll(worldSizeForPixels(pixels, distance, this.camera.fov, height));
      }
    }
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
    return this.wire ? (this.wire.getIndices()?.length ?? 0) / 2 : 0;
  }

  render() {
    this.scene.render();
  }

  dispose() {
    this.scene.dispose();
  }
}
