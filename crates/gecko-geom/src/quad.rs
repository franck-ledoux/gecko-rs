//! The quadrangle a surface wants to be blocked by.
//!
//! A surface of a boundary representation is bounded by curves meeting at
//! corners, and there are usually more than four of them. A structured
//! blocking of it is one quadrangle, so four of those corners have to be
//! chosen and the rest left to lie **inside the sides**: the boundary
//! between two chosen corners is a side, and a side is a run of curves.
//!
//! Which four? The ones the boundary turns a **right angle** at. A corner
//! where the boundary barely bends is not a corner of the quadrangle in any
//! useful sense — it is a point on the way along a side — and one that turns
//! through two right angles is a spike nobody wants a block corner on.
//!
//! The angle is measured the way everything else here is measured: on the
//! triangles. The interior angle of the surface at a node is the sum of the
//! angles its own triangles make there. No tangents are estimated and no
//! curve is differentiated, which matters because a faceted curve has no
//! derivative worth the name.

use std::collections::{HashMap, HashSet};

use crate::model::Geometry;

/// A right angle: what the four corners are chosen for being nearest to.
const RIGHT: f64 = std::f64::consts::FRAC_PI_2;

/// The quadrangle to lay on a surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    /// The four geometric corners, in boundary order.
    pub corners: [i32; 4],
    /// Where they are.
    pub points: [[f64; 3]; 4],
    /// The curves each side runs along, in boundary order: side `k` goes
    /// from corner `k` to corner `k + 1`. One curve usually, several when
    /// the side passes through corners that were not chosen.
    pub sides: [Vec<i32>; 4],
    /// The interior angle found at each chosen corner, in radians — for a
    /// caller that wants to say how square the answer really is.
    pub angles: [f64; 4],
}

/// Why a surface cannot be given a quadrangle.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QuadError {
    #[error("no surface is tagged {0}")]
    NoSuchSurface(i32),
    #[error("surface {0} has no boundary — it is closed, and a quadrangle needs four corners on an edge")]
    NoBoundary(i32),
    #[error("surface {0} has {1} corner(s) on its boundary, and a quadrangle needs four")]
    TooFewCorners(i32, usize),
    #[error("the boundary of surface {0} does not close into a loop")]
    BrokenBoundary(i32),
}

impl Geometry {
    /// The quadrangle to lay on this surface: four corners, and the curves
    /// of the four sides between them.
    ///
    /// The boundary is walked in the surface's **own** orientation — the
    /// winding gmsh wrote its triangles with — so the corners come back in
    /// an order a face can be built from directly, rather than in one that
    /// would fold it over itself.
    ///
    /// A surface with a hole has more than one boundary loop; the longest
    /// is taken and the others ignored, which is the honest reading of "lay
    /// one quadrangle on it".
    pub fn quad_on(&self, surface: i32) -> Result<Quad, QuadError> {
        let triangles = self
            .surfaces()
            .iter()
            .find(|s| s.tag == surface)
            .map(|s| s.triangles.as_slice())
            .ok_or(QuadError::NoSuchSurface(surface))?;

        let loops = boundary_loops(triangles).ok_or(QuadError::BrokenBoundary(surface))?;
        let ring = loops
            .into_iter()
            .max_by_key(|l| l.len())
            .ok_or(QuadError::NoBoundary(surface))?;

        // The corners that lie on this ring, by where they are along it.
        let named: HashMap<u32, i32> = self.corners().iter().map(|c| (c.node, c.tag)).collect();
        let angle = interior_angles(self.nodes(), triangles);
        let mut on_ring: Vec<(usize, u32, i32, f64)> = ring
            .iter()
            .enumerate()
            .filter_map(|(k, &node)| {
                named
                    .get(&node)
                    .map(|&tag| (k, node, tag, angle.get(&node).copied().unwrap_or(0.0)))
            })
            .collect();
        if on_ring.len() < 4 {
            return Err(QuadError::TooFewCorners(surface, on_ring.len()));
        }

        // The four nearest a right angle. Sorted by how far off they are,
        // and ties broken by the corner's own tag so that the same geometry
        // always gives the same quadrangle.
        on_ring.sort_by(|a, b| {
            (a.3 - RIGHT)
                .abs()
                .total_cmp(&(b.3 - RIGHT).abs())
                .then(a.2.cmp(&b.2))
        });
        let mut chosen: Vec<(usize, u32, i32, f64)> = on_ring.into_iter().take(4).collect();
        // Back into boundary order, or the quadrangle would be a bowtie.
        chosen.sort_by_key(|c| c.0);

        // Each side is the run of ring edges from one chosen corner to the
        // next, and the curves those edges belong to, in the order they are
        // met and without repeating one.
        let belongs = curve_of_edge(self);
        let mut sides: Vec<Vec<i32>> = Vec::with_capacity(4);
        for k in 0..4 {
            let (from, to) = (chosen[k].0, chosen[(k + 1) % 4].0);
            let mut curves: Vec<i32> = Vec::new();
            let mut at = from;
            while at != to {
                let edge = (ring[at], ring[(at + 1) % ring.len()]);
                if let Some(&curve) = belongs.get(&key(edge.0, edge.1)) {
                    if curves.last() != Some(&curve) && !curves.contains(&curve) {
                        curves.push(curve);
                    }
                }
                at = (at + 1) % ring.len();
            }
            sides.push(curves);
        }

        let corners = [chosen[0].2, chosen[1].2, chosen[2].2, chosen[3].2];
        let points = [0, 1, 2, 3].map(|k| self.nodes()[chosen[k].1 as usize]);
        let angles = [0, 1, 2, 3].map(|k| chosen[k].3);
        let sides: [Vec<i32>; 4] = sides.try_into().expect("four sides");
        Ok(Quad {
            corners,
            points,
            sides,
            angles,
        })
    }
}

/// An unordered node pair, for looking an edge up whichever way round it is
/// written.
fn key(a: u32, b: u32) -> (u32, u32) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Which curve each segment of the model belongs to.
fn curve_of_edge(geometry: &Geometry) -> HashMap<(u32, u32), i32> {
    let mut out = HashMap::new();
    for curve in geometry.curves() {
        for s in &curve.segments {
            out.insert(key(s[0], s[1]), curve.tag);
        }
    }
    out
}

/// The boundary of a set of triangles, as loops of nodes in the triangles'
/// own winding.
///
/// A directed edge of a triangle is on the boundary when the same edge is
/// not traversed the other way by any other triangle of the set. Chaining
/// those head to tail gives the loops, already oriented.
fn boundary_loops(triangles: &[[u32; 3]]) -> Option<Vec<Vec<u32>>> {
    let mut directed: HashSet<(u32, u32)> = HashSet::new();
    for t in triangles {
        for k in 0..3 {
            directed.insert((t[k], t[(k + 1) % 3]));
        }
    }
    let mut next: HashMap<u32, u32> = HashMap::new();
    for &(a, b) in &directed {
        if !directed.contains(&(b, a)) {
            // Two boundary edges leaving one node means the surface pinches
            // there, and no walk can say which way to go.
            if next.insert(a, b).is_some() {
                return None;
            }
        }
    }

    let mut loops = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        if seen.contains(&start) {
            continue;
        }
        let mut ring = vec![start];
        seen.insert(start);
        let mut at = start;
        while let Some(&on) = next.get(&at) {
            if on == start {
                break;
            }
            if !seen.insert(on) {
                return None;
            }
            ring.push(on);
            at = on;
        }
        loops.push(ring);
    }
    Some(loops)
}

/// The interior angle of a set of triangles at each of its nodes: the sum
/// of the angles the triangles make there.
fn interior_angles(nodes: &[[f64; 3]], triangles: &[[u32; 3]]) -> HashMap<u32, f64> {
    let mut out: HashMap<u32, f64> = HashMap::new();
    for t in triangles {
        for k in 0..3 {
            let (at, u, v) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            let (p, q, r) = (nodes[at as usize], nodes[u as usize], nodes[v as usize]);
            *out.entry(at).or_insert(0.0) += corner_angle(p, q, r);
        }
    }
    out
}

/// The angle at `p` in the triangle `p q r`.
fn corner_angle(p: [f64; 3], q: [f64; 3], r: [f64; 3]) -> f64 {
    let a = [0, 1, 2].map(|k| q[k] - p[k]);
    let b = [0, 1, 2].map(|k| r[k] - p[k]);
    let dot: f64 = (0..3).map(|k| a[k] * b[k]).sum();
    let na = (0..3).map(|k| a[k] * a[k]).sum::<f64>().sqrt();
    let nb = (0..3).map(|k| b[k] * b[k]).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / (na * nb)).clamp(-1.0, 1.0).acos()
}
