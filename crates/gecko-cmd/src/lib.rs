//! # The session
//!
//! One door into the block structure, and every caller goes through it.
//!
//! The static page and the station are not two programs that happen to share a
//! library: they are two *transports* onto this one type. The wasm façade and
//! the PyO3 façade translate and nothing else. That is what makes "the console
//! and the mouse drive the same live object" true rather than aspirational —
//! and it is what makes the two modes testable against each other, by replaying
//! one script through both and comparing the bytes that come out.

pub mod fidelity;
pub mod groups;
pub mod script;
pub mod view;
pub mod vtk;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::groups::{GroupError, Groups};
use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError, Mesh, On, Smoothing, SEGMENTS};
use gecko_geom::model::GeometryError;
use gecko_geom::{BoundsError, Composites, Geometry, Model};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use view::{GeometryBuffer, MeshBuffer, NodeBuffer, ViewBuffer};

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{0}")]
    Blocking(#[from] BlockingError),
    #[error("bad command: {0}")]
    BadCommand(String),
    #[error("a cube's size must be strictly positive, not {0}")]
    BadSize(f64),
    #[error("{0}")]
    Geometry(#[from] GeometryError),
    #[error("{0}")]
    Bounds(#[from] BoundsError),
    #[error("{0}")]
    Quad(#[from] gecko_geom::QuadError),
    #[error("no geometry is loaded — open a .msh first")]
    NoGeometry,
    #[error("there is no {0} on the model")]
    NoSuchEntity(String),
    /// A cell told to go somewhere it cannot be, or cannot stay.
    #[error("{0}")]
    Unplaceable(String),
    #[error("{0}")]
    Group(#[from] GroupError),
    #[error("{0}")]
    Merge(#[from] gecko_geom::MergeError),
    /// A pillowing that would have taken a cell off what it is on: a node
    /// pulled in whose neighbours leave it nowhere to go.
    #[error(
        "node {} cannot be pulled in: {why}{}",
        .node.get().unwrap_or(u32::MAX),
        offered(.offer)
    )]
    PillowStuck {
        node: CellId,
        why: String,
        /// A pillow surface that would pass instead — the blocks behind
        /// this one — or nothing. Handed back as faces as well as said, so
        /// that a page can select them.
        offer: Vec<PickedFace>,
    },
}

/// How far a node pulled in by a pillowing may lean away from its step and
/// still count as moving away from its copy: more than 90° less this, and
/// it is refused.
///
/// The continuity tolerance, so that a dihedral angle nearly flat behaves
/// exactly as a continuous surface does: a pillow surface ending on a curve
/// between two surfaces that meet within this of flat is refused as one
/// ending in the middle of a surface is.
pub const PILLOW_ANGLE_DEGREES: f64 = 10.0;

/// The end of a refused pillowing's message: what would pass instead.
fn offered(offer: &[PickedFace]) -> String {
    if offer.is_empty() {
        return String::new();
    }
    let faces: Vec<String> = offer
        .iter()
        .map(|f| format!("face {} of block {}", f.face, f.block))
        .collect();
    format!(
        " — pillowing the blocks behind it would pass, with the pillow surface made of {}",
        faces.join(", ")
    )
}

/// Where a node a pillowing pulled in goes, and what it is then on — or
/// why it cannot go anywhere. See [`Session::settle_pulled`].
fn place_pulled(
    model: &Model,
    p: &gecko_core::Pulled,
) -> Result<(Option<On>, [f64; 3]), SessionError> {
    if p.neighbours.is_empty() {
        return Ok((None, p.to));
    }
    let stuck = |why: String| SessionError::PillowStuck {
        node: p.node,
        why,
        offer: Vec::new(),
    };
    let held: Vec<(u8, i32)> = p.neighbours.iter().map(|on| (on.dim, on.tag)).collect();
    let named = |e: &(u8, i32)| {
        format!(
            "{}{}",
            ["P", "C", "S"].get(e.0 as usize).unwrap_or(&"?"),
            e.1
        )
    };
    let list = held.iter().map(named).collect::<Vec<_>>().join(", ");
    let Some((dim, tag)) = model.destination(&held, p.to) else {
        return Err(stuck(format!(
            "the cells around it are on {list}, which have nothing in common"
        )));
    };
    if dim == 0 {
        return Err(stuck(format!(
            "the cells around it are on {list}, which leaves it only the point {}",
            named(&(dim, tag))
        )));
    }
    let at = model.project(dim, tag, p.to).unwrap_or(p.to);
    let step: [f64; 3] = [0, 1, 2].map(|k| p.to[k] - p.from[k]);
    let went: [f64; 3] = [0, 1, 2].map(|k| at[k] - p.from[k]);
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (s, w) = (norm(step), norm(went));
    let leaning = if s < 1e-12 || w < 1e-9 * s {
        90.0
    } else {
        let cos = (0..3).map(|k| step[k] * went[k]).sum::<f64>() / (s * w);
        cos.clamp(-1.0, 1.0).acos().to_degrees()
    };
    if leaning > 90.0 - PILLOW_ANGLE_DEGREES {
        return Err(stuck(format!(
            "the cells around it are on {list}, so it has to stay on {}, and it \
             cannot move away from its copy along it",
            named(&(dim, tag))
        )));
    }
    Ok((Some(On { dim, tag }), at))
}

/// How much room a bounding box leaves round the geometry, as a share of
/// each half-side.
///
/// Two percent: a box that hugs the model. The C++ left ten, which puts
/// every face of the box well clear of every surface it is going to be
/// pulled onto — and pulling them on is the first thing anybody does with
/// a box.
pub const BOX_MARGIN: f64 = 0.02;

/// How many steps back the session can go, out of the box.
///
/// Five, as in the C++. Every step is a copy of the whole block structure,
/// which is a few hundred kilobytes on a model of any size anyone edits by
/// hand — so the number is a matter of taste rather than of memory, and
/// [`Command::SetHistory`] changes it.
pub const HISTORY_STEPS: usize = 5;

/// The most it can be set to. Not a technical limit: a hundred thousand
/// steps would be a hundred thousand copies, asked for by a typo.
pub const MAX_HISTORY_STEPS: usize = 50;

/// The most segments an edge may be asked for.
///
/// Not a technical limit: ten thousand segments on an edge is a billion
/// hexahedra in the block it bounds, asked for by a typo.
pub const MAX_SEGMENTS: u32 = 200;

/// How near a node has to be to a geometric entity to be classified on it,
/// as a share of the geometry's own size.
///
/// A share rather than a length, because a tolerance in millimeters is
/// right for exactly one model. One figure for all three dimensions, as the
/// C++ defaults them — the geometry can take a different one per dimension
/// and will, the day a model needs it.
pub const SNAP_SHARE: f64 = 0.02;

/// How many rounds a smoothing makes when nobody says.
pub const SMOOTH_ROUNDS: u32 = 5;

/// How far the first round of a smoothing goes, when nobody says: half of
/// its step, which lets somebody watch what is happening to their model.
/// Later rounds start from twice the last one taken.
pub const SMOOTH_SHARE: f64 = 0.5;

fn default_snap() -> f64 {
    SNAP_SHARE
}

fn default_rounds() -> u32 {
    SMOOTH_ROUNDS
}

fn default_share() -> f64 {
    SMOOTH_SHARE
}

/// How far from smooth a join may be and still be merged, by default: the
/// continuity tolerance, as for a pillow.
pub const MERGE_ANGLE_DEGREES: f64 = PILLOW_ANGLE_DEGREES;

fn default_merge_angle() -> f64 {
    MERGE_ANGLE_DEGREES
}

fn default_segments() -> u32 {
    SEGMENTS
}

/// A removal closes the gap behind it unless it is told not to.
fn closing() -> bool {
    true
}

/// Where a cell is put by hand: one entity of the model, or several of one
/// dimension taken as one.
///
/// Several is what a cell spanning more than one entity needs — an edge
/// running along two curves, a face lying across two surfaces — and a cell
/// is classified on one entity, so the several are named once as a
/// composite and the cell is put on that.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Place {
    /// 0 a corner, 1 a curve, 2 a surface.
    pub dim: u8,
    /// The gmsh entity tag, or a list of them.
    pub tag: Tags,
}

/// One tag, or several: `3` and `[2, 4]` both read, so a page that has only
/// ever sent one goes on working.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Tags {
    One(i32),
    Many(Vec<i32>),
}

impl From<Spot> for Place {
    fn from(s: Spot) -> Self {
        Place {
            dim: s.dim,
            tag: Tags::One(s.tag),
        }
    }
}

impl Tags {
    pub fn list(&self) -> Vec<i32> {
        match self {
            Tags::One(t) => vec![*t],
            Tags::Many(ts) => ts.clone(),
        }
    }
}

/// A geometric entity, as a command names one.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Spot {
    /// 0 a corner, 1 a curve, 2 a surface.
    pub dim: u8,
    /// The gmsh entity tag.
    pub tag: i32,
}

fn default_margin() -> f64 {
    BOX_MARGIN
}

/// What each corner of a laid slab is lying on, with **nothing** spelled
/// `false`.
///
/// Lua has no `nil` that survives in a list — a `nil` inside a table literal
/// ends the array there — so a corner that follows nothing is written
/// `false`, which keeps the list one entry per corner and the fourth the
/// fourth. This is the side that reads it back: without it a script the page
/// wrote is refused when it is replayed, and a recorded script that cannot
/// be replayed is the one thing it must not be.
fn spots<'de, D>(d: D) -> Result<Vec<Option<Spot>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OrNothing {
        On(Spot),
        Nothing(bool),
    }
    let mut out = Vec::new();
    for said in Vec::<Option<OrNothing>>::deserialize(d)? {
        out.push(match said {
            Some(OrNothing::On(spot)) => Some(spot),
            // Only `false`. `true` is nothing anybody writes, and reading it
            // as "follows nothing" would be reading a mistake as a meaning.
            Some(OrNothing::Nothing(true)) => {
                return Err(serde::de::Error::custom(
                    "a corner is lying on a spot, or on nothing, written false",
                ))
            }
            Some(OrNothing::Nothing(false)) | None => None,
        });
    }
    Ok(out)
}

/// Everything that can change a session.
///
/// This set *is* the API. A convenience written in one façade would have to be
/// written again in the other, so anything worth offering is a command — which
/// also means the journal records the intent (`create_cube`) rather than eight
/// anonymous points.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    AddBlock {
        corners: [[f64; 3]; 8],
    },
    /// One quadrangular face, standing on its own.
    ///
    /// Not a block: the beginning of a **surface** blocking, which is laid
    /// on the geometry, cut into a grid, and then extruded into blocks. The
    /// corners go anticlockwise seen from the side the face faces.
    AddFace {
        corners: [[f64; 3]; 4],
        /// Empty, or one entry a corner. `None` — written `false` by the
        /// script, see [`spots`] — where a corner follows nothing.
        #[serde(default, deserialize_with = "spots")]
        on: Vec<Option<Spot>>,
        /// The geometric surface the quadrangle itself is laid on, when one
        /// was picked before the laying started.
        ///
        /// Said rather than worked out. Four corners on curves that happen
        /// to bound one surface are not a claim about which surface the
        /// quadrangle is on, and inferring it would be the interface
        /// deciding something nobody said — the same rule the sides go by.
        #[serde(default)]
        surface: Option<i32>,
    },
    /// Lay a block of small thickness on four points.
    ///
    /// What `Q` does from a surface's own corners, done from four points
    /// somebody placed: the points are the block's outer face, and it is
    /// thickened away from them. A block rather than a face because a face
    /// on its own has no inside, and everything that follows — cutting it
    /// into a grid, growing from it — wants one.
    ///
    /// `on` says what each point is lying on, in the same order, so that
    /// the block arrives already following the model rather than being
    /// classified afterwards in a second step somebody could forget.
    AddSlab {
        corners: [[f64; 3]; 4],
        /// Empty, or one entry a corner. `None` — written `false` by the
        /// script, see [`spots`] — where a corner follows nothing.
        #[serde(default, deserialize_with = "spots")]
        on: Vec<Option<Spot>>,
        /// Signed — the sign picks which side the material is on. Absent
        /// means [`SLAB_SHARE`] of the quadrangle's own shortest diagonal,
        /// which is a thickness that means the same thing on any model.
        #[serde(default)]
        thickness: Option<f64>,
    },
    /// Lay a quadrangle on a geometric surface.
    ///
    /// The four corners are the geometric corners of the surface's boundary
    /// whose interior angle is nearest a right angle, taken in boundary
    /// order. The corners of the face are classified on those, the face on
    /// the surface, and each of its four sides on the run of curves between
    /// two chosen corners — one curve when the side is one curve, and a
    /// **side**, which is a curve made of curves, when it is not.
    FaceOnSurface {
        surface: i32,
    },
    /// An axis-aligned cube, `origin` being its lowest corner.
    CreateCube {
        origin: [f64; 3],
        size: f64,
    },
    /// Move one node, named by its stable id — never by the identifier the map
    /// derives from darts, which every edit rewrites.
    MoveNode {
        node: u32,
        to: [f64; 3],
        /// Nodes that come along, by the same offset.
        ///
        /// What a drag does when several handles are picked: one of them
        /// follows the pointer and the rest keep station with it. The
        /// offset is the one that was **asked for** — `to` less where
        /// `node` was — so what each of them does with it is its own
        /// business: a node on a curve slides along that curve, a node on
        /// nothing goes where it is put, and a frozen one stays.
        #[serde(default)]
        carrying: Vec<u32>,
        /// How near an unclassified node has to come to a geometric entity
        /// to be taken by it, as a share of the geometry's own size.
        ///
        /// On the command rather than on the session because it is a
        /// property of **this** gesture: dropping a node near a curve is a
        /// different act from dragging one past it, and the interface knows
        /// which it was. Left out, the default stands.
        #[serde(default = "default_snap")]
        snap: f64,
    },
    /// Weld two blocks along a face, each pair of corners meeting at the
    /// point between them.
    ///
    /// Classification comes first: a frozen corner does not move and the
    /// other comes to it, and a welded corner is left on the more
    /// constrained of the two classifications and projected onto it. The
    /// order of the two faces decides nothing but which name survives, and
    /// which claim is kept when the two are tied.
    GlueFaces {
        first: u32,
        second: u32,
    },
    /// Take one block away. Whatever it was glued to keeps its own cells —
    /// they are split off on the way out, not deleted with it.
    RemoveBlock {
        block: u32,
    },
    /// Take several blocks away, as one step.
    RemoveBlocks {
        blocks: Vec<u32>,
    },
    /// Take away one face of a patch.
    ///
    /// Whatever it was welded to keeps its own cells, as for a block. A
    /// face of a block is refused: what that means is to take the block.
    RemoveFace {
        face: u32,
    },
    /// Take out the whole sheet of blocks through an edge.
    ///
    /// Whatever was glued to either end of the layer ends up glued to the
    /// other, with the seam half way between.
    RemoveSheet {
        edge: u32,
        /// Whether to close the gap behind the layer. Left out, it closes.
        ///
        /// `false` leaves the two walls the layer was between where they
        /// are, unglued: the model keeps its shape and gains a slot.
        #[serde(default = "closing")]
        close: bool,
    },
    /// Take out the whole chord of blocks through a face.
    ///
    /// The face is given as the pair a click gives — the face and the block
    /// it was picked on — though for a chord the side does not matter: the
    /// same thread runs through the face either way.
    RemoveChord {
        face: u32,
        block: u32,
    },
    /// Lay a layer of hexahedra all the way along a chord: an **o-grid**.
    ///
    /// The side faces of every block of the chord are pillowed and the two
    /// ends are left alone, so each block of the chord becomes five and the
    /// tube stays open at both ends.
    PillowChord {
        face: u32,
        block: u32,
        /// No longer read, as for [`Command::Pillow`]; kept so that a
        /// script that passes it still runs.
        #[serde(default = "default_snap")]
        tolerance: f64,
    },
    /// Cut the whole sheet of hexahedra through an edge, `at` along it.
    ///
    /// The parameter runs the way the sheet reports, so it means the same
    /// side of the sheet on every edge of it, and must fall strictly inside.
    SplitSheet {
        edge: u32,
        at: f64,
    },
    /// Lay a layer of hexahedra along a patch of surface faces.
    ///
    /// Each entry is a face and the block it is taken from. The block may
    /// be left out, and is then found: every face takes the same side of
    /// the surface, and a closed one its inside. The blocks are drawn in
    /// and the layer fills the space, so the model's surface does not move.
    Pillow {
        faces: Vec<PillowFace>,
        /// No longer read: a node drawn in is no longer placed by
        /// proximity but goes where the cells still attached to it allow
        /// (see `Session::settle_pulled`). Kept so that a script that
        /// passes it still runs.
        #[serde(default = "default_snap")]
        tolerance: f64,
    },
    /// Lay a layer of hexahedra round a set of blocks, towards them: along
    /// every face between a block of the set and one outside it and, with
    /// `outer` — the default — along their faces on the outer boundary too.
    PillowBlocks {
        blocks: Vec<u32>,
        #[serde(default = "closing")]
        outer: bool,
    },
    /// Grow a new block out of each of these faces, along the normal at the
    /// center of each.
    ///
    /// A face is given as the pair a click gives — the face and the block it
    /// was picked on — and none of them moves: each new block is laid
    /// against its own and welded on.
    ///
    /// A list rather than one, because two faces that meet have to go up
    /// **together**: raised one after the other they would each bring their
    /// own copy of the edge they share, and the two new blocks would lean on
    /// each other rather than hold together. Sent as one command, the shared
    /// edge rises once and the blocks are welded along the face it becomes.
    ExtrudeFace {
        faces: Vec<PickedFace>,
        height: f64,
    },
    /// Raise a block out of **every** face of a patch.
    ///
    /// The whole connected component at once, because half of one cannot be
    /// raised: a block welded to an open sheet of faces is not a thing a
    /// 3-map can hold. The blocks come out already glued to each other
    /// along the faces the shared edges were raised into, and the sheet is
    /// consumed — its faces are the bottoms of the blocks now, carrying
    /// what they were classified on.
    ExtrudePatch {
        face: u32,
        height: f64,
    },
    /// Move the nodes to where every block is as near a cube as it can be
    /// — square corners, even spacing — as far as what they are classified
    /// on allows. See `gecko_core::smoothing`.
    ///
    /// Every move is worked out from the same positions, so a symmetric
    /// configuration stays symmetric; `share` is how far the first round
    /// goes. Frozen nodes, nodes on a geometric corner, and nodes on the
    /// blocking's boundary that follow nothing all stay where they are.
    Smooth {
        #[serde(default = "default_rounds")]
        rounds: u32,
        #[serde(default = "default_share")]
        share: f64,
    },
    /// Read a geometry from a gmsh surface mesh, replacing whatever was
    /// loaded before.
    ///
    /// The **contents** of the file, not a path to it. A browser has no
    /// filesystem to name, and a command that only one of the two façades
    /// could carry out is the one thing this set is not allowed to grow.
    /// `name` is for the message and nothing else.
    LoadGeometry {
        msh: String,
        #[serde(default)]
        name: String,
    },
    /// Forget the geometry.
    ClearGeometry,
    /// Put blocks into a group, making the group if there is none of that
    /// name.
    ///
    /// Groups are of **blocks**; a face, an edge or a node is in a group
    /// when a block it bounds is. `written` says whether the group
    /// reaches the mesh as a variable of its own — a group that is only a
    /// way of looking at part of the model says no.
    Group {
        name: String,
        #[serde(default)]
        blocks: Vec<u32>,
        /// Whether the group reaches the mesh as a variable of its own.
        ///
        /// **Left out, it is not changed**: a group is created written,
        /// and `gecko.group("fluid", {7})` on a group that is only a way
        /// of looking at the model must not quietly turn it into part of
        /// the file. Said, it is set — which is how the box in the band
        /// is both ticked and unticked.
        #[serde(default)]
        written: Option<bool>,
    },
    /// Take blocks out of a group, leaving the group itself alone.
    Ungroup {
        name: String,
        blocks: Vec<u32>,
    },
    /// Give a group another name.
    RenameGroup {
        name: String,
        to: String,
    },
    /// Forget a group. Every block in it is let go of; nothing else
    /// changes, and the bit it held is free for the next one.
    DropGroup {
        name: String,
    },
    /// Back to an empty session: no blocks, no geometry, no composites, no
    /// mesh.
    ///
    /// What replaying a script needs in front of it — a script describes a
    /// session from nothing, and run on top of a model it would describe
    /// something else. One command, so **one step of undo**: a replay
    /// started by mistake is taken back like any other change.
    ///
    /// The journal is not cleared. It is the record of what was done, and
    /// this was done too; a script written out afterwards therefore carries
    /// what came before the reset, and replaying *that* arrives at the same
    /// place, because the reset in the middle of it wipes exactly what the
    /// lines above it built.
    Reset,
    /// Put every node of the blocking on the geometric entity nearest it,
    /// and every edge on what its two ends agree about.
    ///
    /// `tolerance` is a share of the geometry's own size.
    Classify {
        #[serde(default = "default_snap")]
        tolerance: f64,
    },
    /// Put nodes on a geometric entity, or take them off one.
    ///
    /// A list rather than one node, because the interface has a list: four
    /// corners of a box onto the circle they are meant to follow is one
    /// act, and it should be one step of undo and one settling of the
    /// edges and faces above them rather than four of each.
    ClassifyNode {
        nodes: Vec<u32>,
        /// Nothing frees the nodes.
        #[serde(default)]
        on: Option<Place>,
    },
    /// Put edges on an entity of the model, or on several taken as one.
    ///
    /// Inference never puts a cell on more than one entity — the diagonal
    /// of a face has the same two ends as the side beside it — so an edge
    /// that runs along two curves has to be told. It then keeps it for as
    /// long as its ends allow, and is refused here if they do not already:
    /// a class the next settling would take away is not one worth giving.
    ///
    /// Nothing takes them off the model **and keeps them off**: the
    /// settling would otherwise hand straight back what their ends imply,
    /// which is how a cell on the wrong thing is put right by hand. They
    /// stay on nothing until given a class again, or until `classify`.
    ClassifyEdge {
        edges: Vec<u32>,
        #[serde(default)]
        on: Option<Place>,
    },
    /// Put faces on a surface, or on several taken as one, or on nothing.
    /// See [`ClassifyEdge`](Command::ClassifyEdge).
    ///
    /// With `down`, the face is said to **cover** what it is put on, and its
    /// boundary goes on that boundary: each corner onto the nearest point
    /// of the rim — a corner of the rim within the snap tolerance, or else
    /// the rim curve — and each side along the curve, or the two adjacent
    /// curves, its ends are on. One gesture instead of one per cell.
    ClassifyFace {
        faces: Vec<u32>,
        #[serde(default)]
        on: Option<Place>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        down: bool,
    },
    /// Merge curves, or surfaces, of the geometry into one.
    ///
    /// Said once of the model rather than of every cell on it: the merged
    /// entity then stands in for its pieces everywhere — proximity finds it,
    /// `classify` puts cells on it, and the joins between the pieces it
    /// swallows stop being anywhere a node can be. What a CAD export cuts a
    /// closed curve or a closed surface into, put back together.
    Merge {
        dim: u8,
        tags: Vec<i32>,
    },
    /// Merge whatever meets smoothly: surfaces across a curve within
    /// `angle` degrees of flat, then curves at a corner within `angle` of
    /// straight, beyond how much they turn anyway.
    MergeSmooth {
        #[serde(default = "default_merge_angle")]
        angle: f64,
    },
    /// Stop nodes moving, or let them move again.
    FreezeNode {
        nodes: Vec<u32>,
        frozen: bool,
    },
    /// Say how many segments the sheet through this edge is divided into
    /// when the blocking is meshed.
    ///
    /// On the whole sheet, which is the only way it can mean anything: an
    /// edge divided into four against its neighbor's five leaves a face
    /// with nothing to match along.
    SetSegments {
        edge: u32,
        segments: u32,
    },
    /// Discretise every block into hexahedra, and keep the result.
    ///
    /// Kept rather than handed back, because two things want it — the
    /// picture and the file — and meshing twice to answer the same
    /// question twice would be a waste of the only expensive thing here.
    ///
    /// It does **not** change the blocking, so it is not a step of undo.
    /// It is dropped the moment the blocking changes, because a mesh of a
    /// model you have since edited is a lie, and a lie that looks right.
    Mesh {
        #[serde(default = "default_segments")]
        segments: u32,
    },
    /// Forget the mesh.
    ClearMesh,
    /// Put the block structure back the way it was before the last command
    /// that changed it.
    ///
    /// A command like any other, so that both façades get it for nothing and
    /// neither can implement its own. It is the one command that is not
    /// itself undoable: it takes a state off the stack rather than putting
    /// one on, which is what makes pressing it twice go back two steps.
    Undo,
    /// How many steps back [`Command::Undo`] can go.
    ///
    /// Shortening the history throws away the oldest states at once rather
    /// than waiting for them to fall off the end: a setting that only takes
    /// effect later is a setting nobody can tell the effect of.
    SetHistory {
        steps: usize,
    },
    /// Add one block: a box round the geometry, or round part of it.
    ///
    /// An empty `surfaces` means all of them.
    BoxGeometry {
        #[serde(default)]
        surfaces: Vec<i32>,
        #[serde(default = "default_margin")]
        margin: f64,
    },
}

/// A face, and which of its blocks it was picked on.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PickedFace {
    pub face: u32,
    pub block: u32,
}

impl From<PickedFace> for PillowFace {
    fn from(p: PickedFace) -> Self {
        PillowFace {
            face: p.face,
            block: Some(p.block),
        }
    }
}

/// A face of a pillow surface, and maybe the block it is taken from. Left
/// out, the side is found: see [`Blocking::pillow_sides`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PillowFace {
    pub face: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub message: String,
    /// The line of Lua that would have made this command, for a console to
    /// echo. Absent for the commands that leave no line — an undo takes
    /// one off, and a setting is not a step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// The view is stale and must be re-fetched. The kernel says so; a client
    /// never guesses, which is what keeps the two from drifting apart.
    pub view_changed: bool,
}

pub struct Session {
    blocking: Blocking,
    /// Nodes the command being run put there **deliberately unclassified**,
    /// which the settling must therefore leave alone.
    ///
    /// A slab's inner corners are the case: they are inside the material,
    /// they follow nothing, and each of them has its outer opposite — which
    /// *is* classified — as a neighbor. `settle_fresh` keeps fresh nodes
    /// that are beside a classified one, so without this it would project
    /// them onto the surface and flatten the slab to nothing.
    ///
    /// Here rather than on `Outcome` because it is nobody's business
    /// outside this one step: no client ever sees it, and it is emptied at
    /// the top of every command.
    left_free: Vec<CellId>,
    /// The geometry the blocks are meant to follow, once one is loaded.
    ///
    /// Beside the blocking rather than inside it: nothing in a combinatorial
    /// map wants to know about triangles, and the day a cell is classified
    /// onto a surface it will be the cell that holds the name of the
    /// surface, not the surface that holds a list of cells.
    geometry: Option<Arc<Geometry>>,
    /// The composites named over that geometry: a side is a run of curves,
    /// and a cell is classified on one entity, so a side has to be one.
    ///
    /// Beside the geometry rather than in it, because the geometry is
    /// shared — every state the undo stack holds points at the same one —
    /// and because this table is **monotone**: an undo that takes back the
    /// operation which made a composite leaves a name nobody uses, where
    /// removing it would leave a cell classified on nothing at all.
    composites: Composites,
    /// Bumped whenever the geometry is replaced or dropped.
    ///
    /// This is how a client knows to re-fetch a buffer it would otherwise
    /// have to guess about. A number it compares with its own beats a flag
    /// on an outcome, because it is just as right for a change this client
    /// did not make — someone typing `load_geometry` into the console.
    geometry_generation: u32,
    /// Kept between views so the scene does not slide every time something
    /// moves. See [`view::stable_center`].
    origin: Option<[f64; 3]>,
    /// What the session looked like before each of the last few commands,
    /// oldest at the front.
    ///
    /// Whole states rather than a journal of commands to replay. Both would
    /// give the right answer, and the state is the one that gives it in the
    /// same *names*: a replay rebuilds the model, and a rebuilt model hands
    /// out fresh identifiers, so every name a client was holding — the node
    /// it has selected, the face it has lit — would quietly come to mean
    /// something else, or nothing. See [`Blocking::snapshot`].
    past: VecDeque<State>,
    /// How many of those to keep.
    steps: usize,
    /// The last mesh built, and a number that counts the builds.
    ///
    /// Beside the blocking rather than in it, like the geometry and for
    /// the same reason: a combinatorial map of blocks has no business
    /// holding a hundred thousand hexahedra. The number is how a client
    /// knows the picture it is holding is not the current one — the same
    /// bargain as `geometry_generation`.
    mesh: Option<Mesh>,
    mesh_generation: u32,
    /// The box round the geometry, and which geometry it was worked out
    /// for — the Arc's address. See [`settle_origin`][Self::settle_origin].
    geometry_box: Option<(usize, [f64; 3], [f64; 3])>,
    /// What the groups of blocks are called, and which reach the mesh.
    /// Beside the block structure rather than in it, like the composites
    /// and for the same reason: the model holds which blocks are in a
    /// group, and the words for them are not the model's business.
    groups: Groups,
    /// How near a node has to be to the geometry to be taken by it, as a
    /// share of the model's size — the last one [`Command::Classify`] was
    /// given, or the kernel's own until somebody says.
    ///
    /// Remembered because the nodes an operation makes are judged by it,
    /// and the right yardstick for that is the one whoever is working said
    /// to use: somebody classifying at five percent means five percent.
    /// A setting rather than state, so an undo does not put an old one
    /// back — like how many steps of history there are.
    tolerance: f64,
    /// Everything that was done, in order, as the commands that did it.
    ///
    /// This is the session's history, and it is kept here rather than in
    /// any client because both of them — the mouse and the console — go
    /// through this one door. [`script::write`] turns it into Lua.
    journal: Vec<Command>,
}

/// A session as it stood, as far as anything outside can tell.
///
/// The geometry is shared rather than copied: it is the big object, it is
/// read-only once loaded, and loading one is the only thing that can replace
/// it. Five undo steps therefore cost five block structures and no
/// triangles at all.
struct State {
    blocking: Blocking,
    geometry: Option<Arc<Geometry>>,
    /// Kept beside the geometry rather than inside it, and so saved here
    /// too: `Reset` empties them, and an undo of a reset that gave the
    /// blocking back but not the sides it was classified on would leave a
    /// model classified on entities nothing could resolve.
    composites: Composites,
    /// The same for what the groups are called. Which blocks are in them
    /// travels in the block structure, so the two have to go back
    /// together or a group would come back nameless.
    groups: Groups,
    origin: Option<[f64; 3]>,
    /// How long the journal was. An undo cuts it back to here, so the
    /// script always describes the session as it now stands rather than
    /// everything anybody ever tried.
    journal: usize,
}

impl Session {
    pub fn new() -> Result<Self, SessionError> {
        Ok(Session {
            blocking: Blocking::new()?,
            left_free: Vec::new(),
            geometry: None,
            composites: Composites::default(),
            geometry_box: None,
            groups: Groups::new(),
            geometry_generation: 0,
            origin: None,
            past: VecDeque::new(),
            steps: HISTORY_STEPS,
            mesh: None,
            mesh_generation: 0,
            tolerance: SNAP_SHARE,
            journal: Vec::new(),
        })
    }

    pub fn blocking(&self) -> &Blocking {
        &self.blocking
    }

    pub fn geometry(&self) -> Option<&Geometry> {
        self.geometry.as_deref()
    }

    /// How many steps back it can go, and how many it has.
    pub fn history(&self) -> (usize, usize) {
        (self.steps, self.past.len())
    }

    /// The one mutation path.
    pub fn apply(&mut self, command: Command) -> Result<Outcome, SessionError> {
        // The two that are about the history rather than about the model.
        // Neither leaves a state behind: an undo that remembered what it
        // undid would be a toggle, and a setting is not a step.
        match command {
            Command::Undo => return self.undo(),
            Command::SetHistory { steps } => return Ok(self.set_history(steps)),
            _ => {}
        }

        // Before the command, because that is the state being left; kept in
        // hand rather than pushed, because a command that fails or changes
        // nothing is not a step back either. Copying the structure costs
        // about what one refresh of the view costs, which is the right price
        // for being able to take a thing back.
        //
        // Taken even with the history switched off, and that is not the
        // same thing: the setting says how many states are **kept**, and
        // this one is held for the length of one command. It is what puts
        // the model back when the command fails. An operation that gives up
        // half way — a layer removed and a seam left unsewn — would
        // otherwise leave the model with a hole in it and the reply saying
        // nothing happened.
        let before = self.capture()?;
        // Whatever the last command left here is spent. A command that does
        // not set it leaves nothing free on purpose, which is the usual case.
        self.left_free.clear();
        // Where the nodes end, so that the ones the command is about to
        // make can be told from the ones it renames.
        let mark = self.blocking.corner_mark();
        // Kept before the command is consumed, and trimmed: a geometry
        // command carries the whole file, and the journal wants the name.
        let written = trim(command.clone());
        let mut outcome = match self.run(command) {
            Ok(outcome) => outcome,
            Err(e) => {
                self.restore(before);
                return Err(e);
            }
        };
        outcome.script = Some(script::line(&written));
        self.journal.push(written);
        if outcome.view_changed {
            if self.steps > 0 {
                self.push(before);
            }
            self.settle_origin();
            // The nodes this command made, onto the model if they are on
            // it — bar the ones it left free on purpose. Before the settling
            // below, which reads what they say.
            let free = std::mem::take(&mut self.left_free);
            self.settle_fresh(mark, &free);
            // Every edge, after anything that touched the model. An edge's
            // classification is a statement about its two ends, so it is
            // out of date the moment either of them is — and a cut makes
            // edges nobody has ever classified. Two table lookups apiece,
            // no geometry measured.
            self.settle_classification();
            // A mesh of a blocking that has since changed is a lie, and a
            // lie that looks right. The number goes up so that a client
            // holding the picture learns it has gone.
            self.drop_mesh();
        }
        Ok(outcome)
    }

    fn drop_mesh(&mut self) {
        if self.mesh.take().is_some() {
            self.mesh_generation += 1;
        }
    }

    /// The session as it stands, ready to be put back.
    fn capture(&self) -> Result<State, SessionError> {
        Ok(State {
            blocking: self.blocking.snapshot()?,
            geometry: self.geometry.clone(),
            composites: self.composites.clone(),
            groups: self.groups.clone(),
            origin: self.origin,
            journal: self.journal.len(),
        })
    }

    /// Put a captured state back, whole.
    ///
    /// Nothing is settled afterwards. The state was settled when it was
    /// taken, and running the classification over it again would be a
    /// chance for it to come out differently.
    /// The merges, as a thing to compare: what changes the picture of the
    /// geometry when it changes.
    fn merges_now(&self) -> Vec<((u8, i32), (u8, i32))> {
        self.composites.taken()
    }

    fn restore(&mut self, state: State) {
        let was = self.geometry.as_ref().map(Arc::as_ptr);
        let merges = self.merges_now();
        self.blocking = state.blocking;
        self.geometry = state.geometry;
        self.composites = state.composites;
        self.groups = state.groups;
        self.origin = state.origin;
        self.journal.truncate(state.journal);
        // Forwards even when the geometry goes backwards: the number says
        // "not the one you last drew", and a client that saw it fall would
        // have no reason to trust it the next time it rose.
        // A merge taken back changes what is drawn as much as a geometry
        // taken back does.
        if was != self.geometry.as_ref().map(Arc::as_ptr) || merges != self.merges_now() {
            self.geometry_generation += 1;
        }
    }

    fn push(&mut self, state: State) {
        self.past.push_back(state);
        while self.past.len() > self.steps {
            self.past.pop_front();
        }
    }

    /// Put the last state back.
    fn undo(&mut self) -> Result<Outcome, SessionError> {
        let Some(state) = self.past.pop_back() else {
            return Ok(Outcome {
                script: None,
                message: "nothing to undo".to_string(),
                view_changed: false,
            });
        };
        self.drop_mesh();
        self.restore(state);
        Ok(Outcome {
            script: None,
            message: format!(
                "undone — {} nodes, {} edges, {} faces, {} blocks, {} step{} back left",
                self.blocking.nb_nodes(),
                self.blocking.nb_edges(),
                self.blocking.nb_faces(),
                self.blocking.nb_blocks(),
                self.past.len(),
                plural(self.past.len()),
            ),
            view_changed: true,
        })
    }

    fn set_history(&mut self, steps: usize) -> Outcome {
        self.steps = steps.min(MAX_HISTORY_STEPS);
        while self.past.len() > self.steps {
            self.past.pop_front();
        }
        Outcome {
            script: None,
            message: match self.steps {
                0 => "undo is off".to_string(),
                1 => "one step of undo".to_string(),
                n => format!("{n} steps of undo"),
            },
            // Nothing in the scene moved. A client that redrew here would be
            // redrawing because a number in the drawer changed.
            view_changed: false,
        }
    }

    fn run(&mut self, command: Command) -> Result<Outcome, SessionError> {
        match command {
            Command::AddBlock { corners } => self.add(corners),
            Command::FaceOnSurface { surface } => {
                let Session {
                    blocking,
                    geometry,
                    composites,
                    ..
                } = self;
                let geometry = geometry.as_deref().ok_or(SessionError::NoGeometry)?;
                let quad = geometry.quad_on(surface)?;

                // The corners are already *at* the geometric corners, so
                // nothing has to be projected: the face is laid where the
                // model says, and then told what it is lying on.
                let face = blocking.add_face(quad.points)?;
                let (corners, edges) = blocking
                    .face_cells(face)
                    .expect("the face that was just made");
                for (node, corner) in corners.iter().zip(quad.corners) {
                    blocking.set_node_class(
                        *node,
                        Some(On {
                            dim: gecko_geom::CORNER,
                            tag: corner,
                        }),
                    )?;
                }
                // Side `k` runs from corner `k` to corner `k + 1`, which is
                // the order β₁ walks the face in and the order the points
                // were given in. A run of one curve is that curve; a run of
                // several is a side, named once and for all.
                let mut composed = 0;
                for (edge, side) in edges.iter().zip(&quad.sides) {
                    let tag = composites.group(gecko_geom::CURVE, side);
                    composed += usize::from(side.len() > 1);
                    blocking.set_edge_class(
                        *edge,
                        Some(On {
                            dim: gecko_geom::CURVE,
                            tag,
                        }),
                    )?;
                }
                blocking.set_face_class(
                    face,
                    Some(On {
                        dim: gecko_geom::SURFACE,
                        tag: surface,
                    }),
                )?;

                let squareness: Vec<String> = quad
                    .angles
                    .iter()
                    .map(|a| format!("{:.0}°", a.to_degrees()))
                    .collect();
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "face {face} laid on surface {surface} — corners {:?} at {}{}",
                        quad.corners,
                        squareness.join(", "),
                        match composed {
                            0 => String::new(),
                            1 => ", and one side runs along several curves".to_string(),
                            n => format!(", and {n} sides run along several curves"),
                        }
                    ),
                    view_changed: true,
                })
            }
            Command::AddFace {
                corners,
                on,
                surface,
            } => {
                let laid = self.blocking.lay_face(corners)?;

                // What each point was lying on, told to the corner it
                // became — in the same command, so that a quadrangle laid on
                // the model is one step of undo rather than two.
                let mut followed = 0;
                for (node, spot) in laid
                    .corners
                    .iter()
                    .zip(on.iter().chain(std::iter::repeat(&None)))
                {
                    let Some(spot) = spot else { continue };
                    self.blocking.set_node_class(
                        *node,
                        Some(On {
                            dim: spot.dim,
                            tag: spot.tag,
                        }),
                    )?;
                    followed += 1;
                }
                if let Some(tag) = surface {
                    self.blocking
                        .set_face_class(laid.face, Some(On { dim: 2, tag }))?;
                }

                let welded = match laid.welded {
                    0 => String::new(),
                    n => format!(", welded along {n} side{}", if n == 1 { "" } else { "s" }),
                };
                let on_surface = match surface {
                    Some(tag) => format!(" on surface {tag}"),
                    None => String::new(),
                };
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "face {} laid{on_surface} on {followed} point{}{welded} — \
                         {} nodes, {} edges, {} faces, {} patches",
                        laid.face,
                        if followed == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_patches(),
                    ),
                    view_changed: true,
                })
            }
            Command::AddSlab {
                corners,
                on,
                thickness,
            } => {
                // A share of the quadrangle's own shortest diagonal when
                // nobody says, so that "thin" means the same thing whatever
                // the model is measured in.
                let diagonal = |a: usize, b: usize| {
                    (0..3)
                        .map(|k| (corners[a][k] - corners[b][k]).powi(2))
                        .sum::<f64>()
                        .sqrt()
                };
                let thickness = thickness
                    .unwrap_or_else(|| diagonal(0, 2).min(diagonal(1, 3)) * gecko_core::SLAB_SHARE);
                let slab = self.blocking.add_slab(corners, thickness)?;

                // What each point was lying on, told to the corner it
                // became — in the same command, so that a slab laid on the
                // model is one step of undo rather than two.
                let mut followed = 0;
                for (node, spot) in slab
                    .outer
                    .iter()
                    .zip(on.iter().chain(std::iter::repeat(&None)))
                {
                    let Some(spot) = spot else { continue };
                    self.blocking.set_node_class(
                        *node,
                        Some(On {
                            dim: spot.dim,
                            tag: spot.tag,
                        }),
                    )?;
                    followed += 1;
                }
                // And the four inside the material follow nothing, which is
                // a decision and not an omission: said out loud, so that the
                // settling below leaves them where they were put.
                self.left_free = slab.inner.to_vec();

                let block = slab.block;
                let welded = match slab.welded {
                    0 => String::new(),
                    n => format!(", welded along {n} side{}", if n == 1 { "" } else { "s" }),
                };
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "block {block} laid on {followed} point{} — {} thick{welded}, \
                         {} nodes, {} edges, {} faces, {} blocks",
                        if followed == 1 { "" } else { "s" },
                        format_args!("{thickness:.3}"),
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::CreateCube { origin, size } => {
                // `size <= 0.0` alone would let NaN through, since every
                // comparison with it is false.
                if size <= 0.0 || !size.is_finite() {
                    return Err(SessionError::BadSize(size));
                }
                self.add(cube_corners(origin, size))
            }
            Command::MoveNode {
                node,
                to,
                carrying,
                snap,
            } => {
                let node = CellId::from_raw(node);
                // The offset is read before anything moves, and from where
                // the node **is** rather than where it ends up: the dragged
                // node may be on a curve and so land short of the point
                // asked for, and the others would then all move by that
                // shortfall rather than by the gesture.
                let was = self
                    .blocking
                    .position(node)
                    .ok_or(BlockingError::NoSuchNode(node))?;
                let by = [to[0] - was[0], to[1] - was[1], to[2] - was[2]];

                let others = if carrying.is_empty() {
                    Vec::new()
                } else {
                    self.named(&carrying)?
                };
                let landed = self.move_node(node, to, snap)?;

                // Frozen ones are left where they are rather than refusing
                // the gesture: not moving is what frozen *means*, and the
                // other nodes have nothing to do with it. The one being
                // dragged is a different matter — it is refused, because
                // you grabbed it and are owed an answer.
                let mut moved = 1;
                let mut held = 0;
                for other in others.iter().filter(|o| **o != node) {
                    if self.blocking.is_frozen(*other) {
                        held += 1;
                        continue;
                    }
                    let Some(at) = self.blocking.position(*other) else {
                        continue;
                    };
                    self.move_node(*other, [at[0] + by[0], at[1] + by[1], at[2] + by[2]], snap)?;
                    moved += 1;
                }

                Ok(Outcome {
                    script: None,
                    message: if moved + held > 1 {
                        format!(
                            "{moved} node{} moved{}",
                            plural(moved),
                            match held {
                                0 => String::new(),
                                n => format!(", {n} frozen and left"),
                            }
                        )
                    } else {
                        match self.blocking.node_class(node) {
                            Some(on) => {
                                format!("node {node} moved along {}", name_in(on, &self.composites))
                            }
                            None => format!("node {node} moved to {landed:?}"),
                        }
                    },
                    view_changed: true,
                })
            }
            Command::RemoveBlock { block } => {
                let block = CellId::from_raw(block);
                self.blocking.remove_block(block)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "block {block} removed — {} nodes, {} edges, {} faces, {} blocks",
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::RemoveFace { face } => {
                let face = CellId::from_raw(face);
                self.blocking.remove_face(face)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "face {face} removed — {} nodes, {} edges, {} faces, {} patches",
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_patches(),
                    ),
                    view_changed: true,
                })
            }
            Command::RemoveSheet { edge, close } => {
                let edge = CellId::from_raw(edge);
                if close {
                    self.blocking.remove_sheet(edge)?;
                } else {
                    self.blocking.remove_sheet_leaving_a_gap(edge)?;
                }
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "sheet through {edge} removed{} — \
                         {} nodes, {} edges, {} faces, {} blocks",
                        if close { "" } else { ", gap left open" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::RemoveChord { face, block } => {
                let (face, block) = (CellId::from_raw(face), CellId::from_raw(block));
                let gone = self.blocking.remove_chord(face, block)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "chord through face {face} removed, {} block{} — \
                         {} nodes, {} edges, {} faces, {} blocks",
                        gone.len(),
                        if gone.len() == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::PillowChord {
                face,
                block,
                tolerance,
            } => {
                let (face, block) = (CellId::from_raw(face), CellId::from_raw(block));
                let along = self.blocking.chord_through(face, block)?.len();
                let pulled = self.blocking.pillow_chord_traced(face, block)?;
                // As for a pillow: the nodes that went in slide on what
                // their neighbours are on, or the o-grid is refused.
                let _ = tolerance;
                self.settle_pulled(&pulled)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "o-grid along {along} block{} — {} nodes, {} edges, {} faces, {} blocks",
                        if along == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::SplitSheet { edge, at } => {
                let edge = CellId::from_raw(edge);
                // Each new node has inherited what the edge it came from
                // was classified on, and is sitting on the straight line
                // between that edge's ends. Putting it where its entity
                // says it is is this crate's business, holding both.
                let made = self.blocking.split_sheet(edge, at)?;
                for node in made {
                    self.pull_onto_class(node)?;
                }
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "sheet through {edge} cut at {at:.3} — {} nodes, {} edges, {} faces, {} blocks",
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::RemoveBlocks { blocks } => {
                if blocks.is_empty() {
                    return Err(BlockingError::EmptyPatch.into());
                }
                for block in &blocks {
                    self.blocking.remove_block(CellId::from_raw(*block))?;
                }
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} block{} removed — {} nodes, {} edges, {} faces, {} blocks",
                        blocks.len(),
                        if blocks.len() == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::PillowBlocks { blocks, outer } => {
                let ids: Vec<CellId> = blocks.iter().map(|b| CellId::from_raw(*b)).collect();
                let patch = self.blocking.block_set_surface(&ids, outer)?;
                self.pillow_patch(&patch)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "pillowed round {} block{}, along {} face{} — \
                         {} nodes, {} edges, {} faces, {} blocks",
                        ids.len(),
                        if ids.len() == 1 { "" } else { "s" },
                        patch.len(),
                        if patch.len() == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::Pillow { faces, tolerance } => {
                let given: Vec<_> = faces
                    .iter()
                    .map(|f| (CellId::from_raw(f.face), f.block.map(CellId::from_raw)))
                    .collect();
                let patch = self.blocking.pillow_sides(&given)?;
                let _ = tolerance;
                self.pillow_patch(&patch)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "pillowed {} face{} — {} nodes, {} edges, {} faces, {} blocks",
                        faces.len(),
                        if faces.len() == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::ExtrudeFace { faces, height } => {
                let picked: Vec<(CellId, CellId)> = faces
                    .iter()
                    .map(|f| (CellId::from_raw(f.face), CellId::from_raw(f.block)))
                    .collect();
                let grown = self.blocking.extrude_faces(&picked, height)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} block{} grown by {height:.3} — \
                         {} nodes, {} edges, {} faces, {} blocks",
                        grown.len(),
                        if grown.len() == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::ExtrudePatch { face, height } => {
                let face = CellId::from_raw(face);
                let raised = self.blocking.extrude_patch(face, height)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} block{} raised from the patch by {height:.3} — \
                         {} nodes, {} edges, {} faces, {} blocks",
                        raised.len(),
                        if raised.len() == 1 { "" } else { "s" },
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::Smooth { rounds, share } => {
                let Session {
                    blocking,
                    geometry,
                    composites,
                    ..
                } = self;
                let model = geometry.as_deref().map(|g| Model::new(g, composites));
                let furthest = blocking.smooth(
                    Smoothing {
                        rounds,
                        share,
                        held: &[],
                    },
                    |on, at| model.and_then(|m| m.project(on.dim, on.tag, at)),
                );
                Ok(Outcome {
                    script: None,
                    message: if furthest == 0.0 {
                        "nothing moved — every node is held, follows a point, or is on the \
                         boundary following nothing"
                            .to_string()
                    } else {
                        format!(
                            "smoothed {rounds} time{} by {:.0}% — the furthest any node went was {furthest:.4}",
                            if rounds == 1 { "" } else { "s" },
                            share * 100.0,
                        )
                    },
                    view_changed: true,
                })
            }
            Command::LoadGeometry { msh, name } => {
                let geometry = Geometry::read(&msh)?;
                let what = if name.is_empty() {
                    "geometry".to_string()
                } else {
                    name
                };
                let message = format!(
                    "{what} loaded — {} corner{}, {} curve{}, {} surface{} ({} triangles){}",
                    geometry.corners().len(),
                    plural(geometry.corners().len()),
                    geometry.curves().len(),
                    plural(geometry.curves().len()),
                    geometry.surfaces().len(),
                    plural(geometry.surfaces().len()),
                    geometry.nb_triangles(),
                    match geometry.skipped() {
                        0 => String::new(),
                        n => format!(", {n} element{} of other kinds ignored", plural(n)),
                    },
                );
                self.geometry = Some(Arc::new(geometry));
                self.geometry_generation += 1;
                // The view changes too, even though not one block moved: the
                // rebasing origin is settled over the geometry as well, and a
                // geometry far from the blocks moves it.
                Ok(Outcome {
                    script: None,
                    message,
                    view_changed: true,
                })
            }
            Command::Classify { tolerance } => {
                self.tolerance = tolerance;
                let (nodes, edges, faces) = self.classify(tolerance)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "classified {nodes} node{}, {edges} edge{} and {faces} face{} \
                         onto the geometry",
                        plural(nodes),
                        plural(edges),
                        plural(faces),
                    ),
                    view_changed: true,
                })
            }
            Command::ClassifyNode { nodes, on } => {
                let on = match on {
                    Some(place) => Some(self.place(&place, gecko_geom::CORNER)?),
                    None => None,
                };
                // Every name checked before anything moves. Half a gesture
                // carried out is worse than none of it: the undo would put
                // back a state nobody asked for either.
                let names = self.named(&nodes)?;
                for node in &names {
                    self.blocking.set_node_class(*node, on)?;
                    // Put it where it now says it is, unless it is frozen,
                    // in which case nobody moves it — that is what frozen
                    // means.
                    if !self.blocking.is_frozen(*node) {
                        self.pull_onto_class(*node)?;
                    }
                }
                Ok(Outcome {
                    script: None,
                    message: match on {
                        Some(on) => {
                            format!("{} put on {}", these(&names), name_in(on, &self.composites))
                        }
                        None => format!("{} on nothing now", these(&names)),
                    },
                    view_changed: true,
                })
            }
            Command::ClassifyEdge { edges, on: None } => {
                let names = cells(&edges, "edge")?;
                for edge in &names {
                    self.blocking.set_edge_free(*edge)?;
                }
                Ok(Outcome {
                    script: None,
                    message: format!("{} on nothing now", listed(&names, "edge")),
                    view_changed: true,
                })
            }
            Command::ClassifyFace {
                faces, on: None, ..
            } => {
                let names = cells(&faces, "face")?;
                for face in &names {
                    self.blocking.set_face_free(*face)?;
                }
                Ok(Outcome {
                    script: None,
                    message: format!("{} on nothing now", listed(&names, "face")),
                    view_changed: true,
                })
            }
            Command::ClassifyEdge {
                edges,
                on: Some(on),
            } => {
                let on = self.place(&on, gecko_geom::CURVE)?;
                let names = cells(&edges, "edge")?;
                for edge in &names {
                    self.blocking.set_edge_class(*edge, Some(on))?;
                }
                self.settle_classification();
                for edge in &names {
                    if self.blocking.edge_class(*edge) != Some(on) {
                        let ends: Vec<String> = self
                            .blocking
                            .edges()
                            .find(|e| e.0 == *edge)
                            .map(|(_, a, b, _)| [a, b].map(|n| self.class_of_node(n)).to_vec())
                            .unwrap_or_default();
                        return Err(SessionError::Unplaceable(format!(
                            "edge {edge} cannot stay on {}: its ends are on {}",
                            name_in(on, &self.composites),
                            ends.join(" and ")
                        )));
                    }
                }
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} put on {}",
                        listed(&names, "edge"),
                        name_in(on, &self.composites)
                    ),
                    view_changed: true,
                })
            }
            Command::ClassifyFace {
                faces,
                on: Some(on),
                down,
            } => {
                let on = self.place(&on, gecko_geom::SURFACE)?;
                let names = cells(&faces, "face")?;
                for face in &names {
                    if down {
                        self.lay_down(*face, on)?;
                    }
                    self.blocking.set_face_class(*face, Some(on))?;
                }
                self.settle_classification();
                for face in &names {
                    if self.blocking.face_class(*face) != Some(on) {
                        let sides: Vec<String> = self
                            .blocking
                            .faces()
                            .find(|f| f.0 == *face)
                            .map(|(_, edges)| {
                                edges
                                    .iter()
                                    .map(|e| match self.blocking.edge_class(*e) {
                                        Some(c) => name_in(c, &self.composites),
                                        None => "nothing".to_string(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        return Err(SessionError::Unplaceable(format!(
                            "face {face} cannot stay on {}: its edges are on {}",
                            name_in(on, &self.composites),
                            sides.join(", ")
                        )));
                    }
                }
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} put on {}",
                        listed(&names, "face"),
                        name_in(on, &self.composites)
                    ),
                    view_changed: true,
                })
            }
            Command::Merge { dim, tags } => {
                let geometry = self.geometry.clone().ok_or(SessionError::NoGeometry)?;
                let tag = self.composites.merge(&geometry, dim, &tags)?;
                self.follow_merges()?;
                // Drawn and picked as one now: the picture is another one.
                self.geometry_generation += 1;
                Ok(Outcome {
                    script: None,
                    message: format!("merged into {}", self.merge_said(tag)),
                    view_changed: true,
                })
            }
            Command::MergeSmooth { angle } => {
                let geometry = self.geometry.clone().ok_or(SessionError::NoGeometry)?;
                let found = self.composites.merge_smooth(&geometry, angle);
                self.follow_merges()?;
                if !found.merges.is_empty() || !found.seams.is_empty() {
                    self.geometry_generation += 1;
                }
                let mut made: Vec<String> =
                    found.merges.iter().map(|m| self.merge_said(*m)).collect();
                // A seam is not a merge of two things but the cut down one,
                // gone: said as what it went into, with any corner that
                // went with it onto what is left through it.
                for (curve, surface) in &found.seams {
                    made.push(format!(
                        "curve {curve} into {}",
                        name_in(
                            On {
                                dim: gecko_geom::SURFACE,
                                tag: self.composites.image(gecko_geom::SURFACE, *surface).1,
                            },
                            &self.composites
                        )
                    ));
                }
                for ((dim, tag), by) in self.composites.taken() {
                    if dim == gecko_geom::CORNER && !self.composites.is_merged(by.1) {
                        made.push(format!(
                            "corner {tag} into {}",
                            name_in(
                                On {
                                    dim: by.0,
                                    tag: by.1
                                },
                                &self.composites
                            )
                        ));
                    }
                }
                // The nearest miss, so that a join that was nearly smooth
                // enough is a number to adjust rather than a mystery.
                let miss = found
                    .joins
                    .iter()
                    .filter(|j| !j.merged)
                    .min_by(|a, b| a.degrees.total_cmp(&b.degrees))
                    .map(|j| {
                        format!(
                            "; the nearest miss is {} at {}, {:.1}° off",
                            match j.dim {
                                gecko_geom::CURVE =>
                                    format!("curves {} and {}", j.pair.0, j.pair.1),
                                _ if j.pair.0 == j.pair.1 =>
                                    format!("surface {} with itself", j.pair.0),
                                _ => format!("surfaces {} and {}", j.pair.0, j.pair.1),
                            },
                            name_of(On {
                                dim: j.across.0,
                                tag: j.across.1
                            }),
                            j.degrees
                        )
                    })
                    .unwrap_or_default();
                Ok(Outcome {
                    script: None,
                    message: if made.is_empty() {
                        format!("nothing meets within {angle}° of smooth{miss}")
                    } else {
                        format!("merged {}{miss}", made.join(", and "))
                    },
                    view_changed: true,
                })
            }
            Command::FreezeNode { nodes, frozen } => {
                let names = self.named(&nodes)?;
                for node in &names {
                    self.blocking.set_frozen(*node, frozen)?;
                }
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} {}",
                        these(&names),
                        if frozen { "frozen" } else { "thawed" }
                    ),
                    view_changed: true,
                })
            }
            Command::BoxGeometry { surfaces, margin } => {
                let geometry = self.geometry.as_deref().ok_or(SessionError::NoGeometry)?;
                let corners = geometry.bounding_box(&surfaces, margin)?;
                let id = self.blocking.add_block(corners)?;
                let round = if surfaces.is_empty() {
                    "the whole geometry".to_string()
                } else {
                    format!("{} surface{}", surfaces.len(), plural(surfaces.len()))
                };
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "block {id} boxes {round} — \
                         {} nodes, {} edges, {} faces, {} blocks",
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::Group {
                name,
                blocks,
                written,
            } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err(GroupError::Unnamed.into());
                }
                // Every name checked before anything is written: a group
                // half filled is worse than one not made, since the undo
                // would put back a state nobody asked for.
                let named = self.named_blocks(&blocks)?;
                let bit = self
                    .groups
                    .make(&name, written.unwrap_or(true))
                    .ok_or(GroupError::Full)?
                    .bit;
                if let Some(written) = written {
                    self.groups.write_out(&name, written)?;
                }
                self.blocking.set_in_group(&named, bit, true)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} block{} in {name:?} — {} in all{}",
                        named.len(),
                        plural(named.len()),
                        self.in_group(bit),
                        if self.groups.by_name(&name).is_some_and(|g| g.written) {
                            ""
                        } else {
                            ", not written into the mesh"
                        },
                    ),
                    view_changed: true,
                })
            }
            Command::Ungroup { name, blocks } => {
                let bit = self
                    .groups
                    .by_name(&name)
                    .ok_or_else(|| GroupError::NoSuch(name.clone()))?
                    .bit;
                let named = self.named_blocks(&blocks)?;
                self.blocking.set_in_group(&named, bit, false)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} block{} out of {name:?} — {} left",
                        named.len(),
                        plural(named.len()),
                        self.in_group(bit),
                    ),
                    view_changed: true,
                })
            }
            Command::RenameGroup { name, to } => {
                let to = to.trim().to_string();
                if to.is_empty() {
                    return Err(GroupError::Unnamed.into());
                }
                self.groups.rename(&name, &to)?;
                Ok(Outcome {
                    script: None,
                    message: format!("{name:?} is now {to:?}"),
                    // Nothing moved, but what the scene is colored and
                    // filtered by has a different name on it.
                    view_changed: true,
                })
            }
            Command::DropGroup { name } => {
                let bit = self.groups.drop(&name)?;
                self.blocking.drop_group(bit);
                Ok(Outcome {
                    script: None,
                    message: format!("{name:?} is gone — the blocks that were in it are not"),
                    view_changed: true,
                })
            }
            Command::Reset => {
                self.blocking = Blocking::new()?;
                // The composites go with the geometry they name: their tags
                // are indices into their own list, and a list kept across a
                // new geometry would hand out the old model's entities
                // under the new one's names.
                let had = self.geometry.take().is_some();
                self.composites = Composites::new();
                self.groups = Groups::new();
                self.drop_mesh();
                if had {
                    self.geometry_generation += 1;
                }
                Ok(Outcome {
                    script: None,
                    message: "the session is empty".to_string(),
                    view_changed: true,
                })
            }
            Command::ClearGeometry => {
                self.geometry = None;
                self.geometry_generation += 1;
                Ok(Outcome {
                    script: None,
                    message: "geometry cleared".to_string(),
                    view_changed: true,
                })
            }
            Command::SetSegments { edge, segments } => {
                let edge = CellId::from_raw(edge);
                let segments = segments.clamp(1, MAX_SEGMENTS);
                self.blocking.set_sheet_segments(edge, segments)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "the sheet through {edge} is in {segments} segment{}",
                        plural(segments as usize)
                    ),
                    // The blocking has not moved; what changed is a number
                    // the next mesh will read.
                    view_changed: false,
                })
            }
            Command::Mesh { segments } => {
                let segments = segments.clamp(1, MAX_SEGMENTS);
                let mesh = {
                    let Session {
                        blocking,
                        geometry,
                        composites,
                        ..
                    } = self;
                    let model = geometry.as_deref().map(|g| Model::new(g, composites));
                    blocking.mesh(segments, |on, at| {
                        model.and_then(|m| m.project(on.dim, on.tag, at))
                    })
                };
                let message = format!("meshed — {}; {}", vtk::count(&mesh), quality_line(&mesh));
                self.mesh = Some(mesh);
                self.mesh_generation += 1;
                Ok(Outcome {
                    script: None,
                    message,
                    view_changed: false,
                })
            }
            Command::ClearMesh => {
                self.drop_mesh();
                Ok(Outcome {
                    script: None,
                    message: "mesh cleared".to_string(),
                    view_changed: false,
                })
            }
            // Caught in `apply`, which has to deal with them before it
            // takes the state that an ordinary command is undone back to.
            // Not a panic: a kernel inside a browser tab has nowhere good
            // to panic to.
            Command::Undo | Command::SetHistory { .. } => Err(SessionError::BadCommand(
                "undo and set_history are handled before this".to_string(),
            )),
            Command::GlueFaces { first, second } => {
                let (first, second) = (CellId::from_raw(first), CellId::from_raw(second));
                // The geometry has to be lent to the glue rather than asked
                // about afterwards: the corners meet in the middle, and a
                // welded corner has to land on whatever the two of them
                // leave it classified on, which only the triangles know.
                let Session {
                    blocking,
                    geometry,
                    composites,
                    ..
                } = self;
                let model = geometry.as_deref().map(|g| Model::new(g, composites));
                let unmet = blocking.glue_faces(
                    first,
                    second,
                    |a, b, near| {
                        model.and_then(|m| {
                            m.meet((a.dim, a.tag), (b.dim, b.tag), near)
                                .map(|(dim, tag)| On { dim, tag })
                        })
                    },
                    |on, at| model.and_then(|m| m.project(on.dim, on.tag, at)),
                )?;
                let dropped = unmet
                    .iter()
                    .map(|u| {
                        format!(
                            "; node {} kept {} and left {}",
                            u.node,
                            name_in(u.kept, &self.composites),
                            name_in(u.dropped, &self.composites)
                        )
                    })
                    .collect::<String>();
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "faces {first} and {second} glued — {} nodes, {} edges, {} faces, {} blocks{dropped}",
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
        }
    }

    /// The names a command gave, checked against the model before any of
    /// them is acted on.
    ///
    /// An empty list is refused rather than quietly doing nothing: it means
    /// the interface sent a gesture with nothing selected, and a command
    /// that changed nothing would still cost a step of undo.
    /// The same for **blocks**, and an empty list is not an error.
    ///
    /// Its own function rather than [`named`][Self::named]: that one looks
    /// up nodes, and a block's name and a node's are both small numbers —
    /// so passing blocks to it checks that a node of that number exists,
    /// which is no check at all. And naming no blocks is a thing somebody
    /// does on purpose: making a group before filling it, or saying
    /// whether an existing one reaches the mesh.
    fn named_blocks(&self, blocks: &[u32]) -> Result<Vec<CellId>, SessionError> {
        let names: Vec<CellId> = blocks.iter().copied().map(CellId::from_raw).collect();
        for block in &names {
            if !self.blocking.is_block(*block) {
                return Err(BlockingError::NoSuchBlock(*block).into());
            }
        }
        Ok(names)
    }

    /// What a [`Place`] names, checked against the model: every tag an
    /// entity of that dimension, and several made one composite.
    ///
    /// `at_least` is the lowest dimension the cell may go on — an edge is
    /// never pinned to a point, nor a face to a curve.
    fn place(&mut self, place: &Place, at_least: u8) -> Result<On, SessionError> {
        let geometry = self.geometry.as_deref().ok_or(SessionError::NoGeometry)?;
        let dim = place.dim;
        if dim > gecko_geom::SURFACE || dim < at_least {
            return Err(SessionError::Unplaceable(format!(
                "nothing of dimension {dim} can be put there — {}",
                ["a corner", "a curve", "a surface"][at_least as usize].to_string() + " at least"
            )));
        }
        let tags = place.tag.list();
        if tags.is_empty() {
            return Err(SessionError::BadCommand("no entity named".to_string()));
        }
        for &tag in &tags {
            // A merge is an entity of the model and may be named by its
            // tag — it is what a click on it answers. Any other composite
            // is named by its members, never by its number, which is an
            // accident of the order they were made in.
            if self.composites.is_merged(tag)
                && self.composites.members(tag).is_some_and(|(d, _)| d == dim)
            {
                continue;
            }
            if tag <= 0 || geometry.containing(dim, tag).is_empty() {
                return Err(SessionError::NoSuchEntity(name_of(On { dim, tag })));
            }
        }
        if dim == gecko_geom::CORNER && tags.len() > 1 {
            return Err(SessionError::Unplaceable(
                "a node is on one corner or another, not on several".to_string(),
            ));
        }
        // A piece of a merge means the merge: it is what the model has
        // now, and a cell put on one of its pieces would be put on what
        // the merge was made to get rid of.
        let images: BTreeSet<(u8, i32)> = tags
            .iter()
            .map(|&t| self.composites.image(dim, t))
            .collect();
        if images.len() == 1 {
            let (dim, tag) = *images.iter().next().expect("one");
            if dim < at_least {
                return Err(SessionError::Unplaceable(format!(
                    "{} is no lower than {}",
                    name_in(On { dim, tag }, &self.composites),
                    ["a corner", "a curve", "a surface"][at_least as usize]
                )));
            }
            return Ok(On { dim, tag });
        }
        let mut members: Vec<i32> = Vec::new();
        for (d, t) in images {
            if d != dim {
                return Err(SessionError::Unplaceable(format!(
                    "{} is inside {} now",
                    name_of(On { dim, tag: tags[0] }),
                    name_in(On { dim: d, tag: t }, &self.composites)
                )));
            }
            match self.composites.members(t) {
                Some((_, inside)) => members.extend_from_slice(inside),
                None => members.push(t),
            }
        }
        let tag = self.composites.group(dim, &members);
        Ok(On { dim, tag })
    }

    /// Put a face's corners and sides on the rim of what it is to cover.
    ///
    /// The rim is every curve of the model on which exactly one of the
    /// target's surfaces lies — a seam between two of them is inside it,
    /// not on its rim. A corner of the face goes to the nearest point of
    /// the rim, and is on a corner of the rim if one is within the snap
    /// tolerance of that point, or else on the curve. A side is on the
    /// curve its two ends share, or on the two curves meeting at a corner
    /// of the rim that its ends are on, taken as one; on the target itself
    /// when neither. A target with no rim, a closed surface, takes the
    /// corners itself.
    fn lay_down(&mut self, face: CellId, on: On) -> Result<(), SessionError> {
        let geometry = self.geometry.clone().ok_or(SessionError::NoGeometry)?;
        let (nodes, edges) = self
            .blocking
            .face_cells(face)
            .ok_or(BlockingError::NoSuchFace(face))?;
        let pieces = |composites: &Composites, tag: i32| -> Vec<i32> {
            composites
                .members(tag)
                .map(|(_, m)| m.to_vec())
                .unwrap_or_else(|| vec![tag])
        };
        let target = pieces(&self.composites, on.tag);
        let reach = self.tolerance.max(0.0) * geometry.span();

        /// Where a side of the face goes.
        enum Side {
            Curve(i32),
            Pair(Vec<i32>),
            Across,
        }

        let (placed, sides) = {
            let model = Model::new(&geometry, &self.composites);
            let rim: Vec<i32> = model
                .visible(gecko_geom::CURVE)
                .into_iter()
                .filter(|&c| {
                    pieces(&self.composites, c).iter().all(|&p| {
                        geometry
                            .containing(gecko_geom::CURVE, p)
                            .iter()
                            .filter(|e| e.0 == gecko_geom::SURFACE && target.contains(&e.1))
                            .count()
                            == 1
                    })
                })
                .collect();
            let corners: Vec<i32> = model
                .visible(gecko_geom::CORNER)
                .into_iter()
                .filter(|&p| {
                    let up = model.containing(gecko_geom::CORNER, p);
                    rim.iter().any(|r| up.contains(&(gecko_geom::CURVE, *r)))
                })
                .collect();

            let mut placed: Vec<(CellId, On, [f64; 3])> = Vec::new();
            for node in nodes {
                let at = self
                    .blocking
                    .position(node)
                    .ok_or(BlockingError::NoSuchNode(node))?;
                let landed = rim
                    .iter()
                    .filter_map(|&r| {
                        let q = model.project(gecko_geom::CURVE, r, at)?;
                        Some((apart(q, at), r, q))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                let (class, to) = match landed {
                    Some((_, curve, q)) => corners
                        .iter()
                        .filter_map(|&p| {
                            let c = model.project(gecko_geom::CORNER, p, q)?;
                            Some((apart(c, q), p, c))
                        })
                        .filter(|(d, _, _)| *d <= reach)
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(_, p, c)| {
                            (
                                On {
                                    dim: gecko_geom::CORNER,
                                    tag: p,
                                },
                                c,
                            )
                        })
                        .unwrap_or((
                            On {
                                dim: gecko_geom::CURVE,
                                tag: curve,
                            },
                            q,
                        )),
                    None => (on, model.project(on.dim, on.tag, at).unwrap_or(at)),
                };
                placed.push((node, class, to));
            }

            // The curves of the rim a node is on: its own, or those through
            // its corner.
            let along = |class: On| -> Vec<i32> {
                match class.dim {
                    gecko_geom::CURVE => vec![class.tag],
                    gecko_geom::CORNER => {
                        let up = model.containing(gecko_geom::CORNER, class.tag);
                        rim.iter()
                            .copied()
                            .filter(|r| up.contains(&(gecko_geom::CURVE, *r)))
                            .collect()
                    }
                    _ => Vec::new(),
                }
            };
            let class_of = |n: CellId| placed.iter().find(|p| p.0 == n).map(|p| p.1);
            let mut sides: Vec<(CellId, Side)> = Vec::new();
            for edge in edges {
                let (a, b) = self
                    .blocking
                    .edge_ends(edge)
                    .ok_or(BlockingError::NoSuchEdge(edge))?;
                let (Some(ca), Some(cb)) = (class_of(a), class_of(b)) else {
                    continue;
                };
                let (la, lb) = (along(ca), along(cb));
                let side = if let Some(c) = la.iter().find(|c| lb.contains(c)) {
                    Side::Curve(*c)
                } else if let Some((x, y)) = la
                    .iter()
                    .flat_map(|x| lb.iter().map(move |y| (*x, *y)))
                    .find(|(x, y)| {
                        corners.iter().any(|p| {
                            let up = model.containing(gecko_geom::CORNER, *p);
                            up.contains(&(gecko_geom::CURVE, *x))
                                && up.contains(&(gecko_geom::CURVE, *y))
                        })
                    })
                {
                    let mut both = pieces(&self.composites, x);
                    both.extend(pieces(&self.composites, y));
                    Side::Pair(both)
                } else {
                    Side::Across
                };
                sides.push((edge, side));
            }
            (placed, sides)
        };

        for (node, class, to) in placed {
            // A frozen node stays where it is and as it is: that is what
            // frozen means, and the check after this says if the face
            // cannot then be what it was told.
            if !self.blocking.is_frozen(node) {
                self.blocking.set_node_class(node, Some(class))?;
                self.blocking.set_position(node, to)?;
            }
        }
        for (edge, side) in sides {
            let class = match side {
                Side::Curve(tag) => On {
                    dim: gecko_geom::CURVE,
                    tag,
                },
                Side::Pair(both) => On {
                    dim: gecko_geom::CURVE,
                    tag: self.composites.group(gecko_geom::CURVE, &both),
                },
                Side::Across => on,
            };
            self.blocking.set_edge_class(edge, Some(class))?;
        }
        Ok(())
    }

    /// Every cell on something a merge has taken, onto what stands in for
    /// it: a node on the corner between two curves merged is on the merged
    /// curve now, an edge on the seam between two surfaces merged on the
    /// merged surface. Nothing moves — what took them contains them.
    fn follow_merges(&mut self) -> Result<(), SessionError> {
        let image = |on: On, composites: &Composites| {
            composites
                .stand_in(on.dim, on.tag)
                .map(|(dim, tag)| On { dim, tag })
        };
        let nodes: Vec<(CellId, On)> = self
            .blocking
            .nodes_in_full()
            .filter_map(|(n, _, on, _)| Some((n, image(on?, &self.composites)?)))
            .collect();
        for (n, on) in nodes {
            self.blocking.set_node_class(n, Some(on))?;
        }
        let edges: Vec<(CellId, On)> = self
            .blocking
            .edges()
            .filter_map(|(e, _, _, on)| Some((e, image(on?, &self.composites)?)))
            .collect();
        for (e, on) in edges {
            self.blocking.set_edge_class(e, Some(on))?;
        }
        let faces: Vec<(CellId, On)> = self
            .blocking
            .faces()
            .filter_map(|(f, _)| Some((f, image(self.blocking.face_class(f)?, &self.composites)?)))
            .collect();
        for (f, on) in faces {
            self.blocking.set_face_class(f, Some(on))?;
        }
        Ok(())
    }

    /// A merge as a message says it: what it is, what it is made of, and
    /// what it swallowed.
    fn merge_said(&self, tag: i32) -> String {
        let on = match self.composites.members(tag) {
            Some((dim, _)) => On { dim, tag },
            None => return format!("entity {tag}"),
        };
        let swallowed: Vec<String> = self
            .composites
            .swallowed(tag)
            .into_iter()
            .map(|(dim, tag)| name_of(On { dim, tag }))
            .collect();
        format!(
            "{}{}",
            name_in(on, &self.composites),
            if swallowed.is_empty() {
                String::new()
            } else {
                format!(", across {}", swallowed.join(", "))
            }
        )
    }

    /// What a node is on, as a message says it.
    fn class_of_node(&self, node: CellId) -> String {
        match self.blocking.node_class(node) {
            Some(on) => name_in(on, &self.composites),
            None => "nothing".to_string(),
        }
    }

    fn named(&self, nodes: &[u32]) -> Result<Vec<CellId>, SessionError> {
        if nodes.is_empty() {
            return Err(SessionError::BadCommand("no node named".to_string()));
        }
        let names: Vec<CellId> = nodes.iter().copied().map(CellId::from_raw).collect();
        for node in &names {
            if self.blocking.position(*node).is_none() {
                return Err(BlockingError::NoSuchNode(*node).into());
            }
        }
        Ok(names)
    }

    fn add(&mut self, corners: [[f64; 3]; 8]) -> Result<Outcome, SessionError> {
        let id = self.blocking.add_block(corners)?;
        Ok(Outcome {
            script: None,
            message: format!(
                "block {} added — {} nodes, {} edges, {} faces, {} blocks",
                id,
                self.blocking.nb_nodes(),
                self.blocking.nb_edges(),
                self.blocking.nb_faces(),
                self.blocking.nb_blocks(),
            ),
            view_changed: true,
        })
    }

    /// Classify everything, and put it where it now says it is.
    ///
    /// Nodes by **proximity**, dimension by dimension, stopping at the
    /// lowest dimension with anything in range; edges by **inference**,
    /// from what their two ends agree about. Never the other way round:
    /// measuring one point of an edge can land it on a curve neither of
    /// its endpoints touches, and a cell at odds with its own boundary is
    /// worse than one classified on nothing.
    ///
    /// Frozen nodes are left exactly as they are, classification included.
    /// A reclassification would be followed by a projection, and a
    /// projection is a move.
    ///
    /// Returns how many of each ended up on something.
    fn classify(&mut self, tolerance: f64) -> Result<(usize, usize, usize), SessionError> {
        {
            // Borrowed field by field: the block structure is being
            // written to while the geometry answers questions about it,
            // and they are two fields rather than one object.
            let Session {
                blocking,
                geometry,
                composites,
                ..
            } = self;
            let geometry = geometry.as_deref().ok_or(SessionError::NoGeometry)?;
            // Everything the geometry is asked goes through the model, so
            // that a side answers like any other entity.
            let model = Model::new(geometry, composites);
            let reach = [tolerance.max(0.0) * model.span(); 3];
            blocking.classify_nodes(
                // What a node already claims is ignored here on purpose:
                // this is the command that decides the whole blocking
                // again, and a confirmation of what it already said would
                // be no answer at all.
                |_, at| model.nearest(at, reach).map(|(dim, tag)| On { dim, tag }),
                |on, at| model.project(on.dim, on.tag, at),
            );
        }
        // A fresh start, not a confirmation: the settling keeps a
        // classification its boundary still allows, which is right while
        // editing and wrong when the whole point is to decide again.
        self.blocking.unclassify_cells();
        // Here as well as in `apply`, because the count below is of what
        // this pass achieved and the caller's message says so.
        self.settle_classification();

        let seen = self.blocking.census();
        Ok((
            seen.classified_nodes,
            seen.classified_edges,
            seen.classified_faces,
        ))
    }

    /// How many blocks are in the group with this bit.
    fn in_group(&self, bit: u8) -> usize {
        // One walk of the volumes, not one per block: `block_groups` finds
        // its block by name, which is itself a walk.
        self.blocking.counted_in_groups()[bit as usize]
    }

    /// Put the nodes this command **made** onto the geometry, when they
    /// are near enough to it.
    ///
    /// Only the ones it made. A node that was there before keeps whatever
    /// it had, including nothing: somebody who took a node off the model
    /// is not overruled by the next cut. New-ness is read off the corner
    /// key rather than off the name, because an operation that takes a
    /// block out renames nodes it did not create.
    ///
    /// Why this exists: a cut inherits what the edge it cut was
    /// classified on, which is exact and costs nothing — but a node made
    /// where there was nothing to inherit lands on the model classified on
    /// nothing, and every edge through it is then cleared. Over a session
    /// that bleeds: measured on one, twenty of seventy-two nodes sat on
    /// the geometry claiming nothing, and a fifth of the edges had been
    /// cleared because of them.
    ///
    /// The tolerance is the session's — the one the last `classify` was
    /// given, or the kernel's own until there has been one. Somebody
    /// working at five percent means five percent, and a node this pass
    /// leaves behind is one that comes back as an edge with nothing on
    /// it.
    fn settle_fresh(&mut self, mark: u32, left_free: &[CellId]) {
        let Session {
            blocking,
            geometry,
            composites,
            tolerance,
            ..
        } = self;
        let tolerance = *tolerance;
        let Some(geometry) = geometry.as_deref() else {
            return;
        };
        let fresh: Vec<CellId> = blocking
            .nodes_since(mark)
            .into_iter()
            // Left unclassified on purpose, and so not a node waiting to be
            // told what it is on. A slab's inner corners are inside the
            // material: pulled onto the surface their opposites are on, the
            // slab would have no thickness left.
            .filter(|n| !left_free.contains(n))
            .filter(|n| blocking.node_class(*n).is_none() && !blocking.is_frozen(*n))
            // Born among nodes that are on the model. A block somebody
            // has just made — a cube, the box round a geometry — is not a
            // claim that it follows anything, and pinning its corners
            // because they happen to be near would be the interface
            // deciding something nobody said. A node cut into material
            // that *is* on the model is a different matter: everything
            // around it is pinned, and it was only ever unpinned because
            // there was nothing to inherit.
            .filter(|n| blocking.beside_the_classified(*n))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let model = Model::new(geometry, composites);
        let reach = [tolerance.max(0.0) * model.span(); 3];
        blocking.classify_named(
            &fresh,
            // What it claims is nothing — that is why it is here — so
            // proximity is the whole answer.
            |_, at| model.nearest(at, reach).map(|(dim, tag)| On { dim, tag }),
            |on, at| model.project(on.dim, on.tag, at),
        );
    }

    /// Put each node a pillowing pulled in where its neighbours allow, or
    /// refuse the pillowing.
    ///
    /// The rules: the node goes to the point nearest where the step put it
    /// on what **every** classified neighbour of it is on, and takes the
    /// entity it lands on; with no classified neighbour it is free where
    /// the step put it. It is refused when that leaves it a point, or when
    /// it cannot move away from its copy there — the move it is left with
    /// more than 90° less [`PILLOW_ANGLE_DEGREES`] off its step.
    ///
    /// Every node is decided before any is moved, so a refusal leaves
    /// nothing half done (and the session puts the structure back anyway).
    fn settle_pulled(&mut self, pulled: &[gecko_core::Pulled]) -> Result<(), SessionError> {
        let Some(geometry) = self.geometry.as_deref() else {
            return Ok(());
        };
        let model = Model::new(geometry, &self.composites);
        let mut placed: Vec<(CellId, Option<On>, [f64; 3])> = Vec::new();
        for p in pulled {
            let (on, at) = place_pulled(&model, p)?;
            placed.push((p.node, on, at));
        }
        for (node, on, at) in placed {
            self.blocking.set_node_class(node, on)?;
            self.blocking.set_position(node, at)?;
        }
        Ok(())
    }

    /// Lay the layer along a pillow surface whose sides are known.
    fn pillow_patch(&mut self, patch: &[(CellId, CellId)]) -> Result<(), SessionError> {
        // Kept to try the blocks behind instead, should this one be
        // refused: the attempt below changes the structure.
        let untouched = self.blocking.snapshot()?;
        let pulled = self.blocking.pillow_traced(patch)?;
        // The layer leaves a copy of each node where it was, and that copy
        // keeps what the node was pinned to. The one that went in may only
        // slide on what its neighbours are on — a neighbour never loses its
        // classification to a pillowing — and when it cannot, nothing is
        // done.
        if let Err(SessionError::PillowStuck { node, why, .. }) = self.settle_pulled(&pulled) {
            let offer = self.offer_behind(untouched, patch);
            return Err(SessionError::PillowStuck { node, why, offer });
        }
        Ok(())
    }

    /// The blocks behind a refused pillow surface, as a pillow surface of
    /// their own, if that one would pass.
    fn offer_behind(&self, mut trial: Blocking, patch: &[(CellId, CellId)]) -> Vec<PickedFace> {
        let Some(geometry) = self.geometry.as_deref() else {
            return Vec::new();
        };
        let Ok(bigger) = trial.enlarged_pillow_surface(patch) else {
            return Vec::new();
        };
        if bigger.len() == patch.len() {
            return Vec::new();
        }
        let Ok(pulled) = trial.pillow_traced(&bigger) else {
            return Vec::new();
        };
        let model = Model::new(geometry, &self.composites);
        if pulled.iter().any(|p| place_pulled(&model, p).is_err()) {
            return Vec::new();
        }
        let n = |id: CellId| id.get().unwrap_or(u32::MAX);
        bigger
            .into_iter()
            .map(|(face, block)| PickedFace {
                face: n(face),
                block: n(block),
            })
            .collect()
    }

    /// Put a node where its classification says it is.
    ///
    /// A no-op for an unclassified node, and for one already on its
    /// entity. The projection is here rather than in the block structure
    /// because this is the only place that holds both the cells and the
    /// triangles.
    fn pull_onto_class(&mut self, node: CellId) -> Result<(), SessionError> {
        let Some(on) = self.blocking.node_class(node) else {
            return Ok(());
        };
        let Some(geometry) = self.geometry.as_deref() else {
            return Ok(());
        };
        let model = Model::new(geometry, &self.composites);
        let Some(at) = self.blocking.position(node) else {
            return Ok(());
        };
        if let Some(onto) = model.project(on.dim, on.tag, at) {
            self.blocking.set_position(node, onto)?;
        }
        Ok(())
    }

    /// Move a node, under whatever constraint it is under.
    ///
    /// Classified, it **slides on its own entity**: the asked-for point is
    /// projected first, so a node on a curve stays on that curve however
    /// the pointer wanders. It does not come off by being dragged away —
    /// coming off is [`Command::ClassifyNode`] with nothing, which is a
    /// thing you say rather than a thing that happens to you.
    ///
    /// Unclassified, it goes where it was asked and then looks around: an
    /// entity within reach takes it. That is the other half of the C++'s
    /// pair, `snap_node`, and it is what makes dragging a corner onto the
    /// model the way to classify it.
    fn move_node(
        &mut self,
        node: CellId,
        to: [f64; 3],
        snap: f64,
    ) -> Result<[f64; 3], SessionError> {
        if let Some(on) = self.blocking.node_class(node) {
            let onto = self
                .geometry
                .as_deref()
                .and_then(|g| Model::new(g, &self.composites).project(on.dim, on.tag, to))
                .unwrap_or(to);
            self.blocking.set_position(node, onto)?;
            return Ok(onto);
        }

        self.blocking.set_position(node, to)?;
        let Some(geometry) = self.geometry.as_deref() else {
            return Ok(to);
        };
        let model = Model::new(geometry, &self.composites);
        let reach = [snap.max(0.0) * model.span(); 3];
        if let Some((dim, tag)) = model.nearest(to, reach) {
            self.blocking.set_node_class(node, Some(On { dim, tag }))?;
            self.pull_onto_class(node)?;
        }
        Ok(to)
    }

    /// Put every edge on what its two ends agree about, then every face on
    /// what its four edges agree about.
    ///
    /// In that order, because each level consumes the one below it. Cheap
    /// enough to run after anything that touches the model: a couple of
    /// table lookups and an intersection per cell, and no geometry is
    /// measured. A boundary cell that is on nothing leaves the cell above
    /// it on nothing — there is nothing to infer from.
    /// Settle every edge's and then every face's classification.
    ///
    /// The boundary says which entities a cell **could** be on, never
    /// which one it is: the rim of a disc is on the disc and on the tube
    /// it bounds, so an edge with both ends on that rim could be the rim,
    /// a chord across the disc, or a line up the tube. Two things settle
    /// it, in this order:
    ///
    /// 1. **What the cell already claims**, if the boundary still allows
    ///    it. A cut hands the pieces what was cut, and that is the only
    ///    thing that knows a new edge runs across the inside of a disc
    ///    rather than round its edge.
    /// 2. **Where the cell is**, among the candidates at the lowest
    ///    dimension the boundary leaves open. That is what puts the end
    ///    face of a box on the disc rather than on the tube, both of
    ///    which its four edges bound.
    fn settle_classification(&mut self) {
        let Session {
            blocking,
            geometry,
            composites,
            ..
        } = self;
        let Some(geometry) = geometry.as_deref() else {
            return;
        };
        let model = Model::new(geometry, composites);
        blocking.reclassify(|cell| {
            let pairs: Vec<(u8, i32)> = cell.boundary.iter().map(|on| (on.dim, on.tag)).collect();
            // One intersection, read two ways, and the difference is the
            // whole rule about composites. It has to be the **model's**:
            // the bare geometry cannot so much as look up a side, so a
            // cell with one on its boundary would come back with no
            // candidates at all rather than with the surface it is on.
            //
            // What a cell may **keep** includes sides, so one deliberately
            // put on a side stays there. What it may be **given** does
            // not: the diagonal of a face has the same two ends as the
            // side beside it, and nothing should hand it one.
            //
            // Except a composite the boundary is itself **on**: ends on
            // the patch of two surfaces are not ends that merely touch it,
            // and when nothing but the patch holds them both — a pillow
            // opening onto a face put on it leaves exactly that — the cell
            // between them is on it. The same rule as `destination`.
            let allowed = model.shared(&pairs);
            let shared: Vec<(u8, i32)> = allowed
                .iter()
                .copied()
                .filter(|e| {
                    (!gecko_geom::Composites::is_composite(e.1)
                        || composites.is_merged(e.1)
                        || pairs.contains(e))
                        && composites.stand_in(e.0, e.1).is_none()
                })
                .collect();
            let open = |on: On| on.dim >= cell.at_least && allowed.contains(&(on.dim, on.tag));
            if let Some(on) = cell.already.filter(|on| open(*on)) {
                return Some(on);
            }
            let lowest = shared.iter().map(|e| e.0).find(|d| *d >= cell.at_least)?;
            shared
                .iter()
                .filter(|e| e.0 == lowest)
                .min_by(|a, b| {
                    let far =
                        |e: &(u8, i32)| model.distance(e.0, e.1, cell.middle).unwrap_or(f64::MAX);
                    far(a).partial_cmp(&far(b)).expect("a finite distance")
                })
                .map(|(dim, tag)| On {
                    dim: *dim,
                    tag: *tag,
                })
        });
    }

    /// Settle the rebasing origin against the model as it now stands.
    ///
    /// Done when the model changes, never when someone looks at it. An origin
    /// adopted on read would make `view_buffer` depend on how many times it
    /// had been called — and the two façades, which ask at different moments,
    /// would quietly stop agreeing.
    fn settle_origin(&mut self) {
        // Blocks and geometry together. They share one origin because they
        // share one scene, and an origin settled on the blocks alone would
        // put a geometry a kilometer away past the end of f32's precision.
        //
        // The blocking is walked every time — it is what just changed. The
        // geometry is walked **once per geometry**: its triangles do not
        // move, so the box round them cannot change, and a model of ten
        // thousand points was being walked after every edit to find a
        // number that was already known. The Arc's address says which
        // geometry the box was worked out for, so an undo that puts an
        // older one back is noticed without anything having to say so.
        let mut box_of = view::bounds(self.blocking.nodes().map(|(_, p)| p));
        if let Some(geometry) = &self.geometry {
            let which = Arc::as_ptr(geometry) as usize;
            if self.geometry_box.map(|(at, _, _)| at) != Some(which) {
                self.geometry_box =
                    view::bounds(geometry.nodes().iter().copied()).map(|(lo, hi)| (which, lo, hi));
            }
            if let Some((_, lo, hi)) = self.geometry_box {
                box_of = Some(match box_of {
                    Some((a, b)) => (
                        [0, 1, 2].map(|k| a[k].min(lo[k])),
                        [0, 1, 2].map(|k| b[k].max(hi[k])),
                    ),
                    None => (lo, hi),
                });
            }
        }
        self.origin = Some(match box_of {
            Some((lo, hi)) => view::center_of(lo, hi, self.origin),
            None => self.origin.unwrap_or([0.0; 3]),
        });
    }

    /// Transport-agnostic entry point: a JSON command in, a JSON outcome out.
    /// Both façades bottom out here, so neither can grow behavior of its own.
    pub fn apply_json(&mut self, json: &str) -> String {
        let result = serde_json::from_str::<Command>(json)
            .map_err(|e| SessionError::BadCommand(e.to_string()))
            .and_then(|c| self.apply(c));
        match result {
            Ok(outcome) => serde_json::json!({ "ok": true, "outcome": outcome }).to_string(),
            Err(e) => {
                let mut reply = serde_json::json!({ "ok": false, "error": e.to_string() });
                if let SessionError::PillowStuck { offer, .. } = &e {
                    if !offer.is_empty() {
                        reply["offer"] = serde_json::json!(offer);
                    }
                }
                reply.to_string()
            }
        }
    }

    pub fn view_buffer(&self) -> Vec<u8> {
        ViewBuffer::of(&self.blocking, self.origin).to_bytes()
    }

    /// The nodes alone, for a command that moved one and made nothing.
    ///
    /// See [`NodeBuffer`]: a tenth of the bytes and a twentieth of the time,
    /// at the price of being wrong about anything but nodes — so a caller
    /// asks for this only after a move, and for the whole view otherwise.
    pub fn nodes_buffer(&self) -> Vec<u8> {
        NodeBuffer::of(&self.blocking, self.origin).bytes()
    }

    /// The geometry, in the same shape both transports deliver it.
    ///
    /// Rebased on the origin the view buffer last settled on, so the two land
    /// on top of each other in the scene.
    pub fn geometry_buffer(&self) -> Vec<u8> {
        let center = self.origin.unwrap_or([0.0; 3]);
        match &self.geometry {
            Some(geometry) => GeometryBuffer::of(geometry, &self.composites, center).to_bytes(),
            None => GeometryBuffer::empty(center),
        }
    }

    /// Every node, by stable name and position — what a script needs in order
    /// to have something to move.
    pub fn nodes_json(&self) -> String {
        let nodes: Vec<_> = self
            .blocking
            .nodes_in_full()
            .map(|(id, pos, on, frozen)| {
                serde_json::json!({
                    "id": id.get(),
                    "pos": pos,
                    "on": on.map(spot_json),
                    "frozen": frozen,
                })
            })
            .collect();
        serde_json::Value::Array(nodes).to_string()
    }

    /// Every edge, by stable name, with the names of its two ends and what
    /// it is classified on.
    pub fn edges_json(&self) -> String {
        let edges: Vec<_> = self
            .blocking
            .edges()
            .map(|(edge, a, b, on)| {
                serde_json::json!({
                    "id": edge.get(),
                    "nodes": [a.get(), b.get()],
                    "on": on.map(spot_json),
                })
            })
            .collect();
        serde_json::Value::Array(edges).to_string()
    }

    /// Every face, by stable name, with the block it bounds and its center —
    /// enough for a script to name one to [`Command::GlueFaces`] without
    /// having to see it.
    ///
    /// A face already glued is listed once per block, as the surface is.
    pub fn faces_json(&self) -> String {
        // Both of these are read once for the model rather than once per
        // entry. The list has six entries a block, so asking per entry is a
        // walk of the model squared — which is what it used to be, by a
        // field the block faces carried to avoid exactly that.
        let patches: HashSet<CellId> = self.blocking.patches().into_iter().collect();
        let at: HashMap<CellId, [f64; 3]> = self.blocking.nodes().collect();
        let faces: Vec<_> = self
            .blocking
            .block_faces()
            .iter()
            .map(|side| {
                let mut center = [0.0; 3];
                for node in side.nodes {
                    let p = at.get(&node).copied().unwrap_or([0.0; 3]);
                    for k in 0..3 {
                        center[k] += p[k] / 4.0;
                    }
                }
                serde_json::json!({
                    "id": side.face.get(),
                    "block": side.block.get(),
                    // Whether that "block" is one: a face of a patch is a
                    // face on its own, and what can be done with it is not
                    // the same. The caller should not have to work it out
                    // from the shape of the answer.
                    "patch": patches.contains(&side.block),
                    "center": center,
                    "on": side.on.map(spot_json),
                })
            })
            .collect();
        serde_json::Value::Array(faces).to_string()
    }

    /// The sheet through an edge: every edge of it, with the direction the
    /// cut parameter would run in.
    ///
    /// A question, not a change — the same door's read side, like
    /// [`nodes_json`][Self::nodes_json]. The viewer asks it to light up what
    /// a cut would touch.
    pub fn sheet_json(&self, edge: u32) -> String {
        match self.blocking.sheet_through(CellId::from_raw(edge)) {
            Ok(sheet) => {
                let edges: Vec<_> = sheet
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "id": s.edge.get(),
                            "from": self.blocking.position(s.from).unwrap_or([0.0; 3]),
                            "to": self.blocking.position(s.to).unwrap_or([0.0; 3]),
                        })
                    })
                    .collect();
                // How many blocks it runs through, which is what a removal
                // would take away. The edges alone do not say that, and
                // taking out more than you meant to looks like a hole.
                let blocks = self.blocking.sheet_blocks(&sheet);
                // And how many segments it is divided into when meshed,
                // which is a property of the sheet rather than of any one
                // of its edges — so this is where it is answered.
                let segments = self
                    .blocking
                    .sheet_segments(CellId::from_raw(edge))
                    .unwrap_or(0);
                serde_json::json!({
                    "ok": true,
                    "edges": edges,
                    "blocks": blocks,
                    "segments": if segments == 0 { SEGMENTS } else { segments },
                })
                .to_string()
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
        }
    }

    /// What one layer of blocks is made of, and the layers beside it.
    ///
    /// A different question from [`sheet_json`][Self::sheet_json], which
    /// answers what a **cut** would touch. This one answers what is there,
    /// so a client can show the layer on its own, and which layers it could
    /// step to from it.
    ///
    /// `beside` has two entries, in no particular order, either of which
    /// may be null where that side of the layer is the model's surface.
    /// The one to step to is whichever is not where you came from.
    pub fn sheet_cells_json(&self, edge: u32) -> String {
        let edge = CellId::from_raw(edge);
        match self.blocking.sheet_cells(edge) {
            Ok(cells) => {
                let names =
                    |ids: &[CellId]| -> Vec<u32> { ids.iter().filter_map(|c| c.get()).collect() };
                // A layer whose caps cannot be read is still a layer worth
                // showing, so this is reported as "nowhere to step" rather
                // than as a failure to answer.
                let beside = self.blocking.sheets_beside(edge).unwrap_or([None, None]);
                serde_json::json!({
                    "ok": true,
                    "sheet": cells.sheet.get(),
                    "nodes": names(&cells.nodes),
                    "edges": names(&cells.edges),
                    "faces": names(&cells.faces),
                    "blocks": names(&cells.blocks),
                    "beside": beside.map(|c| c.and_then(|c| c.get())),
                })
                .to_string()
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
        }
    }

    /// The chord through a face, for the view to light up.
    ///
    /// The same shape as [`sheet_json`][Self::sheet_json], so the same
    /// drawing code shows both: a list of segments, and how many blocks are
    /// at stake. A question, not a change — nothing is removed or pillowed
    /// by asking.
    pub fn chord_json(&self, face: u32, block: u32) -> String {
        let (face, block) = (CellId::from_raw(face), CellId::from_raw(block));
        match self.blocking.chord_through(face, block) {
            Ok(chord) => {
                let edges: Vec<_> = self
                    .blocking
                    .chord_wire(&chord)
                    .iter()
                    .map(|(id, from, to)| {
                        serde_json::json!({
                            "id": id.get(),
                            "from": self.blocking.position(*from).unwrap_or([0.0; 3]),
                            "to": self.blocking.position(*to).unwrap_or([0.0; 3]),
                        })
                    })
                    .collect();
                serde_json::json!({
                    "ok": true,
                    "edges": edges,
                    "blocks": chord.len(),
                })
                .to_string()
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
        }
    }

    /// Every entity of the geometry, by dimension and tag.
    ///
    /// What a script needs in order to name one to
    /// [`Command::ClassifyNode`] — there being no other way to find out
    /// what a `.msh` called things.
    pub fn entities_json(&self) -> String {
        let Some(geometry) = &self.geometry else {
            return "[]".to_string();
        };
        // With what stands in for it, when a merge took it: a page picking
        // a piece of a merge means the merge, and has to light all of it.
        let composites = &self.composites;
        let of = |dim: u8, tags: Vec<i32>| {
            tags.into_iter()
                .map(move |tag| match composites.stand_in(dim, tag) {
                    Some((d, t)) => serde_json::json!({
                        "dim": dim, "tag": tag, "in": { "dim": d, "tag": t },
                    }),
                    None => serde_json::json!({ "dim": dim, "tag": tag }),
                })
        };
        let all: Vec<_> = of(
            gecko_geom::CORNER,
            geometry.corners().iter().map(|c| c.tag).collect(),
        )
        .chain(of(
            gecko_geom::CURVE,
            geometry.curves().iter().map(|c| c.tag).collect(),
        ))
        .chain(of(
            gecko_geom::SURFACE,
            geometry.surfaces().iter().map(|s| s.tag).collect(),
        ))
        // And every composite, merged or not, with what it is made of: a
        // tag below zero is otherwise a number nobody can read.
        .chain((0..self.composites.len()).filter_map(|k| {
            let tag = -(k as i32 + 1);
            let (dim, members) = self.composites.members(tag)?;
            Some(serde_json::json!({
                "dim": dim,
                "tag": tag,
                "members": members,
                "merged": self.composites.is_merged(tag),
                "name": name_in(On { dim, tag }, &self.composites),
            }))
        }))
        .collect();
        serde_json::Value::Array(all).to_string()
    }

    /// Every block, by stable name and center — enough for a script to name
    /// one to [`Command::RemoveBlock`] without having to see it.
    pub fn blocks_json(&self) -> String {
        let sides = self.blocking.block_faces();
        let mut names: Vec<CellId> = sides.iter().map(|s| s.block).collect();
        names.dedup();
        let blocks: Vec<_> = names
            .iter()
            .map(|block| {
                let mut center = [0.0; 3];
                let mut corners = 0.0_f64;
                for side in sides.iter().filter(|s| s.block == *block) {
                    for node in side.nodes {
                        let p = self.blocking.position(node).unwrap_or([0.0; 3]);
                        for k in 0..3 {
                            center[k] += p[k];
                        }
                        corners += 1.0;
                    }
                }
                for axis in &mut center {
                    *axis /= corners.max(1.0);
                }
                serde_json::json!({ "id": block.get(), "center": center })
            })
            .collect();
        serde_json::Value::Array(blocks).to_string()
    }

    /// The patches: the blocks that are not blocks.
    ///
    /// A list rather than a flag per face, because the page asks once per
    /// redraw and a patch is named by its component. What it is for:
    /// knowing whether `Del` on a picked face takes the face or the block,
    /// without the page having to work it out from the shape of something
    /// else.
    pub fn patches_json(&self) -> String {
        let names: Vec<u32> = self
            .blocking
            .patches()
            .iter()
            .filter_map(|p| p.get())
            .collect();
        serde_json::to_string(&names).unwrap_or_else(|_| "[]".into())
    }

    /// The groups: what each is called, whether it is written into the
    /// mesh, and how many blocks are in it.
    pub fn groups_json(&self) -> String {
        let all: Vec<_> = self
            .groups
            .all()
            .iter()
            .map(|g| {
                serde_json::json!({
                    "name": g.name,
                    "bit": g.bit,
                    "written": g.written,
                    "blocks": self.in_group(g.bit),
                })
            })
            .collect();
        serde_json::Value::Array(all).to_string()
    }

    pub fn stats_json(&self) -> String {
        // One census rather than eight counts: walking the map is what
        // this costs, and it is walked once per dimension here.
        let seen = self.blocking.census();
        serde_json::json!({
            "nodes": seen.nodes,
            "edges": seen.edges,
            "faces": seen.faces,
            "blocks": seen.blocks,
            "patches": seen.patches,
            "geometry": self.geometry_json(),
            "history": { "steps": self.steps, "back": self.past.len() },
            "mesh": self.mesh_json(),
            "classified": {
                "nodes": seen.classified_nodes,
                "edges": seen.classified_edges,
                "faces": seen.classified_faces,
                "frozen": seen.frozen,
            },
        })
        .to_string()
    }

    /// What there is of a mesh, and **which** mesh.
    ///
    /// `generation` counts builds and drops alike: a client holds the
    /// number it last drew and re-fetches when it differs, which is right
    /// for a mesh that has just been thrown away by an edit as well as for
    /// one just built.
    fn mesh_json(&self) -> serde_json::Value {
        match &self.mesh {
            Some(mesh) => serde_json::json!({
                "generation": self.mesh_generation,
                "nodes": mesh.points.len(),
                "hexes": mesh.hexes.len(),
            }),
            None => serde_json::json!({ "generation": self.mesh_generation }),
        }
    }

    /// The mesh's skin, in the bytes both transports deliver it in.
    ///
    /// Rebased on the same origin as everything else in the scene, which
    /// it has to be: they are drawn on top of each other.
    pub fn mesh_buffer(&self) -> Vec<u8> {
        let center = self.origin.unwrap_or([0.0; 3]);
        match &self.mesh {
            Some(mesh) => MeshBuffer::of(mesh, center).to_bytes(),
            None => MeshBuffer::empty(center),
        }
    }

    /// Every face of every hexahedron worse than `below`, the worst first
    /// and no more than [`MOST_CELLS_SHOWN`] of them, in the mesh buffer's
    /// encoding — the cells to look at, inside the mesh as well as on it.
    pub fn cells_below_buffer(&self, below: f64) -> Vec<u8> {
        let center = self.origin.unwrap_or([0.0; 3]);
        match &self.mesh {
            Some(mesh) => MeshBuffer::cells_below(mesh, center, below, MOST_CELLS_SHOWN).to_bytes(),
            None => MeshBuffer::empty(center),
        }
    }

    /// How good the mesh is: the worst corner of its cells, summed up, and
    /// the blocks the worst of them are in. `null` with no mesh.
    pub fn quality_json(&self) -> String {
        let Some(mesh) = &self.mesh else {
            return "null".to_string();
        };
        let quality = mesh.quality();
        let summary = gecko_core::summarize(&quality);
        // The worst cell of each block, and how many of its cells are
        // turned inside out.
        let mut by_block: HashMap<CellId, (f64, usize, usize)> = HashMap::new();
        for (h, q) in quality.iter().enumerate() {
            let entry = by_block
                .entry(mesh.blocks[h])
                .or_insert((f64::INFINITY, 0, 0));
            entry.0 = entry.0.min(*q);
            entry.1 += usize::from(*q < 0.0);
            entry.2 += 1;
        }
        let mut blocks: Vec<(CellId, (f64, usize, usize))> = by_block.into_iter().collect();
        blocks.sort_by(|a, b| a.1 .0.total_cmp(&b.1 .0).then(a.0.cmp(&b.0)));
        blocks.truncate(WORST_BLOCKS_LISTED);
        let below: serde_json::Map<String, serde_json::Value> = gecko_core::quality::THRESHOLDS
            .iter()
            .zip(summary.below)
            .map(|(t, n)| (format!("{t}"), serde_json::json!(n)))
            .collect();
        let worst = summary.worst.map(|h| {
            serde_json::json!({
                "cell": h,
                "block": mesh.blocks[h].get(),
                "quality": quality[h],
                "center": mesh.hex_center(h),
                // Corner to opposite corner: how far back to stand to see it.
                "size": apart(
                    mesh.points[mesh.hexes[h][0] as usize],
                    mesh.points[mesh.hexes[h][6] as usize],
                ),
            })
        });
        serde_json::json!({
            "generation": self.mesh_generation,
            "cells": summary.cells,
            "min": summary.min,
            "mean": summary.mean,
            "worst": worst,
            "below": below,
            "bins": gecko_core::quality::BINS,
            "histogram": summary.histogram,
            "blocks": blocks
                .iter()
                .map(|(b, (q, inverted, cells))| {
                    serde_json::json!({
                        "block": b.get(),
                        "worst": q,
                        "inverted": inverted,
                        "cells": cells,
                    })
                })
                .collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// How faithfully the mesh follows the geometry, with the tolerance a
    /// share of the local element size. `null` without both a mesh and a
    /// geometry: there is nothing to hold one against.
    ///
    /// Asked for, not worked out with every mesh: it is a nearest-point
    /// search from every node of the skin and every vertex of the geometry.
    pub fn fidelity_json(&self, share: f64) -> String {
        let (Some(mesh), Some(geometry)) = (&self.mesh, &self.geometry) else {
            return "null".to_string();
        };
        let mut out = fidelity::measure(geometry, &self.composites, mesh, share);
        out["generation"] = serde_json::json!(self.mesh_generation);
        out.to_string()
    }

    /// The session as a Lua script: what a reader would have typed to
    /// arrive where it now is.
    pub fn script(&self) -> String {
        script::write(&self.journal)
    }

    /// The mesh as a VTK legacy file, or nothing if none has been built.
    pub fn mesh_vtk(&self) -> Option<String> {
        let mesh = self.mesh.as_ref()?;
        let written = self.groups.written();
        // Which groups each hexahedron is in: its block's, since a cell of
        // a block is part of whatever the block is part of.
        let masks: Vec<u64> = mesh
            .blocks
            .iter()
            .map(|b| self.blocking.block_groups(*b))
            .collect();
        Some(vtk::write(mesh, "gecko blocking", &written, &masks))
    }

    /// What there is of a geometry, and **which** geometry.
    ///
    /// `generation` counts loads rather than describing one: a client holds
    /// the number it last drew, and re-fetches the buffer when it differs.
    /// Two geometries with the same counts are still two geometries.
    fn geometry_json(&self) -> serde_json::Value {
        let Some(geometry) = &self.geometry else {
            return serde_json::json!({ "generation": self.geometry_generation });
        };
        serde_json::json!({
            "generation": self.geometry_generation,
            "corners": geometry.corners().len(),
            "curves": geometry.curves().len(),
            "surfaces": geometry.surfaces().len(),
            "segments": geometry.nb_segments(),
            "triangles": geometry.nb_triangles(),
        })
    }
}

/// How many cells below a threshold are sent to be drawn, the worst first:
/// enough to see where the trouble is, not so many that showing them is
/// the trouble.
pub const MOST_CELLS_SHOWN: usize = 20_000;

/// How many of the worst blocks the quality read lists.
pub const WORST_BLOCKS_LISTED: usize = 20;

/// What a mesh's quality comes to in one line: its worst cell, and how
/// many are turned inside out if any are.
fn quality_line(mesh: &Mesh) -> String {
    let summary = gecko_core::summarize(&mesh.quality());
    let inverted = summary.below[0];
    if inverted == 0 {
        format!("worst cell {:.2}", summary.min)
    } else {
        format!("worst cell {:.2}, {} inverted", summary.min, inverted)
    }
}

/// How a handful of nodes reads in a message: by name while you could
/// still look them up, by number once you could not.
fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
}

/// Raw names as cells, refusing an empty list.
fn cells(raw: &[u32], what: &str) -> Result<Vec<CellId>, SessionError> {
    if raw.is_empty() {
        return Err(SessionError::BadCommand(format!("no {what} named")));
    }
    Ok(raw.iter().copied().map(CellId::from_raw).collect())
}

/// `edge 514`, `edges 514, 5889`, or `7 edges`.
fn listed(cells: &[CellId], what: &str) -> String {
    match cells {
        [one] => format!("{what} {one}"),
        _ if cells.len() <= 4 => format!(
            "{what}s {}",
            cells
                .iter()
                .map(|n| format!("{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => format!("{} {what}s", cells.len()),
    }
}

fn these(nodes: &[CellId]) -> String {
    match nodes {
        [one] => format!("node {one}"),
        _ if nodes.len() <= 4 => format!(
            "nodes {}",
            nodes
                .iter()
                .map(|n| format!("{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => format!("{} nodes", nodes.len()),
    }
}

/// A command as the journal keeps it.
///
/// A geometry arrives with the whole file in its hand, because a browser
/// has no path to send — and a journal holding a copy of every mesh ever
/// loaded would be the session's memory spent on nothing anybody reads.
/// The name is what the script needs.
fn trim(command: Command) -> Command {
    match command {
        Command::LoadGeometry { name, .. } => Command::LoadGeometry {
            msh: String::new(),
            name,
        },
        other => other,
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn spot_json(on: On) -> serde_json::Value {
    serde_json::json!({ "dim": on.dim, "tag": on.tag })
}

/// How a classification reads in a message.
///
/// A composite is printed by what it is made of: `side -1` tells a reader
/// nothing, and the tag of a side is an implementation detail of the table
/// it lives in.
fn name_in(on: On, composites: &Composites) -> String {
    match composites.members(on.tag) {
        None => name_of(on),
        // A merge is one entity of the model, and named as one: by its
        // pieces, run together.
        Some((dim, members)) if composites.is_merged(on.tag) => {
            let list: Vec<String> = members.iter().map(|m| m.to_string()).collect();
            format!(
                "{} {}",
                if dim == gecko_geom::CURVE {
                    "curve"
                } else {
                    "surface"
                },
                list.join("+")
            )
        }
        Some((dim, members)) => {
            let what = if dim == gecko_geom::CURVE {
                "curves"
            } else {
                "surfaces"
            };
            let list: Vec<String> = members.iter().map(|m| m.to_string()).collect();
            format!(
                "{} {} ({what} {})",
                if dim == gecko_geom::CURVE {
                    "side"
                } else {
                    "patch"
                },
                -on.tag,
                list.join(", ")
            )
        }
    }
}

/// How a classification reads in a message.
fn name_of(on: On) -> String {
    let what = match on.dim {
        gecko_geom::CORNER => "corner",
        gecko_geom::CURVE => "curve",
        _ => "surface",
    };
    format!("{what} {}", on.tag)
}
