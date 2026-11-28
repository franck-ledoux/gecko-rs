//! # gecko-geom
//!
//! The geometry a block structure is meant to follow, as against the block
//! structure itself.
//!
//! Two kinds of model. The one a session uses today is **faceted**: there is
//! no analytic surface anywhere, only triangles, segments and points read
//! from a gmsh mesh and grouped into the entities of a boundary
//! representation. That is what `FacetedGeometry` does in the C++
//! GECKO, and the grouping rule is taken from it verbatim — the two have to
//! agree about what a file means, or the same `.msh` describes two different
//! models.
//!
//! The other is **exact**: [`CadGeometry`], whose edges and faces are NURBS
//! curves and surfaces from the [`nurbs`] crate, read from a `.gcad` file
//! ([`gcad`]) or put together in code — a session using one comes next.
//!
//! Nothing here knows about blocks. The dependency runs the other way: a cell
//! of a block structure will eventually be *classified* onto one of these
//! entities, so the block structure gets to depend on the geometry and the
//! geometry gets to be testable on its own.

pub mod bounds;
pub mod cad;
pub mod composite;
pub mod describe;
pub mod gcad;
pub mod model;
pub mod msh;
pub mod quad;
mod reconstruction;
pub mod where_on;

pub use bounds::BoundsError;
pub use cad::{CadBuilder, CadError, CadGeometry, Edge, Face, Triangulation, Trim, Use, Vertex};
pub use composite::{Composites, Join, MergeError, Model, Smoothing};
pub use describe::{CurveFacts, CurveShape, SurfaceFacts, SurfaceShape};
pub use gcad::{GcadError, GcadInfo};
pub use model::{Corner, Curve, Geometry, Surface};
pub use msh::{Msh, MshError, PhysicalName, LINE, POINT, TRIANGLE};
/// The NURBS library the exact geometry is made of.
pub use nurbs;
pub use quad::{Quad, QuadError};
pub use where_on::{CORNER, CURVE, SURFACE};
