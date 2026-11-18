//! # gecko-core
//!
//! The block structure, as a 3-dimensional combinatorial map.
//!
//! Cells exist at all four dimensions — node, edge, face, block — and each one
//! has an identity of its own. That is the whole point of the representation:
//! only a cell that exists can carry a curve, a surface, or a classification,
//! and those are what let a block structure follow a geometry rather than
//! merely sit near it.

#[cfg(test)]
mod hexahedron_tests;
#[cfg(test)]
mod naming_tests;

mod arena;
pub mod attributes;
pub mod blocking;
pub mod cells;
pub mod cellular;
pub mod ids;
pub mod mesh;

pub use attributes::{BlockTag, EdgeTag, FaceTag, NodeTag, On};
#[cfg(not(feature = "cellular"))]
pub use blocking::Blocking;
/// The vocabulary, which belongs to no engine: see [`cells`].
pub use cells::{
    across, best_pairing, cube_corners, flatness, mitre, quad_normal, scaled_jacobian, BlockFace,
    BlockingError, Census, ChordBlock, GroupMasks, Settling, Sheet, SheetCells, SheetEdge, Slab,
    Smoothing, Unmet, EDGE_AXIS, FACE_AXIS, HEX_CORNER_EDGES, HEX_EDGES, HEX_FACES, OPPOSITE,
    PILLOW_SHARE, SLAB_SHARE,
};
#[cfg(feature = "cellular")]
pub use cellular::Blocking;
pub use ids::{CellId, CornerKey};
pub use mesh::{Mesh, SEGMENTS};
