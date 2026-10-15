//! The station: a small server that lets a browser look at the session this
//! Python process is holding.
//!
//! The session lives behind an `Arc<Mutex<…>>` shared by the Python handle and
//! the server's tasks. That sharing is the whole feature — it is what makes
//! "the console and the mouse drive the same live object" literally true,
//! rather than two copies that are kept in step by hand.
//!
//! The server thread never touches the interpreter. It only ever takes the
//! mutex, so there is no GIL to think about on that side.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use gecko_cmd::Session;
use include_dir::{include_dir, Dir};
use tokio::sync::broadcast;

/// The built front-end, baked into the wheel so `show()` has no path to guess.
///
/// `build.rs` makes sure the directory exists, so this crate compiles whether or
/// not the bundle has been built: the binding does not need the viewer, only
/// `show()` does.
static FRONT: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../front/dist");

/// What to say when `show()` is called and no bundle was built.
const NO_BUNDLE: &str = "The viewer bundle is missing. Build it with:\n\n    \
                         ./build-wasm.sh\n    \
                         cd front && npm install && npm run build\n\n\
                         then reinstall this package. The first line is not \
                         optional: the bundle imports the wasm kernel, so \
                         building it without that step fails on a missing \
                         module.";

/// Who made a change. Clients ignore their own, so a tab does not refresh twice
/// for its own command while still hearing about everyone else's.
pub const ORIGIN_PYTHON: u64 = 0;

pub struct Shared {
    pub session: Mutex<Session>,
    pub changed: broadcast::Sender<u64>,
}

impl Shared {
    pub fn new(session: Session) -> Arc<Shared> {
        let (changed, _) = broadcast::channel(16);
        Arc::new(Shared {
            session: Mutex::new(session),
            changed,
        })
    }

    pub fn announce(&self, origin: u64) {
        // An error here only means nobody is listening yet.
        let _ = self.changed.send(origin);
    }
}

/// Hand a URL to the desktop's own opener.
///
/// Deliberately not a crate. The obvious candidate, `webbrowser`, pulls in 39
/// others — `url`, `idna` and the whole `icu` family — to run `open <url>`.
/// Measured, not guessed, and the reason survives the toolchain upgrade that
/// first forced the question.
///
/// It fails quietly on purpose: a browser that will not open is a nuisance, not
/// an error, since `show()` returns the address regardless.
pub fn open_in_browser(url: &str) {
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

/// Start serving, and report the address actually bound.
///
/// Port 0 asks the operating system for a free one: hard-coding a port is how
/// a second session fails to start, or silently attaches to the first.
pub fn serve(shared: Arc<Shared>, port: u16) -> std::io::Result<SocketAddr> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;

    std::thread::Builder::new()
        .name("gecko-station".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).expect("listener handed to tokio");
                let app = Router::new()
                    .route("/mode", get(|| async { "station" }))
                    .route("/ws", get(upgrade))
                    .fallback(get(file))
                    .with_state(shared);
                let _ = axum::serve(listener, app).await;
            });
        })?;

    Ok(address)
}

async fn file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match FRONT.get_file(path) {
        Some(f) => (
            [(
                header::CONTENT_TYPE,
                mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string(),
            )],
            f.contents(),
        )
            .into_response(),
        None if path == "index.html" => {
            (StatusCode::SERVICE_UNAVAILABLE, NO_BUNDLE).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn upgrade(ws: WebSocketUpgrade, State(shared): State<Arc<Shared>>) -> Response {
    ws.on_upgrade(move |socket| talk(socket, shared))
}

/// One browser tab.
///
/// Text frames are JSON carrying a request id; a view reply is a binary frame
/// with that id in its first four bytes. One connection, no second channel to
/// keep in step.
async fn talk(mut socket: WebSocket, shared: Arc<Shared>) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let me = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut changes = shared.changed.subscribe();

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(Message::Text(text))) = incoming else {
                    if matches!(incoming, None | Some(Err(_))) { return; }
                    continue;
                };
                for reply in answer(&shared, me, text.as_str()) {
                    if socket.send(reply).await.is_err() { return; }
                }
            }
            origin = changes.recv() => {
                match origin {
                    // Our own command is already answered; hearing about it
                    // again would make the tab refresh twice.
                    Ok(o) if o == me => {}
                    Ok(_) => {
                        let push = r#"{"kind":"changed"}"#.to_string();
                        if socket.send(Message::Text(push.into())).await.is_err() { return; }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        }
    }
}

fn answer(shared: &Arc<Shared>, me: u64, text: &str) -> Vec<Message> {
    let Ok(request) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![];
    };
    let id = request["id"].as_u64().unwrap_or(0);

    match request["kind"].as_str() {
        Some("apply") => {
            let command = request["command"].to_string();
            let raw = {
                let mut session = shared.session.lock().expect("session mutex");
                session.apply_json(&command)
            };
            let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
            value["id"] = id.into();
            if value["ok"].as_bool() == Some(true) {
                shared.announce(me);
            }
            vec![Message::Text(value.to_string().into())]
        }
        Some("stats") => {
            let stats: serde_json::Value = {
                let session = shared.session.lock().expect("session mutex");
                serde_json::from_str(&session.stats_json()).unwrap_or_default()
            };
            vec![Message::Text(
                serde_json::json!({ "id": id, "stats": stats })
                    .to_string()
                    .into(),
            )]
        }
        Some("vtk") => {
            let vtk = {
                let session = shared.session.lock().expect("session mutex");
                session.mesh_vtk()
            };
            vec![Message::Text(
                serde_json::json!({ "id": id, "vtk": vtk })
                    .to_string()
                    .into(),
            )]
        }
        Some("sheet") => {
            let edge = request["edge"].as_u64().unwrap_or(u64::MAX) as u32;
            let sheet: serde_json::Value = {
                let session = shared.session.lock().expect("session mutex");
                serde_json::from_str(&session.sheet_json(edge)).unwrap_or_default()
            };
            let mut value = sheet;
            value["id"] = id.into();
            vec![Message::Text(value.to_string().into())]
        }
        Some("view") | Some("geometry") | Some("mesh") => {
            let bytes = {
                let session = shared.session.lock().expect("session mutex");
                match request["kind"].as_str() {
                    Some("view") => session.view_buffer(),
                    Some("geometry") => session.geometry_buffer(),
                    _ => session.mesh_buffer(),
                }
            };
            let mut framed = Vec::with_capacity(4 + bytes.len());
            framed.extend_from_slice(&(id as u32).to_le_bytes());
            framed.extend_from_slice(&bytes);
            vec![Message::Binary(framed.into())]
        }
        _ => vec![],
    }
}
