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

pub use attributes::{BlockTag, EdgeTag, FaceTag, NodeTag};
pub use blocking::{cube_corners, BlockFace, Blocking, BlockingError, HEX_FACES};
pub use ids::{CellId, CornerKey};
