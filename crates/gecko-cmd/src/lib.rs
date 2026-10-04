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

use gecko_core::{Blocking, BlockingError, CellId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use view::ViewBuffer;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{0}")]
    Blocking(#[from] BlockingError),
    #[error("bad command: {0}")]
    BadCommand(String),
}

/// Everything that can change a session. Deliberately one variant for now:
/// the point of this step is the path, not what travels down it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    AddBlock { corners: [[f64; 3]; 8] },
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
}

impl Session {
    pub fn new() -> Result<Self, SessionError> {
        Ok(Session {
            blocking: Blocking::new()?,
        })
    }

    pub fn blocking(&self) -> &Blocking {
        &self.blocking
    }

    /// The one mutation path.
    pub fn apply(&mut self, command: Command) -> Result<Outcome, SessionError> {
        match command {
            Command::AddBlock { corners } => {
                let id: CellId = self.blocking.add_block(corners)?;
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
        }
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
        ViewBuffer::of(&self.blocking).to_bytes()
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
