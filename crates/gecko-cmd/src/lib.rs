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

pub mod groups;
pub mod script;
pub mod view;
pub mod vtk;

use std::collections::VecDeque;
use std::sync::Arc;

use crate::groups::{GroupError, Groups};
use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError, Mesh, On, Smoothing, SEGMENTS};
use gecko_geom::model::GeometryError;
use gecko_geom::{BoundsError, Composites, Geometry, Model};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use view::{GeometryBuffer, MeshBuffer, ViewBuffer};

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
    #[error("{0}")]
    Group(#[from] GroupError),
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
/// A share rather than a length, because a tolerance in millimetres is
/// right for exactly one model. One figure for all three dimensions, as the
/// C++ defaults them — the geometry can take a different one per dimension
/// and will, the day a model needs it.
pub const SNAP_SHARE: f64 = 0.02;

/// How many passes a smoothing makes when nobody says.
pub const SMOOTH_ROUNDS: u32 = 5;

/// How far along the way to the middle of its neighbours a node goes in
/// one pass, when nobody says.
///
/// Half, not all: all of it is what makes a Laplacian swing from one side
/// of a crease to the other and never settle, and half of it lets somebody
/// watch what is happening to their model.
pub const SMOOTH_SHARE: f64 = 0.5;

fn yes() -> bool {
    true
}

fn default_snap() -> f64 {
    SNAP_SHARE
}

fn default_rounds() -> u32 {
    SMOOTH_ROUNDS
}

fn default_share() -> f64 {
    SMOOTH_SHARE
}

fn default_segments() -> u32 {
    SEGMENTS
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
        /// As [`Command::Pillow`]: how near a node drawn in has to be to a
        /// geometric entity to be taken by it.
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
    /// Each entry is a face and the block it was picked on — the pair a
    /// click gives. The blocks are drawn in and the layer fills the space,
    /// so the model's surface does not move.
    Pillow {
        faces: Vec<PickedFace>,
        /// How near a node drawn in has to be to a geometric entity to be
        /// taken by it — the same share [`Command::Classify`] uses.
        ///
        /// Pillowing moves nodes, and a node somewhere new is on
        /// something new: the corner of a box drawn in off the rim of a
        /// disc is still on that disc. Only the nodes that moved are
        /// looked at again.
        #[serde(default = "default_snap")]
        tolerance: f64,
    },
    /// Grow a new block out of a face, along the normal at its centre.
    ///
    /// The face is given as the pair a click gives — the face and the block
    /// it was picked on — and does not move: the new block is laid against
    /// it and welded on.
    ExtrudeFace {
        face: u32,
        block: u32,
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
    /// Move the nodes towards the middle of their neighbours, as far as
    /// what they are classified on allows: a **smart Laplacian**.
    ///
    /// A node goes `share` of the way to the average of its one-ring, and
    /// the move is taken only if it does not make the worst corner round
    /// it worse. Frozen nodes, nodes on a geometric corner, and nodes on
    /// the blocking's boundary that follow nothing all stay where they are.
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
        #[serde(default = "yes")]
        written: bool,
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
        on: Option<Spot>,
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
    /// edge divided into four against its neighbour's five leaves a face
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
    /// moves. See [`view::stable_centre`].
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
            geometry: None,
            composites: Composites::default(),
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
        let before = (self.steps > 0).then(|| self.capture()).transpose()?;
        // Where the nodes end, so that the ones the command is about to
        // make can be told from the ones it renames.
        let mark = self.blocking.corner_mark();
        // Kept before the command is consumed, and trimmed: a geometry
        // command carries the whole file, and the journal wants the name.
        let written = trim(command.clone());
        let mut outcome = self.run(command)?;
        outcome.script = Some(script::line(&written));
        self.journal.push(written);
        if outcome.view_changed {
            if let Some(state) = before {
                self.push(state);
            }
            self.settle_origin();
            // The nodes this command made, onto the model if they are on
            // it. Before the settling below, which reads what they say.
            self.settle_fresh(mark);
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

    fn push(&mut self, state: State) {
        self.past.push_back(state);
        while self.past.len() > self.steps {
            self.past.pop_front();
        }
    }

    /// Put the last state back.
    ///
    /// Nothing is settled afterwards. The state was settled when it was
    /// taken, and running the classification over it again would be a
    /// chance for it to come out differently — which is the one thing an
    /// undo must not do.
    fn undo(&mut self) -> Result<Outcome, SessionError> {
        let Some(state) = self.past.pop_back() else {
            return Ok(Outcome {
                script: None,
                message: "nothing to undo".to_string(),
                view_changed: false,
            });
        };
        let was = self.geometry.as_ref().map(Arc::as_ptr);
        self.drop_mesh();
        self.blocking = state.blocking;
        self.geometry = state.geometry;
        self.composites = state.composites;
        self.groups = state.groups;
        self.origin = state.origin;
        self.journal.truncate(state.journal);
        // Forwards even when the geometry goes backwards: the number says
        // "not the one you last drew", and a client that saw it fall would
        // have no reason to trust it the next time it rose.
        if was != self.geometry.as_ref().map(Arc::as_ptr) {
            self.geometry_generation += 1;
        }
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
                        "face {face:?} laid on surface {surface} — corners {:?} at {}{}",
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
            Command::AddFace { corners } => {
                let id = self.blocking.add_face(corners)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "face {:?} added — {} nodes, {} edges, {} faces, {} patches",
                        id,
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_patches(),
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
                    .node_by_id(node)
                    .and_then(|v| self.blocking.position(v))
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
                    let Some(at) = self
                        .blocking
                        .node_by_id(*other)
                        .and_then(|v| self.blocking.position(v))
                    else {
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
                            Some(on) => format!(
                                "node {node:?} moved along {}",
                                name_in(on, &self.composites)
                            ),
                            None => format!("node {node:?} moved to {landed:?}"),
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
                        "block {block:?} removed — {} nodes, {} edges, {} faces, {} blocks",
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
                        "face {face:?} removed — {} nodes, {} edges, {} faces, {} patches",
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_patches(),
                    ),
                    view_changed: true,
                })
            }
            Command::RemoveSheet { edge } => {
                let edge = CellId::from_raw(edge);
                self.blocking.remove_sheet(edge)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "sheet through {edge:?} removed — {} nodes, {} edges, {} faces, {} blocks",
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
                        "chord through face {face:?} removed, {} block{} — \
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
                let drawn = self.blocking.pillow_chord(face, block)?;
                // As for a pillow: the nodes that went in have moved, and
                // where they are now is a different question from where
                // they were.
                self.settle_moved(&drawn, tolerance);
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
                        "sheet through {edge:?} cut at {at:.3} — {} nodes, {} edges, {} faces, {} blocks",
                        self.blocking.nb_nodes(),
                        self.blocking.nb_edges(),
                        self.blocking.nb_faces(),
                        self.blocking.nb_blocks(),
                    ),
                    view_changed: true,
                })
            }
            Command::Pillow { faces, tolerance } => {
                let patch: Vec<_> = faces
                    .iter()
                    .map(|f| (CellId::from_raw(f.face), CellId::from_raw(f.block)))
                    .collect();
                let drawn = self.blocking.pillow(&patch)?;
                // The layer leaves a copy of each node where it was, and
                // that copy keeps what the node was pinned to. The one
                // that went in has moved, so where **it** is has to be
                // looked at again — it has left the curve it was on and
                // may still be on the surface that curve bounds.
                self.settle_moved(&drawn, tolerance);
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
            Command::ExtrudeFace {
                face,
                block,
                height,
            } => {
                let (face, block) = (CellId::from_raw(face), CellId::from_raw(block));
                let grown = self.blocking.extrude_face(face, block, height)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "block {grown:?} grown from face {face:?} by {height:.3} — \
                         {} nodes, {} edges, {} faces, {} blocks",
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
                let on = on.map(|s| On {
                    dim: s.dim,
                    tag: s.tag,
                });
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
                        "block {id:?} boxes {round} — \
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
                let named = self.named(&blocks)?;
                let bit = self
                    .groups
                    .make(&name, written)
                    .ok_or(GroupError::Full)?
                    .bit;
                self.blocking.set_in_group(&named, bit, true)?;
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "{} block{} in {name:?} — {} in all{}",
                        named.len(),
                        plural(named.len()),
                        self.in_group(bit),
                        if written { "" } else { ", not written out" },
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
                let named = self.named(&blocks)?;
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
                    // Nothing moved, but what the scene is coloured and
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
                        "the sheet through {edge:?} is in {segments} segment{}",
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
                let message = format!("meshed — {}", vtk::count(&mesh));
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
                            "; node {:?} kept {} and left {}",
                            u.node,
                            name_in(u.kept, &self.composites),
                            name_in(u.dropped, &self.composites)
                        )
                    })
                    .collect::<String>();
                Ok(Outcome {
                    script: None,
                    message: format!(
                        "faces {first:?} and {second:?} glued — {} nodes, {} edges, {} faces, {} blocks{dropped}",
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
    fn named(&self, nodes: &[u32]) -> Result<Vec<CellId>, SessionError> {
        if nodes.is_empty() {
            return Err(SessionError::BadCommand("no node named".to_string()));
        }
        let names: Vec<CellId> = nodes.iter().copied().map(CellId::from_raw).collect();
        for node in &names {
            if self.blocking.node_by_id(*node).is_none() {
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
                "block {:?} added — {} nodes, {} edges, {} faces, {} blocks",
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
        let mask = 1u64 << bit;
        self.blocking
            .blocks()
            .into_iter()
            .filter(|b| self.blocking.block_groups(*b) & mask != 0)
            .count()
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
    fn settle_fresh(&mut self, mark: u32) {
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

    /// Decide again what these nodes are on, by where they are now.
    ///
    /// For an operation that **moves** nodes: the blanket
    /// [`Command::Classify`] would undo every classification anybody had
    /// made by hand, and leaving them as they were pins a node to a curve
    /// it is no longer anywhere near — which drags the mesh out through
    /// the boundary when it is projected, and turns the cells inside out.
    fn settle_moved(&mut self, nodes: &[(CellId, Option<On>)], tolerance: f64) {
        let Session {
            blocking,
            geometry,
            composites,
            ..
        } = self;
        let Some(geometry) = geometry.as_deref() else {
            // Nothing is written back either: with no triangles there is
            // nobody to decide, and a hint nobody reads is a stale
            // classification.
            return;
        };
        // What each of them was following, put back as a **hint** before
        // anything is decided. The operation let go of it on the way in —
        // rightly, since the node is inside the material — but a node that
        // only slid along what it was on has not left it, and the rule
        // below is the one that can tell.
        for (node, was) in nodes {
            let _ = blocking.set_node_class(*node, *was);
        }
        let names: Vec<CellId> = nodes.iter().map(|(name, _)| *name).collect();
        let model = Model::new(geometry, composites);
        let reach = [tolerance.max(0.0) * model.span(); 3];
        blocking.classify_named(
            &names,
            |already, at| {
                let found = model.nearest(at, reach).map(|(dim, tag)| On { dim, tag });
                // A node deliberately put on a **side** keeps it as long as
                // it is still somewhere on that side.
                //
                // Proximity cannot say "side": a composite is never a
                // candidate, so the nearest thing it can name is one of the
                // curves the side is made of — and being on one of those
                // *is* being on the side. Taking the curve instead would
                // quietly narrow a node that has not moved off anything,
                // which is what a node sliding along the rim of a pillowed
                // patch does every time.
                //
                // Only composites. Between two entities of the model,
                // proximity's answer is the better one: a corner drawn in
                // off the rim of a disc really has left the rim, and
                // keeping what it claimed is exactly the bug this settling
                // exists to avoid.
                match (already, found) {
                    (Some(was), Some(now))
                        if Composites::is_composite(was.tag)
                            && model
                                .containing(now.dim, now.tag)
                                .contains(&(was.dim, was.tag)) =>
                    {
                        Some(was)
                    }
                    (_, now) => now,
                }
            },
            |on, at| model.project(on.dim, on.tag, at),
        );
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
        let Some(vertex) = self.blocking.node_by_id(node) else {
            return Ok(());
        };
        let at = self.blocking.position(vertex).unwrap_or([0.0; 3]);
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
            let allowed = model.shared(&pairs);
            let shared: Vec<(u8, i32)> = allowed
                .iter()
                .copied()
                .filter(|e| !gecko_geom::Composites::is_composite(e.1))
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
                    let far = |e: &(u8, i32)| {
                        geometry.distance(e.0, e.1, cell.middle).unwrap_or(f64::MAX)
                    };
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
        // put a geometry a kilometre away past the end of f32's precision.
        let world: Vec<f64> = self
            .blocking
            .nodes()
            .flat_map(|(_, p)| p)
            .chain(
                self.geometry
                    .iter()
                    .flat_map(|g| g.nodes().iter().flatten().copied()),
            )
            .collect();
        self.origin = Some(view::stable_centre(&world, self.origin));
    }

    /// Transport-agnostic entry point: a JSON command in, a JSON outcome out.
    /// Both façades bottom out here, so neither can grow behaviour of its own.
    pub fn apply_json(&mut self, json: &str) -> String {
        let result = serde_json::from_str::<Command>(json)
            .map_err(|e| SessionError::BadCommand(e.to_string()))
            .and_then(|c| self.apply(c));
        match result {
            Ok(outcome) => serde_json::json!({ "ok": true, "outcome": outcome }).to_string(),
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
        }
    }

    pub fn view_buffer(&self) -> Vec<u8> {
        ViewBuffer::of(&self.blocking, self.origin).to_bytes()
    }

    /// The geometry, in the same shape both transports deliver it.
    ///
    /// Rebased on the origin the view buffer last settled on, so the two land
    /// on top of each other in the scene.
    pub fn geometry_buffer(&self) -> Vec<u8> {
        let centre = self.origin.unwrap_or([0.0; 3]);
        match &self.geometry {
            Some(geometry) => GeometryBuffer::of(geometry, centre).to_bytes(),
            None => GeometryBuffer::empty(centre),
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

    /// Every face, by stable name, with the block it bounds and its centre —
    /// enough for a script to name one to [`Command::GlueFaces`] without
    /// having to see it.
    ///
    /// A face already glued is listed once per block, as the surface is.
    pub fn faces_json(&self) -> String {
        let faces: Vec<_> = self
            .blocking
            .block_faces()
            .iter()
            .map(|side| {
                let mut centre = [0.0; 3];
                for node in side.nodes {
                    let p = self.blocking.position(node).unwrap_or([0.0; 3]);
                    for k in 0..3 {
                        centre[k] += p[k] / 4.0;
                    }
                }
                serde_json::json!({
                    "id": side.face.get(),
                    "block": side.block.get(),
                    // Whether that "block" is one: a face of a patch is a
                    // face on its own, and what can be done with it is not
                    // the same. The caller should not have to work it out
                    // from the shape of the answer.
                    "patch": !self.blocking.named_is_block(side.block),
                    "centre": centre,
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
        let of = |dim: u8, tags: Vec<i32>| {
            tags.into_iter()
                .map(move |tag| serde_json::json!({ "dim": dim, "tag": tag }))
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
        .collect();
        serde_json::Value::Array(all).to_string()
    }

    /// Every block, by stable name and centre — enough for a script to name
    /// one to [`Command::RemoveBlock`] without having to see it.
    pub fn blocks_json(&self) -> String {
        let sides = self.blocking.block_faces();
        let mut names: Vec<CellId> = sides.iter().map(|s| s.block).collect();
        names.dedup();
        let blocks: Vec<_> = names
            .iter()
            .map(|block| {
                let mut centre = [0.0; 3];
                let mut corners = 0.0_f64;
                for side in sides.iter().filter(|s| s.block == *block) {
                    for node in side.nodes {
                        let p = self.blocking.position(node).unwrap_or([0.0; 3]);
                        for k in 0..3 {
                            centre[k] += p[k];
                        }
                        corners += 1.0;
                    }
                }
                for axis in &mut centre {
                    *axis /= corners.max(1.0);
                }
                serde_json::json!({ "id": block.get(), "centre": centre })
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
        let centre = self.origin.unwrap_or([0.0; 3]);
        match &self.mesh {
            Some(mesh) => MeshBuffer::of(mesh, centre).to_bytes(),
            None => MeshBuffer::empty(centre),
        }
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

/// How a handful of nodes reads in a message: by name while you could
/// still look them up, by number once you could not.
fn these(nodes: &[CellId]) -> String {
    match nodes {
        [one] => format!("node {one:?}"),
        _ if nodes.len() <= 4 => format!(
            "nodes {}",
            nodes
                .iter()
                .map(|n| format!("{n:?}"))
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
