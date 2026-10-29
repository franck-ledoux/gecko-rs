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
 * color, and which center the shrink pulls towards. That is why they can both
 * be on at once and still be two separate things on screen.
 *
 * Vertices are **not** shared between quadrangles. They cannot be: each one
 * has its own color and is pulled towards its own center, so a corner on
 * three faces is three different points.
 */

import type { MeshView, View } from "./decode";
import { colorForId, FAMILY, hexToRgb } from "./palette";

/**
 * One color for everything, one per cell name, or one per **kind of thing
 * the cell is classified on**.
 *
 * The third is the one worth explaining. `class` says nothing about which
 * cell this is and everything about where it belongs: one color for a
 * cell on a corner of the model, one for a curve, one for a surface, and
 * the group's own color for a cell on nothing. Three colors and a legend
 * rather than a hue per entity — what you are reading off is whether the
 * blocking has taken hold of the geometry, and that is a question with
 * four answers.
 */
export type Coloring = "uniform" | "id" | "class" | "group";

export interface SurfaceOptions {
  /** Which name picks the color, and which center the shrink pulls towards. */
  by: "face" | "block";
  coloring: Coloring;
  /** Used when `coloring` is `"uniform"`. */
  uniform: string;
  /** 1 leaves a cell where it is; smaller pulls it towards its own center. */
  shrink: number;
  /** Draw faces that have a block on both sides.
   *
   *  Only meaningful for `by: "face"`. A block is a closed shell, so it keeps
   *  all six of its own whatever this says — otherwise shrinking one would
   *  open it. */
  internal: boolean;
  /** Faces to paint in a color of their own, whatever the coloring says:
   *  what the user has picked. Keyed by face name, so a selected face stands
   *  out on both meshes at once. */
  highlight?: ReadonlyMap<number, string>;
  /** The color a face wears for what it is classified on, keyed by face
   *  name, and only when `coloring` is `class`. Beaten by `highlight`:
   *  what you have picked is more urgent than where it belongs. Only
   *  meaningful for `by: "face"` — a block is classified on nothing. */
  classified?: ReadonlyMap<number, string>;
  /** Which quadrangles to draw at all, by their index in the view. Left
   *  out, every one of them is drawn; given, it is how a filter on the
   *  groups keeps the rest of the model off the screen. */
  only?: ReadonlySet<number>;
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
/**
 * The color of a cell by the groups it is in: the **lowest** group it
 * belongs to, since a cell can be in several and a pixel can only be one
 * color. A cell in no group at all gets none, and falls back to whatever
 * its group's own color is.
 *
 * The mask arrives in two halves because the browser has no `u64` worth
 * using; `lo` is bits 0 to 31 and `hi` the rest.
 */
export function groupColor(lo: number, hi: number): string | undefined {
  if (lo === 0 && hi === 0) return undefined;
  // `Math.log2` of the lowest set bit, which for a power of two is exact.
  const bit = lo !== 0 ? Math.log2(lo & -lo) : 32 + Math.log2(hi & -hi);
  return colorForId(bit, FAMILY.groups);
}

export function quadsToDraw(
  view: View,
  by: "face" | "block",
  internal: boolean,
  only?: ReadonlySet<number>,
): number[] {
  const count = view.quadFaces.length;
  const wanted = (q: number) => only === undefined || only.has(q);
  if (by === "block") {
    return Array.from({ length: count }, (_, q) => q).filter(wanted);
  }

  const sides = new Map<number, number>();
  for (const name of view.quadFaces) sides.set(name, (sides.get(name) ?? 0) + 1);

  const kept: number[] = [];
  const taken = new Set<number>();
  for (let q = 0; q < count; q++) {
    const name = view.quadFaces[q];
    if (taken.has(name)) continue;
    if (!wanted(q)) continue;
    if (!internal && (sides.get(name) ?? 0) > 1) continue;
    taken.add(name);
    kept.push(q);
  }
  return kept;
}

/** The center each quadrangle shrinks towards. */
function centers(view: View, by: "face" | "block"): Map<number, [number, number, number]> {
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
  // center, which is still exactly what a shrink wants.
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
  const kept = quadsToDraw(view, options.by, options.internal, options.only);
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
  return buildFrom(view, options, quadsToDraw(view, options.by, options.internal, options.only));
}

function buildFrom(view: View, options: SurfaceOptions, kept: number[]): SurfaceData {
  if (!kept.length) return EMPTY;

  const names = options.by === "face" ? view.quadFaces : view.quadBlocks;
  const dimension = options.by === "face" ? FAMILY.faces : FAMILY.blocks;
  const middle = centers(view, options.by);
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
    const classified =
      options.coloring === "class" && options.by === "face"
        ? options.classified?.get(view.quadFaces[q])
        : undefined;
    const grouped =
      options.coloring === "group"
        ? groupColor(view.quadGroupsLo[q], view.quadGroupsHi[q])
        : undefined;
    const rgb = picked
      ? hexToRgb(picked)
      : classified
        ? hexToRgb(classified)
        : grouped
          ? hexToRgb(grouped)
          : options.coloring === "id"
            ? hexToRgb(colorForId(name, dimension))
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

/**
 * The mesh's skin, as a surface and as a wireframe.
 *
 * Its own builder rather than `buildSurface`'s: a mesh has no selection, no
 * shrink and no classification — it is the answer rather than the thing
 * being worked on — and it has a hundred times as many quadrangles, so the
 * arithmetic per quadrangle is worth keeping to what it needs.
 *
 * Colored by the **block** each quadrangle came out of, so a hexahedron
 * wears the color of the block it was cut from and the two pictures agree.
 */
export function buildMesh(
  mesh: MeshView,
  options: { uniform: string; coloring: Coloring },
): SurfaceData {
  const n = mesh.blocks.length;
  if (!n) return EMPTY;
  const positions = new Float32Array(n * 12);
  const colors = new Float32Array(n * 16);
  const indices = new Uint32Array(n * 6);
  const uniform = hexToRgb(options.uniform);

  for (let q = 0; q < n; q++) {
    const rgb =
      options.coloring === "id"
        ? hexToRgb(colorForId(mesh.blocks[q], FAMILY.blocks))
        : uniform;
    for (let k = 0; k < 4; k++) {
      const node = mesh.quads[q * 4 + k];
      for (let axis = 0; axis < 3; axis++) {
        positions[(q * 4 + k) * 3 + axis] = mesh.positions[node * 3 + axis];
      }
      colors.set([rgb[0], rgb[1], rgb[2], 1], (q * 4 + k) * 4);
    }
    const base = q * 4;
    indices.set([base, base + 1, base + 2, base, base + 2, base + 3], q * 6);
  }
  return { positions, indices, colors, faces: new Uint32Array(n), blocks: mesh.blocks };
}

/**
 * Every edge of the skin, once.
 *
 * Deduplicated by the pair of nodes it joins: each quadrangle has four
 * sides and all but the ones round the rim are shared with a neighbor, so
 * drawing them per quadrangle would draw nearly every line twice.
 */
export function meshEdges(mesh: MeshView): Float32Array {
  const seen = new Set<number>();
  const out: number[] = [];
  const count = mesh.quads.length / 4;
  for (let q = 0; q < count; q++) {
    for (let k = 0; k < 4; k++) {
      const a = mesh.quads[q * 4 + k];
      const b = mesh.quads[q * 4 + ((k + 1) % 4)];
      // One number for the pair, smaller end first, so a side met from
      // either of its quadrangles is the same side.
      const key = a < b ? a * 4294967296 + b : b * 4294967296 + a;
      if (seen.has(key)) continue;
      seen.add(key);
      for (const node of [a, b]) {
        out.push(
          mesh.positions[node * 3],
          mesh.positions[node * 3 + 1],
          mesh.positions[node * 3 + 2],
        );
      }
    }
  }
  return new Float32Array(out);
}
