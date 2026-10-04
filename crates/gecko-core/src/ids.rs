//! The two identities a cell carries, and why one is not enough.
//!
//! A combinatorial map names its cells by darts: `vertex_id`, `edge_id`,
//! `face_id` and `volume_id` all return the smallest dart of an orbit. That is
//! cheap and it is exactly what topological code wants — and it is useless as a
//! name to hold on to, because every insertion and removal rewrites it.
//!
//! So cells carry names of their own, and there are two of them because they
//! answer two different questions.

use std::fmt;

/// A name a cell keeps for as long as it exists, unique among cells of its own
/// dimension and **never reused**.
///
/// This is what a caller holds between operations. It is deliberately cleared
/// when a cell is split: what comes out of a split is a new cell with a new
/// name, not a second claimant to an old one. That uniqueness is the whole
/// value of it, so nothing is exempt.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId(u32);

/// Which *corner* a node is, as against which node it is ([`CellId`]).
///
/// The two are different questions and only one field each can answer. A map
/// splits a node's attribute when the vertex orbit behind it comes apart —
/// deleting a neighbouring block does exactly that: what was one attribute
/// becomes two, and every cell that had that corner keeps one of them. Nothing
/// was cut in two; geometrically, and to those cells, it is still the same
/// corner. So the key is copied across and the cells go on recognising it,
/// while the [`CellId`] is cleared.
///
/// The price is the mirror image: a key is unique only *within a cell*, two
/// copies of one original node never being two corners of the same cell. That
/// is all anything asks of it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CornerKey(u32);

macro_rules! sentinel_id {
    ($t:ty, $label:literal) => {
        impl $t {
            /// Not named yet.
            pub const NONE: Self = Self(u32::MAX);

            pub(crate) fn new(raw: u32) -> Self {
                debug_assert!(raw != u32::MAX, concat!($label, " counter exhausted"));
                Self(raw)
            }

            pub fn is_none(self) -> bool {
                self == Self::NONE
            }

            pub fn is_some(self) -> bool {
                !self.is_none()
            }

            /// The raw number, or `None` while the cell is still unnamed.
            pub fn get(self) -> Option<u32> {
                self.is_some().then_some(self.0)
            }
        }

        impl Default for $t {
            fn default() -> Self {
                Self::NONE
            }
        }

        impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.get() {
                    Some(n) => write!(f, concat!($label, "({})"), n),
                    None => write!(f, concat!($label, "(none)")),
                }
            }
        }
    };
}

sentinel_id!(CellId, "CellId");
sentinel_id!(CornerKey, "CornerKey");

/// A monotonic source of names. It only ever counts up: a number handed out
/// once is never handed out again, which is what makes a [`CellId`] safe to
/// hold across edits.
#[derive(Clone, Debug, Default)]
pub(crate) struct Counter(u32);

impl Counter {
    pub(crate) fn next_cell(&mut self) -> CellId {
        let id = CellId::new(self.0);
        self.0 += 1;
        id
    }

    pub(crate) fn next_corner(&mut self) -> CornerKey {
        let key = CornerKey::new(self.0);
        self.0 += 1;
        key
    }

    pub(crate) fn handed_out(&self) -> u32 {
        self.0
    }
}
