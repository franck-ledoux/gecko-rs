//! The block structure: an explicit complex of cells.
//!
//! **The only state is four arenas.** Everything that climbs — a node's
//! edges, an edge's faces, "does this edge exist already" — is derived and
//! cached, and a cache is not state: the undo stack carries the arenas and
//! nothing else, and `snapshot` is a derive.
//!
//! This replaced a 3-dimensional combinatorial map, which cost some twenty
//! thousand bytes a block because a cell attribute was paid for by the dart.
//! It was written behind the same public surface, so that the whole test
//! suite was an oracle for it rather than a description of the thing it
//! replaced: not a line of the suite changed, and a differential bench ran
//! both at once on the same scripts until the last of them agreed.
//! `docs/developer-guide/cellular.md` is the account of it, measurements
//! and all.
//!
//! What the structure promises about itself is in [`Blocking::validate`],
//! which the tests call after every operation. Three facts shape everything
//! else:
//!
//! * **a face has none, one or two sides**, and `nodes` is wound as side 0
//!   sees it — outward. Side 1 reads the same list backwards, and that is
//!   the whole of the orientation question: no twist byte, no stored
//!   invariant that can rot. None is a face of a patch.
//! * **a name is a slot and the generation it was packed with**, so nothing
//!   is ever renamed and a name handed out before a slot was reused cannot
//!   come back designating the cell that took its place.
//! * **welding two faces leaves one face that both blocks name.** There is
//!   nothing to keep in agreement, because there is nothing in two places.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::arena::{index, Arena};
use crate::cells::{
    across, apart, best_pairing, flatness, mitre, quad_normal, scaled_jacobian, BlockFace,
    BlockingError, Census, ChordBlock, GroupMasks, Hex, On, Settling, Sheet, SheetCells, SheetEdge,
    Slab, Smoothing, Unmet, EDGE_AXIS, FACE_AXIS, HEX_CORNER_EDGES, HEX_EDGES, HEX_FACES, OPPOSITE,
    PILLOW_SHARE, SPLIT_FRAME,
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
///
/// Nothing in here is state: it is all re-derivable from the four arenas,
/// which is why `snapshot` does not copy it and the undo stack does not
/// carry it. Built on first use and dropped by [`Blocking::forget`].
///
/// The note draws these as CSR arrays. A map of vectors holds the same
/// information and is what they will be built from; the arrays matter when
/// the cost of the hashing starts to show, and it does not yet.
#[derive(Clone, Debug, Default)]
pub struct Up {
    pub node_edges: HashMap<NodeId, Vec<EdgeId>>,
    pub edge_faces: HashMap<EdgeId, Vec<FaceId>>,
    pub edge_blocks: HashMap<EdgeId, Vec<BlockId>>,
    pub node_faces: HashMap<NodeId, Vec<FaceId>>,
}

/// Whether one cycle of four is a rotation of another — which is what
/// "wound the same way round" means when nobody gets to choose where the
/// reading starts.
fn rotation_of(here: &[NodeId; 4], want: &[NodeId]) -> bool {
    (0..4).any(|turn| (0..4).all(|k| here[(turn + k) % 4] == want[k]))
}

/// Where to find a cell by the nodes it holds, while an operation makes
/// several of them.
///
/// Kept by the operation and not in [`Up`], because the cache is dropped on
/// every change and an operation that makes fifty blocks would otherwise
/// rebuild it fifty times. Invariant 5 — no two faces on the same four
/// nodes — is what makes the quad a key at all.
struct Known {
    by_pair: HashMap<[NodeId; 2], EdgeId>,
    by_quad: HashMap<[NodeId; 4], FaceId>,
}

/// What a cut makes of one face: the two halves as node quads, the edge it
/// laid across the whole, and what the whole was classified on.
type Halved = ([[NodeId; 4]; 2], [NodeId; 2], Option<On>);

/// One face a pillow draws in: the face, the block drawing it, the four
/// corners as that block winds them, the way in, and how far.
type Drawn = (FaceId, BlockId, [NodeId; 4], [f64; 3], f64);

/// Who holds each node and each edge, while an operation merges several of
/// them.
///
/// Kept by the operation and not in [`Up`], for the same reason [`Known`]
/// is: a merge drops the cache, and welding four hundred seams would
/// rebuild it four hundred times. A merge moves one cell's holders onto
/// another's, which is what keeps this true as the welds go by — and an id
/// it still names after the cell has gone is harmless, because every write
/// below looks the cell up first.
struct Holders {
    node_edges: HashMap<NodeId, Vec<EdgeId>>,
    node_faces: HashMap<NodeId, Vec<FaceId>>,
    node_blocks: HashMap<NodeId, Vec<BlockId>>,
    edge_faces: HashMap<EdgeId, Vec<FaceId>>,
    edge_blocks: HashMap<EdgeId, Vec<BlockId>>,
}

/// A cycle of four, read the other way round.
fn reversed(cycle: [NodeId; 4]) -> [NodeId; 4] {
    [cycle[3], cycle[2], cycle[1], cycle[0]]
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

    /// The side of this face joining those two corners.
    fn edge_between(&self, face: FaceId, from: NodeId, to: NodeId) -> Option<EdgeId> {
        let f = self.faces.get(face)?;
        (0..4)
            .find(|k| {
                let (a, b) = (f.nodes[*k], f.nodes[(*k + 1) % 4]);
                (a, b) == (from, to) || (a, b) == (to, from)
            })
            .map(|k| f.edges[k])
    }

    /// Which faces hold this edge.
    fn faces_on(&self, edge: EdgeId) -> Vec<FaceId> {
        self.climbing(|up| up.edge_faces.get(&edge).cloned().unwrap_or_default())
    }

    /// Which blocks hold this edge.
    fn blocks_on(&self, edge: EdgeId) -> Vec<BlockId> {
        self.climbing(|up| up.edge_blocks.get(&edge).cloned().unwrap_or_default())
    }

    /// The slot a face occupies in a block, and which of its two sides that
    /// is.
    fn slot_of(&self, face: FaceId, block: BlockId) -> Option<(u8, usize)> {
        let b = self.blocks.get(block)?;
        let slot = b.faces.iter().position(|f| *f == face)? as u8;
        let f = self.faces.get(face)?;
        let which = f
            .sides
            .iter()
            .position(|side| side.is_some_and(|s| s.block == block && s.slot == slot))?;
        Some((slot, which))
    }

    /// What is on the other side of a face from the side given.
    ///
    /// Compared as a whole `Side`, block **and** slot, and not by the block
    /// alone: two faces of one block welded to each other leave a face with
    /// the same block on both sides, and a chord that walked out of it by
    /// asking for "a different block" would find nothing and stop short.
    /// That case is a test of its own, and it is why this is not three
    /// lines shorter.
    fn beyond(&self, face: FaceId, from: Side) -> Option<Side> {
        let f = self.faces.get(face)?;
        f.sides
            .iter()
            .flatten()
            .find(|side| **side != from)
            .copied()
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

    /// The blocks and loose faces a node is a corner of.
    fn around_node(&self, node: NodeId) -> (Vec<BlockId>, Vec<FaceId>) {
        self.climbing(|up| {
            let faces: Vec<FaceId> = up.node_faces.get(&node).cloned().unwrap_or_default();
            let mut blocks: Vec<BlockId> = faces
                .iter()
                .filter_map(|f| self.faces.get(*f))
                .flat_map(|f| f.sides.iter().flatten().map(|side| side.block))
                .collect();
            blocks.sort_unstable();
            blocks.dedup();
            let loose: Vec<FaceId> = faces
                .into_iter()
                .filter(|f| {
                    self.faces
                        .get(*f)
                        .is_some_and(|face| face.sides.iter().all(|side| side.is_none()))
                })
                .collect();
            (blocks, loose)
        })
    }

    /// The worst corner of anything this node is a corner of.
    ///
    /// What a smoothing step is judged by: a Laplacian on a concave
    /// boundary would happily turn a cell inside out, and this is how it is
    /// told not to.
    fn worst_corner_at(&self, node: NodeId) -> f64 {
        let mut worst = f64::INFINITY;
        let (blocks, loose) = self.around_node(node);
        for id in blocks {
            let corners = self.blocks.get(id).expect("live").nodes;
            let at = corners.map(|c| self.nodes.get(c).expect("live").at);
            for (corner, sides) in HEX_CORNER_EDGES.iter().enumerate() {
                worst = worst.min(scaled_jacobian(at[corner], sides.map(|s| at[s])));
            }
        }
        // The faces of a patch, which can fold without any block being
        // involved at all.
        for id in loose {
            let at = self
                .faces
                .get(id)
                .expect("live")
                .nodes
                .map(|c| self.nodes.get(c).expect("live").at);
            for k in 0..4 {
                worst = worst.min(flatness(at[k], at[(k + 3) % 4], at[(k + 1) % 4]));
            }
        }
        if worst.is_finite() {
            worst
        } else {
            // A node that is a corner of nothing cannot be made worse.
            0.0
        }
    }

    /// Say where some of the nodes are, and put them there.
    fn classify_some(
        &mut self,
        which: impl Fn(NodeId, &Node) -> bool,
        pick: impl Fn(Option<On>, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) {
        let found: Vec<(NodeId, Option<On>, Option<[f64; 3]>)> = self
            .nodes
            .iter()
            .filter(|(id, node)| !node.frozen && which(*id, node))
            .map(|(id, node)| {
                let on = pick(node.on, node.at);
                (id, on, on.and_then(|on| project(on, node.at)))
            })
            .collect();
        for (id, on, onto) in found {
            let node = self.nodes.get_mut(id).expect("live");
            node.on = on;
            if let Some(at) = onto {
                node.at = at;
            }
        }
    }

    /// Let the two sides of a patch of faces go, so that drawing one in does
    /// not drag the other with it.
    ///
    /// Where the map unsewed β₃ and left honeycomb to split whatever vertex
    /// orbits came apart, here it is said outright. The blocks are grouped
    /// by whether they can reach each other **without crossing the patch**;
    /// the group holding the chosen blocks keeps the cells of the patch,
    /// and every other group is handed its own copy of them.
    ///
    /// A group that can reach the chosen blocks anyway — two blocks joined
    /// along another face as well — is refused, because there is no space
    /// to open up between them.
    ///
    /// Returns where each node of the patch went on the far side, which is
    /// what a caller laying a layer into the gap needs: the far copy did
    /// not move, so it is where the layer's own far face belongs.
    fn separate(
        &mut self,
        picked: &[(FaceId, BlockId)],
    ) -> Result<HashMap<NodeId, NodeId>, BlockingError> {
        let cut: HashSet<FaceId> = picked.iter().map(|(face, _)| *face).collect();
        let front: HashSet<BlockId> = picked.iter().map(|(_, block)| *block).collect();

        // Which blocks can reach which without crossing the patch.
        let mut group: HashMap<BlockId, BlockId> = HashMap::new();
        fn root(group: &HashMap<BlockId, BlockId>, mut b: BlockId) -> BlockId {
            while let Some(up) = group.get(&b) {
                if *up == b {
                    break;
                }
                b = *up;
            }
            b
        }
        for (id, _) in self.blocks.iter() {
            group.insert(id, id);
        }
        for (id, face) in self.faces.iter() {
            if cut.contains(&id) {
                continue;
            }
            if let [Some(a), Some(b)] = face.sides {
                let (ra, rb) = (root(&group, a.block), root(&group, b.block));
                if ra != rb {
                    group.insert(rb, ra);
                }
            }
        }
        let ours: HashSet<BlockId> = front.iter().map(|b| root(&group, *b)).collect();

        // The far side of each face of the patch, and what it holds.
        let mut behind: Vec<(FaceId, Side)> = Vec::new();
        for (face, block) in picked {
            let f = self.faces.get(*face).expect("live");
            let Some(other) = f.sides.iter().flatten().find(|s| s.block != *block) else {
                continue;
            };
            if ours.contains(&root(&group, other.block)) {
                // Still the same group: the two blocks are joined somewhere
                // else as well, so drawing one in would drag the other.
                return Err(BlockingError::NotOnTheSurface(self.face_name(*face)));
            }
            behind.push((*face, *other));
        }
        if behind.is_empty() {
            return Ok(HashMap::new());
        }

        // A copy of every node and edge of the patch, for the far side.
        let mut node_copy: HashMap<NodeId, NodeId> = HashMap::new();
        let mut edge_copy: HashMap<EdgeId, EdgeId> = HashMap::new();
        for (face, _) in &behind {
            let f = self.faces.get(*face).expect("live").clone();
            for node in f.nodes {
                if node_copy.contains_key(&node) {
                    continue;
                }
                let was = self.nodes.get(node).expect("live").clone();
                let born = self.nodes.handed_out();
                let copy = self.nodes.insert(Node { born, ..was });
                node_copy.insert(node, copy);
            }
            for edge in f.edges {
                if edge_copy.contains_key(&edge) {
                    continue;
                }
                let was = self.edges.get(edge).expect("live").clone();
                let ends = was.ends.map(|n| node_copy[&n]);
                let copy = self.edges.insert(Edge { ends, ..was });
                edge_copy.insert(edge, copy);
            }
        }

        // Everything on the far side now names the copies.
        let theirs: Vec<BlockId> = self
            .blocks
            .ids()
            .filter(|b| !ours.contains(&root(&group, *b)))
            .collect();
        let over: HashSet<BlockId> = theirs.iter().copied().collect();
        for id in &theirs {
            let block = self.blocks.get_mut(*id).expect("live");
            for node in block.nodes.iter_mut() {
                if let Some(copy) = node_copy.get(node) {
                    *node = *copy;
                }
            }
            for edge in block.edges.iter_mut() {
                if let Some(copy) = edge_copy.get(edge) {
                    *edge = *copy;
                }
            }
        }
        for id in self.faces.ids().collect::<Vec<_>>() {
            if cut.contains(&id) {
                continue;
            }
            let face = self.faces.get(id).expect("live");
            // Only a face wholly on the far side: one with a side on each
            // is a face of the patch, and those are split below.
            if !face
                .sides
                .iter()
                .flatten()
                .all(|side| over.contains(&side.block))
                || face.sides.iter().flatten().count() == 0
            {
                continue;
            }
            let face = self.faces.get_mut(id).expect("live");
            for node in face.nodes.iter_mut() {
                if let Some(copy) = node_copy.get(node) {
                    *node = *copy;
                }
            }
            for edge in face.edges.iter_mut() {
                if let Some(copy) = edge_copy.get(edge) {
                    *edge = *copy;
                }
            }
        }

        // And each face of the patch becomes two: the one that was there
        // for this side, and a copy of it for the far side.
        for (face, side) in behind {
            let f = self.faces.get(face).expect("live").clone();
            let which = f
                .sides
                .iter()
                .position(|s| *s == Some(side))
                .expect("the far side");
            let mut nodes = f.nodes.map(|n| node_copy[&n]);
            let mut edges = f.edges.map(|e| edge_copy[&e]);
            if which == 1 {
                // The copy is this block's own face now, so it is wound the
                // way that block sees it — which is side 0's way round.
                nodes = reversed(nodes);
                edges = [edges[2], edges[1], edges[0], edges[3]];
            }
            let copy = self.faces.insert(Face {
                nodes,
                edges,
                sides: [Some(side), None],
                on: f.on,
            });
            self.blocks.get_mut(side.block).expect("live").faces[side.slot as usize] = copy;
            let f = self.faces.get_mut(face).expect("live");
            f.sides[which] = None;
            if which == 0 {
                f.sides[0] = f.sides[1].take();
                f.nodes = reversed(f.nodes);
                f.edges = [f.edges[2], f.edges[1], f.edges[0], f.edges[3]];
            }
        }
        self.forget();
        Ok(node_copy)
    }

    /// Raise a block out of each of these faces at once.
    ///
    /// The shared half of all three extrusions. Each side is a face and the
    /// four corners **as the thing growing out of it sees them**; the faces
    /// are consumed, becoming the bottoms of the blocks and carrying what
    /// they were classified on, because they are the same cells.
    ///
    /// Two things make the blocks fit together, and in this structure both
    /// are free:
    ///
    /// * **one step per node, not per face.** A corner on several of the
    ///   faces is one node, so it raises to one new node — and the map
    ///   needed a corner key to say that two faces shared a corner, where
    ///   here they share the `NodeId` itself. The step is the mitre of the
    ///   normals meeting there, so a ridge rises straight up the fold's own
    ///   plane of symmetry.
    /// * **no welding at all.** Two blocks raised from faces that share an
    ///   edge share the two corners of that edge and the two they raise to,
    ///   so `emplace_block` finds the face between them by its four nodes
    ///   and hands the second block its other side. What the map did with a
    ///   `glue_onto` per seam, in an order it had to be careful about,
    ///   happens here by nobody doing anything.
    fn raise_faces(
        &mut self,
        sides: &[(FaceId, [NodeId; 4])],
        height: f64,
        back: bool,
    ) -> Result<Vec<CellId>, BlockingError> {
        // Where every corner goes, by the node itself.
        let mut around: HashMap<NodeId, Vec<[f64; 3]>> = HashMap::new();
        for (_, winding) in sides {
            let points = winding.map(|n| self.nodes.get(n).expect("live").at);
            let normal = quad_normal(points);
            let length = (0..3).map(|k| normal[k] * normal[k]).sum::<f64>().sqrt();
            if length < 1e-12 {
                return Err(BlockingError::LostFace);
            }
            let unit = normal.map(|x| if back { -x / length } else { x / length });
            for node in winding {
                around.entry(*node).or_default().push(unit);
            }
        }
        let mut risen: HashMap<NodeId, NodeId> = HashMap::new();
        for (node, normals) in &around {
            let step = mitre(normals, height);
            let at = self.nodes.get(*node).expect("live").at;
            let born = self.nodes.handed_out();
            let up = self.nodes.insert(Node {
                at: [0, 1, 2].map(|k| at[k] + step[k]),
                // A node raised off the model follows nothing: it is not on
                // the boundary any more.
                on: None,
                frozen: false,
                born,
            });
            risen.insert(*node, up);
        }

        let mut known = self.known();
        let mut out = Vec::with_capacity(sides.len());
        for (_, winding) in sides {
            // The block's own bottom is this face wound the other way: what
            // is outside the face is inside the block growing out of it.
            let base = if back {
                [winding[0], winding[3], winding[2], winding[1]]
            } else {
                [winding[3], winding[0], winding[1], winding[2]]
            };
            let corners = [
                base[0],
                base[1],
                base[2],
                base[3],
                risen[&base[0]],
                risen[&base[1]],
                risen[&base[2]],
                risen[&base[3]],
            ];
            let block = self.emplace_block(corners, &mut known)?;
            out.push(self.block_name(block));
        }
        self.forget();
        Ok(out)
    }

    /// A face of a block, with the four corners that block winds it by —
    /// and only if it is on the surface, since a face with a block on each
    /// side has no room to grow into.
    fn free_side(
        &self,
        face: CellId,
        block: CellId,
    ) -> Result<(FaceId, [NodeId; 4]), BlockingError> {
        let id = self.face_at(face).ok_or(BlockingError::NoSuchFace(face))?;
        let at = self
            .block_at(block)
            .ok_or(BlockingError::NoSuchBlock(block))?;
        let (_, which) = self
            .slot_of(id, at)
            .ok_or(BlockingError::FaceNotOnBlock(face, block))?;
        let f = self.faces.get(id).expect("live");
        if f.sides.iter().flatten().count() > 1 {
            return Err(BlockingError::NotOnTheSurface(face));
        }
        let winding = if which == 1 {
            reversed(f.nodes)
        } else {
            f.nodes
        };
        Ok((id, winding))
    }

    /// Whether anything still holds this edge.
    /// Take a block away, leaving whatever it was holding behind.
    ///
    /// A face it shared with a neighbour stays — that is the neighbour's
    /// boundary now — and where the map unsewed β₃ so the neighbour could
    /// be handed its own copy of the shared cells, there is nothing to
    /// copy: the neighbour already names them, which is the whole of what
    /// this structure is for.
    ///
    /// What **only** this block was holding is left behind as well, and
    /// [`collect`][Self::collect] is what clears it. Separating the two is
    /// not tidiness: a cell is held or not by the whole structure, so
    /// asking per block means a sweep per block, and a layer of four
    /// hundred paid four hundred sweeps. The cells it was holding come back
    /// so that a caller taking several blocks away can ask once.
    fn detach_block(&mut self, id: BlockId) -> Result<Block, BlockingError> {
        let block = self
            .blocks
            .get(id)
            .cloned()
            .ok_or(BlockingError::NoSuchBlock(self.block_name(id)))?;
        for face in block.faces {
            self.detach(face, id);
        }
        self.blocks.remove(id);
        self.forget();
        Ok(block)
    }

    /// Clear away whatever nothing holds any more, among these cells.
    ///
    /// Three passes over the structure for however many blocks were taken
    /// away, rather than twenty per block. A face with no side left goes, an
    /// edge no surviving face or block names goes, and a node no surviving
    /// edge, face or block names goes — in that order, because each one is
    /// what makes the next true.
    fn collect(&mut self, faces: &[FaceId], edges: &[EdgeId], nodes: &[NodeId]) {
        for face in faces {
            if self
                .faces
                .get(*face)
                .is_some_and(|f| f.sides.iter().all(|side| side.is_none()))
            {
                self.faces.remove(*face);
            }
        }

        let mut held: HashSet<EdgeId> = HashSet::new();
        for (_, face) in self.faces.iter() {
            held.extend(face.edges);
        }
        for (_, block) in self.blocks.iter() {
            held.extend(block.edges);
        }
        for edge in edges {
            if !held.contains(edge) {
                self.edges.remove(*edge);
            }
        }

        let mut held: HashSet<NodeId> = HashSet::new();
        for (_, edge) in self.edges.iter() {
            held.extend(edge.ends);
        }
        for (_, face) in self.faces.iter() {
            held.extend(face.nodes);
        }
        for (_, block) in self.blocks.iter() {
            held.extend(block.nodes);
        }
        for node in nodes {
            if !held.contains(node) {
                self.nodes.remove(*node);
            }
        }
        self.forget();
    }

    /// One block away, and whatever only it was holding.
    fn drop_block(&mut self, id: BlockId) -> Result<(), BlockingError> {
        let block = self.detach_block(id)?;
        self.collect(&block.faces, &block.edges, &block.nodes);
        Ok(())
    }

    /// Put every group of nodes a set of seams is about to merge where the
    /// merge would leave them: at their own average.
    ///
    /// A corner two seams share belongs to both, so the groups are the
    /// connected components of the pairs and not the pairs themselves —
    /// otherwise what each seam did would depend on what the one before it
    /// did, and the answer would depend on the order. Settled first, the
    /// two faces of a seam coincide exactly and no later seam can pull
    /// them apart.
    ///
    /// Nowhere does this decide **which** node meets which: that is read
    /// off the structure, which is why a removal works on a model however
    /// distorted.
    fn settle_seams(&mut self, pairs: &[(NodeId, NodeId)]) {
        fn root(parent: &HashMap<NodeId, NodeId>, mut v: NodeId) -> NodeId {
            while let Some(&up) = parent.get(&v) {
                if up == v {
                    break;
                }
                v = up;
            }
            v
        }
        let mut parent: HashMap<NodeId, NodeId> = HashMap::new();
        for (a, b) in pairs {
            for v in [a, b] {
                parent.entry(*v).or_insert(*v);
            }
            let (ra, rb) = (root(&parent, *a), root(&parent, *b));
            if ra != rb {
                parent.insert(rb, ra);
            }
        }
        let mut together: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for v in parent.keys() {
            together.entry(root(&parent, *v)).or_default().push(*v);
        }
        for (_, mut nodes) in together {
            nodes.sort_unstable();
            let known: Vec<[f64; 3]> = nodes
                .iter()
                .filter_map(|v| self.nodes.get(*v).map(|n| n.at))
                .collect();
            if known.is_empty() {
                continue;
            }
            let share = known.len() as f64;
            let at = [0, 1, 2].map(|k| known.iter().map(|p| p[k]).sum::<f64>() / share);
            for v in nodes {
                if let Some(node) = self.nodes.get_mut(v) {
                    node.at = at;
                }
            }
        }
    }

    /// Take out the whole layer of blocks through an edge.
    ///
    /// `close` says whether the gap closes: the two caps of each block the
    /// layer crosses are welded to each other once it is gone, which is
    /// what makes a layer removal leave a model with sound walls rather
    /// than a hole.
    ///
    /// **Which cap meets which is a question about the structure, not
    /// about where anything is.** The two caps of a block are the two ends
    /// of its own axis, and corner `k` of one answers to corner `k + 4` of
    /// the other in the frame that axis names — so a layer comes out of a
    /// model however distorted.
    fn take_sheet(&mut self, edge: CellId, close: bool) -> Result<(), BlockingError> {
        let sheet = self.sheet_through(edge)?;
        let crossed: HashSet<EdgeId> = sheet.iter().filter_map(|s| self.edge_at(s.edge)).collect();

        let mut doomed: Vec<BlockId> = Vec::new();
        for id in &crossed {
            doomed.extend(self.blocks_on(*id));
        }
        doomed.sort_unstable();
        doomed.dedup();

        // What each doomed block's two caps are, and which of their corners
        // answer to which — read before anything is taken away.
        struct Seam {
            caps: [FaceId; 2],
            pairs: [(NodeId, NodeId); 4],
        }
        let mut seams: Vec<Seam> = Vec::new();
        for id in &doomed {
            let block = self.blocks.get(*id).expect("live").clone();
            let along: Vec<usize> = (0..12)
                .filter(|k| crossed.contains(&block.edges[*k]))
                .collect();
            let axis = EDGE_AXIS[along[0]];
            if along.len() != 4 || along.iter().any(|k| EDGE_AXIS[*k] != axis) {
                return Err(BlockingError::BrokenSheet(edge));
            }
            let frame = SPLIT_FRAME[axis as usize].map(|c| block.nodes[c]);
            let low: HashSet<NodeId> = frame[..4].iter().copied().collect();
            let high: HashSet<NodeId> = frame[4..].iter().copied().collect();
            let cap = |want: &HashSet<NodeId>| -> Option<(u8, FaceId)> {
                block.faces.iter().enumerate().find_map(|(slot, face)| {
                    let f = self.faces.get(*face)?;
                    f.nodes
                        .iter()
                        .all(|n| want.contains(n))
                        .then_some((slot as u8, *face))
                })
            };
            let (Some((slot_low, cap_low)), Some((slot_high, cap_high))) = (cap(&low), cap(&high))
            else {
                return Err(BlockingError::BrokenSheet(edge));
            };
            if !close {
                continue;
            }
            let (below, above) = (
                self.beyond(
                    cap_low,
                    Side {
                        block: *id,
                        slot: slot_low,
                    },
                ),
                self.beyond(
                    cap_high,
                    Side {
                        block: *id,
                        slot: slot_high,
                    },
                ),
            );
            let (Some(below), Some(above)) = (below, above) else {
                // A cap with nothing behind it is a cap on the model's
                // surface: there is nothing to close against, so the model
                // simply loses that layer.
                continue;
            };
            // A sheet that comes back round onto itself has no outside to
            // close against.
            if doomed.contains(&below.block) || doomed.contains(&above.block) {
                return Err(BlockingError::BrokenSheet(edge));
            }
            seams.push(Seam {
                caps: [cap_low, cap_high],
                pairs: [0, 1, 2, 3].map(|k| (frame[k], frame[k + 4])),
            });
        }

        // The seams are settled while the layer still holds the two sides
        // apart, and welded once it is gone.
        let pairs: Vec<(NodeId, NodeId)> = seams.iter().flat_map(|seam| seam.pairs).collect();
        self.settle_seams(&pairs);
        // The whole layer detached first and swept once, not swept per
        // block: what a cell is held by is a question about the structure,
        // so asking it per block is asking it four hundred times over.
        let mut faces: Vec<FaceId> = Vec::new();
        let mut edges: Vec<EdgeId> = Vec::new();
        let mut nodes: Vec<NodeId> = Vec::new();
        for id in doomed {
            let block = self.detach_block(id)?;
            faces.extend(block.faces);
            edges.extend(block.edges);
            nodes.extend(block.nodes);
        }
        self.collect(&faces, &edges, &nodes);
        // A corner two seams share is merged by the first of them, so the
        // second has to be told where it went. Nine blocks in a layer and
        // the caps of neighbouring ones share their corners, which a layer
        // one block wide never shows.
        let mut moved: HashMap<NodeId, NodeId> = HashMap::new();
        // One index for the whole layer, kept true by the welds as they go
        // — and built only if there are any, since building it is itself a
        // sweep and a layer taken out leaving the gap open welds nothing.
        let mut holders = if seams.is_empty() {
            None
        } else {
            Some(self.holders())
        };
        let follow = |moved: &HashMap<NodeId, NodeId>, mut n: NodeId| {
            while let Some(to) = moved.get(&n) {
                if *to == n {
                    break;
                }
                n = *to;
            }
            n
        };
        for seam in seams {
            let [keep, gone] = seam.caps;
            let (Some(here), Some(there)) =
                (self.faces.get(keep).cloned(), self.faces.get(gone).cloned())
            else {
                continue;
            };
            // Which quarter turn lines the two up is read off the pairs,
            // not off where the corners are.
            let partner: HashMap<NodeId, NodeId> = seam
                .pairs
                .iter()
                .map(|(a, b)| (follow(&moved, *a), follow(&moved, *b)))
                .flat_map(|(a, b)| [(a, b), (b, a)])
                .collect();
            let want = partner
                .get(&here.nodes[0])
                .copied()
                .ok_or(BlockingError::BrokenSheet(edge))?;
            let turn = there
                .nodes
                .iter()
                .position(|n| *n == want)
                .ok_or(BlockingError::BrokenSheet(edge))?;
            let decided = [0, 1, 2, 3].map(|i| {
                let mine = self.nodes.get(here.nodes[i]).expect("live");
                let theirs = self
                    .nodes
                    .get(there.nodes[(turn + 4 - i) % 4])
                    .expect("live");
                Welded {
                    at: mine.at,
                    on: mine.on.or(theirs.on),
                    frozen: mine.frozen || theirs.frozen,
                }
            });
            let holders = holders.as_mut().expect("a seam to weld");
            for (from, to) in self.join_faces(keep, gone, turn, decided, holders)? {
                moved.insert(from, to);
            }
        }
        self.forget();
        Ok(())
    }

    /// Where every edge and face is, by the nodes it holds.
    fn known(&self) -> Known {
        let mut by_pair = HashMap::new();
        for (id, edge) in self.edges.iter() {
            let mut key = edge.ends;
            key.sort_unstable();
            by_pair.insert(key, id);
        }
        let mut by_quad = HashMap::new();
        for (id, face) in self.faces.iter() {
            let mut key = face.nodes;
            key.sort_unstable();
            by_quad.insert(key, id);
        }
        Known { by_pair, by_quad }
    }

    /// The edge between two nodes, made if nobody holds it.
    ///
    /// `like` is what a fresh one inherits — what a cut edge was classified
    /// on and how finely it was divided — so that cutting an edge along a
    /// curve leaves two edges along that curve.
    fn edge_on(
        &mut self,
        from: NodeId,
        to: NodeId,
        known: &mut Known,
        like: (Option<On>, u32),
    ) -> EdgeId {
        let mut key = [from, to];
        key.sort_unstable();
        if let Some(id) = known.by_pair.get(&key) {
            return *id;
        }
        let id = self.edges.insert(Edge {
            ends: [from, to],
            segments: like.1,
            on: like.0,
        });
        known.by_pair.insert(key, id);
        id
    }

    /// A face on four nodes, made if nobody holds it, with no side taken.
    fn face_on(&mut self, winding: [NodeId; 4], known: &mut Known) -> FaceId {
        let mut key = winding;
        key.sort_unstable();
        if let Some(id) = known.by_quad.get(&key) {
            return *id;
        }
        let edges =
            [0, 1, 2, 3].map(|k| self.edge_on(winding[k], winding[(k + 1) % 4], known, (None, 0)));
        let id = self.faces.insert(Face {
            nodes: winding,
            edges,
            sides: [None, None],
            on: None,
        });
        known.by_quad.insert(key, id);
        id
    }

    /// Put a block on eight corners that already exist, sharing whatever is
    /// already there.
    ///
    /// The primitive the cellular structure makes easy and the map made
    /// hard: twelve edges and six faces found by the nodes they hold, or
    /// made if nobody holds them, and each told which side of it this block
    /// is. A face already bounding another block gains its **second** side
    /// and nothing is sewn.
    ///
    /// Cutting, extruding and pillowing are all this, several times over,
    /// which is most of what those operations were.
    fn emplace_block(
        &mut self,
        nodes: [NodeId; 8],
        known: &mut Known,
    ) -> Result<BlockId, BlockingError> {
        let edges: [EdgeId; 12] =
            HEX_EDGES.map(|[a, b]| self.edge_on(nodes[a], nodes[b], known, (None, 0)));

        // Found or made before the block exists to be named as a side, and
        // which side each is settled here: a face wound the way this block
        // winds it is this block's side 0, and one wound the other way is
        // its side 1. Anything else is two blocks on one side of a face,
        // which is not a blocking.
        let mut wanted = [(FaceId(0), 0usize); 6];
        for (slot, want) in wanted.iter_mut().enumerate() {
            let winding = HEX_FACES[slot].map(|c| nodes[c]);
            let id = self.face_on(winding, known);
            let face = self.faces.get(id).expect("found or made");
            let which = if face.sides[0].is_none() {
                0
            } else if face.sides[1].is_none() && rotation_of(&face.nodes, &reversed(winding)) {
                1
            } else {
                return Err(BlockingError::AlreadyGlued(self.face_name(id)));
            };
            *want = (id, which);
        }

        let block = self.blocks.insert(Block {
            nodes,
            edges,
            faces: wanted.map(|(id, _)| id),
            groups: 0,
        });
        for (slot, (id, which)) in wanted.into_iter().enumerate() {
            let winding = HEX_FACES[slot].map(|c| nodes[c]);
            if which == 0 {
                // Side 0 reads `nodes` forward, so a face that had no side
                // yet is wound this block's way — and its sides with it.
                let edges = [0, 1, 2, 3]
                    .map(|k| self.edge_on(winding[k], winding[(k + 1) % 4], known, (None, 0)));
                let face = self.faces.get_mut(id).expect("found or made");
                face.nodes = winding;
                face.edges = edges;
            }
            self.faces.get_mut(id).expect("found or made").sides[which] = Some(Side {
                block,
                slot: slot as u8,
            });
        }
        self.forget();
        Ok(block)
    }

    /// Take a block's claim off one of its faces.
    ///
    /// A face that loses its **first** side while keeping its second has to
    /// be turned round, because side 0 is the one `nodes` is wound for and
    /// invariant 6 says there is no second side without a first. Which is
    /// four lines here and was a β₃ unsew there.
    fn detach(&mut self, face: FaceId, block: BlockId) {
        let Some(f) = self.faces.get_mut(face) else {
            return;
        };
        for side in f.sides.iter_mut() {
            if side.is_some_and(|s| s.block == block) {
                *side = None;
            }
        }
        if f.sides[0].is_none() && f.sides[1].is_some() {
            f.sides[0] = f.sides[1].take();
            f.nodes = reversed(f.nodes);
            // Side `k` of a cycle read backwards is side `2 - k` of the
            // cycle read forwards.
            f.edges = [f.edges[2], f.edges[1], f.edges[0], f.edges[3]];
        }
        self.forget();
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
        // Asked **before** the names are resolved: gluing a face to itself
        // is a complaint about the request and not about the structure, and
        // it is the same complaint whether or not the name is one anything
        // answers to.
        if first == second {
            return Err(BlockingError::SameFace);
        }
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

    /// Who holds each node and each edge: one sweep, for an operation that
    /// is about to merge several of them.
    fn holders(&self) -> Holders {
        let mut out = Holders {
            node_edges: HashMap::new(),
            node_faces: HashMap::new(),
            node_blocks: HashMap::new(),
            edge_faces: HashMap::new(),
            edge_blocks: HashMap::new(),
        };
        for (id, edge) in self.edges.iter() {
            for end in edge.ends {
                out.node_edges.entry(end).or_default().push(id);
            }
        }
        for (id, face) in self.faces.iter() {
            for node in face.nodes {
                out.node_faces.entry(node).or_default().push(id);
            }
            for edge in face.edges {
                out.edge_faces.entry(edge).or_default().push(id);
            }
        }
        for (id, block) in self.blocks.iter() {
            for node in block.nodes {
                out.node_blocks.entry(node).or_default().push(id);
            }
            for edge in block.edges {
                out.edge_blocks.entry(edge).or_default().push(id);
            }
        }
        out
    }

    /// Every reference to one node becomes a reference to another, and the
    /// first is gone.
    ///
    /// Only what holds it is touched, which is the whole point of knowing
    /// who does: this used to sweep all three arenas, and a weld merges
    /// four corners, so taking a layer of four hundred blocks out of a
    /// model swept it three thousand times. Measured on a twenty-cubed
    /// grid: 111 ms of the 124 a layer removal took.
    fn rewrite_node(&mut self, from: NodeId, to: NodeId, holders: &mut Holders) {
        let edges = holders.node_edges.remove(&from).unwrap_or_default();
        for id in &edges {
            if let Some(edge) = self.edges.get_mut(*id) {
                for end in edge.ends.iter_mut() {
                    if *end == from {
                        *end = to;
                    }
                }
            }
        }
        let faces = holders.node_faces.remove(&from).unwrap_or_default();
        for id in &faces {
            if let Some(face) = self.faces.get_mut(*id) {
                for node in face.nodes.iter_mut() {
                    if *node == from {
                        *node = to;
                    }
                }
            }
        }
        let blocks = holders.node_blocks.remove(&from).unwrap_or_default();
        for id in &blocks {
            if let Some(block) = self.blocks.get_mut(*id) {
                for node in block.nodes.iter_mut() {
                    if *node == from {
                        *node = to;
                    }
                }
            }
        }
        // What held the one now holds the other.
        holders.node_edges.entry(to).or_default().extend(edges);
        holders.node_faces.entry(to).or_default().extend(faces);
        holders.node_blocks.entry(to).or_default().extend(blocks);
        self.nodes.remove(from);
    }

    /// The same for an edge, which only faces and blocks can hold.
    fn rewrite_edge(&mut self, from: EdgeId, to: EdgeId, holders: &mut Holders) {
        let faces = holders.edge_faces.remove(&from).unwrap_or_default();
        for id in &faces {
            if let Some(face) = self.faces.get_mut(*id) {
                for edge in face.edges.iter_mut() {
                    if *edge == from {
                        *edge = to;
                    }
                }
            }
        }
        let blocks = holders.edge_blocks.remove(&from).unwrap_or_default();
        for id in &blocks {
            if let Some(block) = self.blocks.get_mut(*id) {
                for edge in block.edges.iter_mut() {
                    if *edge == from {
                        *edge = to;
                    }
                }
            }
        }
        holders.edge_faces.entry(to).or_default().extend(faces);
        holders.edge_blocks.entry(to).or_default().extend(blocks);
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
        holders: &mut Holders,
    ) -> Result<Vec<(NodeId, NodeId)>, BlockingError> {
        let here = self.faces.get(keep).expect("live").clone();
        let there = self.faces.get(gone).expect("live").clone();
        let side = there.sides[0].expect("a face being welded has one side");

        let mut merged: Vec<(NodeId, NodeId)> = Vec::new();
        for (i, told) in decided.iter().enumerate() {
            let (a, b) = (here.nodes[i], there.nodes[(turn + 4 - i) % 4]);
            {
                let node = self.nodes.get_mut(a).expect("live");
                node.at = told.at;
                node.on = told.on;
                node.frozen = told.frozen;
            }
            if a != b {
                self.rewrite_node(b, a, holders);
                merged.push((b, a));
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
            self.rewrite_edge(gone_edge, keep_edge, holders);
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
        Ok(merged)
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

    /// Read what climbs, building it if it has been dropped.
    ///
    /// A closure rather than a returned reference, because the cache lives
    /// behind a `RefCell` and handing out a borrow across it would make
    /// every caller responsible for not holding it. One sweep of the
    /// arenas, amortised over everything that asks.
    fn climbing<T>(&self, ask: impl FnOnce(&Up) -> T) -> T {
        if self.up.borrow().is_none() {
            let mut built = Up::default();
            for (id, edge) in self.edges.iter() {
                for end in edge.ends {
                    built.node_edges.entry(end).or_default().push(id);
                }
            }
            for (id, face) in self.faces.iter() {
                for edge in face.edges {
                    built.edge_faces.entry(edge).or_default().push(id);
                }
                for node in face.nodes {
                    built.node_faces.entry(node).or_default().push(id);
                }
            }
            for (id, block) in self.blocks.iter() {
                for edge in block.edges {
                    built.edge_blocks.entry(edge).or_default().push(id);
                }
            }
            *self.up.borrow_mut() = Some(built);
        }
        ask(self.up.borrow().as_ref().expect("just built"))
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
        let share = how.share.clamp(0.0, 1.0);
        if share == 0.0 || how.rounds == 0 {
            return 0.0;
        }
        let held: HashSet<CellId> = how.held.iter().copied().collect();

        // By name, so the order does not depend on where anything happens
        // to sit in an arena.
        let mut movable: Vec<(CellId, NodeId)> = self
            .nodes
            .iter()
            .filter(|(_, node)| {
                !node.frozen
                    // A point is a point: a node on a geometric corner has
                    // one place to be.
                    && node.on.is_none_or(|on| on.dim > 0)
            })
            .map(|(id, _)| (self.node_name(id), id))
            .filter(|(name, id)| {
                !held.contains(name)
                    // Free inside, held on the boundary.
                    && (self.nodes.get(*id).expect("live").on.is_some()
                        || !self.on_the_surface(*name))
            })
            .collect();
        movable.sort_by_key(|(name, _)| *name);

        let mut furthest = 0.0f64;
        for _ in 0..how.rounds {
            for &(name, id) in &movable {
                let ring: Vec<[f64; 3]> = self
                    .neighbors(name)
                    .into_iter()
                    .filter_map(|other| self.position(other))
                    .collect();
                if ring.is_empty() {
                    continue;
                }
                let from = self.nodes.get(id).expect("live").at;
                let middle = self.middle(&ring);
                let mut to = [0, 1, 2].map(|k| from[k] + (middle[k] - from[k]) * share);
                // Back onto whatever it follows, before anybody judges it:
                // the point to weigh is the one the node would end up at.
                if let Some(on) = self.nodes.get(id).expect("live").on {
                    if let Some(onto) = project(on, to) {
                        to = onto;
                    }
                }
                let step = apart(from, to);
                if step == 0.0 {
                    continue;
                }

                let before = self.worst_corner_at(id);
                self.nodes.get_mut(id).expect("live").at = to;
                if self.worst_corner_at(id) < before {
                    // Not an improvement. A Laplacian on a concave
                    // boundary would have taken it and turned a cell
                    // inside out.
                    self.nodes.get_mut(id).expect("live").at = from;
                    continue;
                }
                furthest = furthest.max(step);
            }
        }
        furthest
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
        let at = self.node_mut(node)?;
        // A frozen node does not move for anybody. Refused rather than
        // silently ignored: a caller that asked for a move and got nothing
        // back would go on believing it happened.
        if at.frozen {
            return Err(BlockingError::Frozen(node));
        }
        at.at = to;
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
        self.classify_some(|_, _| true, pick, project)
    }

    pub fn classify_named(
        &mut self,
        nodes: &[CellId],
        pick: impl Fn(Option<On>, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) {
        let wanted: HashSet<NodeId> = nodes.iter().filter_map(|n| self.node_at(*n)).collect();
        self.classify_some(|id, _| wanted.contains(&id), pick, project)
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
        let mut holders = self.holders();
        self.join_faces(a, b, turn, decided, &mut holders)?;
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
        let mut holders = self.holders();
        self.join_faces(a, b, turn, decided, &mut holders)?;
        Ok(())
    }

    pub fn sheet_blocks(&self, sheet: &[SheetEdge]) -> usize {
        let mut blocks: Vec<BlockId> = sheet
            .iter()
            .filter_map(|side| self.edge_at(side.edge))
            .flat_map(|edge| self.blocks_on(edge))
            .collect();
        blocks.sort_unstable();
        blocks.dedup();
        blocks.len()
    }

    pub fn sheet_cells(&self, edge: CellId) -> Result<SheetCells, BlockingError> {
        let sheet = self.sheet_through(edge)?;
        let mut blocks: Vec<BlockId> = Vec::new();
        for side in &sheet {
            let id = self
                .edge_at(side.edge)
                .ok_or(BlockingError::NoSuchEdge(side.edge))?;
            blocks.extend(self.blocks_on(id));
        }
        blocks.sort_unstable();
        blocks.dedup();

        // Everything that bounds one of them. Sorted by name, which is what
        // makes the answer the same whatever order the walk found things in.
        let mut nodes: BTreeSet<CellId> = BTreeSet::new();
        let mut edges: BTreeSet<CellId> = BTreeSet::new();
        let mut faces: BTreeSet<CellId> = BTreeSet::new();
        for id in &blocks {
            let block = self.blocks.get(*id).expect("live");
            nodes.extend(block.nodes.iter().map(|n| self.node_name(*n)));
            edges.extend(block.edges.iter().map(|e| self.edge_name(*e)));
            faces.extend(block.faces.iter().map(|f| self.face_name(*f)));
        }
        let named = |ids: BTreeSet<CellId>| -> Vec<CellId> {
            ids.into_iter().filter(|c| c.is_some()).collect()
        };
        Ok(SheetCells {
            sheet: sheet.iter().map(|s| s.edge).min().unwrap_or(edge),
            nodes: named(nodes),
            edges: named(edges),
            faces: named(faces),
            blocks: blocks.iter().map(|b| self.block_name(*b)).collect(),
        })
    }

    pub fn sheets_beside(&self, edge: CellId) -> Result<[Option<CellId>; 2], BlockingError> {
        let sheet = self.sheet_through(edge)?;
        let crossed: HashSet<CellId> = sheet.iter().map(|s| s.edge).collect();
        let start = self.edge_at(edge).ok_or(BlockingError::NoSuchEdge(edge))?;
        let holding = self.blocks_on(start);
        let [block] = holding[..1] else {
            return Err(BlockingError::BrokenSheet(edge));
        };

        // The caps: the two faces of this block that the sheet does not
        // cross. Which is the same as the two perpendicular to the sheet's
        // own axis, and asking it this way needs no axis to be worked out.
        let faces = self.blocks.get(block).expect("live").faces;
        let caps: Vec<(u8, FaceId)> = faces
            .iter()
            .enumerate()
            .filter(|(_, face)| {
                self.faces
                    .get(**face)
                    .expect("live")
                    .edges
                    .iter()
                    .all(|e| !crossed.contains(&self.edge_name(*e)))
            })
            .map(|(slot, face)| (slot as u8, *face))
            .collect();
        let [first, second] = caps[..] else {
            return Err(BlockingError::BrokenSheet(edge));
        };

        let mut out = [None, None];
        for (slot, (at, cap)) in out.iter_mut().zip([first, second]) {
            let Some(there) = self.beyond(cap, Side { block, slot: at }) else {
                continue;
            };
            let (next, there) = (there.block, there.slot);
            // In the block beyond, an edge running the way the sheet does:
            // the cap is flat in one axis, and the edges of that axis are
            // the ones that cross it. Where the map walked darts, the
            // tables say it.
            let axis = FACE_AXIS[there as usize];
            let along = self.blocks.get(next).expect("live").edges;
            let Some(at) = (0..12).find(|k| EDGE_AXIS[*k] == axis) else {
                continue;
            };
            let name = self.edge_name(along[at]);
            if name.is_none() {
                continue;
            }
            // A layer that cannot be read is a layer there is no stepping
            // to, which is the same answer as a cap on the surface.
            *slot = self
                .sheet_through(name)
                .ok()
                .and_then(|sheet| sheet.iter().map(|s| s.edge).min());
        }
        Ok(out)
    }

    pub fn remove_sheet(&mut self, edge: CellId) -> Result<(), BlockingError> {
        self.take_sheet(edge, true)
    }

    pub fn remove_sheet_leaving_a_gap(&mut self, edge: CellId) -> Result<(), BlockingError> {
        self.take_sheet(edge, false)
    }

    pub fn remove_block(&mut self, block: CellId) -> Result<(), BlockingError> {
        let id = self
            .block_at(block)
            .ok_or(BlockingError::NoSuchBlock(block))?;
        self.drop_block(id)
    }

    pub fn remove_face(&mut self, face: CellId) -> Result<(), BlockingError> {
        let id = self.face_at(face).ok_or(BlockingError::NoSuchFace(face))?;
        let f = self.faces.get(id).expect("live");
        // A face of a block is how you point at the block; taking it away
        // on its own would leave a block with a hole in it.
        if f.sides.iter().any(|side| side.is_some()) {
            return Err(BlockingError::NotAPatch(face));
        }
        let (nodes, edges) = (f.nodes, f.edges);
        self.faces.remove(id);
        // Whatever it alone was holding. A neighbour in the same patch
        // keeps the side and the corners they shared, which is where the
        // map had to unsew β₂ to hand them back.
        self.collect(&[], &edges, &nodes);
        Ok(())
    }

    pub fn sheet_through(&self, edge: CellId) -> Result<Vec<SheetEdge>, BlockingError> {
        let start = self.edge_at(edge).ok_or(BlockingError::NoSuchEdge(edge))?;
        let ends = self.edges.get(start).expect("live").ends;

        // The same walk the map did, and in the same words: out of an edge
        // through each face that holds it, to the edge **across** that
        // quadrangle — the one opposite, oriented to match. `across` is
        // four ids and a cycle, which is why it needed no porting at all.
        //
        // Keyed by name and walked in order, so the answer does not depend
        // on the order anything happened to hand things back in.
        let mut found: BTreeMap<CellId, SheetEdge> = BTreeMap::new();
        found.insert(
            edge,
            SheetEdge {
                edge,
                from: self.node_name(ends[0]),
                to: self.node_name(ends[1]),
            },
        );
        let mut pending = vec![(start, ends[0], ends[1])];

        while let Some((current, from, to)) = pending.pop() {
            for face in self.faces_on(current) {
                let quad = self.faces.get(face).expect("live").nodes;
                let Some((next_from, next_to)) = across(quad, from, to) else {
                    continue;
                };
                let Some(next) = self.edge_between(face, next_from, next_to) else {
                    continue;
                };
                let name = self.edge_name(next);
                let (named_from, named_to) = (self.node_name(next_from), self.node_name(next_to));
                if let Some(seen) = found.get(&name) {
                    if (seen.from, seen.to) != (named_from, named_to) {
                        return Err(BlockingError::TangledSheet(edge));
                    }
                    continue;
                }
                found.insert(
                    name,
                    SheetEdge {
                        edge: name,
                        from: named_from,
                        to: named_to,
                    },
                );
                pending.push((next, next_from, next_to));
            }
        }
        Ok(found.into_values().collect())
    }

    pub fn chord_through(
        &self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<ChordBlock>, BlockingError> {
        let id = self.face_at(face).ok_or(BlockingError::NoSuchFace(face))?;
        let start = match self.block_at(block) {
            Some(start) => start,
            None => {
                // A chord steps out of a block through the face opposite,
                // which an open sheet of faces does not have: it would walk
                // out of the structure. A patch being named by a face of
                // its own, that is what a name no block answers to and some
                // loose face does means.
                let loose = self.face_at(block).is_some_and(|f| {
                    self.faces
                        .get(f)
                        .is_some_and(|face| face.sides[0].is_none())
                });
                return Err(if loose {
                    BlockingError::NotABlock(block)
                } else {
                    BlockingError::NoSuchBlock(block)
                });
            }
        };
        let (slot, _) = self
            .slot_of(id, start)
            .ok_or(BlockingError::FaceNotOnBlock(face, block))?;

        // One block of the chord, entered at a slot: the block, the face
        // in, the face out, and what is beyond the far face — which is the
        // way on. `OPPOSITE` is where the map walked three betas.
        let step = |at: BlockId, slot: u8| {
            let faces = self.blocks.get(at).expect("live").faces;
            let out = faces[OPPOSITE[slot as usize] as usize];
            (
                ChordBlock {
                    block: self.block_name(at),
                    through: [self.face_name(faces[slot as usize]), self.face_name(out)],
                },
                out,
            )
        };

        let mut ahead: Vec<ChordBlock> = Vec::new();
        let mut closed = false;
        let mut here = Side { block: start, slot };
        loop {
            let (crossing, out) = step(here.block, here.slot);
            ahead.push(crossing);
            let leaving = Side {
                block: here.block,
                slot: OPPOSITE[here.slot as usize],
            };
            let Some(next) = self.beyond(out, leaving) else {
                break;
            };
            if next == (Side { block: start, slot }) {
                // A ring: going on would walk it for ever, and going back
                // would walk it again the other way.
                closed = true;
                break;
            }
            here = next;
        }

        let mut behind: Vec<ChordBlock> = Vec::new();
        if !closed {
            let mut back = self.beyond(id, Side { block: start, slot });
            while let Some(side) = back {
                let (crossing, out) = step(side.block, side.slot);
                behind.push(crossing);
                back = self.beyond(
                    out,
                    Side {
                        block: side.block,
                        slot: OPPOSITE[side.slot as usize],
                    },
                );
            }
        }
        behind.reverse();
        behind.extend(ahead);

        let mut once: Vec<CellId> = behind.iter().map(|c| c.block).collect();
        once.sort_unstable();
        let crossings = once.len();
        once.dedup();
        if once.len() != crossings {
            return Err(BlockingError::TangledChord(face));
        }
        Ok(behind)
    }

    pub fn chord_wire(&self, chord: &[ChordBlock]) -> Vec<(CellId, CellId, CellId)> {
        let mut out: Vec<(CellId, CellId, CellId)> = Vec::new();
        for step in chord {
            for name in step.through {
                let Some(id) = self.face_at(name) else {
                    continue;
                };
                let face = self.faces.get(id).expect("live");
                for k in 0..4 {
                    let edge = self.edge_name(face.edges[k]);
                    if out.iter().any(|(seen, ..)| *seen == edge) {
                        continue;
                    }
                    out.push((
                        edge,
                        self.node_name(face.nodes[k]),
                        self.node_name(face.nodes[(k + 1) % 4]),
                    ));
                }
            }
        }
        out
    }

    pub fn remove_chord(
        &mut self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<CellId>, BlockingError> {
        // One `remove_block` after another, in the order the chord runs
        // in. Each removal leaves whatever the block was glued to closed
        // again, so a chord taken out of the middle leaves a tunnel with
        // sound walls rather than anything needing repair.
        let chord = self.chord_through(face, block)?;
        let mut gone = Vec::with_capacity(chord.len());
        let (mut faces, mut edges, mut nodes) = (Vec::new(), Vec::new(), Vec::new());
        for step in &chord {
            let id = self
                .block_at(step.block)
                .ok_or(BlockingError::NoSuchBlock(step.block))?;
            let was = self.detach_block(id)?;
            faces.extend(was.faces);
            edges.extend(was.edges);
            nodes.extend(was.nodes);
            gone.push(step.block);
        }
        self.collect(&faces, &edges, &nodes);
        Ok(gone)
    }

    pub fn pillow_chord(
        &mut self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<(CellId, Option<On>)>, BlockingError> {
        // The chord is a tube of blocks, and what is pillowed is its
        // **side**: the four faces of each of its blocks that the chord
        // does not cross. The two at the ends are deliberately left out, so
        // the tube stays open — a node on an end face belongs to two side
        // faces and to no other, so it moves along the mitre of those two,
        // inwards across the chord and not one bit along it.
        let chord = self.chord_through(face, block)?;
        let mut patch: Vec<(CellId, CellId)> = Vec::new();
        for step in &chord {
            for side in self.faces_of(step.block) {
                if !step.through.contains(&side.face) {
                    patch.push((side.face, side.block));
                }
            }
        }
        self.pillow(&patch)
    }

    pub fn split_sheet(&mut self, edge: CellId, t: f64) -> Result<Vec<CellId>, BlockingError> {
        if !t.is_finite() || t <= 0.0 || t >= 1.0 {
            return Err(BlockingError::BadParameter(t));
        }
        let sheet = self.sheet_through(edge)?;

        // --- the new nodes, and the two halves of every edge -----------
        //
        // All three inherit what the edge was classified on, and the node
        // is the load-bearing one: an edge lying along a curve is cut into
        // two edges along that curve and a **point on it**, and the caller
        // projects it because this crate has no idea where the curve goes.
        // Both halves keep the parent's segment count, so a cut doubles the
        // nodes along this direction — which is what cutting is for.
        let mut known = self.known();
        let mut cut: HashMap<EdgeId, NodeId> = HashMap::new();
        let mut made: Vec<NodeId> = Vec::new();
        let mut gone: Vec<EdgeId> = Vec::new();
        for side in &sheet {
            let id = self
                .edge_at(side.edge)
                .ok_or(BlockingError::NoSuchEdge(side.edge))?;
            let (from, to) = (
                self.node_at(side.from)
                    .ok_or(BlockingError::NoSuchNode(side.from))?,
                self.node_at(side.to)
                    .ok_or(BlockingError::NoSuchNode(side.to))?,
            );
            let was = self.edges.get(id).expect("live").clone();
            let (p, q) = (
                self.nodes.get(from).expect("live").at,
                self.nodes.get(to).expect("live").at,
            );
            let at = [0, 1, 2].map(|k| p[k] + (q[k] - p[k]) * t);
            let born = self.nodes.handed_out();
            let middle = self.nodes.insert(Node {
                at,
                on: was.on,
                frozen: false,
                born,
            });
            // Registered as the halves of this edge **before** the parent
            // is forgotten, so that everything built below finds them
            // rather than making a third copy.
            self.edge_on(from, middle, &mut known, (was.on, was.segments));
            self.edge_on(middle, to, &mut known, (was.on, was.segments));
            cut.insert(id, middle);
            made.push(middle);
            gone.push(id);
        }

        // --- what the cut makes, read off the structure as it stands ----
        //
        // Every block and every loose face the sheet crosses, worked out
        // before any of them is touched: one block at a time would leave
        // its neighbour's shared face half cut, and nothing downstream
        // could tell that from a mistake.
        let mut halves: Vec<([[NodeId; 8]; 2], u64)> = Vec::new();
        let mut blocks: Vec<BlockId> = Vec::new();
        for (id, block) in self.blocks.iter() {
            let crossed: Vec<usize> = (0..12)
                .filter(|k| cut.contains_key(&block.edges[*k]))
                .collect();
            if crossed.is_empty() {
                continue;
            }
            // A block the sheet crosses is crossed along all four edges of
            // one axis. Anything else is a sheet that does not close, and
            // cutting it would leave a block with a face half there.
            let axis = EDGE_AXIS[crossed[0]];
            if crossed.len() != 4 || crossed.iter().any(|k| EDGE_AXIS[*k] != axis) {
                return Err(BlockingError::BrokenSheet(edge));
            }
            let frame = SPLIT_FRAME[axis as usize].map(|c| block.nodes[c]);
            // In that frame the four cut edges are the uprights, joining
            // the low cap to the high one.
            let middle: [NodeId; 4] = [0, 1, 2, 3].map(|k| {
                let pair = [frame[k], frame[k + 4]];
                let mut key = pair;
                key.sort_unstable();
                let id = known
                    .by_pair
                    .get(&key)
                    .copied()
                    .expect("the upright is an edge of the block");
                cut[&id]
            });
            halves.push((
                [
                    [
                        frame[0], frame[1], frame[2], frame[3], middle[0], middle[1], middle[2],
                        middle[3],
                    ],
                    [
                        middle[0], middle[1], middle[2], middle[3], frame[4], frame[5], frame[6],
                        frame[7],
                    ],
                ],
                // Both halves are in whatever the block was in: a cut does
                // not take anything out of a group.
                block.groups,
            ));
            blocks.push(id);
        }

        let mut sheets: Vec<Halved> = Vec::new();
        let mut loose: Vec<FaceId> = Vec::new();
        for (id, face) in self.faces.iter() {
            let crossed: Vec<usize> = (0..4)
                .filter(|k| cut.contains_key(&face.edges[*k]))
                .collect();
            if crossed.is_empty() {
                continue;
            }
            let [first, second] = crossed[..] else {
                return Err(BlockingError::BrokenSheet(edge));
            };
            if (second - first) % 2 != 0 {
                // Two sides of a quadrangle that meet at a corner: a sheet
                // crossing those is not a sheet.
                return Err(BlockingError::BrokenSheet(edge));
            }
            // The two halves, as node quads, whether this face bounds a
            // block or not: either way they inherit what the whole was
            // classified on.
            let (p, q) = (cut[&face.edges[first]], cut[&face.edges[second]]);
            let (a, b) = (first, second);
            sheets.push((
                [
                    [face.nodes[a], p, q, face.nodes[(b + 1) % 4]],
                    [p, face.nodes[(a + 1) % 4], face.nodes[b], q],
                ],
                [p, q],
                face.on,
            ));
            // A face of a block is cut by cutting the block; a face of a
            // patch has no block to cut, and cutting it in two **is** the
            // whole of the job. In two dimensions the cut is finished when
            // the faces are.
            if face.sides.iter().all(|side| side.is_none()) {
                loose.push(id);
            }
        }
        if halves.is_empty() && loose.is_empty() {
            return Err(BlockingError::BrokenSheet(edge));
        }

        // --- out with the old ------------------------------------------
        //
        // Before the new, and not after: a cap the sheet does not cross is
        // shared with the block being replaced, and a face cannot have
        // three sides. Detaching first is also what turns a cap that keeps
        // a neighbour the right way round.
        let dropped: Vec<(BlockId, [FaceId; 6])> = blocks
            .iter()
            .map(|id| (*id, self.blocks.get(*id).expect("live").faces))
            .collect();
        for (id, faces) in dropped {
            for face in faces {
                self.detach(face, id);
            }
            self.blocks.remove(id);
        }
        // The faces the sheet crossed are gone with it: what comes out of a
        // cut is two cells, not a cell and a claimant to the old name.
        let crossed: Vec<FaceId> = self
            .faces
            .iter()
            .filter(|(_, face)| face.edges.iter().any(|e| cut.contains_key(e)))
            .map(|(id, _)| id)
            .collect();
        for id in &crossed {
            self.faces.remove(*id);
        }
        for id in gone {
            self.edges.remove(id);
        }

        // --- and in with the new ---------------------------------------
        let mut known = Known {
            by_pair: known.by_pair,
            by_quad: self.known().by_quad,
        };
        known.by_pair.retain(|_, id| self.edges.get(*id).is_some());
        for (pair, groups) in halves {
            for nodes in pair {
                let block = self.emplace_block(nodes, &mut known)?;
                self.blocks.get_mut(block).expect("just made").groups = groups;
            }
        }
        // Every half of every face the cut crossed, told what the whole was
        // on — so a cut along a surface stays on that surface. The halves
        // that bound a block were made by the blocks above and are found
        // here; the ones that bound nothing are made here, a patch having
        // no block to cut.
        //
        // And **the edge the cut laid across each of them** inherits it
        // too, which is the one thing that knows a new edge runs across
        // the inside of a disc rather than round its edge. Its two ends
        // are both on the rim, so the settling asked afterwards would put
        // it on the rim: the boundary of a cell says which entities it
        // could be on and never which one it is.
        for (pair, middle, on) in &sheets {
            for nodes in pair {
                let id = self.face_on(*nodes, &mut known);
                self.faces.get_mut(id).expect("found or made").on = *on;
            }
            if on.is_none() {
                continue;
            }
            let across = self.edge_on(middle[0], middle[1], &mut known, (*on, 0));
            self.edges.get_mut(across).expect("found or made").on = *on;
        }

        self.forget();
        let mut out: Vec<CellId> = made.iter().map(|n| self.node_name(*n)).collect();
        out.sort();
        Ok(out)
    }

    pub fn extrude_patch(
        &mut self,
        face: CellId,
        height: f64,
    ) -> Result<Vec<CellId>, BlockingError> {
        // Either way. A sheet of faces has no inside, so neither side of it
        // is the wrong one: which way the quadrangles happen to be wound
        // came from the triangles of the surface they were laid on, and
        // nobody looking at the screen can tell. Only nowhere is refused.
        if !height.is_finite() || height == 0.0 {
            return Err(BlockingError::BadHeight(height));
        }
        let back = height < 0.0;
        let height = height.abs();
        let start = self.face_at(face).ok_or(BlockingError::NoSuchFace(face))?;
        if self
            .faces
            .get(start)
            .is_some_and(|f| f.sides.iter().any(|side| side.is_some()))
        {
            return Err(BlockingError::NotAPatch(face));
        }

        // All of it or none of it, and that is not a convenience: half a
        // patch raised would leave the other half welded to a block along
        // an edge, which is a sheet with a block hanging off it.
        let component = self
            .patch_components()
            .into_iter()
            .find(|faces| faces.contains(&start))
            .ok_or(BlockingError::NotAPatch(face))?;
        let sides: Vec<(FaceId, [NodeId; 4])> = component
            .iter()
            .map(|id| (*id, self.faces.get(*id).expect("live").nodes))
            .collect();
        self.raise_faces(&sides, height, back)
    }

    pub fn extrude_faces(
        &mut self,
        picked: &[(CellId, CellId)],
        height: f64,
    ) -> Result<Vec<CellId>, BlockingError> {
        if picked.is_empty() {
            return Err(BlockingError::EmptyPatch);
        }
        if !height.is_finite() || height == 0.0 {
            return Err(BlockingError::BadHeight(height));
        }
        if height < 0.0 {
            return Err(BlockingError::Inwards(height));
        }

        // Each face once, in the order asked for: picking the same face
        // twice is a selection, not two extrusions.
        let mut wanted: Vec<(CellId, CellId)> = Vec::new();
        for pair in picked {
            if !wanted.contains(pair) {
                wanted.push(*pair);
            }
        }
        let sides = wanted
            .iter()
            .map(|(face, block)| self.free_side(*face, *block))
            .collect::<Result<Vec<_>, _>>()?;

        // Three chosen faces on one edge would raise three blocks round it,
        // and which two of them meet is not a question with an answer.
        // Refused rather than welded arbitrarily — and refused **here**,
        // because what `emplace_block` would say about it is that some face
        // already has two sides, which is true and unhelpful.
        let mut along: HashMap<EdgeId, usize> = HashMap::new();
        for (id, _) in &sides {
            for edge in self.faces.get(*id).expect("live").edges {
                *along.entry(edge).or_default() += 1;
            }
        }
        if along.values().any(|holders| *holders > 2) {
            return Err(BlockingError::NotOnTheSurface(wanted[0].0));
        }

        self.raise_faces(&sides, height, false)
    }

    pub fn extrude_face(
        &mut self,
        face: CellId,
        block: CellId,
        height: f64,
    ) -> Result<CellId, BlockingError> {
        // Finite first, because NaN passes `<= 0.0`: every comparison with
        // it is false.
        if !height.is_finite() || height == 0.0 {
            return Err(BlockingError::BadHeight(height));
        }
        // And one way only, which is not the same rule as for a patch: the
        // other side of a face of a block is **the block**. Growing into it
        // would lay a second block inside the first.
        if height < 0.0 {
            return Err(BlockingError::Inwards(height));
        }
        let side = self.free_side(face, block)?;
        let raised = self.raise_faces(&[side], height, false)?;
        raised.into_iter().next().ok_or(BlockingError::LostFace)
    }

    pub fn pillow(
        &mut self,
        patch: &[(CellId, CellId)],
    ) -> Result<Vec<(CellId, Option<On>)>, BlockingError> {
        if patch.is_empty() {
            return Err(BlockingError::EmptyPatch);
        }
        // Drawing a block inwards means there is an inward: a patch of free
        // faces has no inside to give way. A name nothing answers to is a
        // different complaint, and the resolving below makes it.
        for (_, block) in patch {
            if self.block_at(*block).is_none() && self.face_at(*block).is_some() {
                return Err(BlockingError::NotABlock(*block));
            }
        }

        // --- read everything before a single node moves ----------------
        let mut picked: Vec<(FaceId, BlockId)> = Vec::new();
        let mut plans: Vec<Drawn> = Vec::new();
        for (face, block) in patch {
            // A pair, and the complaint is about the pair: a face that is
            // not this block's and a name nothing answers to are the same
            // mistake from here, which is what the map said too.
            let (id, at) = self
                .face_at(*face)
                .zip(self.block_at(*block))
                .ok_or(BlockingError::FaceNotOnBlock(*face, *block))?;
            let (_, which) = self
                .slot_of(id, at)
                .ok_or(BlockingError::FaceNotOnBlock(*face, *block))?;
            let f = self.faces.get(id).expect("live");
            let winding = if which == 1 {
                reversed(f.nodes)
            } else {
                f.nodes
            };
            let points = winding.map(|n| self.nodes.get(n).expect("live").at);
            let normal = quad_normal(points);
            let length = (0..3).map(|k| normal[k] * normal[k]).sum::<f64>().sqrt();
            if length < 1e-12 {
                return Err(BlockingError::LostFace);
            }
            // The block's own extent in that direction, from its centre to
            // the face and back again; a third of it is the layer.
            let corners = self.blocks.get(at).expect("live").nodes;
            let centre = self.middle(
                &corners
                    .iter()
                    .map(|n| self.nodes.get(*n).expect("live").at)
                    .collect::<Vec<_>>(),
            );
            let middle = self.middle(&points);
            let reach = (0..3)
                .map(|k| (middle[k] - centre[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            picked.push((id, at));
            plans.push((
                id,
                at,
                winding,
                normal.map(|x| -x / length),
                2.0 * reach * PILLOW_SHARE,
            ));
        }

        // What each node of the patch is, where it is and what it follows,
        // read before anything moves: the node that moves is **not** the
        // one that ends up where it is now. The layer leaves one there, and
        // that one is the one on the geometry.
        let mut was: HashMap<NodeId, ([f64; 3], Option<On>, bool)> = HashMap::new();
        let mut facing: HashMap<NodeId, (Vec<[f64; 3]>, f64)> = HashMap::new();
        for (_, _, winding, inward, depth) in &plans {
            for node in winding {
                let n = self.nodes.get(*node).expect("live");
                was.insert(*node, (n.at, n.on, n.frozen));
                let entry = facing.entry(*node).or_insert((Vec::new(), f64::INFINITY));
                entry.0.push(*inward);
                entry.1 = entry.1.min(*depth);
            }
        }

        // --- let go of the far side ------------------------------------
        let far = self.separate(&picked)?;

        // --- in with the patch -----------------------------------------
        // In a settled order throughout, because a hash map's order is not
        // one: two sessions pillowed the same way have to ship the same
        // bytes, and the names handed out depend on the order the nodes are
        // made in.
        let mut patch: Vec<NodeId> = facing.keys().copied().collect();
        patch.sort_unstable();
        for node in &patch {
            let (inward, depth) = &facing[node];
            let step = mitre(inward, *depth);
            let n = self.nodes.get_mut(*node).expect("live");
            let from = n.at;
            n.at = [0, 1, 2].map(|k| from[k] + step[k]);
            // And it lets go of the geometry on the way in: this node is
            // inside the material now, and what it used to follow is the
            // business of the copy the layer leaves on the surface.
            n.on = None;
            n.frozen = false;
        }

        // --- a block per face, laid into the space that opened ----------
        //
        // Its far four are the **far side's own** nodes, which did not
        // move: nothing beyond the patch moves, which is the whole point
        // of pillowing. Where there is no far side the layer makes them,
        // and they take what the node that moved in was carrying — the
        // classification and the freeze both, a frozen node having been
        // frozen where it stood and not wherever it is pushed to.
        let mut over: HashMap<NodeId, NodeId> = HashMap::new();
        for node in &patch {
            let (at, on, frozen) = was[node];
            let copy = far.get(node).copied().unwrap_or_else(|| {
                let born = self.nodes.handed_out();
                self.nodes.insert(Node {
                    at,
                    on,
                    frozen,
                    born,
                })
            });
            over.insert(*node, copy);
        }

        let mut known = self.known();
        for (_, _, winding, _, _) in &plans {
            // As for an extrusion: what is outside the face is inside the
            // block laid against it.
            let base = [winding[3], winding[0], winding[1], winding[2]];
            let corners = [
                base[0],
                base[1],
                base[2],
                base[3],
                over[&base[0]],
                over[&base[1]],
                over[&base[2]],
                over[&base[3]],
            ];
            self.emplace_block(corners, &mut known)?;
        }
        self.forget();

        // The nodes that were drawn in, each with what it was following
        // before it let go. The caller holds the triangles and may well
        // decide it is still on the same thing — a node sliding along the
        // rim of the patch is the case — which it cannot do without being
        // told what that was.
        let mut out: Vec<(CellId, Option<On>)> = patch
            .iter()
            .map(|node| (self.node_name(*node), was[node].1))
            .collect();
        out.sort_by_key(|(name, _)| *name);
        Ok(out)
    }

    pub fn sheet_segments(&self, edge: CellId) -> Result<u32, BlockingError> {
        Ok(self
            .sheet_through(edge)?
            .iter()
            .filter_map(|s| self.edge_at(s.edge))
            .filter_map(|e| self.edges.get(e))
            .map(|e| e.segments)
            .max()
            .unwrap_or(0))
    }

    pub fn set_sheet_segments(&mut self, edge: CellId, segments: u32) -> Result<(), BlockingError> {
        for side in self.sheet_through(edge)? {
            let Some(id) = self.edge_at(side.edge) else {
                continue;
            };
            self.edges.get_mut(id).expect("live").segments = segments;
        }
        Ok(())
    }

    pub fn block_faces(&self) -> Vec<BlockFace> {
        // **Grouped by what each face bounds**, block by block and then
        // patch by patch, and not face by face. A caller that reads the
        // list to find out what there is relies on that: `dedup` on the
        // block of each entry gives the blocks, and it only does so while
        // one block's faces are together.
        let mut out: Vec<BlockFace> = self
            .blocks
            .ids()
            .flat_map(|id| self.faces_of(self.block_name(id)))
            .collect();
        // A face bounding no block is still listed, against the patch it is
        // part of: the page draws a patch, and a face missing from this
        // listing is a face missing from the screen.
        for component in self.patch_components() {
            let name = self.face_name(component[0]);
            for id in component {
                let Some(face) = self.faces.get(id) else {
                    continue;
                };
                // An open sheet has no inside, so there is no "as that
                // block sees it": the face's own winding is all there is,
                // and it is handed over as it stands.
                out.push(BlockFace {
                    face: self.face_name(id),
                    block: name,
                    groups: 0,
                    nodes: face.nodes.map(|n| self.node_name(n)),
                    on: face.on,
                });
            }
        }
        out
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
        let mut out: Vec<CellId> = self.climbing(|up| {
            up.node_edges
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let edge = self.edges.get(*e)?;
                    match edge.ends {
                        [a, b] if a == id => Some(self.node_name(b)),
                        [a, b] if b == id => Some(self.node_name(a)),
                        _ => None,
                    }
                })
                .collect()
        });
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn on_the_surface(&self, node: CellId) -> bool {
        let Some(id) = self.node_at(node) else {
            return false;
        };
        self.climbing(|up| {
            up.node_faces.get(&id).into_iter().flatten().any(|f| {
                self.faces
                    .get(*f)
                    .is_some_and(|face| face.sides[1].is_none())
            })
        })
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
