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

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError};
use gecko_geom::model::GeometryError;
use gecko_geom::Geometry;
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
    geometry: Option<Geometry>,
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
}

impl Session {
    pub fn new() -> Result<Self, SessionError> {
        Ok(Session {
            blocking: Blocking::new()?,
            geometry: None,
            geometry_generation: 0,
            origin: None,
        })
    }

    pub fn blocking(&self) -> &Blocking {
        &self.blocking
    }

    pub fn geometry(&self) -> Option<&Geometry> {
        self.geometry.as_ref()
    }

    /// The one mutation path.
    pub fn apply(&mut self, command: Command) -> Result<Outcome, SessionError> {
        let outcome = self.run(command)?;
        if outcome.view_changed {
            self.settle_origin();
        }
        Ok(outcome)
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
            Command::MoveNode { node, to } => {
                let node = CellId::from_raw(node);
                self.blocking.set_position(node, to)?;
                Ok(Outcome {
                    message: format!("node {node:?} moved to {to:?}"),
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
                self.blocking.split_sheet(edge, at)?;
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
                self.geometry = Some(geometry);
                self.geometry_generation += 1;
                // The view changes too, even though not one block moved: the
                // rebasing origin is settled over the geometry as well, and a
                // geometry far from the blocks moves it.
                Ok(Outcome {
                    message,
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
            .nodes()
            .map(|(id, pos)| serde_json::json!({ "id": id.get(), "pos": pos }))
            .collect();
        serde_json::Value::Array(nodes).to_string()
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
        serde_json::json!({
            "nodes": self.blocking.nb_nodes(),
            "edges": self.blocking.nb_edges(),
            "faces": self.blocking.nb_faces(),
            "blocks": self.blocking.nb_blocks(),
            "geometry": self.geometry_json(),
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

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
