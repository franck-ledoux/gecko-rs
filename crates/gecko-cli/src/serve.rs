//! Serving the page, and lending it the files around it.
//!
//! What this is **not** is a session. The page holds its own — kernel and
//! interpreter both — exactly as it does when served from a static host.
//! All that is added here is a door onto the filesystem, because that is
//! the one thing a page cannot have on its own.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use include_dir::{include_dir, Dir};

use crate::files::Files;

/// The built page, baked into the binary so there is no path to guess and
/// nothing to install beside it.
static FRONT: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../front/dist");

const NO_BUNDLE: &str = "The page was not built into this binary. Build it with:\n\n    \
                         ./build-wasm.sh\n    ./build-lua.sh\n    \
                         cd front && npm install && npm run build\n\n\
                         then build gecko again.";

pub fn start(port: u16, open: bool) -> Result<(), String> {
    let files = Arc::new(Files::here().map_err(|e| e.to_string())?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    runtime.block_on(async move {
        // Port 0 asks the system for a free one: a hard-coded port is how a
        // second gecko fails to start, or quietly attaches to the first.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| format!("port {port}: {e}"))?;
        let at: SocketAddr = listener.local_addr().map_err(|e| e.to_string())?;
        let url = format!("http://{at}/");
        println!("gecko is serving {} at {url}", files.root().display());
        println!("  the session lives in the page; this lends it the files here");
        println!("  ^C to stop");
        if open {
            open_in_browser(&url);
        }
        axum::serve(listener, router(files))
            .await
            .map_err(|e| e.to_string())
    })
}

#[derive(serde::Deserialize)]
struct At {
    path: String,
}

/// Everything the local server answers: the two file doors, and the page
/// itself for anything else.
///
/// Separate from [`start`] so it can be driven without a port, a runtime
/// or a browser, which is how `tests/serving.rs` asks it things.
pub fn router(files: Arc<Files>) -> Router {
    Router::new()
        .route("/file", get(read).put(write))
        .fallback(get(page))
        .with_state(files)
}

/// How long an answer may be kept.
///
/// The page itself: **not at all**. A build gives it the same name every
/// time, so a browser that kept the last one would go on showing the last
/// application — and without this it may, since a reply with no freshness
/// on it is one a browser is allowed to guess about. The port changes from
/// run to run, which usually hides this; usually is not a guarantee, and
/// "I rebuilt and nothing changed" is a bad half-hour.
///
/// Everything under `assets/`: **for ever**. Those names carry a hash of
/// what is in them, so a name that is still being asked for is a file that
/// has not changed.
fn cacheable(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// The header that says this answer came from **here**.
///
/// Without it the page cannot tell a file from a static host's idea of an
/// unknown path: a single-page host answers anything with the page itself
/// and the status 200, so a script asking for `cylinder.msh` on a static
/// host was handed the HTML of the application and told the kernel it was
/// a gmsh mesh. Saying who answered is the only reliable difference, since
/// a missing file's 404 and a static host's 404 are the same 404.
pub const FROM_GECKO: &str = "x-gecko-file";

async fn read(State(files): State<Arc<Files>>, Query(at): Query<At>) -> Response {
    match files.read(&at.path) {
        Some(text) => ([(FROM_GECKO, "1")], text).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            [(FROM_GECKO, "1")],
            "no file of that name",
        )
            .into_response(),
    }
}

async fn write(State(files): State<Arc<Files>>, Query(at): Query<At>, body: String) -> Response {
    match files.write(&at.path, &body) {
        Ok(()) => ([(FROM_GECKO, "1")], "written").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, [(FROM_GECKO, "1")], e).into_response(),
    }
}

async fn page(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match FRONT.get_file(path) {
        Some(f) => (
            [
                (
                    header::CONTENT_TYPE,
                    mime_guess::from_path(path)
                        .first_or_octet_stream()
                        .to_string(),
                ),
                (header::CACHE_CONTROL, cacheable(path).to_string()),
            ],
            f.contents(),
        )
            .into_response(),
        None if path == "index.html" => {
            (StatusCode::SERVICE_UNAVAILABLE, NO_BUNDLE).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Hand a URL to the desktop's own opener.
///
/// Deliberately not a crate. The obvious candidate, `webbrowser`, pulls in
/// thirty-nine others — `url`, `idna` and the whole `icu` family — to run
/// `open <url>`. It fails quietly: a browser that will not open is a
/// nuisance rather than an error, the address having been printed.
fn open_in_browser(url: &str) {
    let (program, first) = if cfg!(target_os = "macos") {
        ("open", None)
    } else if cfg!(target_os = "windows") {
        ("cmd", Some("/C start"))
    } else {
        ("xdg-open", None)
    };
    let mut command = std::process::Command::new(program);
    if let Some(arg) = first {
        command.args(arg.split(' '));
    }
    let _ = command.arg(url).spawn();
}
