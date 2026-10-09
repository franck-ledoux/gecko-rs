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
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use view::ViewBuffer;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{0}")]
    Blocking(#[from] BlockingError),
    #[error("bad command: {0}")]
    BadCommand(String),
    #[error("a cube's size must be strictly positive, not {0}")]
    BadSize(f64),
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
    /// Kept between views so the scene does not slide every time something
    /// moves. See [`view::stable_centre`].
    origin: Option<[f64; 3]>,
}

impl Session {
    pub fn new() -> Result<Self, SessionError> {
        Ok(Session {
            blocking: Blocking::new()?,
            origin: None,
        })
    }

    pub fn blocking(&self) -> &Blocking {
        &self.blocking
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
        let world: Vec<f64> = self.blocking.nodes().flat_map(|(_, p)| p).collect();
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
        })
        .to_string()
    }
}
