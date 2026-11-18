//! The cellular block structure: an explicit complex of cells.
//!
//! The replacement for [`crate::blocking`], built behind the same public
//! surface so that the whole test suite is an oracle for it rather than a
//! description of the thing it replaces. Which engine the crate exports as
//! `Blocking` is chosen by the `cellular` feature; both are always compiled,
//! so the differential bench can hold one of each.
//!
//! The only state is four arenas. Everything that climbs — a node's edges, an
//! edge's faces, "does this edge exist already" — is derived and cached, and
//! a cache is not state: the undo stack carries the arenas and nothing else.
//!
//! Unimplemented methods are `todo!()`, deliberately and visibly. Progress is
//! the number of the suite's own tests that pass under the feature, which
//! `./cellular.sh` counts and ratchets.

// Three allowances the stubs need, all of which go with the last `todo!()`,
// and `cellular.sh` is what says when that is:
//
// * the arguments are named rather than `_`, because the names are the
//   contract and renaming them twice over is churn for nothing;
// * some of what is already written has no user yet — the cache, the
//   unpacking of a name — because it is what the next stage is written
//   against;
// * `let out: Vec<_> = todo!()` is how a stub returns an opaque iterator
//   type, and clippy reads the diverging right-hand side as a mistake.
#![allow(unused_variables, dead_code, clippy::diverging_sub_expression)]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::arena::{index, Arena};
use crate::attributes::On;
use crate::cells::{
    apart, best_pairing, quad_normal, BlockFace, BlockingError, Census, ChordBlock, GroupMasks,
    Hex, Settling, Sheet, SheetCells, SheetEdge, Slab, Smoothing, Unmet, HEX_EDGES, HEX_FACES,
};
use crate::ids::CellId;

index!(NodeId);
index!(EdgeId);
index!(FaceId);
index!(BlockId);

/// Which block is on one side of a face, and which of its six slots this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Side {
    pub block: BlockId,
    pub slot: u8,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub at: [f64; 3],
    pub on: Option<On>,
    pub frozen: bool,
    /// How many nodes the arena had made when this one was. What
    /// `nodes_since` compares against, and the reason it is here rather
    /// than derived: the map needed a `corner_key` because it **renames**,
    /// and nothing here is ever renamed, so four bytes of birth number
    /// replace an eight-byte key and the two hash tables that resolved it.
    pub born: u32,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub ends: [NodeId; 2],
    pub segments: u32,
    pub on: Option<On>,
}

/// A face, wound as **side 0** sees it, outward. Side 1 reads the same list
/// backwards, and that is the whole of the orientation question.
///
/// `sides` holds none, one or two. None is a face of a patch — a sheet that
/// bounds no block at all — which is why the note's first decision had to be
/// overturned: laying a quadrangle by hand is the only interactive way onto
/// a surface that works, so an open sheet has to be legal.
#[derive(Clone, Debug)]
pub struct Face {
    pub nodes: [NodeId; 4],
    pub edges: [EdgeId; 4],
    pub sides: [Option<Side>; 2],
    pub on: Option<On>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub nodes: [NodeId; 8],
    pub edges: [EdgeId; 12],
    pub faces: [FaceId; 6],
    pub groups: u64,
}

/// What climbs, derived from the arenas and dropped when they change.
#[derive(Clone, Debug, Default)]
pub struct Up {
    /// Stage 3 and after. Empty until something needs it.
    pub placeholder: (),
}

/// Whether one cycle of four is a rotation of another — which is what
/// "wound the same way round" means when nobody gets to choose where the
/// reading starts.
fn rotation_of(here: &[NodeId; 4], want: &[NodeId]) -> bool {
    (0..4).any(|turn| (0..4).all(|k| here[(turn + k) % 4] == want[k]))
}

/// What a weld decided about one pair of corners.
#[derive(Clone, Copy, Debug)]
struct Welded {
    at: [f64; 3],
    on: Option<On>,
    frozen: bool,
}

/// An invariant that does not hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Breach(pub String);

#[derive(Clone, Debug, Default)]
pub struct Blocking {
    nodes: Arena<NodeId, Node>,
    edges: Arena<EdgeId, Edge>,
    faces: Arena<FaceId, Face>,
    blocks: Arena<BlockId, Block>,
    /// Not state: never copied by `snapshot`, dropped on any removal.
    up: RefCell<Option<Up>>,
}

/// The slots a packed name has for the index, leaving eight for the
/// generation. Sixteen million cells of a kind, which is a hundred and sixty
/// times the 10^5 blocks the budget is drawn for.
const INDEX_BITS: u32 = 24;
const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;

/// An arena index and its generation, as the one number a client holds.
///
/// Bare indices inside, packed at the boundary — the note's third decision.
/// The point is not compactness: it is that a name handed out before a slot
/// was reused cannot come back and designate the cell that took its place.
/// Inside the engine a stale id would be a bug; across the boundary, where
/// the page and a script hold names between commands, it is ordinary.
fn pack<I: crate::arena::Idx>(id: I, generation: u8) -> CellId {
    debug_assert!(id.raw() <= INDEX_MASK, "the arena outgrew a packed name");
    CellId::from_raw((id.raw() << 8) | generation as u32)
}

/// The index a packed name holds, and the generation it was packed with.
fn unpack<I: crate::arena::Idx>(name: CellId) -> Option<(I, u8)> {
    if name.is_none() {
        return None;
    }
    let raw = name.get()?;
    Some((I::from_raw(raw >> 8), (raw & 0xff) as u8))
}

/// The name a client holds for a cell, and the cell a name stands for.
///
/// Both ways round for each of the four kinds, and the same four lines each
/// time, so they are written once: a name is the slot and the generation it
/// was packed with, and resolving it checks the generation — which is the
/// answer "no such cell" for a name whose slot has since been reused.
macro_rules! named {
    ($name:ident, $at:ident, $arena:ident, $id:ty) => {
        fn $name(&self, id: $id) -> CellId {
            self.$arena
                .generation(id)
                .map_or(CellId::NONE, |gen| pack(id, gen))
        }

        fn $at(&self, name: CellId) -> Option<$id> {
            let (id, generation) = unpack::<$id>(name)?;
            (self.$arena.generation(id)? == generation).then_some(id)
        }
    };
}

impl Blocking {
    named!(node_name, node_at, nodes, NodeId);
    named!(edge_name, edge_at, edges, EdgeId);
    named!(face_name, face_at, faces, FaceId);
    named!(block_name, block_at, blocks, BlockId);

    fn node(&self, name: CellId) -> Result<&Node, BlockingError> {
        self.node_at(name)
            .and_then(|id| self.nodes.get(id))
            .ok_or(BlockingError::NoSuchNode(name))
    }

    fn node_mut(&mut self, name: CellId) -> Result<&mut Node, BlockingError> {
        let id = self.node_at(name).ok_or(BlockingError::NoSuchNode(name))?;
        self.nodes
            .get_mut(id)
            .ok_or(BlockingError::NoSuchNode(name))
    }

    fn edge(&self, name: CellId) -> Result<&Edge, BlockingError> {
        self.edge_at(name)
            .and_then(|id| self.edges.get(id))
            .ok_or(BlockingError::NoSuchEdge(name))
    }

    fn face(&self, name: CellId) -> Result<&Face, BlockingError> {
        self.face_at(name)
            .and_then(|id| self.faces.get(id))
            .ok_or(BlockingError::NoSuchFace(name))
    }

    /// Weld a slab just laid to the faces it was laid against.
    ///
    /// The rule is the map's, unchanged, because it is about geometry and
    /// not about representation: a side is welded when the two points it
    /// was laid on were both already nodes and an edge joins them, and the
    /// face welded to is the one **leaning the same way** — a slab's edge
    /// carries both the broad face it was laid on and the thin side, and
    /// which way they lean from the middle of that edge is what tells them
    /// apart. Two leaning our way and the side is left free.
    fn weld_slab(&mut self, block: CellId, quad: [[f64; 3]; 4]) -> Result<usize, BlockingError> {
        let reach = (0..4)
            .map(|k| apart(quad[k], quad[(k + 1) % 4]))
            .fold(0.0, f64::max);
        let slack = 1e-6 * reach.max(f64::MIN_POSITIVE);
        let id = self
            .block_at(block)
            .ok_or(BlockingError::NoSuchBlock(block))?;

        let mut welded = 0;
        for slot in 0..4 {
            let mine = self.blocks.get(id).expect("live").faces;
            // The slab's own thin side between corners `slot` and `slot+1`:
            // the face of the block holding those two and their two
            // opposites, which `HEX_FACES` names without anything being
            // looked for by where it is.
            let want = [slot, (slot + 1) % 4, 4 + (slot + 1) % 4, 4 + slot];
            let at = HEX_FACES
                .iter()
                .position(|face| want.iter().all(|c| face.contains(c)))
                .expect("every edge of a hexahedron is on two side faces");
            let side = mine[at];
            if self.faces.get(side).expect("live").sides[1].is_some() {
                continue;
            }
            let Some(ours) = self.leaning(side, quad[slot], quad[(slot + 1) % 4], slack) else {
                continue;
            };

            let mut best: Option<(f64, FaceId)> = None;
            let mut leaning_our_way = 0;
            for (face, _) in self.faces.iter() {
                if mine.contains(&face) {
                    continue;
                }
                let holder = self.faces.get(face).expect("live");
                if holder.sides[0].is_none() || holder.sides[1].is_some() {
                    continue;
                }
                let Some(theirs) = self.leaning(face, quad[slot], quad[(slot + 1) % 4], slack)
                else {
                    continue;
                };
                let along = (0..3).map(|k| ours[k] * theirs[k]).sum::<f64>();
                if along <= 0.5 {
                    continue;
                }
                leaning_our_way += 1;
                if best.is_none_or(|(most, _)| along > most) {
                    best = Some((along, face));
                }
            }

            // Nothing there, or no way to choose: leave the side free.
            if leaning_our_way != 1 {
                continue;
            }
            let (_, against) = best.expect("one face leaning our way");
            let (against, side) = (self.face_name(against), self.face_name(side));
            self.glue_onto(against, side)?;
            welded += 1;
        }
        Ok(welded)
    }

    /// Which way a face leans away from the side between two points: from
    /// the middle of that side toward the middle of its other two corners,
    /// as a unit vector. Nothing when the face does not hold both points.
    fn leaning(&self, id: FaceId, from: [f64; 3], to: [f64; 3], slack: f64) -> Option<[f64; 3]> {
        let face = self.faces.get(id)?;
        let places = face.nodes.map(|n| self.nodes.get(n).expect("live").at);
        let (mut on, mut off) = (Vec::with_capacity(2), Vec::with_capacity(2));
        for at in places {
            if apart(at, from) <= slack || apart(at, to) <= slack {
                on.push(at);
            } else {
                off.push(at);
            }
        }
        if on.len() != 2 {
            return None;
        }
        let (a, b) = (self.middle(&on), self.middle(&off));
        let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let length = (0..3).map(|k| v[k] * v[k]).sum::<f64>().sqrt();
        (length > 0.0).then(|| v.map(|x| x / length))
    }

    /// Which faces hold this edge.
    fn faces_on(&self, edge: EdgeId) -> Vec<FaceId> {
        self.faces
            .iter()
            .filter(|(_, face)| face.edges.contains(&edge))
            .map(|(id, _)| id)
            .collect()
    }

    /// The edge a quadrangle being laid could share along one of its sides,
    /// and which way the face that already holds it traverses it.
    ///
    /// Shareable means: an edge joins the two points, exactly one face
    /// holds it, and that face bounds no block. Where the map asked whether
    /// β₂ was null, this asks the structure the same question in its own
    /// terms — an edge of a block is held by two faces of that block, so it
    /// is not shareable, and two sheets already welded along an edge hold it
    /// twice, so neither is.
    ///
    /// More than one such edge between the same two points — two sheets
    /// lying coincident and unwelded — and the answer is none: the side is
    /// left free rather than welded to a guess.
    fn shareable(
        &self,
        from: [f64; 3],
        to: [f64; 3],
        slack: f64,
    ) -> Option<(EdgeId, NodeId, NodeId, bool)> {
        let at = |p: [f64; 3]| -> Vec<NodeId> {
            self.nodes
                .iter()
                .filter(|(_, node)| apart(node.at, p) <= slack)
                .map(|(id, _)| id)
                .collect()
        };
        let (here, there) = (at(from), at(to));
        if here.is_empty() || there.is_empty() {
            return None;
        }
        let mut found: Vec<(EdgeId, NodeId, NodeId, bool)> = Vec::new();
        for (id, edge) in self.edges.iter() {
            let [a, b] = edge.ends;
            let forward = here.contains(&a) && there.contains(&b);
            let backward = here.contains(&b) && there.contains(&a);
            if !forward && !backward {
                continue;
            }
            let holders = self.faces_on(id);
            let [holder] = holders[..] else { continue };
            let face = self.faces.get(holder).expect("live");
            if face.sides.iter().any(|side| side.is_some()) {
                continue;
            }
            // Which way that face runs along it, which is what says whether
            // the quadrangle being laid should be turned.
            let k = face
                .edges
                .iter()
                .position(|e| *e == id)
                .expect("the holder holds it");
            let same = face.nodes[k] == if forward { a } else { b };
            let (p, q) = if forward { (a, b) } else { (b, a) };
            found.push((id, p, q, same));
        }
        match found[..] {
            [one] => Some(one),
            _ => None,
        }
    }

    /// Two faces that may be welded to each other: live, distinct, and each
    /// with one side taken and one free.
    ///
    /// A face of a patch — no side at all — is refused rather than welded
    /// face to face. In the map it was not refused, because β₃ did not ask;
    /// here the structure does ask, and what two open sheets laid back to
    /// back would be is not a question anything needs answered.
    fn two_free_faces(
        &self,
        first: CellId,
        second: CellId,
    ) -> Result<(FaceId, FaceId), BlockingError> {
        let a = self
            .face_at(first)
            .ok_or(BlockingError::NoSuchFace(first))?;
        let b = self
            .face_at(second)
            .ok_or(BlockingError::NoSuchFace(second))?;
        if a == b {
            return Err(BlockingError::SameFace);
        }
        for (name, id) in [(first, a), (second, b)] {
            let face = self.faces.get(id).expect("live");
            if face.sides[1].is_some() || face.sides[0].is_none() {
                return Err(BlockingError::AlreadyGlued(name));
            }
        }
        Ok((a, b))
    }

    /// Where each corner of a face is.
    fn corners_of(&self, id: FaceId) -> [[f64; 3]; 4] {
        self.faces
            .get(id)
            .expect("live")
            .nodes
            .map(|n| self.nodes.get(n).expect("live").at)
    }

    /// Every reference to one node becomes a reference to another, and the
    /// first is gone.
    ///
    /// A sweep of the three arenas that can hold a node. The cache that
    /// makes this local — `node_edges` and `node_blocks` — arrives with the
    /// sheet walks; until then this is correct and slow rather than clever
    /// and wrong, and the suite is what says which it is.
    fn rewrite_node(&mut self, from: NodeId, to: NodeId) {
        for id in self.edges.ids().collect::<Vec<_>>() {
            let edge = self.edges.get_mut(id).expect("live");
            for end in edge.ends.iter_mut() {
                if *end == from {
                    *end = to;
                }
            }
        }
        for id in self.faces.ids().collect::<Vec<_>>() {
            let face = self.faces.get_mut(id).expect("live");
            for node in face.nodes.iter_mut() {
                if *node == from {
                    *node = to;
                }
            }
        }
        for id in self.blocks.ids().collect::<Vec<_>>() {
            let block = self.blocks.get_mut(id).expect("live");
            for node in block.nodes.iter_mut() {
                if *node == from {
                    *node = to;
                }
            }
        }
        self.nodes.remove(from);
    }

    /// The same for an edge, which only faces and blocks can hold.
    fn rewrite_edge(&mut self, from: EdgeId, to: EdgeId) {
        for id in self.faces.ids().collect::<Vec<_>>() {
            let face = self.faces.get_mut(id).expect("live");
            for edge in face.edges.iter_mut() {
                if *edge == from {
                    *edge = to;
                }
            }
        }
        for id in self.blocks.ids().collect::<Vec<_>>() {
            let block = self.blocks.get_mut(id).expect("live");
            for edge in block.edges.iter_mut() {
                if *edge == from {
                    *edge = to;
                }
            }
        }
        self.edges.remove(from);
    }

    /// Weld two faces into **one** face with a block on each side.
    ///
    /// Where the map sewed two faces together and left them two, here there
    /// is one cell that both blocks name — which is what makes conformity
    /// structural rather than maintained: there is nothing to keep in
    /// agreement, because there is nothing in two places.
    ///
    /// `turn` is the quarter turn that lines the two up, from
    /// [`best_pairing`]: corner `i` of the first meets corner
    /// `(turn - i) mod 4` of the second. The pairing is necessarily
    /// orientation-reversing, which is exactly what the structure wants —
    /// side 0 reads `nodes` forward and side 1 backward — so invariant 3
    /// comes out true without anything being done about it.
    ///
    /// `decided` says, per pair, where the welded node goes, what it is on
    /// and whether it is frozen. Both callers work that out differently and
    /// everything after it is the same, which is why it is a parameter.
    fn join_faces(
        &mut self,
        keep: FaceId,
        gone: FaceId,
        turn: usize,
        decided: [Welded; 4],
    ) -> Result<(), BlockingError> {
        let here = self.faces.get(keep).expect("live").clone();
        let there = self.faces.get(gone).expect("live").clone();
        let side = there.sides[0].expect("a face being welded has one side");

        for (i, told) in decided.iter().enumerate() {
            let (a, b) = (here.nodes[i], there.nodes[(turn + 4 - i) % 4]);
            {
                let node = self.nodes.get_mut(a).expect("live");
                node.at = told.at;
                node.on = told.on;
                node.frozen = told.frozen;
            }
            if a != b {
                self.rewrite_node(b, a);
            }
        }

        // The sides, paired through the corners they now share rather than
        // by arithmetic on `turn`: the same answer, and it cannot be out by
        // one.
        for i in 0..4 {
            let keep_edge = here.edges[i];
            let (from, to) = (here.nodes[i], here.nodes[(i + 1) % 4]);
            let Some(gone_edge) = there.edges.into_iter().find(|e| {
                self.edges
                    .get(*e)
                    .is_some_and(|edge| edge.ends == [from, to] || edge.ends == [to, from])
            }) else {
                continue;
            };
            if gone_edge == keep_edge {
                continue;
            }
            let (on, segments) = {
                let a = self.edges.get(keep_edge).expect("live");
                let b = self.edges.get(gone_edge).expect("live");
                // The finer of two discretisations is the safe one: it can
                // represent what either side was asking for.
                (a.on.or(b.on), a.segments.max(b.segments))
            };
            let edge = self.edges.get_mut(keep_edge).expect("live");
            edge.on = on;
            edge.segments = segments;
            self.rewrite_edge(gone_edge, keep_edge);
        }

        // One face, with a block on each side.
        {
            let face = self.faces.get_mut(keep).expect("live");
            face.sides[1] = Some(side);
            face.on = here.on.or(there.on);
        }
        self.blocks.get_mut(side.block).expect("live").faces[side.slot as usize] = keep;
        self.faces.remove(gone);
        self.forget();
        Ok(())
    }

    /// The middle of a few points.
    fn middle(&self, places: &[[f64; 3]]) -> [f64; 3] {
        let mut at = [0.0; 3];
        for p in places {
            for k in 0..3 {
                at[k] += p[k] / places.len() as f64;
            }
        }
        at
    }

    /// Drop the derived cache. Called by everything that changes the
    /// arenas, because a cache is not state and a stale one is worse than
    /// none.
    fn forget(&mut self) {
        *self.up.borrow_mut() = None;
    }

    /// One face as one of its blocks sees it.
    ///
    /// Side 0 reads `face.nodes` forward and side 1 reads it backwards, and
    /// that is the whole of the orientation question — no twist byte, no
    /// stored invariant that can rot.
    fn seen_from(&self, id: FaceId, face: &Face, which: usize, side: Side) -> BlockFace {
        let mut nodes = face.nodes.map(|n| self.node_name(n));
        if which == 1 {
            nodes.reverse();
        }
        BlockFace {
            face: self.face_name(id),
            block: self.block_name(side.block),
            groups: self.blocks.get(side.block).map_or(0, |b| b.groups),
            nodes,
            on: face.on,
        }
    }

    /// The patches: connected components of the faces that bound no block.
    ///
    /// A patch has no cell of its own to be named by. The note's first
    /// decision was overturned, so an open sheet is legal — but giving it an
    /// identity would mean a fifth arena and a union-find kept true through
    /// every weld and every removal. So it is derived, and the operations
    /// that work on one take a **face** and walk the component themselves.
    /// The representative a listing hands back is the component's lowest
    /// face, which is stable for as long as that face is.
    fn patch_components(&self) -> Vec<Vec<FaceId>> {
        let loose: Vec<FaceId> = self
            .faces
            .iter()
            .filter(|(_, face)| face.sides.iter().all(|side| side.is_none()))
            .map(|(id, _)| id)
            .collect();
        if loose.is_empty() {
            return Vec::new();
        }
        // Which of them meet along each edge. Only among the loose ones: a
        // sheet welded to a block is that block's face and no part of a
        // patch.
        let mut along: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
        for id in &loose {
            for edge in self.faces.get(*id).expect("live").edges {
                along.entry(edge).or_default().push(*id);
            }
        }

        let mut seen: HashSet<FaceId> = HashSet::new();
        let mut out: Vec<Vec<FaceId>> = Vec::new();
        for start in loose {
            if !seen.insert(start) {
                continue;
            }
            let mut component = vec![start];
            let mut todo = vec![start];
            while let Some(id) = todo.pop() {
                for edge in self.faces.get(id).expect("live").edges {
                    for next in along.get(&edge).into_iter().flatten() {
                        if seen.insert(*next) {
                            component.push(*next);
                            todo.push(*next);
                        }
                    }
                }
            }
            component.sort_unstable();
            out.push(component);
        }
        out.sort_unstable();
        out
    }

    /// Everything the structure promises about itself, checked. Called by the
    /// tests after every operation; the invariants are listed in the design
    /// note, and invariant 2 — that `block.faces[s]` and `face.sides[i]`
    /// agree both ways round — is the one that catches a weld bug.
    pub fn validate(&self) -> Result<(), Vec<Breach>> {
        let mut out: Vec<Breach> = Vec::new();
        macro_rules! say {
            ($($arg:tt)*) => {
                out.push(Breach(format!($($arg)*)))
            };
        }

        // 1. every stored id designates a live cell.
        for (id, edge) in self.edges.iter() {
            for end in edge.ends {
                if self.nodes.get(end).is_none() {
                    say!("edge {id:?} has a dead end {end:?}");
                }
            }
        }
        for (id, face) in self.faces.iter() {
            for node in face.nodes {
                if self.nodes.get(node).is_none() {
                    say!("face {id:?} has a dead corner {node:?}");
                }
            }
            for edge in face.edges {
                if self.edges.get(edge).is_none() {
                    say!("face {id:?} has a dead side {edge:?}");
                }
            }
            for side in face.sides.iter().flatten() {
                if self.blocks.get(side.block).is_none() {
                    say!("face {id:?} has a dead side block {:?}", side.block);
                }
            }
        }
        for (id, block) in self.blocks.iter() {
            for node in block.nodes {
                if self.nodes.get(node).is_none() {
                    say!("block {id:?} has a dead corner {node:?}");
                }
            }
            for edge in block.edges {
                if self.edges.get(edge).is_none() {
                    say!("block {id:?} has a dead edge {edge:?}");
                }
            }
            for face in block.faces {
                if self.faces.get(face).is_none() {
                    say!("block {id:?} has a dead face {face:?}");
                }
            }
        }
        // Everything below dereferences, so stop while a dangling id could
        // be what is wrong: one breach reported honestly beats a panic.
        if !out.is_empty() {
            return Err(out);
        }

        for (id, block) in self.blocks.iter() {
            for (slot, face) in block.faces.iter().enumerate() {
                let f = self.faces.get(*face).expect("live");

                // 2. the link holds in both directions.
                let want = Side {
                    block: id,
                    slot: slot as u8,
                };
                let which = f.sides.iter().position(|side| *side == Some(want));
                let Some(which) = which else {
                    say!(
                        "block {id:?} slot {slot} names face {face:?}, which does not name it back"
                    );
                    continue;
                };

                // 3. the face is wound as that side sees it: a rotation of
                //    the block's own four corners for side 0, and of their
                //    reverse for side 1.
                let mut want: Vec<NodeId> =
                    HEX_FACES[slot].iter().map(|c| block.nodes[*c]).collect();
                if which == 1 {
                    want.reverse();
                }
                if !rotation_of(&f.nodes, &want) {
                    say!("face {face:?} is not wound as block {id:?} slot {slot} sees it");
                }
            }

            // 7. the twelve edges are in canonical order.
            for (k, edge) in block.edges.iter().enumerate() {
                let e = self.edges.get(*edge).expect("live");
                let [a, b] = HEX_EDGES[k].map(|c| block.nodes[c]);
                if e.ends != [a, b] && e.ends != [b, a] {
                    say!("block {id:?} edge {k} is {edge:?}, which joins the wrong two corners");
                }
            }
        }

        for (id, face) in self.faces.iter() {
            // 4. each side of a face joins the two corners it runs between.
            for k in 0..4 {
                let e = self.edges.get(face.edges[k]).expect("live");
                let (from, to) = (face.nodes[k], face.nodes[(k + 1) % 4]);
                if e.ends != [from, to] && e.ends != [to, from] {
                    say!("face {id:?} side {k} does not join its two corners");
                }
            }
            // 6. a face has none, one or two sides, and side 0 before side 1.
            if face.sides[0].is_none() && face.sides[1].is_some() {
                say!("face {id:?} has a second side and no first");
            }
            if let [Some(a), Some(b)] = face.sides {
                if a == b {
                    say!("face {id:?} has the same block on both sides");
                }
            }
        }

        // 5. no two faces carry the same four nodes.
        let mut by_quad: HashMap<[NodeId; 4], FaceId> = HashMap::new();
        for (id, face) in self.faces.iter() {
            let mut key = face.nodes;
            key.sort_unstable();
            if let Some(other) = by_quad.insert(key, id) {
                say!("faces {id:?} and {other:?} carry the same four corners");
            }
        }

        if out.is_empty() {
            Ok(())
        } else {
            Err(out)
        }
    }
}

impl Blocking {
    pub fn new() -> Result<Self, BlockingError> {
        Ok(Self::default())
    }

    /// Four arenas and a derive, where the map took seventy lines of
    /// hand-written dart copying. The cache is not copied: it is derived.
    pub fn snapshot(&self) -> Result<Blocking, BlockingError> {
        Ok(Self {
            up: RefCell::new(None),
            ..self.clone()
        })
    }

    pub fn add_block(&mut self, corners: [[f64; 3]; 8]) -> Result<CellId, BlockingError> {
        let nodes: [NodeId; 8] = corners.map(|at| {
            let born = self.nodes.handed_out();
            self.nodes.insert(Node {
                at,
                on: None,
                frozen: false,
                born,
            })
        });
        let edges: [EdgeId; 12] = HEX_EDGES.map(|[a, b]| {
            self.edges.insert(Edge {
                ends: [nodes[a], nodes[b]],
                // Nought meaning nobody has said, as everywhere else.
                segments: 0,
                on: None,
            })
        });
        // The faces first, with no side filled in: the block they belong to
        // does not exist yet, and a `Side` naming a block before the block
        // is there would be a stored id resolving to nothing — which is
        // what invariant 1 is about.
        let faces: [FaceId; 6] = HEX_FACES.map(|corners_of| {
            let fnodes = corners_of.map(|c| nodes[c]);
            let fedges = [0, 1, 2, 3].map(|k| {
                let (from, to) = (fnodes[k], fnodes[(k + 1) % 4]);
                let at = HEX_EDGES
                    .iter()
                    .position(|[a, b]| {
                        let (p, q) = (nodes[*a], nodes[*b]);
                        (p, q) == (from, to) || (p, q) == (to, from)
                    })
                    .expect("every side of a face of a hexahedron is one of its edges");
                edges[at]
            });
            self.faces.insert(Face {
                nodes: fnodes,
                edges: fedges,
                sides: [None, None],
                on: None,
            })
        });
        let block = self.blocks.insert(Block {
            nodes,
            edges,
            faces,
            groups: 0,
        });
        // And now each face learns which block is on which side of it. Side
        // 0, because `HEX_FACES` is wound anticlockwise seen from outside
        // and this block is inside it: the other side is free, which is
        // what a free-standing block means.
        for (slot, face) in faces.iter().enumerate() {
            self.faces.get_mut(*face).expect("just made").sides[0] = Some(Side {
                block,
                slot: slot as u8,
            });
        }
        self.forget();
        Ok(self.block_name(block))
    }

    pub fn add_slab(&mut self, quad: [[f64; 3]; 4], thickness: f64) -> Result<Slab, BlockingError> {
        if !thickness.is_finite() || thickness == 0.0 {
            return Err(BlockingError::BadHeight(thickness));
        }
        let normal = quad_normal(quad);
        let length = (0..3).map(|k| normal[k] * normal[k]).sum::<f64>().sqrt();
        // Four points in a line, or on top of each other: no quadrangle, so
        // no side to be thick on.
        if length < 1e-12 {
            return Err(BlockingError::LostFace);
        }
        let step = normal.map(|x| x * thickness / length);

        let mut corners = [[0.0; 3]; 8];
        corners[..4].copy_from_slice(&quad);
        for k in 0..4 {
            corners[4 + k] = [0, 1, 2].map(|axis| quad[k][axis] + step[axis]);
        }
        let block = self.add_block(corners)?;
        let welded = self.weld_slab(block, quad)?;

        // Read after the welding: a corner swallowed by a weld comes back as
        // the node that swallowed it.
        let frame = self.hex_corners(block).expect("just made");
        Ok(Slab {
            block,
            outer: [frame[0], frame[1], frame[2], frame[3]],
            inner: [frame[4], frame[5], frame[6], frame[7]],
            welded,
        })
    }

    pub fn add_face(&mut self, corners: [[f64; 3]; 4]) -> Result<CellId, BlockingError> {
        let nodes: [NodeId; 4] = corners.map(|at| {
            let born = self.nodes.handed_out();
            self.nodes.insert(Node {
                at,
                on: None,
                frozen: false,
                born,
            })
        });
        let edges: [EdgeId; 4] = [0, 1, 2, 3].map(|k| {
            self.edges.insert(Edge {
                ends: [nodes[k], nodes[(k + 1) % 4]],
                segments: 0,
                on: None,
            })
        });
        // No side at all, which is what an open sheet is: a face that
        // bounds no block. `Face.sides` holding none is the whole of what
        // overturning the note's first decision cost the structure.
        let face = self.faces.insert(Face {
            nodes,
            edges,
            sides: [None, None],
            on: None,
        });
        self.forget();
        Ok(self.face_name(face))
    }

    pub fn lay_face(&mut self, corners: [[f64; 3]; 4]) -> Result<Sheet, BlockingError> {
        // Two points are "the same point" relative to the size of the thing
        // being laid, as everywhere else here.
        let reach = (0..4)
            .map(|k| apart(corners[k], corners[(k + 1) % 4]))
            .fold(0.0, f64::max);
        let slack = 1e-6 * reach.max(f64::MIN_POSITIVE);

        // Wound to agree with whatever it is laid against, before a cell
        // exists. Two adjacent faces of a consistently wound sheet traverse
        // the side they share in **opposite** directions, so a neighbour
        // running the same way as this quadrangle would is a vote for
        // turning it. A sheet has no inside, so which way round it is says
        // nothing anybody meant: the clicks do not decide it, the
        // neighbours do.
        let (mut given, mut turned) = (0, 0);
        for k in 0..4 {
            if let Some((_, _, _, same)) = self.shareable(corners[k], corners[(k + 1) % 4], slack) {
                if same {
                    turned += 1;
                } else {
                    given += 1;
                }
            }
        }
        let order = if turned > given {
            [0, 3, 2, 1]
        } else {
            [0, 1, 2, 3]
        };
        let wound = order.map(|k| corners[k]);

        // What each side shares, if anything. Found before anything is
        // made, so that the corners can be the neighbour's own.
        let shared: [Option<(EdgeId, NodeId, NodeId, bool)>; 4] =
            [0, 1, 2, 3].map(|k| self.shareable(wound[k], wound[(k + 1) % 4], slack));

        // A corner is the neighbour's node when either side at it is
        // shared; otherwise it is new.
        let mut nodes = [None; 4];
        for k in 0..4 {
            if let Some((_, from, to, _)) = shared[k] {
                nodes[k] = Some(from);
                nodes[(k + 1) % 4] = Some(to);
            }
        }
        let nodes: [NodeId; 4] = [0, 1, 2, 3].map(|k| {
            nodes[k].unwrap_or_else(|| {
                let born = self.nodes.handed_out();
                self.nodes.insert(Node {
                    at: wound[k],
                    on: None,
                    frozen: false,
                    born,
                })
            })
        });
        let edges: [EdgeId; 4] = [0, 1, 2, 3].map(|k| match shared[k] {
            Some((edge, _, _, _)) => edge,
            None => self.edges.insert(Edge {
                ends: [nodes[k], nodes[(k + 1) % 4]],
                segments: 0,
                on: None,
            }),
        });
        let welded = shared.iter().flatten().count();

        let face = self.faces.insert(Face {
            nodes,
            edges,
            sides: [None, None],
            on: None,
        });
        self.forget();

        // In the order the **points** were given, whichever way round the
        // quadrangle ended up wound, because that is the order the caller
        // counts in.
        let mut by_point = [CellId::NONE; 4];
        for (slot, point) in order.iter().enumerate() {
            by_point[*point] = self.node_name(nodes[slot]);
        }
        Ok(Sheet {
            face: self.face_name(face),
            corners: by_point,
            welded,
        })
    }

    pub fn face_cells(&self, face: CellId) -> Option<([CellId; 4], [CellId; 4])> {
        let id = self.face_at(face)?;
        let f = self.faces.get(id)?;
        Some((
            f.nodes.map(|n| self.node_name(n)),
            f.edges.map(|e| self.edge_name(e)),
        ))
    }

    pub fn corner_mark(&self) -> u32 {
        self.nodes.handed_out()
    }

    pub fn nodes_since(&self, mark: u32) -> Vec<CellId> {
        let mut out: Vec<CellId> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.born >= mark)
            .map(|(id, _)| self.node_name(id))
            .collect();
        out.sort_unstable();
        out
    }

    pub fn blocks(&self) -> Vec<CellId> {
        self.blocks.ids().map(|id| self.block_name(id)).collect()
    }

    pub fn block_groups(&self, block: CellId) -> u64 {
        self.block_at(block)
            .and_then(|id| self.blocks.get(id))
            .map_or(0, |b| b.groups)
    }

    pub fn counted_in_groups(&self) -> [usize; 64] {
        let mut out = [0usize; 64];
        for (_, block) in self.blocks.iter() {
            for (bit, count) in out.iter_mut().enumerate() {
                if block.groups & (1 << bit) != 0 {
                    *count += 1;
                }
            }
        }
        out
    }

    pub fn set_in_group(
        &mut self,
        blocks: &[CellId],
        bit: u8,
        inside: bool,
    ) -> Result<(), BlockingError> {
        // Every name resolved before anything is written: a refused call
        // leaves the grouping exactly as it was.
        let ids = blocks
            .iter()
            .map(|name| {
                self.block_at(*name)
                    .ok_or(BlockingError::NoSuchBlock(*name))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for id in ids {
            let block = self.blocks.get_mut(id).expect("just resolved");
            if inside {
                block.groups |= 1u64 << bit;
            } else {
                block.groups &= !(1u64 << bit);
            }
        }
        Ok(())
    }

    pub fn drop_group(&mut self, bit: u8) {
        let mask = !(1u64 << bit);
        for id in self.blocks.ids().collect::<Vec<_>>() {
            self.blocks.get_mut(id).expect("live").groups &= mask;
        }
    }

    pub fn group_masks(&self) -> GroupMasks {
        // Nothing grouped means every mask is nought, and that says it
        // already: the loose sets exist only to answer "does this cell also
        // bound a block in no group", which is a question nobody asks until
        // something is grouped.
        let mut out = GroupMasks::default();
        if self.blocks.iter().all(|(_, b)| b.groups == 0) {
            return out;
        }
        for (_, block) in self.blocks.iter() {
            for node in block.nodes {
                let name = self.node_name(node);
                *out.nodes.entry(name).or_default() |= block.groups;
                if block.groups == 0 {
                    out.loose_nodes.insert(name);
                }
            }
            for edge in block.edges {
                let name = self.edge_name(edge);
                *out.edges.entry(name).or_default() |= block.groups;
                if block.groups == 0 {
                    out.loose_edges.insert(name);
                }
            }
        }
        out
    }

    pub fn beside_the_classified(&self, node: CellId) -> bool {
        self.neighbors(node)
            .into_iter()
            .any(|other| self.node_class(other).is_some())
    }

    pub fn smooth(
        &mut self,
        how: Smoothing<'_>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) -> f64 {
        todo!("cellular: smooth")
    }

    pub fn patches(&self) -> Vec<CellId> {
        self.patch_components()
            .into_iter()
            .map(|faces| self.face_name(faces[0]))
            .collect()
    }

    pub fn assign_missing_ids(&mut self) {
        // Nothing is ever unnamed: a cell's name is the slot it is in. The
        // map needed this because sewing and unsewing split and merged
        // orbits as a side effect, leaving cells nobody had named.
    }

    pub fn set_position(&mut self, node: CellId, to: [f64; 3]) -> Result<(), BlockingError> {
        self.node_mut(node)?.at = to;
        Ok(())
    }

    pub fn node_class(&self, node: CellId) -> Option<On> {
        self.node(node).ok().and_then(|n| n.on)
    }

    pub fn set_node_class(&mut self, node: CellId, on: Option<On>) -> Result<(), BlockingError> {
        self.node_mut(node)?.on = on;
        Ok(())
    }

    pub fn is_frozen(&self, node: CellId) -> bool {
        self.node(node).map(|n| n.frozen).unwrap_or(false)
    }

    pub fn set_frozen(&mut self, node: CellId, frozen: bool) -> Result<(), BlockingError> {
        self.node_mut(node)?.frozen = frozen;
        Ok(())
    }

    pub fn edge_class(&self, edge: CellId) -> Option<On> {
        self.edge(edge).ok().and_then(|e| e.on)
    }

    pub fn set_edge_class(&mut self, edge: CellId, on: Option<On>) -> Result<(), BlockingError> {
        let id = self.edge_at(edge).ok_or(BlockingError::NoSuchEdge(edge))?;
        self.edges
            .get_mut(id)
            .ok_or(BlockingError::NoSuchEdge(edge))?
            .on = on;
        Ok(())
    }

    pub fn face_class(&self, face: CellId) -> Option<On> {
        self.face(face).ok().and_then(|f| f.on)
    }

    pub fn set_face_class(&mut self, face: CellId, on: Option<On>) -> Result<(), BlockingError> {
        let id = self.face_at(face).ok_or(BlockingError::NoSuchFace(face))?;
        self.faces
            .get_mut(id)
            .ok_or(BlockingError::NoSuchFace(face))?
            .on = on;
        Ok(())
    }

    pub fn faces(&self) -> impl Iterator<Item = (CellId, [CellId; 4])> + '_ {
        self.faces
            .iter()
            .map(|(id, face)| (self.face_name(id), face.edges.map(|e| self.edge_name(e))))
    }

    pub fn edges(&self) -> impl Iterator<Item = (CellId, CellId, CellId, Option<On>)> + '_ {
        self.edges.iter().map(|(id, edge)| {
            (
                self.edge_name(id),
                self.node_name(edge.ends[0]),
                self.node_name(edge.ends[1]),
                edge.on,
            )
        })
    }

    pub fn nodes_in_full(&self) -> impl Iterator<Item = (CellId, [f64; 3], Option<On>, bool)> + '_ {
        self.nodes
            .iter()
            .map(|(id, node)| (self.node_name(id), node.at, node.on, node.frozen))
    }

    pub fn census(&self) -> Census {
        Census {
            nodes: self.nodes.len(),
            edges: self.edges.len(),
            faces: self.faces.len(),
            blocks: self.blocks.len(),
            patches: self.patch_components().len(),
            classified_nodes: self.nodes.iter().filter(|(_, n)| n.on.is_some()).count(),
            classified_edges: self.edges.iter().filter(|(_, e)| e.on.is_some()).count(),
            classified_faces: self.faces.iter().filter(|(_, f)| f.on.is_some()).count(),
            frozen: self.nodes.iter().filter(|(_, n)| n.frozen).count(),
        }
    }

    pub fn classify_nodes(
        &mut self,
        pick: impl Fn(Option<On>, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) {
        todo!("cellular: classify_nodes")
    }

    pub fn classify_named(
        &mut self,
        nodes: &[CellId],
        pick: impl Fn(Option<On>, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) {
        todo!("cellular: classify_named")
    }

    pub fn reclassify(&mut self, settle: impl Fn(Settling<'_>) -> Option<On>) {
        // Edges from their two ends, then faces from their four edges — in
        // that order, because each level consumes the one below it. Decided
        // first and written after, so that a face reads the edges as this
        // pass has just left them and not as they were half way through.
        let settled: Vec<(EdgeId, Option<On>)> = self
            .edges
            .iter()
            .map(|(id, edge)| {
                let [a, b] = edge.ends.map(|n| self.nodes.get(n).expect("live"));
                let already = edge.on;
                let middle = self.middle(&[a.at, b.at]);
                let on = match (a.on, b.on) {
                    // An edge is never pinned to a point, which would
                    // over-constrain it.
                    (Some(x), Some(y)) => settle(Settling {
                        boundary: &[x, y],
                        at_least: 1,
                        already,
                        middle,
                    }),
                    // One end left to go on. Nothing is handed out from a
                    // single end — the diagonal of a face has one end on a
                    // curve too, and it is not on that curve — but what the
                    // edge already had stays while that end allows it.
                    (Some(x), None) | (None, Some(x)) => {
                        let kept = settle(Settling {
                            boundary: &[x],
                            at_least: 1,
                            already,
                            middle,
                        });
                        already.filter(|on| kept == Some(*on))
                    }
                    // Neither end on anything is the inside of the model.
                    (None, None) => None,
                };
                (id, on)
            })
            .collect();
        for (id, on) in settled {
            self.edges.get_mut(id).expect("live").on = on;
        }

        let settled: Vec<(FaceId, Option<On>)> = self
            .faces
            .iter()
            .map(|(id, face)| {
                let sides: Vec<Option<On>> = face
                    .edges
                    .iter()
                    .map(|e| self.edges.get(*e).expect("live").on)
                    .collect();
                let places: Vec<[f64; 3]> = face
                    .nodes
                    .iter()
                    .map(|n| self.nodes.get(*n).expect("live").at)
                    .collect();
                let middle = self.middle(&places);
                let already = face.on;
                let known: Vec<On> = sides.iter().flatten().copied().collect();
                let on = if known.len() == sides.len() {
                    settle(Settling {
                        boundary: &known,
                        // Nor a face to a curve.
                        at_least: 2,
                        already,
                        middle,
                    })
                } else if known.is_empty() {
                    None
                } else {
                    // The same rule as an edge's, one dimension up.
                    let kept = settle(Settling {
                        boundary: &known,
                        at_least: 2,
                        already,
                        middle,
                    });
                    already.filter(|on| kept == Some(*on))
                };
                (id, on)
            })
            .collect();
        for (id, on) in settled {
            self.faces.get_mut(id).expect("live").on = on;
        }
    }

    pub fn unclassify_cells(&mut self) {
        for id in self.edges.ids().collect::<Vec<_>>() {
            self.edges.get_mut(id).expect("live").on = None;
        }
        for id in self.faces.ids().collect::<Vec<_>>() {
            self.faces.get_mut(id).expect("live").on = None;
        }
    }

    pub fn nodes(&self) -> impl Iterator<Item = (CellId, [f64; 3])> + '_ {
        self.nodes
            .iter()
            .map(|(id, node)| (self.node_name(id), node.at))
    }

    pub fn glue_faces(
        &mut self,
        first: CellId,
        second: CellId,
        common: impl Fn(On, On, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) -> Result<Vec<Unmet>, BlockingError> {
        let (a, b) = self.two_free_faces(first, second)?;
        let (pa, pb) = (self.corners_of(a), self.corners_of(b));
        let turn = best_pairing(&pa, &pb);
        let here = self.faces.get(a).expect("live").nodes;
        let there = self.faces.get(b).expect("live").nodes;

        // Two frozen corners "coincide" relative to the size of what is
        // being glued, not to an absolute number: a blocking of a turbine
        // blade and one of a building are the same shape to a thousandth of
        // a side.
        let side = (0..4)
            .map(|i| apart(pa[i], pa[(i + 1) % 4]))
            .fold(0.0, f64::max);
        let slack = 1e-6 * side.max(f64::MIN_POSITIVE);

        // Every pair decided before a single node moves. A refused glue has
        // to leave the blocking exactly as it was, and the only way to
        // promise that is to find out everything first.
        let mut decided: Vec<Welded> = Vec::with_capacity(4);
        let mut unmet: Vec<Unmet> = Vec::new();
        for i in 0..4 {
            let (mine, partner) = (here[i], there[(turn + 4 - i) % 4]);
            let (ta, tb) = (
                self.nodes.get(mine).expect("live").clone(),
                self.nodes.get(partner).expect("live").clone(),
            );
            let (qa, qb) = (pa[i], pb[(turn + 4 - i) % 4]);

            // Where this corner is heading, as far as can be said before
            // the classification is settled — and that is what settles it,
            // when two entities share more than one thing.
            let hint = match (ta.frozen, tb.frozen) {
                (true, _) => qa,
                (false, true) => qb,
                (false, false) => [
                    0.5 * (qa[0] + qb[0]),
                    0.5 * (qa[1] + qb[1]),
                    0.5 * (qa[2] + qb[2]),
                ],
            };

            let on = match (ta.on, tb.on) {
                (None, None) => None,
                (Some(only), None) | (None, Some(only)) => Some(only),
                (Some(x), Some(y)) if x == y => Some(x),
                (Some(x), Some(y)) => Some(common(x, y, hint).unwrap_or_else(|| {
                    // Nothing in common. The more constrained entity is the
                    // one a node can be on while being near the other, so
                    // it is the one to keep — and a tie goes to the first
                    // face, as its name does when the two nodes merge.
                    let (kept, dropped) = if y.dim < x.dim { (y, x) } else { (x, y) };
                    unmet.push(Unmet {
                        node: self.node_name(mine),
                        kept,
                        dropped,
                    });
                    kept
                })),
            };

            let at = match (ta.frozen, tb.frozen) {
                (true, true) if apart(qa, qb) > slack => {
                    return Err(BlockingError::FrozenApart(
                        self.node_name(mine),
                        self.node_name(partner),
                    ))
                }
                // A frozen node is where somebody put it: `hint` is already
                // that point, and no projection may move it off.
                (true, _) | (false, true) => hint,
                // Here there is nobody to contradict.
                (false, false) => on.and_then(|o| project(o, hint)).unwrap_or(hint),
            };

            decided.push(Welded {
                at,
                on,
                frozen: ta.frozen || tb.frozen,
            });
        }

        let decided: [Welded; 4] = decided.try_into().expect("four corners");
        self.join_faces(a, b, turn, decided)?;
        Ok(unmet)
    }

    pub fn glue_onto(&mut self, first: CellId, second: CellId) -> Result<(), BlockingError> {
        let (a, b) = self.two_free_faces(first, second)?;
        let (pa, pb) = (self.corners_of(a), self.corners_of(b));
        let turn = best_pairing(&pa, &pb);

        // The caller has already put the two in the same place, so there is
        // nothing to decide about where the corners go: the first face's
        // place is the answer, and which of the two nodes survives cannot
        // be told apart. What each node **claims** still has to be carried
        // over, and that is what a merge policy used to do silently.
        let there = self.faces.get(b).expect("live").nodes;
        let here = self.faces.get(a).expect("live").nodes;
        let decided = [0, 1, 2, 3].map(|i| {
            let partner = self.nodes.get(there[(turn + 4 - i) % 4]).expect("live");
            let mine = self.nodes.get(here[i]).expect("live");
            Welded {
                at: pa[i],
                on: mine.on.or(partner.on),
                frozen: mine.frozen || partner.frozen,
            }
        });
        self.join_faces(a, b, turn, decided)
    }

    pub fn sheet_blocks(&self, sheet: &[SheetEdge]) -> usize {
        todo!("cellular: sheet_blocks")
    }

    pub fn sheet_cells(&self, edge: CellId) -> Result<SheetCells, BlockingError> {
        todo!("cellular: sheet_cells")
    }

    pub fn sheets_beside(&self, edge: CellId) -> Result<[Option<CellId>; 2], BlockingError> {
        todo!("cellular: sheets_beside")
    }

    pub fn remove_sheet(&mut self, edge: CellId) -> Result<(), BlockingError> {
        todo!("cellular: remove_sheet")
    }

    pub fn remove_sheet_leaving_a_gap(&mut self, edge: CellId) -> Result<(), BlockingError> {
        todo!("cellular: remove_sheet_leaving_a_gap")
    }

    pub fn remove_block(&mut self, block: CellId) -> Result<(), BlockingError> {
        todo!("cellular: remove_block")
    }

    pub fn remove_face(&mut self, face: CellId) -> Result<(), BlockingError> {
        todo!("cellular: remove_face")
    }

    pub fn sheet_through(&self, edge: CellId) -> Result<Vec<SheetEdge>, BlockingError> {
        todo!("cellular: sheet_through")
    }

    pub fn chord_through(
        &self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<ChordBlock>, BlockingError> {
        todo!("cellular: chord_through")
    }

    pub fn chord_wire(&self, chord: &[ChordBlock]) -> Vec<(CellId, CellId, CellId)> {
        todo!("cellular: chord_wire")
    }

    pub fn remove_chord(
        &mut self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<CellId>, BlockingError> {
        todo!("cellular: remove_chord")
    }

    pub fn pillow_chord(
        &mut self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<(CellId, Option<On>)>, BlockingError> {
        todo!("cellular: pillow_chord")
    }

    pub fn split_sheet(&mut self, edge: CellId, t: f64) -> Result<Vec<CellId>, BlockingError> {
        todo!("cellular: split_sheet")
    }

    pub fn extrude_patch(
        &mut self,
        face: CellId,
        height: f64,
    ) -> Result<Vec<CellId>, BlockingError> {
        todo!("cellular: extrude_patch")
    }

    pub fn extrude_faces(
        &mut self,
        picked: &[(CellId, CellId)],
        height: f64,
    ) -> Result<Vec<CellId>, BlockingError> {
        todo!("cellular: extrude_faces")
    }

    pub fn extrude_face(
        &mut self,
        face: CellId,
        block: CellId,
        height: f64,
    ) -> Result<CellId, BlockingError> {
        todo!("cellular: extrude_face")
    }

    pub fn pillow(
        &mut self,
        patch: &[(CellId, CellId)],
    ) -> Result<Vec<(CellId, Option<On>)>, BlockingError> {
        todo!("cellular: pillow")
    }

    pub fn sheet_segments(&self, edge: CellId) -> Result<u32, BlockingError> {
        todo!("cellular: sheet_segments")
    }

    pub fn set_sheet_segments(&mut self, edge: CellId, segments: u32) -> Result<(), BlockingError> {
        todo!("cellular: set_sheet_segments")
    }

    pub fn block_faces(&self) -> Vec<BlockFace> {
        // Which patch each loose face belongs to, so that a face bounding no
        // block is still listed — against the patch it is part of. The page
        // draws a patch, and a face missing from this listing is a face
        // missing from the screen.
        let mut patch_of: HashMap<FaceId, CellId> = HashMap::new();
        for component in self.patch_components() {
            let name = self.face_name(component[0]);
            for id in component {
                patch_of.insert(id, name);
            }
        }

        self.faces
            .iter()
            .flat_map(|(id, face)| {
                let sides: Vec<BlockFace> = face
                    .sides
                    .iter()
                    .enumerate()
                    .filter_map(|(which, side)| {
                        side.map(|side| self.seen_from(id, face, which, side))
                    })
                    .collect();
                if sides.is_empty() {
                    // An open sheet has no inside, so there is no "as that
                    // block sees it": the face's own winding is all there
                    // is, and it is handed over as it stands.
                    return vec![BlockFace {
                        face: self.face_name(id),
                        block: patch_of.get(&id).copied().unwrap_or(CellId::NONE),
                        groups: 0,
                        nodes: face.nodes.map(|n| self.node_name(n)),
                        on: face.on,
                    }];
                }
                sides
            })
            .collect()
    }

    pub fn hexes(&self) -> Vec<Hex> {
        self.blocks
            .iter()
            .map(|(id, b)| Hex {
                block: self.block_name(id),
                corners: b.nodes.map(|n| self.node_name(n)),
                edges: b.edges.map(|e| self.edge_name(e)),
                faces: b.faces.map(|f| self.face_name(f)),
            })
            .collect()
    }

    pub fn node_names(&self) -> Vec<CellId> {
        self.nodes.ids().map(|id| self.node_name(id)).collect()
    }

    pub fn edge_names(&self) -> Vec<CellId> {
        self.edges.ids().map(|id| self.edge_name(id)).collect()
    }

    pub fn face_names(&self) -> Vec<CellId> {
        self.faces.ids().map(|id| self.face_name(id)).collect()
    }

    pub fn component_names(&self) -> Vec<CellId> {
        let mut out: Vec<CellId> = self.blocks();
        out.extend(self.patches());
        out
    }

    pub fn position(&self, node: CellId) -> Option<[f64; 3]> {
        self.node(node).ok().map(|n| n.at)
    }

    pub fn edge_ends(&self, edge: CellId) -> Option<(CellId, CellId)> {
        let e = self.edge(edge).ok()?;
        Some((self.node_name(e.ends[0]), self.node_name(e.ends[1])))
    }

    pub fn hex_corners(&self, block: CellId) -> Option<[CellId; 8]> {
        let id = self.block_at(block)?;
        let b = self.blocks.get(id)?;
        Some(b.nodes.map(|n| self.node_name(n)))
    }

    pub fn neighbors(&self, node: CellId) -> Vec<CellId> {
        let Some(id) = self.node_at(node) else {
            return Vec::new();
        };
        // A sweep of the edges. The cache that answers this in O(1) comes
        // with the sheet walks, which are the first thing that cannot
        // afford a sweep; until then a sweep is the honest implementation
        // and not a slow one worth hiding.
        let mut out: Vec<CellId> = self
            .edges
            .iter()
            .filter_map(|(_, edge)| match edge.ends {
                [a, b] if a == id => Some(self.node_name(b)),
                [a, b] if b == id => Some(self.node_name(a)),
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn on_the_surface(&self, node: CellId) -> bool {
        let Some(id) = self.node_at(node) else {
            return false;
        };
        self.faces
            .iter()
            .any(|(_, face)| face.nodes.contains(&id) && face.sides[1].is_none())
    }

    pub fn is_block(&self, block: CellId) -> bool {
        self.block_at(block).is_some()
    }

    pub fn faces_of(&self, block: CellId) -> Vec<BlockFace> {
        // A **component** name, which is a block or a patch: the map's
        // `faces_of` took a volume and a patch was a volume, so both have
        // to answer here too. A patch is named by a face of its own (see
        // `patch_components`), so a name that is not a block's is looked
        // for among them.
        let Some(block) = self.block_at(block) else {
            let Some(id) = self.face_at(block) else {
                return Vec::new();
            };
            let Some(component) = self
                .patch_components()
                .into_iter()
                .find(|faces| faces.contains(&id))
            else {
                return Vec::new();
            };
            let name = self.face_name(component[0]);
            return component
                .into_iter()
                .filter_map(|id| {
                    let face = self.faces.get(id)?;
                    Some(BlockFace {
                        face: self.face_name(id),
                        block: name,
                        groups: 0,
                        nodes: face.nodes.map(|n| self.node_name(n)),
                        on: face.on,
                    })
                })
                .collect();
        };
        let Some(b) = self.blocks.get(block) else {
            return Vec::new();
        };
        b.faces
            .iter()
            .filter_map(|id| {
                let face = self.faces.get(*id)?;
                let which = face
                    .sides
                    .iter()
                    .position(|side| side.is_some_and(|s| s.block == block))?;
                Some(self.seen_from(*id, face, which, face.sides[which]?))
            })
            .collect()
    }

    pub fn edges_in_full(
        &self,
    ) -> impl Iterator<Item = (CellId, [CellId; 2], u32, Option<On>)> + '_ {
        self.edges.iter().map(|(id, edge)| {
            (
                self.edge_name(id),
                edge.ends.map(|n| self.node_name(n)),
                edge.segments,
                edge.on,
            )
        })
    }

    pub fn nb_nodes(&self) -> usize {
        self.nodes.len()
    }

    pub fn nb_edges(&self) -> usize {
        self.edges.len()
    }

    pub fn nb_faces(&self) -> usize {
        self.faces.len()
    }

    pub fn nb_blocks(&self) -> usize {
        self.blocks.len()
    }

    pub fn nb_patches(&self) -> usize {
        self.patch_components().len()
    }

    pub fn names_handed_out(&self) -> [u32; 4] {
        [
            self.nodes.handed_out(),
            self.edges.handed_out(),
            self.faces.handed_out(),
            self.blocks.handed_out(),
        ]
    }
}
