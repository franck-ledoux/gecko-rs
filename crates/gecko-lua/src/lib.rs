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

mod local;

pub use local::Local;

use std::rc::Rc;

use mlua::{Lua, LuaSerdeExt, Table, Value, Variadic};
use serde_json::{json, Map, Value as Json};

/// Everything Lua can ask of the world it is running in.
pub trait Kernel {
    /// A command in, an outcome out — both JSON, exactly as
    /// `Session::apply_json` takes and gives.
    fn apply(&self, command: &str) -> String;

    /// A read, by name: `nodes`, `edges`, `faces`, `blocks`, `stats`,
    /// `entities`, `sheet`, `vtk`. `arg` is whatever that read takes, as
    /// JSON, and the answer is JSON.
    fn query(&self, kind: &str, arg: &str) -> String;

    /// A file, if this host has any. The page has none until the local
    /// binary serves it, and says so rather than pretending.
    fn read(&self, path: &str) -> Result<String, String>;
    fn write(&self, path: &str, text: &str) -> Result<(), String>;
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
    command(lua, &gecko, &kernel, "remove_block", |lua, a| {
        Ok(json!({ "op": "remove_block", "block": at(lua, &a, 0)? }))
    })?;
    command(lua, &gecko, &kernel, "glue_faces", |lua, a| {
        Ok(json!({
            "op": "glue_faces",
            "first": at(lua, &a, 0)?,
            "second": at(lua, &a, 1)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "extrude_face", |lua, a| {
        Ok(json!({
            "op": "extrude_face",
            "face": at(lua, &a, 0)?,
            "block": at(lua, &a, 1)?,
            "height": at(lua, &a, 2)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "pillow", |lua, a| {
        // A list of `{face, block}` pairs, which is what a Lua table of
        // two-element tables comes out as.
        let patch: Vec<Json> = match at(lua, &a, 0)? {
            Json::Array(pairs) => pairs
                .iter()
                .map(|p| json!({ "face": p[0], "block": p[1] }))
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

    // --- sheets ----------------------------------------------------------
    command(lua, &gecko, &kernel, "split_sheet", |lua, a| {
        Ok(json!({
            "op": "split_sheet",
            "edge": at(lua, &a, 0)?,
            "at": at(lua, &a, 1)?,
        }))
    })?;
    command(lua, &gecko, &kernel, "remove_sheet", |lua, a| {
        Ok(json!({ "op": "remove_sheet", "edge": at(lua, &a, 0)? }))
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
    command(lua, &gecko, &kernel, "classify", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(with(json!({ "op": "classify" }), &o, &["tolerance"]))
    })?;

    // --- meshing ---------------------------------------------------------
    command(lua, &gecko, &kernel, "mesh", |lua, a| {
        let o = options(lua, &a, 0)?;
        Ok(with(json!({ "op": "mesh" }), &o, &["segments"]))
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
    for kind in ["nodes", "edges", "faces", "blocks", "stats", "entities"] {
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

    lua.globals().set("gecko", gecko)?;
    Ok(())
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
fn answer(lua: &Lua, json: &str) -> mlua::Result<Value> {
    let value: Json = serde_json::from_str(json).map_err(mlua::Error::external)?;
    lua.to_value(&value)
}

/// The n-th positional argument, as JSON.
fn at(lua: &Lua, args: &Variadic<Value>, n: usize) -> mlua::Result<Json> {
    lua.from_value(args.get(n).cloned().unwrap_or(Value::Nil))
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
