//! Browser façade, for the statically deployed mode.
//!
//! Note how little is here. It owns no state and makes no decisions: JSON
//! commands in, buffers out. The PyO3 façade next door is this same file with
//! different attribute macros. Anything that has to be *implemented* in a
//! façade belongs in `gecko-cmd` instead — that is the rule that keeps the two
//! modes from drifting.

use gecko_cmd::Session;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    // Rust panics become readable JS stack traces rather than "unreachable".
    console_error_panic_hook::set_once();
}

#[wasm_bindgen(js_name = Session)]
pub struct WasmSession {
    inner: Session,
}

#[wasm_bindgen(js_class = Session)]
impl WasmSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<WasmSession, JsError> {
        Session::new()
            .map(|inner| WasmSession { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Apply one command. Returns `{ok, outcome}` or `{ok:false, error}`.
    #[wasm_bindgen(js_name = applyJson)]
    pub fn apply_json(&mut self, json: &str) -> String {
        self.inner.apply_json(json)
    }

    /// The view, in the same bytes the station sends down its socket.
    #[wasm_bindgen(js_name = viewBuffer)]
    pub fn view_buffer(&self) -> Vec<u8> {
        self.inner.view_buffer()
    }

    /// The geometry, in the same bytes the station sends down its socket.
    ///
    /// Separate from the view because it changes on a different clock: see
    /// `GeometryBuffer`.
    #[wasm_bindgen(js_name = geometryBuffer)]
    pub fn geometry_buffer(&self) -> Vec<u8> {
        self.inner.geometry_buffer()
    }

    /// The sheet through an edge. A question, so it has a reply of its own
    /// rather than going through `applyJson`.
    #[wasm_bindgen(js_name = sheetJson)]
    pub fn sheet_json(&self, edge: u32) -> String {
        self.inner.sheet_json(edge)
    }

    #[wasm_bindgen(js_name = statsJson)]
    pub fn stats_json(&self) -> String {
        self.inner.stats_json()
    }
}
