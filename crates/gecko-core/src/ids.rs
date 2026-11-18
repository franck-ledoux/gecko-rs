//! The name a cell carries.
//!
//! There used to be two of them. A combinatorial map names its cells by
//! darts — the smallest dart of an orbit — which is what topological code
//! wants and useless as a name to hold on to, since every insertion and
//! removal rewrites it; so cells carried a name of their own. And because
//! the map **split** a node's attribute when the orbit behind it came
//! apart, they carried a `CornerKey` as well, to say that two copies were
//! still the same corner to the cells that had it.
//!
//! An arena renames nothing, so the second question stopped being asked.
//! `CornerKey` and the counter that handed out both went with the map.

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

macro_rules! sentinel_id {
    ($t:ty, $label:literal) => {
        impl $t {
            /// Not named yet.
            pub const NONE: Self = Self(u32::MAX);

            /// Rebuild a name from the number a client sent back.
            ///
            /// Deliberately unchecked: a name that no cell answers to is not a
            /// malformed value, it is a question whose answer is "no such
            /// cell", and that is for the lookup to say.
            pub fn from_raw(raw: u32) -> Self {
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
