/**
 * Turning the geometry buffer into something a scene can hold.
 *
 * The counterpart of `surface.ts`, and pure arithmetic for the same reason:
 * what is worth testing is which triangles end up in which mesh and where
 * their corners are, and none of that needs a GPU.
 *
 * **One mesh per geometric entity.** A surface is one CAD face however many
 * triangles it was faceted into, and one mesh per entity is what lets it be
 * coloured, faded and hidden as the one thing it is. It also gives Babylon's
 * back-to-front sorter something to sort, which is what a translucent model
 * needs — the same reason the blocking draws a mesh per block.
 */

import type { GeometryView } from "./decode";

/** The three groups a geometry is drawn in. */
export type GeomGroup = "corners" | "curves" | "surfaces";

/** Listed by falling dimension, which is the order the drawer shows them in
 *  and the order their keys run: `4` surfaces, `5` curves, `6` corners. */
export const GEOM_GROUPS: readonly GeomGroup[] = ["surfaces", "curves", "corners"];

/** One geometric entity, named the way the kernel names one: a dimension
 *  and the gmsh entity tag. The same pair a classification carries. */
export interface Spot {
  dim: number;
  tag: number;
}

export const CORNER = 0;
export const CURVE = 1;
export const SURFACE = 2;

/** A group's dimension, and the other way round. Two tables rather than
 *  arithmetic on the group's position in `GEOM_GROUPS`, which is in falling
 *  order and would read as a subtraction nobody could check at a glance. */
export const DIM_OF: Record<GeomGroup, number> = {
  corners: CORNER,
  curves: CURVE,
  surfaces: SURFACE,
};
export const GROUP_OF: readonly GeomGroup[] = ["corners", "curves", "surfaces"];

/** How an entity reads in a message, the same way the kernel says it. */
export function spotName(on: Spot): string {
  return `${GROUP_OF[on.dim]?.replace(/s$/, "") ?? "?"} ${on.tag}`;
}

export interface GeomSurface {
  /** The gmsh entity tag — the surface's name, and what a colour-by-id uses. */
  tag: number;
  /** Three per vertex. */
  positions: Float32Array;
  /** Three per triangle. */
  indices: Uint32Array;
}

export interface GeomCurve {
  tag: number;
  /** Six numbers per segment: both ends. */
  segments: Float32Array;
}

export interface GeomCorner {
  tag: number;
  at: [number, number, number];
}

/**
 * One mesh's worth of triangles per surface tag.
 *
 * Vertices are **shared** inside a surface, unlike the blocking's, where
 * every corner is a vertex of its own because each carries its own colour.
 * Here a surface is one colour throughout, so sharing costs nothing — and
 * it buys the right shading: normals averaged across a surface's own
 * triangles make a faceted sphere look like a sphere, while nothing is
 * shared *between* surfaces, so the edge where two meet stays a crease.
 */
export function geomSurfaces(view: GeometryView): GeomSurface[] {
  const byTag = new Map<number, number[]>();
  for (let t = 0; t < view.triangleTags.length; t++) {
    const tag = view.triangleTags[t];
    const already = byTag.get(tag);
    if (already) already.push(t);
    else byTag.set(tag, [t]);
  }

  return [...byTag.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([tag, triangles]) => {
      // The nodes this surface actually uses, renumbered from nothing: a
      // mesh holding the whole model's positions to draw a tenth of them is
      // a buffer nine tenths wasted, once per surface.
      const local = new Map<number, number>();
      const positions: number[] = [];
      const indices = new Uint32Array(triangles.length * 3);
      triangles.forEach((t, i) => {
        for (let k = 0; k < 3; k++) {
          const node = view.triangles[t * 3 + k];
          let to = local.get(node);
          if (to === undefined) {
            to = positions.length / 3;
            local.set(node, to);
            positions.push(
              view.positions[node * 3],
              view.positions[node * 3 + 1],
              view.positions[node * 3 + 2],
            );
          }
          indices[i * 3 + k] = to;
        }
      });
      return { tag, positions: new Float32Array(positions), indices };
    });
}

/** The segments of each curve, six numbers apiece. */
export function geomCurves(view: GeometryView): GeomCurve[] {
  const byTag = new Map<number, number[]>();
  for (let s = 0; s < view.segmentTags.length; s++) {
    const tag = view.segmentTags[s];
    const already = byTag.get(tag);
    if (already) already.push(s);
    else byTag.set(tag, [s]);
  }

  return [...byTag.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([tag, found]) => {
      const segments = new Float32Array(found.length * 6);
      found.forEach((s, i) => {
        for (const [end, node] of [view.segments[s * 2], view.segments[s * 2 + 1]].entries()) {
          for (let axis = 0; axis < 3; axis++) {
            segments[i * 6 + end * 3 + axis] = view.positions[node * 3 + axis];
          }
        }
      });
      return { tag, segments };
    });
}

export function geomCorners(view: GeometryView): GeomCorner[] {
  return [...view.cornerTags]
    .map((tag, i) => {
      const node = view.corners[i];
      return {
        tag,
        at: [
          view.positions[node * 3],
          view.positions[node * 3 + 1],
          view.positions[node * 3 + 2],
        ] as [number, number, number],
      };
    })
    .sort((a, b) => a.tag - b.tag);
}

/**
 * The geometry's extent, or nothing when there is none.
 *
 * Over the nodes it actually draws rather than over every node of the file,
 * so a corner left behind by a skipped tetrahedron cannot stretch the box —
 * though the kernel has already dropped those, which is belt and braces.
 */
export function geomBounds(
  view: GeometryView,
): { lo: [number, number, number]; hi: [number, number, number] } | null {
  if (!view.positions.length) return null;
  const lo: [number, number, number] = [Infinity, Infinity, Infinity];
  const hi: [number, number, number] = [-Infinity, -Infinity, -Infinity];
  for (let n = 0; n < view.positions.length; n += 3) {
    for (let axis = 0; axis < 3; axis++) {
      lo[axis] = Math.min(lo[axis], view.positions[n + axis]);
      hi[axis] = Math.max(hi[axis], view.positions[n + axis]);
    }
  }
  return { lo, hi };
}
