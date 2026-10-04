//! What the map stores on each cell.
//!
//! These are deliberately tiny. `AttributeUpdate` requires `Copy`, so nothing
//! of variable size can live here — no list of classification targets, no
//! vector of control points. That constraint is not in the way; it is the
//! design. An attribute carries only a **stable name**, and everything that
//! grows hangs off that name in tables beside the map.
//!
//! The `merge` and `split` behaviour below is the whole contract with the map,
//! and it is where this kind of code goes wrong, so it is spelled out rather
//! than defaulted.

use honeycomb_core::attributes::{AttrSparseVec, AttributeBind, AttributeError, AttributeUpdate};
use honeycomb_core::cmap::{EdgeIdType, FaceIdType, OrbitPolicy, VertexIdType, VolumeIdType};

use crate::ids::{CellId, CornerKey};

/// Attribute of a 0-cell: a node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeTag {
    pub id: CellId,
    pub corner_key: CornerKey,
}

/// Attribute of a 1-cell: an edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdgeTag {
    pub id: CellId,
}

/// Attribute of a 2-cell: a face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaceTag {
    pub id: CellId,
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
    fn merge(a: Self, _b: Self) -> Result<Self, AttributeError> {
        Ok(a)
    }

    /// One node becoming two.
    ///
    /// The name is **cleared** on both sides — neither copy may keep claiming
    /// the original's identity — and a later `assign_missing_ids` hands out
    /// fresh ones. The corner key is **copied** instead, because a split orbit
    /// is still the same corner to every cell that had it, and those cells have
    /// to go on recognising it.
    fn split(a: Self) -> Result<(Self, Self), AttributeError> {
        let kept = Self {
            id: CellId::NONE,
            corner_key: a.corner_key,
        };
        Ok((kept, kept))
    }
}

impl AttributeBind for NodeTag {
    type StorageType = AttrSparseVec<Self>;
    type IdentifierType = VertexIdType;
    const BIND_POLICY: OrbitPolicy = OrbitPolicy::Vertex;
}

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

named_cell!(EdgeTag, EdgeIdType, OrbitPolicy::Edge);
named_cell!(FaceTag, FaceIdType, OrbitPolicy::Face);
named_cell!(BlockTag, VolumeIdType, OrbitPolicy::Volume);
