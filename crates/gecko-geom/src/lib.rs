//! # gecko-geom
//!
//! The geometry a block structure is meant to follow, as against the block
//! structure itself.
//!
//! It is **faceted**: there is no analytic surface anywhere, only triangles,
//! segments and points read from a gmsh mesh and grouped into the entities of
//! a boundary representation. That is what `FacetedGeometry` does in the C++
//! GECKO, and the grouping rule is taken from it verbatim — the two have to
//! agree about what a file means, or the same `.msh` describes two different
//! models.
//!
//! Nothing here knows about blocks. The dependency runs the other way: a cell
//! of a block structure will eventually be *classified* onto one of these
//! entities, so the block structure gets to depend on the geometry and the
//! geometry gets to be testable on its own.

pub mod bounds;
pub mod model;
pub mod msh;

pub use bounds::{BoundsError, Frame};
pub use model::{Corner, Curve, Geometry, Surface};
pub use msh::{Msh, MshError, PhysicalName, LINE, POINT, TRIANGLE};
