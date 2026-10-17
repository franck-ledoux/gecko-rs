//! A session in this process, and the files around it.
//!
//! The host a terminal gets. The page's host is the other one: a session in
//! another wasm module, reached through JavaScript. Both answer the same
//! four questions, which is the point of [`Kernel`] being four questions
//! wide.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use gecko_cmd::{Session, SessionError};

use crate::Kernel;

pub struct Local {
    session: RefCell<Session>,
}

impl Local {
    pub fn new() -> Result<Rc<Self>, SessionError> {
        Ok(Rc::new(Local {
            session: RefCell::new(Session::new()?),
        }))
    }

    /// The session itself, for what a script cannot ask for: the buffers a
    /// viewer draws.
    pub fn session(&self) -> Ref<'_, Session> {
        self.session.borrow()
    }
}

impl Kernel for Local {
    fn apply(&self, command: &str) -> String {
        self.session.borrow_mut().apply_json(command)
    }

    fn query(&self, kind: &str, arg: &str) -> String {
        let session = self.session.borrow();
        match kind {
            "nodes" => session.nodes_json(),
            "edges" => session.edges_json(),
            "faces" => session.faces_json(),
            "blocks" => session.blocks_json(),
            "stats" => session.stats_json(),
            "entities" => session.entities_json(),
            "sheet" => session.sheet_json(arg.trim().parse().unwrap_or(u32::MAX)),
            // Two numbers, as a JSON array: `query` carries one string, and
            // a chord is named by a face **and** the block it was clicked on.
            "chord" => {
                let pair: Vec<u32> = serde_json::from_str(arg).unwrap_or_default();
                session.chord_json(
                    pair.first().copied().unwrap_or(u32::MAX),
                    pair.get(1).copied().unwrap_or(u32::MAX),
                )
            }
            // `null` when nothing has been meshed, which is a JSON answer
            // rather than a special case for the caller.
            "vtk" => serde_json::to_string(&session.mesh_vtk()).unwrap_or_else(|_| "null".into()),
            "script" => serde_json::to_string(&session.script()).unwrap_or_else(|_| "null".into()),
            _ => "null".to_string(),
        }
    }

    fn read(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
    }

    fn write(&self, path: &str, text: &str) -> Result<(), String> {
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
    }
}
