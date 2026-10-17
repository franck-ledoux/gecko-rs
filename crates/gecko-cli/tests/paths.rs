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
    //
    // "In full" means a drive on Windows, which is why this is two
    // strings rather than one — the first version of this test said
    // `/tmp` everywhere and Windows was right to disagree.
    let full = if cfg!(windows) {
        r"C:\tmp\gecko\mesh.vtk"
    } else {
        "/tmp/gecko/mesh.vtk"
    };
    assert_eq!(here().at(full), Some(PathBuf::from(full)));
}

#[test]
#[cfg(windows)]
fn a_root_without_a_drive_is_refused() {
    // `/tmp/x` names the root of *some* drive, and which one would be a
    // guess. Resolving it under the folder gecko was started in would
    // mean something different again, so it is refused outright.
    assert_eq!(here().at("/tmp/x"), None);
}
