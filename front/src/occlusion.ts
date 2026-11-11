/**
 * Screen-space ambient occlusion, and what it takes to make it mean
 * something here.
 *
 * The algorithm is Babylon's — `SSAO2RenderingPipeline`, sixteen samples in
 * a hemisphere around each pixel, read back out of a geometry buffer and
 * denoised. None of that is written here. What is written here is the three
 * things it needs that no library can know:
 *
 * **Its reach is in world units.** The radius it searches and the distance
 * past which it gives up are lengths, and a blocking is as likely to be a
 * unit cube as a part in millimetres. A fixed radius either finds nothing
 * at all or shades the whole model black, which is exactly what the first
 * attempt did. So both come off the model's own diagonal.
 *
 * **It must not see the wireframe or the handles.** They are geometry like
 * everything else, and a line lying along an edge of the model occludes the
 * faces either side of it: every edge ends up with a soot mark along it, and
 * every node with a ring. Only the surfaces go into the geometry buffer.
 * The pixels left out read as depth zero, which the shader's own range check
 * rejects, so the lines and handles come through at full brightness while the
 * faces behind them are shaded off the surfaces alone.
 *
 * **The prepass is not an option.** Given the choice the pipeline takes a
 * scene-wide prepass, which rides over the corner triad's camera and its
 * viewport and leaves the corner empty — and a prepass has no render list,
 * so the paragraph above becomes impossible. `forceGeometryBuffer` is what
 * keeps both the triad and the list.
 */

import type { AbstractMesh } from "@babylonjs/core/Meshes/abstractMesh";
import type { Camera } from "@babylonjs/core/Cameras/camera";
import type { Scene } from "@babylonjs/core/scene";
import { SSAO2RenderingPipeline } from "@babylonjs/core/PostProcesses/RenderPipeline/Pipelines/ssao2RenderingPipeline";
// Not for a name: importing these is what installs the pipeline manager and
// the geometry buffer on the scene. Without them the pipeline constructs
// against a scene that has nowhere to put it.
import "@babylonjs/core/PostProcesses/RenderPipeline/postProcessRenderPipelineManagerSceneComponent";
import "@babylonjs/core/Rendering/geometryBufferRendererSceneComponent";

/** How far around a pixel the occlusion looks, as a share of the model's
 *  diagonal. Small enough to shade the inside of a corner rather than one
 *  whole block, large enough to be more than noise. */
const RADIUS_SHARE = 0.14;
/** How far away it still bothers, as a multiple of the diagonal. Past this
 *  the shader fades the occlusion out, so it has to clear the far side of
 *  the model however far back the camera has been pulled. */
const REACH = 6;
/** How hard the shading bites.
 *
 *  Well above the pipeline's own default of one, which on these models is
 *  all but invisible: a blocking is nearly all flat right angles, the share
 *  of a hemisphere a right angle takes away is small, and sixteen samples
 *  average it down further. Two and a half darkens a corner enough to read
 *  as a corner without turning the inside of a pocket black. */
const STRENGTH = 2.5;
/** Samples per pixel. Sixteen is the pipeline's own figure for not needing
 *  the forgiving blur. */
const SAMPLES = 16;

/**
 * The occlusion, which may or may not be switched on.
 *
 * Off is the absence of the pipeline rather than a flag on it: a pipeline
 * that is present and inert still renders a geometry buffer every frame, and
 * the whole point of making this a setting is that it costs nothing when you
 * are not using it.
 */
export class Occlusion {
  private pipe?: SSAO2RenderingPipeline;
  /** The meshes the shading is computed from, as last handed to the
   *  geometry buffer. Kept so that a frame that changed nothing does not
   *  hand it the same list again. */
  private shaded: AbstractMesh[] = [];

  constructor(
    private readonly scene: Scene,
    private readonly camera: Camera,
  ) {}

  /** Whether this engine can do it at all. False under `NullEngine`, which
   *  is where the cheap tests run, and on anything without multiple render
   *  targets. */
  get available(): boolean {
    return SSAO2RenderingPipeline.IsSupported;
  }

  get on(): boolean {
    return this.pipe !== undefined;
  }

  /**
   * Switch it on or off.
   *
   * Returns whether it is now on, which is not the same as what was asked:
   * an engine that cannot do it says no.
   */
  set(on: boolean): boolean {
    if (on === this.on) return this.on;
    if (!on) {
      // With the geometry buffer, which exists for this and nothing else.
      this.pipe!.dispose(true);
      this.pipe = undefined;
      this.shaded = [];
      return false;
    }
    if (!this.available) return false;
    this.pipe = new SSAO2RenderingPipeline(
      "ao",
      this.scene,
      { ssaoRatio: 1, blurRatio: 1 },
      [this.camera],
      true,
    );
    this.pipe.samples = SAMPLES;
    this.pipe.totalStrength = STRENGTH;
    // No floor. The pipeline's `base` is added to the result rather than
    // being a minimum, so anything but zero wipes out the gentle shading
    // this is mostly made of and leaves only the deepest creases.
    this.pipe.base = 0;
    // The bilateral blur, which keeps the shading off the far side of an
    // edge instead of smearing it across.
    this.pipe.expensiveBlur = true;
    return true;
  }

  /**
   * Set the reach from the size of what is being looked at.
   *
   * Called whenever the model changes shape, which is the only thing that
   * changes the answer: the camera's distance does not, the shader working
   * in view space and fading on depth rather than on how far it has been
   * pulled back.
   */
  tune(diagonal: number) {
    if (!this.pipe || !(diagonal > 0)) return;
    this.pipe.radius = diagonal * RADIUS_SHARE;
    this.pipe.maxZ = diagonal * REACH;
  }

  /**
   * Shade from these meshes and no others.
   *
   * Handed the whole list every time rather than told what changed: the
   * surfaces are rebuilt from scratch by every edit, and a list holding one
   * mesh that has since been disposed is a crash in the middle of a frame.
   */
  shade(meshes: AbstractMesh[]) {
    const buffer = this.scene.geometryBufferRenderer;
    if (!this.pipe || !buffer) return;
    if (same(meshes, this.shaded)) return;
    this.shaded = meshes;
    buffer.renderList = meshes;
  }
}

function same(a: readonly AbstractMesh[], b: readonly AbstractMesh[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}
