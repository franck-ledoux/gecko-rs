//! # gecko-core
//!
//! The block structure, as a 3-dimensional combinatorial map.
//!
//! Cells exist at all four dimensions — node, edge, face, block — and each one
//! has an identity of its own. That is the whole point of the representation:
//! only a cell that exists can carry a curve, a surface, or a classification,
//! and those are what let a block structure follow a geometry rather than
//! merely sit near it.

pub mod attributes;
pub mod blocking;
pub mod ids;
pub mod mesh;

pub use attributes::{BlockTag, EdgeTag, FaceTag, NodeTag, On};
pub use blocking::{
    across, best_pairing, cube_corners, flatness, mitre, quad_normal, scaled_jacobian, BlockFace,
    Blocking, BlockingError, ChordBlock, Settling, SheetEdge, Smoothing, Unmet, HEX_CORNER_EDGES,
    HEX_FACES, PILLOW_SHARE,
};
pub use ids::{CellId, CornerKey};
pub use mesh::{Mesh, SEGMENTS};
