//! A slot vector whose index **is** the id.
//!
//! Insertion takes a slot off the free list or grows the vector; removal
//! empties the slot and leaves its neighbours alone. Nothing is ever
//! compacted, because compacting renumbers and renumbering is the thing this
//! whole rewrite is escaping: a name handed to the page, to a script or to
//! the undo stack has to go on meaning the same cell.
//!
//! Two things come with it that are not obvious from the shape:
//!
//! * **a generation per slot.** A slot that is reused would otherwise let a
//!   stale name designate a different cell, silently. The generation goes up
//!   on every insertion, and a slot that reaches 255 **retires** rather than
//!   going back on the free list — so the pathological case costs a slot, not
//!   a wrong answer.
//! * **a journal of what was inserted.** The session clears it at the command
//!   boundary and asks it what the command made, which is what
//!   `settle_fresh` needs. The arena notes it rather than the operation
//!   declaring it, so no operation can forget.

// The whole surface the stages to come are written against — removal, the
// journal, the mutable borrow — is here from the start, and the compiler
// cannot see a user for all of it yet. Goes with the last `todo!()` in
// `cellular.rs`.
#![allow(dead_code)]

use std::marker::PhantomData;

/// An index into an arena, newtyped per kind of cell so that the compiler
/// refuses a face where a node is wanted.
pub trait Idx: Copy + Eq + std::fmt::Debug {
    fn from_raw(raw: u32) -> Self;
    fn raw(self) -> u32;
}

/// Declare one.
macro_rules! index {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);
        impl $crate::arena::Idx for $name {
            fn from_raw(raw: u32) -> Self {
                Self(raw)
            }
            fn raw(self) -> u32 {
                self.0
            }
        }
    };
}
pub(crate) use index;

/// The generation a slot retires at rather than being reused.
const LAST_GENERATION: u8 = u8::MAX;

#[derive(Clone, Debug)]
pub struct Arena<I: Idx, T> {
    slots: Vec<Option<T>>,
    /// One per slot, so that a name carries which incarnation it meant.
    gen: Vec<u8>,
    free: Vec<u32>,
    /// What has been inserted since this was last cleared.
    made: Vec<I>,
    /// How many cells this arena has ever made. Monotone, never reset: it is
    /// what `corner_mark` reads, and a cell's own birth number is compared
    /// against it to say whether an operation made that cell or merely
    /// touched it.
    handed: u32,
    kind: PhantomData<fn() -> I>,
}

impl<I: Idx, T> Default for Arena<I, T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            gen: Vec::new(),
            free: Vec::new(),
            made: Vec::new(),
            handed: 0,
            kind: PhantomData,
        }
    }
}

impl<I: Idx, T> Arena<I, T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many cells are live. Not how many slots there are.
    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len() - self.retired()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Slots emptied and never to be reused.
    fn retired(&self) -> usize {
        self.slots
            .iter()
            .zip(&self.gen)
            .filter(|(slot, gen)| slot.is_none() && **gen == LAST_GENERATION)
            .count()
    }

    pub fn insert(&mut self, value: T) -> I {
        let at = match self.free.pop() {
            Some(at) => {
                self.gen[at as usize] += 1;
                at
            }
            None => {
                self.slots.push(None);
                self.gen.push(0);
                (self.slots.len() - 1) as u32
            }
        };
        self.slots[at as usize] = Some(value);
        let id = I::from_raw(at);
        self.made.push(id);
        self.handed += 1;
        id
    }

    /// How many cells this arena has ever made.
    pub fn handed_out(&self) -> u32 {
        self.handed
    }

    pub fn remove(&mut self, id: I) -> Option<T> {
        let at = id.raw() as usize;
        let gone = self.slots.get_mut(at)?.take();
        if gone.is_some() && self.gen[at] < LAST_GENERATION {
            self.free.push(id.raw());
        }
        gone
    }

    pub fn get(&self, id: I) -> Option<&T> {
        self.slots.get(id.raw() as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: I) -> Option<&mut T> {
        self.slots.get_mut(id.raw() as usize)?.as_mut()
    }

    /// The generation of a live slot, for packing a name at the boundary.
    pub fn generation(&self, id: I) -> Option<u8> {
        self.get(id).map(|_| self.gen[id.raw() as usize])
    }

    /// Every live cell, by id, in slot order — which is the order names were
    /// handed out in, as long as nothing has been removed.
    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(at, slot)| slot.as_ref().map(|t| (I::from_raw(at as u32), t)))
    }

    pub fn ids(&self) -> impl Iterator<Item = I> + '_ {
        self.iter().map(|(id, _)| id)
    }

    /// What has been inserted since [`clear_journal`](Self::clear_journal),
    /// minus whatever has since been removed again.
    pub fn made(&self) -> impl Iterator<Item = I> + '_ {
        self.made
            .iter()
            .copied()
            .filter(|id| self.get(*id).is_some())
    }

    pub fn clear_journal(&mut self) {
        self.made.clear();
    }
}
