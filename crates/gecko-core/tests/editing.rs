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

    let mut places: Vec<[i64; 3]> = names
        .iter()
        .map(|id| b.position(*id).expect("a named node resolves"))
        .map(|at| at.map(|x| (x * 1e9).round() as i64))
        .collect();
    places.sort_unstable();
    places.dedup();
    assert_eq!(places.len(), 8, "two names resolved to one cell");

    // An unnamed or unknown name is a question with the answer "no such cell",
    // not an error.
    assert!(b.position(CellId::NONE).is_none());
    assert!(b.position(CellId::from_raw(9999)).is_none());
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
        b.position(target).is_some(),
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
        assert!(b.position(*id).is_some(), "{id:?} was lost");
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

#[test]
fn a_frozen_node_does_not_move_for_anybody() {
    use gecko_core::On;
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let (node, before) = b.nodes().next().unwrap();

    b.set_frozen(node, true).unwrap();
    assert!(b.is_frozen(node));
    let refused = b.set_position(node, [9.0, 9.0, 9.0]).expect_err("frozen");
    assert!(
        matches!(refused, BlockingError::Frozen(n) if n == node),
        "{refused:?}"
    );
    // Refused, not quietly ignored: a caller told nothing would go on
    // believing the move happened.
    assert_eq!(b.position(node), Some(before));

    b.set_frozen(node, false).unwrap();
    b.set_position(node, [9.0, 9.0, 9.0]).expect("thawed");
    assert_eq!(b.position(node), Some([9.0, 9.0, 9.0]));

    // And the freeze is independent of the classification: one says where
    // a node may go, the other says whether it may go at all.
    b.set_node_class(node, Some(On { dim: 2, tag: 5 })).unwrap();
    assert_eq!(b.node_class(node), Some(On { dim: 2, tag: 5 }));
    assert!(!b.is_frozen(node));
    b.set_node_class(node, None).unwrap();
    assert_eq!(b.node_class(node), None);
}

#[test]
fn an_edge_knows_the_names_of_its_own_two_ends() {
    // What the inference walks: an edge, and the two nodes whose
    // classification decides its own.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let edges: Vec<_> = b.edges().collect();
    assert_eq!(edges.len(), 12);
    for (edge, a, c, on) in edges {
        assert!(edge.is_some() && a.is_some() && c.is_some());
        assert_ne!(a, c, "an edge's two ends are two nodes");
        assert_eq!(on, None, "nothing is classified yet");
    }
}

/// A block of small thickness, laid on four points.
mod slabs {
    use gecko_core::{Blocking, BlockingError};

    const SQUARE: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    #[test]
    fn four_points_make_a_block_with_a_thin_side() {
        let mut b = Blocking::new().unwrap();
        let slab = b.add_slab(SQUARE, 0.1).expect("a slab");
        let (outer, inner) = (slab.outer, slab.inner);

        let seen = b.census();
        assert_eq!((seen.blocks, seen.nodes, seen.faces), (1, 8, 6));
        assert!(b.is_block(slab.block));
        assert_eq!(slab.welded, 0, "the first slab has nothing to weld to");

        // The four points came back as the four points, in order.
        for (k, name) in outer.iter().enumerate() {
            assert_eq!(b.position(*name).unwrap(), SQUARE[k], "corner {k}");
        }
        // And the other four are one thickness away, along the normal — the
        // square is wound anticlockwise seen from +z, so that is +z.
        for (k, name) in inner.iter().enumerate() {
            let at = b.position(*name).unwrap();
            assert_eq!([at[0], at[1]], [SQUARE[k][0], SQUARE[k][1]], "corner {k}");
            assert!((at[2] - 0.1).abs() < 1e-9, "thickness: {}", at[2]);
        }
        // Outer and inner are eight different nodes.
        let mut all: Vec<_> = outer.iter().chain(inner.iter()).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 8);
    }

    #[test]
    fn the_sign_picks_the_side_the_material_is_on() {
        let mut b = Blocking::new().unwrap();
        for name in b.add_slab(SQUARE, -0.1).expect("a slab").inner {
            assert!(b.position(name).unwrap()[2] < 0.0, "the other way");
        }
    }

    #[test]
    fn no_thickness_and_no_quadrangle_are_both_refused() {
        let mut b = Blocking::new().unwrap();
        assert!(matches!(
            b.add_slab(SQUARE, 0.0),
            Err(BlockingError::BadHeight(_))
        ));
        let flat = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]];
        assert!(matches!(
            b.add_slab(flat, 0.1),
            Err(BlockingError::LostFace)
        ));
    }

    /// The square next door, sharing the side from (1,0,0) to (1,1,0) and
    /// wound the same way round.
    const NEXT: [[f64; 3]; 4] = [
        [1.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];

    #[test]
    fn a_slab_laid_on_a_neighbors_corners_is_welded_to_its_thin_side() {
        let mut b = Blocking::new().unwrap();
        b.add_slab(SQUARE, 0.1).expect("the first slab");
        let next = b.add_slab(NEXT, 0.1).expect("the second");

        assert_eq!(next.welded, 1, "one shared side, one weld");
        let seen = b.census();
        // Four corners in common, and the face between them counted once:
        // two free-standing slabs would be 16 nodes and 12 faces.
        assert_eq!((seen.blocks, seen.nodes, seen.faces), (2, 12, 11));
    }

    #[test]
    fn the_slab_being_laid_adopts_the_thickness_it_is_welded_to() {
        let mut b = Blocking::new().unwrap();
        b.add_slab(SQUARE, 0.1).expect("the first slab");
        let next = b.add_slab(NEXT, 0.5).expect("the second");

        assert_eq!(next.welded, 1);
        // The two corners of the welded side came onto the old face, and the
        // two at the far end kept the thickness they were asked for: the
        // neighbor does not move, the slab being laid does.
        let high = |k: usize| b.position(next.inner[k]).unwrap()[2];
        assert!((high(0) - 0.1).abs() < 1e-9, "on the weld: {}", high(0));
        assert!((high(3) - 0.1).abs() < 1e-9, "on the weld: {}", high(3));
        assert!((high(1) - 0.5).abs() < 1e-9, "away from it: {}", high(1));
        assert!((high(2) - 0.5).abs() < 1e-9, "away from it: {}", high(2));
    }

    #[test]
    fn a_slab_thickened_the_other_way_is_not_welded_to() {
        let mut b = Blocking::new().unwrap();
        b.add_slab(SQUARE, 0.1).expect("the first slab");
        // The same four points of the shared side, but the square wound the
        // other way round, so its material is on the other side of the
        // surface. Nothing there leans our way.
        let back = [NEXT[0], NEXT[3], NEXT[2], NEXT[1]];
        let next = b.add_slab(back, 0.1).expect("the second");

        assert_eq!(next.welded, 0, "the two are back to back");
        let seen = b.census();
        assert_eq!((seen.blocks, seen.nodes, seen.faces), (2, 16, 12));
    }

    #[test]
    fn the_fourth_slab_of_a_paving_is_welded_along_two_sides() {
        let square = |x: f64, y: f64| {
            [
                [x, y, 0.0],
                [x + 1.0, y, 0.0],
                [x + 1.0, y + 1.0, 0.0],
                [x, y + 1.0, 0.0],
            ]
        };
        let mut b = Blocking::new().unwrap();
        for (x, y, want) in [
            (0.0, 0.0, 0),
            (1.0, 0.0, 1),
            (0.0, 1.0, 1),
            // Two sides in common, and the corner they meet at is one the
            // first weld has already merged — which is why the second side
            // is looked for by where its two points **are** and not by which
            // nodes this slab started with.
            (1.0, 1.0, 2),
        ] {
            let laid = b.add_slab(square(x, y), 0.1).expect("a slab");
            assert_eq!(laid.welded, want, "the slab at ({x}, {y})");
        }
        let seen = b.census();
        assert_eq!((seen.blocks, seen.nodes), (4, 18), "one two-by-two grid");
    }

    #[test]
    fn two_faces_leaning_the_same_way_leave_the_side_free() {
        let mut b = Blocking::new().unwrap();
        b.add_slab(SQUARE, 0.1).expect("the first slab");
        // A block of its own, overlapping the first slab, whose face on the
        // side from (1,0,0) to (1,1,0) leans the same way the slab's thin
        // side does. Now that side has two things it could be welded to and
        // no reason to prefer either, so it is welded to neither.
        b.add_block([
            [0.5, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.5, 1.0, 0.0],
            [0.5, 0.0, 0.1],
            [1.0, 0.0, 0.1],
            [1.0, 1.0, 0.1],
            [0.5, 1.0, 0.1],
        ])
        .expect("a block");

        let next = b.add_slab(NEXT, 0.1).expect("the second slab");
        assert_eq!(next.welded, 0, "nothing to prefer, so nothing welded");
        assert_eq!(b.census().blocks, 3);
    }

    #[test]
    fn a_slab_sharing_one_corner_only_is_welded_to_nothing() {
        let mut b = Blocking::new().unwrap();
        b.add_slab(SQUARE, 0.1).expect("the first slab");
        // Corner to corner at (1,1,0), with no side in common.
        let across = [
            [1.0, 1.0, 0.0],
            [2.0, 1.0, 0.0],
            [2.0, 2.0, 0.0],
            [1.0, 2.0, 0.0],
        ];
        assert_eq!(b.add_slab(across, 0.1).expect("the second").welded, 0);
        assert_eq!(b.census().nodes, 16, "nothing merged");
    }
}
