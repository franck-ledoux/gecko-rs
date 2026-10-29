//! What the local server answers, driven without a port or a browser.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gecko_cli::{files::Files, serve};
use std::sync::Arc;
use tower::ServiceExt;

async fn ask(path: &str) -> axum::http::Response<Body> {
    let files = Arc::new(Files::here().expect("a folder"));
    serve::router(files)
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("an answer")
}

async fn body(answer: axum::http::Response<Body>) -> String {
    String::from_utf8(
        http_body_util::BodyExt::collect(answer.into_body())
            .await
            .expect("a body")
            .to_bytes()
            .to_vec(),
    )
    .expect("utf-8")
}

// Whether the page is baked into this test binary depends on whether
// `front/dist` existed when it was compiled, and that is a build product:
// `cargo test` on its own does not make it. So the next two tests cover one
// world each and ask which one they are in, rather than one of them failing
// wherever the other one belongs.

#[tokio::test]
async fn the_page_is_never_kept_and_its_assets_are_kept_for_ever() {
    if !serve::has_page() {
        return; // The test below is the one with something to say here.
    }

    // The half-hour this is here to save: rebuild, reload, and see the
    // application you had before. A build gives the page the same name
    // every time, so a browser allowed to guess about freshness may go on
    // showing the last one — and the names under `assets/` carry a hash
    // of their contents, so one still being asked for cannot have
    // changed.
    let page = ask("/").await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(
        page.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-cache"),
    );

    // Whatever the bundle is called today — the name carries a hash of
    // its contents — the page says so, so the test reads it from there
    // rather than guessing.
    let html = body(page).await;
    let at = html.find("assets/").expect("the page loads a bundle");
    let name: String = html[at..]
        .chars()
        .take_while(|c| *c != '"' && *c != '\'')
        .collect();

    let asset = ask(&format!("/{name}")).await;
    assert_eq!(asset.status(), StatusCode::OK, "{name}");
    assert_eq!(
        asset
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("public, max-age=31536000, immutable"),
    );
}

#[tokio::test]
async fn a_binary_without_the_page_says_how_to_build_it() {
    if serve::has_page() {
        return; // The test above is the one with something to say here.
    }

    // Not a 404: the page is not missing, it was never built. The two
    // read the same in a browser's console and mean entirely different
    // things to whoever has to fix it, so the answer carries the three
    // commands that build it.
    let page = ask("/").await;
    assert_eq!(page.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        body(page).await.contains("./build-wasm.sh"),
        "the refusal says how to build the page"
    );

    // Anything else is a plain 404, because that one really is missing.
    let asset = ask("/assets/whatever.js").await;
    assert_eq!(asset.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_file_is_only_lent_to_a_caller_that_says_who_it_is() {
    // The header is what tells the page a file came from here rather than
    // from a static host answering everything with the application.
    let plain = ask("/file?path=anything.msh").await;
    assert_eq!(plain.status(), StatusCode::NOT_FOUND);
    assert!(
        plain.headers().get(serve::FROM_GECKO).is_some(),
        "even a refusal says who answered"
    );
}
