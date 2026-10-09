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
import { Matrix, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Animation } from "@babylonjs/core/Animations/animation";
import { CubicEase, EasingFunction } from "@babylonjs/core/Animations/easing";
import "@babylonjs/core/Animations/animatable";
import { Color3, Color4 } from "@babylonjs/core/Maths/math.color";
import { Mesh } from "@babylonjs/core/Meshes/mesh";
import { VertexData } from "@babylonjs/core/Meshes/mesh.vertexData";
import { VertexBuffer } from "@babylonjs/core/Buffers/buffer";
import type { LinesMesh } from "@babylonjs/core/Meshes/linesMesh";
import { CreateSphere } from "@babylonjs/core/Meshes/Builders/sphereBuilder";
import { CreateLineSystem } from "@babylonjs/core/Meshes/Builders/linesBuilder";
import { CreateGreasedLine } from "@babylonjs/core/Meshes/Builders/greasedLineBuilder";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import { PointerDragBehavior } from "@babylonjs/core/Behaviors/Meshes/pointerDragBehavior";
import "@babylonjs/core/Culling/ray";
import type { View } from "./decode";
import { AxisGizmo, MAIN_LAYER } from "./axes";
import { DEFAULTS, type Layer, type ViewPrefs } from "./theme";
import { ANCHOR, MATE } from "./palette";
import {
  buildSurface,
  edgeSegments,
  type SurfaceData,
  type SurfaceOptions,
} from "./surface";

/** The two groups drawn from the same quadrangles. */
export type Group = "faces" | "blocks";

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

/** How near a line the pointer has to be to count as on it. Lines are one
 *  pixel wide and nobody can hit one. */
export const EDGE_PICK_PIXELS = 7;

/**
 * How thick a lit sheet is drawn.
 *
 * WebGL caps `lineWidth` at one pixel whatever you ask for, so a thick line
 * is not a line at all — it is a ribbon built as geometry. That is what
 * `CreateGreasedLine` makes, and with `sizeAttenuation` off its width is a
 * share of the screen rather than of the model, so a sheet reads the same
 * however far away it is.
 */
export const SHEET_WIDTH = 0.012;

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
  /** The sheet the viewer has been asked to light up, by edge name. */
  private lit: number[] = [];
  /** The edge that sheet was asked for, so a second click on it can put the
   *  sheet out again. View state, like the face selection. */
  selectedEdge: number | null = null;
  private litWire?: Mesh;
  private surfaces: Record<Group, { mesh?: Mesh; data?: SurfaceData }> = {
    faces: {},
    blocks: {},
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
    const handles = (m: { metadata?: { name?: number } }) => (m.metadata?.name ?? -1) >= 0;
    this.scene.pointerDownPredicate = handles;
    this.scene.pointerUpPredicate = handles;
    this.scene.pointerMovePredicate = handles;
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
      (m) => m.name === "faces" || m.name === "blocks",
      false,
      this.camera,
    );
    if (!hit?.hit || !hit.pickedMesh) return null;
    const data = this.surfaces[hit.pickedMesh.name as Group].data;
    const quad = Math.floor(hit.faceId / 2);
    if (!data || quad < 0 || quad >= data.faces.length) return null;
    return { face: data.faces[quad], block: data.blocks[quad] };
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
    const slot = this.surfaces[group];
    const layer = this.layerOf(group);
    const data =
      this.view && layer.visible && this.view.quads.length
        ? buildSurface(this.view, this.surfaceOptions(group))
        : undefined;

    if (!data?.indices.length) {
      slot.mesh?.dispose();
      slot.mesh = undefined;
      slot.data = undefined;
      return;
    }

    if (slot.mesh && slot.data?.positions.length === data.positions.length) {
      slot.data = data;
      slot.mesh.updateVerticesData(VertexBuffer.PositionKind, data.positions);
      slot.mesh.updateVerticesData(VertexBuffer.ColorKind, data.colors);
      slot.mesh.updateVerticesData(VertexBuffer.NormalKind, normalsFor(data));
      (slot.mesh.material as StandardMaterial).alpha = layer.alpha;
      return;
    }

    slot.mesh?.dispose();
    const mesh = new Mesh(group, this.scene);
    const vertices = new VertexData();
    vertices.positions = data.positions;
    vertices.indices = data.indices;
    vertices.colors = data.colors;
    vertices.normals = normalsFor(data);
    // Updatable: everything above depends on it.
    vertices.applyToMesh(mesh, true);

    const material = new StandardMaterial(`${group}Mat`, this.scene);
    material.specularColor = Color3.Black();
    // Both sides: a translucent shell you can only see the outside of is a
    // picture of its silhouette.
    material.backFaceCulling = false;
    material.alpha = layer.alpha;
    // Push the surface a hair away from the camera in depth only. The edges
    // run exactly along it — they are the same corners — so without this the
    // depth test decides between them arbitrarily, and at full opacity the
    // wireframe simply vanishes into the faces it borders.
    material.zOffset = SURFACE_DEPTH_BIAS.factor;
    material.zOffsetUnits = SURFACE_DEPTH_BIAS.units;
    mesh.material = material;
    // Pickable, because a face is something you choose with the mouse. The
    // node handles still win where they overlap: a sphere is centred on the
    // surface, so its near half is in front, and that is the half you aim at.
    mesh.isPickable = true;
    mesh.layerMask = MAIN_LAYER;

    slot.mesh = mesh;
    slot.data = data;
  }

  /** The same bargain as the surfaces: reuse the mesh while the number of
   *  segments holds, which is every case except a new view. */
  private rebuildEdges() {
    if (!this.view?.edges.length) {
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

  /** Keep the corner viewport square when the canvas changes shape. */
  resize() {
    this.axes.resize();
  }

  attachTo(canvas: HTMLCanvasElement) {
    this.camera.attachControl(canvas, true);
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
  highlightEdges(names: readonly number[]) {
    this.lit = [...names];
    this.rebuildHighlight();
  }

  get highlighted(): number[] {
    return [...this.lit];
  }

  private rebuildHighlight() {
    this.litWire?.dispose();
    this.litWire = undefined;
    if (!this.view || !this.lit.length) return;

    const wanted = new Set(this.lit);
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
      { color: Color3.FromHexString(ANCHOR), width: SHEET_WIDTH, sizeAttenuation: false },
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
      const data = this.surfaces[group].data;
      const quad = data ? data.faces.indexOf(name) : -1;
      if (!data || quad < 0) continue;
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

  /** The model's extent, from the handles, which are on every node there is. */
  private bounds(): { lo: Vector3; hi: Vector3 } | null {
    if (!this.nodes.length) return null;
    let lo = new Vector3(Infinity, Infinity, Infinity);
    let hi = new Vector3(-Infinity, -Infinity, -Infinity);
    for (const m of this.nodes) {
      lo = Vector3.Minimize(lo, m.position);
      hi = Vector3.Maximize(hi, m.position);
    }
    return { lo, hi };
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
    if (!this.nodes.length) return;
    for (const mesh of this.nodes) {
      const distance = Vector3.Distance(mesh.position, this.camera.position);
      const wanted = worldSizeForPixels(HANDLE_PIXELS, distance, this.camera.fov, height);
      mesh.scaling.setAll(wanted);
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
    const slot = this.surfaces[group];
    if (!slot.mesh || !slot.data) return null;
    const seen = new Set<string>();
    for (let v = 0; v < slot.data.colors.length; v += 4) {
      seen.add(slot.data.colors.slice(v, v + 3).join(","));
    }
    return {
      quads: slot.data.indices.length / 6,
      alpha: (slot.mesh.material as StandardMaterial).alpha,
      colours: seen.size,
      positions: slot.data.positions,
      faces: slot.data.faces,
      blocks: slot.data.blocks,
    };
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
