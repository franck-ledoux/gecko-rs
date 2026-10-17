//! The viewer bundle this crate embeds is a build artefact of a different
//! toolchain, so it may legitimately not be there yet.
//!
//! `include_dir!` needs the directory to exist at compile time. Creating it
//! here means a fresh clone compiles without anyone having run `npm` first —
//! making a Rust extension fail to *compile* because a TypeScript bundle is
//! missing would be the wrong dependency. `show()` says what to do about an
//! empty one at run time.

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    let bundle = std::path::Path::new(&manifest).join("../../front/dist");
    std::fs::create_dir_all(&bundle).expect("create the bundle directory");
    // Rebuilding the front end has to re-embed it; without this, a stale
    // viewer survives inside the wheel.
    println!("cargo:rerun-if-changed={}", bundle.display());
}
