//! Every script in `tests/scripts` is a test.
//!
//! The suite is written in Lua on purpose: what is being tested is the API
//! a reader will write against, so the test *is* an example of writing
//! against it. A script asserts with Lua's own `assert`, and a failure
//! arrives with the file and the line.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gecko_lua::Local;

/// Where a script may write, and where its fixtures are.
fn harness(lua: &mlua::Lua, here: &Path) -> mlua::Result<()> {
    let fixtures = here.join("fixtures");
    let scratch = std::env::temp_dir().join("gecko-lua-tests");
    std::fs::create_dir_all(&scratch).ok();
    lua.globals()
        .set("FIXTURES", fixtures.to_string_lossy().to_string())?;
    lua.globals()
        .set("SCRATCH", scratch.to_string_lossy().to_string())?;
    Ok(())
}

fn run(script: &Path, here: &Path) -> mlua::Result<()> {
    let lua = mlua::Lua::new();
    gecko_lua::open(
        &lua,
        Local::new().expect("a session") as Rc<dyn gecko_lua::Kernel>,
    )?;
    harness(&lua, here)?;
    let source = std::fs::read_to_string(script).expect("the script");
    lua.load(&source)
        .set_name(script.file_name().unwrap().to_string_lossy())
        .exec()
}

#[test]
fn every_script_runs() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(here.join("scripts"))
        .expect("the scripts")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "lua"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no scripts to run");

    let mut failed = Vec::new();
    for script in &scripts {
        if let Err(e) = run(script, &here) {
            failed.push(format!("{}: {e}", script.display()));
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n\n"));
}
