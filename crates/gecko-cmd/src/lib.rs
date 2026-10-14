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

pub mod view;

use std::collections::VecDeque;
use std::sync::Arc;

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError, On};
use gecko_geom::model::GeometryError;
use gecko_geom::{BoundsError, Geometry};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use view::{GeometryBuffer, ViewBuffer};

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
    #[error("no geometry is loaded — open a .msh first")]
    NoGeometry,
}

/// How much room a bounding box leaves round the geometry, as a share of
/// each half-side. The C++'s own default.
pub const BOX_MARGIN: f64 = 0.1;

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

/// How near a node has to be to a geometric entity to be classified on it,
/// as a share of the geometry's own size.
///
/// A share rather than a length, because a tolerance in millimetres is
/// right for exactly one model. One figure for all three dimensions, as the
/// C++ defaults them — the geometry can take a different one per dimension
/// and will, the day a model needs it.
pub const SNAP_SHARE: f64 = 0.02;

fn default_snap() -> f64 {
    SNAP_SHARE
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
    /// Weld two blocks along a face. `first` stays where it is and `second`
    /// is brought onto it, so the order is part of the command rather than a
    /// detail of how it is carried out.
    GlueFaces {
        first: u32,
        second: u32,
    },
    /// Take one block away. Whatever it was glued to keeps its own cells —
    /// they are split off on the way out, not deleted with it.
    RemoveBlock {
        block: u32,
    },
    /// Take out the whole sheet of blocks through an edge.
    ///
    /// Whatever was glued to either end of the layer ends up glued to the
    /// other, with the seam half way between.
    RemoveSheet {
        edge: u32,
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
    origin: Option<[f64; 3]>,
}

impl Session {
    pub fn new() -> Result<Self, SessionError> {
        Ok(Session {
            blocking: Blocking::new()?,
            geometry: None,
            geometry_generation: 0,
            origin: None,
            past: VecDeque::new(),
            steps: HISTORY_STEPS,
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
        let outcome = self.run(command)?;
        if outcome.view_changed {
            if let Some(state) = before {
                self.push(state);
            }
            self.settle_origin();
            // Every edge, after anything that touched the model. An edge's
            // classification is a statement about its two ends, so it is
            // out of date the moment either of them is — and a cut makes
            // edges nobody has ever classified. Two table lookups apiece,
            // no geometry measured.
            self.settle_classification();
        }
        Ok(outcome)
    }

    /// The session as it stands, ready to be put back.
    fn capture(&self) -> Result<State, SessionError> {
        Ok(State {
            blocking: self.blocking.snapshot()?,
            geometry: self.geometry.clone(),
            origin: self.origin,
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
                message: "nothing to undo".to_string(),
                view_changed: false,
            });
        };
        let was = self.geometry.as_ref().map(Arc::as_ptr);
        self.blocking = state.blocking;
        self.geometry = state.geometry;
        self.origin = state.origin;
        // Forwards even when the geometry goes backwards: the number says
        // "not the one you last drew", and a client that saw it fall would
        // have no reason to trust it the next time it rose.
        if was != self.geometry.as_ref().map(Arc::as_ptr) {
            self.geometry_generation += 1;
        }
        Ok(Outcome {
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
            Command::CreateCube { origin, size } => {
                // `size <= 0.0` alone would let NaN through, since every
                // comparison with it is false.
                if size <= 0.0 || !size.is_finite() {
                    return Err(SessionError::BadSize(size));
                }
                self.add(cube_corners(origin, size))
            }
            Command::MoveNode { node, to, snap } => {
                let node = CellId::from_raw(node);
                let landed = self.move_node(node, to, snap)?;
                Ok(Outcome {
                    message: match self.blocking.node_class(node) {
                        Some(on) => format!("node {node:?} moved along {}", name_of(on)),
                        None => format!("node {node:?} moved to {landed:?}"),
                    },
                    view_changed: true,
                })
            }
            Command::RemoveBlock { block } => {
                let block = CellId::from_raw(block);
                self.blocking.remove_block(block)?;
                Ok(Outcome {
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
            Command::RemoveSheet { edge } => {
                let edge = CellId::from_raw(edge);
                self.blocking.remove_sheet(edge)?;
                Ok(Outcome {
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
            Command::Pillow { faces } => {
                let patch: Vec<_> = faces
                    .iter()
                    .map(|f| (CellId::from_raw(f.face), CellId::from_raw(f.block)))
                    .collect();
                self.blocking.pillow(&patch)?;
                Ok(Outcome {
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
                    message,
                    view_changed: true,
                })
            }
            Command::Classify { tolerance } => {
                let (nodes, edges, faces) = self.classify(tolerance)?;
                Ok(Outcome {
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
                    message: match on {
                        Some(on) => format!("{} put on {}", these(&names), name_of(on)),
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
            Command::ClearGeometry => {
                self.geometry = None;
                self.geometry_generation += 1;
                Ok(Outcome {
                    message: "geometry cleared".to_string(),
                    view_changed: true,
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
                self.blocking.glue_faces(first, second)?;
                Ok(Outcome {
                    message: format!(
                        "faces {first:?} and {second:?} glued — {} nodes, {} edges, {} faces, {} blocks",
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
                blocking, geometry, ..
            } = self;
            let geometry = geometry.as_deref().ok_or(SessionError::NoGeometry)?;
            let reach = [tolerance.max(0.0) * geometry.span(); 3];
            blocking.classify_nodes(
                |at| {
                    geometry
                        .nearest(at, reach)
                        .map(|(dim, tag)| On { dim, tag })
                },
                |on, at| geometry.project(on.dim, on.tag, at),
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
        let Some(vertex) = self.blocking.node_by_id(node) else {
            return Ok(());
        };
        let at = self.blocking.position(vertex).unwrap_or([0.0; 3]);
        if let Some(onto) = geometry.project(on.dim, on.tag, at) {
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
                .as_ref()
                .and_then(|g| g.project(on.dim, on.tag, to))
                .unwrap_or(to);
            self.blocking.set_position(node, onto)?;
            return Ok(onto);
        }

        self.blocking.set_position(node, to)?;
        let Some(geometry) = self.geometry.as_deref() else {
            return Ok(to);
        };
        let reach = [snap.max(0.0) * geometry.span(); 3];
        if let Some((dim, tag)) = geometry.nearest(to, reach) {
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
            blocking, geometry, ..
        } = self;
        let Some(geometry) = geometry.as_deref() else {
            return;
        };
        blocking.reclassify(|cell| {
            let pairs: Vec<(u8, i32)> = cell.boundary.iter().map(|on| (on.dim, on.tag)).collect();
            let shared = geometry.shared(&pairs);
            let open = |on: On| on.dim >= cell.at_least && shared.contains(&(on.dim, on.tag));
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
                serde_json::json!({ "ok": true, "edges": edges, "blocks": blocks }).to_string()
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

    pub fn stats_json(&self) -> String {
        // One census rather than eight counts: walking the map is what
        // this costs, and it is walked once per dimension here.
        let seen = self.blocking.census();
        serde_json::json!({
            "nodes": seen.nodes,
            "edges": seen.edges,
            "faces": seen.faces,
            "blocks": seen.blocks,
            "geometry": self.geometry_json(),
            "history": { "steps": self.steps, "back": self.past.len() },
            "classified": {
                "nodes": seen.classified_nodes,
                "edges": seen.classified_edges,
                "faces": seen.classified_faces,
                "frozen": seen.frozen,
            },
        })
        .to_string()
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
fn name_of(on: On) -> String {
    let what = match on.dim {
        gecko_geom::CORNER => "corner",
        gecko_geom::CURVE => "curve",
        _ => "surface",
    };
    format!("{what} {}", on.tag)
}
