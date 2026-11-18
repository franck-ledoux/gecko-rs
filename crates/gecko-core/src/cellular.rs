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

use crate::arena::{index, Arena};
use crate::attributes::On;
use crate::cells::{
    BlockFace, BlockingError, Census, ChordBlock, GroupMasks, Hex, Settling, Sheet, SheetCells,
    SheetEdge, Slab, Smoothing, Unmet,
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

impl Blocking {
    /// The name a client holds for this node, or [`CellId::NONE`] if the
    /// node is gone.
    fn node_name(&self, id: NodeId) -> CellId {
        self.nodes
            .generation(id)
            .map_or(CellId::NONE, |gen| pack(id, gen))
    }

    /// The node a name stands for, or nothing — which is the answer when the
    /// cell has been removed, whether or not its slot has been reused.
    fn node_at(&self, name: CellId) -> Option<NodeId> {
        let (id, generation) = unpack::<NodeId>(name)?;
        (self.nodes.generation(id)? == generation).then_some(id)
    }

    /// Everything the structure promises about itself, checked. Called by the
    /// tests after every operation; the invariants are listed in the design
    /// note, and invariant 2 — that `block.faces[s]` and `face.sides[i]`
    /// agree both ways round — is the one that catches a weld bug.
    pub fn validate(&self) -> Result<(), Vec<Breach>> {
        let breaches: Vec<Breach> = Vec::new();
        // The cells themselves arrive in stage 1; an empty complex breaches
        // nothing, which is the first thing worth being sure of.
        if breaches.is_empty() {
            Ok(())
        } else {
            Err(breaches)
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
        todo!("cellular: add_block")
    }

    pub fn add_slab(&mut self, quad: [[f64; 3]; 4], thickness: f64) -> Result<Slab, BlockingError> {
        todo!("cellular: add_slab")
    }

    pub fn add_face(&mut self, corners: [[f64; 3]; 4]) -> Result<CellId, BlockingError> {
        todo!("cellular: add_face")
    }

    pub fn lay_face(&mut self, corners: [[f64; 3]; 4]) -> Result<Sheet, BlockingError> {
        todo!("cellular: lay_face")
    }

    pub fn face_cells(&self, face: CellId) -> Option<([CellId; 4], [CellId; 4])> {
        todo!("cellular: face_cells")
    }

    pub fn corner_mark(&self) -> u32 {
        todo!("cellular: corner_mark")
    }

    pub fn nodes_since(&self, mark: u32) -> Vec<CellId> {
        todo!("cellular: nodes_since")
    }

    pub fn blocks(&self) -> Vec<CellId> {
        todo!("cellular: blocks")
    }

    pub fn block_groups(&self, block: CellId) -> u64 {
        todo!("cellular: block_groups")
    }

    pub fn counted_in_groups(&self) -> [usize; 64] {
        todo!("cellular: counted_in_groups")
    }

    pub fn set_in_group(
        &mut self,
        blocks: &[CellId],
        bit: u8,
        inside: bool,
    ) -> Result<(), BlockingError> {
        todo!("cellular: set_in_group")
    }

    pub fn drop_group(&mut self, bit: u8) {
        todo!("cellular: drop_group")
    }

    pub fn group_masks(&self) -> GroupMasks {
        todo!("cellular: group_masks")
    }

    pub fn beside_the_classified(&self, node: CellId) -> bool {
        todo!("cellular: beside_the_classified")
    }

    pub fn smooth(
        &mut self,
        how: Smoothing<'_>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) -> f64 {
        todo!("cellular: smooth")
    }

    pub fn patches(&self) -> Vec<CellId> {
        todo!("cellular: patches")
    }

    pub fn assign_missing_ids(&mut self) {
        todo!("cellular: assign_missing_ids")
    }

    pub fn set_position(&mut self, node: CellId, to: [f64; 3]) -> Result<(), BlockingError> {
        todo!("cellular: set_position")
    }

    pub fn node_class(&self, node: CellId) -> Option<On> {
        todo!("cellular: node_class")
    }

    pub fn set_node_class(&mut self, node: CellId, on: Option<On>) -> Result<(), BlockingError> {
        todo!("cellular: set_node_class")
    }

    pub fn is_frozen(&self, node: CellId) -> bool {
        todo!("cellular: is_frozen")
    }

    pub fn set_frozen(&mut self, node: CellId, frozen: bool) -> Result<(), BlockingError> {
        todo!("cellular: set_frozen")
    }

    pub fn edge_class(&self, edge: CellId) -> Option<On> {
        todo!("cellular: edge_class")
    }

    pub fn set_edge_class(&mut self, edge: CellId, on: Option<On>) -> Result<(), BlockingError> {
        todo!("cellular: set_edge_class")
    }

    pub fn face_class(&self, face: CellId) -> Option<On> {
        todo!("cellular: face_class")
    }

    pub fn set_face_class(&mut self, face: CellId, on: Option<On>) -> Result<(), BlockingError> {
        todo!("cellular: set_face_class")
    }

    #[allow(unreachable_code)]
    pub fn faces(&self) -> impl Iterator<Item = (CellId, [CellId; 4])> + '_ {
        let out: Vec<(CellId, [CellId; 4])> = todo!("cellular: faces");
        out.into_iter()
    }

    #[allow(unreachable_code)]
    pub fn edges(&self) -> impl Iterator<Item = (CellId, CellId, CellId, Option<On>)> + '_ {
        let out: Vec<(CellId, CellId, CellId, Option<On>)> = todo!("cellular: edges");
        out.into_iter()
    }

    #[allow(unreachable_code)]
    pub fn nodes_in_full(&self) -> impl Iterator<Item = (CellId, [f64; 3], Option<On>, bool)> + '_ {
        let out: Vec<(CellId, [f64; 3], Option<On>, bool)> = todo!("cellular: nodes_in_full");
        out.into_iter()
    }

    pub fn census(&self) -> Census {
        Census {
            nodes: self.nodes.len(),
            edges: self.edges.len(),
            faces: self.faces.len(),
            blocks: self.blocks.len(),
            patches: 0,
            ..Census::default()
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
        todo!("cellular: reclassify")
    }

    pub fn unclassify_cells(&mut self) {
        todo!("cellular: unclassify_cells")
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
        todo!("cellular: block_faces")
    }

    pub fn hexes(&self) -> Vec<Hex> {
        todo!("cellular: hexes")
    }

    pub fn node_names(&self) -> Vec<CellId> {
        todo!("cellular: node_names")
    }

    pub fn edge_names(&self) -> Vec<CellId> {
        todo!("cellular: edge_names")
    }

    pub fn face_names(&self) -> Vec<CellId> {
        todo!("cellular: face_names")
    }

    pub fn component_names(&self) -> Vec<CellId> {
        todo!("cellular: component_names")
    }

    pub fn position(&self, node: CellId) -> Option<[f64; 3]> {
        todo!("cellular: position")
    }

    pub fn edge_ends(&self, edge: CellId) -> Option<(CellId, CellId)> {
        todo!("cellular: edge_ends")
    }

    pub fn hex_corners(&self, block: CellId) -> Option<[CellId; 8]> {
        todo!("cellular: hex_corners")
    }

    pub fn neighbors(&self, node: CellId) -> Vec<CellId> {
        todo!("cellular: neighbors")
    }

    pub fn on_the_surface(&self, node: CellId) -> bool {
        todo!("cellular: on_the_surface")
    }

    pub fn is_block(&self, block: CellId) -> bool {
        todo!("cellular: is_block")
    }

    pub fn faces_of(&self, block: CellId) -> Vec<BlockFace> {
        todo!("cellular: faces_of")
    }

    #[allow(unreachable_code)]
    pub fn edges_in_full(
        &self,
    ) -> impl Iterator<Item = (CellId, [CellId; 2], u32, Option<On>)> + '_ {
        let out: Vec<(CellId, [CellId; 2], u32, Option<On>)> = todo!("cellular: edges_in_full");
        out.into_iter()
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
        todo!("cellular: nb_patches")
    }

    pub fn names_handed_out(&self) -> [u32; 4] {
        todo!("cellular: names_handed_out")
    }
}
