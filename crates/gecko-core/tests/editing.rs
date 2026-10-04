//! Creating a cube, and moving one of its nodes.
//!
//! The point of interest is not the arithmetic — it is that a node is named by
//! something that still means the same thing on the next call.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError};

fn unit_cube() -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b
}

fn sorted_positions(b: &Blocking) -> Vec<[f64; 3]> {
    let mut p: Vec<[f64; 3]> = b.nodes().map(|(_, pos)| pos).collect();
    p.sort_by(|a, c| a.partial_cmp(c).unwrap());
    p
}

#[test]
fn a_cube_is_eight_corners_in_the_order_the_faces_read_them() {
    let c = cube_corners([0.0, 0.0, 0.0], 1.0);
    assert_eq!(c[0], [0.0, 0.0, 0.0]);
    assert_eq!(c[6], [1.0, 1.0, 1.0]);
    // Corner 1 is along +x of corner 0, corner 3 along +y, corner 4 along +z —
    // which is what makes `HEX_FACES` describe outward faces.
    assert_eq!(c[1], [1.0, 0.0, 0.0]);
    assert_eq!(c[3], [0.0, 1.0, 0.0]);
    assert_eq!(c[4], [0.0, 0.0, 1.0]);

    let moved = cube_corners([2.0, -1.0, 0.5], 3.0);
    assert_eq!(moved[0], [2.0, -1.0, 0.5]);
    assert_eq!(moved[6], [5.0, 2.0, 3.5]);
}

#[test]
fn a_cube_builds_a_well_formed_block() {
    let b = unit_cube();
    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
        (8, 12, 6, 1)
    );

    let mut expected = cube_corners([0.0, 0.0, 0.0], 1.0);
    expected.sort_by(|a, c| a.partial_cmp(c).unwrap());
    assert_eq!(sorted_positions(&b), expected);
}

#[test]
fn every_node_answers_to_its_name_and_no_two_share_a_cell() {
    let b = unit_cube();
    let names: Vec<CellId> = b.nodes().map(|(id, _)| id).collect();
    assert_eq!(names.len(), 8);

    let mut cells: Vec<_> = names
        .iter()
        .map(|id| b.node_by_id(*id).expect("a named node resolves"))
        .collect();
    cells.sort_unstable();
    cells.dedup();
    assert_eq!(cells.len(), 8, "two names resolved to one cell");

    // An unnamed or unknown name is a question with the answer "no such cell",
    // not an error.
    assert!(b.node_by_id(CellId::NONE).is_none());
    assert!(b.node_by_id(CellId::from_raw(9999)).is_none());
}

#[test]
fn moving_a_node_moves_exactly_that_one() {
    let mut b = unit_cube();
    let target = b.nodes().find(|(_, p)| *p == [1.0, 1.0, 1.0]).unwrap().0;
    let others: Vec<_> = b.nodes().filter(|(id, _)| *id != target).collect();

    b.set_position(target, [4.0, 5.0, 6.0]).unwrap();

    assert_eq!(
        b.nodes().find(|(id, _)| *id == target).unwrap().1,
        [4.0, 5.0, 6.0]
    );
    for (id, was) in others {
        assert_eq!(
            b.nodes().find(|(i, _)| *i == id).unwrap().1,
            was,
            "{id:?} moved too"
        );
    }
    // Moving a node is not a topological change: nothing is created or renamed.
    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
        (8, 12, 6, 1)
    );
    assert!(
        b.node_by_id(target).is_some(),
        "the name did not survive the move"
    );
}

#[test]
fn moving_a_node_that_does_not_exist_changes_nothing() {
    let mut b = unit_cube();
    let before = sorted_positions(&b);

    let err = b
        .set_position(CellId::from_raw(4242), [9.0, 9.0, 9.0])
        .unwrap_err();
    assert!(matches!(err, BlockingError::NoSuchNode(_)), "{err:?}");
    assert_eq!(sorted_positions(&b), before);
}

#[test]
fn two_cubes_keep_their_names_and_their_nodes_apart() {
    let mut b = unit_cube();
    let first: Vec<CellId> = b.nodes().map(|(id, _)| id).collect();
    b.add_block(cube_corners([3.0, 0.0, 0.0], 1.0)).unwrap();

    assert_eq!(b.nb_blocks(), 2);
    assert_eq!(b.nb_nodes(), 16);
    let all: Vec<CellId> = b.nodes().map(|(id, _)| id).collect();
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 16, "names collided");

    // The first cube's names still resolve, and to the same places.
    for id in &first {
        assert!(b.node_by_id(*id).is_some(), "{id:?} was lost");
    }
    let target = first[0];
    let before = b.nodes().find(|(i, _)| *i == target).unwrap().1;
    b.set_position(target, [-7.0, 0.0, 0.0]).unwrap();
    assert_ne!(b.nodes().find(|(i, _)| *i == target).unwrap().1, before);
    // The second cube is untouched: still eight corners of a unit cube at x=3.
    let second: Vec<[f64; 3]> = b
        .nodes()
        .filter(|(id, _)| !first.contains(id))
        .map(|(_, p)| p)
        .collect();
    assert_eq!(second.len(), 8);
    assert!(
        second.iter().all(|p| p[0] == 3.0 || p[0] == 4.0),
        "{second:?}"
    );
}
