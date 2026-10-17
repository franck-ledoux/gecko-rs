//! Where a path lands.

use std::path::PathBuf;

/// The module under test is the binary's, so it is reached the way a
/// binary's modules are: by including the file. Only the path arithmetic
/// is wanted here, so the two that touch the disk are unused and said to
/// be.
#[path = "../src/files.rs"]
#[allow(dead_code)]
mod files;

use files::Files;

fn here() -> Files {
    // The root is wherever the process is, which for a test is the crate.
    Files::here().expect("a directory")
}

#[test]
fn a_relative_path_lands_under_the_root() {
    let files = here();
    let at = files.at("meshes/cube.msh").expect("a path");
    assert_eq!(at, files.root().join("meshes/cube.msh"));
}

#[test]
fn a_path_that_climbs_out_of_the_root_is_refused() {
    // "Serve this folder" and "serve the disk" are different promises.
    let files = here();
    assert_eq!(files.at("../../../etc/passwd"), None);
    assert_eq!(files.at("meshes/../../.."), None);
    // Climbing and coming back is fine: it never left.
    assert_eq!(
        files.at("meshes/../cube.msh"),
        Some(files.root().join("cube.msh"))
    );
}

#[test]
fn an_absolute_path_is_taken_as_given() {
    // Written out in full, into a console on this machine, reaching a
    // server bound to this machine. It is the same authority as `gecko
    // run`, and mangling it into the root is how a file ends up in a
    // directory named after itself.
    let files = here();
    let at = files.at("/tmp/gecko/mesh.vtk").expect("a path");
    assert_eq!(at, PathBuf::from("/tmp/gecko/mesh.vtk"));
}
