//! A smooth surface and smooth curves under the facets, for projecting onto.
//!
//! A faceted geometry is a polygon where the part is a circle, and a
//! polyhedron where it is a cylinder. A node kept on the facets is kept on
//! those flats and kinks: its distance from the axis rises and falls as it
//! slides round, and a smoothing reads that as a row of shallow dips to
//! settle in — which one depends on where it started, not on the part. A
//! symmetric part, triangulated a little unevenly, came out of smoothing
//! visibly lopsided.
//!
//! So a point is projected onto a **local quadratic reconstruction** of the
//! facets, in the spirit of Frey's surface simplification within a Hausdorff
//! envelope, which fits a quadric at every vertex of the triangulation:
//!
//! - at every node of a **curve**, the quadratic through it and its two
//!   neighbours along the curve, by chord length — or, at an end, through it
//!   and the next two — which gives the node a tangent and a curvature;
//! - at every node of a **surface**, a normal from the triangles round it,
//!   and the height `z = a x² + b x y + c y²` over its tangent plane that
//!   best fits its neighbours, least squares.
//!
//! A point is projected onto the facets as before, and the foot is then
//! **lifted**: onto the quadratic of each end of its segment, or each corner
//! of its triangle, and the lifted points blended by where the foot is
//! between them. At a node of the faceting that is the node itself; on a
//! flat face or a straight curve it is the foot unchanged. The lift is not
//! the nearest point of the reconstruction, only a point of it near the
//! foot — what keeps a node on its entity, smoothly, rather than what
//! measures a distance to the last digit.
//!
//! Each entity is fitted on its own triangles and segments only: a crease
//! between two surfaces stays a crease.
//!
//! Nothing but the four operations and square roots, like the smoothing it
//! serves: the page and the terminal have to land on the same bytes.

use std::collections::{BTreeMap, BTreeSet};

/// A curve near one of its nodes: the unit tangent, and the curvature
/// vector square to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CurveFit {
    tangent: [f64; 3],
    curvature: [f64; 3],
}

/// A surface near one of its nodes: a frame whose third axis is the normal,
/// and the height over the tangent plane, `a x² + b x y + c y²`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceFit {
    frame: [[f64; 3]; 3],
    height: [f64; 3],
}

/// The fits of every entity, by tag and then by node.
#[derive(Clone, Debug, Default)]
pub(crate) struct Fits {
    curves: BTreeMap<i32, BTreeMap<u32, CurveFit>>,
    surfaces: BTreeMap<i32, BTreeMap<u32, SurfaceFit>>,
}

impl Fits {
    pub(crate) fn of(
        nodes: &[[f64; 3]],
        curves: &[crate::Curve],
        surfaces: &[crate::Surface],
    ) -> Fits {
        Fits {
            curves: curves
                .iter()
                .map(|c| (c.tag, curve_fits(nodes, &c.segments)))
                .collect(),
            surfaces: surfaces
                .iter()
                .map(|s| (s.tag, surface_fits(nodes, &s.triangles)))
                .collect(),
        }
    }

    /// `q`, a point of the segment from node `a` to node `b` of curve
    /// `tag`, a share `u` of the way along it, lifted onto the curve.
    pub(crate) fn lift_curve(
        &self,
        nodes: &[[f64; 3]],
        tag: i32,
        [a, b]: [u32; 2],
        u: f64,
        q: [f64; 3],
    ) -> [f64; 3] {
        let Some(fits) = self.curves.get(&tag) else {
            return q;
        };
        let lift = |n: u32| match fits.get(&n) {
            Some(f) => {
                let at = nodes[n as usize];
                let s = dot(minus(q, at), f.tangent);
                [0, 1, 2].map(|k| at[k] + s * f.tangent[k] + 0.5 * s * s * f.curvature[k])
            }
            None => q,
        };
        let (la, lb) = (lift(a), lift(b));
        [0, 1, 2].map(|k| (1.0 - u) * la[k] + u * lb[k])
    }

    /// `q`, a point of triangle `t` of surface `tag` with barycentric
    /// coordinates `w`, lifted onto the surface.
    pub(crate) fn lift_surface(
        &self,
        nodes: &[[f64; 3]],
        tag: i32,
        t: [u32; 3],
        w: [f64; 3],
        q: [f64; 3],
    ) -> [f64; 3] {
        let Some(fits) = self.surfaces.get(&tag) else {
            return q;
        };
        let mut out = [0.0; 3];
        for (n, share) in t.iter().zip(w) {
            let lifted = match fits.get(n) {
                Some(f) => {
                    let at = nodes[*n as usize];
                    let d = minus(q, at);
                    let (x, y) = (dot(d, f.frame[0]), dot(d, f.frame[1]));
                    let z = f.height[0] * x * x + f.height[1] * x * y + f.height[2] * y * y;
                    [0, 1, 2]
                        .map(|k| at[k] + x * f.frame[0][k] + y * f.frame[1][k] + z * f.frame[2][k])
                }
                None => q,
            };
            for k in 0..3 {
                out[k] += share * lifted[k];
            }
        }
        out
    }
}

/// The tangent and curvature at every node of a curve.
fn curve_fits(nodes: &[[f64; 3]], segments: &[[u32; 2]]) -> BTreeMap<u32, CurveFit> {
    let mut next: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for [a, b] in segments {
        if a != b {
            next.entry(*a).or_default().push(*b);
            next.entry(*b).or_default().push(*a);
        }
    }
    let mut out = BTreeMap::new();
    for (&v, around) in &next {
        let p = |n: u32| nodes[n as usize];
        // Three points of the curve, by chord length from this node.
        let three = match around.as_slice() {
            [a, b] => {
                let (da, db) = (apart(p(*a), p(v)), apart(p(*b), p(v)));
                Some([(-da, p(*a)), (0.0, p(v)), (db, p(*b))])
            }
            // An end: this node, its neighbour, and the neighbour's other.
            [a] => next[a].iter().find(|n| **n != v).map(|a2| {
                let d1 = apart(p(*a), p(v));
                let d2 = d1 + apart(p(*a2), p(*a));
                [(0.0, p(v)), (d1, p(*a)), (d2, p(*a2))]
            }),
            // A branch, or nothing: no one curve to follow.
            _ => None,
        };
        if let Some(fit) = three.and_then(quadratic_at_zero) {
            out.insert(v, fit);
        }
    }
    out
}

/// The tangent and curvature at parameter nought of the quadratic through
/// three points given with their parameters.
fn quadratic_at_zero(points: [(f64, [f64; 3]); 3]) -> Option<CurveFit> {
    let s = points.map(|(s, _)| s);
    let mut first = [0.0; 3];
    let mut second = [0.0; 3];
    for i in 0..3 {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        let den = (s[i] - s[j]) * (s[i] - s[k]);
        if den.abs() <= 1e-300 {
            return None;
        }
        // Lagrange's basis: L_i(s) = (s - s_j)(s - s_k) / den.
        let d1 = (-s[j] - s[k]) / den;
        let d2 = 2.0 / den;
        for c in 0..3 {
            first[c] += d1 * points[i].1[c];
            second[c] += d2 * points[i].1[c];
        }
    }
    let speed2 = dot(first, first);
    if speed2 <= 1e-300 {
        return None;
    }
    let speed = speed2.sqrt();
    let tangent = first.map(|x| x / speed);
    let along = dot(second, tangent);
    let curvature = [0, 1, 2].map(|c| (second[c] - along * tangent[c]) / speed2);
    Some(CurveFit { tangent, curvature })
}

/// The normal and height function at every node of a surface.
fn surface_fits(nodes: &[[f64; 3]], triangles: &[[u32; 3]]) -> BTreeMap<u32, SurfaceFit> {
    let mut normal: BTreeMap<u32, [f64; 3]> = BTreeMap::new();
    let mut ring: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for t in triangles {
        let [a, b, c] = t.map(|n| nodes[n as usize]);
        // Twice the area, outward as the file wound it: a bigger triangle
        // says more about which way is out.
        let n = cross(minus(b, a), minus(c, a));
        for (i, v) in t.iter().enumerate() {
            let sum = normal.entry(*v).or_insert([0.0; 3]);
            for k in 0..3 {
                sum[k] += n[k];
            }
            for (j, w) in t.iter().enumerate() {
                if i != j {
                    ring.entry(*v).or_default().insert(*w);
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    for (&v, sum) in &normal {
        let Some(n) = unit(*sum) else {
            continue;
        };
        // The axis least along the normal, crossed with it: a tangent that
        // never comes out short.
        let axis = if n[0].abs() <= n[1].abs() && n[0].abs() <= n[2].abs() {
            [1.0, 0.0, 0.0]
        } else if n[1].abs() <= n[2].abs() {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let Some(e1) = unit(cross(n, axis)) else {
            continue;
        };
        let e2 = cross(n, e1);
        let frame = [e1, e2, n];
        // The ring round the node, and the ring round that if the first
        // cannot pin three coefficients down.
        let first: BTreeSet<u32> = ring[&v].clone();
        let mut height = fit_height(nodes, v, &frame, &first);
        if height.is_none() {
            let mut wider = first.clone();
            for w in &first {
                wider.extend(ring.get(w).into_iter().flatten().filter(|x| **x != v));
            }
            height = fit_height(nodes, v, &frame, &wider);
        }
        out.insert(
            v,
            SurfaceFit {
                frame,
                height: height.unwrap_or([0.0; 3]),
            },
        );
    }
    out
}

/// The least-squares `a x² + b x y + c y² = z` over the neighbours of `v`,
/// in its frame, or nothing when they do not pin it down.
fn fit_height(
    nodes: &[[f64; 3]],
    v: u32,
    frame: &[[f64; 3]; 3],
    around: &BTreeSet<u32>,
) -> Option<[f64; 3]> {
    let at = nodes[v as usize];
    let mut m = [[0.0; 3]; 3];
    let mut r = [0.0; 3];
    let mut scale = 0.0f64;
    for w in around {
        let d = minus(nodes[*w as usize], at);
        let (x, y, z) = (dot(d, frame[0]), dot(d, frame[1]), dot(d, frame[2]));
        let row = [x * x, x * y, y * y];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += row[i] * row[j];
            }
            r[i] += row[i] * z;
        }
        scale = scale.max(x * x + y * y);
    }
    if around.len() < 3 || scale <= 1e-300 {
        return None;
    }
    solve3(m, r, scale * scale * scale * scale)
}

/// `m x = r` by Cramer's rule, or nothing when `m` is too near singular
/// against `size`, the order of its determinant were it well conditioned.
fn solve3(m: [[f64; 3]; 3], r: [f64; 3], size: f64) -> Option<[f64; 3]> {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    if d.abs() <= 1e-9 * size.max(1e-300) {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, slot) in out.iter_mut().enumerate() {
        let mut a = m;
        for i in 0..3 {
            a[i][k] = r[i];
        }
        *slot = det(a) / d;
    }
    Some(out)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    dot(minus(a, b), minus(a, b)).sqrt()
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(a, a).sqrt();
    (n > 1e-300).then(|| a.map(|x| x / n))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A circle of radius `r` round the z axis as a polygon of `n` sides.
    fn polygon(n: usize, r: f64) -> (Vec<[f64; 3]>, Vec<[u32; 2]>) {
        // Angles by rotation, so only the four operations and a square root
        // build it — the same rule as the code under test.
        let (mut c, mut s) = (1.0f64, 0.0f64);
        let step = 2.0 * std::f64::consts::PI / n as f64;
        let (cs, ss) = (step.cos(), step.sin());
        let mut nodes = Vec::new();
        for _ in 0..n {
            nodes.push([r * c, r * s, 0.0]);
            (c, s) = (c * cs - s * ss, s * cs + c * ss);
        }
        let segments = (0..n as u32).map(|i| [i, (i + 1) % n as u32]).collect();
        (nodes, segments)
    }

    #[test]
    fn a_point_lifted_off_a_side_of_a_polygon_lands_on_the_circle() {
        let (nodes, segments) = polygon(12, 2.0);
        let fits = Fits {
            curves: BTreeMap::from([(1, curve_fits(&nodes, &segments))]),
            surfaces: BTreeMap::new(),
        };
        // The middle of a side is where the polygon is furthest inside.
        let q = [0, 1, 2].map(|k| 0.5 * (nodes[0][k] + nodes[1][k]));
        let flat = (q[0] * q[0] + q[1] * q[1]).sqrt();
        let lifted = fits.lift_curve(&nodes, 1, [0, 1], 0.5, q);
        let round = (lifted[0] * lifted[0] + lifted[1] * lifted[1]).sqrt();
        assert!((2.0 - flat) > 0.06, "the side sags: {flat}");
        assert!((2.0 - round).abs() < 0.005, "the lift does not: {round}");
        // And the ends of the side are its ends.
        assert_eq!(fits.lift_curve(&nodes, 1, [0, 1], 0.0, nodes[0]), nodes[0]);
    }

    #[test]
    fn a_straight_curve_and_a_flat_surface_are_left_as_they_are() {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 1.0, 0.0],
        ];
        let segments = vec![[0, 1], [1, 2]];
        let triangles = vec![[0, 1, 4], [0, 4, 3], [1, 2, 5], [1, 5, 4]];
        let fits = Fits {
            curves: BTreeMap::from([(1, curve_fits(&nodes, &segments))]),
            surfaces: BTreeMap::from([(1, surface_fits(&nodes, &triangles))]),
        };
        let q = [0.3, 0.0, 0.0];
        assert_eq!(fits.lift_curve(&nodes, 1, [0, 1], 0.3, q), q);
        let w = [0.2, 0.5, 0.3];
        let q = [0, 1, 2].map(|k| w[0] * nodes[0][k] + w[1] * nodes[1][k] + w[2] * nodes[4][k]);
        let lifted = fits.lift_surface(&nodes, 1, [0, 1, 4], w, q);
        for k in 0..3 {
            assert!((lifted[k] - q[k]).abs() < 1e-12, "{lifted:?} {q:?}");
        }
    }

    #[test]
    fn a_point_lifted_off_a_facet_of_a_sphere_lands_on_the_sphere() {
        // An octahedron split twice and pushed out onto the unit sphere.
        let mut nodes: Vec<[f64; 3]> = vec![
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        let mut triangles: Vec<[u32; 3]> = vec![
            [0, 2, 4],
            [2, 1, 4],
            [1, 3, 4],
            [3, 0, 4],
            [2, 0, 5],
            [1, 2, 5],
            [3, 1, 5],
            [0, 3, 5],
        ];
        for _ in 0..3 {
            let mut middle: BTreeMap<(u32, u32), u32> = BTreeMap::new();
            let mut split = Vec::new();
            for t in &triangles {
                let mut m = [0u32; 3];
                for i in 0..3 {
                    let (a, b) = (t[i].min(t[(i + 1) % 3]), t[i].max(t[(i + 1) % 3]));
                    m[i] = *middle.entry((a, b)).or_insert_with(|| {
                        let p = [0, 1, 2].map(|k| nodes[a as usize][k] + nodes[b as usize][k]);
                        nodes.push(unit(p).unwrap());
                        nodes.len() as u32 - 1
                    });
                }
                split.push([t[0], m[0], m[2]]);
                split.push([m[0], t[1], m[1]]);
                split.push([m[2], m[1], t[2]]);
                split.push([m[0], m[1], m[2]]);
            }
            triangles = split;
        }
        let fits = Fits {
            curves: BTreeMap::new(),
            surfaces: BTreeMap::from([(1, surface_fits(&nodes, &triangles))]),
        };
        let (mut flat, mut round) = (0.0f64, 0.0f64);
        for t in &triangles {
            let w = [1.0 / 3.0; 3];
            let q = [0, 1, 2].map(|k| (0..3).map(|i| nodes[t[i] as usize][k]).sum::<f64>() / 3.0);
            let lifted = fits.lift_surface(&nodes, 1, *t, w, q);
            flat = flat.max(1.0 - dot(q, q).sqrt());
            round = round.max((1.0 - dot(lifted, lifted).sqrt()).abs());
        }
        assert!(flat > 0.01, "the facets sag: {flat}");
        assert!(
            round < flat / 5.0,
            "the lift a fraction of that: {round} against {flat}"
        );
    }
}
