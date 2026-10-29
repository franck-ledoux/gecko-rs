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

#[tokio::test]
async fn the_page_is_never_kept_and_its_assets_are_kept_for_ever() {
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
    let html = String::from_utf8(
        http_body_util::BodyExt::collect(page.into_body())
            .await
            .expect("the page")
            .to_bytes()
            .to_vec(),
    )
    .expect("utf-8");
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
