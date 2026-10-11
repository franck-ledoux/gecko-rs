//! Python façade.
//!
//! Compare it with `gecko-wasm/src/lib.rs`: the same methods, in the same
//! order, with the same absence of logic. Both are transports onto
//! `gecko_cmd::Session`. Anything that has to be *implemented* in a façade
//! belongs in `gecko-cmd` instead — that rule is what keeps the two modes from
//! drifting, and it is what the cross-mode test checks.
//!
//! What this one has that the browser's does not is [`Session::show`]: the
//! session it holds can also be looked at, live, in a page.

mod station;

use std::sync::Arc;

use gecko_cmd::Session as Inner;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use serde_json::Value;
use station::{Shared, ORIGIN_PYTHON};

/// serde_json::Value -> native Python objects, so callers get real dicts and
/// lists rather than JSON strings to re-parse.
fn to_py(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    Ok(match value {
        Value::Null => py.None(),
        Value::Bool(b) => b.into_pyobject(py)?.to_owned().into_any().unbind(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into_pyobject(py)?.into_any().unbind(),
            None => n
                .as_f64()
                .unwrap_or(f64::NAN)
                .into_pyobject(py)?
                .into_any()
                .unbind(),
        },
        Value::String(s) => s.into_pyobject(py)?.into_any().unbind(),
        Value::Array(a) => {
            let list = PyList::empty(py);
            for item in a {
                list.append(to_py(py, item)?)?;
            }
            list.into_any().unbind()
        }
        Value::Object(o) => {
            let dict = PyDict::new(py);
            for (k, v) in o {
                dict.set_item(k, to_py(py, v)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

#[pyclass(name = "Session")]
pub struct PySession {
    shared: Arc<Shared>,
    address: Option<String>,
}

#[pymethods]
impl PySession {
    #[new]
    fn new() -> PyResult<Self> {
        let inner = Inner::new().map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PySession {
            shared: Shared::new(inner),
            address: None,
        })
    }

    /// Add one hexahedral block from its eight corner points, in the usual
    /// VTK/Gmsh order.
    fn add_block(&self, corners: [[f64; 3]; 8]) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "add_block", "corners": corners }))
    }

    /// An axis-aligned cube, `origin` being its lowest corner.
    #[pyo3(signature = (origin=[0.0, 0.0, 0.0], size=1.0))]
    fn create_cube(&self, origin: [f64; 3], size: f64) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "create_cube", "origin": origin, "size": size }))
    }

    /// Move one node.
    ///
    /// `node` is the stable id from [`Session::nodes`], not a position in that
    /// list: the list is reordered by edits, the id is not.
    fn move_node(&self, node: u32, to: [f64; 3]) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "move_node", "node": node, "to": to }))
    }

    /// Weld two blocks along a face.
    ///
    /// `first` stays put and `second` is brought onto it, so the order
    /// decides where the seam ends up. Only the face moves: the rest of the
    /// second block stays where it is.
    fn glue_faces(&self, first: u32, second: u32) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "glue_faces", "first": first, "second": second }))
    }

    /// Cut the whole sheet of hexahedra through an edge, `at` along it.
    ///
    /// `at` runs the way [`Session::sheet`] reports, so it means the same
    /// side of the sheet on every edge of it, and has to fall strictly
    /// between the ends.
    fn split_sheet(&self, edge: u32, at: f64) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "split_sheet", "edge": edge, "at": at }))
    }

    /// Take out the whole sheet of blocks through an edge.
    ///
    /// Whatever was glued to either end of the layer ends up glued to the
    /// other, with the seam half way between. A cap on the model's surface
    /// has nothing to close against, so the model simply loses that layer.
    fn remove_sheet(&self, edge: u32) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "remove_sheet", "edge": edge }))
    }

    /// Lay a layer of hexahedra along a patch of surface faces.
    ///
    /// `patch` is a list of `(face, block)` pairs: a face, and which of its
    /// blocks it belongs to. The blocks are drawn in and the layer fills the
    /// space, so the model's surface does not move. Faces inside the model
    /// are refused.
    fn pillow(&self, patch: Vec<(u32, u32)>) -> PyResult<String> {
        let faces: Vec<_> = patch
            .iter()
            .map(|(face, block)| serde_json::json!({ "face": face, "block": block }))
            .collect();
        self.command(serde_json::json!({ "op": "pillow", "faces": faces }))
    }

    /// Take one block away.
    ///
    /// Whatever it was glued to keeps its own cells: the shared ones are
    /// split off on the way out rather than going with it.
    fn remove_block(&self, block: u32) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "remove_block", "block": block }))
    }

    /// Grow a new block out of a face, along the normal at its centre.
    ///
    /// `face` and `block` are the pair a click gives: a face, and which of
    /// its blocks it belongs to. The face does not move — the new block is
    /// laid against it and welded on. A face inside the model is refused;
    /// it has a block on each side already.
    fn extrude_face(&self, face: u32, block: u32, height: f64) -> PyResult<String> {
        self.command(serde_json::json!({
            "op": "extrude_face", "face": face, "block": block, "height": height,
        }))
    }

    /// Read a geometry from a gmsh surface mesh.
    ///
    /// MSH 2.2 ASCII, triangles for surfaces, lines for curves and point
    /// elements for corners, each grouped by its **elementary entity tag**.
    /// Anything else in the file — tetrahedra, almost always — is skipped
    /// and counted in the message.
    ///
    /// The path is read here and the contents go into the command, because
    /// the command set has to be the same one the browser can send and a
    /// browser has no path to give.
    fn load_geometry(&self, path: &str) -> PyResult<String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| PyValueError::new_err(format!("{path}: {e}")))?;
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        self.command(serde_json::json!({ "op": "load_geometry", "msh": text, "name": name }))
    }

    /// Forget the geometry.
    fn clear_geometry(&self) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "clear_geometry" }))
    }

    /// Put every node on the geometric entity nearest it, and every edge
    /// on what its two ends agree about.
    ///
    /// Nodes by proximity, dimension by dimension, stopping at the lowest
    /// dimension with anything within `tolerance` — a share of the
    /// geometry's own size, not a length. Edges never by proximity: an
    /// edge goes on the lowest-dimensional entity containing **both** its
    /// ends' classifications, which is the only rule that keeps it
    /// coherent with its own boundary.
    ///
    /// Frozen nodes are left alone, classification included.
    #[pyo3(signature = (tolerance=0.02))]
    fn classify(&self, tolerance: f64) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "classify", "tolerance": tolerance }))
    }

    /// Put one node on a geometric entity: `dim` 0 a corner, 1 a curve, 2
    /// a surface, and `tag` the gmsh entity tag. The node is then pulled
    /// onto it.
    fn classify_node(&self, node: u32, dim: u8, tag: i32) -> PyResult<String> {
        self.command(serde_json::json!({
            "op": "classify_node", "node": node, "on": { "dim": dim, "tag": tag },
        }))
    }

    /// Take a node off whatever it was classified on. It stays where it
    /// is; what changes is that it may now go anywhere.
    fn unclassify_node(&self, node: u32) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "classify_node", "node": node }))
    }

    /// Stop a node moving. It stays put for every operation, this one's
    /// own projection included, until [`thaw_node`] lets it go.
    fn freeze_node(&self, node: u32) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "freeze_node", "node": node, "frozen": true }))
    }

    fn thaw_node(&self, node: u32) -> PyResult<String> {
        self.command(serde_json::json!({ "op": "freeze_node", "node": node, "frozen": false }))
    }

    /// Add one block: a box round the geometry, or round part of it.
    ///
    /// `surfaces` is a list of gmsh entity tags, or `None` for the lot.
    /// `margin` grows each half-side by that share; 0 rests the box
    /// exactly on the geometry.
    #[pyo3(signature = (surfaces=None, margin=0.1))]
    fn box_geometry(&self, surfaces: Option<Vec<i32>>, margin: f64) -> PyResult<String> {
        self.command(serde_json::json!({
            "op": "box_geometry",
            "surfaces": surfaces.unwrap_or_default(),
            "margin": margin,
        }))
    }

    /// The sheet of hexahedra through an edge: every edge of it, with the
    /// direction a cut parameter would run in.
    fn sheet(&self, py: Python<'_>, edge: u32) -> PyResult<PyObject> {
        let json = self.locked(|s| s.sheet_json(edge));
        let value: Value =
            serde_json::from_str(&json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        if value["ok"].as_bool() != Some(true) {
            return Err(PyValueError::new_err(
                value["error"]
                    .as_str()
                    .unwrap_or("no such sheet")
                    .to_string(),
            ));
        }
        to_py(py, &value["edges"])
    }

    /// Every block, as `{"id": …, "centre": [x, y, z]}` — what a script needs
    /// in order to have something to remove.
    #[getter]
    fn blocks(&self, py: Python<'_>) -> PyResult<PyObject> {
        let json = self.locked(|s| s.blocks_json());
        let value: Value =
            serde_json::from_str(&json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        to_py(py, &value)
    }

    /// Every face, as `{"id": …, "block": …, "centre": [x, y, z]}` — what a
    /// script needs in order to have something to glue. A face with a block
    /// on each side is listed once per block.
    #[getter]
    fn faces(&self, py: Python<'_>) -> PyResult<PyObject> {
        let json = self.locked(|s| s.faces_json());
        let value: Value =
            serde_json::from_str(&json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        to_py(py, &value)
    }

    /// Every node, as `{"id": …, "pos": [x, y, z]}` — what a script needs in
    /// order to have something to move.
    /// Every entity of the loaded geometry, by dimension and tag — what
    /// you name to `classify_node`.
    ///
    /// ```python
    /// >>> s.entities[:2]
    /// [{'dim': 0, 'tag': 1}, {'dim': 0, 'tag': 2}]
    /// ```
    #[getter]
    fn entities(&self, py: Python<'_>) -> PyResult<PyObject> {
        let json = self.locked(|s| s.entities_json());
        to_py(py, &serde_json::from_str(&json).unwrap_or_default())
    }

    /// Every edge, by stable name, with its two ends and what it is
    /// classified on.
    #[getter]
    fn edges(&self, py: Python<'_>) -> PyResult<PyObject> {
        let json = self.locked(|s| s.edges_json());
        to_py(py, &serde_json::from_str(&json).unwrap_or_default())
    }

    #[getter]
    fn nodes(&self, py: Python<'_>) -> PyResult<PyObject> {
        let json = self.locked(|s| s.nodes_json());
        let value: Value =
            serde_json::from_str(&json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        to_py(py, &value)
    }

    /// Send a raw command. The same door the browser uses, exposed so a script
    /// can reach anything the typed methods have not caught up with yet.
    fn apply(&self, py: Python<'_>, command: &str) -> PyResult<PyObject> {
        let value: Value =
            serde_json::from_str(command).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let message = self.command(value)?;
        to_py(py, &Value::String(message))
    }

    #[getter]
    fn stats(&self, py: Python<'_>) -> PyResult<PyObject> {
        let json = self.locked(|s| s.stats_json());
        let value: Value =
            serde_json::from_str(&json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        to_py(py, &value)
    }

    /// The view buffer: the exact bytes the browser decodes, in either mode.
    /// The geometry, in the bytes the browser decodes.
    #[getter]
    fn geometry_buffer(&self) -> Vec<u8> {
        self.locked(|s| s.geometry_buffer())
    }

    #[getter]
    fn view_buffer(&self) -> Vec<u8> {
        self.locked(|s| s.view_buffer())
    }

    /// Serve this session to a browser, and return the address.
    ///
    /// Idempotent: calling it again gives the same address rather than starting
    /// a second server on the same session.
    #[pyo3(signature = (port=0, open=true))]
    fn show(&mut self, port: u16, open: bool) -> PyResult<String> {
        if let Some(url) = &self.address {
            return Ok(url.clone());
        }
        let bound = station::serve(self.shared.clone(), port)
            .map_err(|e| PyRuntimeError::new_err(format!("could not start the station: {e}")))?;
        let url = format!("http://{bound}/");
        if open {
            station::open_in_browser(&url);
        }
        self.address = Some(url.clone());
        Ok(url)
    }

    /// The address the station is serving on, or `None` if `show()` has not
    /// been called.
    #[getter]
    fn url(&self) -> Option<String> {
        self.address.clone()
    }

    fn __repr__(&self) -> String {
        let stats = self.locked(|s| s.stats_json());
        match &self.address {
            Some(url) => format!("<gecko.Session {stats} at {url}>"),
            None => format!("<gecko.Session {stats}>"),
        }
    }
}

impl PySession {
    fn locked<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        let mut session = self.shared.session.lock().expect("session mutex");
        f(&mut session)
    }

    /// Apply a command and tell any watching browser that the view moved.
    fn command(&self, value: Value) -> PyResult<String> {
        let raw = self.locked(|s| s.apply_json(&value.to_string()));
        let reply: Value =
            serde_json::from_str(&raw).map_err(|e| PyValueError::new_err(e.to_string()))?;
        if reply["ok"].as_bool() != Some(true) {
            return Err(PyValueError::new_err(
                reply["error"]
                    .as_str()
                    .unwrap_or("command refused")
                    .to_string(),
            ));
        }
        self.shared.announce(ORIGIN_PYTHON);
        Ok(reply["outcome"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}

#[pymodule]
fn gecko(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySession>()?;
    m.add(
        "__doc__",
        "Block toolkit: Rust kernel, Python API, browser viewer.",
    )?;
    Ok(())
}
