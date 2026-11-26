//! What an entity **is**, as against which triangles it is made of.
//!
//! A block structure is planned from facts about the part — the hole is a
//! cylinder of radius 2 along x, this face is the plane z = 7.5, that curve
//! is an arc of 180° — and a faceting states none of them. Somebody looking
//! at the part on a screen reads them off without thinking; a script, or a
//! program planning a blocking, has only the triangles. So they are worked
//! out here, once, from the faceting:
//!
//! - every entity's **size**: a curve's length, a surface's area, a box;
//! - a curve's nodes **in order**, as one or more polylines, and the corners
//!   at its ends;
//! - and its **shape**, when it has a simple one: a curve that is a straight
//!   line or an arc of a circle, a surface that is a plane, a cylinder or a
//!   sphere — with the numbers that pin it down.
//!
//! A shape is only claimed when **every node** of the entity lies on it,
//! within a small share of the entity's own size. A CAD model meshed by gmsh
//! puts its nodes on the exact surface, so a real cylinder fits to rounding
//! and anything else fails by far more than the tolerance; "other" is the
//! honest answer for a surface that is none of the three, not a failure.
//!
//! Which way a curved surface turns is read from the triangles' winding,
//! which gmsh writes outward from the volume: a cylinder whose normals point
//! away from its axis has the material inside it — a boss, an outer wall —
//! and one whose normals point at its axis is a hole. A file wound the other
//! way round says the opposite, which is why this is reported as what the
//! winding says rather than as a fact about the part.

use crate::composite::Model;
use crate::model::Geometry;
use crate::{CORNER, CURVE, SURFACE};

/// A share of an entity's own size: how far a node may be from a shape and
/// still be on it. Generous next to the rounding of a CAD export, and far
/// below what a shape that is *nearly* a cylinder misses by.
pub const SHAPE_TOLERANCE: f64 = 1e-3;

/// What a curve is.
#[derive(Clone, Debug, PartialEq)]
pub enum CurveShape {
    /// Straight, along `direction` (unit).
    Line {
        direction: [f64; 3],
    },
    /// On a circle: a whole one when `sweep` is 360.
    Circle {
        center: [f64; 3],
        /// Unit normal of the circle's plane.
        normal: [f64; 3],
        radius: f64,
        /// How much of the circle the curve covers, in degrees.
        sweep: f64,
    },
    Other,
}

/// What a surface is.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceShape {
    Plane {
        /// Unit normal, turned the way the triangles face.
        normal: [f64; 3],
        /// `normal · p` for every point p of the plane.
        offset: f64,
    },
    Cylinder {
        /// A point of the axis: the one level with the middle of the surface.
        point: [f64; 3],
        /// Unit direction of the axis.
        axis: [f64; 3],
        radius: f64,
        /// How far round the axis the surface goes, in degrees.
        sweep: f64,
        /// How long it is along the axis.
        length: f64,
        /// The triangles face away from the axis: the material is inside.
        convex: bool,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
        convex: bool,
    },
    Other,
}

/// Facts about one curve.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveFacts {
    pub length: f64,
    pub bounds: ([f64; 3], [f64; 3]),
    /// The curve's nodes in order, one list per connected run: a curve is
    /// almost always one, but a file is free to say otherwise.
    pub chains: Vec<Vec<[f64; 3]>>,
    /// The single run comes back to where it started.
    pub closed: bool,
    pub shape: CurveShape,
}

/// Facts about one surface.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceFacts {
    pub area: f64,
    pub bounds: ([f64; 3], [f64; 3]),
    /// The area-weighted mean of the triangles' normals, unit, or nought
    /// when they cancel — as they do round a whole cylinder.
    pub normal: [f64; 3],
    pub shape: SurfaceShape,
}

impl<'a> Model<'a> {
    /// The segments of a curve, a merge's being its members' together.
    pub fn segments_of(&self, tag: i32) -> Vec<[u32; 2]> {
        let members = self.members_or_self(CURVE, tag);
        self.geometry
            .curves()
            .iter()
            .filter(|c| members.contains(&c.tag))
            .flat_map(|c| c.segments.iter().copied())
            .collect()
    }

    /// The triangles of a surface, a merge's being its members' together.
    pub fn triangles_of(&self, tag: i32) -> Vec<[u32; 3]> {
        let members = self.members_or_self(SURFACE, tag);
        self.geometry
            .surfaces()
            .iter()
            .filter(|s| members.contains(&s.tag))
            .flat_map(|s| s.triangles.iter().copied())
            .collect()
    }

    fn members_or_self(&self, dim: u8, tag: i32) -> Vec<i32> {
        match self.composites.members(tag) {
            Some((d, members)) if d == dim => members.to_vec(),
            Some(_) => Vec::new(),
            None => vec![tag],
        }
    }

    /// Where a corner is.
    pub fn corner_at(&self, tag: i32) -> Option<[f64; 3]> {
        self.geometry
            .corners()
            .iter()
            .find(|c| c.tag == tag)
            .map(|c| self.geometry.nodes()[c.node as usize])
    }

    /// Facts about a curve, or nothing when there is no such curve.
    pub fn curve_facts(&self, tag: i32) -> Option<CurveFacts> {
        let segments = self.segments_of(tag);
        if segments.is_empty() {
            return None;
        }
        Some(curve_facts(self.geometry, &segments))
    }

    /// Facts about a surface, or nothing when there is no such surface.
    pub fn surface_facts(&self, tag: i32) -> Option<SurfaceFacts> {
        let triangles = self.triangles_of(tag);
        if triangles.is_empty() {
            return None;
        }
        Some(surface_facts(self.geometry, &triangles))
    }

    /// The visible corners at the ends of a curve's runs: the corners a
    /// block edge along it would start and finish on.
    pub fn ends_of(&self, tag: i32) -> Vec<i32> {
        let chains = chains(&self.segments_of(tag));
        let mut ends = Vec::new();
        for chain in &chains {
            let (Some(first), Some(last)) = (chain.first(), chain.last()) else {
                continue;
            };
            for corner in self.geometry.corners() {
                let shown = self.composites.stand_in(CORNER, corner.tag).is_none();
                if shown
                    && (corner.node == *first || corner.node == *last)
                    && !ends.contains(&corner.tag)
                {
                    ends.push(corner.tag);
                }
            }
        }
        ends
    }
}

/// The facts of a run of segments.
pub fn curve_facts(geometry: &Geometry, segments: &[[u32; 2]]) -> CurveFacts {
    let nodes = geometry.nodes();
    let runs = chains(segments);
    let closed = runs.len() == 1 && runs[0].len() > 2 && runs[0].first() == runs[0].last();
    let points: Vec<[f64; 3]> = unique(segments.iter().flatten().copied())
        .into_iter()
        .map(|n| nodes[n as usize])
        .collect();
    let length = segments
        .iter()
        .map(|s| apart(nodes[s[0] as usize], nodes[s[1] as usize]))
        .sum();
    let bounds = bounds_of(&points);
    let size = apart(bounds.0, bounds.1).max(length);
    CurveFacts {
        length,
        bounds,
        chains: runs
            .iter()
            .map(|run| run.iter().map(|&n| nodes[n as usize]).collect())
            .collect(),
        closed,
        shape: curve_shape(&points, size * SHAPE_TOLERANCE),
    }
}

/// The facts of a set of triangles.
pub fn surface_facts(geometry: &Geometry, triangles: &[[u32; 3]]) -> SurfaceFacts {
    let nodes = geometry.nodes();
    let corners = |t: &[u32; 3]| t.map(|n| nodes[n as usize]);
    // Each triangle's normal, its length twice the area.
    let normals: Vec<[f64; 3]> = triangles
        .iter()
        .map(|t| {
            let [a, b, c] = corners(t);
            cross(minus(b, a), minus(c, a))
        })
        .collect();
    let area = normals.iter().map(|n| norm(*n)).sum::<f64>() / 2.0;
    let sum = normals.iter().fold([0.0; 3], |s, n| plus(s, *n));
    let normal = unit(sum).unwrap_or([0.0; 3]);
    let points: Vec<[f64; 3]> = unique(triangles.iter().flatten().copied())
        .into_iter()
        .map(|n| nodes[n as usize])
        .collect();
    let bounds = bounds_of(&points);
    let tolerance = apart(bounds.0, bounds.1) * SHAPE_TOLERANCE;
    let centers: Vec<[f64; 3]> = triangles
        .iter()
        .map(|t| {
            let [a, b, c] = corners(t);
            scale(plus(plus(a, b), c), 1.0 / 3.0)
        })
        .collect();
    SurfaceFacts {
        area,
        bounds,
        normal,
        shape: surface_shape(&points, &normals, &centers, sum, tolerance),
    }
}

/// The runs of a set of segments, as node indices in order. A run that
/// closes on itself repeats its first node at the end.
pub fn chains(segments: &[[u32; 2]]) -> Vec<Vec<u32>> {
    use std::collections::BTreeMap;
    let mut at: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (k, s) in segments.iter().enumerate() {
        at.entry(s[0]).or_default().push(k);
        at.entry(s[1]).or_default().push(k);
    }
    let mut used = vec![false; segments.len()];
    let mut runs = Vec::new();
    // Ends first, so an open curve is walked from one end rather than from
    // the middle; then whatever is left, which is closed.
    let mut starts: Vec<u32> = at
        .iter()
        .filter(|(_, ks)| ks.len() == 1)
        .map(|(n, _)| *n)
        .collect();
    starts.extend(segments.iter().map(|s| s[0]));
    for start in starts {
        let Some(&first) = at[&start].iter().find(|k| !used[**k]) else {
            continue;
        };
        let mut run = vec![start];
        let (mut here, mut next) = (start, Some(first));
        while let Some(k) = next {
            used[k] = true;
            let s = segments[k];
            here = if s[0] == here { s[1] } else { s[0] };
            run.push(here);
            next = at[&here].iter().copied().find(|k| !used[*k]);
        }
        runs.push(run);
    }
    runs
}

fn curve_shape(points: &[[f64; 3]], tolerance: f64) -> CurveShape {
    // A curve of no length — an export's leftover — is no shape at all.
    if points.len() < 2 || tolerance <= 0.0 {
        return CurveShape::Other;
    }
    let center = centroid(points);
    let (values, vectors) = eigen(covariance(points, center));
    let direction = vectors[2];
    if points
        .iter()
        .all(|p| off_line(*p, center, direction) <= tolerance)
    {
        return CurveShape::Line { direction };
    }
    // Three points are on a circle whatever they are, so it takes four to
    // say a curve is one.
    if points.len() < 4 {
        return CurveShape::Other;
    }
    // On a plane, and on a circle in it. Points all along one line have
    // no plane of their own.
    let normal = vectors[0];
    if values[1] <= f64::EPSILON * values[2] {
        return CurveShape::Other;
    }
    if points
        .iter()
        .any(|p| dot(minus(*p, center), normal).abs() > tolerance)
    {
        return CurveShape::Other;
    }
    let Some((center, radius)) = circle(points, center, normal) else {
        return CurveShape::Other;
    };
    if radius <= tolerance
        || points
            .iter()
            .any(|p| (apart(*p, center) - radius).abs() > tolerance)
    {
        return CurveShape::Other;
    }
    CurveShape::Circle {
        center,
        normal,
        radius,
        sweep: sweep(points, center, normal),
    }
}

fn surface_shape(
    points: &[[f64; 3]],
    normals: &[[f64; 3]],
    centers: &[[f64; 3]],
    outward: [f64; 3],
    tolerance: f64,
) -> SurfaceShape {
    if points.len() < 3 {
        return SurfaceShape::Other;
    }
    let middle = centroid(points);

    // A plane: every node on the best one through them.
    let (_, vectors) = eigen(covariance(points, middle));
    let mut normal = vectors[0];
    if points
        .iter()
        .all(|p| dot(minus(*p, middle), normal).abs() <= tolerance)
    {
        if dot(normal, outward) < 0.0 {
            normal = scale(normal, -1.0);
        }
        return SurfaceShape::Plane {
            normal,
            offset: dot(normal, middle),
        };
    }

    // A cylinder: the axis is the direction every normal is square to, and
    // the nodes, seen down it, are on a circle.
    let mut m = [[0.0; 3]; 3];
    for n in normals {
        // Weighted by area already: a normal's length is twice it.
        let w = 1.0 / norm(*n).max(f64::MIN_POSITIVE);
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += n[i] * n[j] * w;
            }
        }
    }
    let (values, vectors) = eigen(m);
    // Facets of a real cylinder have three nodes anywhere on it, so their
    // normals are only nearly square to the axis: this is a first guess,
    // and the nodes — which *are* on the cylinder — settle it.
    // And the normals have to turn: parallel ones are flat faces, whose
    // corners may well happen to be on a circle — a square's are.
    if values[0] <= 0.05 * values[2] && values[1] > 1e-9 * values[2] {
        let axis = refine_axis(points, middle, vectors[0]);
        if let Some((center, radius)) = circle(points, middle, axis) {
            let on = points
                .iter()
                .all(|p| (off_line(*p, center, axis) - radius).abs() <= tolerance);
            let radial = facing_out_from(normals, centers, |c| {
                let r = minus(c, center);
                minus(r, scale(axis, dot(r, axis)))
            });
            if on && radial && radius > tolerance {
                let along: Vec<f64> = points
                    .iter()
                    .map(|p| dot(minus(*p, center), axis))
                    .collect();
                let (lo, hi) = along
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| {
                        (lo.min(*t), hi.max(*t))
                    });
                let point = plus(center, scale(axis, (lo + hi) / 2.0));
                // Away from the axis, on the whole: the material is inside.
                let away: f64 = normals
                    .iter()
                    .zip(centers)
                    .map(|(n, c)| {
                        let r = minus(*c, center);
                        dot(*n, minus(r, scale(axis, dot(r, axis))))
                    })
                    .sum();
                return SurfaceShape::Cylinder {
                    point,
                    axis,
                    radius,
                    sweep: sweep(points, center, axis),
                    length: hi - lo,
                    convex: away > 0.0,
                };
            }
        }
    }

    // A sphere.
    if let Some((center, radius)) = sphere(points) {
        let on = points
            .iter()
            .all(|p| (apart(*p, center) - radius).abs() <= tolerance);
        if on && radius > tolerance && facing_out_from(normals, centers, |c| minus(c, center)) {
            let away: f64 = normals
                .iter()
                .zip(centers)
                .map(|(n, c)| dot(*n, minus(*c, center)))
                .sum();
            return SurfaceShape::Sphere {
                center,
                radius,
                convex: away > 0.0,
            };
        }
    }
    SurfaceShape::Other
}

/// Whether the facets face along the radius at their middles, one way or
/// the other — within the angle a coarse faceting's facets turn by. Nearly
/// all of the area rather than every facet: a mesher leaves slivers round a
/// hole whose three nodes, all on the surface, span a plane tilted any way
/// at all. Without it a cube's corners are on a sphere, and a square's on a
/// circle.
fn facing_out_from(
    normals: &[[f64; 3]],
    centers: &[[f64; 3]],
    radius: impl Fn([f64; 3]) -> [f64; 3],
) -> bool {
    let total: f64 = normals.iter().map(|n| norm(*n)).sum();
    let facing: f64 = normals
        .iter()
        .zip(centers)
        .filter(|(n, c)| matches!((unit(**n), unit(radius(**c))), (Some(n), Some(r)) if dot(n, r).abs() >= 0.95))
        .map(|(n, _)| norm(*n))
        .sum();
    facing >= 0.95 * total
}

/// The axis, near `guess`, down which the points look most like a circle:
/// the worst miss made as small as it will go, by a search that halves its
/// step whenever no tilt helps.
fn refine_axis(points: &[[f64; 3]], middle: [f64; 3], guess: [f64; 3]) -> [f64; 3] {
    let miss = |axis: [f64; 3]| match circle(points, middle, axis) {
        Some((center, radius)) => points
            .iter()
            .map(|p| (off_line(*p, center, axis) - radius).powi(2))
            .sum::<f64>(),
        None => f64::INFINITY,
    };
    let mut axis = guess;
    let mut best = miss(axis);
    let mut step = 0.05;
    while step > 1e-9 {
        let (u, v) = square_to(axis);
        let better = [u, v, scale(u, -1.0), scale(v, -1.0)]
            .into_iter()
            .filter_map(|d| unit(plus(axis, scale(d, step))))
            .map(|a| (miss(a), a))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match better {
            Some((m, a)) if m < best => (best, axis) = (m, a),
            _ => step /= 2.0,
        }
    }
    axis
}

/// The circle through points seen down `normal`, by least squares: its
/// center in space, level with `through`, and its radius.
fn circle(points: &[[f64; 3]], through: [f64; 3], normal: [f64; 3]) -> Option<([f64; 3], f64)> {
    let (u, v) = square_to(normal);
    // x² + y² + D x + E y + F = 0, in the plane's own coordinates.
    let mut a = [[0.0; 3]; 3];
    let mut b = [0.0; 3];
    for p in points {
        let d = minus(*p, through);
        let (x, y) = (dot(d, u), dot(d, v));
        let row = [x, y, 1.0];
        let rhs = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * rhs;
        }
    }
    let [dx, ey, f] = solve3(a, b)?;
    let (cx, cy) = (-dx / 2.0, -ey / 2.0);
    let r2 = cx * cx + cy * cy - f;
    if r2 <= 0.0 {
        return None;
    }
    Some((plus(through, plus(scale(u, cx), scale(v, cy))), r2.sqrt()))
}

/// The sphere through points, by least squares.
fn sphere(points: &[[f64; 3]]) -> Option<([f64; 3], f64)> {
    let middle = centroid(points);
    // x² + y² + z² + D x + E y + F z + G = 0, about the middle.
    let mut a = [[0.0; 4]; 4];
    let mut b = [0.0; 4];
    for p in points {
        let d = minus(*p, middle);
        let row = [d[0], d[1], d[2], 1.0];
        let rhs = -dot(d, d);
        for i in 0..4 {
            for j in 0..4 {
                a[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * rhs;
        }
    }
    let [dd, e, f, g] = solve4(a, b)?;
    let c = [-dd / 2.0, -e / 2.0, -f / 2.0];
    let r2 = dot(c, c) - g;
    (r2 > 0.0).then(|| (plus(middle, c), r2.sqrt()))
}

/// How far round `axis` through `center` the points go, in degrees: all the
/// way less the widest gap between them. A whole circle has gaps between
/// its nodes all round, unevenly where a mesher bunched them, and none of
/// them is a gap in the circle: the widest one is only a gap when it stands
/// out from the next widest.
fn sweep(points: &[[f64; 3]], center: [f64; 3], axis: [f64; 3]) -> f64 {
    let (u, v) = square_to(axis);
    let mut angles: Vec<f64> = points
        .iter()
        .map(|p| {
            let d = minus(*p, center);
            dot(d, v).atan2(dot(d, u))
        })
        .collect();
    angles.sort_by(f64::total_cmp);
    // A cylinder has every angle once per ring of nodes.
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let full = std::f64::consts::TAU;
    let (Some(first), Some(last)) = (angles.first(), angles.last()) else {
        return 0.0;
    };
    let mut gaps: Vec<f64> = angles.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.push(first + full - last);
    gaps.sort_by(|a, b| b.total_cmp(a));
    let widest = gaps[0];
    let next = gaps.get(1).copied().unwrap_or(0.0);
    if widest <= 2.5 * next {
        360.0
    } else {
        (full - widest).to_degrees()
    }
}

// --- small linear algebra ---------------------------------------------------

fn covariance(points: &[[f64; 3]], center: [f64; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for p in points {
        let d = minus(*p, center);
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += d[i] * d[j];
            }
        }
    }
    m
}

/// Eigenvalues of a symmetric 3×3, ascending, and their unit vectors.
fn eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        // The largest off-diagonal entry, rotated away (Jacobi).
        let (mut p, mut q, mut big) = (0, 1, a[0][1].abs());
        for (i, j) in [(0, 2), (1, 2)] {
            if a[i][j].abs() > big {
                (p, q, big) = (i, j, a[i][j].abs());
            }
        }
        let scale = a[0][0].abs() + a[1][1].abs() + a[2][2].abs();
        if big <= 1e-15 * scale.max(f64::MIN_POSITIVE) {
            break;
        }
        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        let sign = if theta >= 0.0 { 1.0 } else { -1.0 };
        let t = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
        let c = 1.0 / (t * t + 1.0).sqrt();
        let s = t * c;
        let mut b = a;
        for k in 0..3 {
            b[k][p] = c * a[k][p] - s * a[k][q];
            b[k][q] = s * a[k][p] + c * a[k][q];
        }
        a = b;
        for k in 0..3 {
            b[p][k] = c * a[p][k] - s * a[q][k];
            b[q][k] = s * a[p][k] + c * a[q][k];
        }
        a = b;
        for row in v.iter_mut() {
            let (vp, vq) = (row[p], row[q]);
            row[p] = c * vp - s * vq;
            row[q] = s * vp + c * vq;
        }
    }
    let mut order = [0, 1, 2];
    order.sort_by(|i, j| a[*i][*i].total_cmp(&a[*j][*j]));
    let values = order.map(|i| a[i][i]);
    let vectors = order.map(|i| unit([v[0][i], v[1][i], v[2][i]]).unwrap_or([0.0; 3]));
    (values, vectors)
}

fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(a);
    if d.abs() <= f64::MIN_POSITIVE {
        return None;
    }
    let mut x = [0.0; 3];
    for (k, out) in x.iter_mut().enumerate() {
        let mut m = a;
        for i in 0..3 {
            m[i][k] = b[i];
        }
        *out = det(m) / d;
    }
    Some(x)
}

fn solve4(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for col in 0..4 {
        let pivot = (col..4).max_by(|i, j| a[*i][col].abs().total_cmp(&a[*j][col].abs()))?;
        if a[pivot][col].abs() <= f64::MIN_POSITIVE {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..4 {
            let f = a[row][col] / a[col][col];
            let pivot_row = a[col];
            for (k, x) in a[row].iter_mut().enumerate().skip(col) {
                *x -= f * pivot_row[k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = [0.0; 4];
    for row in (0..4).rev() {
        let rest: f64 = (row + 1..4).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - rest) / a[row][row];
    }
    Some(x)
}

/// Two unit vectors square to `n` and to each other.
fn square_to(n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let helper = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = unit(cross(n, helper)).unwrap_or([1.0, 0.0, 0.0]);
    (u, cross(n, u))
}

fn off_line(p: [f64; 3], through: [f64; 3], direction: [f64; 3]) -> f64 {
    let d = minus(p, through);
    norm(minus(d, scale(direction, dot(d, direction))))
}

fn unique(nodes: impl Iterator<Item = u32>) -> Vec<u32> {
    let mut all: Vec<u32> = nodes.collect();
    all.sort_unstable();
    all.dedup();
    all
}

fn centroid(points: &[[f64; 3]]) -> [f64; 3] {
    let n = points.len().max(1) as f64;
    scale(points.iter().fold([0.0; 3], |s, p| plus(s, *p)), 1.0 / n)
}

pub fn bounds_of(points: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
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

fn plus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = norm(a);
    (n > f64::MIN_POSITIVE).then(|| scale(a, 1.0 / n))
}
fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(minus(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box, an open cylinder and a hemisphere's worth of triangles, as an
    /// MSH 2.2 file: the shapes the fitting has to recognise.
    fn faceted(
        surfaces: Vec<(i32, Vec<[[f64; 3]; 3]>)>,
        curves: Vec<(i32, Vec<[f64; 3]>)>,
    ) -> Geometry {
        fn index(p: [f64; 3], nodes: &mut Vec<[f64; 3]>) -> usize {
            if let Some(k) = nodes.iter().position(|q| apart(*q, p) < 1e-12) {
                return k + 1;
            }
            nodes.push(p);
            nodes.len()
        }
        let mut nodes: Vec<[f64; 3]> = Vec::new();
        let mut elements = Vec::new();
        for (tag, triangles) in &surfaces {
            for t in triangles {
                let ids = t.map(|p| index(p, &mut nodes));
                elements.push(format!("2 2 0 {tag} {} {} {}", ids[0], ids[1], ids[2]));
            }
        }
        for (tag, points) in &curves {
            for w in points.windows(2) {
                let (a, b) = (index(w[0], &mut nodes), index(w[1], &mut nodes));
                elements.push(format!("1 2 0 {tag} {a} {b}"));
            }
        }
        let mut text = String::from("$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n");
        text.push_str(&format!("{}\n", nodes.len()));
        for (k, p) in nodes.iter().enumerate() {
            text.push_str(&format!("{} {} {} {}\n", k + 1, p[0], p[1], p[2]));
        }
        text.push_str(&format!("$EndNodes\n$Elements\n{}\n", elements.len()));
        for (k, e) in elements.iter().enumerate() {
            let (kind, rest) = e.split_once(' ').unwrap();
            text.push_str(&format!("{} {kind} {rest}\n", k + 1));
        }
        text.push_str("$EndElements\n");
        Geometry::read(&text).expect("a geometry")
    }

    fn cylinder(
        radius: f64,
        from: f64,
        to: f64,
        outward: bool,
        n: usize,
        sweep: f64,
    ) -> Vec<[[f64; 3]; 3]> {
        let at = |k: usize, z: f64| {
            let a = sweep.to_radians() * k as f64 / n as f64;
            [radius * a.cos(), radius * a.sin(), z]
        };
        let mut out = Vec::new();
        for k in 0..n {
            let (a, b, c, d) = (at(k, from), at(k + 1, from), at(k + 1, to), at(k, to));
            // Counter-clockwise seen from outside faces away from the axis.
            if outward {
                out.push([a, b, c]);
                out.push([a, c, d]);
            } else {
                out.push([a, c, b]);
                out.push([a, d, c]);
            }
        }
        out
    }

    #[test]
    fn a_plane_a_cylinder_and_a_hole_are_told_apart() {
        let square = vec![
            [[0.0, 0.0, 7.5], [2.0, 0.0, 7.5], [2.0, 1.0, 7.5]],
            [[0.0, 0.0, 7.5], [2.0, 1.0, 7.5], [0.0, 1.0, 7.5]],
        ];
        let g = faceted(
            vec![
                (1, square),
                (2, cylinder(5.0, -2.5, 7.5, true, 24, 180.0)),
                (3, cylinder(2.0, 0.0, 3.0, false, 32, 360.0)),
            ],
            vec![],
        );
        let composites = crate::Composites::new();
        let model = Model::new(&g, &composites);

        let plane = model.surface_facts(1).unwrap();
        assert!((plane.area - 2.0).abs() < 1e-12);
        match plane.shape {
            SurfaceShape::Plane { normal, offset } => {
                assert!((normal[2] - 1.0).abs() < 1e-9, "{normal:?}");
                assert!((offset - 7.5).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }

        match model.surface_facts(2).unwrap().shape {
            SurfaceShape::Cylinder {
                axis,
                radius,
                sweep,
                length,
                convex,
                point,
            } => {
                assert!(axis[2].abs() > 1.0 - 1e-9, "{axis:?}");
                assert!((radius - 5.0).abs() < 1e-9);
                assert!((sweep - 180.0).abs() < 1e-6, "{sweep}");
                assert!((length - 10.0).abs() < 1e-9);
                assert!((point[2] - 2.5).abs() < 1e-9);
                assert!(convex, "a wall facing away from its axis");
            }
            other => panic!("{other:?}"),
        }
        match model.surface_facts(3).unwrap().shape {
            SurfaceShape::Cylinder {
                radius,
                sweep,
                convex,
                ..
            } => {
                assert!((radius - 2.0).abs() < 1e-9);
                assert_eq!(sweep, 360.0);
                assert!(!convex, "a hole faces its axis");
            }
            other => panic!("{other:?}"),
        }
    }

    /// A cylinder the way gmsh facets one: nodes scattered over it, not in
    /// rings, so no facet's normal is square to the axis.
    #[test]
    fn a_cylinder_faceted_unevenly_is_still_a_cylinder() {
        // Axis along x through (0, 0, 3), radius 2: B1's hole.
        let at = |a: f64, x: f64| [x, 2.0 * a.cos(), 3.0 + 2.0 * a.sin()];
        let mut triangles = Vec::new();
        let rows = 7;
        for i in 0..rows {
            for k in 0..16 {
                // Every other row shifted by half a step, and the lengths uneven.
                let shift = if i % 2 == 0 { 0.0 } else { 0.5 };
                let a0 = std::f64::consts::PI * (k as f64 + shift) / 16.0;
                let a1 = std::f64::consts::PI * (k as f64 + 1.0 + shift) / 16.0;
                let am = std::f64::consts::PI * (k as f64 + 0.5 + (1.0 - shift)) / 16.0;
                let (x0, x1) = (
                    3.0 + 0.3 * i as f64 + 0.01 * (k % 3) as f64,
                    3.0 + 0.3 * (i + 1) as f64,
                );
                triangles.push([at(a0, x0), at(am, x1), at(a1, x0)]);
            }
        }
        let g = faceted(vec![(1, triangles)], vec![]);
        let composites = crate::Composites::new();
        match Model::new(&g, &composites).surface_facts(1).unwrap().shape {
            SurfaceShape::Cylinder {
                axis,
                radius,
                point,
                ..
            } => {
                assert!(axis[0].abs() > 1.0 - 1e-9, "{axis:?}");
                assert!((radius - 2.0).abs() < 1e-6, "{radius}");
                assert!(
                    (point[1]).abs() < 1e-6 && (point[2] - 3.0).abs() < 1e-6,
                    "{point:?}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_line_an_arc_and_a_wiggle() {
        let arc: Vec<[f64; 3]> = (0..=12)
            .map(|k| {
                let a = std::f64::consts::PI * k as f64 / 12.0;
                [8.0 + 3.0 * a.cos(), 3.0 * a.sin(), -2.5]
            })
            .collect();
        let wiggle: Vec<[f64; 3]> = (0..=12)
            .map(|k| [k as f64, (k as f64).sin(), 0.1 * k as f64])
            .collect();
        let square = vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]]];
        let g = faceted(
            vec![(1, square)],
            vec![
                (1, vec![[3.0, 0.0, -2.5], [3.0, 0.0, 1.0], [3.0, 0.0, 7.5]]),
                (2, arc),
                (3, wiggle),
                (4, vec![[0.0, 5.0, 0.0], [0.1, 5.0, 0.2], [0.0, 5.0, 0.5]]),
                (5, vec![[9.0, 9.0, 9.0], [9.0, 9.0, 9.0], [9.0, 9.0, 9.0]]),
            ],
        );
        let composites = crate::Composites::new();
        let model = Model::new(&g, &composites);

        let line = model.curve_facts(1).unwrap();
        assert!((line.length - 10.0).abs() < 1e-12);
        assert_eq!(line.chains.len(), 1);
        assert!(!line.closed);
        assert!(
            matches!(line.shape, CurveShape::Line { direction } if direction[2].abs() > 1.0 - 1e-12)
        );

        match model.curve_facts(2).unwrap().shape {
            CurveShape::Circle {
                center,
                radius,
                sweep,
                normal,
            } => {
                assert!(apart(center, [8.0, 0.0, -2.5]) < 1e-9, "{center:?}");
                assert!((radius - 3.0).abs() < 1e-9);
                assert!((sweep - 180.0).abs() < 1e-6, "{sweep}");
                assert!(normal[2].abs() > 1.0 - 1e-9);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(model.curve_facts(3).unwrap().shape, CurveShape::Other);
        // Three points are on some circle; that says nothing.
        assert_eq!(model.curve_facts(4).unwrap().shape, CurveShape::Other);
        // Nor does a curve of no length.
        assert_eq!(model.curve_facts(5).unwrap().shape, CurveShape::Other);
        assert!(model.curve_facts(99).is_none());
    }

    #[test]
    fn runs_are_walked_end_to_end_and_a_loop_closes() {
        // Given out of order and backwards.
        let open = chains(&[[2, 3], [1, 2], [4, 3]]);
        assert_eq!(open, vec![vec![1, 2, 3, 4]]);
        let closed = chains(&[[1, 2], [3, 1], [2, 3]]);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].first(), closed[0].last());
        assert_eq!(closed[0].len(), 4);
    }

    #[test]
    fn eigen_finds_the_axes_of_a_flat_cloud() {
        let (values, vectors) = eigen([[4.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]);
        assert_eq!(values, [0.0, 1.0, 4.0]);
        assert!(vectors[0][2].abs() > 1.0 - 1e-12);
        let (values, _) = eigen([[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]]);
        assert!((values[0] - 1.0).abs() < 1e-12 && (values[1] - 3.0).abs() < 1e-12);
    }
}
