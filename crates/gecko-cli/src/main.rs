//! # gecko, on this machine
//!
//! Two jobs, and they are deliberately not the same session:
//!
//! - `gecko` serves the page and opens it. The session lives **in the
//!   page**, as it does wherever the page is served from; what this adds is
//!   files, which a page has none of.
//! - `gecko run script.lua` runs a script with no browser at all, for a
//!   parameter sweep or a build step.
//!
//! The same Lua API either way — one binding, in `gecko-lua`, over a trait
//! four methods wide — so a script that works in the console works here.

mod files;
mod serve;

use std::process::ExitCode;
use std::rc::Rc;

const USAGE: &str = "\
gecko — a block toolkit

    gecko                    serve the page and open it
    gecko run <script.lua>   run a script, with no browser
    gecko --help

    --port <n>    serve on this port rather than one the system picks
    --no-open     serve, but do not open a browser
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);

    if flag("--help") || flag("-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    if args.first().is_some_and(|a| a == "run") {
        let Some(script) = args.get(1) else {
            eprintln!("gecko run: which script?\n\n{USAGE}");
            return ExitCode::FAILURE;
        };
        return match run(script) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if let Some(unknown) = args
        .iter()
        .find(|a| !a.starts_with("--") && !args.windows(2).any(|w| w[0] == "--port" && &w[1] == *a))
    {
        eprintln!("gecko: what is {unknown}?\n\n{USAGE}");
        return ExitCode::FAILURE;
    }

    let port = args
        .windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(0);
    match serve::start(port, !flag("--no-open")) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gecko: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Run a script against a session of this process's own.
fn run(script: &str) -> Result<(), String> {
    let source = std::fs::read_to_string(script).map_err(|e| format!("{script}: {e}"))?;
    let lua = mlua::Lua::new();
    let kernel = gecko_lua::Local::new().map_err(|e| e.to_string())?;
    gecko_lua::open(&lua, kernel as Rc<dyn gecko_lua::Kernel>).map_err(|e| e.to_string())?;
    lua.load(&source)
        .set_name(script)
        .exec()
        .map_err(|e| e.to_string())
}
