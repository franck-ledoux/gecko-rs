//! The `.gcad` file: an exact geometry, as `scripts/step_to_gcad.py` writes
//! one from a STEP file.
//!
//! JSON, one entity to a line, so that two imports of a part can be diffed.
//! At the top, what it is and how it was made:
//!
//! ```text
//! "format": "gcad", "version": 1,
//! "source": "B10.step", "units": "mm",
//! "deflection": 0.0173,   how far the tessellation strays from the shapes
//! "tolerance": 1e-5,      how far apart the model's "same point" may be
//! ```
//!
//! Then the entities, each with a tag from one:
//!
//! - a **vertex**: `{"tag", "point": [x, y, z]}`;
//! - an **edge**: `{"tag", "ends": [vertex, vertex], "curve", "polyline"}`,
//!   with `curve` null for a degenerate edge — a pole, an apex;
//! - a **face**: `{"tag", "reversed", "surface", "uv", "edges", "loops",
//!   "triangulation"}`, where `edges` lists every edge bounding it, each
//!   loop is a list of `{"edge", "reversed", "pcurve"}`, and
//!   `triangulation` is `{"nodes": [[x, y, z]], "triangles": [[i, j, k]]}`
//!   indexed from nought.
//!
//! A curve is `{"degree", "knots", "points", "weights"}`, the knots in full,
//! each as many times as its multiplicity; a p-curve the same with points
//! `[u, v]`. A surface is `{"degree": [p, q], "knots": [u, v], "points",
//! "weights"}`, points and weights a row per control point along `u`.
//!
//! A file of another format, or of a version this does not know, is refused
//! by name: read as if it were this one, it would be a wrong geometry rather
//! than an error.

use crate::cad::{CadBuilder, CadError, CadGeometry, Triangulation, Trim, Use};
use nurbs::{Curve, NurbsError, Surface};
use serde::Deserialize;
use thiserror::Error;

/// The version of the format this reads.
pub const VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum GcadError {
    #[error("not a .gcad file: {0}")]
    Json(String),
    #[error("not a .gcad file: its format is {0:?}")]
    Format(String),
    #[error(
        "a .gcad file of version {0}, and this reads version {VERSION}: \
         import the STEP file again, or update gecko"
    )]
    Version(u32),
    #[error("{what} {tag}: {error}")]
    Nurbs {
        what: &'static str,
        tag: i32,
        error: NurbsError,
    },
    #[error(transparent)]
    Cad(#[from] CadError),
}

/// What a `.gcad` file says about itself, besides its geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct GcadInfo {
    pub source: String,
    pub units: String,
    pub deflection: f64,
    pub tolerance: f64,
}

#[derive(Deserialize)]
struct File {
    format: String,
    version: u32,
    #[serde(default)]
    source: String,
    #[serde(default)]
    units: String,
    deflection: f64,
    tolerance: f64,
    vertices: Vec<VertexJson>,
    edges: Vec<EdgeJson>,
    faces: Vec<FaceJson>,
}

#[derive(Deserialize)]
struct VertexJson {
    tag: i32,
    point: [f64; 3],
}

#[derive(Deserialize)]
struct EdgeJson {
    tag: i32,
    ends: [i32; 2],
    curve: Option<CurveJson>,
    #[serde(default)]
    polyline: Vec<[f64; 3]>,
}

#[derive(Deserialize)]
struct CurveJson {
    degree: usize,
    knots: Vec<f64>,
    points: Vec<Vec<f64>>,
    weights: Vec<f64>,
}

#[derive(Deserialize)]
struct SurfaceJson {
    degree: [usize; 2],
    knots: [Vec<f64>; 2],
    points: Vec<Vec<[f64; 3]>>,
    weights: Vec<Vec<f64>>,
}

#[derive(Deserialize)]
struct UseJson {
    edge: i32,
    reversed: bool,
    pcurve: Option<CurveJson>,
}

#[derive(Deserialize)]
struct TriangulationJson {
    nodes: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

#[derive(Deserialize)]
struct FaceJson {
    tag: i32,
    #[serde(default)]
    reversed: bool,
    surface: SurfaceJson,
    edges: Vec<i32>,
    loops: Vec<Vec<UseJson>>,
    triangulation: Option<TriangulationJson>,
}

impl CurveJson {
    /// A curve, its points in 3D — a p-curve's `[u, v]` with nought after.
    fn curve(self) -> Result<Curve, NurbsError> {
        let points = self
            .points
            .iter()
            .map(|p| {
                [
                    p.first().copied().unwrap_or(0.0),
                    p.get(1).copied().unwrap_or(0.0),
                    p.get(2).copied().unwrap_or(0.0),
                ]
            })
            .collect();
        Curve::new(self.degree, self.knots, points, self.weights)
    }
}

/// Read a `.gcad` file: its geometry, and what it says about itself.
pub fn read(text: &str) -> Result<(CadGeometry, GcadInfo), GcadError> {
    let file: File = serde_json::from_str(text).map_err(|e| GcadError::Json(e.to_string()))?;
    if file.format != "gcad" {
        return Err(GcadError::Format(file.format));
    }
    if file.version != VERSION {
        return Err(GcadError::Version(file.version));
    }
    let mut b = CadBuilder::new();
    // Twice the model's own: an edge's end and its vertex are each within
    // it of where they are meant to be.
    b.tolerance(2.0 * file.tolerance);
    for v in &file.vertices {
        b.vertex(v.tag, v.point);
    }
    for e in file.edges {
        match e.curve {
            Some(c) => {
                let curve = c.curve().map_err(|error| GcadError::Nurbs {
                    what: "edge",
                    tag: e.tag,
                    error,
                })?;
                b.edge(e.tag, curve, e.ends);
            }
            None => {
                b.degenerate_edge(e.tag, e.ends[0]);
            }
        }
        if !e.polyline.is_empty() {
            b.polyline(e.tag, e.polyline);
        }
    }
    for f in file.faces {
        let nurbs = |error| GcadError::Nurbs {
            what: "face",
            tag: f.tag,
            error,
        };
        let s = f.surface;
        let surface = Surface::new(s.degree, s.knots, s.points, s.weights).map_err(nurbs)?;
        let mut loops = Vec::new();
        for l in f.loops {
            let mut uses = Vec::new();
            for u in l {
                let mut taken = if u.reversed {
                    Use::against(u.edge)
                } else {
                    Use::along(u.edge)
                };
                if let Some(c) = u.pcurve {
                    taken = taken.on(c.curve().map_err(nurbs)?);
                }
                uses.push(taken);
            }
            loops.push(uses);
        }
        let trim = if loops.is_empty() {
            Trim::Natural
        } else {
            Trim::Loops(loops)
        };
        b.face(f.tag, surface, &f.edges, trim);
        if f.reversed {
            b.reversed(f.tag);
        }
        if let Some(t) = f.triangulation {
            b.triangulation(
                f.tag,
                Triangulation {
                    nodes: t.nodes,
                    triangles: t.triangles,
                },
            );
        }
    }
    let info = GcadInfo {
        source: file.source,
        units: file.units,
        deflection: file.deflection,
        tolerance: file.tolerance,
    };
    Ok((b.build()?, info))
}
