//! What the map stores on each cell.
//!
//! These are deliberately tiny. `AttributeUpdate` requires `Copy`, so nothing
//! of variable size can live here — no *list* of classification targets, no
//! vector of control points. That constraint is not in the way; it is the
//! design. An attribute carries a **stable name** and a handful of fixed
//! fields, and everything that grows hangs off that name in tables beside
//! the map.
//!
//! One of those fixed fields is [`On`]: which geometric entity a cell is
//! classified on. One entity, not a set of them — which the `Copy`
//! requirement all but settles, and which costs nothing, because what a set
//! would be *for* is reading off the entities that contain the one named
//! here, and the geometry can answer that from the name alone.
//!
//! The `merge` and `split` behaviour below is the whole contract with the map,
//! and it is where this kind of code goes wrong, so it is spelled out rather
//! than defaulted.

use honeycomb_core::attributes::{AttrSparseVec, AttributeBind, AttributeError, AttributeUpdate};
use honeycomb_core::cmap::{EdgeIdType, FaceIdType, OrbitPolicy, VertexIdType, VolumeIdType};

use crate::ids::{CellId, CornerKey};

/// Which geometric entity a cell is classified on.
///
/// `dim` is 0 for a corner, 1 for a curve, 2 for a surface, and `tag` is the
/// gmsh entity tag — the geometry's own name for it. Deliberately plain
/// numbers: the block structure has no business knowing what a triangle is,
/// and the crate that does the projecting is the one that holds both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct On {
    pub dim: u8,
    pub tag: i32,
}

/// Attribute of a 0-cell: a node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeTag {
    pub id: CellId,
    pub corner_key: CornerKey,
    /// The geometric entity this node is classified on, if any.
    pub on: Option<On>,
    /// Frozen nodes do not move, for anybody, until they are thawed.
    pub frozen: bool,
}

/// Attribute of a 1-cell: an edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdgeTag {
    pub id: CellId,
    /// The geometric entity this edge is classified on, if any. Never set
    /// by measuring the edge: it is inferred from its two ends.
    pub on: Option<On>,
}

/// Attribute of a 2-cell: a face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaceTag {
    pub id: CellId,
    /// The geometric entity this face is classified on, if any. Inferred
    /// from its four edges, never measured.
    pub on: Option<On>,
}

/// Attribute of a 3-cell: a block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockTag {
    pub id: CellId,
}

impl AttributeUpdate for NodeTag {
    /// Two nodes becoming one. The first name survives and the second is
    /// dropped: a merge does not create a cell, so it must not consume a new
    /// name, and one of the two existing names has to win.
    ///
    /// The classification and the freeze do **not** follow that rule, and
    /// deliberately. A merge is two nodes becoming one *point*: if either of
    /// them was pinned to the geometry then the point that remains is on the
    /// geometry, and if either was frozen then somebody asked for it not to
    /// move. Taking the first blindly would quietly throw away a constraint
    /// nobody withdrew. When both are classified and disagree, the first
    /// wins, as its name does.
    fn merge(a: Self, b: Self) -> Result<Self, AttributeError> {
        Ok(Self {
            id: a.id,
            corner_key: a.corner_key,
            on: a.on.or(b.on),
            frozen: a.frozen || b.frozen,
        })
    }

    /// One node becoming two.
    ///
    /// The name is **cleared** on both sides — neither copy may keep claiming
    /// the original's identity — and a later `assign_missing_ids` hands out
    /// fresh ones. The corner key is **copied** instead, because a split orbit
    /// is still the same corner to every cell that had it, and those cells have
    /// to go on recognising it.
    ///
    /// The classification is copied for the same reason, and it is a
    /// different reason from the name's: where a cell *is* is not a claim
    /// about which cell it is, and a split moves nothing. Both halves of a
    /// node on a curve are on that curve. So is the freeze.
    fn split(a: Self) -> Result<(Self, Self), AttributeError> {
        let kept = Self {
            id: CellId::NONE,
            corner_key: a.corner_key,
            on: a.on,
            frozen: a.frozen,
        };
        Ok((kept, kept))
    }
}

impl AttributeBind for NodeTag {
    type StorageType = AttrSparseVec<Self>;
    type IdentifierType = VertexIdType;
    const BIND_POLICY: OrbitPolicy = OrbitPolicy::Vertex;
}

/// The same two rules for the cells that carry a classification as well as
/// a name: a merge keeps one from either side, a split keeps it on both.
///
/// A cell cut in two gives two new cells — new names, and the same
/// classification, because both halves lie where the whole did. A later
/// pass re-infers it from the boundary either way; this is what keeps the
/// picture right in between.
macro_rules! classified_cell {
    ($t:ty, $id:ty, $policy:expr) => {
        impl AttributeUpdate for $t {
            fn merge(a: Self, b: Self) -> Result<Self, AttributeError> {
                Ok(Self {
                    id: a.id,
                    on: a.on.or(b.on),
                })
            }

            fn split(a: Self) -> Result<(Self, Self), AttributeError> {
                let fresh = Self {
                    id: CellId::NONE,
                    on: a.on,
                };
                Ok((fresh, fresh))
            }
        }

        impl AttributeBind for $t {
            type StorageType = AttrSparseVec<Self>;
            type IdentifierType = $id;
            const BIND_POLICY: OrbitPolicy = $policy;
        }
    };
}

classified_cell!(EdgeTag, EdgeIdType, OrbitPolicy::Edge);
classified_cell!(FaceTag, FaceIdType, OrbitPolicy::Face);

/// The same rule for the cells that have only a name: keep the first on a
/// merge, clear both on a split.
macro_rules! named_cell {
    ($t:ty, $id:ty, $policy:expr) => {
        impl AttributeUpdate for $t {
            fn merge(a: Self, _b: Self) -> Result<Self, AttributeError> {
                Ok(a)
            }

            fn split(_: Self) -> Result<(Self, Self), AttributeError> {
                let fresh = Self { id: CellId::NONE };
                Ok((fresh, fresh))
            }
        }

        impl AttributeBind for $t {
            type StorageType = AttrSparseVec<Self>;
            type IdentifierType = $id;
            const BIND_POLICY: OrbitPolicy = $policy;
        }
    };
}

named_cell!(BlockTag, VolumeIdType, OrbitPolicy::Volume);
