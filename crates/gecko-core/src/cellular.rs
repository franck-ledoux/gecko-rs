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
    BlockFace, BlockingError, Census, ChordBlock, GroupMasks, Hex, Settling, Sheet, SheetCells,
    SheetEdge, Slab, Smoothing, Unmet, HEX_EDGES, HEX_FACES,
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
        todo!("cellular: add_slab")
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
        todo!("cellular: lay_face")
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
        todo!("cellular: glue_faces")
    }

    pub fn glue_onto(&mut self, first: CellId, second: CellId) -> Result<(), BlockingError> {
        todo!("cellular: glue_onto")
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
        let Some(block) = self.block_at(block) else {
            return Vec::new();
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
