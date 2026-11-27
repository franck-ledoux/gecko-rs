//! How faithfully a mesh follows its geometry.
//!
//! Quality says the cells are well shaped; this says the mesh is the right
//! **shape** — that it lies on the part, and that the part lies on it. A
//! mesh can score perfectly by quality with a hole never cut, a notch left
//! filled, or a sharp edge rounded off, and none of that lowers a scaled
//! Jacobian.
//!
//! Three measurements, each against a tolerance that is a share of the
//! local element size — a gap of 0.02 is nothing on cells a quarter across
//! and a great deal on cells a hundredth across:
//!
//! 1. **The mesh to the geometry**: how far each node of the skin is from
//!    the nearest surface. Skin floating off the part — a face classified
//!    on nothing, a node stranded between two surfaces.
//! 2. **The geometry to the mesh**: how far each vertex and the middle of
//!    each triangle of the geometry is from the skin. The direction that
//!    catches what is **missing**: a feature the blocking never made leaves
//!    geometry far from any mesh.
//! 3. **Curves and corners**: whether every curve has mesh **edges** along
//!    it, not merely the skin passing near, and every corner a mesh node on
//!    it. What "aligned with the boundary" means.
//!
//! Everything is measured against the **smooth** curves and surfaces the
//! mesh is projected onto — the quadratic reconstruction under the facets —
//! not the facets themselves. A mesh put on that reconstruction lies on the
//! circle and cuts inside the corners of the polygon it was given as;
//! measured against the polygon, a rim followed exactly read as missed.
//!
//! A curve is a feature if the model still has it as a curve — itself, or a
//! merge of curves. One a merge of surfaces swallowed, the seam of a
//! cylinder, is not: the mesh has no reason to follow it. Corners the same.

use std::collections::{BTreeMap, HashMap};

use gecko_core::Mesh;
use gecko_geom::{Composites, Geometry};

/// The default tolerance, as a share of the local element size.
///
/// A quarter. A mesh that does follow a curve still cuts across it between
/// nodes, by about an eighth of the element size where the elements are as
/// long as the curve's radius — and further across the corners of a curve
/// the geometry gives as a polygon. A feature the mesh misses leaves gaps
/// the size of an element or more. A tenth flagged the circles of a part
/// meshed correctly.
pub const DEFAULT_SHARE: f64 = 0.25;

/// How many missed pieces are sent to be drawn, of each kind.
pub const MOST_MISSES_SHOWN: usize = 20_000;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// The distance from a point to a segment.
fn to_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(b, a);
    let dd = dot(d, d);
    let t = if dd > 0.0 {
        (dot(sub(p, a), d) / dd).clamp(0.0, 1.0)
    } else {
        0.0
    };
    length(sub(p, lerp(a, b, t)))
}

/// The distance from a point to a triangle: the closest point inside it,
/// or on one of its sides.
fn to_triangle(p: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    let va = d3 * d6 - d5 * d4;
    let vb = d5 * d2 - d1 * d6;
    let vc = d1 * d4 - d3 * d2;
    let sum = va + vb + vc;
    if va >= 0.0 && vb >= 0.0 && vc >= 0.0 && sum > 0.0 {
        let (v, w) = (vb / sum, vc / sum);
        let q = [0, 1, 2].map(|k| a[k] + ab[k] * v + ac[k] * w);
        return length(sub(p, q));
    }
    to_segment(p, a, b)
        .min(to_segment(p, b, c))
        .min(to_segment(p, c, a))
}

/// Things with a box round them, filed in a uniform grid so that the
/// nearest one to a point is found by looking in the cells round it, ring
/// by ring, rather than at every one of them.
struct Grid {
    lo: [f64; 3],
    cell: f64,
    dims: [i64; 3],
    buckets: HashMap<[i64; 3], Vec<usize>>,
}

impl Grid {
    fn new(boxes: &[([f64; 3], [f64; 3])], cell: f64) -> Grid {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for (a, b) in boxes {
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        if boxes.is_empty() {
            lo = [0.0; 3];
            hi = [0.0; 3];
        }
        // No more than 128 cells along any side: a grid finer than the
        // things in it is all empty cells to walk past.
        let span = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
        let cell = cell.max(span / 128.0).max(1e-12);
        let dims = [0, 1, 2].map(|k| ((hi[k] - lo[k]) / cell).floor() as i64 + 1);
        let mut grid = Grid {
            lo,
            cell,
            dims,
            buckets: HashMap::new(),
        };
        for (i, (a, b)) in boxes.iter().enumerate() {
            let (from, to) = (grid.index(*a), grid.index(*b));
            for x in from[0]..=to[0] {
                for y in from[1]..=to[1] {
                    for z in from[2]..=to[2] {
                        grid.buckets.entry([x, y, z]).or_default().push(i);
                    }
                }
            }
        }
        grid
    }

    fn index(&self, p: [f64; 3]) -> [i64; 3] {
        [0, 1, 2].map(|k| ((p[k] - self.lo[k]) / self.cell).floor() as i64)
    }

    /// The nearest thing to `p`, by `distance`, and how far it is.
    fn nearest(&self, p: [f64; 3], distance: impl Fn(usize) -> f64) -> Option<(f64, usize)> {
        let at = self.index(p);
        let mut best: Option<(f64, usize)> = None;
        // Far enough out that every cell of the grid has been seen.
        let reach = (0..3)
            .map(|k| at[k].abs().max((at[k] - self.dims[k]).abs()))
            .max()
            .unwrap_or(0);
        for ring in 0..=reach {
            for x in at[0] - ring..=at[0] + ring {
                for y in at[1] - ring..=at[1] + ring {
                    for z in at[2] - ring..=at[2] + ring {
                        let on_ring = (x - at[0]).abs() == ring
                            || (y - at[1]).abs() == ring
                            || (z - at[2]).abs() == ring;
                        if !on_ring {
                            continue;
                        }
                        let Some(found) = self.buckets.get(&[x, y, z]) else {
                            continue;
                        };
                        for &i in found {
                            let d = distance(i);
                            if best.is_none_or(|(b, j)| d < b || (d == b && i < j)) {
                                best = Some((d, i));
                            }
                        }
                    }
                }
            }
            // Anything in a further ring is at least this far away.
            if best.is_some_and(|(b, _)| b <= ring as f64 * self.cell) {
                break;
            }
        }
        best
    }
}

fn bounds(points: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// The worst miss of one kind: where, by how much, and how big the cells
/// are there.
#[derive(Clone, Copy)]
struct Miss {
    at: [f64; 3],
    distance: f64,
    size: f64,
}

impl Miss {
    /// How badly it misses: its distance over the tolerance there.
    fn ratio(&self, share: f64) -> f64 {
        self.distance / (share * self.size).max(1e-300)
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({ "at": self.at, "distance": self.distance, "size": self.size })
    }
}

/// Measure it all. `share` is the tolerance as a share of the local
/// element size.
pub fn measure(
    geometry: &Geometry,
    composites: &Composites,
    mesh: &Mesh,
    share: f64,
) -> serde_json::Value {
    let share = if share.is_finite() && share > 0.0 {
        share
    } else {
        DEFAULT_SHARE
    };
    let points = &mesh.points;
    let g = geometry.nodes();

    // --- the skin: its quadrangles, nodes and edges ----------------------
    let (quads, _) = mesh.skin_cells();
    let mut edges: BTreeMap<(u32, u32), ()> = BTreeMap::new();
    for q in &quads {
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            edges.insert((a.min(b), a.max(b)), ());
        }
    }
    let edges: Vec<(u32, u32)> = edges.into_keys().collect();
    // How big the cells are round each node of the skin: the mean of the
    // skin edges leaving it.
    let mut around: HashMap<u32, (f64, usize)> = HashMap::new();
    for &(a, b) in &edges {
        let l = length(sub(points[a as usize], points[b as usize]));
        for n in [a, b] {
            let e = around.entry(n).or_insert((0.0, 0));
            e.0 += l;
            e.1 += 1;
        }
    }
    let size_at = |n: u32| around.get(&n).map_or(0.0, |(s, c)| s / *c as f64);
    let mut skin_nodes: Vec<u32> = around.keys().copied().collect();
    skin_nodes.sort_unstable();
    let quad_size = |q: &[u32; 4]| q.iter().map(|n| size_at(*n)).sum::<f64>() / 4.0;
    let typical = if edges.is_empty() {
        1.0
    } else {
        edges
            .iter()
            .map(|(a, b)| length(sub(points[*a as usize], points[*b as usize])))
            .sum::<f64>()
            / edges.len() as f64
    };

    // --- 1. the skin to the geometry -------------------------------------
    let triangles: Vec<(i32, [u32; 3])> = geometry
        .surfaces()
        .iter()
        .flat_map(|s| s.triangles.iter().map(move |t| (s.tag, *t)))
        .collect();
    let tri_boxes: Vec<_> = triangles
        .iter()
        .map(|(_, t)| bounds(&t.map(|i| g[i as usize])))
        .collect();
    let tri_grid = Grid::new(&tri_boxes, 2.0 * typical);
    let mut skin_worst: Option<Miss> = None;
    let (mut skin_sum, mut skin_beyond) = (0.0, 0usize);
    for &n in &skin_nodes {
        let p = points[n as usize];
        // To the smooth surface under each triangle, where a node is put:
        // the nearest of those, not the smooth surface under the nearest
        // facet — a node of a disc beside its rim can be nearer the tube's
        // facet than the disc's, and is not on the tube.
        let Some((d, _)) = tri_grid.nearest(p, |i| {
            let (tag, t) = triangles[i];
            length(sub(p, geometry.surface_point_near(tag, t, p)))
        }) else {
            continue;
        };
        let miss = Miss {
            at: p,
            distance: d,
            size: size_at(n),
        };
        skin_sum += d;
        if miss.ratio(share) > 1.0 {
            skin_beyond += 1;
        }
        if skin_worst.is_none_or(|w| d > w.distance) {
            skin_worst = Some(miss);
        }
    }

    // --- 2. the geometry to the skin -------------------------------------
    let skin_tris: Vec<([u32; 3], f64)> = quads
        .iter()
        .flat_map(|q| {
            let size = quad_size(q);
            [([q[0], q[1], q[2]], size), ([q[0], q[2], q[3]], size)]
        })
        .collect();
    let skin_boxes: Vec<_> = skin_tris
        .iter()
        .map(|(t, _)| bounds(&t.map(|i| points[i as usize])))
        .collect();
    let skin_grid = Grid::new(&skin_boxes, 2.0 * typical);
    let to_skin = |p: [f64; 3]| -> Option<Miss> {
        skin_grid
            .nearest(p, |i| {
                let t = skin_tris[i].0;
                to_triangle(
                    p,
                    points[t[0] as usize],
                    points[t[1] as usize],
                    points[t[2] as usize],
                )
            })
            .map(|(d, i)| Miss {
                at: p,
                distance: d,
                size: skin_tris[i].1,
            })
    };
    let mut vertex_miss: HashMap<u32, Option<Miss>> = HashMap::new();
    let mut geom_worst: Option<Miss> = None;
    let (mut samples, mut geom_beyond) = (0usize, 0usize);
    let mut by_surface: BTreeMap<i32, (f64, usize)> = BTreeMap::new();
    let mut missed_triangles: Vec<[f64; 9]> = Vec::new();
    // Every sample off the mesh, drawn as a mark: a geometry faceted with
    // a few large triangles has none missed through and through, and
    // still has corners of them nowhere near the mesh.
    let mut missed_points: Vec<[f64; 3]> = Vec::new();
    for surface in geometry.surfaces() {
        let tag = composites.image(2, surface.tag).1;
        for t in &surface.triangles {
            let corners = t.map(|i| {
                *vertex_miss
                    .entry(i)
                    .or_insert_with(|| to_skin(g[i as usize]))
            });
            let (a, b, c) = (g[t[0] as usize], g[t[1] as usize], g[t[2] as usize]);
            // Samples on the smooth surface under the triangle, not on its
            // flat: that is where the mesh was put.
            let on = |w: [f64; 3]| geometry.surface_point(surface.tag, *t, w);
            let middle = on([1.0 / 3.0; 3]);
            let mut all = vec![corners[0], corners[1], corners[2], to_skin(middle)];
            // A triangle larger than the cells is looked at all over, on a
            // lattice about a cell apart: a geometry faceted coarsely has
            // triangles a feature of the mesh could hide inside.
            let longest = length(sub(a, b))
                .max(length(sub(b, c)))
                .max(length(sub(c, a)));
            let steps = (longest / typical).ceil().min(64.0) as usize;
            for i in 0..=steps {
                for j in 0..=steps - i {
                    let k = steps - i - j;
                    if steps < 2 || i == steps || j == steps || k == steps {
                        continue;
                    }
                    let (u, v, w) = (
                        i as f64 / steps as f64,
                        j as f64 / steps as f64,
                        k as f64 / steps as f64,
                    );
                    all.push(to_skin(on([u, v, w])));
                }
            }
            let mut out = 0;
            for m in all.iter().flatten() {
                samples += 1;
                if m.ratio(share) > 1.0 {
                    out += 1;
                    geom_beyond += 1;
                    if missed_points.len() < MOST_MISSES_SHOWN {
                        missed_points.push(m.at);
                    }
                    let e = by_surface.entry(tag).or_insert((0.0, 0));
                    e.0 = e.0.max(m.distance);
                    e.1 += 1;
                }
                if geom_worst.is_none_or(|w| m.ratio(share) > w.ratio(share)) {
                    geom_worst = Some(*m);
                }
            }
            // Drawn when it is missed through and through: every point of
            // it looked at off the mesh.
            if out == all.iter().flatten().count()
                && out > 0
                && missed_triangles.len() < MOST_MISSES_SHOWN
            {
                missed_triangles.push([a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]]);
            }
        }
    }

    // --- 3. curves and corners -------------------------------------------
    let edge_boxes: Vec<_> = edges
        .iter()
        .map(|(a, b)| bounds(&[points[*a as usize], points[*b as usize]]))
        .collect();
    let edge_grid = Grid::new(&edge_boxes, 2.0 * typical);
    // The nearest mesh edge running the curve's way: within 30 degrees of
    // its direction. Any edge at all would let a curve that crosses the
    // mesh's edges, rather than running along them, count as followed —
    // every point of it is near one.
    let aligned = 30f64.to_radians().cos();
    let to_edge = |p: [f64; 3], along: [f64; 3]| -> Option<Miss> {
        let along_length = length(along);
        edge_grid
            .nearest(p, |i| {
                let (a, b) = edges[i];
                let (pa, pb) = (points[a as usize], points[b as usize]);
                let e = sub(pb, pa);
                let cos = dot(e, along).abs() / (length(e) * along_length).max(1e-300);
                if cos < aligned {
                    return f64::INFINITY;
                }
                to_segment(p, pa, pb)
            })
            .filter(|(d, _)| d.is_finite())
            .map(|(d, i)| {
                let (a, b) = edges[i];
                Miss {
                    at: p,
                    distance: d,
                    size: length(sub(points[a as usize], points[b as usize])),
                }
            })
    };
    // Every feature curve, by what the model calls it now.
    let mut curves: BTreeMap<i32, (f64, usize, usize)> = BTreeMap::new();
    let mut missed_segments: Vec<[f64; 6]> = Vec::new();
    let mut curve_worst: Option<(i32, Miss)> = None;
    for curve in geometry.curves() {
        let (dim, tag) = composites.image(1, curve.tag);
        if dim != 1 {
            continue;
        }
        for s in &curve.segments {
            let (a, b) = (g[s[0] as usize], g[s[1] as usize]);
            let mut off = false;
            // About half a cell apart: a segment longer than the cells could
            // otherwise wander between the mesh's edges unseen.
            let pieces = (length(sub(a, b)) / (0.5 * typical))
                .ceil()
                .clamp(2.0, 256.0) as usize;
            // Along the smooth curve under the segment, where the mesh was
            // put; the segment's direction still says which way it runs.
            for p in
                (0..=pieces).map(|i| geometry.curve_point(curve.tag, *s, i as f64 / pieces as f64))
            {
                let Some(m) = to_edge(p, sub(b, a)) else {
                    // No mesh edge anywhere runs this way.
                    let e = curves.entry(tag).or_insert((f64::INFINITY, 0, 0));
                    e.0 = f64::INFINITY;
                    e.1 += 1;
                    e.2 += 1;
                    off = true;
                    continue;
                };
                let e = curves.entry(tag).or_insert((0.0, 0, 0));
                e.0 = e.0.max(m.distance);
                e.2 += 1;
                if m.ratio(share) > 1.0 {
                    e.1 += 1;
                    off = true;
                }
                if curve_worst.is_none_or(|(_, w)| m.ratio(share) > w.ratio(share)) {
                    curve_worst = Some((tag, m));
                }
            }
            if off && missed_segments.len() < MOST_MISSES_SHOWN {
                missed_segments.push([a[0], a[1], a[2], b[0], b[1], b[2]]);
            }
        }
    }
    let node_boxes: Vec<_> = skin_nodes
        .iter()
        .map(|n| (points[*n as usize], points[*n as usize]))
        .collect();
    let node_grid = Grid::new(&node_boxes, 2.0 * typical);
    let mut corners_checked = 0usize;
    let mut missed_corners = Vec::new();
    let mut corner_worst: Option<(i32, Miss)> = None;
    for corner in geometry.corners() {
        if composites.image(0, corner.tag).0 != 0 {
            continue;
        }
        corners_checked += 1;
        let p = g[corner.node as usize];
        let Some((d, i)) = node_grid.nearest(p, |i| length(sub(p, points[skin_nodes[i] as usize])))
        else {
            continue;
        };
        let m = Miss {
            at: p,
            distance: d,
            size: size_at(skin_nodes[i]),
        };
        if m.ratio(share) > 1.0 {
            missed_corners.push(serde_json::json!({
                "tag": corner.tag, "at": p, "distance": d,
            }));
        }
        if corner_worst.is_none_or(|(_, w)| m.ratio(share) > w.ratio(share)) {
            corner_worst = Some((corner.tag, m));
        }
    }

    // --- the worst of it, wherever it is ---------------------------------
    let candidates = [
        skin_worst.map(|m| ("the mesh off the geometry".to_string(), m)),
        geom_worst.map(|m| ("geometry off the mesh".to_string(), m)),
        curve_worst.map(|(t, m)| (format!("curve {t}"), m)),
        corner_worst.map(|(t, m)| (format!("corner {t}"), m)),
    ];
    let worst = candidates
        .into_iter()
        .flatten()
        .filter(|(_, m)| m.ratio(share) > 1.0)
        .max_by(|a, b| a.1.ratio(share).total_cmp(&b.1.ratio(share)));

    let missed_curves: Vec<serde_json::Value> = curves
        .iter()
        .filter(|(_, (_, off, _))| *off > 0)
        .map(|(tag, (max, off, of))| {
            serde_json::json!({ "tag": tag, "max": max, "beyond": off, "samples": of })
        })
        .collect();
    serde_json::json!({
        "share": share,
        "skin": {
            "nodes": skin_nodes.len(),
            "max": skin_worst.map_or(0.0, |m| m.distance),
            "mean": if skin_nodes.is_empty() { 0.0 } else { skin_sum / skin_nodes.len() as f64 },
            "beyond": skin_beyond,
            "worst": skin_worst.map(|m| m.json()),
        },
        "geometry": {
            "samples": samples,
            "max": geom_worst.map_or(0.0, |m| m.distance),
            "beyond": geom_beyond,
            "worst": geom_worst.map(|m| m.json()),
            "surfaces": by_surface
                .iter()
                .map(|(tag, (max, n))| serde_json::json!({ "tag": tag, "max": max, "beyond": n }))
                .collect::<Vec<_>>(),
        },
        "curves": {
            "checked": curves.len(),
            "followed": curves.len() - missed_curves.len(),
            "missed": missed_curves,
        },
        "corners": {
            "checked": corners_checked,
            "missed": missed_corners,
        },
        "worst": worst.map(|(what, m)| {
            let mut j = m.json();
            j["what"] = serde_json::json!(what);
            j
        }),
        "overlay": {
            "segments": missed_segments,
            "triangles": missed_triangles,
            "points": missed_points,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_to_a_segment_and_a_triangle() {
        let (a, b, c) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!((to_segment([0.5, 2.0, 0.0], a, b) - 2.0).abs() < 1e-12);
        assert!((to_segment([-3.0, 4.0, 0.0], a, b) - 5.0).abs() < 1e-12);
        assert!((to_triangle([0.2, 0.2, 3.0], a, b, c) - 3.0).abs() < 1e-12);
        assert!((to_triangle([2.0, 0.0, 0.0], a, b, c) - 1.0).abs() < 1e-12);
        assert!((to_triangle([-1.0, -1.0, 0.0], a, b, c) - 2f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn the_grid_finds_the_same_nearest_thing_as_looking_at_everything() {
        // Points on a twisted line, and probes all round them.
        let points: Vec<[f64; 3]> = (0..200)
            .map(|i| {
                let t = i as f64 * 0.1;
                [t.cos() * 3.0, t.sin() * 3.0, t * 0.5]
            })
            .collect();
        let boxes: Vec<_> = points.iter().map(|p| (*p, *p)).collect();
        let grid = Grid::new(&boxes, 0.3);
        for k in 0..50 {
            let s = k as f64 * 0.37;
            let probe = [s.sin() * 5.0, s.cos() * 4.0, s - 2.0];
            let d = |i: usize| length(sub(probe, points[i]));
            let brute = (0..points.len())
                .map(|i| (d(i), i))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            let found = grid.nearest(probe, d).unwrap();
            assert_eq!(found.1, brute.1, "probe {probe:?}");
        }
    }
}
