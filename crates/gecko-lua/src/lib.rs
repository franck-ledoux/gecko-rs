//! # The scripting API
//!
//! A session, as Lua sees it: one function per command — the same set the
//! other transports carry, because that set **is** the API — plus the
//! reads, and two doors onto files.
//!
//! ```lua
//! gecko.load_geometry("cylinder.msh")
//! gecko.box_geometry{ margin = 0 }
//!
//! for _, node in ipairs(gecko.nodes()) do
//!   if node.pos[3] < 0.5 then
//!     gecko.classify_node(node.id, 1, 201)
//!   end
//! end
//!
//! gecko.mesh{ segments = 8 }
//! gecko.export_vtk("mesh.vtk")
//! ```
//!
//! **Those two paths want a word.** A script reaches files only where
//! there are files to reach: in the terminal, and in a page served by
//! `gecko` on your own machine — where a relative path is relative to the
//! folder `gecko` was started in, and an absolute path is taken as given.
//! A page on a static host has none, so a read says so and a write hands
//! the browser a download. See [`Kernel::read`].
//!
//! **Required things are positional; optional things live in a table at the
//! end.** A command with nothing required takes the table alone, which is
//! why `gecko.mesh{ segments = 8 }` has no brackets: that is Lua's own
//! shorthand for a call with one table.
//!
//! ## One binding, two hosts
//!
//! Lua runs in two places — a terminal, against a session in the same
//! process, and the page, against a session in another wasm module with
//! JavaScript in between. Those are two plumbing jobs and exactly one API,
//! so the plumbing is [`Kernel`], four methods wide, and everything above
//! it is written once.

#[cfg(feature = "local")]
mod local;

#[cfg(feature = "local")]
pub use local::Local;

use std::rc::Rc;

use mlua::serde::SerializeOptions;
use mlua::{Lua, LuaSerdeExt, Table, Value, Variadic};
use serde_json::{json, Map, Value as Json};

/// Everything Lua can ask of the world it is running in.
pub trait Kernel {
    /// A command in, an outcome out — both JSON, exactly as
    /// `Session::apply_json` takes and gives.
    fn apply(&self, command: &str) -> String;

    /// A read, by name: `nodes`, `edges`, `faces`, `blocks`, `stats`, `quality`,
    /// `entities`, `sheet`, `vtk`. `arg` is whatever that read takes, as
    /// JSON, and the answer is JSON.
    fn query(&self, kind: &str, arg: &str) -> String;

    /// A file, if this host has any.
    ///
    /// Where there are files, a **relative path is relative to where the
    /// host was started** — the terminal's working directory, or the
    /// folder `gecko` was launched in — and an absolute path is taken as
    /// given. A page served from anywhere else has no files at all, and
    /// says so rather than pretending: no path of any shape will reach
    /// one, which is worth saying because the obvious guess when a read
    /// fails is that the path was not absolute enough.
    fn read(&self, path: &str) -> Result<String, String>;

    /// The files that can be opened **by name**, without a path.
    ///
    /// In a terminal that is the working directory; in a page it is what
    /// the reader handed over — chosen with the button, or dropped on the
    /// window. A page cannot be given a path, so this is the whole of what
    /// a script there can open, and a script that wants to know has to be
    /// able to ask.
    fn files(&self) -> Vec<String>;
    fn write(&self, path: &str, text: &str) -> Result<(), String>;

    /// Bytes rather than text — a picture. Only where there are files to
    /// write them to: the page passes text to its host and nothing else,
    /// and has a picture button of its own.
    fn write_bytes(&self, path: &str, _bytes: &[u8]) -> Result<(), String> {
        Err(format!(
            "{path}: a picture is written from a terminal, with `gecko run`; \
             in the page, the camera button in the toolbar saves one"
        ))
    }
}

/// Put the `gecko` table into this interpreter.
pub fn open(lua: &Lua, kernel: Rc<dyn Kernel>) -> mlua::Result<()> {
    let gecko = lua.create_table()?;

    // --- making and unmaking blocks -------------------------------------
    command(lua, &gecko, &kernel, "add_block", |lua, a| {
        Ok(json!({ "op": "add_block", "corners": at(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "create_cube", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(json!({
            "op": "create_cube",
            "origin": o.get("origin").cloned().unwrap_or(json!([0, 0, 0])),
            "size": o.get("size").cloned().unwrap_or(json!(1.0)),
        }))
    })?;
    command(lua, &gecko, &kernel, "face_on_surface", |lua, a| {
        Ok(json!({ "op": "face_on_surface", "surface": at(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "add_slab", |lua, a| {
        // The four points, then what each is lying on and how thick — both
        // optional, and `false` for a point that follows nothing.
        let o = options(lua, &a, 1)?;
        Ok(with(
            json!({ "op": "add_slab", "corners": at(lua, &a, 0)? }),
            &o,
            &["on", "thickness"],
        ))
    })?;
    command(lua, &gecko, &kernel, "add_face", |lua, a| {
        // The four points, then what each is lying on and which surface the
        // quadrangle is on — both optional, and `false` for a point that
        // follows nothing.
        let o = options(lua, &a, 1)?;
        Ok(with(
            json!({ "op": "add_face", "corners": at(lua, &a, 0)? }),
            &o,
            &["on", "surface"],
        ))
    })?;
    command(lua, &gecko, &kernel, "remove_face", |lua, a| {
        Ok(json!({ "op": "remove_face", "face": at(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "remove_block", |lua, a| {
        Ok(json!({ "op": "remove_block", "block": at(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "remove_blocks", |lua, a| {
        Ok(json!({ "op": "remove_blocks", "blocks": at(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "pillow_blocks", |lua, a| {
        let o = options(lua, &a, 1)?;
        Ok(with(
            json!({ "op": "pillow_blocks", "blocks": at(lua, &a, 0)? }),
            &o,
            &["outer"],
        ))
    })?;
    command(lua, &gecko, &kernel, "glue_faces", |lua, a| {
        Ok(json!({
            "op": "glue_faces",
            "first": at(lua, &a, 0)?,
            "second": at(lua, &a, 1)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "extrude_patch", |lua, a| {
        Ok(json!({
            "op": "extrude_patch",
            "face": at(lua, &a, 0)?,
            "height": at(lua, &a, 1)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "extrude_face", |lua, a| {
        // Either `(face, block, height)` for one, or `({{f, b}, …}, height)`
        // for several — the same two shapes a click gives and a band would.
        // Several go up together, which is what welds the blocks that meet.
        Ok(match at(lua, &a, 0)? {
            Json::Array(pairs) => json!({
                "op": "extrude_face",
                "faces": pairs
                    .iter()
                    .map(|p| json!({ "face": p[0], "block": p[1] }))
                    .collect::<Vec<Json>>(),
                "height": at(lua, &a, 1)?,
            }),
            face => json!({
                "op": "extrude_face",
                "faces": [{ "face": face, "block": at(lua, &a, 1)? }],
                "height": at(lua, &a, 2)?,
            }),
        })
    })?;
    command(lua, &gecko, &kernel, "pillow", |lua, a| {
        // A list of `{face, block}` pairs, which is what a Lua table of
        // two-element tables comes out as — or of bare faces, whose block
        // the kernel finds.
        let patch: Vec<Json> = match at(lua, &a, 0)? {
            Json::Array(pairs) => pairs
                .iter()
                .map(|p| match p {
                    Json::Array(pair) if pair.len() > 1 => {
                        json!({ "face": pair[0], "block": pair[1] })
                    }
                    Json::Array(pair) => json!({ "face": pair.first() }),
                    face => json!({ "face": face }),
                })
                .collect(),
            _ => Vec::new(),
        };
        let o = options(lua, &a, 1)?;
        Ok(with(
            json!({ "op": "pillow", "faces": patch }),
            &o,
            &["tolerance"],
        ))
    })?;

    command(lua, &gecko, &kernel, "remove_chord", |lua, a| {
        Ok(json!({
            "op": "remove_chord",
            "face": at(lua, &a, 0)?,
            "block": at(lua, &a, 1)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "pillow_chord", |lua, a| {
        let o = options(lua, &a, 2)?;
        Ok(with(
            json!({
                "op": "pillow_chord",
                "face": at(lua, &a, 0)?,
                "block": at(lua, &a, 1)?,
            }),
            &o,
            &["tolerance"],
        ))
    })?;

    command(lua, &gecko, &kernel, "smooth", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(with(json!({ "op": "smooth" }), &o, &["rounds", "share"]))
    })?;

    // --- sheets ----------------------------------------------------------
    command(lua, &gecko, &kernel, "split_sheet", |lua, a| {
        Ok(json!({
            "op": "split_sheet",
            "edge": at(lua, &a, 0)?,
            "at": at(lua, &a, 1)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "remove_sheet", |lua, a| {
        let mut out = json!({ "op": "remove_sheet", "edge": at(lua, &a, 0)? });
        // `gecko.remove_sheet(e, false)` leaves the gap open. Left out, the
        // gap closes, which is what every script written so far says.
        if let Json::Bool(close) = at(lua, &a, 1)? {
            out["close"] = json!(close);
        }
        Ok(out)
    })?;
    command(lua, &gecko, &kernel, "set_segments", |lua, a| {
        Ok(json!({
            "op": "set_segments",
            "edge": at(lua, &a, 0)?,
            "segments": at(lua, &a, 1)?,
        }))
    })?;

    // --- nodes -----------------------------------------------------------
    command(lua, &gecko, &kernel, "move_node", |lua, a| {
        let o = options(lua, &a, 2)?;
        Ok(with(
            json!({ "op": "move_node", "node": at(lua, &a, 0)?, "to": at(lua, &a, 1)? }),
            &o,
            &["carrying", "snap"],
        ))
    })?;
    command(lua, &gecko, &kernel, "classify_node", |lua, a| {
        Ok(json!({
            "op": "classify_node",
            "nodes": list(lua, &a, 0)?,
            "on": { "dim": at(lua, &a, 1)?, "tag": at(lua, &a, 2)? },
        }))
    })?;
    // An edge along two curves, a face across two surfaces: the tag may
    // be a list, and the several are taken as one.
    command(lua, &gecko, &kernel, "classify_edge", |lua, a| {
        Ok(json!({
            "op": "classify_edge",
            "edges": list(lua, &a, 0)?,
            "on": { "dim": at(lua, &a, 1)?, "tag": at(lua, &a, 2)? },
        }))
    })?;
    // `{ down = true }`: the face covers what it is put on, and its corners
    // and sides go on the rim of it.
    command(lua, &gecko, &kernel, "classify_face", |lua, a| {
        let o = options(lua, &a, 3)?;
        Ok(with(
            json!({
                "op": "classify_face",
                "faces": list(lua, &a, 0)?,
                "on": { "dim": at(lua, &a, 1)?, "tag": at(lua, &a, 2)? },
            }),
            &o,
            &["down"],
        ))
    })?;
    // Off the model, and kept off: the settling leaves them on nothing.
    command(lua, &gecko, &kernel, "unclassify_edge", |lua, a| {
        Ok(json!({ "op": "classify_edge", "edges": list(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "unclassify_face", |lua, a| {
        Ok(json!({ "op": "classify_face", "faces": list(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "unclassify_node", |lua, a| {
        Ok(json!({ "op": "classify_node", "nodes": list(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "freeze_node", |lua, a| {
        Ok(json!({ "op": "freeze_node", "nodes": list(lua, &a, 0)?, "frozen": true }))
    })?;
    command(lua, &gecko, &kernel, "thaw_node", |lua, a| {
        Ok(json!({ "op": "freeze_node", "nodes": list(lua, &a, 0)?, "frozen": false }))
    })?;

    // --- the geometry ----------------------------------------------------
    command(lua, &gecko, &kernel, "clear_geometry", |_, _| {
        Ok(json!({ "op": "clear_geometry" }))
    })?;
    command(lua, &gecko, &kernel, "box_geometry", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(with(
            json!({ "op": "box_geometry", "surfaces": o.get("surfaces").cloned().unwrap_or(json!([])) }),
            &o,
            &["margin"],
        ))
    })?;
    // Pieces of the geometry run together, once, for the whole model.
    command(lua, &gecko, &kernel, "merge_curves", |lua, a| {
        Ok(json!({ "op": "merge", "dim": 1, "tags": list(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "merge_surfaces", |lua, a| {
        Ok(json!({ "op": "merge", "dim": 2, "tags": list(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "merge_smooth", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(with(json!({ "op": "merge_smooth" }), &o, &["angle"]))
    })?;
    command(lua, &gecko, &kernel, "classify", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(with(json!({ "op": "classify" }), &o, &["tolerance"]))
    })?;

    // --- meshing ---------------------------------------------------------
    command(lua, &gecko, &kernel, "mesh", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(with(json!({ "op": "mesh" }), &o, &["segments"]))
    })?;
    // --- groups of blocks -------------------------------------------------
    command(lua, &gecko, &kernel, "group", |lua, a| {
        let o = options(lua, &a, 2)?;
        Ok(with(
            json!({ "op": "group", "name": at(lua, &a, 0)?, "blocks": list(lua, &a, 1)? }),
            &o,
            &["written"],
        ))
    })?;
    command(lua, &gecko, &kernel, "ungroup", |lua, a| {
        Ok(json!({
            "op": "ungroup",
            "name": at(lua, &a, 0)?,
            "blocks": list(lua, &a, 1)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "rename_group", |lua, a| {
        Ok(json!({ "op": "rename_group", "name": at(lua, &a, 0)?, "to": at(lua, &a, 1)? }))
    })?;
    command(lua, &gecko, &kernel, "drop_group", |lua, a| {
        Ok(json!({ "op": "drop_group", "name": at(lua, &a, 0)? }))
    })?;

    command(lua, &gecko, &kernel, "reset", |_, _| {
        Ok(json!({ "op": "reset" }))
    })?;
    command(lua, &gecko, &kernel, "clear_mesh", |_, _| {
        Ok(json!({ "op": "clear_mesh" }))
    })?;

    // --- going back ------------------------------------------------------
    command(lua, &gecko, &kernel, "undo", |_, _| {
        Ok(json!({ "op": "undo" }))
    })?;
    command(lua, &gecko, &kernel, "set_history", |lua, a| {
        Ok(json!({ "op": "set_history", "steps": at(lua, &a, 0)? }))
    })?;

    // --- the reads -------------------------------------------------------
    for kind in [
        "nodes", "edges", "faces", "blocks", "groups", "stats", "entities", "script", "vtk",
    ] {
        read(lua, &gecko, &kernel, kind, kind)?;
    }
    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "sheet",
            lua.create_function(move |lua, edge: u32| {
                answer(lua, &host.query("sheet", &edge.to_string()))
            })?,
        )?;
    }

    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "sheet_cells",
            lua.create_function(move |lua, edge: u32| {
                answer(lua, &host.query("sheet_cells", &edge.to_string()))
            })?,
        )?;
    }

    {
        // `gecko.quality()`, `gecko.quality("skewness")`: one metric of the
        // mesh, the scaled Jacobian when none is named. A name that is no
        // metric is an error, which names the ones there are.
        let host = Rc::clone(&kernel);
        gecko.set(
            "quality",
            lua.create_function(move |lua, metric: Option<String>| {
                let json = host.query("quality", metric.as_deref().unwrap_or(""));
                if let Ok(Json::Object(map)) = serde_json::from_str::<Json>(&json) {
                    if let Some(Json::String(error)) = map.get("error") {
                        return Err(mlua::Error::RuntimeError(error.clone()));
                    }
                }
                answer(lua, &json)
            })?,
        )?;
    }

    {
        // `gecko.fidelity{ tolerance = 0.1 }`: the tolerance as a share of
        // the local element size, the kernel's default when left out.
        let host = Rc::clone(&kernel);
        gecko.set(
            "fidelity",
            lua.create_function(move |lua, options: Option<mlua::Table>| {
                let share: Option<f64> = match &options {
                    Some(t) => t.get("tolerance")?,
                    None => None,
                };
                let arg = share.map(|x| x.to_string()).unwrap_or_default();
                answer(lua, &host.query("fidelity", &arg))
            })?,
        )?;
    }

    // The geometry, described: `gecko.geometry()` for every entity's size,
    // incidence and shape; `gecko.polyline(tag)` for a curve's nodes in
    // order; `gecko.closest({x, y, z})` for the nearest corner, curve and
    // surface — or `gecko.closest(p, { dim = 1, tag = 5 })` for one.
    read(lua, &gecko, &kernel, "geometry", "geometry")?;
    // And the block structure the same way: `gecko.blocking()`, every block,
    // node, edge, face and sheet, with what looks wrong with them.
    read(lua, &gecko, &kernel, "blocking", "blocking")?;
    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "polyline",
            lua.create_function(move |lua, tag: i32| {
                answer(lua, &host.query("polyline", &tag.to_string()))
            })?,
        )?;
    }
    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "closest",
            lua.create_function(move |lua, (point, options): (Value, Option<Table>)| {
                let point: Json = lua.from_value(point)?;
                let mut asked = json!({ "point": point });
                if let Some(options) = options {
                    let dim: Option<u8> = options.get("dim")?;
                    let tag: Option<i32> = options.get("tag")?;
                    if let (Some(dim), Some(tag)) = (dim, tag) {
                        asked["dim"] = json!(dim);
                        asked["tag"] = json!(tag);
                    }
                }
                answer(lua, &host.query("closest", &asked.to_string()))
            })?,
        )?;
    }

    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "chord",
            lua.create_function(move |lua, (face, block): (u32, u32)| {
                answer(lua, &host.query("chord", &json!([face, block]).to_string()))
            })?,
        )?;
    }

    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "files",
            lua.create_function(move |lua, ()| lua.to_value(&host.files()))?,
        )?;
    }

    // --- files -----------------------------------------------------------
    //
    // Reading the file here rather than in the kernel, because a command
    // has to be one a browser could also send and a browser has no path to
    // name. The same arrangement the other transports had.
    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "load_geometry",
            lua.create_function(move |_, path: String| {
                let msh = host.read(&path).map_err(mlua::Error::external)?;
                let name = path.rsplit(['/', '\\']).next().unwrap_or(&path);
                let command = json!({ "op": "load_geometry", "msh": msh, "name": name });
                said(&host.apply(&command.to_string()))
            })?,
        )?;
    }
    {
        let host = Rc::clone(&kernel);
        gecko.set(
            "export_vtk",
            lua.create_function(move |_, (path, segments): (String, Option<u32>)| {
                // Mesh first if nothing has been meshed since the last
                // edit, so the file is of the blocking as it now stands.
                if host.query("vtk", "").trim() == "null" {
                    let n = segments.unwrap_or(10);
                    said(&host.apply(&json!({ "op": "mesh", "segments": n }).to_string()))?;
                }
                let file: Json =
                    serde_json::from_str(&host.query("vtk", "")).map_err(mlua::Error::external)?;
                let text = file
                    .as_str()
                    .ok_or_else(|| mlua::Error::external("there is nothing to mesh"))?;
                host.write(&path, text).map_err(mlua::Error::external)?;
                Ok(format!("{} bytes written to {path}", text.len()))
            })?,
        )?;
    }

    {
        // `gecko.render("view.png", { view = "iso", show = {"mesh"}, below = 0.3 })`:
        // a picture of the session, drawn without a browser. The kernel
        // answers with the PNG in base64, since a read answers with text.
        let host = Rc::clone(&kernel);
        gecko.set(
            "render",
            lua.create_function(move |lua, (path, options): (String, Option<Value>)| {
                let options: Json = match options {
                    Some(v) => empty_as_list(lua.from_value(v)?),
                    None => Json::Null,
                };
                let options = match options {
                    Json::Array(a) if a.is_empty() => Json::Null,
                    other => other,
                };
                let reply: Json = serde_json::from_str(&host.query("render", &options.to_string()))
                    .map_err(mlua::Error::external)?;
                if let Some(error) = reply["error"].as_str() {
                    return Err(mlua::Error::RuntimeError(error.to_string()));
                }
                let Some(png) = reply["png"].as_str() else {
                    return Err(mlua::Error::RuntimeError(
                        "a picture is drawn from a terminal, with `gecko run`; in the page, \
                         the camera button in the toolbar saves one"
                            .into(),
                    ));
                };
                let bytes = unbase64(png)
                    .ok_or_else(|| mlua::Error::external("a picture that is not base64"))?;
                host.write_bytes(&path, &bytes)
                    .map_err(mlua::Error::external)?;
                let shown: Vec<&str> = reply["shown"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Json::as_str).collect())
                    .unwrap_or_default();
                let mut said = format!(
                    "{}×{} picture of {} written to {path}",
                    reply["width"],
                    reply["height"],
                    shown.join(", ")
                );
                if let Some(n) = reply["flagged"].as_u64().filter(|n| *n > 0) {
                    said.push_str(&format!("; {n} cells below the threshold drawn over it"));
                }
                Ok(said)
            })?,
        )?;
    }

    lua.globals().set("gecko", gecko)?;
    Ok(())
}

/// Standard base64, padded, back to bytes; nothing when it is not that.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().rev().take_while(|c| **c == b'=').count();
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = (n << 6) | value(c)?;
        }
        n <<= 6 * pad as u32;
        let got = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&got[..3 - pad]);
    }
    Some(out)
}

/// One command, as a Lua function: build the JSON, send it, hand back what
/// the kernel said — or raise what it refused, so that a script stops where
/// it went wrong rather than carrying on over a mistake.
fn command(
    lua: &Lua,
    gecko: &Table,
    kernel: &Rc<dyn Kernel>,
    name: &str,
    build: fn(&Lua, Variadic<Value>) -> mlua::Result<Json>,
) -> mlua::Result<()> {
    let host = Rc::clone(kernel);
    let f = lua.create_function(move |lua, args: Variadic<Value>| {
        said(&host.apply(&build(lua, args)?.to_string()))
    })?;
    gecko.set(name, f)
}

/// What the kernel answered, or its refusal as a Lua error.
fn said(reply: &str) -> mlua::Result<String> {
    let reply: Json = serde_json::from_str(reply).map_err(mlua::Error::external)?;
    if reply["ok"].as_bool() != Some(true) {
        return Err(mlua::Error::external(
            reply["error"].as_str().unwrap_or("refused").to_string(),
        ));
    }
    Ok(reply["outcome"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string())
}

/// One read, as a Lua function of no arguments.
fn read(
    lua: &Lua,
    gecko: &Table,
    kernel: &Rc<dyn Kernel>,
    name: &str,
    kind: &'static str,
) -> mlua::Result<()> {
    let host = Rc::clone(kernel);
    let f = lua.create_function(move |lua, ()| answer(lua, &host.query(kind, "")))?;
    gecko.set(name, f)
}

/// JSON from the kernel, as a Lua table.
///
/// `null` arrives as **nil**, not as a sentinel. A node that is on nothing
/// has `on = nil`, and `if node.on then` is how anybody would write that —
/// a sentinel that is not nil but prints like nothing is a trap.
fn answer(lua: &Lua, json: &str) -> mlua::Result<Value> {
    let value: Json = serde_json::from_str(json).map_err(mlua::Error::external)?;
    lua.to_value_with(
        &value,
        SerializeOptions::new()
            .serialize_none_to_null(false)
            .serialize_unit_to_null(false),
    )
}

/// The n-th positional argument, as JSON.
fn at(lua: &Lua, args: &Variadic<Value>, n: usize) -> mlua::Result<Json> {
    let value: Json = lua.from_value(args.get(n).cloned().unwrap_or(Value::Nil))?;
    Ok(empty_as_list(value))
}

/// An empty table as an empty list, however deep.
///
/// Lua has one kind of table, and an empty one says nothing about whether
/// it was meant as a list or as a set of named fields: it arrives as an
/// empty map. Every empty table a command takes is a list — `group("A",
/// {})`, `surfaces = {}` — and an empty map of options means no options
/// either way, so this is always the right reading. Without it, a script
/// the page wrote, with a group emptied in its history, stopped there.
fn empty_as_list(value: Json) -> Json {
    match value {
        Json::Object(map) if map.is_empty() => Json::Array(Vec::new()),
        Json::Object(map) => Json::Object(
            map.into_iter()
                .map(|(k, v)| (k, empty_as_list(v)))
                .collect(),
        ),
        Json::Array(many) => Json::Array(many.into_iter().map(empty_as_list).collect()),
        other => other,
    }
}

/// The same, as a list: one node or several, written either way.
fn list(lua: &Lua, args: &Variadic<Value>, n: usize) -> mlua::Result<Json> {
    Ok(match at(lua, args, n)? {
        Json::Array(many) => Json::Array(many),
        one => json!([one]),
    })
}

/// The optional table at the end of a call.
fn options(lua: &Lua, args: &Variadic<Value>, n: usize) -> mlua::Result<Map<String, Json>> {
    Ok(match at(lua, args, n)? {
        Json::Object(map) => map,
        _ => Map::new(),
    })
}

/// Those of the named options the command takes, copied onto it. What is
/// not given is left out, so the kernel's own default stands.
fn with(mut command: Json, options: &Map<String, Json>, wanted: &[&str]) -> Json {
    for key in wanted {
        if let Some(value) = options.get(*key) {
            command[*key] = value.clone();
        }
    }
    command
}

#[cfg(test)]
mod tests {
    #[test]
    fn unbase64_reads_what_base64_writes() {
        assert_eq!(super::unbase64("").unwrap(), b"");
        assert_eq!(super::unbase64("Zg==").unwrap(), b"f");
        assert_eq!(super::unbase64("Zm8=").unwrap(), b"fo");
        assert_eq!(super::unbase64("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(super::unbase64("+/8=").unwrap(), [0xfb, 0xff]);
        assert!(super::unbase64("Zm9").is_none());
        assert!(super::unbase64("Zm9*").is_none());
    }
}
