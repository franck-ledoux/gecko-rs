//! What the groups of blocks are **called**, and which of them reach the
//! mesh.
//!
//! The membership itself is not here: it is a bit per group on each
//! block's own attribute, where the block structure carries it through
//! cuts and welds on its own. What is here is the part a block structure
//! has no business holding — the words, and whether a group is one of the
//! model's own or a way of looking at it today.

use serde::{Deserialize, Serialize};

/// How many there can be: one bit each in a `u64`.
pub const MOST: usize = 64;

/// One group of blocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// Which bit of a block's mask this group is.
    pub bit: u8,
    pub name: String,
    /// Written into the mesh as a variable of its own, or kept to this
    /// session. A temporary group is for looking at a part of the model;
    /// it is still saved with the session and replayed by its script,
    /// because it is something somebody said.
    pub written: bool,
}

/// The groups a session holds, in the order their bits were handed out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Groups {
    kept: Vec<Group>,
}

impl Groups {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn all(&self) -> &[Group] {
        &self.kept
    }

    pub fn by_name(&self, name: &str) -> Option<&Group> {
        self.kept.iter().find(|g| g.name == name)
    }

    pub fn by_bit(&self, bit: u8) -> Option<&Group> {
        self.kept.iter().find(|g| g.bit == bit)
    }

    /// The group of that name, making one if there is none.
    ///
    /// Create-or-find, because a script that says `gecko.group("fluid",
    /// …)` twice means one group both times, and because the interface
    /// has nowhere sensible to put the difference.
    ///
    /// The bit is the lowest one free, so a group dropped and another
    /// made takes the gap rather than running the model out of bits.
    pub fn make(&mut self, name: &str, written: bool) -> Option<&Group> {
        if let Some(at) = self.kept.iter().position(|g| g.name == name) {
            return self.kept.get(at);
        }
        let taken: u64 = self.kept.iter().map(|g| 1u64 << g.bit).sum();
        let bit = (0..MOST as u8).find(|b| taken & (1u64 << b) == 0)?;
        self.kept.push(Group {
            bit,
            name: name.to_string(),
            written,
        });
        // Kept in bit order, so that two hosts list them the same way.
        self.kept.sort_by_key(|g| g.bit);
        self.by_bit(bit)
    }

    /// Give a group another name, unless something else has it.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<u8, GroupError> {
        if self.kept.iter().any(|g| g.name == to && g.name != from) {
            return Err(GroupError::Taken(to.to_string()));
        }
        let group = self
            .kept
            .iter_mut()
            .find(|g| g.name == from)
            .ok_or_else(|| GroupError::NoSuch(from.to_string()))?;
        group.name = to.to_string();
        Ok(group.bit)
    }

    /// Forget a group. The bit it held is free again.
    pub fn drop(&mut self, name: &str) -> Result<u8, GroupError> {
        let at = self
            .kept
            .iter()
            .position(|g| g.name == name)
            .ok_or_else(|| GroupError::NoSuch(name.to_string()))?;
        Ok(self.kept.remove(at).bit)
    }

    /// Those that are written into the mesh, with their bits.
    pub fn written(&self) -> Vec<(&str, u64)> {
        self.kept
            .iter()
            .filter(|g| g.written)
            .map(|g| (g.name.as_str(), 1u64 << g.bit))
            .collect()
    }
}

/// What can go wrong with the words rather than with the model.
#[derive(Debug, PartialEq, Eq)]
pub enum GroupError {
    NoSuch(String),
    Taken(String),
    /// Sixty-four is all a `u64` holds, and a group lost quietly would be
    /// worse than one refused.
    Full,
    /// A group has to be called something: a row with no name in it is a
    /// row nobody can ask for again.
    Unnamed,
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuch(name) => write!(f, "no group is called {name:?}"),
            Self::Taken(name) => write!(f, "a group is already called {name:?}"),
            Self::Full => write!(f, "there is room for {MOST} groups and they are all taken"),
            Self::Unnamed => write!(f, "a group has to be called something"),
        }
    }
}

impl std::error::Error for GroupError {}
