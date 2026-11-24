//! The vocabulary the block structure is spoken in.
//!
//! The plain data: what an operation hands back, what it refuses with, the
//! hexahedron's own tables, and the geometry that is arithmetic rather than
//! topology. Nothing in here knows how the structure is represented, which
//! is exactly why it is in here — and it is what let the representation be
//! replaced underneath without the tests, the session, the page or Lua
//! noticing. Five of `BlockingError`'s variants went with the combinatorial
//! map, being its own failures put into words.

use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::ids::CellId;

/// Which geometric entity a cell is classified on.
///
/// `dim` is 0 for a corner, 1 for a curve, 2 for a surface, and `tag` is the
/// gmsh entity tag — the geometry's own name for it. Deliberately plain
/// numbers: the block structure has no business knowing what a triangle is,
/// and the crate that does the projecting is the one that holds both.
///
/// One entity, not a set of them, which costs nothing: what a set would be
/// *for* is reading off the entities that contain the one named here, and
/// the geometry can answer that from the name alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct On {
    pub dim: u8,
    pub tag: i32,
}

/// A node a pillowing pulled into the material, and what whoever holds the
/// geometry needs to decide where it may go.
///
/// The block structure moves the node by the step and lets go of what it
/// was on; it cannot say more, having no triangles. What it **can** say is
/// which cells stayed attached to the node: the edges and faces that were
/// there before, are not part of the pillow surface, and were not handed to
/// the copy on the far side. Those are the node's **neighbours**, and the
/// rules of pillowing are about them — a neighbour on a surface has to stay
/// on it, so the node may only slide along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pulled {
    /// The node, under its own name: it is the one that moved, and the copy
    /// the layer left where it was is a new node.
    pub node: CellId,
    /// What it was on before it let go.
    pub was: Option<On>,
    /// Where it was, which is where its copy is now.
    pub from: [f64; 3],
    /// Where the step put it.
    pub to: [f64; 3],
    /// What its classified neighbours are on, each entity once, in a settled
    /// order.
    pub neighbours: Vec<On>,
}

/// A slab laid on four points, and what it found there.
///
/// The names are the ones that **survived**: a side welded to a face
/// already in the model merges two nodes into one, and what comes back is
/// the node that remains. So a caller can go on to say what each corner is
/// lying on without first asking whether its corner was swallowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slab {
    /// The block.
    pub block: CellId,
    /// Its four outer corners — the points, in the order they were given.
    pub outer: [CellId; 4],
    /// Its four inner ones, inside the material. They follow nothing, and
    /// the caller has to say so out loud: see [`Blocking::add_slab`].
    pub inner: [CellId; 4],
    /// How many of its four sides were welded to a face already there.
    pub welded: usize,
}

/// A quadrangle laid on four points, and what it met there.
///
/// As for [`Slab`], the corners are the ones that **survived**: a side
/// welded to an edge already in the model merges two nodes into one, and
/// what comes back is the node that remains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// The face.
    pub face: CellId,
    /// Its four corners, in the order the points were given.
    pub corners: [CellId; 4],
    /// How many of its four sides were welded to an edge already there.
    pub welded: usize,
}

/// One block, as the mesher needs it: its eight corners in VTK order, and
/// the cells of the blocking they bound. All by name.
pub struct Hex {
    pub block: CellId,
    pub corners: [CellId; 8],
    /// In [`HEX_EDGES`] order.
    pub edges: [CellId; 12],
    /// In [`HEX_FACES`] order.
    pub faces: [CellId; 6],
}

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

/// How thick a pillow layer is, as a share of the cell it is cut from.
pub const PILLOW_SHARE: f64 = 1.0 / 3.0;

/// How thick a slab is by default, as a share of its own shortest diagonal.
///
/// A share rather than a length, for the reason every other one here is: a
/// thickness in millimeters is right for exactly one model.
pub const SLAB_SHARE: f64 = 0.1;

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
    pub nodes: HashMap<CellId, u64>,
    pub edges: HashMap<CellId, u64>,
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
    pub loose_nodes: HashSet<CellId>,
    pub loose_edges: HashSet<CellId>,
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
    pub from: CellId,
    pub to: CellId,
}

/// Everything one layer of blocks is made of.
///
/// Not what a cut would touch — that is the sheet's edges, and
/// [`SheetEdge`] says it — but what is **there**: the blocks the sheet runs
/// through, and every face, edge and node that bounds one of them. What a
/// client needs in order to show the layer and nothing else.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetCells {
    /// The edge this layer answers to: the least of its sheet's edges.
    ///
    /// A layer has as many names as it has edges, and which one you get
    /// depends on where you were pointing. That is no use to anything that
    /// has to recognise a layer it has seen before — stepping from one to
    /// the next, above all, where the one not to go back to is the one you
    /// came from. The least is the same from wherever you read it.
    pub sheet: CellId,
    pub nodes: Vec<CellId>,
    pub edges: Vec<CellId>,
    pub faces: Vec<CellId>,
    pub blocks: Vec<CellId>,
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

/// The twelve edges of a hexahedron, as pairs of corners, in the order the
/// VTK ordering implies: the bottom ring, the top ring, then the uprights.
pub const HEX_EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

/// Which axis each of the twelve edges runs along, in [`HEX_EDGES`] order.
///
/// Derived from the corner numbering rather than trusted: the test below
/// rebuilds it from [`cube_corners`] and fails if the table has drifted.
pub const EDGE_AXIS: [u8; 12] = [0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2];

/// Which axis each of the six faces is **perpendicular** to, in
/// [`HEX_FACES`] order. So the four edges of axis `a` lie on the four faces
/// whose entry is not `a`, and the two that are are the sheet's caps.
pub const FACE_AXIS: [u8; 6] = [2, 2, 1, 0, 1, 0];

/// The face opposite each face, in [`HEX_FACES`] order. One step of a chord.
pub const OPPOSITE: [u8; 6] = [1, 0, 4, 5, 2, 3];

/// The corners of a hexahedron re-read with one axis playing the part of
/// `z`, in [`HEX_FACES`] order — so `SPLIT_FRAME[a][..4]` is the cap at the
/// low end of axis `a` and `[4..]` the cap at the high end, each wound as a
/// hexahedron wants them.
///
/// What it is for: cutting a sheet of axis `a` in two means making a block
/// out of the low cap and the four new nodes, and another out of the new
/// nodes and the high cap. Said in this frame that is one expression for
/// all three axes instead of three.
///
/// The permutations are cyclic — `(z,x,y)` and `(y,z,x)`, not a swap — so
/// that the reframed block is wound the same way round and not inside out.
/// Asserted below rather than trusted.
pub const SPLIT_FRAME: [[usize; 8]; 3] = [
    [0, 3, 7, 4, 1, 2, 6, 5],
    [0, 4, 5, 1, 3, 7, 6, 2],
    [0, 1, 2, 3, 4, 5, 6, 7],
];

#[cfg(test)]
mod hexahedron_tables {
    use super::*;

    /// A unit cube read through the tables: an edge runs along one axis and
    /// a face is flat in one, and that is what the tables say they are.
    #[test]
    fn the_axes_are_what_the_corner_numbering_makes_them() {
        let at = cube_corners([0.0, 0.0, 0.0], 1.0);
        for (k, [a, b]) in HEX_EDGES.iter().enumerate() {
            let along: Vec<usize> = (0..3).filter(|i| at[*a][*i] != at[*b][*i]).collect();
            assert_eq!(along, vec![EDGE_AXIS[k] as usize], "edge {k}");
        }
        for (f, corners) in HEX_FACES.iter().enumerate() {
            let flat: Vec<usize> = (0..3)
                .filter(|i| corners.iter().all(|c| at[*c][*i] == at[corners[0]][*i]))
                .collect();
            assert_eq!(flat, vec![FACE_AXIS[f] as usize], "face {f}");
        }
    }

    /// A cube re-read through each frame is still a cube in the same
    /// order, with the two caps at the two ends of that axis and the block
    /// not turned inside out.
    #[test]
    fn a_reframed_cube_is_still_a_cube() {
        let at = cube_corners([0.0, 0.0, 0.0], 1.0);
        for (axis, frame) in SPLIT_FRAME.iter().enumerate() {
            let p: Vec<[f64; 3]> = frame.iter().map(|c| at[*c]).collect();
            // Axis-aligned in the same order: every edge of the table moves
            // in exactly one coordinate.
            for [a, b] in HEX_EDGES {
                let moved = (0..3).filter(|k| p[a][*k] != p[b][*k]).count();
                assert_eq!(moved, 1, "axis {axis}, edge {a}-{b}");
            }
            // The low cap at one end of this axis, the high cap at the other.
            assert!(
                p[..4].iter().all(|q| q[axis] == 0.0),
                "axis {axis}: the low cap"
            );
            assert!(
                p[4..].iter().all(|q| q[axis] == 1.0),
                "axis {axis}: the high cap"
            );
            // And the right way out: a cyclic permutation of the axes, not
            // a reflection.
            let sides = [0, 1, 2].map(|k| {
                let other = p[HEX_CORNER_EDGES[0][k]];
                [0, 1, 2].map(|i| other[i] - p[0][i])
            });
            assert!(
                scaled_jacobian(p[0], sides) > 0.0,
                "axis {axis}: inside out"
            );
        }
    }

    /// The opposite face shares no corner, and is the only one that does
    /// not.
    #[test]
    fn opposite_is_the_face_with_no_corner_in_common() {
        for (f, corners) in HEX_FACES.iter().enumerate() {
            let apart: Vec<usize> = (0..6)
                .filter(|g| HEX_FACES[*g].iter().all(|c| !corners.contains(c)))
                .collect();
            assert_eq!(apart, vec![OPPOSITE[f] as usize], "face {f}");
        }
    }
}

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
pub fn across<T: Copy + PartialEq>(quad: [T; 4], from: T, to: T) -> Option<(T, T)> {
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
    /// Which groups the block is in. Carried here for the same reason `on`
    /// is: everything that draws a surface walks this list, and a lookup per
    /// entry turns a walk of the model into a walk of the model squared. It
    /// cost three seconds at a hundred and twenty-five blocks.
    ///
    /// Whether the block is a block at all rather than a patch is **not**
    /// here, and deliberately: it is a question about the shell closing, so
    /// it costs a walk of the volume, and only the face listing ever asks
    /// it — once for the model, out of [`Blocking::patches`], rather than
    /// once per side on every redraw.
    pub groups: u64,
    pub nodes: [CellId; 4],
    /// What the face is classified on. Carried here rather than looked up
    /// by name: everything that draws a surface walks this list, and a
    /// lookup per entry is a scan of the whole map per entry.
    pub on: Option<On>,
}

#[derive(Debug, Error)]
pub enum BlockingError {
    #[error("no node is named {0}")]
    NoSuchNode(CellId),
    #[error("no face is named {0}")]
    NoSuchFace(CellId),
    #[error("a face cannot be glued to itself")]
    SameFace,
    #[error("face {0} is already glued to another block")]
    AlreadyGlued(CellId),
    #[error("no block is named {0}")]
    NoSuchBlock(CellId),
    #[error("no edge is named {0}")]
    NoSuchEdge(CellId),
    #[error("the sheet through edge {0} meets itself the wrong way round")]
    TangledSheet(CellId),
    #[error("a cut has to fall strictly inside the edge, not at {0}")]
    BadParameter(f64),
    #[error("the sheet through edge {0} does not close into a surface")]
    BrokenSheet(CellId),
    #[error("the chord through face {0} comes back through a block it has already crossed")]
    TangledChord(CellId),
    #[error("patch {0} is an open sheet of faces, and this needs a block")]
    NotABlock(CellId),
    #[error("face {0} belongs to a block — take the block away, not one of its sides")]
    NotAPatch(CellId),
    #[error(
        "a face of a block grows away from it, and {0} goes the other way — \
         into the block. A face of a patch grows either way."
    )]
    Inwards(f64),
    #[error("nothing was selected to pillow")]
    EmptyPatch,
    #[error("face {0} is not a face of block {1}")]
    FaceNotOnBlock(CellId, CellId),
    #[error("face {0} is inside the model, with a block on each side")]
    NotOnTheSurface(CellId),
    #[error("could not find the face a new layer should be glued along")]
    LostFace,
    #[error(
        "the pillow surface is in more than one piece: face {0} shares no edge \
         with the rest of it"
    )]
    PillowApart(CellId),
    #[error("edge {0} is shared by more than two faces of the pillow surface")]
    PillowBranches(CellId),
    #[error(
        "the pillow surface stops at edge {0}, inside the block structure — an \
         open pillow surface has to end on the outer boundary"
    )]
    PillowEndsInside(CellId),
    /// Along an edge, two faces of the pillow surface picked from opposite
    /// sides of it: the edge, then each face with the block it was picked
    /// on.
    #[error(
        "the faces of the pillow surface take opposite sides of it along edge {0}: \
         face {1} is taken from block {2}, face {3} from block {4}, which is on the \
         other side"
    )]
    PillowTwoSides(CellId, CellId, CellId, CellId, CellId),
    #[error(
        "the pillow surface is closed and its faces take its outside — a closed \
         pillow surface takes its inside"
    )]
    PillowOutside,
    #[error("an extrusion has to go somewhere: {0} is not a positive distance")]
    BadHeight(f64),
    #[error("node {0} is frozen; thaw it before moving it")]
    Frozen(CellId),
    #[error("nodes {0} and {1} are both frozen, and not in the same place")]
    FrozenApart(CellId, CellId),
}

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
pub(crate) fn apart(p: [f64; 3], q: [f64; 3]) -> f64 {
    (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>().sqrt()
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
