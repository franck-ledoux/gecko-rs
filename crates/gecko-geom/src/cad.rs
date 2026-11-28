//! An exact geometry: a boundary representation whose curves and surfaces
//! are NURBS, from the [`nurbs`] crate.
//!
//! The faceted [`Geometry`](crate::Geometry) is a gmsh mesh grouped into
//! entities, and projects onto a smooth reconstruction of its facets. This
//! one is what a CAD model *is*: every edge a rational B-spline curve,
//! every face a rational B-spline surface, and a point projected onto them
//! is on them to the precision of the search, not of a faceting.
//!
//! # What is in one
//!
//! - **Vertices**, each a point.
//! - **Edges**, each a curve — its whole domain, so a piece of a longer
//!   curve is split off first ([`nurbs::Curve::split`]) — between two
//!   vertices, the same one twice for a closed curve.
//! - **Faces**, each a surface and the edges that bound it, and a
//!   [`Trim`]: the surface's whole domain, or the part of it inside loops
//!   of those edges.
//!
//! The **incidence** is the topology's: an edge is on the faces that list
//! it, a vertex on the edges that end at it and on their faces. Nothing is
//! deduced from shared nodes, as the faceted model has to.
//!
//! # Trimming
//!
//! A loop is a list of edges, each either way round, ending where the
//! next begins. Its polygon in the face's parameters is sampled once, when
//! the geometry is built: each edge is sampled and every sample projected
//! onto the surface. Where the surface is periodic — a cylinder round its
//! seam — consecutive samples are kept within half a period of each other,
//! so a loop straddling the seam stays in one piece.
//!
//! A point of the parameters is inside the trim when it is inside an odd
//! number of the loops — the outer one and not a hole — whichever way
//! round each was given.
//!
//! A loop that would not close in the parameters is refused rather than
//! guessed at: one going right round a periodic surface, or one through a
//! pole of a sphere, where one point of space is a whole line of
//! parameters. Those faces are left untrimmed — a full sphere, a full
//! cylinder — which is what they are.
//!
//! # Projection
//!
//! - Onto a vertex: the vertex.
//! - Onto an edge: the nearest point of its curve.
//! - Onto a face: the nearest point of its surface; and when that point is
//!   outside the trim, the nearest point of the face's boundary edges
//!   instead.
//!
//! The last is the nearest point of the face whenever the nearest point of
//! the whole surface is outside it and the surface has no other minimum
//! inside — true of every face the tests build, and of a face that is not
//! folded over itself.
//!
//! # The same bits everywhere
//!
//! The [`nurbs`] crate uses only the four operations and square roots, and
//! so does this: a geometry built and projected onto in the page gives the
//! terminal's numbers, bit for bit.

use crate::where_on::{CORNER, CURVE, SURFACE};
use nurbs::{Curve, Surface};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// How finely an edge is sampled to draw its loop in a face's parameters.
const LOOP_SAMPLES: usize = 48;

/// A tolerance, as a share of the geometry's span: how far apart two
/// points meant to be one may be.
const TOLERANCE: f64 = 1e-9;

#[derive(Debug, Error, PartialEq)]
pub enum CadError {
    #[error("two {kind}s are tagged {tag}: a tag names one entity of each dimension")]
    Repeated { kind: &'static str, tag: i32 },
    #[error("edge {edge} ends at vertex {vertex}, and there is no such vertex")]
    NoVertex { edge: i32, vertex: i32 },
    #[error("edge {edge} {end} is {gap:.3e} from vertex {vertex}, which it is meant to end at")]
    EndApart {
        edge: i32,
        end: &'static str,
        vertex: i32,
        gap: f64,
    },
    #[error("face {face} is bounded by edge {edge}, and there is no such edge")]
    NoEdge { face: i32, edge: i32 },
    #[error("face {face} has edge {edge} in a loop and not among the edges bounding it")]
    NotBounding { face: i32, edge: i32 },
    #[error("face {face} has an empty loop")]
    EmptyLoop { face: i32 },
    #[error(
        "face {face}: in loop {ring}, edge {edge} does not begin where the edge before it ends"
    )]
    OpenLoop { face: i32, ring: usize, edge: i32 },
    #[error(
        "face {face}: loop {ring} does not close in the surface's parameters — it goes round \
         a periodic surface, or through a pole — so it cannot trim it; leave such a face \
         untrimmed"
    )]
    Unclosed { face: i32, ring: usize },
}

/// A point of the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Vertex {
    pub tag: i32,
    pub point: [f64; 3],
}

/// A curve of the model, between two vertices.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub tag: i32,
    /// The whole domain of it is the edge.
    pub curve: Curve,
    /// The vertex at the start of its domain, and the one at the end: the
    /// same one twice for a closed curve.
    pub ends: [i32; 2],
}

/// Which part of its surface a face is.
#[derive(Clone, Debug, PartialEq)]
pub enum Trim {
    /// All of it.
    Natural,
    /// What is inside the loops: each a list of edges, `true` for one
    /// taken against its curve's direction.
    Loops(Vec<Vec<(i32, bool)>>),
}

/// A surface of the model, and what bounds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub tag: i32,
    pub surface: Surface,
    /// Every edge on its boundary, whether or not a loop trims along it —
    /// the seam of a cylinder is one, and trims nothing.
    pub edges: Vec<i32>,
    pub trim: Trim,
    /// The loops' polygons in the surface's parameters, sampled when the
    /// geometry was built; none for a face left whole.
    polygons: Vec<Vec<[f64; 2]>>,
    /// The period of each parameter, where the surface closes up on itself.
    periods: [Option<f64>; 2],
}

impl Face {
    /// The period of each parameter, `u` then `v`, where the surface closes
    /// up on itself along it.
    pub fn periods(&self) -> [Option<f64>; 2] {
        self.periods
    }

    /// Whether `(u, v)` is part of the face.
    pub fn contains(&self, uv: [f64; 2]) -> bool {
        if self.polygons.is_empty() {
            return true;
        }
        // The point, and its copies a period away where the surface is
        // periodic: a loop straddling the seam is in one piece, past one
        // end of the domain.
        let shifts = |p: Option<f64>| match p {
            Some(p) => vec![0.0, -p, p],
            None => vec![0.0],
        };
        shifts(self.periods[0]).into_iter().any(|du| {
            shifts(self.periods[1]).into_iter().any(|dv| {
                let q = [uv[0] + du, uv[1] + dv];
                self.polygons.iter().filter(|l| inside(q, l)).count() % 2 == 1
            })
        })
    }
}

/// Puts a [`CadGeometry`] together, and checks it when it is built.
#[derive(Clone, Debug, Default)]
pub struct CadBuilder {
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    faces: Vec<(i32, Surface, Vec<i32>, Trim)>,
}

impl CadBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertex(&mut self, tag: i32, point: [f64; 3]) -> &mut Self {
        self.vertices.push(Vertex { tag, point });
        self
    }

    pub fn edge(&mut self, tag: i32, curve: Curve, ends: [i32; 2]) -> &mut Self {
        self.edges.push(Edge { tag, curve, ends });
        self
    }

    pub fn face(&mut self, tag: i32, surface: Surface, edges: &[i32], trim: Trim) -> &mut Self {
        self.faces.push((tag, surface, edges.to_vec(), trim));
        self
    }

    pub fn build(&self) -> Result<CadGeometry, CadError> {
        repeated("vertex", self.vertices.iter().map(|v| v.tag))?;
        repeated("edge", self.edges.iter().map(|e| e.tag))?;
        repeated("face", self.faces.iter().map(|f| f.0))?;

        let span = span(&self.vertices, &self.edges, &self.faces);
        let tolerance = TOLERANCE * span.max(1.0);
        let vertex = |tag: i32| self.vertices.iter().find(|v| v.tag == tag);
        let edge = |tag: i32| self.edges.iter().find(|e| e.tag == tag);

        for e in &self.edges {
            let (a, b) = e.curve.domain();
            for (end, t, tag) in [("start", a, e.ends[0]), ("end", b, e.ends[1])] {
                let v = vertex(tag).ok_or(CadError::NoVertex {
                    edge: e.tag,
                    vertex: tag,
                })?;
                let gap = apart(e.curve.eval(t), v.point);
                if gap > tolerance {
                    return Err(CadError::EndApart {
                        edge: e.tag,
                        end,
                        vertex: tag,
                        gap,
                    });
                }
            }
        }

        let mut faces = Vec::new();
        for (tag, surface, edges, trim) in &self.faces {
            let face = *tag;
            for &e in edges {
                edge(e).ok_or(CadError::NoEdge { face, edge: e })?;
            }
            let periods = periods(surface, tolerance);
            let mut polygons = Vec::new();
            if let Trim::Loops(loops) = trim {
                for (k, l) in loops.iter().enumerate() {
                    if l.is_empty() {
                        return Err(CadError::EmptyLoop { face });
                    }
                    for &(e, _) in l {
                        if !edges.contains(&e) {
                            return Err(CadError::NotBounding { face, edge: e });
                        }
                    }
                    // Each edge begins where the one before it ends.
                    let oriented = |&(e, back): &(i32, bool)| {
                        let ends = edge(e).expect("checked above").ends;
                        if back {
                            [ends[1], ends[0]]
                        } else {
                            ends
                        }
                    };
                    for (i, step) in l.iter().enumerate() {
                        let before = &l[(i + l.len() - 1) % l.len()];
                        if oriented(before)[1] != oriented(step)[0] {
                            return Err(CadError::OpenLoop {
                                face,
                                ring: k,
                                edge: step.0,
                            });
                        }
                    }
                    let polygon =
                        polygon(surface, periods, l, |e| &edge(e).expect("checked").curve)
                            .ok_or(CadError::Unclosed { face, ring: k })?;
                    polygons.push(polygon);
                }
            }
            faces.push(Face {
                tag: face,
                surface: surface.clone(),
                edges: edges.clone(),
                trim: trim.clone(),
                polygons,
                periods,
            });
        }

        let mut geometry = CadGeometry {
            vertices: self.vertices.clone(),
            edges: self.edges.clone(),
            faces,
            containment: BTreeMap::new(),
            span,
        };
        geometry.containment = containment(&geometry);
        Ok(geometry)
    }
}

/// A boundary representation with NURBS curves and surfaces: see the
/// [module](self).
#[derive(Clone, Debug)]
pub struct CadGeometry {
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    faces: Vec<Face>,
    /// Everything containing each entity, itself first.
    containment: BTreeMap<(u8, i32), Vec<(u8, i32)>>,
    span: f64,
}

impl CadGeometry {
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    pub fn vertex(&self, tag: i32) -> Option<&Vertex> {
        self.vertices.iter().find(|v| v.tag == tag)
    }

    pub fn edge(&self, tag: i32) -> Option<&Edge> {
        self.edges.iter().find(|e| e.tag == tag)
    }

    pub fn face(&self, tag: i32) -> Option<&Face> {
        self.faces.iter().find(|f| f.tag == tag)
    }

    /// How big the geometry is, corner to corner, as for the faceted one:
    /// what a tolerance is a share of.
    pub fn span(&self) -> f64 {
        self.span
    }

    /// Everything containing `(dim, tag)`, itself first, or nothing when no
    /// entity answers to that pair.
    pub fn containing(&self, dim: u8, tag: i32) -> &[(u8, i32)] {
        self.containment
            .get(&(dim, tag))
            .map(|up| up.as_slice())
            .unwrap_or(&[])
    }

    /// The point of an entity nearest `p`, or nothing when no entity
    /// answers to `(dim, tag)`.
    pub fn project(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<[f64; 3]> {
        match dim {
            CORNER => self.vertex(tag).map(|v| v.point),
            CURVE => self.edge(tag).map(|e| e.curve.project(p).1),
            SURFACE => {
                let face = self.face(tag)?;
                let (uv, q, _) = face.surface.project(p);
                if face.contains(uv) {
                    return Some(q);
                }
                // Outside the trim: the nearest point of the boundary.
                face.edges
                    .iter()
                    .filter_map(|&e| self.edge(e))
                    .map(|e| e.curve.project(p))
                    .min_by(|a, b| a.2.partial_cmp(&b.2).expect("a finite distance"))
                    .map(|(_, q, _)| q)
                    .or(Some(q))
            }
            _ => None,
        }
    }

    /// The parameters of the point of a face nearest `p` — on its surface,
    /// whether or not inside the trim — and whether the face contains it.
    pub fn parameters(&self, face: i32, p: [f64; 3]) -> Option<([f64; 2], bool)> {
        let face = self.face(face)?;
        let (uv, _, _) = face.surface.project(p);
        Some((uv, face.contains(uv)))
    }

    /// How far `p` is from an entity.
    pub fn distance(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<f64> {
        self.project(dim, tag, p).map(|q| apart(p, q))
    }

    /// The entity to classify a point on, or nothing when it is near none:
    /// the lowest dimension with anything within its tolerance, as for the
    /// faceted geometry.
    pub fn nearest(&self, p: [f64; 3], tolerance: [f64; 3]) -> Option<(u8, i32)> {
        for dim in [CORNER, CURVE, SURFACE] {
            let tags: Vec<i32> = match dim {
                CORNER => self.vertices.iter().map(|v| v.tag).collect(),
                CURVE => self.edges.iter().map(|e| e.tag).collect(),
                _ => self.faces.iter().map(|f| f.tag).collect(),
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

fn repeated(kind: &'static str, tags: impl Iterator<Item = i32>) -> Result<(), CadError> {
    let mut seen = BTreeSet::new();
    for tag in tags {
        if !seen.insert(tag) {
            return Err(CadError::Repeated { kind, tag });
        }
    }
    Ok(())
}

/// The diagonal of what the shapes cover, from a sampling of each.
fn span(vertices: &[Vertex], edges: &[Edge], faces: &[(i32, Surface, Vec<i32>, Trim)]) -> f64 {
    let mut points: Vec<[f64; 3]> = vertices.iter().map(|v| v.point).collect();
    for e in edges {
        points.extend(e.curve.sample(33));
    }
    for f in faces {
        points.extend(f.1.sample([17, 17]).into_iter().flatten());
    }
    if points.is_empty() {
        return 0.0;
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &points {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (0..3)
        .map(|k| (hi[k] - lo[k]) * (hi[k] - lo[k]))
        .sum::<f64>()
        .sqrt()
}

/// Where a surface closes up on itself: along `u` when its first and last
/// iso-curves of constant `u` coincide, the same for `v`.
fn periods(s: &Surface, tolerance: f64) -> [Option<f64>; 2] {
    let [(u0, u1), (v0, v1)] = s.domain();
    let across = |k: usize| (0..=8).map(move |i| i as f64 / 8.0).map(move |t| (k, t));
    let closes = |k: usize| {
        across(k).all(|(k, t)| {
            let (a, b) = if k == 0 {
                let v = v0 + (v1 - v0) * t;
                (s.eval(u0, v), s.eval(u1, v))
            } else {
                let u = u0 + (u1 - u0) * t;
                (s.eval(u, v0), s.eval(u, v1))
            };
            apart(a, b) <= tolerance
        })
    };
    // A sphere's poles do not make its meridian direction periodic: its
    // two ends there are two different points, not one curve twice.
    [closes(0).then_some(u1 - u0), closes(1).then_some(v1 - v0)]
}

/// A loop's polygon in a surface's parameters, or nothing when it does not
/// close there.
fn polygon<'a>(
    surface: &Surface,
    periods: [Option<f64>; 2],
    l: &[(i32, bool)],
    curve: impl Fn(i32) -> &'a Curve,
) -> Option<Vec<[f64; 2]>> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for &(e, back) in l {
        let mut samples = curve(e).sample(LOOP_SAMPLES + 1);
        if back {
            samples.reverse();
        }
        // The last sample is where the next edge begins.
        samples.pop();
        for p in samples {
            let (mut uv, _, _) = surface.project(p);
            if let Some(prev) = out.last() {
                uv = unwrapped(uv, *prev, periods);
            }
            out.push(uv);
        }
    }
    // Closed, if the first point comes back to itself from the last.
    let first = *out.first()?;
    let back = unwrapped(first, *out.last()?, periods);
    let step = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs().max((a[1] - b[1]).abs());
    (step(back, first) <= 1e-9 * (1.0 + step(first, [0.0, 0.0]))).then_some(out)
}

/// `uv`, moved by whole periods to within half of one of `near`.
fn unwrapped(mut uv: [f64; 2], near: [f64; 2], periods: [Option<f64>; 2]) -> [f64; 2] {
    for k in 0..2 {
        if let Some(p) = periods[k] {
            while uv[k] - near[k] > p / 2.0 {
                uv[k] -= p;
            }
            while near[k] - uv[k] > p / 2.0 {
                uv[k] += p;
            }
        }
    }
    uv
}

/// Whether `q` is inside a polygon, by the crossings of a ray from it.
fn inside(q: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut odd = false;
    let n = polygon.len();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let x = a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if q[0] < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// Who contains whom, from the topology.
fn containment(g: &CadGeometry) -> BTreeMap<(u8, i32), Vec<(u8, i32)>> {
    let mut up: BTreeMap<(u8, i32), BTreeSet<(u8, i32)>> = BTreeMap::new();
    for v in &g.vertices {
        up.entry((CORNER, v.tag)).or_default();
    }
    for e in &g.edges {
        up.entry((CURVE, e.tag)).or_default();
        for v in e.ends {
            up.entry((CORNER, v)).or_default().insert((CURVE, e.tag));
        }
    }
    for f in &g.faces {
        up.entry((SURFACE, f.tag)).or_default();
        for &e in &f.edges {
            up.entry((CURVE, e)).or_default().insert((SURFACE, f.tag));
            if let Some(edge) = g.edge(e) {
                for v in edge.ends {
                    up.entry((CORNER, v)).or_default().insert((SURFACE, f.tag));
                }
            }
        }
    }
    up.into_iter()
        .map(|(me, others)| {
            let mut all = vec![me];
            all.extend(others);
            (me, all)
        })
        .collect()
}

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}
