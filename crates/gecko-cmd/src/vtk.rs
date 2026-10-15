//! Writing a mesh out as VTK legacy ASCII.
//!
//! The oldest and plainest of the VTK formats, and the one every reader
//! still takes: a header, the points, the cells, and what kind each cell is.
//! No library, because the file is four lists and a preamble, and a
//! dependency that writes four lists is a dependency to keep up to date.

use gecko_core::Mesh;
use std::fmt::Write;

/// What VTK calls a hexahedron.
pub const VTK_HEXAHEDRON: u8 = 12;

/// The mesh, as a `.vtk` file.
///
/// The hexahedra carry the name of the block they came out of as cell data,
/// which is what lets a reader colour the mesh by block — the one thing a
/// picture of a mesh of a blocking is for.
pub fn write(mesh: &Mesh, title: &str) -> String {
    let mut out = String::with_capacity(mesh.points.len() * 48 + mesh.hexes.len() * 40);
    out.push_str("# vtk DataFile Version 2.0\n");
    // One line, and the format says so: a title with a newline in it is a
    // file no reader will take.
    let _ = writeln!(out, "{}", title.replace(['\n', '\r'], " "));
    out.push_str("ASCII\nDATASET UNSTRUCTURED_GRID\n");

    let _ = writeln!(out, "POINTS {} double", mesh.points.len());
    for p in &mesh.points {
        let _ = writeln!(out, "{} {} {}", p[0], p[1], p[2]);
    }

    // Nine numbers a cell: how many corners, and which.
    let _ = writeln!(out, "\nCELLS {} {}", mesh.hexes.len(), mesh.hexes.len() * 9);
    for hex in &mesh.hexes {
        let _ = writeln!(
            out,
            "8 {} {} {} {} {} {} {} {}",
            hex[0], hex[1], hex[2], hex[3], hex[4], hex[5], hex[6], hex[7]
        );
    }

    let _ = writeln!(out, "\nCELL_TYPES {}", mesh.hexes.len());
    for _ in &mesh.hexes {
        let _ = writeln!(out, "{VTK_HEXAHEDRON}");
    }

    let _ = writeln!(out, "\nCELL_DATA {}", mesh.hexes.len());
    out.push_str("SCALARS block int 1\nLOOKUP_TABLE default\n");
    for block in &mesh.blocks {
        // A cell of a blocking always has a name by the time anyone can
        // mesh it; -1 is for the case that cannot happen.
        let _ = writeln!(out, "{}", block.get().map_or(-1, |n| n as i64));
    }
    out
}

/// What a name reads as in a message.
pub fn count(mesh: &Mesh) -> String {
    format!(
        "{} node{}, {} hexahedr{}",
        mesh.points.len(),
        if mesh.points.len() == 1 { "" } else { "s" },
        mesh.hexes.len(),
        if mesh.hexes.len() == 1 { "on" } else { "a" },
    )
}
