//! Lua for the page.
//!
//! The interpreter rides **beside** the kernel rather than inside it: mlua
//! builds for `wasm32-unknown-emscripten` and the kernel is built by
//! wasm-bindgen for `wasm32-unknown-unknown`, and those two cannot be
//! linked into one module. So there are two modules and JavaScript between
//! them — which is exactly the four-method boundary [`gecko_lua::Kernel`]
//! was drawn at, and the reason it was drawn there.
//!
//! Everything above those four methods is the same code the terminal runs.

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::rc::Rc;

use mlua::Lua;

// Answered in JavaScript; see `host.js`. Each returns a string this side
// owns and frees, or null.
extern "C" {
    fn gecko_apply(command: *const c_char) -> *mut c_char;
    fn gecko_query(kind: *const c_char, arg: *const c_char) -> *mut c_char;
    fn gecko_read(path: *const c_char) -> *mut c_char;
    fn gecko_write(path: *const c_char, text: *const c_char) -> *mut c_char;
}

/// A session in the other module, reached through JavaScript.
struct Web;

impl gecko_lua::Kernel for Web {
    fn apply(&self, command: &str) -> String {
        unsafe { answer(gecko_apply(send(command).as_ptr())) }.unwrap_or_default()
    }

    fn query(&self, kind: &str, arg: &str) -> String {
        let (kind, arg) = (send(kind), send(arg));
        unsafe { answer(gecko_query(kind.as_ptr(), arg.as_ptr())) }.unwrap_or("null".to_string())
    }

    fn read(&self, path: &str) -> Result<String, String> {
        // The host answers with the file, or with nothing at all when it
        // has no files to give — a page served from anywhere but the local
        // binary. Saying so beats pretending.
        unsafe { answer(gecko_read(send(path).as_ptr())) }
            .ok_or_else(|| format!("{path}: no file of that name, or no files here at all"))
    }

    fn write(&self, path: &str, text: &str) -> Result<(), String> {
        let (path, text) = (send(path), send(text));
        match unsafe { answer(gecko_write(path.as_ptr(), text.as_ptr())) } {
            Some(trouble) if !trouble.is_empty() => Err(trouble),
            _ => Ok(()),
        }
    }
}

fn send(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}

/// A string JavaScript allocated for us, taken and freed.
unsafe fn answer(from: *mut c_char) -> Option<String> {
    if from.is_null() {
        return None;
    }
    let text = CStr::from_ptr(from).to_string_lossy().into_owned();
    free(from as *mut _);
    Some(text)
}

extern "C" {
    fn free(p: *mut std::ffi::c_void);
}

thread_local! {
    /// One interpreter for the life of the page, so that what a line
    /// defines the next line can use — which is what makes it a console
    /// rather than a series of unrelated scripts.
    static LUA: RefCell<Option<Lua>> = const { RefCell::new(None) };
}

thread_local! {
    /// What `print` wrote while the last chunk ran.
    static PRINTED: RefCell<String> = const { RefCell::new(String::new()) };
}

fn interpreter() -> mlua::Result<()> {
    LUA.with(|cell| {
        if cell.borrow().is_some() {
            return Ok(());
        }
        let lua = Lua::new();
        gecko_lua::open(&lua, Rc::new(Web) as Rc<dyn gecko_lua::Kernel>)?;
        // `print` goes to the console's scrollback rather than to a
        // terminal nobody is looking at.
        let print = lua.create_function(|lua, args: mlua::Variadic<mlua::Value>| {
            let mut line = String::new();
            for (i, value) in args.iter().enumerate() {
                if i > 0 {
                    line.push('\t');
                }
                line.push_str(
                    &lua.coerce_string(value.clone())?
                        .map_or_else(|| format!("{value:?}"), |s| s.to_string_lossy()),
                );
            }
            PRINTED.with(|out| {
                let mut out = out.borrow_mut();
                out.push_str(&line);
                out.push('\n');
            });
            Ok(())
        })?;
        lua.globals().set("print", print)?;
        *cell.borrow_mut() = Some(lua);
        Ok(())
    })
}

/// Run a chunk, and say what happened as JSON.
///
/// # Safety
/// `code` is a NUL-terminated string. The answer is this caller's to free.
#[no_mangle]
pub unsafe extern "C" fn lua_run(code: *const c_char) -> *mut c_char {
    let source = CStr::from_ptr(code).to_string_lossy().into_owned();
    let answer = match run(&source) {
        Ok(value) => serde_json::json!({ "ok": true, "value": value, "out": taken() }),
        Err(e) => serde_json::json!({ "ok": false, "error": e.to_string(), "out": taken() }),
    };
    CString::new(answer.to_string())
        .unwrap_or_default()
        .into_raw()
}

fn taken() -> String {
    PRINTED.with(|out| std::mem::take(&mut *out.borrow_mut()))
}

/// A chunk's value, if it has one.
///
/// Tried as an expression first, so that typing `gecko.stats()` shows the
/// answer rather than nothing — the thing every console does and nobody
/// thinks about until it is missing.
fn run(source: &str) -> mlua::Result<Option<String>> {
    interpreter()?;
    LUA.with(|cell| {
        let borrowed = cell.borrow();
        let lua = borrowed.as_ref().expect("an interpreter");
        let value: mlua::Value = match lua.load(format!("return {source}")).eval() {
            Ok(value) => value,
            Err(_) => lua.load(source).eval()?,
        };
        Ok(match value {
            mlua::Value::Nil => None,
            other => Some(show(lua, &other, 0)),
        })
    })
}

/// A value, as a console prints one: tables opened out one level deep,
/// because a table is what most of the reads answer with and `table:
/// 0x…` tells nobody anything.
fn show(lua: &Lua, value: &mlua::Value, depth: usize) -> String {
    match value {
        mlua::Value::Table(t) if depth < 2 => {
            let mut parts: Vec<String> = Vec::new();
            for pair in t.clone().pairs::<mlua::Value, mlua::Value>().flatten() {
                let (key, item) = pair;
                let shown = show(lua, &item, depth + 1);
                match key {
                    mlua::Value::Integer(_) => parts.push(shown),
                    other => parts.push(format!(
                        "{} = {shown}",
                        lua.coerce_string(other)
                            .ok()
                            .flatten()
                            .map_or_else(|| "?".to_string(), |s| s.to_string_lossy())
                    )),
                }
                if parts.len() == 8 {
                    parts.push("…".to_string());
                    break;
                }
            }
            format!("{{{}}}", parts.join(", "))
        }
        mlua::Value::String(s) => s.to_string_lossy(),
        other => lua
            .coerce_string(other.clone())
            .ok()
            .flatten()
            .map_or_else(|| format!("{other:?}"), |s| s.to_string_lossy()),
    }
}

fn main() {}
