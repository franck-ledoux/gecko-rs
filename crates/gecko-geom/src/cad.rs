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
//! the geometry is built: from each edge's curve in those parameters, its
//! p-curve, which a CAD model gives for every edge of every face — or,
//! without one, by sampling the edge and projecting every sample onto the
//! surface. Where the surface is periodic — a cylinder round its
//! seam — consecutive samples are kept within half a period of each other,
//! so a loop straddling the seam stays in one piece.
//!
//! A point of the parameters is inside the trim when it is inside an odd
//! number of the loops — the outer one and not a hole — whichever way
//! round each was given.
//!
//! A loop that would not close in the parameters is refused rather than
//! guessed at. Projected, that is one going right round a periodic surface,
//! or one through a pole of a sphere, where one point of space is a whole
//! line of parameters: such faces are left untrimmed, or given their
//! p-curves, with which a CAD model's full sphere — a seam and two
//! degenerate edges at its poles — closes up like any other face.
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
    /// The whole domain of it is the edge. For a degenerate edge, a curve
    /// that stays at its vertex.
    pub curve: Curve,
    /// The vertex at the start of its domain, and the one at the end: the
    /// same one twice for a closed curve.
    pub ends: [i32; 2],
    /// A point in space, and a line in the parameters of the faces it
    /// bounds: the pole of a sphere, the apex of a cone.
    pub degenerate: bool,
    /// Points along it, for drawing it: none unless given.
    pub polyline: Vec<[f64; 3]>,
    /// The curve's parameter at each point of the polyline, when known:
    /// where a projection onto the edge starts from.
    pub params: Vec<f64>,
}

/// One edge of a loop, as the loop runs along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Use {
    pub edge: i32,
    /// Taken against its curve's direction.
    pub reversed: bool,
    /// The edge in the face's parameters, `(u, v)` as the first two
    /// coordinates of each control point, running the way the edge's own
    /// curve does. Given by a CAD model for every edge of every face, and
    /// what draws the loop when it is there; without it, the loop is found
    /// by projecting the edge onto the surface.
    pub pcurve: Option<Curve>,
}

impl Use {
    /// The edge, along its curve.
    pub fn along(edge: i32) -> Use {
        Use {
            edge,
            reversed: false,
            pcurve: None,
        }
    }

    /// The edge, against its curve.
    pub fn against(edge: i32) -> Use {
        Use {
            reversed: true,
            ..Use::along(edge)
        }
    }

    /// With its curve in the face's parameters.
    pub fn on(self, pcurve: Curve) -> Use {
        Use {
            pcurve: Some(pcurve),
            ..self
        }
    }
}

/// Which part of its surface a face is.
#[derive(Clone, Debug, PartialEq)]
pub enum Trim {
    /// All of it.
    Natural,
    /// What is inside the loops, each a list of edges end to end.
    Loops(Vec<Vec<Use>>),
}

/// Triangles drawing a face.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Triangulation {
    pub nodes: Vec<[f64; 3]>,
    /// The surface's parameters at each node, when known: where a
    /// projection onto the face starts from.
    pub uv: Vec<[f64; 2]>,
    /// Three indices into `nodes` each, wound outward from the solid.
    pub triangles: Vec<[u32; 3]>,
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
    /// Its outward normal is the surface's turned round.
    pub reversed: bool,
    /// Triangles drawing it, when given.
    pub triangulation: Option<Triangulation>,
    /// The loops in the surface's parameters, sampled when the geometry
    /// was built; none for a face left whole.
    rings: Vec<Ring>,
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
        if self.rings.is_empty() {
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
                within(q, &self.rings)
            })
        })
    }
}

/// Puts a [`CadGeometry`] together, and checks it when it is built.
#[derive(Clone, Debug, Default)]
pub struct CadBuilder {
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    /// Degenerate edges, at a vertex: their curve is made when the vertex
    /// is known.
    points: Vec<(i32, i32)>,
    faces: Vec<(i32, Surface, Vec<i32>, Trim)>,
    polylines: BTreeMap<i32, (Vec<[f64; 3]>, Vec<f64>)>,
    triangulations: BTreeMap<i32, Triangulation>,
    reversed: BTreeSet<i32>,
    tolerance: f64,
    deflection: f64,
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
        self.edges.push(Edge {
            tag,
            curve,
            ends,
            degenerate: false,
            polyline: Vec::new(),
            params: Vec::new(),
        });
        self
    }

    /// An edge that is a single point, at `vertex`: see [`Edge::degenerate`].
    pub fn degenerate_edge(&mut self, tag: i32, vertex: i32) -> &mut Self {
        self.points.push((tag, vertex));
        self
    }

    /// Points drawing an edge, and the curve's parameter at each when
    /// known (else nothing).
    pub fn polyline(&mut self, edge: i32, points: Vec<[f64; 3]>, params: Vec<f64>) -> &mut Self {
        self.polylines.insert(edge, (points, params));
        self
    }

    /// Triangles drawing a face.
    pub fn triangulation(&mut self, face: i32, triangulation: Triangulation) -> &mut Self {
        self.triangulations.insert(face, triangulation);
        self
    }

    /// A face whose outward normal is its surface's turned round.
    pub fn reversed(&mut self, face: i32) -> &mut Self {
        self.reversed.insert(face);
        self
    }

    /// How far the tessellation strays from the exact shapes, at most: what
    /// lets the faceting stand in for them when looking for what is near a
    /// point. Nothing, by default: then it never does.
    pub fn deflection(&mut self, deflection: f64) -> &mut Self {
        self.deflection = deflection;
        self
    }

    /// How far apart two points meant to be one may be — where an edge ends
    /// and its vertex, say. By default a billionth of the span; a CAD model
    /// says its own, usually far looser.
    pub fn tolerance(&mut self, tolerance: f64) -> &mut Self {
        self.tolerance = tolerance;
        self
    }

    pub fn face(&mut self, tag: i32, surface: Surface, edges: &[i32], trim: Trim) -> &mut Self {
        self.faces.push((tag, surface, edges.to_vec(), trim));
        self
    }

    pub fn build(&self) -> Result<CadGeometry, CadError> {
        repeated("vertex", self.vertices.iter().map(|v| v.tag))?;
        let vertex = |tag: i32| self.vertices.iter().find(|v| v.tag == tag);
        let mut all_edges = self.edges.clone();
        for &(tag, at) in &self.points {
            let v = vertex(at).ok_or(CadError::NoVertex {
                edge: tag,
                vertex: at,
            })?;
            all_edges.push(Edge {
                tag,
                curve: nurbs::shapes::line(v.point, v.point),
                ends: [at, at],
                degenerate: true,
                polyline: Vec::new(),
                params: Vec::new(),
            });
        }
        all_edges.sort_by_key(|e| e.tag);
        for e in &mut all_edges {
            if let Some((points, params)) = self.polylines.get(&e.tag) {
                e.polyline = points.clone();
                if params.len() == points.len() {
                    e.params = params.clone();
                }
            }
        }
        let all_edges = all_edges;
        repeated("edge", all_edges.iter().map(|e| e.tag))?;
        repeated("face", self.faces.iter().map(|f| f.0))?;

        let span = span(&self.vertices, &all_edges, &self.faces);
        let tolerance = (TOLERANCE * span.max(1.0)).max(self.tolerance);
        let edge = |tag: i32| all_edges.iter().find(|e| e.tag == tag);

        for e in &all_edges {
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
            // How far a loop's polygon may cut across the face's true
            // boundary, in space: a point that far inside the face is taken
            // for one outside it and put on the boundary — which, smoothing
            // a node near a rim round after round, pulls it onto the rim.
            let chord = if self.deflection > 0.0 {
                self.deflection / 100.0
            } else {
                1e-6 * span.max(1.0)
            }
            .max(tolerance);
            let mut rings = Vec::new();
            if let Trim::Loops(loops) = trim {
                for (k, l) in loops.iter().enumerate() {
                    if l.is_empty() {
                        return Err(CadError::EmptyLoop { face });
                    }
                    for u in l {
                        if !edges.contains(&u.edge) {
                            return Err(CadError::NotBounding { face, edge: u.edge });
                        }
                    }
                    // Each edge begins where the one before it ends.
                    let oriented = |u: &Use| {
                        let ends = edge(u.edge).expect("checked above").ends;
                        if u.reversed {
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
                                edge: step.edge,
                            });
                        }
                    }
                    let drawn = ring(surface, periods, l, chord, |e| {
                        &edge(e).expect("checked").curve
                    })
                    .ok_or(CadError::Unclosed { face, ring: k })?;
                    rings.push(drawn);
                }
            }
            faces.push(Face {
                tag: face,
                surface: surface.clone(),
                edges: edges.clone(),
                trim: trim.clone(),
                reversed: self.reversed.contains(&face),
                triangulation: self.triangulations.get(&face).cloned(),
                rings: sided(rings),
                periods,
            });
        }

        let mut geometry = CadGeometry {
            vertices: self.vertices.clone(),
            edges: all_edges,
            faces,
            containment: BTreeMap::new(),
            span,
            tolerance,
            deflection: self.deflection,
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
    /// How far apart two points meant to be one may be.
    tolerance: f64,
    /// How far the tessellation strays from the shapes, at most; nought
    /// when not known.
    deflection: f64,
}

impl CadGeometry {
    /// How far apart two points meant to be one may be: the model's own,
    /// or a billionth of its span, whichever is the larger.
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// How far the tessellation strays from the exact shapes, at most, or
    /// nought when that is not known.
    pub fn deflection(&self) -> f64 {
        self.deflection
    }

    /// The tessellation, as a faceted geometry with the same tags: what
    /// draws the model, and what everything that reads a faceting —
    /// describing, merging, a box round it — reads.
    ///
    /// Each face's triangles, each edge's polyline and each vertex, with
    /// the nodes they share in space merged into one: conformal, as a gmsh
    /// mesh of the model would be, provided the polylines are the nodes the
    /// faces' triangles have along each edge — which is how
    /// `scripts/step_to_gcad.py` writes them. Degenerate edges have no
    /// segments, and are not in it.
    ///
    /// Nothing at all for a model with no triangles to draw it.
    pub fn faceting(&self) -> Option<crate::Geometry> {
        self.facets()
    }

    /// The faceting, carrying the exact geometry with it
    /// ([`Geometry::exact`](crate::Geometry::exact)): what a session holds,
    /// drawn and described from its triangles, and projected onto exactly.
    pub fn into_geometry(self) -> Option<crate::Geometry> {
        let faceting = self.facets()?;
        Some(faceting.with_exact(self))
    }

    fn facets(&self) -> Option<crate::Geometry> {
        use crate::msh::{Element, Msh, LINE, POINT, TRIANGLE};
        let mut nodes = Merged::new(self.tolerance);
        let mut elements = Vec::new();
        for v in &self.vertices {
            let n = nodes.at(v.point);
            elements.push(Element {
                kind: POINT,
                physical: 0,
                entity: v.tag,
                nodes: [n, n, n],
            });
        }
        for e in self.edges.iter().filter(|e| !e.degenerate) {
            for w in e.polyline.windows(2) {
                let (a, b) = (nodes.at(w[0]), nodes.at(w[1]));
                if a != b {
                    elements.push(Element {
                        kind: LINE,
                        physical: 0,
                        entity: e.tag,
                        nodes: [a, b, b],
                    });
                }
            }
        }
        for f in &self.faces {
            let Some(t) = &f.triangulation else { continue };
            let here: Vec<u32> = t.nodes.iter().map(|p| nodes.at(*p)).collect();
            for tri in &t.triangles {
                let [a, b, c] = tri.map(|i| here[i as usize]);
                if a != b && b != c && a != c {
                    elements.push(Element {
                        kind: TRIANGLE,
                        physical: 0,
                        entity: f.tag,
                        nodes: [a, b, c],
                    });
                }
            }
        }
        let msh = Msh {
            nodes: nodes.points,
            elements,
            names: Vec::new(),
            skipped: 0,
        };
        crate::Geometry::of(&msh).ok()
    }

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
            CURVE => self.edge(tag).map(|e| on_edge(e, p).1),
            SURFACE => {
                let face = self.face(tag)?;
                let (uv, q, _) = on_face(face, p);
                if face.contains(uv) {
                    return Some(q);
                }
                // Outside the trim: the nearest point of the boundary.
                face.edges
                    .iter()
                    .filter_map(|&e| self.edge(e))
                    .map(|e| on_edge(e, p))
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
        let (uv, _, _) = on_face(face, p);
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

/// How many of the nearest points of a tessellation a projection starts
/// from: a few, in case the nearest is across a fold from the answer.
const SEEDS: usize = 3;

/// The indices of the `SEEDS` points nearest `p`, nearest first, no two
/// of them the same point of space: the apex of a cone is a node of every
/// triangle round it, each with parameters of its own, and three seeds
/// there are one seed — at the one point of the surface where its
/// parameters say nothing of which way to go.
fn nearest_of(points: &[[f64; 3]], p: [f64; 3]) -> Vec<usize> {
    let gap = |a: [f64; 3], b: [f64; 3]| {
        let d = [0, 1, 2].map(|k| a[k] - b[k]);
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    };
    // The nearest few, in one pass; all of them in order only when those
    // are too few distinct points — a pinch that many triangles share.
    let few = 4 * SEEDS;
    let mut order: Vec<(f64, usize)> = Vec::with_capacity(few + 1);
    for (i, q) in points.iter().enumerate() {
        let d = gap(*q, p);
        if order.len() < few || d < order[few - 1].0 {
            let at = order.partition_point(|(e, j)| (*e, *j) < (d, i));
            order.insert(at, (d, i));
            order.truncate(few);
        }
    }
    let distinct = |order: &[(f64, usize)]| {
        let mut chosen: Vec<usize> = Vec::with_capacity(SEEDS);
        for &(_, i) in order {
            let q = points[i];
            if chosen
                .iter()
                .all(|&j| gap(points[j], q) > 1e-24 * (1.0 + gap(q, [0.0; 3])))
            {
                chosen.push(i);
                if chosen.len() == SEEDS {
                    break;
                }
            }
        }
        chosen
    };
    let chosen = distinct(&order);
    if chosen.len() == SEEDS || order.len() == points.len() {
        return chosen;
    }
    let mut order: Vec<(f64, usize)> = points
        .iter()
        .enumerate()
        .map(|(i, q)| (gap(*q, p), i))
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    distinct(&order)
}

/// The point of an edge nearest `p`: from the nearest points of its
/// polyline when it has their parameters, and a search of the whole curve
/// otherwise.
fn on_edge(e: &Edge, p: [f64; 3]) -> (f64, [f64; 3], f64) {
    if e.params.is_empty() {
        return e.curve.project(p);
    }
    let seeds: Vec<f64> = nearest_of(&e.polyline, p)
        .into_iter()
        .map(|i| e.params[i])
        .collect();
    e.curve.project_from(p, &seeds)
}

/// The point of a face's surface nearest `p`, the same way: from the
/// nearest nodes of its triangles when it has their parameters.
fn on_face(f: &Face, p: [f64; 3]) -> ([f64; 2], [f64; 3], f64) {
    match &f.triangulation {
        Some(t) if !t.uv.is_empty() && t.uv.len() == t.nodes.len() => {
            // A CAD model's parameters of a periodic surface can be a period
            // off its domain: brought back into it, not clamped to its end.
            let domain = f.surface.domain();
            let wrapped = |mut uv: [f64; 2]| {
                for k in 0..2 {
                    if let Some(period) = f.periods[k] {
                        let (lo, hi) = [domain[0], domain[1]][k];
                        while uv[k] < lo {
                            uv[k] += period;
                        }
                        while uv[k] > hi {
                            uv[k] -= period;
                        }
                    }
                }
                uv
            };
            let seeds: Vec<[f64; 2]> = nearest_of(&t.nodes, p)
                .into_iter()
                .map(|i| wrapped(t.uv[i]))
                .collect();
            let found = f.surface.project_from(p, &seeds);
            // Landed where the surface pinches to a point — an apex, a
            // pole — whose parameters say nothing of the way off it: the
            // whole surface searched as well, and the nearer kept.
            let d = f.surface.derivatives(found.0[0], found.0[1], 1);
            let (su, sv) = (norm(d[1][0]), norm(d[0][1]));
            if su.min(sv) <= 1e-9 * su.max(sv) {
                let full = f.surface.project(p);
                if full.2 < found.2 {
                    return full;
                }
            }
            found
        }
        _ => f.surface.project(p),
    }
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Points, those within a tolerance of one another taken as one: a grid of
/// cells a tolerance wide, and a look in the neighbouring cells.
struct Merged {
    points: Vec<[f64; 3]>,
    cells: BTreeMap<[i64; 3], Vec<u32>>,
    tolerance: f64,
}

impl Merged {
    fn new(tolerance: f64) -> Self {
        Merged {
            points: Vec::new(),
            cells: BTreeMap::new(),
            tolerance: tolerance.max(1e-300),
        }
    }

    fn at(&mut self, p: [f64; 3]) -> u32 {
        let cell = p.map(|x| (x / self.tolerance).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let near = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    for &n in self.cells.get(&near).into_iter().flatten() {
                        if apart(self.points[n as usize], p) <= self.tolerance {
                            return n;
                        }
                    }
                }
            }
        }
        let n = self.points.len() as u32;
        self.points.push(p);
        self.cells.entry(cell).or_default().push(n);
        n
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

/// Which of a ring's p-curves a segment of its polygon follows, the curve's
/// parameters at the segment's two ends, and how far the polygon was moved
/// by whole periods there.
type Follows = (usize, f64, f64, [f64; 2]);

/// A loop in a surface's parameters: its polygon, and for each of its
/// segments the p-curve it follows, so that near the boundary the curve
/// itself can say which side a point is on.
#[derive(Clone, Debug, PartialEq)]
struct Ring {
    points: Vec<[f64; 2]>,
    /// For the segment from each point to the next: which of `curves` it
    /// follows, the curve's parameters at its two ends, and how far the
    /// polygon was moved by whole periods there. None for a loop found by
    /// projection, which has no curve to ask.
    along: Vec<Option<Follows>>,
    /// The p-curves, each with whether the loop runs against it.
    curves: Vec<(Curve, bool)>,
    /// How far the polygon strays from its curves at most, in the
    /// parameters: nearer the polygon than this, the curve decides.
    band: f64,
    /// Whether the face is on the left of the loop as it runs.
    left: bool,
}

/// A loop's ring in a surface's parameters, or nothing when it does not
/// close there.
fn ring<'a>(
    surface: &Surface,
    periods: [Option<f64>; 2],
    l: &[Use],
    chord: f64,
    curve: impl Fn(i32) -> &'a Curve,
) -> Option<Ring> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut along_: Vec<Option<Follows>> = Vec::new();
    let mut curves: Vec<(Curve, bool)> = Vec::new();
    let mut band = 0.0f64;
    for u in l {
        // In the parameters straight away, from the p-curve, as finely as
        // keeps every piece of the polygon within `chord` of the edge on
        // the surface; or projected there, point by point.
        let mut samples: Vec<([f64; 2], f64)> = match &u.pcurve {
            Some(c) => {
                let (samples, strays) = along(surface, c, chord);
                band = band.max(strays);
                curves.push((c.clone(), u.reversed));
                samples
            }
            None => curve(u.edge)
                .sample(LOOP_SAMPLES + 1)
                .into_iter()
                .map(|p| (surface.project(p).0, f64::NAN))
                .collect(),
        };
        if u.reversed {
            samples.reverse();
        }
        let which = u.pcurve.as_ref().map(|_| curves.len() - 1);
        // The last sample is where the next edge begins.
        for k in 0..samples.len().saturating_sub(1) {
            let (raw, t0) = samples[k];
            let t1 = samples[k + 1].1;
            let uv = match points.last() {
                Some(prev) => unwrapped(raw, *prev, periods),
                None => raw,
            };
            let moved = [uv[0] - raw[0], uv[1] - raw[1]];
            points.push(uv);
            along_.push(which.map(|c| (c, t0, t1, moved)));
        }
    }
    // Closed, if the first point comes back to itself from the last.
    let first = *points.first()?;
    let back = unwrapped(first, *points.last()?, periods);
    let step = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs().max((a[1] - b[1]).abs());
    if step(back, first) > 1e-9 * (1.0 + step(first, [0.0, 0.0])) {
        return None;
    }
    Some(Ring {
        points,
        along: along_,
        curves,
        // Four times what was measured at the middles of the pieces: a
        // piece's worst is near, not always at, its middle.
        band: 4.0 * band + 1e-12,
        left: true,
    })
}

/// A p-curve's points, from one end to the other, each with the curve's
/// parameter there: a fixed number to begin with, and each piece halved
/// while the surface's point at its middle is more than `chord` from the
/// surface's point at the middle of its chord. And how far, in the
/// parameters, the pieces kept stray from the curve at their middles.
fn along(surface: &Surface, c: &Curve, chord: f64) -> (Vec<([f64; 2], f64)>, f64) {
    let (a, b) = c.domain();
    let at = |t: f64| {
        let p = c.eval(t);
        [p[0], p[1]]
    };
    let on = |uv: [f64; 2]| surface.eval(uv[0], uv[1]);
    #[allow(clippy::too_many_arguments)]
    fn halve(
        t0: f64,
        t1: f64,
        depth: u32,
        at: &dyn Fn(f64) -> [f64; 2],
        on: &dyn Fn([f64; 2]) -> [f64; 3],
        chord: f64,
        out: &mut Vec<([f64; 2], f64)>,
        strays: &mut f64,
    ) {
        let (p0, p1) = (at(t0), at(t1));
        let tm = 0.5 * (t0 + t1);
        let middle = at(tm);
        let straight = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
        if depth < 12 && apart(on(middle), on(straight)) > chord {
            halve(t0, tm, depth + 1, at, on, chord, out, strays);
            halve(tm, t1, depth + 1, at, on, chord, out, strays);
        } else {
            let d = ((middle[0] - straight[0]).powi(2) + (middle[1] - straight[1]).powi(2)).sqrt();
            *strays = strays.max(d);
            out.push((p1, t1));
        }
    }
    let mut out = vec![(at(a), a)];
    let mut strays = 0.0;
    for i in 0..LOOP_SAMPLES {
        let t0 = a + (b - a) * i as f64 / LOOP_SAMPLES as f64;
        let t1 = a + (b - a) * (i + 1) as f64 / LOOP_SAMPLES as f64;
        halve(t0, t1, 0, &at, &on, chord, &mut out, &mut strays);
    }
    (out, strays)
}

/// Which side of its rings a point of the parameters is on: inside an odd
/// number of their polygons — and, nearer a polygon than it strays from its
/// curve, on the face's side of that curve.
fn within(q: [f64; 2], rings: &[Ring]) -> bool {
    let odd = rings.iter().filter(|r| inside(q, &r.points)).count() % 2 == 1;
    // The nearest segment of any ring.
    let mut near: Option<(f64, usize, usize)> = None;
    for (r, ring) in rings.iter().enumerate() {
        let n = ring.points.len();
        for k in 0..n {
            let d = to_segment(q, ring.points[k], ring.points[(k + 1) % n]);
            if near.is_none_or(|(best, _, _)| d < best) {
                near = Some((d, r, k));
            }
        }
    }
    let Some((d, r, k)) = near else { return odd };
    let ring = &rings[r];
    let Some((c, t0, t1, moved)) = ring.along[k] else {
        return odd;
    };
    if d > ring.band {
        return odd;
    }
    let (curve, back) = &ring.curves[c];
    let at = [q[0] - moved[0], q[1] - moved[1], 0.0];
    let (t, foot, _) = curve.project_from(at, &[t0, t1, 0.5 * (t0 + t1)]);
    let mut tangent = curve.derivatives(t, 1)[1];
    if *back {
        tangent = tangent.map(|x| -x);
    }
    let cross = tangent[0] * (at[1] - foot[1]) - tangent[1] * (at[0] - foot[0]);
    cross == 0.0 || (cross > 0.0) == ring.left
}

/// How far `q` is from the segment `a`–`b`, in the parameters.
fn to_segment(q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let len = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len > 0.0 {
        (((q[0] - a[0]) * ab[0] + (q[1] - a[1]) * ab[1]) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = [q[0] - a[0] - t * ab[0], q[1] - a[1] - t * ab[1]];
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

/// Which side of each ring the face is on: a step to the left of its first
/// segment, and whether the polygons have that point inside.
fn sided(mut rings: Vec<Ring>) -> Vec<Ring> {
    let lefts: Vec<bool> = rings
        .iter()
        .map(|ring| {
            let (a, b) = (ring.points[0], ring.points[1 % ring.points.len()]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-300);
            let step = 1e-6 * len;
            let probe = [
                0.5 * (a[0] + b[0]) - d[1] / len * step,
                0.5 * (a[1] + b[1]) + d[0] / len * step,
            ];
            rings.iter().filter(|r| inside(probe, &r.points)).count() % 2 == 1
        })
        .collect();
    for (ring, left) in rings.iter_mut().zip(lefts) {
        ring.left = left;
    }
    rings
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
