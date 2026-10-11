/**
 * Turning the view's quadrangles into something a scene can hold.
 *
 * Pure arithmetic on typed arrays, with no Babylon in sight, because this is
 * where the two things worth testing live: which quadrangles get drawn, and
 * where shrinking puts their corners.
 *
 * **One surface, two meshes.** The kernel ships each quadrangle once, carrying
 * both the face's name and the block's. Drawing faces and drawing blocks is
 * then the same geometry read twice — differing in which name picks the
 * colour, and which centre the shrink pulls towards. That is why they can both
 * be on at once and still be two separate things on screen.
 *
 * Vertices are **not** shared between quadrangles. They cannot be: each one
 * has its own colour and is pulled towards its own centre, so a corner on
 * three faces is three different points.
 */

import type { View } from "./decode";
import { colourForId, FAMILY, hexToRgb } from "./palette";

/** One colour for everything, or one per cell name. */
export type Colouring = "uniform" | "id";

export interface SurfaceOptions {
  /** Which name picks the colour, and which centre the shrink pulls towards. */
  by: "face" | "block";
  colouring: Colouring;
  /** Used when `colouring` is `"uniform"`. */
  uniform: string;
  /** 1 leaves a cell where it is; smaller pulls it towards its own centre. */
  shrink: number;
  /** Draw faces that have a block on both sides.
   *
   *  Only meaningful for `by: "face"`. A block is a closed shell, so it keeps
   *  all six of its own whatever this says — otherwise shrinking one would
   *  open it. */
  internal: boolean;
  /** Faces to paint in a colour of their own, whatever the colouring says:
   *  what the user has picked. Keyed by face name, so a selected face stands
   *  out on both meshes at once. */
  highlight?: ReadonlyMap<number, string>;
  /** The colour a face wears for the geometric surface it is classified
   *  on, keyed by face name. Beaten by `highlight` — what you have picked
   *  is more urgent than what it follows — and beating the colouring
   *  below, which is about telling cells apart rather than about where
   *  they belong. Only meaningful for `by: "face"`: a block is not
   *  classified on anything. */
  classified?: ReadonlyMap<number, string>;
}

export interface SurfaceData {
  /** Three per vertex, four vertices per quadrangle. */
  positions: Float32Array;
  /** Six per quadrangle: two triangles. */
  indices: Uint32Array;
  /** Four per vertex, RGBA. The alpha is left at 1 — a group's transparency
   *  is one number on its material, not a thousand copies in a buffer. */
  colors: Float32Array;
  /** The face each drawn quadrangle belongs to.
   *
   *  Carried whichever group this is, because picking has to answer "which
   *  face" even when what is on screen is the blocks. */
  faces: Uint32Array;
  /** The block each drawn quadrangle belongs to. */
  blocks: Uint32Array;
}

const EMPTY: SurfaceData = {
  positions: new Float32Array(0),
  indices: new Uint32Array(0),
  colors: new Float32Array(0),
  faces: new Uint32Array(0),
  blocks: new Uint32Array(0),
};

/**
 * Which quadrangles to draw.
 *
 * For blocks, all of them. For faces, one per face name — a face shared by two
 * blocks arrives twice, once per block, and it is still one face — and then
 * only the shared ones or only the unshared ones, as asked.
 */
export function quadsToDraw(view: View, by: "face" | "block", internal: boolean): number[] {
  const count = view.quadFaces.length;
  if (by === "block") return Array.from({ length: count }, (_, q) => q);

  const sides = new Map<number, number>();
  for (const name of view.quadFaces) sides.set(name, (sides.get(name) ?? 0) + 1);

  const kept: number[] = [];
  const taken = new Set<number>();
  for (let q = 0; q < count; q++) {
    const name = view.quadFaces[q];
    if (taken.has(name)) continue;
    if (!internal && (sides.get(name) ?? 0) > 1) continue;
    taken.add(name);
    kept.push(q);
  }
  return kept;
}

/** The centre each quadrangle shrinks towards. */
function centres(view: View, by: "face" | "block"): Map<number, [number, number, number]> {
  const names = by === "face" ? view.quadFaces : view.quadBlocks;
  const sums = new Map<number, [number, number, number, number]>();
  for (let q = 0; q < names.length; q++) {
    const at = sums.get(names[q]) ?? [0, 0, 0, 0];
    for (let k = 0; k < 4; k++) {
      const p = view.quads[q * 4 + k] * 3;
      at[0] += view.positions[p];
      at[1] += view.positions[p + 1];
      at[2] += view.positions[p + 2];
      at[3] += 1;
    }
    sums.set(names[q], at);
  }
  // A plain mean over the corners as they are listed. For a block it counts
  // each of the eight corners three times — once per face it is on — which,
  // being the same count for every corner, lands on the same point as
  // averaging them once. For anything less regular it is an area-weighted
  // centre, which is still exactly what a shrink wants.
  const out = new Map<number, [number, number, number]>();
  for (const [name, [x, y, z, n]] of sums) out.set(name, [x / n, y / n, z / n]);
  return out;
}

/**
 * One `SurfaceData` per block.
 *
 * Not one for the lot. Babylon sorts transparent **meshes** back to front
 * and does nothing about the triangles inside one, so a single mesh holding
 * every cell leaves it nothing to sort: a block behind another could be
 * drawn over it. A mesh per block gives the sorter something to work with,
 * at the cost of a draw call each — which is the trade to revisit when
 * there are thousands of them, not now.
 */
export function buildSurfaces(
  view: View,
  options: SurfaceOptions,
): { block: number; data: SurfaceData }[] {
  const kept = quadsToDraw(view, options.by, options.internal);
  const byBlock = new Map<number, number[]>();
  for (const q of kept) {
    const block = view.quadBlocks[q];
    const already = byBlock.get(block);
    if (already) already.push(q);
    else byBlock.set(block, [q]);
  }
  return [...byBlock.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([block, quads]) => ({ block, data: buildFrom(view, options, quads) }));
}

export function buildSurface(view: View, options: SurfaceOptions): SurfaceData {
  return buildFrom(view, options, quadsToDraw(view, options.by, options.internal));
}

function buildFrom(view: View, options: SurfaceOptions, kept: number[]): SurfaceData {
  if (!kept.length) return EMPTY;

  const names = options.by === "face" ? view.quadFaces : view.quadBlocks;
  const dimension = options.by === "face" ? FAMILY.faces : FAMILY.blocks;
  const middle = centres(view, options.by);
  const uniform = hexToRgb(options.uniform);

  const positions = new Float32Array(kept.length * 12);
  const colors = new Float32Array(kept.length * 16);
  const indices = new Uint32Array(kept.length * 6);
  const faces = new Uint32Array(kept.length);
  const blocks = new Uint32Array(kept.length);

  kept.forEach((q, i) => {
    const name = names[q];
    faces[i] = view.quadFaces[q];
    blocks[i] = view.quadBlocks[q];
    const c = middle.get(name) ?? [0, 0, 0];
    const picked = options.highlight?.get(view.quadFaces[q]);
    const follows = options.by === "face" ? options.classified?.get(view.quadFaces[q]) : undefined;
    const rgb = picked
      ? hexToRgb(picked)
      : follows
        ? hexToRgb(follows)
        : options.colouring === "id"
          ? hexToRgb(colourForId(name, dimension))
          : uniform;

    for (let k = 0; k < 4; k++) {
      const p = view.quads[q * 4 + k] * 3;
      const v = (i * 4 + k) * 3;
      for (let axis = 0; axis < 3; axis++) {
        positions[v + axis] = c[axis] + (view.positions[p + axis] - c[axis]) * options.shrink;
      }
      const col = (i * 4 + k) * 4;
      colors[col] = rgb[0];
      colors[col + 1] = rgb[1];
      colors[col + 2] = rgb[2];
      colors[col + 3] = 1;
    }

    // Two triangles, keeping the quadrangle's outward winding.
    const base = i * 4;
    indices.set([base, base + 1, base + 2, base, base + 2, base + 3], i * 6);
  });

  return { positions, indices, colors, faces, blocks };
}

/**
 * The edges, each pulled towards its own midpoint.
 *
 * Six numbers per segment. Shrinking these is what separates the wireframe
 * from the surfaces it used to sit exactly on top of — which is the point of
 * having the control at all.
 */
export function edgeSegments(view: View, shrink: number): Float32Array {
  const out = new Float32Array((view.edges.length / 2) * 6);
  for (let e = 0; e < view.edges.length; e += 2) {
    const a = view.edges[e] * 3;
    const b = view.edges[e + 1] * 3;
    const at = (e / 2) * 6;
    for (let axis = 0; axis < 3; axis++) {
      const mid = (view.positions[a + axis] + view.positions[b + axis]) / 2;
      out[at + axis] = mid + (view.positions[a + axis] - mid) * shrink;
      out[at + 3 + axis] = mid + (view.positions[b + axis] - mid) * shrink;
    }
  }
  return out;
}
