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
