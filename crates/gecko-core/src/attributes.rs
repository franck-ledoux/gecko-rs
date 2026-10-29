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
    /// How many segments this edge is divided into when the blocking is
    /// meshed. **0 means nobody has said**, and the mesher uses its
    /// default.
    ///
    /// Stored per edge and read per **sheet**: every edge of a sheet has to
    /// carry the same number or the mesh is not conforming, so setting one
    /// sets them all, and the mesher resolves what it finds to a single
    /// number per sheet whatever happened in between.
    pub segments: u32,
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
    /// Which groups this block is in, one bit each — so sixty-four of
    /// them, and the session refuses the sixty-fifth rather than losing
    /// one quietly.
    ///
    /// On the block and nowhere else. A face, an edge or a node is in a
    /// group when a block it bounds is, which is **computed** rather than
    /// marked: one place to write means no second place to forget. And
    /// the map carries this through its own operations for free — see the
    /// merge and the split below, which is the whole reason it lives here
    /// rather than in a list of members kept beside the model.
    pub groups: u64,
}

impl AttributeUpdate for BlockTag {
    /// Two blocks becoming one. The first name survives, as everywhere
    /// else; the groups are **both** blocks', because a block that is
    /// part of the fluid and part of the pipe is part of both however it
    /// came to be one block.
    fn merge(a: Self, b: Self) -> Result<Self, AttributeError> {
        Ok(Self {
            id: a.id,
            groups: a.groups | b.groups,
        })
    }

    /// One block becoming two. The name is cleared on both sides, since
    /// neither half may go on claiming it; the groups are **copied**, for
    /// the reason a node's classification is: cutting a block of the fluid
    /// in two gives two blocks of the fluid, and a cut is not a statement
    /// about what anything is part of.
    fn split(a: Self) -> Result<(Self, Self), AttributeError> {
        let halved = Self {
            id: CellId::NONE,
            groups: a.groups,
        };
        Ok((halved, halved))
    }
}

impl AttributeBind for BlockTag {
    type StorageType = AttrSparseVec<Self>;
    type IdentifierType = VolumeIdType;
    const BIND_POLICY: OrbitPolicy = OrbitPolicy::Volume;
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

classified_cell!(FaceTag, FaceIdType, OrbitPolicy::Face);

impl AttributeUpdate for EdgeTag {
    /// As [`FaceTag`]'s, and the discretisation besides.
    ///
    /// Two edges becoming one take the **larger** of the two numbers, 0
    /// meaning nobody said. The finer of two answers is the safe one: it
    /// is the one that can represent what either side was asking for.
    fn merge(a: Self, b: Self) -> Result<Self, AttributeError> {
        Ok(Self {
            id: a.id,
            on: a.on.or(b.on),
            segments: a.segments.max(b.segments),
        })
    }

    /// One edge becoming two. Both halves keep the number, so cutting a
    /// sheet doubles the nodes along that direction — which is what
    /// cutting it is for.
    fn split(a: Self) -> Result<(Self, Self), AttributeError> {
        let fresh = Self {
            id: CellId::NONE,
            on: a.on,
            segments: a.segments,
        };
        Ok((fresh, fresh))
    }
}

impl AttributeBind for EdgeTag {
    type StorageType = AttrSparseVec<Self>;
    type IdentifierType = EdgeIdType;
    const BIND_POLICY: OrbitPolicy = OrbitPolicy::Edge;
}
