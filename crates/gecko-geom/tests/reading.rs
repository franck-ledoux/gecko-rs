//! Reading a gmsh file, and what it is taken to mean.
//!
//! The fixtures are written out in full rather than loaded from disk: the
//! point of most of them is one malformed or missing line, and a file you
//! have to open in another window to see that line is a test you cannot read.

use gecko_geom::{Geometry, Msh};

/// A unit square: two triangles on one surface, its four sides each a curve
/// of its own, its four corners each a point element.
///
/// Physical tag 0 throughout and the entity tag carrying everything, which is
/// exactly how gmsh writes a file with no physical groups declared — and how
/// every `.msh` in the C++ GECKO's test data is written.
const SQUARE: &str = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
4
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
$EndNodes
$Elements
10
1 15 2 0 21 1
2 15 2 0 22 2
3 15 2 0 23 3
4 15 2 0 24 4
5 1 2 0 11 1 2
6 1 2 0 12 2 3
7 1 2 0 13 3 4
8 1 2 0 14 4 1
9 2 2 0 1 1 2 3
10 2 2 0 1 1 3 4
$EndElements
";

#[test]
fn a_square_is_one_surface_four_curves_and_four_corners() {
    let geometry = Geometry::read(SQUARE).expect("a square");
    assert_eq!(geometry.nodes().len(), 4);

    assert_eq!(geometry.surfaces().len(), 1);
    assert_eq!(geometry.surfaces()[0].tag, 1);
    assert_eq!(geometry.surfaces()[0].triangles, [[0, 1, 2], [0, 2, 3]]);

    // One curve per side, named by its own tag — not four segments of one.
    let tags: Vec<_> = geometry.curves().iter().map(|c| c.tag).collect();
    assert_eq!(tags, [11, 12, 13, 14]);
    assert!(geometry.curves().iter().all(|c| c.segments.len() == 1));

    let corners: Vec<_> = geometry.corners().iter().map(|c| (c.tag, c.node)).collect();
    assert_eq!(corners, [(21, 0), (22, 1), (23, 2), (24, 3)]);

    assert_eq!(geometry.nb_triangles(), 2);
    assert_eq!(geometry.nb_segments(), 4);
    assert_eq!(geometry.skipped(), 0);
}

#[test]
fn triangles_of_one_tag_are_one_surface_however_many_there_are() {
    let geometry = Geometry::read(SQUARE).expect("a square");
    assert_eq!(geometry.surfaces().len(), 1);
    assert_eq!(geometry.surfaces()[0].triangles.len(), 2);
}

/// What the user chose when told that every `.msh` in GECKO's test data
/// carries its volume mesh too: read the skin, count what was left.
#[test]
fn tetrahedra_are_skipped_and_their_interior_nodes_with_them() {
    // Node 5 is used by the tetrahedron alone. It must not reach the model:
    // on a real file that is tens of thousands of points nothing draws.
    let text = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
5
1 0 0 0
2 1 0 0
3 0 1 0
4 0 0 1
5 9 9 9
$EndNodes
$Elements
3
1 2 2 0 1 1 2 3
2 4 2 0 7 1 2 3 5
3 4 2 0 7 1 2 4 5
$EndElements
";
    let geometry = Geometry::read(text).expect("the skin of a tetrahedral mesh");
    assert_eq!(geometry.skipped(), 2);
    assert_eq!(geometry.surfaces().len(), 1);
    assert_eq!(geometry.nodes().len(), 3, "node 5 is a tetrahedron's alone");
    assert_eq!(geometry.surfaces()[0].triangles, [[0, 1, 2]]);
}

#[test]
fn an_element_on_no_elementary_entity_belongs_to_nothing() {
    let text = SQUARE.replace("9 2 2 0 1 1 2 3", "9 2 2 0 0 1 2 3");
    let geometry = Geometry::read(&text).expect("a square with one loose triangle");
    assert_eq!(geometry.surfaces().len(), 1);
    assert_eq!(
        geometry.surfaces()[0].triangles,
        [[0, 2, 3]],
        "the tag-0 triangle is not part of any surface"
    );
}

#[test]
fn the_file_s_own_node_numbers_need_not_be_contiguous() {
    // A mesh with parts deleted has gaps in its numbering, and gmsh does not
    // renumber. Reading the numbers as indices would put the triangle on the
    // wrong nodes, or off the end of the array.
    let text = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
3
40 0 0 0
7 1 0 0
900 0 1 0
$EndNodes
$Elements
1
1 2 2 0 1 40 7 900
$EndElements
";
    let geometry = Geometry::read(text).expect("a gappy numbering");
    assert_eq!(geometry.surfaces()[0].triangles, [[0, 1, 2]]);
    assert_eq!(geometry.nodes()[1], [1.0, 0.0, 0.0]);
}

#[test]
fn physical_names_are_read_and_do_not_group_anything() {
    let text = SQUARE.replace(
        "$EndMeshFormat\n",
        "$EndMeshFormat\n$PhysicalNames\n2\n2 1 \"the top face\"\n1 11 \"a side\"\n$EndPhysicalNames\n",
    );
    let geometry = Geometry::read(&text).expect("a square with names");
    assert_eq!(geometry.names().len(), 2);
    assert_eq!(geometry.names()[0].name, "the top face");
    assert_eq!(geometry.names()[0].dimension, 2);
    assert_eq!(geometry.names()[0].tag, 1);
    // And the grouping is unchanged: it is the entity tag that groups.
    assert_eq!(geometry.surfaces().len(), 1);
    assert_eq!(geometry.curves().len(), 4);
}

#[test]
fn windows_line_endings_are_no_obstacle() {
    let text = SQUARE.replace('\n', "\r\n");
    let geometry = Geometry::read(&text).expect("a file written on Windows");
    assert_eq!(geometry.surfaces().len(), 1);
    assert_eq!(geometry.corners().len(), 4);
}

#[test]
fn a_version_4_file_is_refused_by_name() {
    // The one that matters: modern gmsh writes 4.1 by default, where the
    // entity tags live in `$Entities` and a `$Nodes` line is a tag followed
    // by nothing. Read as 2.2 it would be a mesh of plausible nonsense.
    let text = SQUARE.replace("2.2 0 8", "4.1 0 8");
    let message = Geometry::read(&text).expect_err("4.1").to_string();
    assert!(message.contains("4.1"), "{message}");
    assert!(message.contains("-format msh2"), "{message}");
}

#[test]
fn a_binary_file_is_refused() {
    let text = SQUARE.replace("2.2 0 8", "2.2 1 8");
    let message = Geometry::read(&text).expect_err("binary").to_string();
    assert!(message.contains("binary"), "{message}");
}

#[test]
fn something_that_is_not_a_gmsh_file_at_all_says_so() {
    let message = Geometry::read("hello\n")
        .expect_err("not a mesh")
        .to_string();
    assert!(message.contains("$MeshFormat"), "{message}");
}

#[test]
fn a_file_with_no_triangles_is_not_a_surface_mesh() {
    let text = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
2
1 0 0 0
2 1 0 0
$EndNodes
$Elements
1
1 1 2 0 11 1 2
$EndElements
";
    let message = Geometry::read(text).expect_err("no triangles").to_string();
    assert!(message.contains("surface mesh"), "{message}");
}

#[test]
fn triangles_with_no_entity_tag_at_all_say_so_rather_than_build_nothing() {
    // The failure mode worth a message of its own: a file whose elements
    // carry no tags loads as an empty model, and an empty model looks like a
    // bug in the viewer rather than a fact about the file.
    let text = SQUARE
        .replace("9 2 2 0 1 1 2 3", "9 2 0 1 2 3")
        .replace("10 2 2 0 1 1 3 4", "10 2 0 1 3 4");
    let message = Geometry::read(&text).expect_err("untagged").to_string();
    assert!(message.contains("elementary entity tag"), "{message}");
    assert!(message.contains('2'), "{message}");
}

#[test]
fn an_element_naming_a_node_the_file_never_defines_is_an_error() {
    let text = SQUARE.replace("9 2 2 0 1 1 2 3", "9 2 2 0 1 1 2 77");
    let message = Geometry::read(&text)
        .expect_err("dangling node")
        .to_string();
    assert!(message.contains("77"), "{message}");
}

#[test]
fn a_truncated_section_says_which_line_ran_out() {
    let text = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
4
1 0 0 0
";
    let message = Geometry::read(text).expect_err("truncated").to_string();
    assert!(message.contains("$Nodes"), "{message}");
    assert!(message.contains("truncated"), "{message}");
}

#[test]
fn a_coordinate_that_is_not_a_number_names_its_line() {
    let text = SQUARE.replace("2 1 0 0", "2 1 x 0");
    let message = Geometry::read(&text)
        .expect_err("bad coordinate")
        .to_string();
    assert!(message.contains("line 7"), "{message}");
}

#[test]
fn the_same_file_read_twice_comes_out_in_the_same_order() {
    // Tag order, not hash order: a colour-by-id palette keyed on the position
    // in these arrays would otherwise repaint the model on every reload.
    let once = Geometry::read(SQUARE).expect("a square");
    let twice = Geometry::read(SQUARE).expect("a square");
    let tags = |g: &Geometry| -> Vec<i32> { g.curves().iter().map(|c| c.tag).collect() };
    assert_eq!(tags(&once), tags(&twice));
    assert_eq!(tags(&once), [11, 12, 13, 14]);
}

#[test]
fn the_reader_keeps_the_file_as_it_stands_and_interprets_nothing() {
    // The split that makes the reader testable on its own: it reports the
    // elements, tags and all, and says nothing about surfaces or curves.
    let file: Msh = gecko_geom::msh::read(SQUARE).expect("a square");
    assert_eq!(file.nodes.len(), 4);
    assert_eq!(file.elements.len(), 10);
    assert!(file.elements.iter().all(|e| e.physical == 0));
    assert_eq!(file.skipped, 0);
}
