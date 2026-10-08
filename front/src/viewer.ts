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
import { Color3, Color4 } from "@babylonjs/core/Maths/math.color";
import type { Mesh } from "@babylonjs/core/Meshes/mesh";
import type { LinesMesh } from "@babylonjs/core/Meshes/linesMesh";
import { CreateSphere } from "@babylonjs/core/Meshes/Builders/sphereBuilder";
import { CreateLineSystem } from "@babylonjs/core/Meshes/Builders/linesBuilder";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import { PointerDragBehavior } from "@babylonjs/core/Behaviors/Meshes/pointerDragBehavior";
import "@babylonjs/core/Culling/ray";
import type { View } from "./decode";
import { AxisGizmo, MAIN_LAYER } from "./axes";
import { DEFAULTS, type Theme } from "./theme";

/** Where a dragged node was let go, in model coordinates. */
export type Drop = (name: number, to: [number, number, number]) => void;

/** How many pixels across a node handle should be, whatever the zoom. */
export const HANDLE_PIXELS = 11;

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
  /** A working copy of the view: the drag previews against it, so the scene can
   *  follow the mouse without a round trip to the kernel. */
  private view?: View;
  private dropped: Drop[] = [];
  private theme: Theme = { ...DEFAULTS.theme };
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
    this.camera.wheelPrecision = 40;
    this.camera.minZ = 0.001;
    this.camera.layerMask = MAIN_LAYER;
    new HemisphericLight("hemi", new Vector3(0, 1, 0), this.scene).intensity = 0.9;

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
    this.applyTheme(this.theme);
  }

  /** How the scene looks. Not the kernel's business, so it never goes through
   *  it: the kernel says what exists, not what colour it is here. */
  applyTheme(theme: Theme) {
    this.theme = { ...theme };
    this.scene.clearColor = Color3.FromHexString(theme.background).toColor4(1);
    const nodes = Color3.FromHexString(theme.nodes);
    for (const mesh of this.nodes) {
      (mesh.material as StandardMaterial).emissiveColor = nodes;
    }
    if (this.wire) this.wire.color = Color3.FromHexString(theme.edges);
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
    };
    this.nodes.forEach((m) => m.dispose());
    this.nodes = [];
    this.wire?.dispose();
    this.wire = undefined;

    if (view.edges.length) {
      // Updatable, so a drag can redraw the edges without rebuilding the mesh.
      this.wire = CreateLineSystem(
        "edges",
        { lines: this.segments(), updatable: true },
        this.scene,
      );
      this.wire.color = Color3.FromHexString(this.theme.edges);
      this.wire.isPickable = false;
      this.wire.layerMask = MAIN_LAYER;
    }

    for (let n = 0; n < view.positions.length / 3; n++) {
      // Unit diameter; the per-frame pass below scales it to a fixed number
      // of pixels.
      const s = CreateSphere(`node${n}`, { diameter: 1, segments: 8 }, this.scene);
      s.position = this.point(n);
      // The kernel's name, not this index: the index is only good until the
      // next edit reorders the view.
      s.metadata = { index: n, name: view.names[n] };
      const m = new StandardMaterial(`nodeMat${n}`, this.scene);
      m.emissiveColor = Color3.FromHexString(this.theme.nodes);
      m.disableLighting = true;
      s.material = m;
      s.layerMask = MAIN_LAYER;
      this.nodes.push(s);
      this.makeDraggable(s, n);
    }
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
    if (this.wire) {
      this.wire = CreateLineSystem(
        "edges",
        { lines: this.segments(), updatable: true, instance: this.wire },
        this.scene,
      );
    }
  }

  private point(i: number): Vector3 {
    const p = this.view!.positions;
    return new Vector3(p[i * 3], p[i * 3 + 1], p[i * 3 + 2]);
  }

  private segments(): Vector3[][] {
    const edges = this.view!.edges;
    const out: Vector3[][] = [];
    for (let e = 0; e < edges.length; e += 2) {
      out.push([this.point(edges[e]), this.point(edges[e + 1])]);
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

  /** The node with this name, in screen coordinates — the inverse of
   *  `pickNode`, and what lets a test aim a real mouse at a real handle. */
  nodeScreen(name: number): [number, number] | null {
    const mesh = this.nodes.find((m) => m.metadata.name === name);
    if (!mesh) return null;
    const engine = this.scene.getEngine();
    // The main camera's own matrices, not `scene.getTransformMatrix()`: with
    // two active cameras that holds whichever rendered last, which is the
    // corner gizmo.
    const v = Vector3.Project(
      mesh.position,
      Matrix.Identity(),
      this.camera.getViewMatrix().multiply(this.camera.getProjectionMatrix()),
      this.camera.viewport.toGlobal(engine.getRenderWidth(), engine.getRenderHeight()),
    );
    return [v.x, v.y];
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

  /** Frame everything. Derived from the bounds alone, never from wherever the
   *  user last left the camera — screenshot tests depend on that. */
  frameAll() {
    if (!this.nodes.length) return;
    let lo = new Vector3(Infinity, Infinity, Infinity);
    let hi = new Vector3(-Infinity, -Infinity, -Infinity);
    for (const m of this.nodes) {
      lo = Vector3.Minimize(lo, m.position);
      hi = Vector3.Maximize(hi, m.position);
    }
    this.camera.setTarget(lo.add(hi).scale(0.5));
    this.camera.radius = hi.subtract(lo).length() * 1.8 || 4;
    this.camera.alpha = -Math.PI / 3;
    this.camera.beta = Math.PI / 3;
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
    if (!this.nodes.length) return;
    const engine = this.scene.getEngine();
    const height = engine.getRenderHeight();
    for (const mesh of this.nodes) {
      const distance = Vector3.Distance(mesh.position, this.camera.position);
      const wanted = worldSizeForPixels(HANDLE_PIXELS, distance, this.camera.fov, height);
      mesh.scaling.setAll(wanted);
    }
  }

  get nodeCount() {
    return this.nodes.length;
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
