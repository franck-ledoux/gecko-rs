/**
 * The window, split in four.
 *
 * Three orthographic views down the axes beside the perspective one. The
 * four are **not** kept in step with each other: there is one scene and one
 * set of meshes, so moving a node moves it in all four because it is the
 * same node. Only the cameras differ, and that is the whole of it — nothing
 * here can fall out of synchronisation because nothing here is
 * synchronised.
 *
 * The arithmetic is in this file and the cameras are not, for the usual
 * reason: where each pane sits, which pane a click lands in, and how big a
 * handle has to be to cover so many pixels are the things worth testing,
 * and none of them needs a GPU.
 */

import type { Axis } from "./axes";

/** The four panes, in the order they are laid out. */
export type Pane = "3d" | "x" | "y" | "z";

export const PANES: readonly Pane[] = ["3d", "x", "y", "z"];

/** Which way an axis pane looks. The 3-D one looks wherever you point it. */
export const PANE_AXIS: Record<Pane, Axis | null> = {
  "3d": null,
  x: "+x",
  y: "+y",
  z: "+z",
};

/** A rectangle in Babylon's fractions of the canvas, **y from the bottom**. */
export interface Fractions {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Where a pane sits.
 *
 * ```text
 *   ┌────────┬────────┐
 *   │   3D   │   X    │
 *   ├────────┼────────┤
 *   │   Y    │   Z    │
 *   └────────┴────────┘
 * ```
 *
 * On one pane the 3-D view has the lot and the other three have nothing —
 * they are not drawn at all, so the figures are only there to keep the
 * function total.
 */
export function paneViewport(pane: Pane, quad: boolean): Fractions {
  if (!quad) return pane === "3d" ? { x: 0, y: 0, w: 1, h: 1 } : { x: 0, y: 0, w: 0, h: 0 };
  const left = pane === "3d" || pane === "y";
  const top = pane === "3d" || pane === "x";
  return { x: left ? 0 : 0.5, y: top ? 0.5 : 0, w: 0.5, h: 0.5 };
}

/** The same rectangle in **CSS pixels from the top left**, which is the
 *  frame a pointer event speaks. */
export function paneRect(
  pane: Pane,
  quad: boolean,
  width: number,
  height: number,
): { x: number; y: number; width: number; height: number } {
  const f = paneViewport(pane, quad);
  return {
    x: f.x * width,
    // A Babylon viewport measures y from the bottom and a pointer from the
    // top, which is the one difference a full-screen viewport hides.
    y: (1 - f.y - f.h) * height,
    width: f.w * width,
    height: f.h * height,
  };
}

/** Which pane a point of the canvas is in. */
export function paneAt(quad: boolean, x: number, y: number, width: number, height: number): Pane {
  if (!quad) return "3d";
  const left = x < width / 2;
  const top = y < height / 2;
  return top ? (left ? "3d" : "x") : left ? "y" : "z";
}

/**
 * The half-height an orthographic camera needs to hold a model of this size.
 *
 * Half a diagonal, which holds the model whichever way it is turned, plus a
 * margin so it is not flush with the edge.
 */
export function orthoHalfFor(diagonal: number): number {
  return (diagonal || 2) * 0.62;
}

/**
 * How far an orthographic camera reaches, left/right/top/bottom.
 *
 * The height is the half-height and the width follows the pane's shape, so
 * the model is not stretched. A pane of no height — which is every pane but
 * one when the window is not split — would divide by nothing, so it is
 * given a square.
 */
export function orthoExtents(
  half: number,
  width: number,
  height: number,
): { left: number; right: number; top: number; bottom: number } {
  const aspect = height > 0 ? width / height : 1;
  return { left: -half * aspect, right: half * aspect, top: half, bottom: -half };
}

/**
 * The world size that covers `pixels` of an orthographic pane.
 *
 * No distance in it, which is the whole difference from the perspective
 * case: an orthographic projection draws everything at one scale, so the
 * answer depends on how much of the world the pane shows and on how many
 * pixels tall it is, and on nothing else.
 */
export function worldSizeForPixelsOrtho(
  pixels: number,
  orthoHalfHeight: number,
  paneHeightPixels: number,
): number {
  if (paneHeightPixels <= 0) return 0;
  return (pixels * 2 * orthoHalfHeight) / paneHeightPixels;
}
