//! `gecko run --json`: a run, written up for a program to read.
//!
//! A script run for a person prints what it likes and stops on the first
//! error. A script run by another program — a test bench, an agent sweeping
//! models — wants one answer it can parse whatever happened: whether the run
//! got to the end, what it printed, what it returned, and where the session
//! stood when it stopped. Reading that back out of `print` lines meant every
//! caller writing its own formatting and its own parser, and a script that
//! failed half way said nothing about the half it did.
//!
//! So the report is **one JSON object on stdout**, and nothing else is: what
//! the script prints is captured into the report rather than mixed with it.
//! It is written even when the script fails — `ok` is false and `error` says
//! why — because the state at the failure is exactly what is worth having.
//!
//! The mesh is measured under every metric, and against the geometry, if
//! there are both: those are the questions anybody judging a run asks, and
//! asking them is cheap next to building the mesh.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gecko_lua::{Kernel, Local};
use mlua::serde::de::Options as DeserializeOptions;
use mlua::{Lua, LuaSerdeExt, Value, Variadic};
use serde_json::{json, Value as Json};

/// Run `source` against a fresh session and write the run up.
///
/// `name` is what the script is called in error messages and in the
/// report. Never fails: a script that does is a report that says so.
pub fn run(name: &str, source: &str) -> Json {
    let started = Instant::now();
    let printed = Rc::new(RefCell::new(Printed::default()));

    let kernel = match Local::new() {
        Ok(kernel) => kernel,
        Err(e) => return failed(name, &e.to_string(), started),
    };
    let lua = Lua::new();
    let outcome = open(&lua, &kernel, &printed).and_then(|()| {
        let values = lua.load(source).set_name(name).eval::<mlua::MultiValue>()?;
        Ok(values.into_iter().next().map(|v| returned(&lua, v)))
    });

    let (ok, error, returned) = match outcome {
        Ok(value) => (true, Json::Null, value.unwrap_or(Json::Null)),
        Err(e) => (false, json!(e.to_string()), Json::Null),
    };
    let printed = printed.borrow_mut().take();
    let measured = measure(&kernel);
    let mut report = json!({
        "gecko": env!("CARGO_PKG_VERSION"),
        "script": name,
        "ok": ok,
        "error": error,
        "printed": printed,
        "returned": returned,
        "seconds": started.elapsed().as_secs_f64(),
    });
    for (key, value) in measured {
        report[key] = value;
    }
    report
}

/// The session as the run left it: counts, the mesh under every metric,
/// and the mesh against the geometry. `null` where there is nothing to say
/// — no mesh, or no geometry to hold it against.
fn measure(kernel: &Rc<Local>) -> Vec<(&'static str, Json)> {
    let read = |kind: &str, arg: &str| -> Json {
        serde_json::from_str(&kernel.query(kind, arg)).unwrap_or(Json::Null)
    };
    let stats = read("stats", "");
    let meshed = stats["mesh"]["hexes"].is_number();
    let quality = if meshed {
        json!({
            "scaled_jacobian": read("quality", "scaled_jacobian"),
            "skewness": read("quality", "skewness"),
            "aspect_ratio": read("quality", "aspect_ratio"),
        })
    } else {
        Json::Null
    };
    // `null` by itself without a mesh or a geometry. Less its overlay: that
    // is the page's drawing of what was missed, points by the thousand, and
    // the curves and corners it draws are named in the rest already.
    let mut fidelity = read("fidelity", "");
    if let Json::Object(map) = &mut fidelity {
        map.remove("overlay");
    }
    vec![
        ("stats", stats),
        // Written whatever happened: when a script stops half way, the
        // blocking it got to is what says why.
        ("blocking", read("blocking", "")),
        ("quality", quality),
        ("fidelity", fidelity),
    ]
}

/// A report for a run that never started.
fn failed(name: &str, error: &str, started: Instant) -> Json {
    json!({
        "gecko": env!("CARGO_PKG_VERSION"),
        "script": name,
        "ok": false,
        "error": error,
        "printed": [],
        "returned": null,
        "seconds": started.elapsed().as_secs_f64(),
        "stats": null,
        "blocking": null,
        "quality": null,
        "fidelity": null,
    })
}

/// What the script returned, as JSON. A value JSON has no word for — a
/// function, a coroutine — is described rather than dropped, so a caller
/// can tell "returned nothing" from "returned something unprintable".
fn returned(lua: &Lua, value: Value) -> Json {
    let options = DeserializeOptions::new().deny_unsupported_types(false);
    match lua.from_value_with::<Json>(value.clone(), options) {
        Ok(Json::Null) if !value.is_nil() => json!(format!("<{}>", value.type_name())),
        Ok(json) => json,
        Err(_) => json!(format!("<{}>", value.type_name())),
    }
}

/// Lines `print` and `io.write` wrote, kept rather than sent to stdout,
/// which belongs to the report.
#[derive(Default)]
struct Printed {
    lines: Vec<String>,
    /// What `io.write` has written since the last newline.
    pending: String,
}

impl Printed {
    fn write(&mut self, text: &str) {
        self.pending.push_str(text);
        while let Some(at) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=at).collect();
            self.lines.push(line.trim_end_matches('\n').to_string());
        }
    }

    fn take(&mut self) -> Vec<String> {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            self.lines.push(rest);
        }
        std::mem::take(&mut self.lines)
    }
}

/// The `gecko` table, and `print` and `io.write` turned towards the report.
fn open(lua: &Lua, kernel: &Rc<Local>, printed: &Rc<RefCell<Printed>>) -> mlua::Result<()> {
    gecko_lua::open(lua, Rc::clone(kernel) as Rc<dyn Kernel>)?;

    let into = Rc::clone(printed);
    let print = lua.create_function(move |lua, args: Variadic<Value>| {
        let mut line = String::new();
        for (i, value) in args.iter().enumerate() {
            if i > 0 {
                line.push('\t');
            }
            line.push_str(&shown(lua, value)?);
        }
        line.push('\n');
        into.borrow_mut().write(&line);
        Ok(())
    })?;
    lua.globals().set("print", print)?;

    let into = Rc::clone(printed);
    let write = lua.create_function(move |lua, args: Variadic<Value>| {
        for value in args.iter() {
            into.borrow_mut().write(&shown(lua, value)?);
        }
        Ok(())
    })?;
    let io: mlua::Table = lua.globals().get("io")?;
    io.set("write", write)?;
    Ok(())
}

/// A value as `print` would show it.
fn shown(lua: &Lua, value: &Value) -> mlua::Result<String> {
    let text: mlua::LuaString = lua
        .globals()
        .get::<mlua::Function>("tostring")?
        .call(value.clone())?;
    Ok(text.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_meshes_is_measured() {
        let report = run(
            "cube.lua",
            "gecko.create_cube{ origin = {0,0,0}, size = 1 }\n\
             gecko.mesh{ segments = 2 }\n\
             print('meshed', 8)\n\
             io.write('half ') io.write('a line\\n')\n\
             return { hexes = gecko.stats().mesh.hexes }",
        );
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(report["error"], Json::Null);
        assert_eq!(report["printed"], json!(["meshed\t8", "half a line"]));
        assert_eq!(report["returned"], json!({ "hexes": 8 }));
        assert_eq!(report["stats"]["blocks"], 1);
        let sj = &report["quality"]["scaled_jacobian"];
        assert_eq!(sj["min"], 1.0);
        assert_eq!(sj["inverted"], 0);
        assert_eq!(report["quality"]["aspect_ratio"]["metric"], "aspect_ratio");
        // No geometry, so nothing to be faithful to.
        assert_eq!(report["fidelity"], Json::Null);
    }

    #[test]
    fn a_run_that_fails_still_says_where_it_got_to() {
        let report = run(
            "broken.lua",
            "gecko.create_cube{}\nprint('before')\ngecko.remove_block(424242)\nprint('after')",
        );
        assert_eq!(report["ok"], false);
        assert!(report["error"].as_str().unwrap().contains("broken.lua"));
        assert_eq!(report["printed"], json!(["before"]));
        assert_eq!(report["stats"]["blocks"], 1, "the state at the failure");
        // And the blocking it got to, whole.
        assert_eq!(report["blocking"]["blocks"].as_array().unwrap().len(), 1);
        assert_eq!(report["blocking"]["nodes"].as_array().unwrap().len(), 8);
        assert_eq!(report["quality"], Json::Null);
    }

    #[test]
    fn a_returned_function_is_named_rather_than_lost() {
        let report = run("f.lua", "return print");
        assert_eq!(report["returned"], "<function>");
        let report = run("nil.lua", "return nil");
        assert_eq!(report["returned"], Json::Null);
    }
}
