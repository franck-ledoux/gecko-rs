/**
 * The orientation gizmo: a little X/Y/Z triad in the corner, which you can
 * click to look down an axis.
 *
 * It is a **second camera on a corner viewport**, not an HTML overlay. Two
 * things follow from that and neither is available to a div: it shares the
 * main camera's orientation exactly, because it *is* a camera looking at the
 * same rotation; and it can be picked, because it is geometry.
 *
 * Separation is by layer mask. Every mesh in the scene has to opt into one
 * side or the other — a mesh created without one renders in both, so the
 * viewer sets [`MAIN_LAYER`] on everything it builds.
 */

import { ArcRotateCamera } from "@babylonjs/core/Cameras/arcRotateCamera";
import { Animation } from "@babylonjs/core/Animations/animation";
// Side-effect import: it is what puts `beginDirectAnimation` on Scene.
// Importing `Animation` alone gets you the class and a runtime TypeError —
// tree-shaking has no way to know the method is wanted.
import "@babylonjs/core/Animations/animatable";
import { DynamicTexture } from "@babylonjs/core/Materials/Textures/dynamicTexture";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import { Color3 } from "@babylonjs/core/Maths/math.color";
import { Matrix, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Viewport } from "@babylonjs/core/Maths/math.viewport";
import { Mesh } from "@babylonjs/core/Meshes/mesh";
import { CreateLineSystem } from "@babylonjs/core/Meshes/Builders/linesBuilder";
import { CreatePlane } from "@babylonjs/core/Meshes/Builders/planeBuilder";
import { CreateSphere } from "@babylonjs/core/Meshes/Builders/sphereBuilder";
import type { Scene } from "@babylonjs/core/scene";

/** Meshes the main camera draws. */
export const MAIN_LAYER = 0x1;
/** How many pixels across the corner triad should be. */
export const CORNER_PIXELS = 150;

/** Meshes only the corner viewport draws. */
export const GIZMO_LAYER = 0x2;

export type Axis = "+x" | "-x" | "+y" | "-y" | "+z" | "-z";

export const AXES: readonly Axis[] = ["+x", "-x", "+y", "-y", "+z", "-z"];

/** The conventional colours: X red, Y green, Z blue. */
const COLOUR: Record<"x" | "y" | "z", Color3> = {
  x: new Color3(0.92, 0.3, 0.33),
  y: new Color3(0.35, 0.82, 0.4),
  z: new Color3(0.36, 0.56, 0.95),
};

/**
 * Looking straight down the Y axis is a singularity for an arc-rotate camera:
 * at `beta` exactly 0 or π the azimuth means nothing and the view can flip.
 * Stopping a thousandth of a radian short is invisible and well-defined.
 */
const POLE = 1e-3;

/**
 * Where the camera goes to look down `axis`.
 *
 * A pure function, so the six cases are checkable without a scene. The
 * convention is that the camera sits **on** the named axis — clicking `+x`
 * puts you on +X looking back at the model, which is the YZ plane.
 *
 * For Babylon's arc-rotate camera the position is
 * `(r·cos α·sin β, r·cos β, r·sin α·sin β)`.
 */
export function orientationFor(axis: Axis): { alpha: number; beta: number } {
  switch (axis) {
    case "+x":
      return { alpha: 0, beta: Math.PI / 2 };
    case "-x":
      return { alpha: Math.PI, beta: Math.PI / 2 };
    case "+z":
      return { alpha: Math.PI / 2, beta: Math.PI / 2 };
    case "-z":
      return { alpha: -Math.PI / 2, beta: Math.PI / 2 };
    case "+y":
      return { alpha: -Math.PI / 2, beta: POLE };
    case "-y":
      return { alpha: -Math.PI / 2, beta: Math.PI - POLE };
  }
}

/**
 * The equivalent of `target` nearest to `from`.
 *
 * Azimuth is periodic, so 3.1 and −3.1 are a tenth of a turn apart and also
 * six radians apart. Animating to the raw value takes the long way round,
 * which looks like the model spinning for no reason.
 */
export function shortestTurn(from: number, target: number): number {
  return target + 2 * Math.PI * Math.round((from - target) / (2 * Math.PI));
}

function canDrawText(): boolean {
  return typeof document !== "undefined" || typeof OffscreenCanvas !== "undefined";
}

export class AxisGizmo {
  readonly camera: ArcRotateCamera;
  private meshes: Mesh[] = [];
  private shown = true;

  constructor(
    private scene: Scene,
    private main: ArcRotateCamera,
  ) {
    // Far enough back that the triad and its labels fit the corner whatever
    // way round it is turned: the axes reach 2 and the labels sit beyond that,
    // so the bounding sphere is about 3.
    this.camera = new ArcRotateCamera("axesCam", main.alpha, main.beta, 10, Vector3.Zero(), scene);
    this.camera.layerMask = GIZMO_LAYER;
    this.build();
    this.resize();

    // Mirror the main camera's rotation, but never its distance: the triad
    // must not shrink when you zoom out.
    scene.onBeforeRenderObservable.add(() => {
      this.camera.alpha = main.alpha;
      this.camera.beta = main.beta;
    });
  }

  private build() {
    const unlit = (name: string, colour: Color3) => {
      const m = new StandardMaterial(name, this.scene);
      m.emissiveColor = colour;
      m.disableLighting = true;
      return m;
    };

    for (const axis of AXES) {
      const letter = axis[1] as "x" | "y" | "z";
      const sign = axis[0] === "+" ? 1 : -1;
      const end = new Vector3(
        letter === "x" ? sign * 2 : 0,
        letter === "y" ? sign * 2 : 0,
        letter === "z" ? sign * 2 : 0,
      );

      const ball = CreateSphere(`axis${axis}`, { diameter: sign > 0 ? 0.9 : 0.6 }, this.scene);
      ball.position = end;
      ball.material = unlit(`axisMat${axis}`, COLOUR[letter].scale(sign > 0 ? 1 : 0.45));
      // What the pick reads back.
      ball.metadata = { axis };
      this.adopt(ball);

      if (sign > 0) {
        const line = CreateLineSystem(
          `axisLine${axis}`,
          { lines: [[Vector3.Zero(), end]] },
          this.scene,
        );
        line.color = COLOUR[letter];
        line.isPickable = false;
        this.adopt(line as unknown as Mesh);
        // The lettering needs a 2D canvas to draw into, and a headless engine
        // has none. The triad is still built and still pickable without it,
        // which is all the headless tests ask of it — and nobody is reading
        // an "X" in Node.
        if (canDrawText()) this.adopt(this.label(letter, end.scale(1.38)));
      }
    }
  }

  /** A billboarded letter, so the triad reads without a legend. */
  private label(letter: "x" | "y" | "z", at: Vector3): Mesh {
    const texture = new DynamicTexture(`axisLabel${letter}`, { width: 64, height: 64 }, this.scene, false);
    texture.hasAlpha = true;
    // The letter takes its axis's colour rather than white: white vanishes on
    // a light background, and the theme is the viewer's to change.
    texture.drawText(
      letter.toUpperCase(),
      null,
      50,
      "bold 52px sans-serif",
      COLOUR[letter].toHexString(),
      "transparent",
      true,
    );

    const material = new StandardMaterial(`axisLabelMat${letter}`, this.scene);
    material.diffuseTexture = texture;
    material.emissiveTexture = texture;
    material.opacityTexture = texture;
    material.disableLighting = true;
    material.backFaceCulling = false;

    const plane = CreatePlane(`axisLabelPlane${letter}`, { size: 1.3 }, this.scene);
    plane.position = at;
    plane.material = material;
    plane.billboardMode = Mesh.BILLBOARDMODE_ALL;
    plane.isPickable = false;
    return plane;
  }

  private adopt(mesh: Mesh) {
    mesh.layerMask = GIZMO_LAYER;
    mesh.renderingGroupId = 1;
    this.meshes.push(mesh);
  }

  /**
   * Keep the corner square, in the corner of the pane it belongs to.
   *
   * A viewport is given in fractions of the **canvas**, so a fixed pair of
   * fractions is only square when the canvas is, and the corner of a pane
   * is not the corner of the window. Both are worked out here: the height
   * from the width and the aspect ratio, the position from where the pane
   * sits.
   *
   * `host` is the pane's rectangle in CSS pixels from the top left, and
   * defaults to the whole canvas.
   */
  resize(
    host?: { x: number; y: number; width: number; height: number },
    canvasWidth?: number,
    canvasHeight?: number,
  ) {
    const engine = this.scene.getEngine();
    // CSS pixels, not the backing store. A fraction worked out from the
    // backing store makes the triad **half the size** on a high-density
    // display, because 150 of those pixels are 75 of the ones anyone sees.
    const css = engine.getHardwareScalingLevel();
    const width = canvasWidth ?? (engine.getRenderWidth() || 1) * css;
    const height = canvasHeight ?? (engine.getRenderHeight() || 1) * css;
    const pane = host ?? { x: 0, y: 0, width, height };
    const w = Math.min(0.22, CORNER_PIXELS / width);
    const h = (w * width) / height;
    // The pane's bottom right, less the same small gap, expressed back in
    // fractions of the whole canvas.
    const right = (pane.x + pane.width) / width;
    const bottom = (height - (pane.y + pane.height)) / height;
    this.camera.viewport = new Viewport(right - w - 0.012, bottom + 0.018, w, h);
  }

  setVisible(visible: boolean) {
    this.shown = visible;
    this.meshes.forEach((m) => m.setEnabled(visible));
  }

  get visible() {
    return this.shown;
  }

  /** The axis under these screen coordinates, if any. */
  pick(x: number, y: number): Axis | null {
    if (!this.shown) return null;
    const hit = this.scene.pick(x, y, (m) => !!m.metadata?.axis, false, this.camera);
    return hit?.hit && hit.pickedMesh ? (hit.pickedMesh.metadata.axis as Axis) : null;
  }

  /**
   * Where an axis handle is on screen, in **CSS pixels** — the inverse of
   * [`AxisGizmo.pick`], and in the frame that takes.
   *
   * Projected through the gizmo's **own** camera and viewport, not the
   * scene's: the corner is a different projection entirely, and using the main
   * camera's transform would point somewhere in the middle of the model.
   *
   * * Render pixels to the CSS pixels a pointer event speaks.
   *
   * `Vector3.Project` works in the canvas's backing store, which on a
   * high-density display is twice the size of the canvas itself. `scene.pick`
   * and `offsetX`/`offsetY` are both in CSS pixels. The two frames coincide at
   * a device pixel ratio of 1 — which is what a headless test runs at, so no
   * test here can see the difference — and are a factor of two apart on the
   * machine this is used on.
   */
  axisScreen(axis: Axis): [number, number] | null {
    const mesh = this.meshes.find((m) => m.metadata?.axis === axis);
    if (!mesh) return null;
    const engine = this.scene.getEngine();
    const width = engine.getRenderWidth();
    const height = engine.getRenderHeight();
    const pixels = this.camera.viewport.toGlobal(width, height);
    // A Babylon viewport measures y from the bottom; `Project` wants it from
    // the top. Full-screen viewports hide the difference because the offset is
    // zero either way — this one is in the bottom corner, so it does not.
    const topDown = new Viewport(pixels.x, height - pixels.y - pixels.height, pixels.width, pixels.height);
    const projected = Vector3.Project(
      mesh.position,
      Matrix.Identity(),
      this.camera.getViewMatrix().multiply(this.camera.getProjectionMatrix()),
      topDown,
    );
    const css = engine.getHardwareScalingLevel();
    return [projected.x * css, projected.y * css];
  }

  /** Swing the main camera onto an axis, rather than snapping: a jump loses
   *  the reader's sense of which way round the model is. */
  lookDown(axis: Axis, milliseconds = 350) {
    const { alpha, beta } = orientationFor(axis);
    const frames = 30;
    Animation.CreateAndStartAnimation(
      "axisAlpha",
      this.main,
      "alpha",
      (frames * 1000) / milliseconds,
      frames,
      this.main.alpha,
      shortestTurn(this.main.alpha, alpha),
      Animation.ANIMATIONLOOPMODE_CONSTANT,
    );
    Animation.CreateAndStartAnimation(
      "axisBeta",
      this.main,
      "beta",
      (frames * 1000) / milliseconds,
      frames,
      this.main.beta,
      beta,
      Animation.ANIMATIONLOOPMODE_CONSTANT,
    );
  }

  /** Put the camera on an axis at once, with no animation. */
  snapTo(axis: Axis) {
    const { alpha, beta } = orientationFor(axis);
    this.main.alpha = shortestTurn(this.main.alpha, alpha);
    this.main.beta = beta;
  }

  dispose() {
    this.meshes.forEach((m) => m.dispose());
    this.camera.dispose();
  }
}
