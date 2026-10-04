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
import { Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Color3, Color4 } from "@babylonjs/core/Maths/math.color";
import type { Mesh } from "@babylonjs/core/Meshes/mesh";
import type { LinesMesh } from "@babylonjs/core/Meshes/linesMesh";
import { CreateSphere } from "@babylonjs/core/Meshes/Builders/sphereBuilder";
import { CreateLineSystem } from "@babylonjs/core/Meshes/Builders/linesBuilder";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import type { View } from "./decode";

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
    new HemisphericLight("hemi", new Vector3(0, 1, 0), this.scene).intensity = 0.9;
  }

  attachTo(canvas: HTMLCanvasElement) {
    this.camera.attachControl(canvas, true);
  }

  setView(view: View) {
    this.centre = view.centre;
    this.nodes.forEach((m) => m.dispose());
    this.nodes = [];
    this.wire?.dispose();
    this.wire = undefined;

    const point = (i: number) =>
      new Vector3(view.positions[i * 3], view.positions[i * 3 + 1], view.positions[i * 3 + 2]);

    const segments: Vector3[][] = [];
    for (let e = 0; e < view.edges.length; e += 2) {
      segments.push([point(view.edges[e]), point(view.edges[e + 1])]);
    }
    if (segments.length) {
      this.wire = CreateLineSystem("edges", { lines: segments }, this.scene);
      this.wire.color = new Color3(0.95, 0.95, 1);
      this.wire.isPickable = false;
    }

    const size = this.span(view) * 0.03;
    for (let n = 0; n < view.positions.length / 3; n++) {
      const s = CreateSphere(`node${n}`, { diameter: size, segments: 8 }, this.scene);
      s.position = point(n);
      s.metadata = { node: n };
      const m = new StandardMaterial(`nodeMat${n}`, this.scene);
      m.emissiveColor = new Color3(0.4, 0.7, 1);
      m.disableLighting = true;
      s.material = m;
      this.nodes.push(s);
    }
  }

  private span(view: View): number {
    let max = 0;
    for (const x of view.positions) max = Math.max(max, Math.abs(x));
    return max * 2 || 1;
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
