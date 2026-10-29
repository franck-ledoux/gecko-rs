//! The block structure.
//!
//! A thin, deliberate layer over [`CMap3`]: the map owns the topology, this
//! owns the naming. Nothing here reimplements what a combinatorial map already
//! does — sewing, orbits, attribute merge and split are the map's business, and
//! the one thing it cannot do for us is hand out names that survive its own
//! renumbering.

use std::collections::{BTreeMap, HashMap, HashSet};

use honeycomb_core::attributes::AttributeError;
use honeycomb_core::cmap::{
    CMap3, CMapBuilder, DartIdType, DartReleaseError, EdgeIdType, FaceIdType, LinkError,
    OrbitPolicy, SewError, VertexIdType, VolumeIdType, NULL_DART_ID,
};
use honeycomb_core::stm::atomically;
use thiserror::Error;

use crate::attributes::{BlockTag, EdgeTag, FaceTag, NodeTag, On};
use crate::ids::{CellId, CornerKey, Counter};

/// The six faces of a hexahedron, each listed anticlockwise **seen from
/// outside**, over corners in the usual VTK/Gmsh order.
///
/// Outward orientation is not cosmetic here: it is what makes two faces of the
/// same block traverse their shared edge in opposite directions, which is
/// exactly the condition β₂ needs to sew them.
pub const HEX_FACES: [[usize; 4]; 6] = [
    [0, 3, 2, 1],
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
];

/// Darts per block: four per face, six faces.
const DARTS_PER_BLOCK: usize = 24;

/// The eight corners of an axis-aligned cube, in the order [`HEX_FACES`] reads
/// them.
pub fn cube_corners(origin: [f64; 3], size: f64) -> [[f64; 3]; 8] {
    let [x, y, z] = origin;
    let s = size;
    [
        [x, y, z],
        [x + s, y, z],
        [x + s, y + s, z],
        [x, y + s, z],
        [x, y, z + s],
        [x + s, y, z + s],
        [x + s, y + s, z + s],
        [x, y + s, z + s],
    ]
}

/// The dart a corner of a freshly built block starts at.
///
/// [`Blocking::build_block`] lays the darts out face by face, four to a
/// face, in [`HEX_FACES`] order — so a corner is found by looking for it
/// in that table. Any of the three faces it is on will do: they all begin
/// a dart there.
fn corner_dart(base: DartIdType, corner: usize) -> DartIdType {
    HEX_FACES
        .iter()
        .enumerate()
        .find_map(|(face, corners)| {
            corners
                .iter()
                .position(|c| *c == corner)
                .map(|slot| base + (face * 4 + slot) as DartIdType)
        })
        .expect("every corner of a hexahedron is on some face")
}

/// Two points close enough to be the same node.
fn same_point(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
}

/// How thick a pillow layer is, as a share of the cell it is cut from.
pub const PILLOW_SHARE: f64 = 1.0 / 3.0;

/// Where a node of a pillowed patch goes.
///
/// A mitre, as in a picture frame. The copy has to end up `depth` clear of
/// **every** selected face it sits on, and neither of the obvious answers
/// does that: summing the face normals sends a node in the middle of a flat
/// patch four times too far, and averaging them leaves a node on a corner
/// short of both its faces.
///
/// So: go along the average direction, and stretch by however much that
/// direction is tilted away from the worst of the faces. On a flat patch
/// that factor is one; on a right-angled corner it is √2, which puts the
/// copy `depth` in along each face, where it belongs.
///
/// `inward` are unit normals pointing into the material.
pub fn mitre(inward: &[[f64; 3]], depth: f64) -> [f64; 3] {
    let mut sum = [0.0; 3];
    for n in inward {
        for k in 0..3 {
            sum[k] += n[k];
        }
    }
    let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
    if length < 1e-12 {
        return [0.0; 3];
    }
    let mean = [sum[0] / length, sum[1] / length, sum[2] / length];
    let tilt = inward
        .iter()
        .map(|n| (0..3).map(|k| mean[k] * n[k]).sum::<f64>())
        .fold(f64::INFINITY, f64::min);
    // Faces folded back on each other have no inside to move into.
    if tilt < 1e-6 {
        return [0.0; 3];
    }
    mean.map(|x| x * depth / tilt)
}

/// The outward normal of a quadrangle, by the shoelace formula in 3D, and
/// its center. Not normalised: the caller decides what to do with a face of
/// no area.
pub fn quad_normal(p: [[f64; 3]; 4]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for k in 0..4 {
        let (a, b) = (p[k], p[(k + 1) % 4]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// What there is, counted once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Census {
    pub nodes: usize,
    pub edges: usize,
    pub faces: usize,
    pub blocks: usize,
    /// Open sheets of faces, each one a connected component. A patch is on
    /// its way to being blocks — it is extruded — and until then it is not
    /// counted as one.
    pub patches: usize,
    pub classified_nodes: usize,
    pub classified_edges: usize,
    pub classified_faces: usize,
    pub frozen: usize,
}

/// Which groups each cell of a blocking is in, by the blocks that bound
/// it. Empty where nothing is grouped, which is the usual case.
#[derive(Clone, Debug, Default)]
pub struct GroupMasks {
    pub nodes: HashMap<VertexIdType, u64>,
    pub edges: HashMap<EdgeIdType, u64>,
    /// Cells that bound a block in **no** group, as well as whatever else
    /// they bound.
    ///
    /// The mask alone cannot say this, and that is the whole reason this
    /// is here: an edge between a block of the fluid and a block of
    /// nothing carries the fluid's bit, so hiding the fluid would take it
    /// away — and leave the block that is still on screen with a hole
    /// where its edge used to be. A cell is drawn when **any** block it
    /// bounds is drawn, and this is the half of that sentence a mask
    /// cannot hold.
    ///
    /// Only filled when something is grouped: where nothing is, every
    /// cell's mask is zero and that says it already.
    pub loose_nodes: HashSet<VertexIdType>,
    pub loose_edges: HashSet<EdgeIdType>,
}

/// A cell asking where it belongs, and everything there is to answer with.
///
/// The last two fields are what makes the difference between a rule that
/// works on a box and one that works on a cylinder. A boundary all on one
/// circle leaves a cell free to be the circle, the disc it bounds, or the
/// tube it bounds — so what the cell already claims is believed when it is
/// still possible, and otherwise where the cell *is* decides.
pub struct Settling<'a> {
    /// What each cell of the boundary is classified on.
    pub boundary: &'a [On],
    /// The cell's own dimension. It may not be classified below this.
    pub at_least: u8,
    /// What the cell is classified on as things stand.
    pub already: Option<On>,
    /// The middle of the cell, for deciding between candidates that the
    /// boundary cannot tell apart.
    pub middle: [f64; 3],
}

/// One edge of a sheet, with the direction the cut parameter runs in.
///
/// `from`/`to` are what makes a sheet cuttable at a single parameter: the
/// same `t` has to mean the same side of the sheet on every edge of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetEdge {
    pub edge: CellId,
    pub from: VertexIdType,
    pub to: VertexIdType,
}

/// What a smoothing pass is asked to do.
#[derive(Clone, Copy, Debug)]
pub struct Smoothing<'a> {
    /// How many passes. Each one moves every node once.
    pub rounds: u32,
    /// How far along the way to the middle of its neighbors a node goes,
    /// from 0 (nowhere) to 1 (all the way).
    pub share: f64,
    /// Nodes that must not move whatever their classification allows —
    /// the one somebody has just put somewhere, above all.
    pub held: &'a [CellId],
}

/// For each corner of a hexahedron, the three corners an edge joins it to,
/// in the order [`HEX_FACES`] implies.
pub const HEX_CORNER_EDGES: [[usize; 3]; 8] = [
    [1, 3, 4],
    [2, 0, 5],
    [3, 1, 6],
    [0, 2, 7],
    [7, 5, 0],
    [4, 6, 1],
    [5, 7, 2],
    [6, 4, 3],
];

/// How square a corner is: the volume the three edges leaving it span, over
/// the product of their lengths. One at a right angle, nought where they
/// fall into a plane, negative where the cell is inside out.
pub fn scaled_jacobian(at: [f64; 3], sides: [[f64; 3]; 3]) -> f64 {
    let e = sides.map(|p| [0, 1, 2].map(|k| p[k] - at[k]));
    let len = e.map(|v| (0..3).map(|k| v[k] * v[k]).sum::<f64>().sqrt());
    if len.iter().any(|l| *l < 1e-12) {
        return 0.0;
    }
    let cross = [
        e[1][1] * e[2][2] - e[1][2] * e[2][1],
        e[1][2] * e[2][0] - e[1][0] * e[2][2],
        e[1][0] * e[2][1] - e[1][1] * e[2][0],
    ];
    let volume: f64 = (0..3).map(|k| e[0][k] * cross[k]).sum();
    volume / (len[0] * len[1] * len[2])
}

/// The same for a corner of a quadrangle: the sine of the angle between the
/// two edges leaving it, which is one at a right angle and nought where the
/// corner has folded flat.
pub fn flatness(at: [f64; 3], before: [f64; 3], after: [f64; 3]) -> f64 {
    let (u, v) = (
        [0, 1, 2].map(|k| before[k] - at[k]),
        [0, 1, 2].map(|k| after[k] - at[k]),
    );
    let (nu, nv) = (
        (0..3).map(|k| u[k] * u[k]).sum::<f64>().sqrt(),
        (0..3).map(|k| v[k] * v[k]).sum::<f64>().sqrt(),
    );
    if nu < 1e-12 || nv < 1e-12 {
        return 0.0;
    }
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    (0..3).map(|k| cross[k] * cross[k]).sum::<f64>().sqrt() / (nu * nv)
}

/// One block of a chord, and the two faces the chord crosses it by.
///
/// The pair is unordered: walking a chord the other way round swaps them,
/// and nothing that uses a chord cares which end it came in at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordBlock {
    pub block: CellId,
    pub through: [CellId; 2],
}

/// Across a quadrangle: the edge opposite `from`→`to`, oriented to match.
///
/// This is the whole orientation question, in one place and with no map in
/// sight. In a quadrangle A→B→C→D the edge opposite AB is CD — but the
/// corners that correspond are A↔D and B↔C, so AB traversed A→B matches CD
/// traversed **D→C**. The opposite edge runs the other way round the cycle.
///
/// Getting it wrong does not fail: it cuts alternate cells at `t` and
/// `1 - t`, and the sheet comes out as a concertina. Which is why it is a
/// function of four numbers, tested on its own.
pub fn across(
    quad: [VertexIdType; 4],
    from: VertexIdType,
    to: VertexIdType,
) -> Option<(VertexIdType, VertexIdType)> {
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        if (a, b) == (from, to) {
            return Some((quad[(i + 3) % 4], quad[(i + 2) % 4]));
        }
        if (a, b) == (to, from) {
            return Some((quad[(i + 2) % 4], quad[(i + 3) % 4]));
        }
    }
    None
}

/// One face of one block: the four nodes it runs through, wound outward **as
/// that block sees it**, with both names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockFace {
    pub face: CellId,
    pub block: CellId,
    /// The block's volume in the map, for the same reason `on` is here:
    /// everything that draws a surface walks this list, and finding a
    /// block by name is a scan of every volume — so asking per entry
    /// turns a walk of the model into a walk of the model squared. It
    /// cost three seconds at a hundred and twenty-five blocks.
    pub volume: VolumeIdType,
    pub nodes: [VertexIdType; 4],
    /// What the face is classified on. Carried here rather than looked up
    /// by name: everything that draws a surface walks this list, and a
    /// lookup per entry is a scan of the whole map per entry.
    pub on: Option<On>,
}

#[derive(Debug, Error)]
pub enum BlockingError {
    #[error("could not build the map: {0}")]
    Build(String),
    #[error("could not link darts: {0}")]
    Link(#[from] LinkError),
    #[error("attribute error: {0}")]
    Attribute(#[from] AttributeError),
    #[error("no node is named {0:?}")]
    NoSuchNode(CellId),
    #[error("no face is named {0:?}")]
    NoSuchFace(CellId),
    #[error("a face cannot be glued to itself")]
    SameFace,
    #[error("face {0:?} is already glued to another block")]
    AlreadyGlued(CellId),
    #[error("could not glue: {0}")]
    Sew(#[from] SewError),
    #[error("no block is named {0:?}")]
    NoSuchBlock(CellId),
    #[error("could not free dart {0}")]
    Release(DartIdType),
    #[error("no edge is named {0:?}")]
    NoSuchEdge(CellId),
    #[error("the sheet through edge {0:?} meets itself the wrong way round")]
    TangledSheet(CellId),
    #[error("a cut has to fall strictly inside the edge, not at {0}")]
    BadParameter(f64),
    #[error("the sheet through edge {0:?} does not close into a surface")]
    BrokenSheet(CellId),
    #[error("the chord through face {0:?} comes back through a block it has already crossed")]
    TangledChord(CellId),
    #[error("{0:?} is a patch — an open sheet of faces — and this needs a block")]
    NotABlock(CellId),
    #[error("face {0:?} belongs to a block — take the block away, not one of its sides")]
    NotAPatch(CellId),
    #[error(
        "a face of a block grows away from it, and {0} goes the other way — \
         into the block. A face of a patch grows either way."
    )]
    Inwards(f64),
    #[error("nothing was selected to pillow")]
    EmptyPatch,
    #[error("face {0:?} is not a face of block {1:?}")]
    FaceNotOnBlock(CellId, CellId),
    #[error("face {0:?} is inside the model, with a block on each side")]
    NotOnTheSurface(CellId),
    #[error("could not find the face a new layer should be glued along")]
    LostFace,
    #[error("an extrusion has to go somewhere: {0} is not a positive distance")]
    BadHeight(f64),
    #[error("node {0:?} is frozen; thaw it before moving it")]
    Frozen(CellId),
    #[error("nodes {0:?} and {1:?} are both frozen, and not in the same place")]
    FrozenApart(CellId, CellId),
}

impl From<DartReleaseError> for BlockingError {
    fn from(e: DartReleaseError) -> Self {
        BlockingError::Release(e.0)
    }
}

/// Which quarter turn lines two faces up best.
///
/// The pairing is necessarily orientation-reversing — glued faces are
/// back-to-back, so one is traversed the way the other is not — which leaves
/// exactly four candidates: `a[i]` meets `b[(m - i) mod 4]`, for `m` in 0..4.
/// The answer is the one that moves the corners least.
///
/// Pure, and separate from the gluing, because it is the only part of this
/// with a choice in it. Note what it cannot do: two blocks turned against each
/// other have no right answer here, only a least bad quarter turn.
/// A corner of a glue whose two nodes were classified on entities with
/// nothing in common — so one of the two classifications was dropped.
///
/// Not an error: gluing a node on one curve to a node on another is a
/// perfectly ordinary thing to ask for, and the answer is that the welded
/// node cannot be on both. The caller is told which one survived so it can
/// pass that on rather than leaving it to be discovered later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unmet {
    pub node: CellId,
    pub kept: On,
    pub dropped: On,
}

/// How far apart two points are.
fn apart(p: [f64; 3], q: [f64; 3]) -> f64 {
    (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>().sqrt()
}

pub fn best_pairing(a: &[[f64; 3]; 4], b: &[[f64; 3]; 4]) -> usize {
    let cost = |m: usize| -> f64 {
        (0..4)
            .map(|i| {
                let (p, q) = (a[i], b[(m + 4 - i) % 4]);
                (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>()
            })
            .sum()
    };
    // `total_cmp` and a stable minimum: ties go to the lowest turn, so the
    // same two faces always glue the same way.
    (0..4)
        .min_by(|&m, &n| cost(m).total_cmp(&cost(n)))
        .expect("four turns")
}

pub struct Blocking {
    map: CMap3<f64>,
    /// One counter per dimension: names are unique among cells of the same
    /// dimension, not across dimensions.
    names: [Counter; 4],
    corners: Counter,
    /// Where each name was last seen, so that finding a cell by its name
    /// is a lookup rather than a walk of the whole map.
    ///
    /// A **hint**, not a register: every answer is checked against the
    /// cell's own tag before it is given, and a miss falls back to the
    /// walk. That is what makes it safe to leave alone — a stale entry
    /// costs one scan, which is what the lookup cost before there were any
    /// hints, and never a wrong answer.
    ///
    /// Written where names are handed out, one entry per cell named, and
    /// nowhere else. Rebuilding the whole thing there instead was tried
    /// and was worse: it walks the four dimensions on every operation,
    /// which cost more on a model being built than the walks it saves.
    ///
    /// Why it had to exist: `pull_onto_class` is called once per node a
    /// cut makes, and each call began by finding its node by name. At a
    /// hundred and twenty-five blocks that was twelve milliseconds a node
    /// and half a second a cut.
    seen: Seen,
}

/// Where each name was last seen. See [`Blocking::seen`].
#[derive(Clone, Debug, Default)]
struct Seen {
    nodes: HashMap<CellId, VertexIdType>,
    edges: HashMap<CellId, EdgeIdType>,
    faces: HashMap<CellId, FaceIdType>,
    blocks: HashMap<CellId, VolumeIdType>,
}

impl Blocking {
    pub fn new() -> Result<Self, BlockingError> {
        let map = CMapBuilder::<3>::from_n_darts(0)
            .add_attribute::<NodeTag>()
            .add_attribute::<EdgeTag>()
            .add_attribute::<FaceTag>()
            .add_attribute::<BlockTag>()
            .build()
            .map_err(|e| BlockingError::Build(e.to_string()))?;
        Ok(Blocking {
            map,
            names: Default::default(),
            corners: Counter::default(),
            seen: Seen::default(),
        })
    }

    /// A copy of the whole structure, to put back later.
    ///
    /// Written out by hand because [`CMap3`] is not `Clone`, and it is worth
    /// saying what that costs and what it does not. The map is four things —
    /// the β functions, the vertex positions, the attribute storages, and the
    /// record of which darts are in use — and all four can be read and written
    /// from outside, so this is a copy and not a reconstruction: nothing is
    /// re-sewn, nothing is re-derived, no operation is replayed.
    ///
    /// That is what makes it usable as an undo state. Every dart keeps its own
    /// number, so every cell keeps the identifier the map derives from its
    /// orbit; the name counters come across as they stand, so a [`CellId`] a
    /// caller is still holding means the cell it meant; and the used-and-unused
    /// pattern is reproduced position by position, so the next dart the map
    /// hands out is the one it would have handed out anyway. A state put back
    /// is indistinguishable from the state that was taken, rather than merely
    /// the same shape.
    ///
    /// Attributes of cells that no longer exist are the one thing left behind.
    /// No orbit names them and nothing reads them.
    pub fn snapshot(&self) -> Result<Blocking, BlockingError> {
        let mut copy = Blocking::new()?;
        let total = self.map.n_darts() as DartIdType;

        // Darts in runs of one kind, so each one lands on its own number.
        // Dart 0 is the null dart: it belongs to the map, and a fresh one
        // has it already.
        let mut d: DartIdType = 1;
        while d < total {
            let unused = self.map.is_unused(d);
            let mut run = 1;
            while d + run < total && self.map.is_unused(d + run) == unused {
                run += 1;
            }
            if unused {
                copy.map.allocate_unused_darts(run as usize);
            } else {
                copy.map.allocate_used_darts(run as usize);
            }
            d += run;
        }

        // The topology before anything that hangs off it: a cell is named by
        // the smallest dart of its orbit, and until β is in place there are
        // no orbits to name.
        //
        // Read in one transaction, because a single `beta` is a transaction
        // of its own and there are four of them per dart.
        let betas: Vec<[DartIdType; 4]> = atomically(|t| {
            let mut out = Vec::with_capacity(total as usize);
            for d in 1..total {
                out.push([
                    self.map.beta_tx::<0>(t, d)?,
                    self.map.beta_tx::<1>(t, d)?,
                    self.map.beta_tx::<2>(t, d)?,
                    self.map.beta_tx::<3>(t, d)?,
                ]);
            }
            Ok(out)
        });
        for (d, beta) in (1..total).zip(betas) {
            copy.map.set_betas(d, beta);
        }

        for v in self.map_vertices() {
            if let Some(at) = self.map.read_vertex(v) {
                copy.map.write_vertex(v, at);
            }
            copy.map.write_attribute(v, self.node_tag(v));
        }
        for e in self.map_edges() {
            copy.map.write_attribute(e, self.edge_tag(e));
        }
        for f in self.map_faces() {
            copy.map.write_attribute(f, self.face_tag(f));
        }
        for b in self.map_volumes() {
            copy.map.write_attribute(b, self.block_tag(b));
        }

        copy.names = self.names.clone();
        copy.corners = self.corners.clone();
        // The darts land on the same numbers as the original's, so where
        // each name was seen is still where it is.
        copy.seen = self.seen.clone();
        Ok(copy)
    }

    /// Add one hexahedral block from its eight corner points, in the usual
    /// VTK/Gmsh order.
    ///
    /// The block is built free-standing: every β₃ is left unset, so it is a
    /// closed volume bounded by the model's exterior until something is sewn to
    /// it. Joining blocks is a separate operation, and a later step.
    pub fn add_block(&mut self, corners: [[f64; 3]; 8]) -> Result<CellId, BlockingError> {
        Ok(self.build_block(corners)?.0)
    }

    /// A single quadrangular face, standing on its own.
    ///
    /// Not a block: four darts, a β₁ cycle, and β₂ left **null** all the way
    /// round, which is what an open sheet of faces is made of. β₃ is never
    /// touched — in a 3-map β₂ joins two faces *of one volume* and β₃ joins
    /// two volumes, so faces welded edge to edge are welded by β₂ and are
    /// therefore, in the map's eyes, one volume.
    ///
    /// That is not a trick played on the map; it is what the map already
    /// says. A connected component of faces **is** a volume orbit, which is
    /// why it has a name of its own without anybody keeping a list, and why
    /// [`is_block`][Self::is_block] can tell the two apart by asking whether
    /// the shell closes.
    ///
    /// The corners are taken anticlockwise seen from the side the face
    /// faces; [`quad_normal`] is what that means in arithmetic.
    pub fn add_face(&mut self, corners: [[f64; 3]; 4]) -> Result<CellId, BlockingError> {
        let base = self.map.allocate_used_darts(4);
        for slot in 0..4 {
            self.map.link::<1>(
                base + slot as DartIdType,
                base + ((slot + 1) % 4) as DartIdType,
            )?;
        }
        for (slot, point) in corners.iter().enumerate() {
            let vertex = self.map.vertex_id(base + slot as DartIdType);
            self.map
                .write_vertex(vertex, (point[0], point[1], point[2]));
            self.map.write_attribute(
                vertex,
                NodeTag {
                    id: CellId::NONE,
                    corner_key: self.corners.next_corner(),
                    on: None,
                    frozen: false,
                },
            );
        }
        self.assign_missing_ids();
        Ok(self.face_tag(self.map.face_id(base)).id)
    }

    /// A face's four corners and four edges, in the order β₁ walks them.
    ///
    /// Edge `k` joins corner `k` to corner `k + 1`, which is what lets a
    /// caller that built the face from a list of points say what each of
    /// its sides is on without going looking for one by where it is.
    pub fn face_cells(&self, face: CellId) -> Option<([CellId; 4], [CellId; 4])> {
        let f = self.face_by_id(face)?;
        let darts = self.face_darts(f);
        Some((
            darts.map(|d| self.node_tag(self.map.vertex_id(d)).id),
            darts.map(|d| self.edge_tag(self.map.edge_id(d)).id),
        ))
    }

    /// A mark for "every node that exists now".
    ///
    /// Read before a command and handed back to [`nodes_since`] after it,
    /// which is how the session tells a node an operation **made** from one
    /// it merely renamed. The name is no good for that: taking a block out
    /// splits the vertex orbits around it, and every copy comes out
    /// unnamed and is given a fresh name afterwards, though nothing new
    /// was created. The corner key is the identity that survives exactly
    /// that, which is what it was written for.
    ///
    /// [`nodes_since`]: Self::nodes_since
    pub fn corner_mark(&self) -> u32 {
        self.corners.handed_out()
    }

    /// The nodes whose corner was made at or after `mark`.
    pub fn nodes_since(&self, mark: u32) -> Vec<CellId> {
        let mut out: Vec<CellId> = self
            .map_vertices()
            .into_iter()
            .filter(|v| {
                self.node_tag(*v)
                    .corner_key
                    .get()
                    .is_some_and(|key| key >= mark)
            })
            .map(|v| self.node_tag(v).id)
            .filter(|id| id.is_some())
            .collect();
        // By name, so that two hosts walking the same model reach the same
        // answers in the same order.
        out.sort();
        out
    }

    /// Every block of the structure, by name. Patches are left out: an
    /// open sheet of faces is not a block and cannot be in a group.
    pub fn blocks(&self) -> Vec<CellId> {
        let mut out: Vec<CellId> = self
            .map_volumes()
            .into_iter()
            .filter(|v| self.is_block(*v))
            .map(|v| self.block_tag(v).id)
            .filter(|id| id.is_some())
            .collect();
        out.sort();
        out
    }

    /// Which groups a block is in.
    pub fn block_groups(&self, block: CellId) -> u64 {
        self.block_by_id(block)
            .map(|v| self.block_tag(v).groups)
            .unwrap_or(0)
    }

    /// How many blocks are in each group, by bit: one walk of the volumes
    /// rather than one per group or one per block.
    pub fn counted_in_groups(&self) -> [usize; 64] {
        let mut out = [0usize; 64];
        for volume in self.map_volumes() {
            if !self.is_block(volume) {
                continue;
            }
            let groups = self.block_tag(volume).groups;
            for (bit, count) in out.iter_mut().enumerate() {
                if groups & (1u64 << bit) != 0 {
                    *count += 1;
                }
            }
        }
        out
    }

    /// Put blocks into a group, or take them out of it.
    ///
    /// By bit rather than by name: what a group is **called** is no
    /// business of a block structure, which holds the model and not the
    /// words for it.
    pub fn set_in_group(
        &mut self,
        blocks: &[CellId],
        bit: u8,
        inside: bool,
    ) -> Result<(), BlockingError> {
        let mask = 1u64 << bit;
        let found: Vec<VolumeIdType> = blocks
            .iter()
            .map(|b| {
                self.block_by_id(*b)
                    .filter(|v| self.is_block(*v))
                    .ok_or(BlockingError::NoSuchBlock(*b))
            })
            .collect::<Result<_, _>>()?;
        for volume in found {
            let tag = self.block_tag(volume);
            let groups = if inside {
                tag.groups | mask
            } else {
                tag.groups & !mask
            };
            self.map.write_attribute(volume, BlockTag { groups, ..tag });
        }
        Ok(())
    }

    /// Take one group away from every block that is in it.
    pub fn drop_group(&mut self, bit: u8) {
        let mask = !(1u64 << bit);
        let blocks: Vec<VolumeIdType> = self.map_volumes();
        for volume in blocks {
            let tag = self.block_tag(volume);
            if tag.groups & !mask != 0 {
                self.map.write_attribute(
                    volume,
                    BlockTag {
                        groups: tag.groups & mask,
                        ..tag
                    },
                );
            }
        }
    }

    /// Which groups every cell of the blocking is in.
    ///
    /// A face, an edge or a node is in a group when a block it bounds is
    /// in it — the transitivity, computed here rather than marked on each
    /// cell, so that there is one place to write and none to forget. The
    /// walk skips blocks that are in no group at all, which is every block
    /// of a model where nobody has made one: the whole pass then costs a
    /// look at each volume's tag.
    pub fn group_masks(&self) -> GroupMasks {
        let mut out = GroupMasks::default();
        let mut grouped = false;
        for volume in self.map_volumes() {
            let groups = self.block_tag(volume).groups;
            if groups == 0 {
                continue;
            }
            grouped = true;
            for dart in self.map.orbit(OrbitPolicy::Volume, volume as DartIdType) {
                *out.nodes.entry(self.map.vertex_id(dart)).or_default() |= groups;
                *out.edges.entry(self.map.edge_id(dart)).or_default() |= groups;
            }
        }
        // A second pass, and only when there is something to tell apart:
        // which cells touch a block that is in no group. Without it, an
        // edge between a grouped block and an ungrouped one would go when
        // the group did, taking a side off the block left behind.
        if grouped {
            for volume in self.map_volumes() {
                if self.block_tag(volume).groups != 0 {
                    continue;
                }
                for dart in self.map.orbit(OrbitPolicy::Volume, volume as DartIdType) {
                    out.loose_nodes.insert(self.map.vertex_id(dart));
                    out.loose_edges.insert(self.map.edge_id(dart));
                }
            }
        }
        out
    }

    /// Is this node joined by an edge to one that is on the geometry?
    ///
    /// What tells a node made **into** a classified model from one that
    /// arrived with a block of its own.
    pub fn beside_the_classified(&self, node: CellId) -> bool {
        let Some(vertex) = self.node_by_id(node) else {
            return false;
        };
        self.neighbors(vertex)
            .into_iter()
            .any(|other| self.node_tag(other).on.is_some())
    }

    /// Move the nodes towards the middle of their neighbors, as far as
    /// what they are classified on allows.
    ///
    /// A **smart Laplacian**, and both halves of that name earn their keep:
    ///
    /// * each node goes a **share** of the way to the average of its
    ///   one-ring, rather than all of it. All of it is what makes a
    ///   Laplacian oscillate between the two sides of a crease.
    /// * a move is **taken only if it does not make the worst corner of the
    ///   cells round that node any worse**. Laplacian smoothing on a
    ///   concave boundary happily turns cells inside out; this is what
    ///   stops it, and it is why the operation can be handed to somebody
    ///   who has not studied it.
    ///
    /// Who may move at all, and how far, is the **classification**:
    ///
    /// | the node | what it does |
    /// | --- | --- |
    /// | frozen | stays |
    /// | on a geometric corner | stays, its entity being a point |
    /// | on a curve, or a side | moves, then is projected onto it |
    /// | on a surface | moves, then is projected onto it |
    /// | on the blocking's boundary, classified on nothing | stays |
    /// | inside, classified on nothing | moves freely |
    ///
    /// The last two are the pair that matters. An interior node has blocks
    /// all round it and nothing to fall off; a boundary node with no
    /// classification has no surface to be kept on, and moving it would
    /// pull the blocking away from the model it is meant to follow.
    ///
    /// Nodes are visited **by name**, and each move is applied before the
    /// next is worked out — Gauss-Seidel, which settles faster than
    /// averaging everything first and, because the order is the naming
    /// order, gives the same answer in both façades.
    ///
    /// Returns the furthest any one node moved, which is what tells a
    /// caller whether another round is worth it.
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

        // By name, so the order does not depend on how the map happened to
        // lay its darts out.
        let mut movable: Vec<(CellId, VertexIdType)> = self
            .map_vertices()
            .into_iter()
            .map(|v| (self.node_tag(v).id, v))
            .filter(|(name, v)| {
                let tag = self.node_tag(*v);
                !tag.frozen
                    && !held.contains(name)
                    // A point is a point: a node on a geometric corner has
                    // one place to be.
                    && tag.on.is_none_or(|on| on.dim > 0)
                    // And the pair above: free inside, held on the boundary.
                    && (tag.on.is_some() || !self.on_the_surface(*v))
            })
            .collect();
        movable.sort_by_key(|(name, _)| *name);

        let mut furthest = 0.0f64;
        for _ in 0..how.rounds {
            for &(_, v) in &movable {
                let ring = self.neighbors(v);
                if ring.is_empty() {
                    continue;
                }
                let from = self.position(v).unwrap_or([0.0; 3]);
                let mut middle = [0.0; 3];
                for other in &ring {
                    let p = self.position(*other).unwrap_or([0.0; 3]);
                    for k in 0..3 {
                        middle[k] += p[k] / ring.len() as f64;
                    }
                }
                let mut to = [0, 1, 2].map(|k| from[k] + (middle[k] - from[k]) * share);
                // Back onto whatever it follows, before anybody judges it:
                // the point to weigh is the one the node would end up at.
                if let Some(on) = self.node_tag(v).on {
                    if let Some(onto) = project(on, to) {
                        to = onto;
                    }
                }
                let step = apart(from, to);
                if step == 0.0 {
                    continue;
                }

                let before = self.worst_corner_at(v);
                self.map.write_vertex(v, (to[0], to[1], to[2]));
                if self.worst_corner_at(v) < before {
                    // Not an improvement. A Laplacian on a concave
                    // boundary would have taken it and turned a cell
                    // inside out.
                    self.map.write_vertex(v, (from[0], from[1], from[2]));
                    continue;
                }
                furthest = furthest.max(step);
            }
        }
        furthest
    }

    /// The worst corner of everything this node is a corner of.
    ///
    /// For a block, the **scaled Jacobian** at each of its eight corners:
    /// the volume of the three edges leaving a corner over the product of
    /// their lengths, which is 1 at a right-angled corner, 0 where three
    /// edges fall into a plane, and negative where the cell is inside out.
    /// For a face of a patch, the same thing a dimension down.
    ///
    /// All eight corners of each block, not just this node's: moving one
    /// corner changes the shape at every other.
    fn worst_corner_at(&self, node: VertexIdType) -> f64 {
        let mut worst = f64::INFINITY;
        let mut blocks: Vec<VolumeIdType> = self
            .map
            .orbit(OrbitPolicy::Vertex, node as DartIdType)
            .map(|d| self.map.volume_id(d))
            .collect();
        blocks.sort_unstable();
        blocks.dedup();
        for volume in blocks {
            if let Some(corners) = self.hex_corners(volume) {
                let at = corners.map(|c| self.position(c).unwrap_or([0.0; 3]));
                for (corner, sides) in HEX_CORNER_EDGES.iter().enumerate() {
                    worst = worst.min(scaled_jacobian(at[corner], sides.map(|s| at[s])));
                }
            }
        }
        // The faces of a patch, which can fold without any block being
        // involved at all.
        let mut faces: Vec<FaceIdType> = self
            .map
            .orbit(OrbitPolicy::Vertex, node as DartIdType)
            .map(|d| self.map.face_id(d))
            .collect();
        faces.sort_unstable();
        faces.dedup();
        for face in faces {
            if self.is_block(self.map.volume_id(face as DartIdType)) {
                continue;
            }
            let corners = self.face_corners(face);
            let at = corners.map(|c| self.position(c).unwrap_or([0.0; 3]));
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

    /// The eight corners of a block, in [`HEX_FACES`] order.
    ///
    /// The walk the mesher does to read a hexahedron, which smoothing needs
    /// as well: it has to look at what a move would do to the blocks round
    /// a node, and it cannot ask for every block in the model to find four
    /// of them.
    ///
    /// Nothing for a patch: the walk crosses β₂ to find the corner above
    /// one of its own, and an open sheet has none.
    pub fn hex_corners(&self, volume: VolumeIdType) -> Option<[VertexIdType; 8]> {
        if !self.is_block(volume) {
            return None;
        }
        let mut ring = [volume as DartIdType; 4];
        for k in 1..4 {
            ring[k] = self.map.beta::<1>(ring[k - 1]);
        }
        let up = ring.map(|d| self.map.beta::<1>(self.map.beta::<2>(d)));
        let at = |d| self.map.vertex_id(d);
        let end = |d| self.map.vertex_id(self.map.beta::<1>(d));
        Some([
            at(ring[0]),
            at(ring[3]),
            at(ring[2]),
            at(ring[1]),
            end(up[0]),
            end(up[3]),
            end(up[2]),
            end(up[1]),
        ])
    }

    /// The nodes an edge of the blocking joins to this one — its one-ring.
    ///
    /// What a Laplacian averages. Read off the vertex's own orbit, so a
    /// node shared by several blocks has each of its neighbors once
    /// however many blocks it is shared by.
    pub fn neighbors(&self, node: VertexIdType) -> Vec<VertexIdType> {
        let mut out: Vec<VertexIdType> = self
            .map
            .orbit(OrbitPolicy::Vertex, node as DartIdType)
            .map(|d| self.map.vertex_id(self.map.beta::<1>(d)))
            .collect();
        out.sort_unstable();
        out.dedup();
        out.retain(|v| *v != node);
        out
    }

    /// Whether this node is on the **blocking's** own boundary: some face
    /// at it has nothing glued behind.
    ///
    /// Read off the map, not off the geometry. It is what tells a node that
    /// may be smoothed freely from one that may not: an interior node has
    /// blocks all round it and nowhere to go wrong, while one on the
    /// boundary and classified on nothing has no surface to be kept on, so
    /// moving it would pull the blocking off the model.
    ///
    /// Every node of a patch is one, which is the right answer: an open
    /// sheet of faces is all boundary.
    pub fn on_the_surface(&self, node: VertexIdType) -> bool {
        self.map
            .orbit(OrbitPolicy::Vertex, node as DartIdType)
            .any(|d| self.map.beta::<3>(d) == NULL_DART_ID)
    }

    /// Whether this volume is a **block** — a shell that closes — rather
    /// than a **patch**: an open sheet of faces.
    ///
    /// Asked of the topology rather than kept in a flag, because a flag can
    /// come to disagree with the structure it describes and this cannot. A
    /// block has a β₂ partner for every one of its darts; an open sheet has
    /// a boundary, and a boundary is exactly where β₂ is null.
    ///
    /// Six faces welded into a closed box therefore *are* a block, which is
    /// the right answer rather than a loophole.
    pub fn is_block(&self, volume: VolumeIdType) -> bool {
        self.map
            .orbit(OrbitPolicy::Volume, volume as DartIdType)
            .all(|d| self.map.beta::<2>(d) != NULL_DART_ID)
    }

    /// The same, by name.
    pub fn named_is_block(&self, block: CellId) -> bool {
        self.block_by_id(block).is_some_and(|v| self.is_block(v))
    }

    /// Every patch: the connected components of faces that are not blocks.
    pub fn patches(&self) -> Vec<CellId> {
        self.map_volumes()
            .into_iter()
            .filter(|v| !self.is_block(*v))
            .map(|v| self.block_tag(v).id)
            .collect()
    }

    /// The same, and **which dart each face starts at**.
    ///
    /// The darts are allocated in one run, [`HEX_FACES`] in order, four to a
    /// face — so a caller that built the corners itself knows which of the
    /// six faces is which without going looking for one by where it is. That
    /// matters: matching a face by position is what the sheet removal got
    /// wrong, and it was wrong for the same reason it would be wrong here.
    fn build_block(
        &mut self,
        corners: [[f64; 3]; 8],
    ) -> Result<(CellId, DartIdType), BlockingError> {
        let base = self.map.allocate_used_darts(DARTS_PER_BLOCK);
        let dart = |face: usize, slot: usize| base + (face * 4 + slot) as DartIdType;

        // β₁ walks each face round its four darts.
        for face in 0..6 {
            for slot in 0..4 {
                self.map
                    .link::<1>(dart(face, slot), dart(face, (slot + 1) % 4))?;
            }
        }

        // β₂ joins the two darts that traverse one edge in opposite directions.
        // Each is visited once, by only acting on the half with the lower
        // corner first.
        let mut by_edge: HashMap<(usize, usize), DartIdType> = HashMap::new();
        for (face, corners) in HEX_FACES.iter().enumerate() {
            for slot in 0..4 {
                by_edge.insert((corners[slot], corners[(slot + 1) % 4]), dart(face, slot));
            }
        }
        for (face, corners) in HEX_FACES.iter().enumerate() {
            for slot in 0..4 {
                let (from, to) = (corners[slot], corners[(slot + 1) % 4]);
                if from < to {
                    self.map.link::<2>(dart(face, slot), by_edge[&(to, from)])?;
                }
            }
        }

        // Positions and corner keys, once the orbits they belong to exist.
        for (corner, point) in corners.iter().enumerate() {
            let d = HEX_FACES
                .iter()
                .enumerate()
                .find_map(|(f, corners)| {
                    corners
                        .iter()
                        .position(|c| *c == corner)
                        .map(|s| dart(f, s))
                })
                .expect("every corner of a hexahedron is on some face");
            let vertex = self.map.vertex_id(d);
            self.map
                .write_vertex(vertex, (point[0], point[1], point[2]));
            self.map.write_attribute(
                vertex,
                NodeTag {
                    id: CellId::NONE,
                    corner_key: self.corners.next_corner(),
                    on: None,
                    frozen: false,
                },
            );
        }

        // Only the new block can be unnamed: nothing else was touched.
        self.name_cells_near(OrbitPolicy::Volume, base);
        Ok((self.block_tag(self.map.volume_id(base)).id, base))
    }

    /// Hand a name to every cell that does not have one.
    ///
    /// Separate from the operations that create cells, because that is the only
    /// arrangement that stays correct: an edit splits and merges cells as a side
    /// effect of changing topology, and asking each edit to name what it
    /// happened to create is how a cell ends up with two names or none.
    pub fn assign_missing_ids(&mut self) {
        self.name_cells(None);
    }

    /// The same naming, but only among the cells one orbit runs through —
    /// which is all that laying one block or sewing one face can have left
    /// unnamed.
    ///
    /// Those two are called once per block and once per face glued: a
    /// pillow along a chord of a hundred-and-fifty-block grid does eighty
    /// of them, and a sweep of the whole map apiece is where its time went.
    ///
    /// The lists are in ascending order, as a sweep of the whole map would
    /// hand them over, so the **names come out the same** either way. That
    /// matters: a name is what a script says, and renumbering would make
    /// every script written before mean something else.
    fn name_cells_near(&mut self, policy: OrbitPolicy, dart: DartIdType) {
        self.name_cells(Some((policy, dart)));
    }

    /// The cells of each dimension that one orbit runs through, each named
    /// by the dart the map names it by, in ascending order.
    fn cells_near(&self, policy: OrbitPolicy, dart: DartIdType) -> [Vec<DartIdType>; 4] {
        atomically(|t| {
            // The orbit's own iterator holds the transaction while it runs,
            // so the walk comes first and the questions after.
            let mut darts: Vec<DartIdType> = Vec::new();
            for d in self.map.orbit_tx(t, policy.clone(), dart) {
                darts.push(d?);
            }
            let mut out: [Vec<DartIdType>; 4] = Default::default();
            for d in darts {
                let cells = [
                    self.map.vertex_id_tx(t, d)?,
                    self.map.edge_id_tx(t, d)?,
                    self.map.face_id_tx(t, d)?,
                    self.map.volume_id_tx(t, d)?,
                ];
                for (slot, cell) in out.iter_mut().zip(cells) {
                    if !slot.contains(&cell) {
                        slot.push(cell);
                    }
                }
            }
            for slot in out.iter_mut() {
                slot.sort_unstable();
            }
            Ok(out)
        })
    }

    fn name_cells(&mut self, near: Option<(OrbitPolicy, DartIdType)>) {
        let [every_node, every_edge, every_face, every_block] = match near {
            Some((policy, dart)) => self.cells_near(policy, dart),
            None => [
                self.map_vertices(),
                self.map_edges(),
                self.map_faces(),
                self.map_volumes(),
            ],
        };
        let nodes: Vec<_> = every_node
            .into_iter()
            .filter(|v| self.node_tag(*v).id.is_none())
            .collect();
        for v in nodes {
            let tag = self.node_tag(v);
            let id = self.names[0].next_cell();
            self.map.write_attribute(v, NodeTag { id, ..tag });
            self.seen.nodes.insert(id, v);
        }

        let edges: Vec<_> = every_edge
            .into_iter()
            .filter(|e| self.edge_tag(*e).id.is_none())
            .collect();
        for e in edges {
            let tag = self.edge_tag(e);
            let id = self.names[1].next_cell();
            // `..tag`, not a fresh one: naming a cell must not quietly undo
            // what it was classified on.
            self.map.write_attribute(e, EdgeTag { id, ..tag });
            self.seen.edges.insert(id, e);
        }

        let faces: Vec<_> = every_face
            .into_iter()
            .filter(|f| self.face_tag(*f).id.is_none())
            .collect();
        for f in faces {
            let tag = self.face_tag(f);
            let id = self.names[2].next_cell();
            self.map.write_attribute(f, FaceTag { id, ..tag });
            self.seen.faces.insert(id, f);
        }

        let blocks: Vec<_> = every_block
            .into_iter()
            .filter(|b| self.block_tag(*b).id.is_none())
            .collect();
        for b in blocks {
            let id = self.names[3].next_cell();
            let tag = self.block_tag(b);
            self.map.write_attribute(b, BlockTag { id, ..tag });
            self.seen.blocks.insert(id, b);
        }
    }

    /// Move one node.
    ///
    /// The node is named by its stable [`CellId`], never by the identifier the
    /// map derives from darts: that one is rewritten by every edit, so a caller
    /// holding it between two operations would be pointing at something else.
    pub fn set_position(&mut self, node: CellId, to: [f64; 3]) -> Result<(), BlockingError> {
        let vertex = self
            .node_by_id(node)
            .ok_or(BlockingError::NoSuchNode(node))?;
        // A frozen node does not move for anybody. Refused rather than
        // silently ignored: a caller that asked for a move and got nothing
        // back would go on believing it happened.
        if self.node_tag(vertex).frozen {
            return Err(BlockingError::Frozen(node));
        }
        self.map.write_vertex(vertex, (to[0], to[1], to[2]));
        Ok(())
    }

    /// Where a node is classified, if anywhere.
    pub fn node_class(&self, node: CellId) -> Option<On> {
        self.node_by_id(node).and_then(|v| self.node_tag(v).on)
    }

    /// Put a node on a geometric entity, or take it off one.
    ///
    /// The kernel does not check that the node is anywhere near it: what a
    /// cell is classified on is a statement of intent, and it is the
    /// projection — which lives where the triangles are — that makes it
    /// true.
    pub fn set_node_class(&mut self, node: CellId, on: Option<On>) -> Result<(), BlockingError> {
        self.update_node(node, |tag| tag.on = on)
    }

    pub fn is_frozen(&self, node: CellId) -> bool {
        self.node_by_id(node)
            .map(|v| self.node_tag(v).frozen)
            .unwrap_or(false)
    }

    pub fn set_frozen(&mut self, node: CellId, frozen: bool) -> Result<(), BlockingError> {
        self.update_node(node, |tag| tag.frozen = frozen)
    }

    fn update_node(
        &mut self,
        node: CellId,
        change: impl FnOnce(&mut NodeTag),
    ) -> Result<(), BlockingError> {
        let vertex = self
            .node_by_id(node)
            .ok_or(BlockingError::NoSuchNode(node))?;
        let mut tag = self.node_tag(vertex);
        change(&mut tag);
        self.map.write_attribute(vertex, tag);
        Ok(())
    }

    /// Where an edge is classified, if anywhere.
    pub fn edge_class(&self, edge: CellId) -> Option<On> {
        self.edge_by_id(edge).and_then(|e| self.edge_tag(e).on)
    }

    /// Put an edge on a geometric entity, or take it off one.
    ///
    /// Never called with something measured: an edge's classification is
    /// inferred from its two ends, which is the only rule that keeps it
    /// coherent with them.
    pub fn set_edge_class(&mut self, edge: CellId, on: Option<On>) -> Result<(), BlockingError> {
        let id = self
            .edge_by_id(edge)
            .ok_or(BlockingError::NoSuchEdge(edge))?;
        let tag = self.edge_tag(id);
        self.map.write_attribute(id, EdgeTag { on, ..tag });
        Ok(())
    }

    /// Where a face is classified, if anywhere.
    pub fn face_class(&self, face: CellId) -> Option<On> {
        self.face_by_id(face).and_then(|f| self.face_tag(f).on)
    }

    /// Put a face on a geometric entity, or take it off one. Inferred from
    /// its four edges, never measured.
    pub fn set_face_class(&mut self, face: CellId, on: Option<On>) -> Result<(), BlockingError> {
        let id = self
            .face_by_id(face)
            .ok_or(BlockingError::NoSuchFace(face))?;
        let tag = self.face_tag(id);
        self.map.write_attribute(id, FaceTag { on, ..tag });
        Ok(())
    }

    /// Every face by name, with the names of its four edges.
    pub fn faces(&self) -> impl Iterator<Item = (CellId, [CellId; 4])> + '_ {
        self.map_faces().into_iter().map(move |f| {
            let edges = self
                .face_darts(f)
                .map(|d| self.edge_tag(self.map.edge_id(d)).id);
            (self.face_tag(f).id, edges)
        })
    }

    /// Every edge by name, with the names of its two ends and what it is
    /// classified on.
    pub fn edges(&self) -> impl Iterator<Item = (CellId, CellId, CellId, Option<On>)> + '_ {
        self.map_edges().into_iter().map(move |e| {
            let (a, b) = self.edge_endpoints(e);
            let tag = self.edge_tag(e);
            (tag.id, self.node_tag(a).id, self.node_tag(b).id, tag.on)
        })
    }

    /// Every node by name, with where it is, what it is classified on and
    /// whether it is frozen.
    ///
    /// Everything about a node in one pass. The alternative — a name, then
    /// `node_class(name)`, then `is_frozen(name)` — is three scans of the
    /// whole map per node, which is quadratic and was: `nodes_json` took
    /// six seconds on sixty-four blocks.
    pub fn nodes_in_full(&self) -> impl Iterator<Item = (CellId, [f64; 3], Option<On>, bool)> + '_ {
        self.map_vertices().into_iter().map(move |v| {
            let tag = self.node_tag(v);
            (
                tag.id,
                self.position(v).unwrap_or([0.0; 3]),
                tag.on,
                tag.frozen,
            )
        })
    }

    /// How many cells there are, and how much of them is pinned to a
    /// geometry — everything the readout says, counted in **one pass per
    /// dimension**.
    ///
    /// Walking the map is the expensive thing this crate does: a vertex
    /// is found by taking the smallest dart of its orbit, so iterating
    /// them costs a dart-orbit walk apiece, and those orbits grow as more
    /// blocks meet at a node. Asking separately for the nodes, the
    /// classified nodes and the frozen ones was three of those walks
    /// where one does.
    pub fn census(&self) -> Census {
        let mut out = Census::default();
        for v in self.map_vertices() {
            let tag = self.node_tag(v);
            out.nodes += 1;
            out.classified_nodes += usize::from(tag.on.is_some());
            out.frozen += usize::from(tag.frozen);
        }
        for e in self.map_edges() {
            out.edges += 1;
            out.classified_edges += usize::from(self.edge_tag(e).on.is_some());
        }
        for f in self.map_faces() {
            out.faces += 1;
            out.classified_faces += usize::from(self.face_tag(f).on.is_some());
        }
        for v in self.map_volumes() {
            if self.is_block(v) {
                out.blocks += 1;
            } else {
                out.patches += 1;
            }
        }
        out
    }

    /// Put every node on an entity, and then where that entity says it is.
    ///
    /// `pick` is the proximity search and `project` the pull onto what it
    /// found — both questions only the geometry can answer, and both
    /// passed in rather than reached for, so that the block structure goes
    /// on knowing nothing about triangles.
    ///
    /// Frozen nodes are skipped entirely: a projection is a move.
    ///
    /// One pass, no names resolved. Going through `set_node_class` and
    /// `set_position` instead is two scans of the whole map per node.
    pub fn classify_nodes(
        &mut self,
        pick: impl Fn(Option<On>, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) {
        self.classify_some(|_| true, pick, project);
    }

    /// The same, for the nodes a caller names.
    ///
    /// `pick` is given **what the node already claims** as well as where it
    /// is, for the same reason the edge and face settling is: proximity
    /// answers with entities of the model, and some of what a node can be
    /// on — a side, which is several curves under one name — is not one of
    /// them. A caller that wants a fresh start ignores the argument; one
    /// settling nodes that merely moved can keep a name that still holds.
    ///
    /// What an operation that **moves** nodes needs afterwards: a node
    /// somewhere new is on something new, and the only way to find out
    /// what is to look. Pillowing is the case that wants it — the corner
    /// of a box drawn in off the rim of a disc is still on that disc —
    /// and reaching for the blanket version instead would undo every
    /// classification anybody had made by hand.
    pub fn classify_named(
        &mut self,
        nodes: &[CellId],
        pick: impl Fn(Option<On>, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) {
        let wanted: HashSet<CellId> = nodes.iter().copied().collect();
        self.classify_some(|tag| wanted.contains(&tag.id), pick, project);
    }

    fn classify_some(
        &mut self,
        which: impl Fn(&NodeTag) -> bool,
        pick: impl Fn(Option<On>, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) {
        let found: Vec<(VertexIdType, Option<On>, Option<[f64; 3]>)> = self
            .map_vertices()
            .into_iter()
            .filter(|v| {
                let tag = self.node_tag(*v);
                !tag.frozen && which(&tag)
            })
            .map(|v| {
                let at = self.position(v).unwrap_or([0.0; 3]);
                let on = pick(self.node_tag(v).on, at);
                (v, on, on.and_then(|on| project(on, at)))
            })
            .collect();
        for (v, on, onto) in found {
            let tag = self.node_tag(v);
            self.map.write_attribute(v, NodeTag { on, ..tag });
            if let Some(p) = onto {
                self.map.write_vertex(v, (p[0], p[1], p[2]));
            }
        }
    }

    /// Settle every edge's classification from its two ends, and then
    /// every face's from its four edges.
    ///
    /// In that order, because each level consumes the one below it.
    ///
    /// `settle` answers the one question the block structure cannot. It is
    /// given what the cells of the boundary are on, the dimension this
    /// cell may not go below, **what the cell already claims**, and where
    /// the cell is — and it needs all four, because a boundary says which
    /// entities a cell could be on and never which one it is.
    ///
    /// No name is resolved anywhere in here. It runs after everything that
    /// touches the model, so quadratic was not an option: at sixty-four
    /// blocks it had moving one node taking a tenth of a second.
    pub fn reclassify(&mut self, settle: impl Fn(Settling<'_>) -> Option<On>) {
        let edges: Vec<(EdgeIdType, Option<On>)> = self
            .map_edges()
            .into_iter()
            .map(|e| {
                let (a, b) = self.edge_endpoints(e);
                let already = self.edge_tag(e).on;
                let on = match (self.node_tag(a).on, self.node_tag(b).on) {
                    // An edge is never pinned to a point, which would
                    // over-constrain it.
                    (Some(x), Some(y)) => settle(Settling {
                        boundary: &[x, y],
                        at_least: 1,
                        already,
                        middle: self.middle(&[a, b]),
                    }),
                    // One end left to go on. Nothing is **handed out** from
                    // a single end — the diagonal of a face has one end on
                    // a curve too, and it is not on that curve — but what
                    // the edge already had stays while the end that is
                    // still classified allows it.
                    //
                    // Without this a session bleeds: a pillow draws four
                    // nodes into the material, rightly letting go of the
                    // geometry on the way in, and every edge that touches
                    // one of them is cleared though it has not moved. Then
                    // a cut of one of *those* inherits nothing, and so on.
                    // Measured on one session: a fifth of the edges had
                    // lost a classification nothing had contradicted.
                    (Some(x), None) | (None, Some(x)) => {
                        let kept = settle(Settling {
                            boundary: &[x],
                            at_least: 1,
                            already,
                            middle: self.middle(&[a, b]),
                        });
                        already.filter(|on| kept == Some(*on))
                    }
                    // Neither end on anything is the inside of the model:
                    // an edge drawn in by a pillow or raised by an
                    // extrusion is not on the boundary any more, and
                    // keeping what it used to follow is what tears a mesh
                    // out through the surface.
                    (None, None) => None,
                };
                (e, on)
            })
            .collect();
        for (e, on) in edges {
            let tag = self.edge_tag(e);
            self.map.write_attribute(e, EdgeTag { on, ..tag });
        }

        let faces: Vec<(FaceIdType, Option<On>)> = self
            .map_faces()
            .into_iter()
            .map(|f| {
                let darts = self.face_darts(f);
                let edges: Vec<Option<On>> = darts
                    .iter()
                    .map(|d| self.edge_tag(self.map.edge_id(*d)).on)
                    .collect();
                let corners = darts.map(|d| self.map.vertex_id(d));
                let already = self.face_tag(f).on;
                let middle = self.middle(&corners);
                let known: Vec<On> = edges.iter().flatten().copied().collect();
                let on = if known.len() == edges.len() {
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
                    // The same rule as an edge's, one dimension up: what
                    // the face already had stays while the edges that are
                    // still classified allow it, and nothing is handed to
                    // a face whose boundary is only partly known.
                    let kept = settle(Settling {
                        boundary: &known,
                        at_least: 2,
                        already,
                        middle,
                    });
                    already.filter(|on| kept == Some(*on))
                };
                (f, on)
            })
            .collect();
        for (f, on) in faces {
            let tag = self.face_tag(f);
            self.map.write_attribute(f, FaceTag { on, ..tag });
        }
    }

    /// Forget what every edge and face is classified on.
    ///
    /// What makes a global `Session::classify` a fresh start rather
    /// than a confirmation of what is already there: the settling keeps a
    /// classification that is still consistent with its boundary, which is
    /// right while editing and wrong when the whole point is to decide
    /// again.
    pub fn unclassify_cells(&mut self) {
        for e in self.map_edges() {
            let tag = self.edge_tag(e);
            self.map.write_attribute(e, EdgeTag { on: None, ..tag });
        }
        for f in self.map_faces() {
            let tag = self.face_tag(f);
            self.map.write_attribute(f, FaceTag { on: None, ..tag });
        }
    }

    fn middle(&self, nodes: &[VertexIdType]) -> [f64; 3] {
        let mut at = [0.0; 3];
        for n in nodes {
            let p = self.position(*n).unwrap_or([0.0; 3]);
            for k in 0..3 {
                at[k] += p[k] / nodes.len() as f64;
            }
        }
        at
    }

    /// The cell a stable name currently stands for.
    ///
    /// A scan. An index would have to be invalidated by every topological
    /// change, and getting that wrong is silent; at this size the scan is not
    /// worth the hazard. It goes here when it starts to matter.
    pub fn node_by_id(&self, node: CellId) -> Option<VertexIdType> {
        if node.is_none() {
            return None;
        }
        if let Some(v) = self.seen.nodes.get(&node) {
            if self.node_tag(*v).id == node {
                return Some(*v);
            }
        }
        self.map_vertices()
            .into_iter()
            .find(|v| self.node_tag(*v).id == node)
    }

    /// Every node, by stable name and position.
    pub fn nodes(&self) -> impl Iterator<Item = (CellId, [f64; 3])> + '_ {
        self.map_vertices()
            .into_iter()
            .map(move |v| (self.node_tag(v).id, self.position(v).unwrap_or([0.0; 3])))
    }

    /// Two faces that may legally be glued to each other: both exist, they
    /// are not the same one, and neither is already welded to something.
    fn two_free_faces(
        &self,
        first: CellId,
        second: CellId,
    ) -> Result<(FaceIdType, FaceIdType), BlockingError> {
        let a = self
            .face_by_id(first)
            .ok_or(BlockingError::NoSuchFace(first))?;
        let b = self
            .face_by_id(second)
            .ok_or(BlockingError::NoSuchFace(second))?;
        if a == b {
            return Err(BlockingError::SameFace);
        }
        for (name, face) in [(first, a), (second, b)] {
            if self.map.beta::<3>(face as DartIdType) != NULL_DART_ID {
                return Err(BlockingError::AlreadyGlued(name));
            }
        }
        Ok((a, b))
    }

    /// Glue two faces together, welding each pair of corners **at the point
    /// between them**.
    ///
    /// The operation a user asks for. Neither block is the privileged one:
    /// two blocks brought together meet in the middle, so each gives way by
    /// half instead of one of them being dragged the whole distance onto the
    /// other.
    ///
    /// Classification overrides that, because it says something the glue does
    /// not know. Per pair of corners:
    ///
    /// * **both frozen** — somebody has pinned both. If they coincide there is
    ///   nothing to decide; if they do not, there is no legal answer and the
    ///   glue is refused rather than moving a node somebody froze.
    /// * **one frozen** — it does not move, and the other comes to it.
    /// * **neither** — the midpoint, projected onto whatever the welded node
    ///   turns out to be classified on. A node on a circle must end up on the
    ///   circle, and the midpoint of two points on a circle is not on it.
    ///
    /// and the classification of the welded node is the **more constrained**
    /// of the two: a corner is on a curve, and it is also on both surfaces
    /// that meet there, so the curve is what it must stay on. That is what
    /// `common` is asked for — the geometry's `meet`, not its common
    /// container — because the honest answer is often neither of the
    /// two: gluing a node on one surface to a node on another welds it onto
    /// the curve where they meet. When they share nothing at all, the lower
    /// dimension wins and the pair is reported in the return so the caller
    /// can say what it dropped.
    ///
    /// Both closures come from the geometry, which lives a layer up: the
    /// block structure holds no triangles and cannot project anything by
    /// itself. With no geometry loaded, `|_, _| None` twice degrades this to
    /// the midpoint rule, which is the right answer when nothing is
    /// classified.
    pub fn glue_faces(
        &mut self,
        first: CellId,
        second: CellId,
        common: impl Fn(On, On, [f64; 3]) -> Option<On>,
        project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>,
    ) -> Result<Vec<Unmet>, BlockingError> {
        let (a, b) = self.two_free_faces(first, second)?;
        let (da, db) = (self.face_darts(a), self.face_darts(b));
        let corner = |d: DartIdType| {
            self.position(self.map.vertex_id(d))
                .unwrap_or([f64::NAN; 3])
        };
        let (pa, pb) = (da.map(corner), db.map(corner));
        let turn = best_pairing(&pa, &pb);

        // Two frozen corners "coincide" relative to the size of what is being
        // glued, not to an absolute number: a blocking of a turbine blade and
        // one of a building are the same shape to a thousandth of a side.
        let side = (0..4)
            .map(|i| apart(pa[i], pa[(i + 1) % 4]))
            .fold(0.0, f64::max);
        let slack = 1e-6 * side.max(f64::MIN_POSITIVE);

        // Decide all four pairs before a single node moves. A refused glue
        // has to leave the blocking exactly as it was, and the only way to
        // promise that is to find out everything first.
        struct Welded {
            here: VertexIdType,
            there: VertexIdType,
            at: [f64; 3],
            on: Option<On>,
            frozen: bool,
        }
        let mut welds: Vec<Welded> = Vec::with_capacity(4);
        let mut unmet: Vec<Unmet> = Vec::new();
        for i in 0..4 {
            let (here, there) = (
                self.map.vertex_id(da[i]),
                self.map.vertex_id(db[(turn + 4 - i) % 4]),
            );
            let (ta, tb) = (self.node_tag(here), self.node_tag(there));
            let (qa, qb) = (pa[i], pb[(turn + 4 - i) % 4]);

            // Where this corner is heading, as far as can be said before the
            // classification is settled — and that is what settles it, when
            // two entities share more than one thing.
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
                    // one a node can be on while being near the other, so it
                    // is the one to keep — and a tie goes to the first face,
                    // as its name does when the two nodes merge.
                    let (kept, dropped) = if y.dim < x.dim { (y, x) } else { (x, y) };
                    unmet.push(Unmet {
                        node: ta.id,
                        kept,
                        dropped,
                    });
                    kept
                })),
            };

            let at = match (ta.frozen, tb.frozen) {
                (true, true) if apart(qa, qb) > slack => {
                    return Err(BlockingError::FrozenApart(ta.id, tb.id))
                }
                // A frozen node is where somebody put it: `hint` is
                // already that point, and no projection may move it off.
                (true, _) | (false, true) => hint,
                // Here there is nobody to contradict.
                (false, false) => on.and_then(|o| project(o, hint)).unwrap_or(hint),
            };

            welds.push(Welded {
                here,
                there,
                at,
                on,
                frozen: ta.frozen || tb.frozen,
            });
        }

        for w in &welds {
            for v in [w.here, w.there] {
                self.map.write_vertex(v, (w.at[0], w.at[1], w.at[2]));
            }
        }

        self.map.sew::<3>(da[0], db[(turn + 3) % 4])?;
        self.name_cells_near(OrbitPolicy::Face, da[0]);

        // The map's own merge keeps "either of the two, and the first when
        // both" — a reasonable default everywhere else, and not the rule for
        // a glue. What this operation decided is written over it.
        for (i, w) in welds.iter().enumerate() {
            let v = self.map.vertex_id(da[i]);
            let mut tag = self.node_tag(v);
            tag.on = w.on;
            tag.frozen = w.frozen;
            self.map.write_attribute(v, tag);
        }

        Ok(unmet)
    }

    /// Glue two faces together, welding the blocks they belong to, and
    /// **move the second onto the first**.
    ///
    /// For a caller that has already put the two faces in the same place:
    /// pillowing and extrusion both build a block against a face and then
    /// glue it there, so the corners coincide already and which one survives
    /// cannot be told apart. [`glue_faces`][Self::glue_faces] is the
    /// operation a user asks for, and it welds at the midpoint instead.
    ///
    /// The position has to be settled one way or the other before the sew:
    /// honeycomb merges two vertices by **averaging** them, so sewing faces
    /// that are apart leaves the shared corners half way between the two,
    /// whatever anybody intended. Deciding it here keeps the rule where it
    /// can be read, instead of in a merge policy that belongs to the map.
    ///
    /// Only the four corners of the face move. The rest of the second block
    /// stays where it is, so gluing blocks that are far apart deforms one of
    /// them rather than carrying it over.
    pub fn glue_onto(&mut self, first: CellId, second: CellId) -> Result<(), BlockingError> {
        let (a, b) = self.two_free_faces(first, second)?;
        let (da, db) = (self.face_darts(a), self.face_darts(b));
        let corner = |d: DartIdType| {
            self.position(self.map.vertex_id(d))
                .unwrap_or([f64::NAN; 3])
        };
        let (pa, pb) = (da.map(corner), db.map(corner));
        let turn = best_pairing(&pa, &pb);

        for i in 0..4 {
            let vertex = self.map.vertex_id(db[(turn + 4 - i) % 4]);
            self.map
                .write_vertex(vertex, (pa[i][0], pa[i][1], pa[i][2]));
        }

        // One call. `three_sew` walks the face orbit itself — β₁ down one side
        // and β₀ up the other — so it links all four pairs and merges the
        // faces, edges and vertices along the way.
        //
        // `da[0]` runs from corner A₀ to A₁, so its partner must run from the
        // B corner now sitting on A₁ back to the one on A₀. That is the dart
        // starting at `b[(turn + 3) % 4]`.
        self.map.sew::<3>(da[0], db[(turn + 3) % 4])?;
        // A sew touches the face it joins, its four edges and their nodes,
        // and nothing else.
        self.name_cells_near(OrbitPolicy::Face, da[0]);
        Ok(())
    }

    /// How many blocks a sheet runs through — what a removal would take.
    pub fn sheet_blocks(&self, sheet: &[SheetEdge]) -> usize {
        let index = self.edge_index();
        let mut blocks: Vec<VolumeIdType> = sheet
            .iter()
            .filter_map(|side| index.get(&side.edge))
            .flat_map(|at| {
                self.map
                    .orbit(OrbitPolicy::Edge, *at as DartIdType)
                    .map(|d| self.map.volume_id(d))
                    .collect::<Vec<_>>()
            })
            .collect();
        blocks.sort();
        blocks.dedup();
        blocks.len()
    }

    /// Take out the whole sheet of blocks through an edge, closing the gap.
    ///
    /// The sheet is a layer: each of its blocks is capped at either end by
    /// a face the sheet does not cross, and whatever was glued to those two
    /// caps ends up glued to each other once the layer is gone.
    ///
    /// **Which face meets which is a question about the map, not about
    /// where anything is.** From a dart of one cap, crossing into the
    /// lateral face beside it and out the far side lands on the cap at the
    /// other end, on the matching edge. Taking the β₃ of both gives the two
    /// darts the neighbors will have to be sewn along — read before the
    /// layer goes, since they are what the layer was holding together.
    ///
    /// Nothing is moved by hand either. Sewing merges the two vertices by
    /// averaging them, so the seam lands half way between the caps of its
    /// own accord, and the gap closes from both sides.
    ///
    /// A cap with nothing behind it is a cap on the model's surface; there
    /// is nothing to close against, so the model simply loses that layer.
    pub fn remove_sheet(&mut self, edge: CellId) -> Result<(), BlockingError> {
        let sheet = self.sheet_through(edge)?;

        // Which blocks the sheet runs through, and which of their edges it
        // is made of. A hexahedron the sheet crosses has four.
        let index = self.edge_index();
        let mut crossing: BTreeMap<VolumeIdType, Vec<CellId>> = BTreeMap::new();
        for side in &sheet {
            let at = *index
                .get(&side.edge)
                .ok_or(BlockingError::NoSuchEdge(side.edge))?;
            let mut blocks: Vec<VolumeIdType> = self
                .map
                .orbit(OrbitPolicy::Edge, at as DartIdType)
                .map(|d| self.map.volume_id(d))
                .collect();
            blocks.sort();
            blocks.dedup();
            for block in blocks {
                crossing.entry(block).or_default().push(side.edge);
            }
        }

        let doomed: Vec<VolumeIdType> = crossing.keys().copied().collect();
        let mut rejoin: Vec<(DartIdType, DartIdType)> = Vec::new();

        for (volume, edges) in &crossing {
            if edges.len() != 4 {
                return Err(BlockingError::BrokenSheet(edge));
            }
            let crossed: HashSet<CellId> = edges.iter().copied().collect();
            // A cap is a face of this block that the sheet does not cross.
            let caps: Vec<DartIdType> = self
                .block_face_darts(*volume)
                .into_iter()
                .filter(|d| {
                    self.face_darts(self.map.face_id(*d))
                        .iter()
                        .all(|e| !crossed.contains(&self.edge_tag(self.map.edge_id(*e)).id))
                })
                .collect();
            if caps.len() != 2 {
                return Err(BlockingError::BrokenSheet(edge));
            }

            // One dart of one cap, and the dart it answers to on the other.
            let here = caps[0];
            let there = self.across_the_block(here);
            if self.map.face_id(there) != self.map.face_id(caps[1]) {
                return Err(BlockingError::BrokenSheet(edge));
            }
            let (above, below) = (self.map.beta::<3>(here), self.map.beta::<3>(there));
            if above == NULL_DART_ID || below == NULL_DART_ID {
                continue;
            }
            // A sheet that comes back round onto itself has no outside to
            // close against.
            for neighbor in [above, below] {
                if crossing.contains_key(&self.map.volume_id(neighbor)) {
                    return Err(BlockingError::BrokenSheet(edge));
                }
            }
            // The sew will refuse a pair that does not run opposite ways,
            // and by then the layer is gone and the model has a hole in
            // it. Same test, asked while there is still nothing to undo.
            let direction = |d: DartIdType| {
                let at = self.position(self.map.vertex_id(d)).unwrap_or([0.0; 3]);
                let to = self
                    .position(self.map.vertex_id(self.map.beta::<1>(d)))
                    .unwrap_or([0.0; 3]);
                [0, 1, 2].map(|k| to[k] - at[k])
            };
            let (a, c) = (direction(above), direction(below));
            if (0..3).map(|k| a[k] * c[k]).sum::<f64>() >= 0.0 {
                return Err(BlockingError::BrokenSheet(edge));
            }
            rejoin.push((above, below));
        }

        for volume in doomed {
            self.drop_volume(volume)?;
        }
        // The darts belong to the blocks that stayed, so they outlived the
        // layer that was between them.
        for (above, below) in rejoin {
            self.map.sew::<3>(above, below)?;
        }

        self.assign_missing_ids();
        Ok(())
    }

    /// One dart per face of a block, on that block's own side.
    fn block_face_darts(&self, volume: VolumeIdType) -> Vec<DartIdType> {
        let mut seen: Vec<FaceIdType> = Vec::new();
        let mut out = Vec::new();
        for dart in self.map.orbit(OrbitPolicy::Volume, volume as DartIdType) {
            let face = self.map.face_id(dart);
            if !seen.contains(&face) {
                seen.push(face);
                out.push(dart);
            }
        }
        out
    }

    /// From a dart of one face of a hexahedron to the matching dart of the
    /// face opposite.
    ///
    /// Out through the lateral face beside this edge — β₂ — along it twice,
    /// and back in through the far side. The dart that comes out runs the
    /// other way round, which is exactly what a sew wants of a pair.
    fn across_the_block(&self, dart: DartIdType) -> DartIdType {
        let beside = self.map.beta::<2>(dart);
        let along = self.map.beta::<1>(self.map.beta::<1>(beside));
        self.map.beta::<2>(along)
    }

    /// Remove one block, leaving whatever it was glued to intact.
    ///
    /// Three steps, in this order because each needs what the previous one
    /// has not yet destroyed:
    ///
    /// 1. **unsew β₃**, while the orbits that walk takes still exist. This is
    ///    where a neighbor gets its own copy of the shared cells back — and
    ///    it is the case [`CornerKey`][crate::ids::CornerKey] was written
    ///    for: the names on both sides are cleared, but the neighbor goes on
    ///    recognizing the corner.
    /// 2. **unlink β₂ then β₁** — the raw operations, not the sewing ones.
    ///    There is no cell left to split by then; what is left is a block
    ///    being taken apart.
    /// 3. **release the darts**, which the map only allows once every β of
    ///    theirs is null — hence the order.
    pub fn remove_block(&mut self, block: CellId) -> Result<(), BlockingError> {
        let volume = self
            .block_by_id(block)
            .ok_or(BlockingError::NoSuchBlock(block))?;
        self.drop_volume(volume)?;
        // The neighbor's cells came out of the split unnamed.
        self.assign_missing_ids();
        Ok(())
    }

    /// The same, by the map's own identifier and **without** naming what is
    /// left over.
    ///
    /// Naming walks every cell of every dimension. Doing it once per block
    /// is what made taking a sheet out of a large grid slow: a layer of
    /// twenty-five blocks paid for twenty-five whole passes over the model
    /// where one at the end will do.
    fn drop_volume(&mut self, volume: VolumeIdType) -> Result<(), BlockingError> {
        // Collected first: the orbit is about to stop being walkable. β₃ is
        // not one of its generators, so this is one block's darts, never a
        // neighbor's.
        let darts: Vec<DartIdType> = self
            .map
            .orbit(OrbitPolicy::Volume, volume as DartIdType)
            .collect();

        for &dart in &darts {
            if self.map.beta::<3>(dart) != NULL_DART_ID {
                self.map.unsew::<3>(dart)?;
            }
        }
        for &dart in &darts {
            if self.map.beta::<2>(dart) != NULL_DART_ID {
                self.map.unlink::<2>(dart)?;
            }
        }
        for &dart in &darts {
            if self.map.beta::<1>(dart) != NULL_DART_ID {
                self.map.unlink::<1>(dart)?;
            }
        }
        for &dart in &darts {
            self.map.release_dart(dart)?;
        }
        Ok(())
    }

    /// Take away one face of a patch.
    ///
    /// The mirror of [`remove_block`][Self::remove_block], a dimension
    /// down, and it unsews for the same reason: whatever the face was
    /// welded to keeps its own cells. They are **split off** on the way
    /// out, not deleted with it, so a face taken out of the middle of a
    /// patch leaves the ones around it whole, with the edges that were
    /// shared back on their own boundary and free to be welded to
    /// something else.
    ///
    /// A face of a **block** is refused. Taking one side out of a shell
    /// would leave a volume that is neither open nor closed, and what the
    /// caller means is [`remove_block`][Self::remove_block].
    pub fn remove_face(&mut self, face: CellId) -> Result<(), BlockingError> {
        let f = self
            .face_by_id(face)
            .ok_or(BlockingError::NoSuchFace(face))?;
        let volume = self.map.volume_id(f as DartIdType);
        if self.is_block(volume) {
            return Err(BlockingError::NotAPatch(face));
        }

        let darts = self.face_darts(f).to_vec();
        for &dart in &darts {
            if self.map.beta::<2>(dart) != NULL_DART_ID {
                // Unsewn rather than unlinked: this is where the neighbor
                // gets its own copy of the edge and corners back.
                self.map.unsew::<2>(dart)?;
            }
        }
        for &dart in &darts {
            if self.map.beta::<1>(dart) != NULL_DART_ID {
                self.map.unlink::<1>(dart)?;
            }
        }
        for &dart in &darts {
            self.map.release_dart(dart)?;
        }
        self.assign_missing_ids();
        Ok(())
    }

    /// The block a stable name currently stands for. A scan, for the same
    /// reason [`node_by_id`][Self::node_by_id] is one.
    pub fn block_by_id(&self, block: CellId) -> Option<VolumeIdType> {
        if block.is_none() {
            return None;
        }
        if let Some(v) = self.seen.blocks.get(&block) {
            if self.block_tag(*v).id == block {
                return Some(*v);
            }
        }
        self.map_volumes()
            .into_iter()
            .find(|v| self.block_tag(*v).id == block)
    }

    /// Every edge of the sheet through this one, oriented to agree with it.
    ///
    /// A sheet is the transitive closure of "the opposite edge in a
    /// quadrangle". On a lone block that is the four edges parallel to the
    /// one you name; across glued blocks it runs on through them.
    ///
    /// The orientation is carried along with the walk, so that one cut
    /// parameter means the same thing everywhere on the sheet. If the closure
    /// ever comes back to an edge it has already seen and disagrees about
    /// which way round it runs, the sheet is not orientable — it meets itself
    /// — and that is refused rather than cut badly.
    pub fn sheet_through(&self, edge: CellId) -> Result<Vec<SheetEdge>, BlockingError> {
        let start = self
            .edge_by_id(edge)
            .ok_or(BlockingError::NoSuchEdge(edge))?;
        let (from, to) = self.edge_endpoints(start);

        // Keyed by name and walked in order, so the answer does not depend on
        // the order a hash map happened to hand things back in.
        let mut found: BTreeMap<CellId, SheetEdge> = BTreeMap::new();
        found.insert(edge, SheetEdge { edge, from, to });
        let mut pending = vec![(start, from, to)];

        while let Some((current, from, to)) = pending.pop() {
            for face in self.faces_around(current) {
                let quad = self.face_corners(face);
                let Some((next_from, next_to)) = across(quad, from, to) else {
                    continue;
                };
                let Some(next) = self.edge_between(face, next_from, next_to) else {
                    continue;
                };
                let name = self.edge_tag(next).id;
                if let Some(seen) = found.get(&name) {
                    if (seen.from, seen.to) != (next_from, next_to) {
                        return Err(BlockingError::TangledSheet(edge));
                    }
                    continue;
                }
                found.insert(
                    name,
                    SheetEdge {
                        edge: name,
                        from: next_from,
                        to: next_to,
                    },
                );
                pending.push((next, next_from, next_to));
            }
        }
        Ok(found.into_values().collect())
    }

    /// A dart of `face`, on `block`'s own side of it.
    ///
    /// A glued face has one on each side, and which side is being spoken
    /// about is the whole content of a pick: the same face belongs to two
    /// blocks and the chord through it runs a different way in each.
    fn side_dart(&self, face: CellId, block: CellId) -> Result<DartIdType, BlockingError> {
        let f = self
            .face_by_id(face)
            .ok_or(BlockingError::NoSuchFace(face))?;
        let v = self
            .block_by_id(block)
            .ok_or(BlockingError::NoSuchBlock(block))?;
        self.block_face_darts(v)
            .into_iter()
            .find(|&d| self.map.face_id(d) == f)
            .ok_or(BlockingError::FaceNotOnBlock(face, block))
    }

    /// The chord of blocks through this face, in order.
    ///
    /// A **chord** is a dual curve: in at one face of a block, out at the
    /// face opposite, and on through whatever is glued there. Exactly one
    /// chord runs through any face — a face has one pair of opposite faces
    /// across its own block — which is why one click names one.
    ///
    /// It is walked both ways from the face it was asked about, so the
    /// answer does not depend on which end was clicked. It stops at the
    /// model's boundary, where there is nothing glued on to go to, or when
    /// it closes into a ring.
    ///
    /// A chord that meets **itself** — back through a block it has already
    /// crossed, by a different pair of faces — is refused. Removing it would
    /// take one block away twice, and inserting a layer along it would ask a
    /// block to shrink in four directions it has already shrunk in.
    pub fn chord_through(
        &self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<ChordBlock>, BlockingError> {
        let start = self.side_dart(face, block)?;
        // A chord steps out of a block through the face opposite, which an
        // open sheet of faces does not have: it would walk out of the map.
        if !self.named_is_block(block) {
            return Err(BlockingError::NotABlock(block));
        }

        // One block of the chord, read from a dart of the face it is entered
        // by: the block, the face in, the face out, and the dart that leaves
        // it — which is the way on to the next block.
        let step = |d: DartIdType| {
            let out = self.across_the_block(d);
            (
                ChordBlock {
                    block: self.block_tag(self.map.volume_id(d)).id,
                    through: [
                        self.face_tag(self.map.face_id(d)).id,
                        self.face_tag(self.map.face_id(out)).id,
                    ],
                },
                out,
            )
        };

        let mut ahead: Vec<ChordBlock> = Vec::new();
        let mut closed = false;
        let mut d = start;
        loop {
            let (here, out) = step(d);
            ahead.push(here);
            let next = self.map.beta::<3>(out);
            if next == NULL_DART_ID {
                break;
            }
            if next == start {
                // A ring: going on would walk it for ever, and going back
                // would walk it again the other way.
                closed = true;
                break;
            }
            d = next;
        }

        let mut behind: Vec<ChordBlock> = Vec::new();
        if !closed {
            let mut d = self.map.beta::<3>(start);
            while d != NULL_DART_ID {
                let (here, out) = step(d);
                behind.push(here);
                d = self.map.beta::<3>(out);
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

    /// The edges to draw to show a chord: the outline of every face it
    /// crosses.
    ///
    /// A sheet is lit by its own edges, which is what a sheet *is*. A chord
    /// is a thread through faces, and the readable picture of one is those
    /// faces' outlines — a ladder of slices through the blocking, which is
    /// exactly what a removal would take out. Consecutive blocks share a
    /// face, so each edge comes back once.
    pub fn chord_wire(&self, chord: &[ChordBlock]) -> Vec<(CellId, VertexIdType, VertexIdType)> {
        let mut out: Vec<(CellId, VertexIdType, VertexIdType)> = Vec::new();
        for step in chord {
            for name in step.through {
                let Some(face) = self.face_by_id(name) else {
                    continue;
                };
                for dart in self.face_darts(face) {
                    let edge = self.edge_tag(self.map.edge_id(dart)).id;
                    if out.iter().any(|(seen, ..)| *seen == edge) {
                        continue;
                    }
                    out.push((
                        edge,
                        self.map.vertex_id(dart),
                        self.map.vertex_id(self.map.beta::<1>(dart)),
                    ));
                }
            }
        }
        out
    }

    /// Take away every block of the chord through this face.
    ///
    /// One [`remove_block`][Self::remove_block] after another, in the order
    /// the chord runs in. Each removal leaves whatever the block was glued to
    /// closed again, so a chord taken out of the middle of a blocking leaves
    /// a tunnel with sound walls rather than anything needing repair.
    pub fn remove_chord(
        &mut self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<CellId>, BlockingError> {
        let chord = self.chord_through(face, block)?;
        let gone: Vec<CellId> = chord.iter().map(|c| c.block).collect();
        for block in &gone {
            self.remove_block(*block)?;
        }
        Ok(gone)
    }

    /// Insert a layer of hexahedra all the way along a chord — an **o-grid**.
    ///
    /// The chord is a tube of blocks, and what is pillowed is its **side**:
    /// the four faces of each of its blocks that the chord does not cross.
    /// Each block of the chord draws those four in and a ring of four new
    /// blocks is laid into the space that opens up, so a cross-section of the
    /// chord goes from one block to five — which is the o-grid.
    ///
    /// The faces at the two **ends** are deliberately left out, so the tube
    /// stays open. A node on an end face belongs to two of the side faces and
    /// to no other, so it moves along the mitre of those two: inwards across
    /// the chord, and not one bit along it. The ends stay exactly where they
    /// were, flush with whatever they were on.
    pub fn pillow_chord(
        &mut self,
        face: CellId,
        block: CellId,
    ) -> Result<Vec<(CellId, Option<On>)>, BlockingError> {
        let chord = self.chord_through(face, block)?;
        let sides = self.block_faces();
        let patch: Vec<(CellId, CellId)> = chord
            .iter()
            .flat_map(|step| {
                sides
                    .iter()
                    .filter(|s| s.block == step.block && !step.through.contains(&s.face))
                    .map(|s| (s.face, s.block))
            })
            .collect();
        self.pillow(&patch)
    }

    /// Cut one edge in two, putting a node at `at`.
    ///
    /// An edge in a 3-map is a ring of darts — one per face-side around it —
    /// and every one of them has to become two. For a dart running A→B the
    /// original keeps A→P and a new one takes P→B.
    ///
    /// The β₂ and β₃ partners are then re-paired **crosswise**. If `d` runs
    /// A→B its partner `g` runs B→A, so after the cut `d` is A→P and `g`'s
    /// new half is P→A: those two are the pair. Pairing `d` with `g` again
    /// would join A→P to B→P, which share no edge at all.
    ///
    /// Both halves come out unnamed, as a split should: what comes out of
    /// one is two new cells, not a cell and a claimant to the old name.
    ///
    /// # Why the raw setters
    ///
    /// `link` and `unlink` are not the per-dart operations they look like.
    /// `one_link` also links the β₃ mirror, to keep β₃∘β₁ = β₀∘β₃; `three_link`
    /// walks whole faces and refuses a pair of different lengths. Both are
    /// right for editing a finished map and wrong here: halfway through a
    /// split the two sides of a face genuinely are different lengths, and the
    /// mirror is something this already knows how to wire. Judged by those
    /// operators, every intermediate state is invalid.
    ///
    /// So the wiring is set outright, and `is_coherent` in the tests below
    /// checks afterwards what they would have checked throughout.
    fn split_edge(
        &mut self,
        edge: EdgeIdType,
        at: [f64; 3],
    ) -> Result<VertexIdType, BlockingError> {
        let darts: Vec<DartIdType> = self
            .map
            .orbit(OrbitPolicy::Edge, edge as DartIdType)
            .collect();
        let base = self.map.allocate_used_darts(darts.len());
        let fresh: HashMap<DartIdType, DartIdType> = darts
            .iter()
            .enumerate()
            .map(|(i, d)| (*d, base + i as DartIdType))
            .collect();

        // Read every pairing before any of it is disturbed.
        let after: HashMap<DartIdType, DartIdType> =
            darts.iter().map(|&d| (d, self.map.beta::<1>(d))).collect();
        let two: HashMap<DartIdType, DartIdType> =
            darts.iter().map(|&d| (d, self.map.beta::<2>(d))).collect();
        let three: HashMap<DartIdType, DartIdType> =
            darts.iter().map(|&d| (d, self.map.beta::<3>(d))).collect();

        for &d in &darts {
            let (n, next) = (fresh[&d], after[&d]);
            self.map.set_beta::<1>(d, n);
            self.map.set_beta::<0>(n, d);
            self.map.set_beta::<1>(n, next);
            self.map.set_beta::<0>(next, n);

            // Crosswise, and only where there was a partner to begin with.
            for (i, partner) in [(2u8, two[&d]), (3u8, three[&d])] {
                if partner == NULL_DART_ID {
                    continue;
                }
                let mirror = fresh[&partner];
                if i == 2 {
                    self.map.set_beta::<2>(d, mirror);
                    self.map.set_beta::<2>(n, partner);
                } else {
                    self.map.set_beta::<3>(d, mirror);
                    self.map.set_beta::<3>(n, partner);
                }
            }
        }

        // The node in the middle, and the two halves left to be named.
        //
        // All three **inherit what the edge was classified on**. For the
        // node that is the load-bearing one: an edge lying along a curve
        // is cut into two edges along that curve and a point on it, and a
        // point on it is what the new node has to be. The caller projects
        // it, this crate having no idea where the curve goes.
        //
        // The two halves could be left to the inference — their ends now
        // agree about the same entity, so it would reach the same answer
        // — but there is no reason to make them wait for it.
        let parent = self.edge_tag(self.map.edge_id(darts[0]));
        let was = parent.on;
        let middle = self.map.vertex_id(fresh[&darts[0]]);
        self.map.write_vertex(middle, (at[0], at[1], at[2]));
        self.map.write_attribute(
            middle,
            NodeTag {
                id: CellId::NONE,
                corner_key: self.corners.next_corner(),
                on: was,
                frozen: false,
            },
        );
        for &d in &[darts[0], fresh[&darts[0]]] {
            let half = self.map.edge_id(d);
            self.map.write_attribute(
                half,
                EdgeTag {
                    id: CellId::NONE,
                    on: was,
                    // Both halves keep the parent's number, so a cut
                    // doubles the nodes along this direction — which is
                    // what cutting is for.
                    segments: parent.segments,
                },
            );
        }
        Ok(middle)
    }

    /// Cut the whole sheet through an edge, at `t` along it.
    ///
    /// Three passes, in this order, because each needs the one before it
    /// finished across the **whole** sheet rather than cell by cell: split
    /// every edge, then every face the sheet crosses, then every block.
    /// Doing one block at a time would leave its neighbor's shared face
    /// half-cut, and nothing downstream could tell that from a mistake.
    ///
    /// `t` runs along the direction [`sheet_through`][Self::sheet_through]
    /// reports, which is the same way round on every edge of the sheet —
    /// otherwise one parameter would mean `t` on some cells and `1 - t` on
    /// the next.
    pub fn split_sheet(&mut self, edge: CellId, t: f64) -> Result<Vec<CellId>, BlockingError> {
        if !t.is_finite() || t <= 0.0 || t >= 1.0 {
            return Err(BlockingError::BadParameter(t));
        }
        let sheet = self.sheet_through(edge)?;
        let from_nodes: HashSet<VertexIdType> = sheet.iter().map(|s| s.from).collect();

        // --- the edges ------------------------------------------------
        // Splitting one edge leaves the others' identifiers alone — their
        // darts are not touched — so one index serves the whole pass.
        let index = self.edge_index();
        let mut fresh: HashSet<VertexIdType> = HashSet::new();
        for side in &sheet {
            let target = *index
                .get(&side.edge)
                .ok_or(BlockingError::NoSuchEdge(side.edge))?;
            let p = self.position(side.from).unwrap_or([0.0; 3]);
            let q = self.position(side.to).unwrap_or([0.0; 3]);
            let at = [0, 1, 2].map(|k| p[k] + (q[k] - p[k]) * t);
            fresh.insert(self.split_edge(target, at)?);
        }

        // --- the faces ------------------------------------------------
        // Every cycle read first: splitting one face leaves the others
        // alone, so the list taken now is still the list to work through.
        let sides = self.face_sides();
        let mut cuts: Vec<(DartIdType, DartIdType)> = Vec::new();
        for cycle in &sides {
            if let Some(pair) = self.split_face_side(cycle, &fresh, &from_nodes) {
                cuts.push(pair);
            }
        }
        if cuts.is_empty() {
            return Err(BlockingError::BrokenSheet(edge));
        }

        // The new darts' β₃, from the invariant β₃∘β₁ = β₀∘β₃: both sides of
        // a shared face have their new dart by now, so each can find its
        // opposite number through the dart it was inserted after.
        for &(u, v) in &cuts {
            for x in [u, v] {
                let behind = self.map.beta::<0>(x);
                let mirror = self.map.beta::<3>(behind);
                if mirror != NULL_DART_ID {
                    let partner = self.map.beta::<0>(mirror);
                    self.map.set_beta::<3>(x, partner);
                    self.map.set_beta::<3>(partner, x);
                }
            }
        }

        // --- the blocks -----------------------------------------------
        // Grouped before any of them is split, since splitting one rewrites
        // its own identifier.
        let mut rings: BTreeMap<VolumeIdType, Vec<(DartIdType, DartIdType)>> = BTreeMap::new();
        for &(u, v) in &cuts {
            rings.entry(self.map.volume_id(u)).or_default().push((u, v));
        }
        for (volume, ring) in rings {
            // Blocks only, and this is the one step that is about volumes
            // at all. A block is cut in two by closing a new face between
            // its halves, along the ring of four new edges the sheet left
            // round it. A **patch** has no ring and needs none: cutting its
            // β₁ cycle in two already made two faces, welded to each other
            // along the new edge by the β₂ the cut itself set. In two
            // dimensions the cut is finished when the faces are.
            if self.is_block(volume) {
                self.insert_cut_face(&ring, edge)?;
            }
        }

        // What was split is gone: two new cells each, named afresh, and
        // each still lying where the whole did until the inference says
        // otherwise.
        for &(u, v) in &cuts {
            for x in [u, v] {
                let face = self.map.face_id(x);
                let on = self.face_tag(face).on;
                self.map.write_attribute(
                    face,
                    FaceTag {
                        id: CellId::NONE,
                        on,
                    },
                );
            }
        }
        self.assign_missing_ids();

        // The nodes the cut made, now that they have names. The caller
        // wants them because each has inherited a classification and is
        // still sitting on the straight line between the ends of the edge
        // it came from — and only the caller knows where the geometry
        // says it should be instead.
        let mut made: Vec<CellId> = fresh.iter().map(|v| self.node_tag(*v).id).collect();
        made.sort();
        Ok(made)
    }

    /// Close one block off along its ring of new edges, making two.
    ///
    /// The ring is the new edge of each side face the sheet crossed. Until
    /// now the two halves of the block are still joined, through the β₂ that
    /// pairs each ring edge's two darts across its own face. The cut face
    /// goes between them: `u` loses `v` as a partner and gains a dart of the
    /// new face, and `v` likewise, and the two new faces are β₃ to each
    /// other — which is what two blocks sharing a face are.
    fn insert_cut_face(
        &mut self,
        ring: &[(DartIdType, DartIdType)],
        edge: CellId,
    ) -> Result<(), BlockingError> {
        // Walk the ring by its geometry: the next edge starts where this one
        // ends. A block the sheet crosses has four, in one closed loop.
        let starts: HashMap<VertexIdType, usize> = ring
            .iter()
            .enumerate()
            .map(|(k, &(u, _))| (self.map.vertex_id(u), k))
            .collect();
        let mut order = vec![0usize];
        for _ in 1..ring.len() {
            let last = *order.last().unwrap();
            let ends_at = self.map.vertex_id(self.map.beta::<1>(ring[last].0));
            match starts.get(&ends_at) {
                Some(&next) if !order.contains(&next) => order.push(next),
                _ => return Err(BlockingError::BrokenSheet(edge)),
            }
        }
        let closes = self
            .map
            .vertex_id(self.map.beta::<1>(ring[*order.last().unwrap()].0));
        if closes != self.map.vertex_id(ring[order[0]].0) {
            return Err(BlockingError::BrokenSheet(edge));
        }

        // What the block being cut is part of, read before it becomes two.
        // The halves keep it: cutting a block of the fluid in two gives
        // two blocks of the fluid.
        let groups = self.block_tag(self.map.volume_id(ring[order[0]].0)).groups;

        let n = order.len();
        let base = self.map.allocate_used_darts(n * 2);
        let (a, b): (Vec<_>, Vec<_>) = (
            (0..n).map(|k| base + k as DartIdType).collect(),
            (0..n).map(|k| base + (n + k) as DartIdType).collect(),
        );

        for k in 0..n {
            let (u, v) = ring[order[k]];
            // `a` runs against `u`, so its cycle runs backwards round the
            // ring; `b` runs with it, so its cycle runs forwards.
            let behind = a[(k + n - 1) % n];
            self.map.set_beta::<1>(a[k], behind);
            self.map.set_beta::<0>(behind, a[k]);
            let ahead = b[(k + 1) % n];
            self.map.set_beta::<1>(b[k], ahead);
            self.map.set_beta::<0>(ahead, b[k]);

            self.map.set_beta::<2>(a[k], u);
            self.map.set_beta::<2>(u, a[k]);
            self.map.set_beta::<2>(b[k], v);
            self.map.set_beta::<2>(v, b[k]);
            self.map.set_beta::<3>(a[k], b[k]);
            self.map.set_beta::<3>(b[k], a[k]);
        }

        // Both halves are new blocks; neither inherits the name, and both
        // inherit the groups.
        for &d in &[a[0], b[0]] {
            let block = self.map.volume_id(d);
            self.map.write_attribute(
                block,
                BlockTag {
                    id: CellId::NONE,
                    groups,
                },
            );
        }
        Ok(())
    }

    /// Grow a block out of **every** face of a patch, all at once.
    ///
    /// A patch is extruded whole or not at all. Raising one face of it and
    /// leaving its neighbors behind would weld a block to an open sheet of
    /// faces, and in a 3-map that is not a thing: β₂ joins the faces of one
    /// volume, so the sheet and the block would have to be the same volume
    /// and a different one at once.
    ///
    /// Three things make the blocks fit together:
    ///
    /// * **one offset per node**, not per face. Each node moves along the
    ///   mitre of the normals of the faces meeting there, so two blocks
    ///   raised from two faces sharing an edge raise that edge to the *same*
    ///   two corners rather than to two pairs that happen to be close.
    /// * **a glue along every shared edge**, so the two blocks end up
    ///   sharing the face that edge was raised into — which is what makes
    ///   the result one object rather than a heap of boxes.
    /// * **the patch is consumed.** Its faces are now the bottoms of the
    ///   blocks, carrying what they were classified on, and the sheet
    ///   itself is gone.
    ///
    /// Built from [`add_block`][Self::add_block] and
    /// [`glue_onto`][Self::glue_onto], like everything else that makes
    /// blocks here.
    ///
    /// Returns the blocks, in the order their faces were read.
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
        let start = self
            .face_by_id(face)
            .ok_or(BlockingError::NoSuchFace(face))?;
        let volume = self.map.volume_id(start as DartIdType);
        if self.is_block(volume) {
            return Err(BlockingError::NotAPatch(face));
        }

        // --- read the whole patch before anything is built ---------------
        // One face of it per entry: the darts round it, the corners they
        // start at, and what every cell of it is classified on.
        struct Side {
            darts: [DartIdType; 4],
            points: [[f64; 3]; 4],
            nodes: [NodeTag; 4],
            edges: [EdgeTag; 4],
            on: Option<On>,
        }
        let mut faces: Vec<FaceIdType> = self
            .map
            .orbit(OrbitPolicy::Volume, volume as DartIdType)
            .map(|d| self.map.face_id(d))
            .collect();
        faces.sort_unstable();
        faces.dedup();

        let sides: Vec<Side> = faces
            .iter()
            .map(|&f| {
                let darts = self.face_darts(f);
                Side {
                    darts,
                    points: darts.map(|d| self.position(self.map.vertex_id(d)).unwrap_or([0.0; 3])),
                    nodes: darts.map(|d| self.node_tag(self.map.vertex_id(d))),
                    edges: darts.map(|d| self.edge_tag(self.map.edge_id(d))),
                    on: self.face_tag(f).on,
                }
            })
            .collect();

        // Where each dart's neighbor is, as (face, slot) — the pairs that
        // have to end up sharing a face once the blocks are up.
        let mut slot_of: HashMap<DartIdType, (usize, usize)> = HashMap::new();
        for (k, side) in sides.iter().enumerate() {
            for (slot, &d) in side.darts.iter().enumerate() {
                slot_of.insert(d, (k, slot));
            }
        }
        let mut seams: Vec<((usize, usize), (usize, usize))> = Vec::new();
        for (k, side) in sides.iter().enumerate() {
            for (slot, &d) in side.darts.iter().enumerate() {
                let partner = self.map.beta::<2>(d);
                if partner == NULL_DART_ID {
                    continue;
                }
                let Some(&(other, other_slot)) = slot_of.get(&partner) else {
                    // Welded to something outside the patch, which cannot
                    // happen while β₂ stays inside one volume.
                    return Err(BlockingError::LostFace);
                };
                if k < other {
                    seams.push(((k, slot), (other, other_slot)));
                }
            }
        }

        // --- where every node goes ---------------------------------------
        // By corner key rather than by position: two faces of a patch share
        // the node itself, and the map says so without anything being
        // measured.
        let mut normals: HashMap<CornerKey, Vec<[f64; 3]>> = HashMap::new();
        for side in &sides {
            let normal = quad_normal(side.points);
            let length = (0..3).map(|k| normal[k] * normal[k]).sum::<f64>().sqrt();
            if length < 1e-12 {
                return Err(BlockingError::LostFace);
            }
            let unit = normal.map(|x| if back { -x / length } else { x / length });
            for tag in &side.nodes {
                normals.entry(tag.corner_key).or_default().push(unit);
            }
        }
        let steps: HashMap<CornerKey, [f64; 3]> = normals
            .iter()
            .map(|(key, around)| (*key, mitre(around, height)))
            .collect();

        // --- out with the sheet, up with the blocks ----------------------
        self.drop_volume(volume)?;

        // Going the other way, a face is read round the other way: the
        // first four corners of a block are the face whose own outward
        // normal points back the way the block was grown from, so reading
        // them forwards while stepping backwards would build every block
        // inside out.
        let corner_at = if back { [0, 3, 2, 1] } else { [0, 1, 2, 3] };
        let edge_at = |k: usize| if back { 3 - k } else { k };

        let mut raised: Vec<(CellId, DartIdType)> = Vec::with_capacity(sides.len());
        for side in &sides {
            let mut corners = [[0.0; 3]; 8];
            for k in 0..4 {
                let from = side.points[corner_at[k]];
                let step = steps
                    .get(&side.nodes[corner_at[k]].corner_key)
                    .copied()
                    .unwrap_or([0.0; 3]);
                corners[k] = from;
                corners[4 + k] = [0, 1, 2].map(|axis| from[axis] + step[axis]);
            }
            let built = self.build_block(corners)?;
            raised.push(built);
        }

        // What the sheet was on, handed to the faces that took its place.
        for (side, &(_, first)) in sides.iter().zip(&raised) {
            let bottom = self.map.face_id(first);
            let tag = self.face_tag(bottom);
            self.map
                .write_attribute(bottom, FaceTag { on: side.on, ..tag });
            let darts = self.face_darts(bottom);
            // `HEX_FACES[0]` is read [0, 3, 2, 1], so the bottom face runs
            // backwards round the corners the patch's face ran forwards.
            for (slot, &d) in darts.iter().enumerate() {
                // Through the block's corner, and then through whichever
                // of the sheet's corners that was.
                let was = corner_at[[0, 3, 2, 1][slot]];
                let vertex = self.map.vertex_id(d);
                let node = self.node_tag(vertex);
                self.map.write_attribute(
                    vertex,
                    NodeTag {
                        on: side.nodes[was].on,
                        frozen: side.nodes[was].frozen,
                        ..node
                    },
                );
                // The edge of the bottom face leaving this corner is the
                // one the patch's face entered it by.
                let edge = self.map.edge_id(d);
                let tag = self.edge_tag(edge);
                let from = side.edges[edge_at(([0, 3, 2, 1][slot] + 3) % 4)];
                self.map.write_attribute(
                    edge,
                    EdgeTag {
                        on: from.on,
                        segments: from.segments,
                        ..tag
                    },
                );
            }
        }

        // --- and the blocks to each other --------------------------------
        for ((a, a_slot), (b, b_slot)) in seams {
            // A seam was written down in the sheet's own numbering; the
            // block's sides are numbered the way its base was read.
            //
            // Derived from `corner_at`, and **not pinned by a test**: one
            // face, two faces evenly and unevenly cut, and three in a row
            // all come out identical whether this mapping is applied or
            // not, going either way. Whoever finds the case that tells
            // them apart should put it in `tests/patches.rs`.
            let slot_of = |slot: usize| (0..4).find(|k| edge_at(*k) == slot).expect("a slot");
            let first = self.lateral(raised[a].1, slot_of(a_slot));
            let second = self.lateral(raised[b].1, slot_of(b_slot));
            self.glue_onto(first, second)?;
        }

        self.assign_missing_ids();
        Ok(raised.iter().map(|(name, _)| *name).collect())
    }

    /// The face of a block raised from slot `k` of the face it was built
    /// on: the one whose corners are `k`, `k + 1` and those two moved up.
    ///
    /// Found in [`HEX_FACES`] rather than by looking at where it is, and
    /// then by arithmetic on the base dart, because `build_block` lays the
    /// faces out in that order and knowing which is which is the whole
    /// reason it hands the dart back.
    fn lateral(&self, first: DartIdType, slot: usize) -> CellId {
        let want = [slot, (slot + 1) % 4, 4 + (slot + 1) % 4, 4 + slot];
        let found = HEX_FACES
            .iter()
            .position(|face| want.iter().all(|c| face.contains(c)))
            .expect("every edge of a hexahedron is on two side faces");
        self.face_tag(self.map.face_id(first + 4 * found as DartIdType))
            .id
    }

    /// Grow a new block out of a face, along the normal at its center.
    ///
    /// The face stays exactly where it is and the new block is laid against
    /// it: four corners copied, four more `height` away along the outward
    /// normal, and the two welded together. Which is literally what it does
    /// — [`add_block`](Self::add_block) then
    /// [`glue_faces`](Self::glue_faces), two operations that were already
    /// here and already tested, rather than a third piece of dart surgery to
    /// get wrong.
    ///
    /// The weld moves nothing: the new block's own corners were built *on*
    /// the old face's, so averaging each pair lands back where it started.
    ///
    /// Only a face on the surface can be grown from. One with a block on
    /// each side already has everything it can be glued to.
    ///
    /// Returns the new block.
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
        // would lay a second block inside the first and weld the two along
        // a face they are both behind, which is not a structure, it is a
        // mistake with a name.
        if height < 0.0 {
            return Err(BlockingError::Inwards(height));
        }

        let sides = self.block_faces();
        let side = sides
            .iter()
            .find(|s| s.face == face && s.block == block)
            .ok_or(BlockingError::FaceNotOnBlock(face, block))?;
        if sides.iter().any(|s| s.face == face && s.block != block) {
            return Err(BlockingError::NotOnTheSurface(face));
        }

        let base = side.nodes.map(|n| self.position(n).unwrap_or([0.0; 3]));
        let normal = quad_normal(base);
        let length = (normal[0].powi(2) + normal[1].powi(2) + normal[2].powi(2)).sqrt();
        // A face with no area has no normal to grow along, and nothing
        // sensible to do about it.
        if length < 1e-12 {
            return Err(BlockingError::LostFace);
        }
        let step = normal.map(|x| x * height / length);

        // `block_faces` winds a face outward from its own block, so the four
        // corners in that order are the new block's **first** face — the one
        // `HEX_FACES[0]` describes, whose own outward normal points back the
        // way we came. The other four are those, moved along.
        let mut corners = [[0.0; 3]; 8];
        corners[..4].copy_from_slice(&base);
        for k in 0..4 {
            corners[4 + k] = [0, 1, 2].map(|axis| base[k][axis] + step[axis]);
        }

        let (grown, first) = self.build_block(corners)?;
        // Which face to weld is known because we built the block: it is the
        // one the first dart is on. Nothing is searched for by position.
        let against = self.face_tag(self.map.face_id(first)).id;
        self.glue_onto(face, against)?;
        Ok(grown)
    }

    /// Insert a layer of hexahedra along a patch of faces.
    ///
    /// Each selected face is given as the pair a click gives: the face, and
    /// the block it was clicked on. That block's own nodes are drawn
    /// inwards and one hexahedron is laid into the space that opens up, so
    /// **nothing on the far side of the patch moves** — which is the point
    /// of pillowing.
    ///
    /// A face on the model's surface needs only that. A face **inside** the
    /// model needs the two blocks to stop sharing their nodes first, or
    /// drawing one block in would drag the other with it and leave no space
    /// at all. Unsewing the face is exactly that: the map splits the shared
    /// vertices and each side comes away with its own copy.
    ///
    /// Built from unsewing, [`add_block`][Self::add_block],
    /// [`glue_faces`][Self::glue_faces] and moving nodes, rather than from
    /// dart surgery. The topology is the same either way, and these already
    /// have tests behind them.
    ///
    /// A node on the **rim** of the patch is shared with blocks outside it,
    /// and moving it takes them with it. The mesh stays conforming and those
    /// blocks are deformed a little; that is the honest consequence of a
    /// shared node being shared.
    ///
    /// Returns the nodes that were drawn in, each with **what it was
    /// following** before it let go. The node itself is on nothing now and
    /// free to move: it is inside the material, and a node pinned to a
    /// curve it is no longer near drags the mesh out through the boundary.
    /// But a rim node has often only slid *along* what it was on, and the
    /// caller — which holds the triangles — cannot notice that without
    /// being told what to look for.
    pub fn pillow(
        &mut self,
        patch: &[(CellId, CellId)],
    ) -> Result<Vec<(CellId, Option<On>)>, BlockingError> {
        if patch.is_empty() {
            return Err(BlockingError::EmptyPatch);
        }
        // Drawing a block inwards means there is an inward: a patch of free
        // faces has no inside to give way.
        // A name nothing answers to is a different complaint, and the code
        // below makes it: this one is only about a block that exists and is
        // an open sheet.
        for &(_, block) in patch {
            if self.block_by_id(block).is_some_and(|v| !self.is_block(v)) {
                return Err(BlockingError::NotABlock(block));
            }
        }

        // --- read everything before a single node moves ----------------
        struct Plan {
            block: CellId,
            behind: Option<CellId>,
            points: [[f64; 3]; 4],
            inward: [f64; 3],
            depth: f64,
        }
        let sides = self.block_faces();
        let mut plans: Vec<Plan> = Vec::new();
        for &(face, block) in patch {
            let side = sides
                .iter()
                .find(|s| s.face == face && s.block == block)
                .ok_or(BlockingError::FaceNotOnBlock(face, block))?;
            let behind = sides
                .iter()
                .find(|s| s.face == face && s.block != block)
                .map(|s| s.block);

            let points = side.nodes.map(|n| self.position(n).unwrap_or([0.0; 3]));
            let normal = quad_normal(points);
            let length = (normal[0].powi(2) + normal[1].powi(2) + normal[2].powi(2)).sqrt();
            if length < 1e-12 {
                return Err(BlockingError::LostFace);
            }
            // The block's own extent in that direction, from its center to
            // the face and back again; a third of it is the layer.
            let center = self.block_center(&sides, block);
            let middle = [0, 1, 2].map(|k| points.iter().map(|p| p[k]).sum::<f64>() / 4.0);
            let reach = (0..3)
                .map(|k| (middle[k] - center[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            plans.push(Plan {
                block,
                behind,
                points,
                inward: normal.map(|x| -x / length),
                depth: 2.0 * reach * PILLOW_SHARE,
            });
        }

        // --- let go of the far side ------------------------------------
        // All of them before anything is read again: two blocks joined
        // along several faces keep sharing their nodes until the last of
        // those is undone, so one at a time would see a half-detached map.
        for &(face, _) in patch {
            if let Some(at) = self.face_by_id(face) {
                if self.map.beta::<3>(at as DartIdType) != NULL_DART_ID {
                    self.map.unsew::<3>(at as DartIdType)?;
                }
            }
        }
        // The split left the cells it touched unnamed, and the faces have
        // to be findable again below.
        self.assign_missing_ids();

        // --- where each node of the patch goes --------------------------
        let sides = self.block_faces();
        let mut facing: HashMap<VertexIdType, (Vec<[f64; 3]>, f64)> = HashMap::new();
        let mut near: Vec<BlockFace> = Vec::new();
        for plan in &plans {
            let side = self
                .side_at(&sides, plan.block, &plan.points)
                .ok_or(BlockingError::LostFace)?;
            if let Some(behind) = plan.behind {
                let other = self
                    .side_at(&sides, behind, &plan.points)
                    .ok_or(BlockingError::LostFace)?;
                // Still the same nodes: the two blocks are joined somewhere
                // else as well, so drawing one in would drag the other.
                if side.nodes.iter().any(|n| other.nodes.contains(n)) {
                    return Err(BlockingError::NotOnTheSurface(side.face));
                }
            }
            for node in side.nodes {
                let entry = facing.entry(node).or_insert((Vec::new(), f64::INFINITY));
                entry.0.push(plan.inward);
                entry.1 = entry.1.min(plan.depth);
            }
            near.push(side);
        }

        // What each node of the patch is pinned to, and where it is.
        //
        // Read before anything moves, because the node that is about to
        // move is **not** the one that ends up where it is now: the layer
        // puts a new node there, and that new one is the one on the
        // geometry. See the hand-over below.
        let was: Vec<([f64; 3], Option<On>, bool)> = facing
            .keys()
            .map(|node| {
                let tag = self.node_tag(*node);
                (self.position(*node).unwrap_or([0.0; 3]), tag.on, tag.frozen)
            })
            .collect();

        // What each of them was following, kept to hand back. The node
        // itself lets go below — it is inside the material now — but the
        // caller holds the triangles and may well decide it is still on
        // the same thing, which it cannot do without being told what that
        // was. A node sliding along the rim of the patch is the case.
        let followed: HashMap<VertexIdType, Option<On>> = facing
            .keys()
            .map(|node| (*node, self.node_tag(*node).on))
            .collect();

        let mut moved: HashMap<VertexIdType, [f64; 3]> = HashMap::new();
        for (node, (inward, depth)) in &facing {
            let from = self.position(*node).unwrap_or([0.0; 3]);
            let step = mitre(inward, *depth);
            moved.insert(*node, [0, 1, 2].map(|k| from[k] + step[k]));
        }
        for (node, to) in &moved {
            self.map.write_vertex(*node, (to[0], to[1], to[2]));
            // And it lets go of the geometry on the way in. This node is
            // **inside the material** now; what it used to follow is the
            // business of the copy the layer leaves on the surface, and a
            // node pinned to a curve it is no longer anywhere near drags
            // the mesh out through the boundary when it is projected.
            let tag = self.node_tag(*node);
            self.map.write_attribute(
                *node,
                NodeTag {
                    on: None,
                    frozen: false,
                    ..tag
                },
            );
        }

        // --- a block per face, laid into the space that opened ----------
        struct Layer {
            block: CellId,
            shrunk: [[f64; 3]; 4],
        }
        let mut layers: Vec<Layer> = Vec::new();
        for (plan, side) in plans.iter().zip(&near) {
            let shrunk = side.nodes.map(|n| moved[&n]);
            // `add_block` wants the far face first, wound so that its own
            // normal points away from the block. The patch face is wound
            // away from **its** block, which is the other way round, and
            // listing the shrunk corners first puts that right.
            let mut corners = [[0.0; 3]; 8];
            corners[..4].copy_from_slice(&shrunk);
            corners[4..].copy_from_slice(&plan.points);
            let (block, base) = self.build_block(corners)?;

            // The far four of the new block are where the patch was, so
            // they are the nodes on the geometry now. They take what the
            // nodes that moved in were carrying — the classification and
            // the freeze both, since a frozen node was frozen where it
            // stood rather than wherever it is pushed to.
            for (c, at) in corners.iter().enumerate().skip(4) {
                let Some((_, on, frozen)) = was.iter().find(|(p, _, _)| same_point(*p, *at)) else {
                    continue;
                };
                let vertex = self.map.vertex_id(corner_dart(base, c));
                let tag = self.node_tag(vertex);
                self.map.write_attribute(
                    vertex,
                    NodeTag {
                        on: *on,
                        frozen: *frozen,
                        ..tag
                    },
                );
            }
            layers.push(Layer { block, shrunk });
        }

        // Each layer onto the block it came off, the existing face named
        // first so that its nodes keep the names they had — and then onto
        // whatever was on the far side, which has not moved.
        for ((plan, side), layer) in plans.iter().zip(&near).zip(&layers) {
            let under = self
                .face_at(layer.block, &layer.shrunk)
                .ok_or(BlockingError::LostFace)?;
            self.glue_onto(side.face, under)?;
            if let Some(behind) = plan.behind {
                let far = self
                    .side_of(behind, &plan.points)
                    .ok_or(BlockingError::LostFace)?;
                let over = self
                    .face_at(layer.block, &plan.points)
                    .ok_or(BlockingError::LostFace)?;
                self.glue_onto(far.face, over)?;
            }
        }

        // And to each other, wherever two selected faces met along an edge.
        for a in 0..plans.len() {
            for b in (a + 1)..plans.len() {
                for i in 0..4 {
                    let edge = [plans[a].points[i], plans[a].points[(i + 1) % 4]];
                    let shared = (0..4).any(|j| {
                        let other = [plans[b].points[j], plans[b].points[(j + 1) % 4]];
                        same_point(edge[0], other[0]) && same_point(edge[1], other[1])
                            || same_point(edge[0], other[1]) && same_point(edge[1], other[0])
                    });
                    if !shared {
                        continue;
                    }
                    let wall = |n: usize, i: usize| {
                        [
                            plans[n].points[i],
                            plans[n].points[(i + 1) % 4],
                            layers[n].shrunk[(i + 1) % 4],
                            layers[n].shrunk[i],
                        ]
                    };
                    let here = self.face_at(layers[a].block, &wall(a, i));
                    let there = (0..4).find_map(|j| {
                        self.face_at(layers[b].block, &wall(b, j)).filter(|_| {
                            let (w, m) = (wall(b, j), wall(a, i));
                            (0..4).all(|k| m.iter().any(|p| same_point(*p, w[k])))
                        })
                    });
                    if let (Some(x), Some(y)) = (here, there) {
                        self.glue_onto(x, y)?;
                    }
                }
            }
        }

        self.assign_missing_ids();

        // The nodes that were drawn in, now that the gluing has settled
        // which vertex is which. The caller wants them because each is
        // somewhere new and so may be **on** something new: the corner of
        // a box drawn in off the rim of a disc is still on that disc, and
        // only the caller holds the triangles that say so.
        let mut drawn: Vec<(CellId, Option<On>)> = Vec::new();
        for v in self.map_vertices() {
            let at = self.position(v).unwrap_or([0.0; 3]);
            let Some((from, _)) = moved.iter().find(|(_, to)| same_point(**to, at)) else {
                continue;
            };
            drawn.push((
                self.node_tag(v).id,
                followed.get(from).copied().unwrap_or(None),
            ));
        }
        // By name, which is all that orders them: two entries for one node
        // cannot disagree about what it was following.
        drawn.sort_by_key(|(name, _)| *name);
        drawn.dedup_by_key(|(name, _)| *name);
        Ok(drawn)
    }

    /// The side of `block` whose four corners are these four points.
    fn side_at(
        &self,
        sides: &[BlockFace],
        block: CellId,
        points: &[[f64; 3]; 4],
    ) -> Option<BlockFace> {
        sides
            .iter()
            .filter(|s| s.block == block)
            .find(|s| {
                s.nodes.iter().all(|n| {
                    let p = self.position(*n).unwrap_or([0.0; 3]);
                    points.iter().any(|q| same_point(p, *q))
                })
            })
            .copied()
    }

    /// The center of a block: the mean of the corners its faces list, each
    /// of the eight counted once per face it is on, which weighs them all
    /// the same.
    fn block_center(&self, sides: &[BlockFace], block: CellId) -> [f64; 3] {
        let mut sum = [0.0; 3];
        let mut n = 0.0_f64;
        for side in sides.iter().filter(|s| s.block == block) {
            for node in side.nodes {
                let p = self.position(node).unwrap_or([0.0; 3]);
                for k in 0..3 {
                    sum[k] += p[k];
                }
                n += 1.0;
            }
        }
        sum.map(|x| x / n.max(1.0))
    }

    /// The face of a block whose four corners are these four points.
    fn face_at(&self, block: CellId, points: &[[f64; 3]; 4]) -> Option<CellId> {
        self.side_of(block, points).map(|s| s.face)
    }

    /// The side of `block` whose four corners are these four points, read
    /// from that one block rather than from every face in the map.
    fn side_of(&self, block: CellId, points: &[[f64; 3]; 4]) -> Option<BlockFace> {
        let volume = self.block_by_id(block)?;
        self.side_at(&self.faces_of(volume), block, points)
    }

    /// Every β₁ cycle in the map: one per face side.
    fn face_sides(&self) -> Vec<Vec<DartIdType>> {
        let mut seen = vec![false; self.map.n_darts()];
        let mut sides = Vec::new();
        for start in 1..self.map.n_darts() as DartIdType {
            if self.map.is_unused(start) || seen[start as usize] {
                continue;
            }
            let mut cycle = Vec::new();
            let mut d = start;
            loop {
                seen[d as usize] = true;
                cycle.push(d);
                d = self.map.beta::<1>(d);
                if d == start || d == NULL_DART_ID {
                    break;
                }
            }
            sides.push(cycle);
        }
        sides
    }

    /// Cut one face side in two, along a new edge between its two new nodes.
    ///
    /// Returns the pair of darts on that new edge: the first lies on the
    /// **`from`** side of the sheet, the second on the `to` side. Which is
    /// which is not a detail — it is what tells the block split which half
    /// of itself each quadrangle belongs to.
    fn split_face_side(
        &mut self,
        cycle: &[DartIdType],
        fresh: &HashSet<VertexIdType>,
        from_nodes: &HashSet<VertexIdType>,
    ) -> Option<(DartIdType, DartIdType)> {
        let starts: Vec<VertexIdType> = cycle.iter().map(|&d| self.map.vertex_id(d)).collect();
        let marks: Vec<usize> = (0..cycle.len())
            .filter(|&k| fresh.contains(&starts[k]))
            .collect();
        // Two new nodes, opposite each other: anything else is not a face
        // this cut touches.
        if marks.len() != 2
            || !cycle.len().is_multiple_of(2)
            || marks[1] - marks[0] != cycle.len() / 2
        {
            return None;
        }
        // The half that keeps the `from` side is the one entered from a
        // `from` node, and exactly one of the two is.
        let n = cycle.len();
        let i = if from_nodes.contains(&starts[(marks[0] + n - 1) % n]) {
            marks[0]
        } else {
            marks[1]
        };
        let j = (i + n / 2) % n;

        // What the face was on, read before the surgery takes it apart.
        // The new edge runs across the inside of that face, so that is
        // where it is — and nothing about its two ends would say so: both
        // of them are on the boundary of the face, which the face shares
        // with whatever else that boundary bounds.
        let was = self.face_tag(self.map.face_id(cycle[0])).on;

        let base = self.map.allocate_used_darts(2);
        let (u, v) = (base, base + 1);
        let (before_i, before_j) = (cycle[(i + n - 1) % n], cycle[(j + n - 1) % n]);

        self.map.set_beta::<1>(before_i, u);
        self.map.set_beta::<0>(u, before_i);
        self.map.set_beta::<1>(u, cycle[j]);
        self.map.set_beta::<0>(cycle[j], u);

        self.map.set_beta::<1>(before_j, v);
        self.map.set_beta::<0>(v, before_j);
        self.map.set_beta::<1>(v, cycle[i]);
        self.map.set_beta::<0>(cycle[i], v);

        self.map.set_beta::<2>(u, v);
        self.map.set_beta::<2>(v, u);
        let fresh = self.map.edge_id(u);
        self.map.write_attribute(
            fresh,
            EdgeTag {
                id: CellId::NONE,
                on: was,
                // Left unsaid: this edge runs **across** the face, so it
                // belongs to a sheet of its own, and that sheet's other
                // edges carry its number already. The mesher resolves a
                // sheet to one number, so it picks this one up from them.
                segments: 0,
            },
        );
        Some((u, v))
    }

    /// How many segments the sheet through this edge is divided into when
    /// the blocking is meshed, and 0 for "nobody has said".
    ///
    /// Read per sheet rather than per edge: every edge of a sheet has to
    /// carry the same number or the mesh does not match across the faces
    /// between them. What is found is resolved to the **largest** number on
    /// the sheet, so a sheet made by gluing two that were set differently
    /// is still answered conformingly, and a new edge nobody has set picks
    /// up the number its sheet already had.
    pub fn sheet_segments(&self, edge: CellId) -> Result<u32, BlockingError> {
        let index = self.edge_index();
        Ok(self
            .sheet_through(edge)?
            .iter()
            .filter_map(|s| index.get(&s.edge))
            .map(|e| self.edge_tag(*e).segments)
            .max()
            .unwrap_or(0))
    }

    /// Say how many segments the sheet through this edge is divided into.
    ///
    /// On every edge of the sheet, which is the only way it can mean
    /// anything: an edge divided into four against its neighbor's five
    /// leaves a face with nothing to match along.
    pub fn set_sheet_segments(&mut self, edge: CellId, segments: u32) -> Result<(), BlockingError> {
        let index = self.edge_index();
        for side in self.sheet_through(edge)? {
            let Some(e) = index.get(&side.edge) else {
                continue;
            };
            let tag = self.edge_tag(*e);
            self.map.write_attribute(*e, EdgeTag { segments, ..tag });
        }
        Ok(())
    }

    /// The faces an edge belongs to, by identifier, without repeats.
    fn faces_around(&self, edge: EdgeIdType) -> Vec<FaceIdType> {
        let mut faces: Vec<FaceIdType> = self
            .map
            .orbit(OrbitPolicy::Edge, edge as DartIdType)
            .map(|d| self.map.face_id(d))
            .collect();
        faces.sort_unstable();
        faces.dedup();
        faces
    }

    /// A face's four corners, in the order β₁ walks them.
    fn face_corners(&self, face: FaceIdType) -> [VertexIdType; 4] {
        self.face_darts(face).map(|d| self.map.vertex_id(d))
    }

    /// The edge of this face joining those two corners.
    fn edge_between(
        &self,
        face: FaceIdType,
        from: VertexIdType,
        to: VertexIdType,
    ) -> Option<EdgeIdType> {
        self.face_darts(face).into_iter().find_map(|d| {
            let (a, b) = self.edge_endpoints(self.map.edge_id(d));
            ((a, b) == (from, to) || (a, b) == (to, from)).then(|| self.map.edge_id(d))
        })
    }

    /// Every edge's name, in one pass.
    ///
    /// For when a whole sheet has to be resolved: asking one at a time is a
    /// scan each, and a sheet across a large grid has as many edges as the
    /// grid has rows. Built fresh for each operation and thrown away — the
    /// hazard a standing index carries is that an edit invalidates it
    /// silently, and this one never outlives the loop it was made for.
    fn edge_index(&self) -> HashMap<CellId, EdgeIdType> {
        self.map_edges()
            .into_iter()
            .map(|e| (self.edge_tag(e).id, e))
            .collect()
    }

    /// The edge a stable name currently stands for. A scan, for the same
    /// reason [`node_by_id`][Self::node_by_id] is one.
    pub fn edge_by_id(&self, edge: CellId) -> Option<EdgeIdType> {
        if edge.is_none() {
            return None;
        }
        if let Some(e) = self.seen.edges.get(&edge) {
            if self.edge_tag(*e).id == edge {
                return Some(*e);
            }
        }
        self.map_edges()
            .into_iter()
            .find(|e| self.edge_tag(*e).id == edge)
    }

    /// The four darts of a face, in the order β₁ walks them.
    ///
    /// Only meaningful for a face with one side: for a glued face the
    /// identifier is the smallest dart of an orbit spanning both blocks, and
    /// which of the two you would get is not something a caller should have to
    /// reason about. [`glue_faces`][Self::glue_faces] refuses those anyway.
    pub(crate) fn face_darts(&self, face: FaceIdType) -> [DartIdType; 4] {
        let mut out = [0; 4];
        let mut dart = face as DartIdType;
        for slot in out.iter_mut() {
            *slot = dart;
            dart = self.map.beta::<1>(dart);
        }
        out
    }

    /// The face a stable name currently stands for. A scan, for the same
    /// reason [`node_by_id`][Self::node_by_id] is one.
    pub fn face_by_id(&self, face: CellId) -> Option<FaceIdType> {
        if face.is_none() {
            return None;
        }
        if let Some(f) = self.seen.faces.get(&face) {
            if self.face_tag(*f).id == face {
                return Some(*f);
            }
        }
        self.map_faces()
            .into_iter()
            .find(|f| self.face_tag(*f).id == face)
    }

    /// Every face of every block.
    ///
    /// Listed per block *side* rather than per face. Today that is the same
    /// thing — `add_block` leaves β₃ unset, so no face is shared and this
    /// yields six entries per block — but once blocks are sewn a face between
    /// two of them belongs to both, with opposite windings, and each block
    /// still wants a closed surface of its own. Having the shape right now
    /// costs nothing and saves a format change then.
    pub fn block_faces(&self) -> Vec<BlockFace> {
        self.map_volumes()
            .into_iter()
            .flat_map(|volume| self.faces_of(volume))
            .collect()
    }

    /// The faces of **one** volume, exactly as
    /// [`block_faces`][Self::block_faces] lists them for the whole map.
    ///
    /// Asking about one block is the common case — which face of this one
    /// is at these four corners — and the whole map is a thousand times
    /// the work for it.
    pub fn faces_of(&self, volume: VolumeIdType) -> Vec<BlockFace> {
        let block = self.block_tag(volume).id;
        // One transaction for the lot: every β lookup and every cell name
        // this map hands out is a transaction of its own otherwise.
        let sides: Vec<(FaceIdType, [VertexIdType; 4])> = atomically(|t| {
            // The first dart of each face **on this block's side**.
            // `face_id` alone would not do: it is the smallest dart of the
            // whole face orbit, which once β₃ is set may well be the other
            // block's.
            // The orbit's own iterator holds the transaction while it
            // runs, so the walk comes first and the questions after.
            let mut darts: Vec<DartIdType> = Vec::new();
            for dart in self
                .map
                .orbit_tx(t, OrbitPolicy::Volume, volume as DartIdType)
            {
                darts.push(dart?);
            }
            let mut seen: Vec<FaceIdType> = Vec::new();
            let mut out: Vec<(FaceIdType, [VertexIdType; 4])> = Vec::new();
            for dart in darts {
                let face = self.map.face_id_tx(t, dart)?;
                if seen.contains(&face) {
                    continue;
                }
                seen.push(face);
                let mut nodes = [0; 4];
                let mut walk = dart;
                for slot in nodes.iter_mut() {
                    *slot = self.map.vertex_id_tx(t, walk)?;
                    walk = self.map.beta_tx::<1>(t, walk)?;
                }
                debug_assert_eq!(walk, dart, "a block face that is not a quadrangle");
                out.push((face, nodes));
            }
            // Sorted, so the order cannot depend on how the orbit happened
            // to be walked. Both modes ship these bytes and compare them.
            out.sort_by_key(|(face, _)| *face);
            Ok(out)
        });
        sides
            .into_iter()
            .map(|(face, nodes)| {
                let tag = self.face_tag(face);
                BlockFace {
                    face: tag.id,
                    block,
                    volume,
                    nodes,
                    on: tag.on,
                }
            })
            .collect()
    }

    // --- reading ----------------------------------------------------------

    /// The map itself. Topological questions belong to it, not here.
    pub fn map(&self) -> &CMap3<f64> {
        &self.map
    }

    /// Every cell of one dimension, each named by the dart the map names it
    /// by: the smallest of its orbit.
    ///
    /// `iter_vertices` and its siblings ask the map, **for every dart**,
    /// which cell that dart belongs to, and each of those questions walks
    /// the whole orbit over again. One walk per cell is enough. Going up the
    /// darts in order, the first dart of an orbit that has not been marked
    /// yet *is* the smallest of that orbit, which is exactly the name the
    /// map would give it; walking the orbit once to mark the rest off leaves
    /// one walk per cell where there were as many as the cell has darts.
    ///
    /// Several blocks meeting at a node is what makes that difference grow:
    /// the orbit is long, and the map was walking it once per dart of it.
    fn cells(&self, policy: OrbitPolicy) -> Vec<DartIdType> {
        let darts = self.map.n_darts();
        // One transaction for the whole sweep, rather than one per β
        // lookup: in this map every single `beta` is a transaction of its
        // own, and that — not the walking — is where the time went.
        atomically(|t| {
            let mut seen = vec![false; darts];
            let mut out = Vec::new();
            for start in 1..darts as DartIdType {
                if seen[start as usize] || self.map.is_unused(start) {
                    continue;
                }
                for d in self.map.orbit_tx(t, policy.clone(), start) {
                    seen[d? as usize] = true;
                }
                out.push(start);
            }
            Ok(out)
        })
    }

    /// The map's vertices, as [`CMap3::iter_vertices`] names them.
    pub fn map_vertices(&self) -> Vec<VertexIdType> {
        self.cells(OrbitPolicy::Vertex)
    }

    /// The map's edges, as [`CMap3::iter_edges`] names them.
    pub fn map_edges(&self) -> Vec<EdgeIdType> {
        self.cells(OrbitPolicy::Edge)
    }

    /// The map's faces, as [`CMap3::iter_faces`] names them.
    pub fn map_faces(&self) -> Vec<FaceIdType> {
        self.cells(OrbitPolicy::Face)
    }

    /// The map's volumes, as [`CMap3::iter_volumes`] names them.
    pub fn map_volumes(&self) -> Vec<VolumeIdType> {
        self.cells(OrbitPolicy::Volume)
    }

    pub fn node_tag(&self, node: VertexIdType) -> NodeTag {
        self.map.read_attribute(node).unwrap_or_default()
    }

    pub fn edge_tag(&self, edge: EdgeIdType) -> EdgeTag {
        self.map.read_attribute(edge).unwrap_or_default()
    }

    pub fn face_tag(&self, face: FaceIdType) -> FaceTag {
        self.map.read_attribute(face).unwrap_or_default()
    }

    pub fn block_tag(&self, block: VolumeIdType) -> BlockTag {
        self.map.read_attribute(block).unwrap_or_default()
    }

    /// The two nodes an edge joins.
    ///
    /// An edge is named by one of its darts; that dart starts at one end, and
    /// β₁ takes it to the other.
    pub fn edge_endpoints(&self, edge: EdgeIdType) -> (VertexIdType, VertexIdType) {
        let dart = edge as DartIdType;
        (
            self.map.vertex_id(dart),
            self.map.vertex_id(self.map.beta::<1>(dart)),
        )
    }

    pub fn position(&self, node: VertexIdType) -> Option<[f64; 3]> {
        self.map.read_vertex(node).map(|v| [v.0, v.1, v.2])
    }

    pub fn nb_nodes(&self) -> usize {
        self.map_vertices().len()
    }

    pub fn nb_edges(&self) -> usize {
        self.map_edges().len()
    }

    pub fn nb_faces(&self) -> usize {
        self.map_faces().len()
    }

    /// How many **blocks** — shells that close. A patch is not one.
    pub fn nb_blocks(&self) -> usize {
        self.map_volumes()
            .into_iter()
            .filter(|v| self.is_block(*v))
            .count()
    }

    /// How many patches: open sheets of faces, each one a connected
    /// component.
    pub fn nb_patches(&self) -> usize {
        self.map_volumes()
            .into_iter()
            .filter(|v| !self.is_block(*v))
            .count()
    }

    /// Darts of the orbit a cell stands for.
    pub fn orbit(&self, policy: OrbitPolicy, dart: DartIdType) -> Vec<DartIdType> {
        self.map.orbit(policy, dart).collect()
    }

    /// How many names have been handed out at each dimension. Only ever goes
    /// up, which is what makes a [`CellId`] safe to keep.
    pub fn names_handed_out(&self) -> [u32; 4] {
        [
            self.names[0].handed_out(),
            self.names[1].handed_out(),
            self.names[2].handed_out(),
            self.names[3].handed_out(),
        ]
    }
}

#[cfg(test)]
mod surgery {
    //! The dart surgery, tested where it lives.
    //!
    //! These reach into private operations on purpose. A sheet cut is three
    //! of them stacked, and finding out which one is wrong from the shape of
    //! the finished mesh is exactly the debugging session worth avoiding.

    use super::*;

    fn cube() -> Blocking {
        let mut b = Blocking::new().unwrap();
        b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
        b
    }

    /// Everything a well-formed map owes us, checked over every used dart.
    pub(super) fn is_coherent(b: &Blocking) {
        for d in 1..b.map.n_darts() as DartIdType {
            if b.map.is_unused(d) {
                continue;
            }
            let next = b.map.beta::<1>(d);
            assert_ne!(next, NULL_DART_ID, "dart {d} has no β₁");
            assert_eq!(b.map.beta::<0>(next), d, "β₀ does not undo β₁ at {d}");
            for (i, partner) in [(2, b.map.beta::<2>(d)), (3, b.map.beta::<3>(d))] {
                if partner == NULL_DART_ID {
                    continue;
                }
                let back = if i == 2 {
                    b.map.beta::<2>(partner)
                } else {
                    b.map.beta::<3>(partner)
                };
                assert_eq!(back, d, "β{i} is not an involution at {d}");
                // And it joins the same two nodes, the other way round.
                let (u, v) = b.edge_endpoints(b.map.edge_id(d));
                let (x, y) = b.edge_endpoints(b.map.edge_id(partner));
                assert_eq!((u, v), (x, y), "β{i} at {d} joins a different edge");
            }
        }
    }

    #[test]
    fn splitting_an_edge_adds_a_node_and_an_edge() {
        let mut b = cube();
        let edge = b.map.iter_edges().next().unwrap();
        let (u, v) = b.edge_endpoints(edge);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
        let middle = [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0);

        b.split_edge(edge, middle).unwrap();
        b.assign_missing_ids();

        assert_eq!(
            (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
            (9, 13, 6, 1)
        );
        is_coherent(&b);
        assert!(
            b.nodes()
                .any(|(_, at)| (0..3).all(|k| (at[k] - middle[k]).abs() < 1e-9)),
            "the new node is not where it was asked for"
        );
    }

    #[test]
    fn the_faces_either_side_grow_by_one() {
        // The edge's two faces become pentagons. They are still those two
        // faces: splitting an edge does not split a face.
        let mut b = cube();
        let edge = b.map.iter_edges().next().unwrap();
        let before: Vec<usize> = b
            .faces_around(edge)
            .iter()
            .map(|f| b.map.orbit(OrbitPolicy::Face, *f as DartIdType).count())
            .collect();
        assert_eq!(before, vec![4, 4]);

        let (u, v) = b.edge_endpoints(edge);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
        b.split_edge(edge, [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0))
            .unwrap();
        b.assign_missing_ids();

        let after: Vec<usize> = b
            .map
            .iter_faces()
            .map(|f| b.map.orbit(OrbitPolicy::Face, f as DartIdType).count())
            .collect();
        assert_eq!(after.iter().filter(|n| **n == 5).count(), 2);
        assert_eq!(after.iter().filter(|n| **n == 4).count(), 4);
    }

    #[test]
    fn both_halves_come_out_unnamed_and_are_then_named_afresh() {
        let mut b = cube();
        let edge = b.map.iter_edges().next().unwrap();
        let was = b.edge_tag(edge).id;
        let (u, v) = b.edge_endpoints(edge);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());

        b.split_edge(edge, [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0))
            .unwrap();
        b.assign_missing_ids();

        // What comes out of a split is two new cells, not one of them
        // inheriting the name.
        let names: Vec<_> = b.map.iter_edges().map(|e| b.edge_tag(e).id).collect();
        assert!(!names.contains(&was), "the old name survived the split");
        assert!(names.iter().all(|n| n.is_some()));
    }

    #[test]
    fn an_edge_shared_by_two_blocks_splits_on_both_sides() {
        // The β₃ path: the ring of darts round the edge spans both blocks,
        // and every one of them has to be cut.
        let mut b = cube();
        b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
        let mut names: Vec<_> = b.block_faces().iter().map(|s| s.block).collect();
        names.dedup();
        let seam = |b: &Blocking, block: CellId| {
            b.block_faces()
                .iter()
                .filter(|s| s.block == block)
                .find(|s| {
                    s.nodes
                        .iter()
                        .all(|n| (b.position(*n).unwrap()[0] - 1.0).abs() < 1e-9)
                })
                .unwrap()
                .face
        };
        let (first, second) = (seam(&b, names[0]), seam(&b, names[1]));
        b.glue_onto(first, second).unwrap();

        // An edge lying in the seam belongs to both blocks: four darts.
        let shared = b
            .map
            .iter_edges()
            .find(|e| {
                let (u, v) = b.edge_endpoints(*e);
                [u, v]
                    .iter()
                    .all(|n| (b.position(*n).unwrap()[0] - 1.0).abs() < 1e-9)
            })
            .expect("an edge in the seam");
        assert_eq!(
            b.map.orbit(OrbitPolicy::Edge, shared as DartIdType).count(),
            4
        );

        let (u, v) = b.edge_endpoints(shared);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
        b.split_edge(shared, [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0))
            .unwrap();
        b.assign_missing_ids();

        // One node and one edge more, and the blocks still share what they
        // shared: the two halves each have four darts round them.
        assert_eq!((b.nb_nodes(), b.nb_edges(), b.nb_blocks()), (13, 21, 2));
        is_coherent(&b);
    }
}

#[cfg(test)]
mod cutting {
    //! Cutting a sheet: the three surgeries stacked.

    use super::*;
    use crate::blocking::surgery::is_coherent;

    fn cube() -> Blocking {
        let mut b = Blocking::new().unwrap();
        b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
        b
    }

    /// An edge whose ends differ only along this axis.
    fn along(b: &Blocking, axis: usize) -> CellId {
        b.map
            .iter_edges()
            .find(|e| {
                let (u, v) = b.edge_endpoints(*e);
                let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
                (0..3).all(|k| {
                    if k == axis {
                        (p[k] - q[k]).abs() > 0.5
                    } else {
                        (p[k] - q[k]).abs() < 1e-9
                    }
                })
            })
            .map(|e| b.edge_tag(e).id)
            .expect("an edge along that axis")
    }

    #[test]
    fn cutting_a_lone_cube_makes_two_blocks() {
        let mut b = cube();
        b.split_sheet(along(&b, 2), 0.25).unwrap();

        // Two stacked hexahedra: twelve nodes, twenty edges, eleven faces.
        // The same counts as two cubes glued, which is what this now is.
        assert_eq!(
            (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
            (12, 20, 11, 2)
        );
        is_coherent(&b);
        assert!(b.block_faces().len() == 12, "each block a closed surface");
    }

    #[test]
    fn the_cut_lands_where_the_parameter_says() {
        let mut b = cube();
        b.split_sheet(along(&b, 2), 0.25).unwrap();

        // Four new nodes on the plane, and none anywhere else.
        let at: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
        assert_eq!(at.iter().filter(|z| (**z - 0.25).abs() < 1e-9).count(), 4);
        assert_eq!(at.iter().filter(|z| z.abs() < 1e-9).count(), 4);
        assert_eq!(at.iter().filter(|z| (**z - 1.0).abs() < 1e-9).count(), 4);
    }

    #[test]
    fn one_parameter_means_one_plane_across_the_whole_sheet() {
        // The concertina test. Two blocks glued, cut along a sheet that
        // crosses both: an orientation that flipped on the way through would
        // put half the new nodes at `t` and half at `1 - t`.
        let mut b = cube();
        b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
        let mut names: Vec<_> = b.block_faces().iter().map(|s| s.block).collect();
        names.dedup();
        let seam = |b: &Blocking, block: CellId| {
            b.block_faces()
                .iter()
                .filter(|s| s.block == block)
                .find(|s| {
                    s.nodes
                        .iter()
                        .all(|n| (b.position(*n).unwrap()[0] - 1.0).abs() < 1e-9)
                })
                .unwrap()
                .face
        };
        let (first, second) = (seam(&b, names[0]), seam(&b, names[1]));
        b.glue_onto(first, second).unwrap();

        b.split_sheet(along(&b, 2), 0.25).unwrap();

        let at: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
        assert_eq!(
            at.iter().filter(|z| (**z - 0.25).abs() < 1e-9).count(),
            6,
            "six nodes on one plane, not some at 0.25 and some at 0.75"
        );
        assert!(
            at.iter()
                .all(|z| z.abs() < 1e-9 || (*z - 0.25).abs() < 1e-9 || (*z - 1.0).abs() < 1e-9),
            "a node landed off the cut plane: {at:?}"
        );
        assert_eq!(b.nb_blocks(), 4);
        is_coherent(&b);
    }

    #[test]
    fn a_cut_can_be_cut_again() {
        // The strongest check that the result is a proper mesh and not
        // something that merely counts right.
        let mut b = cube();
        b.split_sheet(along(&b, 2), 0.5).unwrap();
        assert_eq!(b.nb_blocks(), 2);
        b.split_sheet(along(&b, 0), 0.5).unwrap();
        assert_eq!(b.nb_blocks(), 4);
        b.split_sheet(along(&b, 1), 0.5).unwrap();
        assert_eq!(b.nb_blocks(), 8);
        assert_eq!(
            (b.nb_nodes(), b.nb_edges(), b.nb_faces()),
            (27, 54, 36),
            "a 2x2x2 grid of hexahedra"
        );
        is_coherent(&b);
    }

    #[test]
    fn every_name_in_the_cut_is_fresh_and_nothing_is_left_unnamed() {
        let mut b = cube();
        let before: Vec<CellId> = b.map.iter_faces().map(|f| b.face_tag(f).id).collect();
        b.split_sheet(along(&b, 2), 0.5).unwrap();

        let after: Vec<CellId> = b.map.iter_faces().map(|f| b.face_tag(f).id).collect();
        assert!(after.iter().all(|n| n.is_some()), "a face came out unnamed");
        // The four faces the sheet crossed are gone, replaced by eight new
        // ones plus the cut face; the two it did not touch keep their names.
        assert_eq!(after.iter().filter(|n| before.contains(n)).count(), 2);
        assert!(b.nodes().all(|(id, _)| id.is_some()));
    }

    #[test]
    fn a_parameter_outside_the_edge_is_refused() {
        for t in [0.0, 1.0, -0.5, 1.5, f64::NAN] {
            let mut b = cube();
            let before = (b.nb_nodes(), b.nb_edges(), b.nb_blocks());
            assert!(
                matches!(
                    b.split_sheet(along(&b, 2), t),
                    Err(BlockingError::BadParameter(_))
                ),
                "t = {t} should be refused"
            );
            assert_eq!((b.nb_nodes(), b.nb_edges(), b.nb_blocks()), before);
        }
    }
}
