//! # gecko-core
//!
//! The block structure, as an explicit complex of cells.
//!
//! Cells exist at all four dimensions — node, edge, face, block — and each one
//! has an identity of its own. That is the whole point of the representation:
//! only a cell that exists can carry a curve, a surface, or a classification,
//! and those are what let a block structure follow a geometry rather than
//! merely sit near it.
//!
//! It was a 3-dimensional combinatorial map until 0.2.0, and
//! `docs/developer-guide/cellular.md` says why it is not any more: a cell
//! attribute was paid for by the dart, at some twenty thousand bytes a
//! block, and the one extension this project is heading for is attaching a
//! curve to every edge.

mod arena;
pub mod blocking;
pub mod cells;
pub mod ids;
pub mod mesh;
pub mod quality;
pub mod smoothing;

pub use blocking::Blocking;
/// The vocabulary: the plain data the structure is spoken in. See [`cells`].
pub use cells::{
    across, best_pairing, cube_corners, flatness, mitre, quad_normal, scaled_jacobian, BlockFace,
    BlockingError, Census, ChordBlock, GroupMasks, On, Pulled, Settling, Sheet, SheetCells,
    SheetEdge, Slab, Smoothing, Unmet, EDGE_AXIS, FACE_AXIS, HEX_CORNER_EDGES, HEX_EDGES,
    HEX_FACES, OPPOSITE, PILLOW_SHARE, SLAB_SHARE,
};
pub use ids::CellId;
pub use mesh::{Mesh, SEGMENTS};
pub use quality::{hex_aspect_ratio, hex_quality, hex_skewness, summarize, Metric, Summary};
