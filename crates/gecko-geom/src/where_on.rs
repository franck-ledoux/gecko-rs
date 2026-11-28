//! Where a point is on the geometry, and what contains what.
//!
//! Two questions a block structure asks of a geometry once its cells are
//! classified onto it:
//!
//! - **where** — the nearest entity to a point, and the point on that entity
//!   nearest to it. That is how a node is classified in the first place, and
//!   how it is kept on its entity afterwards.
//! - **what contains what** — which curves pass through a corner, which
//!   surfaces a curve bounds. That is how an *edge* is classified, from its
//!   two ends rather than from any measurement of its own.
//!
//! The containment is recovered from **shared mesh nodes**, not from
//! proximity: a corner is on a curve when its own node is one of the curve's,
//! a curve is on a surface when every one of its nodes is one of the
//! surface's. Integer set membership — no tolerance, nothing ambiguous. It is
//! what `FacetedGeometry::containing_entities` does in the C++, and for the
//! reason given there: `$Elements` states nowhere which curve bounds which
//! surface, so it has to be recovered.
//!
//! The assumption is that the faceting is **conformal** — a curve's nodes
//! are the *same node numbers* as its bounding surface's. True of any `.msh`
//! gmsh wrote from one CAD model; false of one assembled from parts meshed
//! separately, where this reports fewer containments and never wrong ones.

use crate::model::Geometry;
use std::collections::{BTreeMap, BTreeSet};

/// A geometric corner. The dimension a cell classified on one carries.
pub const CORNER: u8 = 0;
/// A geometric curve.
pub const CURVE: u8 = 1;
/// A geometric surface.
pub const SURFACE: u8 = 2;

/// Everything containing an entity, itself included, lowest dimension first.
///
/// Built once when the geometry is read, because every classification of an
/// edge intersects two of these.
pub(crate) fn containment(geometry: &Geometry) -> BTreeMap<(u8, i32), Vec<(u8, i32)>> {
    let curve_nodes: Vec<(i32, BTreeSet<u32>)> = geometry
        .curves()
        .iter()
        .map(|c| (c.tag, c.segments.iter().flatten().copied().collect()))
        .collect();
    let surface_nodes: Vec<(i32, BTreeSet<u32>)> = geometry
        .surfaces()
        .iter()
        .map(|s| (s.tag, s.triangles.iter().flatten().copied().collect()))
        .collect();
    // The sides of each surface's triangles, each as its two nodes in order.
    let side = |a: u32, b: u32| if a < b { [a, b] } else { [b, a] };
    let surface_sides: BTreeMap<i32, BTreeSet<[u32; 2]>> = geometry
        .surfaces()
        .iter()
        .map(|s| {
            let sides = s
                .triangles
                .iter()
                .flat_map(|t| [side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0])])
                .collect();
            (s.tag, sides)
        })
        .collect();

    let mut out: BTreeMap<(u8, i32), Vec<(u8, i32)>> = BTreeMap::new();
    // Every entity contains itself, so intersecting two up-sets never comes
    // back empty merely because both cells are on the same thing.
    // A curve is on a surface when every one of its segments is a side of
    // one of the surface's triangles: running along it, not merely touching
    // it at its ends — a straight curve of one segment has only its two end
    // nodes, and a surface meeting it at both corners has them too.
    for curve in geometry.curves() {
        let mut up = vec![(CURVE, curve.tag)];
        for (surface, sides) in &surface_sides {
            if curve
                .segments
                .iter()
                .all(|s| sides.contains(&side(s[0], s[1])))
            {
                up.push((SURFACE, *surface));
            }
        }
        out.insert((CURVE, curve.tag), up);
    }
    for corner in geometry.corners() {
        let mut up = vec![(CORNER, corner.tag)];
        for (tag, nodes) in &curve_nodes {
            if nodes.contains(&corner.node) {
                up.push((CURVE, *tag));
            }
        }
        for (tag, nodes) in &surface_nodes {
            if nodes.contains(&corner.node) {
                up.push((SURFACE, *tag));
            }
        }
        out.insert((CORNER, corner.tag), up);
    }
    for (tag, _) in &surface_nodes {
        out.insert((SURFACE, *tag), vec![(SURFACE, *tag)]);
    }
    out
}

impl Geometry {
    /// How big the geometry is, corner to corner.
    ///
    /// What a tolerance is a share of: "within a hundredth" has to mean a
    /// hundredth of something, and a figure in millimeters would be wrong
    /// for every model but the one it was chosen on.
    pub fn span(&self) -> f64 {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in self.nodes() {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if self.nodes().is_empty() {
            return 0.0;
        }
        (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt()
    }

    /// Everything containing `(dim, tag)`, itself first, or nothing when no
    /// entity answers to that pair.
    pub fn containing(&self, dim: u8, tag: i32) -> &[(u8, i32)] {
        self.containment()
            .get(&(dim, tag))
            .map(|up| up.as_slice())
            .unwrap_or(&[])
    }

    /// Everything containing **every** one of a set of classifications.
    ///
    /// What a cell's boundary can tell you about where the cell is: not
    /// which entity it is on, but which entities it *could* be on. Often
    /// several, and the difference matters — the rim of a disc is on the
    /// disc and on the cylinder it bounds, so an edge with both ends on
    /// that rim could be the rim itself, or a chord across the disc, or a
    /// line on the cylinder, and no amount of looking at its ends will
    /// say which.
    ///
    /// Sorted by dimension and then tag.
    pub fn shared(&self, boundary: &[(u8, i32)]) -> Vec<(u8, i32)> {
        let Some((first, rest)) = boundary.split_first() else {
            return Vec::new();
        };
        let mut common: BTreeSet<(u8, i32)> =
            self.containing(first.0, first.1).iter().copied().collect();
        for other in rest {
            let theirs: BTreeSet<(u8, i32)> =
                self.containing(other.0, other.1).iter().copied().collect();
            common.retain(|e| theirs.contains(e));
            if common.is_empty() {
                break;
            }
        }
        common.into_iter().collect()
    }

    /// The lowest-dimensional entity containing **every** one of a set of
    /// classifications — what a cell bounded by them lies on.
    ///
    /// Topological throughout: no point of the cell is ever measured. An
    /// edge whose two corners sit on one curve belongs on that curve; one
    /// whose corners sit on two curves bounding a common surface belongs
    /// on that surface; a face whose four edges all lie on one surface
    /// belongs on that surface. A cell whose boundary has nothing in
    /// common belongs nowhere, and says so.
    ///
    /// `at_least` is the cell's own dimension — an edge is never pinned to
    /// a single point, nor a face to a curve, which would over-constrain
    /// them.
    pub fn common_of(&self, boundary: &[(u8, i32)], at_least: u8) -> Option<(u8, i32)> {
        let (first, rest) = boundary.split_first()?;
        let mut common: BTreeSet<(u8, i32)> =
            self.containing(first.0, first.1).iter().copied().collect();
        for other in rest {
            let theirs: BTreeSet<(u8, i32)> =
                self.containing(other.0, other.1).iter().copied().collect();
            common.retain(|e| theirs.contains(e));
            if common.is_empty() {
                return None;
            }
        }
        // The set is ordered by (dimension, tag), so the first entry at or
        // above the cell's own dimension is already the lowest.
        common.into_iter().find(|e| e.0 >= at_least)
    }

    /// [`common_of`](Self::common_of) for the two ends of an edge.
    pub fn common(&self, a: (u8, i32), b: (u8, i32), at_least: u8) -> Option<(u8, i32)> {
        self.common_of(&[a, b], at_least)
    }

    /// The most constrained entity that is on **both** — the other direction
    /// from [`common`](Self::common).
    ///
    /// What **welding two classified nodes into one** needs, and it is the
    /// opposite question. `common` asks what contains both, because an edge
    /// between two corners lies on a surface they share. A welded node is a
    /// single point that has to satisfy both statements at once, and a point
    /// on a curve is already on every surface that curve bounds — so the
    /// curve is the statement to keep, not the surface.
    ///
    /// Three cases, in order:
    ///
    /// * one **contains** the other — keep the contained one, which is the
    ///   stronger claim: a node on a circle glued to a node on the cylinder
    ///   it bounds is on the circle. An entity contains itself, so the two
    ///   being the same thing needs no case of its own — and it does need
    ///   answering before the search below, which would tie between a
    ///   surface and its own boundary for a point outside it.
    /// * they **meet** — keep what they share: two surfaces welded along
    ///   their seam give the curve of the seam.
    /// * they share **nothing**, and there is no answer. Said so, rather
    ///   than guessed at.
    ///
    /// Of the entities they share, the one **nearest `near`** wins, and a tie
    /// goes to the lower dimension. Two surfaces can meet along more than one
    /// curve — two halves of a cylinder meet along both of its sides — and
    /// nothing but the position can say which one this weld is on. A curve
    /// always runs at least as close as its own end corners, so the corner
    /// only wins when the weld really is at the end of the curve, where it is
    /// the better answer.
    pub fn meet(&self, a: (u8, i32), b: (u8, i32), near: [f64; 3]) -> Option<(u8, i32)> {
        if self.containing(a.0, a.1).contains(&b) {
            return Some(a);
        }
        if self.containing(b.0, b.1).contains(&a) {
            return Some(b);
        }
        self.containment()
            .keys()
            .filter(|&&e| {
                let up = self.containing(e.0, e.1);
                up.contains(&a) && up.contains(&b)
            })
            .filter_map(|&e| self.distance(e.0, e.1, near).map(|d| (d, e)))
            .min_by(|(d, e), (f, g)| d.total_cmp(f).then(e.0.cmp(&g.0)))
            .map(|(_, e)| e)
    }

    /// Where `p` lands on an entity, or nothing when no entity answers to
    /// that pair: the nearest point of the facets, lifted onto the smooth
    /// curve or surface reconstructed under them — see
    /// [`crate::reconstruction`]. A corner is its own node.
    pub fn project(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<[f64; 3]> {
        match dim {
            CORNER => self
                .corners()
                .iter()
                .find(|c| c.tag == tag)
                .map(|c| self.nodes()[c.node as usize]),
            CURVE => self.curves().iter().find(|c| c.tag == tag).map(|curve| {
                let nodes = self.nodes();
                let best = curve
                    .segments
                    .iter()
                    .map(|s| {
                        let (q, u) = on_segment(p, nodes[s[0] as usize], nodes[s[1] as usize]);
                        (apart(q, p), q, u, *s)
                    })
                    .min_by(|a, b| a.0.partial_cmp(&b.0).expect("a finite distance"));
                match best {
                    Some((_, q, u, s)) => self.fits().lift_curve(nodes, tag, s, u, q),
                    None => p,
                }
            }),
            SURFACE => self
                .surfaces()
                .iter()
                .find(|s| s.tag == tag)
                .map(|surface| {
                    let nodes = self.nodes();
                    let best = surface
                        .triangles
                        .iter()
                        .map(|t| {
                            let corners = t.map(|n| nodes[n as usize]);
                            let q = on_triangle(p, corners);
                            (apart(q, p), q, *t, corners)
                        })
                        .min_by(|a, b| a.0.partial_cmp(&b.0).expect("a finite distance"));
                    match best {
                        Some((_, q, t, corners)) => match barycentric(q, corners) {
                            Some(w) => self.fits().lift_surface(nodes, tag, t, w, q),
                            None => q,
                        },
                        None => p,
                    }
                }),
            _ => None,
        }
    }

    /// How far `p` is from an entity.
    pub fn distance(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<f64> {
        self.project(dim, tag, p).map(|q| apart(p, q))
    }

    /// The point of the smooth curve reconstructed under segment `s` of
    /// curve `tag`, a share `u` of the way from its first node: where a
    /// point of that segment is put, so what to measure a mesh on the curve
    /// against. The segment's own point on a straight curve.
    pub fn curve_point(&self, tag: i32, s: [u32; 2], u: f64) -> [f64; 3] {
        let nodes = self.nodes();
        let (a, b) = (nodes[s[0] as usize], nodes[s[1] as usize]);
        let q = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * u);
        self.fits().lift_curve(nodes, tag, s, u, q)
    }

    /// The point of the smooth surface reconstructed under triangle `t` of
    /// surface `tag`, at barycentric coordinates `w`.
    pub fn surface_point(&self, tag: i32, t: [u32; 3], w: [f64; 3]) -> [f64; 3] {
        let nodes = self.nodes();
        let corners = t.map(|n| nodes[n as usize]);
        let q =
            [0, 1, 2].map(|k| w[0] * corners[0][k] + w[1] * corners[1][k] + w[2] * corners[2][k]);
        self.fits().lift_surface(nodes, tag, t, w, q)
    }

    /// Where `p` lands on the smooth surface under triangle `t` of surface
    /// `tag`, as [`project`](Self::project) would put it there: the
    /// nearest point of the triangle, lifted.
    pub fn surface_point_near(&self, tag: i32, t: [u32; 3], p: [f64; 3]) -> [f64; 3] {
        let nodes = self.nodes();
        let corners = t.map(|n| nodes[n as usize]);
        let q = on_triangle(p, corners);
        match barycentric(q, corners) {
            Some(w) => self.fits().lift_surface(nodes, tag, t, w, q),
            None => q,
        }
    }

    /// The entity to classify a point on, or nothing when it is near none.
    ///
    /// Dimension by dimension, stopping at the **lowest** one with anything
    /// within its tolerance — a point near a corner is on the corner, not on
    /// the three curves through it. `tolerance` is one figure per dimension,
    /// corners first; the corner one is usually the tightest, since two
    /// corners are typically far closer to each other than either is to a
    /// curve it does not touch.
    pub fn nearest(&self, p: [f64; 3], tolerance: [f64; 3]) -> Option<(u8, i32)> {
        for dim in [CORNER, CURVE, SURFACE] {
            let tags: Vec<i32> = match dim {
                CORNER => self.corners().iter().map(|c| c.tag).collect(),
                CURVE => self.curves().iter().map(|c| c.tag).collect(),
                _ => self.surfaces().iter().map(|s| s.tag).collect(),
            };
            let best = tags
                .into_iter()
                .filter_map(|tag| self.distance(dim, tag, p).map(|d| (d, tag)))
                .filter(|(d, _)| *d <= tolerance[dim as usize])
                .min_by(|a, b| a.0.partial_cmp(&b.0).expect("a finite distance"));
            if let Some((_, tag)) = best {
                return Some((dim, tag));
            }
        }
        None
    }
}

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn along(from: [f64; 3], direction: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|k| from[k] + direction[k] * t)
}

/// The point of a segment nearest `p`, ends included, and how far along
/// the segment it is, from nought at `a` to one at `b`.
fn on_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> ([f64; 3], f64) {
    let ab = minus(b, a);
    let length = dot(ab, ab);
    if length <= 0.0 {
        return (a, 0.0);
    }
    let u = (dot(minus(p, a), ab) / length).clamp(0.0, 1.0);
    (along(a, ab, u), u)
}

/// The barycentric coordinates of a point of a triangle's plane, or nothing
/// for a triangle with no area.
fn barycentric(q: [f64; 3], t: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let (v0, v1, v2) = (minus(t[1], t[0]), minus(t[2], t[0]), minus(q, t[0]));
    let (d00, d01, d11) = (dot(v0, v0), dot(v0, v1), dot(v1, v1));
    let (d20, d21) = (dot(v2, v0), dot(v2, v1));
    let den = d00 * d11 - d01 * d01;
    if den <= 1e-300 * (d00 * d11).max(1e-300) {
        return None;
    }
    let v = (d11 * d20 - d01 * d21) / den;
    let w = (d00 * d21 - d01 * d20) / den;
    Some([1.0 - v - w, v, w])
}

/// The point of a triangle nearest `p`.
///
/// The usual seven-region split — inside the face, past one of the three
/// corners, or alongside one of the three edges — which gets the answer in
/// one pass of comparisons rather than by projecting onto the plane and
/// hoping the foot lands inside.
fn on_triangle(p: [f64; 3], t: [[f64; 3]; 3]) -> [f64; 3] {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (minus(b, a), minus(c, a), minus(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = minus(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return along(a, ab, d1 / (d1 - d3));
    }

    let cp = minus(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return along(a, ac, d2 / (d2 - d6));
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return along(b, minus(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }

    let denominator = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denominator, vc * denominator);
    [0, 1, 2].map(|k| a[k] + ab[k] * v + ac[k] * w)
}
