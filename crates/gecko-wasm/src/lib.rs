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

    /// The nodes alone, for a command that moved one and made nothing.
    #[wasm_bindgen(js_name = nodesBuffer)]
    pub fn nodes_buffer(&self) -> Vec<u8> {
        self.inner.nodes_buffer()
    }

    /// The geometry, in the same bytes the station sends down its socket.
    ///
    /// Separate from the view because it changes on a different clock: see
    /// `GeometryBuffer`.
    #[wasm_bindgen(js_name = geometryBuffer)]
    pub fn geometry_buffer(&self) -> Vec<u8> {
        self.inner.geometry_buffer()
    }

    /// What the layer through an edge is made of, and the layers beside
    /// it. A question too, and a different one: what is **there**, rather
    /// than what a cut would touch.
    #[wasm_bindgen(js_name = sheetCellsJson)]
    pub fn sheet_cells_json(&self, edge: u32) -> String {
        self.inner.sheet_cells_json(edge)
    }

    /// The sheet through an edge. A question, so it has a reply of its own
    /// rather than going through `applyJson`.
    #[wasm_bindgen(js_name = sheetJson)]
    pub fn sheet_json(&self, edge: u32) -> String {
        self.inner.sheet_json(edge)
    }

    /// Which version this is, from the crate that was built.
    ///
    /// One source: the workspace's own number, which is also what a
    /// release is tagged with. A string in the page would be a second one,
    /// and the two would disagree the first time somebody forgot.
    #[wasm_bindgen(js_name = version)]
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// The groups of blocks: what each is called, whether it is written
    /// into the mesh, and how many blocks are in it.
    #[wasm_bindgen(js_name = groupsJson)]
    pub fn groups_json(&self) -> String {
        self.inner.groups_json()
    }

    /// Which of the blocking's components are patches rather than blocks.
    #[wasm_bindgen(js_name = patchesJson)]
    pub fn patches_json(&self) -> String {
        self.inner.patches_json()
    }

    /// The chord through a face, the same way. The block is which side of
    /// the face was clicked; a chord does not depend on it, and the kernel
    /// still has to be told which block a face belongs to.
    #[wasm_bindgen(js_name = chordJson)]
    pub fn chord_json(&self, face: u32, block: u32) -> String {
        self.inner.chord_json(face, block)
    }

    /// The mesh's skin, in the same bytes the station sends down its
    /// socket. Empty until something has been meshed.
    /// One metric of the mesh, by name — the scaled Jacobian when the name
    /// is empty — or `{"error": …}` for a name that is no metric.
    #[wasm_bindgen(js_name = qualityJson)]
    pub fn quality_json(&self, metric: &str) -> String {
        self.inner.quality_json_named(metric)
    }

    #[wasm_bindgen(js_name = fidelityJson)]
    pub fn fidelity_json(&self, share: f64) -> String {
        self.inner.fidelity_json(share)
    }

    /// The cells whose figure under `metric` is in `[from, to)`, in the
    /// mesh buffer's encoding; empty for a name that is no metric.
    #[wasm_bindgen(js_name = cellsBuffer)]
    pub fn cells_buffer(&self, metric: &str, from: f64, to: f64) -> Vec<u8> {
        match gecko_cmd::Metric::parse(metric) {
            Some(m) => self.inner.cells_buffer(m, from, to),
            None => Vec::new(),
        }
    }

    /// The skin's score under `metric`, a little-endian f32 per quadrangle
    /// of the mesh buffer; empty for a name that is no metric.
    #[wasm_bindgen(js_name = skinScores)]
    pub fn skin_scores(&self, metric: &str) -> Vec<u8> {
        match gecko_cmd::Metric::parse(metric) {
            Some(m) => self.inner.skin_scores(m),
            None => Vec::new(),
        }
    }

    #[wasm_bindgen(js_name = meshBuffer)]
    pub fn mesh_buffer(&self) -> Vec<u8> {
        self.inner.mesh_buffer()
    }

    /// The mesh as a VTK legacy file, or nothing if none has been built.
    #[wasm_bindgen(js_name = meshVtk)]
    pub fn mesh_vtk(&self) -> Option<String> {
        self.inner.mesh_vtk()
    }

    /// The reads a script asks for, each the same JSON the other
    /// transports deliver.
    #[wasm_bindgen(js_name = nodesJson)]
    pub fn nodes_json(&self) -> String {
        self.inner.nodes_json()
    }

    #[wasm_bindgen(js_name = edgesJson)]
    pub fn edges_json(&self) -> String {
        self.inner.edges_json()
    }

    #[wasm_bindgen(js_name = facesJson)]
    pub fn faces_json(&self) -> String {
        self.inner.faces_json()
    }

    #[wasm_bindgen(js_name = blocksJson)]
    pub fn blocks_json(&self) -> String {
        self.inner.blocks_json()
    }

    #[wasm_bindgen(js_name = entitiesJson)]
    pub fn entities_json(&self) -> String {
        self.inner.entities_json()
    }

    /// The geometry described: sizes, incidence and shapes.
    #[wasm_bindgen(js_name = geometryJson)]
    pub fn geometry_json(&self) -> String {
        self.inner.geometry_describe_json()
    }

    /// The block structure written up, with what looks wrong with it.
    #[wasm_bindgen(js_name = blockingJson)]
    pub fn blocking_json(&self) -> String {
        self.inner.blocking_json()
    }

    #[wasm_bindgen(js_name = polylineJson)]
    pub fn polyline_json(&self, tag: i32) -> String {
        self.inner.polyline_json(tag)
    }

    #[wasm_bindgen(js_name = closestJson)]
    pub fn closest_json(&self, arg: &str) -> String {
        self.inner.closest_json(arg)
    }

    /// The session as a Lua script — its history, in the one form that
    /// can be run again.
    #[wasm_bindgen(js_name = script)]
    pub fn script(&self) -> String {
        self.inner.script()
    }

    #[wasm_bindgen(js_name = statsJson)]
    pub fn stats_json(&self) -> String {
        self.inner.stats_json()
    }
}
