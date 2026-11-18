//! Faces that are not part of any block.
//!
//! A blocking of a surface comes before a blocking of a volume: quadrangles
//! laid on the geometry, cut into a grid, and then extruded into blocks. So
//! a face has to be able to stand on its own, and the map has to go on
//! making sense while it does.
//!
//! A face carries none, one or two **sides** — which block is behind it, and
//! at which of that block's six slots. None is a patch face: faces welded
//! edge to edge share an edge and bound nothing, and a connected component
//! of them is a patch, derived rather than named, with a
//! name of its own that nobody had to keep a list for. What tells it from a
//! block is whether the shell closes.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError, On};

/// The unit square in z = 0, anticlockwise seen from above.
const SQUARE: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
];

#[test]
fn a_face_on_its_own_is_a_patch_and_not_a_block() {
    let mut b = Blocking::new().unwrap();
    let face = b.add_face(SQUARE).expect("a quadrangle");

    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces()),
        (4, 4, 1),
        "four corners, four sides, one face"
    );
    // Not a block, and said so in both directions.
    assert_eq!(b.nb_blocks(), 0);
    assert_eq!(b.nb_patches(), 1);
    assert_eq!(b.patches().len(), 1);
    assert!(!b.is_block(b.patches()[0]));

    let seen = b.census();
    assert_eq!((seen.faces, seen.blocks, seen.patches), (1, 0, 1));
    assert!(!face.is_none(), "and it has a name");
}

#[test]
fn a_block_is_still_a_block() {
    // The predicate has to answer the old question as well as the new one:
    // a block names its six faces and each names it back, and that is the whole
    // difference.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    assert_eq!((b.nb_blocks(), b.nb_patches()), (1, 0));
    assert!(b.patches().is_empty());

    b.add_face(SQUARE).unwrap();
    assert_eq!(
        (b.nb_blocks(), b.nb_patches()),
        (1, 1),
        "one of each, counted apart"
    );
}

#[test]
fn the_mesher_walks_past_a_patch() {
    // The walk that finds the corner above one of a block's own crosses a
    // face,
    // which is null at the boundary of an open sheet. Meshing a blocking
    // that has one has to pass it by rather than step off the end of it.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_face([
        [3.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 1.0, 0.0],
        [3.0, 1.0, 0.0],
    ])
    .unwrap();

    assert_eq!(b.hexes().len(), 1, "the block, and not the face");
    let mesh = b.mesh(2, |_, _| None);
    assert_eq!(mesh.hexes.len(), 8, "two segments each way, one block");
}

#[test]
fn the_operations_that_need_an_inside_say_so() {
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let patch = b.patches()[0];
    let side = b.block_faces();
    let face = side[0].face;

    // A chord steps out through the face opposite; a pillow draws a block
    // inwards. An open sheet has neither.
    assert!(matches!(
        b.chord_through(face, patch),
        Err(BlockingError::NotABlock(_))
    ));
    assert!(matches!(
        b.pillow(&[(face, patch)]),
        Err(BlockingError::NotABlock(_))
    ));

    // And a name nothing answers to is still its own complaint.
    assert!(matches!(
        b.pillow(&[(face, CellId::from_raw(9999))]),
        Err(BlockingError::FaceNotOnBlock(..))
    ));
}

#[test]
fn a_patch_comes_back_whole_from_a_snapshot() {
    // A face with no side at all is exactly the case a copy could quietly
    // get wrong by assuming every face bounds something.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_face(SQUARE).unwrap();

    let copy = b.snapshot().expect("a copy");
    assert_eq!(
        (copy.nb_nodes(), copy.nb_edges(), copy.nb_faces()),
        (b.nb_nodes(), b.nb_edges(), b.nb_faces())
    );
    assert_eq!((copy.nb_blocks(), copy.nb_patches()), (1, 1));
    let mut theirs: Vec<[f64; 3]> = copy.nodes().map(|(_, p)| p).collect();
    let mut ours: Vec<[f64; 3]> = b.nodes().map(|(_, p)| p).collect();
    for list in [&mut theirs, &mut ours] {
        list.sort_by(|a, c| a.partial_cmp(c).unwrap());
    }
    assert_eq!(theirs, ours, "and every corner is where it was");
}

#[test]
fn cutting_a_free_quadrangle_leaves_two_faces_in_one_patch() {
    // The sheet machinery is written in terms of edges and the faces that
    // hold them, and every step across a face is guarded — so it ought to
    // cut a sheet that has
    // no volumes at all. This is where that is found out, because the rest
    // of a surface blocking depends on it: one quadrangle per surface, cut
    // into a grid, then extruded.
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let along_x = b
        .edges_in_full()
        .find(|(_, [u, v], ..)| {
            let (p, q) = (b.position(*u).unwrap(), b.position(*v).unwrap());
            (p[1] - q[1]).abs() < 1e-9 && p[1].abs() < 1e-9
        })
        .map(|(e, ..)| e)
        .expect("the side at y = 0");

    b.split_sheet(along_x, 0.5).expect("a sheet of one face");

    assert_eq!(b.nb_faces(), 2, "the quadrangle is two now");
    assert_eq!(b.nb_nodes(), 6);
    assert_eq!(b.nb_edges(), 7);
    // Sharing the edge the cut laid, so they are one component — which is what
    // an extrusion will be asked to take all of.
    assert_eq!(
        (b.nb_blocks(), b.nb_patches()),
        (0, 1),
        "still one patch, and still no block"
    );
}

#[test]
fn a_face_of_a_patch_can_be_taken_away() {
    // Cut a quadrangle in two and take one half: what is left is whole,
    // with the edge that was shared back on its own boundary.
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let along_x = b
        .edges_in_full()
        .find(|(_, [u, v], ..)| {
            let (p, q) = (b.position(*u).unwrap(), b.position(*v).unwrap());
            (p[1] - q[1]).abs() < 1e-9 && p[1].abs() < 1e-9
        })
        .map(|(e, ..)| e)
        .expect("the side at y = 0");
    b.split_sheet(along_x, 0.5).unwrap();
    assert_eq!((b.nb_faces(), b.nb_nodes(), b.nb_edges()), (2, 6, 7));

    let going = b.block_faces()[0].face;
    b.remove_face(going).expect("a face of a patch");

    assert_eq!(b.nb_faces(), 1, "one left");
    assert_eq!(b.nb_patches(), 1);
    // Four corners and four sides again: the half that stayed came away
    // with its own copies of what the two of them shared.
    assert_eq!((b.nb_nodes(), b.nb_edges()), (4, 4));
}

#[test]
fn a_side_of_a_block_is_not_a_face_to_take_away() {
    // It would leave a volume that is neither open nor closed, and what
    // the caller means is to take the block.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let face = b.block_faces()[0].face;
    assert!(matches!(
        b.remove_face(face),
        Err(BlockingError::NotAPatch(_))
    ));
    assert_eq!((b.nb_blocks(), b.nb_faces()), (1, 6), "and nothing moved");

    assert!(matches!(
        b.remove_face(CellId::from_raw(9999)),
        Err(BlockingError::NoSuchFace(_))
    ));
}

#[test]
fn taking_the_last_face_leaves_nothing_behind() {
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let face = b.block_faces()[0].face;
    b.remove_face(face).expect("the only face");
    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_patches()),
        (0, 0, 0, 0)
    );
}

/// The square, cut in two along x = 0.5: a patch of two faces.
fn two_faces() -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let along_x = b
        .edges_in_full()
        .find(|(_, [u, v], ..)| {
            let (p, q) = (b.position(*u).unwrap(), b.position(*v).unwrap());
            (p[1] - q[1]).abs() < 1e-9 && p[1].abs() < 1e-9
        })
        .map(|(e, ..)| e)
        .expect("the side at y = 0");
    b.split_sheet(along_x, 0.5).unwrap();
    b
}

#[test]
fn extruding_one_free_face_makes_one_block() {
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let face = b.block_faces()[0].face;

    let made = b.extrude_patch(face, 2.0).expect("a patch to raise");
    assert_eq!(made.len(), 1);
    assert_eq!(
        (b.nb_blocks(), b.nb_patches(), b.nb_nodes(), b.nb_faces()),
        (1, 0, 8, 6),
        "a block, and the sheet is gone"
    );
    // Four corners where the face was, four `height` above.
    let z: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
    assert_eq!(z.iter().filter(|v| v.abs() < 1e-9).count(), 4);
    assert_eq!(z.iter().filter(|v| (**v - 2.0).abs() < 1e-9).count(), 4);
}

#[test]
fn a_patch_of_two_faces_raises_two_blocks_already_glued() {
    // The point of doing the whole component at once: the two blocks share
    // the face the edge between the faces was raised into, so what comes
    // out is one object and not two boxes side by side.
    let mut b = two_faces();
    assert_eq!((b.nb_faces(), b.nb_nodes()), (2, 6));
    let face = b.block_faces()[0].face;

    let made = b.extrude_patch(face, 1.0).expect("a patch to raise");
    assert_eq!(made.len(), 2);
    assert_eq!(b.nb_blocks(), 2);
    assert_eq!(b.nb_patches(), 0);
    // Twelve, not sixteen: the four corners of the shared face are shared.
    assert_eq!(b.nb_nodes(), 12);
    assert_eq!(b.nb_faces(), 11, "six and six, less the one between them");
}

#[test]
fn what_the_sheet_was_on_comes_up_with_it() {
    let mut b = two_faces();
    // Said by hand, since this test is about carrying it rather than about
    // where it came from.
    let face = b.block_faces()[0].face;
    let (corners, edges) = b.face_cells(face).expect("the face");
    b.set_face_class(face, Some(On { dim: 2, tag: 7 })).unwrap();
    b.set_node_class(corners[0], Some(On { dim: 0, tag: 21 }))
        .unwrap();
    b.set_edge_class(edges[0], Some(On { dim: 1, tag: 11 }))
        .unwrap();
    b.set_frozen(corners[1], true).unwrap();

    b.extrude_patch(face, 1.0).expect("a patch to raise");

    // The bottom of the block that took that face's place carries all of
    // it: the face, the corner, the edge, and the freeze.
    let bottom = b
        .block_faces()
        .into_iter()
        .find(|s| s.on == Some(On { dim: 2, tag: 7 }))
        .expect("the face kept what it was on");
    let (corners, edges) = b.face_cells(bottom.face).expect("its cells");
    assert!(
        corners
            .iter()
            .any(|c| b.node_class(*c) == Some(On { dim: 0, tag: 21 })),
        "the corner kept its corner"
    );
    assert!(
        edges
            .iter()
            .any(|e| b.edge_class(*e) == Some(On { dim: 1, tag: 11 })),
        "the edge kept its curve"
    );
    assert_eq!(
        corners.iter().filter(|c| b.is_frozen(**c)).count(),
        1,
        "and the frozen corner is still frozen"
    );
}

#[test]
fn a_fold_in_the_patch_does_not_pull_the_blocks_apart() {
    // A tent: the square cut down the middle and the ridge lifted, so the
    // two faces have different normals. A step per **face** would raise the
    // ridge to two different pairs of corners; one per **node**, along the
    // mitre of the faces meeting at it, raises it once.
    let mut b = two_faces();
    let ridge: Vec<CellId> = b
        .nodes()
        .filter(|(_, p)| (p[0] - 0.5).abs() < 1e-9)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ridge.len(), 2, "the two ends of the cut");
    for node in &ridge {
        let at = b.position(*node).unwrap();
        b.set_position(*node, [at[0], at[1], 0.5]).unwrap();
    }

    let face = b.block_faces()[0].face;
    let made = b.extrude_patch(face, 1.0).expect("a folded patch");
    assert_eq!(made.len(), 2);
    assert_eq!((b.nb_blocks(), b.nb_patches()), (2, 0));
    assert_eq!(b.nb_nodes(), 12, "the ridge was raised once, not twice");

    // Every node rises; the ridge's two rise **in the fold's own plane of
    // symmetry**, because two faces leaning the same amount either way
    // average to a step with no lean. A step per face would send them to
    // two different points, and the glue would then drag one onto the
    // other and leave neither at x = 0.5.
    let top: Vec<[f64; 3]> = b
        .nodes()
        .map(|(_, p)| p)
        .filter(|p| p[2] > 0.5 + 1e-9)
        .collect();
    assert_eq!(top.len(), 6, "one above each corner of the tent: {top:?}");
    let ridge_top: Vec<[f64; 3]> = top
        .iter()
        .copied()
        .filter(|p| (p[0] - 0.5).abs() < 1e-9)
        .collect();
    assert_eq!(
        ridge_top.len(),
        2,
        "the ridge rose once, straight up: {top:?}"
    );
    assert!((ridge_top[0][2] - ridge_top[1][2]).abs() < 1e-9);
}

#[test]
fn only_the_patch_that_was_asked_about_is_raised() {
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let flat = b.block_faces()[0].face;
    b.add_face([
        [3.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 1.0, 0.0],
        [3.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(b.nb_patches(), 2, "nothing welds them, so they are two");

    assert_eq!(b.extrude_patch(flat, 0.5).expect("the first").len(), 1);
    assert_eq!(
        (b.nb_blocks(), b.nb_patches()),
        (1, 1),
        "the other sheet is untouched"
    );
}

#[test]
fn a_block_is_not_a_patch_to_raise() {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let face = b.block_faces()[0].face;
    assert!(matches!(
        b.extrude_patch(face, 1.0),
        Err(BlockingError::NotAPatch(_))
    ));
    assert!(matches!(
        b.extrude_patch(face, 0.0),
        Err(BlockingError::BadHeight(_))
    ));
}

/// The sign of a hexahedron's volume, by the only thing that matters here:
/// whether it agrees with a block everyone agrees is the right way round.
fn handedness(c: &[[f64; 3]; 8]) -> f64 {
    let edge = |a: usize, b: usize| [0, 1, 2].map(|k| c[b][k] - c[a][k]);
    let (u, v, w) = (edge(0, 1), edge(0, 3), edge(0, 4));
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    (0..3).map(|k| cross[k] * w[k]).sum()
}

#[test]
fn a_patch_grows_either_way_and_neither_is_inside_out() {
    // Which way a quadrangle is wound came from the triangles of the
    // surface it was laid on, and nobody looking at the screen can tell.
    // So both directions are the right one — and the block has to come out
    // the right way round both times, not merely exist.
    let upright = {
        let mut b = Blocking::new().unwrap();
        b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
        handedness(
            &b.hexes()[0]
                .corners
                .map(|n| b.position(n).unwrap_or([0.0; 3])),
        )
    };
    assert!(
        upright.abs() > 1e-9,
        "a cube has a handedness to compare to"
    );

    for height in [1.0, -1.0] {
        let mut b = Blocking::new().unwrap();
        b.add_face(SQUARE).unwrap();
        let face = b.block_faces()[0].face;
        b.extrude_patch(face, height).expect("either way");

        assert_eq!((b.nb_blocks(), b.nb_patches()), (1, 0));
        let z: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
        assert_eq!(
            z.iter().filter(|v| (**v - height).abs() < 1e-9).count(),
            4,
            "four corners {height} away"
        );

        let hex = b.hexes();
        assert_eq!(hex.len(), 1);
        let corners = hex[0].corners.map(|n| b.position(n).unwrap_or([0.0; 3]));
        assert!(
            handedness(&corners) * upright > 0.0,
            "the block grown by {height} is inside out: {corners:?}"
        );
    }
}

#[test]
fn going_the_other_way_carries_the_classification_too() {
    // The corners are read round the other way to keep the block the right
    // way round, so what each of them was on has to follow that same
    // reading rather than the one it was written in.
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let face = b.block_faces()[0].face;
    let (corners, edges) = b.face_cells(face).unwrap();
    b.set_node_class(corners[1], Some(On { dim: 0, tag: 21 }))
        .unwrap();
    b.set_edge_class(edges[1], Some(On { dim: 1, tag: 11 }))
        .unwrap();
    b.set_face_class(face, Some(On { dim: 2, tag: 7 })).unwrap();
    let pinned = b.position(corners[1]).unwrap();

    b.extrude_patch(face, -1.0).expect("the other way");

    // The corner that was on corner 21 is still the one at that point.
    let landed = b
        .nodes()
        .find(|(_, p)| (0..3).all(|k| (p[k] - pinned[k]).abs() < 1e-9))
        .map(|(id, _)| id)
        .expect("the corner is still there");
    assert_eq!(b.node_class(landed), Some(On { dim: 0, tag: 21 }));
    assert_eq!(
        b.block_faces()
            .iter()
            .filter(|s| s.on == Some(On { dim: 2, tag: 7 }))
            .count(),
        1,
        "and the face kept its surface"
    );
    assert_eq!(
        b.edges_in_full()
            .filter(|(.., on)| *on == Some(On { dim: 1, tag: 11 }))
            .count(),
        1,
        "and the edge kept its curve"
    );
}

#[test]
fn a_patch_of_two_goes_the_other_way_in_one_piece_too() {
    // The seams were written down in the sheet's own numbering, and going
    // the other way the blocks number their sides the other way: welding
    // by the first numbering would glue two faces that are not against
    // each other.
    let mut b = two_faces();
    let face = b.block_faces()[0].face;
    b.extrude_patch(face, -1.0).expect("the other way");

    assert_eq!((b.nb_blocks(), b.nb_patches()), (2, 0));
    assert_eq!(b.nb_nodes(), 12, "glued to each other, not sixteen");
    assert_eq!(b.nb_faces(), 11);

    // Both blocks below, and neither inside out.
    let upright = {
        let mut c = Blocking::new().unwrap();
        c.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
        handedness(
            &c.hexes()[0]
                .corners
                .map(|n| c.position(n).unwrap_or([0.0; 3])),
        )
    };
    for hex in b.hexes() {
        let corners = hex.corners.map(|n| b.position(n).unwrap_or([0.0; 3]));
        assert!(
            handedness(&corners) * upright > 0.0,
            "inside out: {corners:?}"
        );
        assert!(corners.iter().all(|p| p[2] <= 1e-9));
    }
}

/// A quadrangle laid against another is welded to it along the side they
/// share, which is what makes a strip of them one patch to raise.
mod laying {
    use gecko_core::{Blocking, On};

    /// The unit square, and the one next to it sharing the side from
    /// (1,0,0) to (1,1,0).
    const FIRST: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    const NEXT: [[f64; 3]; 4] = [
        [1.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];

    #[test]
    fn two_laid_on_a_shared_side_are_one_patch() {
        let mut b = Blocking::new().unwrap();
        let first = b.lay_face(FIRST).expect("the first");
        assert_eq!(first.welded, 0, "the first has nothing to weld to");

        let next = b.lay_face(NEXT).expect("the second");
        assert_eq!(next.welded, 1, "one shared side, one weld");
        let seen = b.census();
        // Eight corners less the two that met, and the shared side counted
        // once: two free-standing quadrangles would be 8 nodes and 8 edges.
        assert_eq!((seen.faces, seen.nodes, seen.edges), (2, 6, 7));
        assert_eq!(b.nb_patches(), 1, "welded edge to edge, so one patch");
    }

    #[test]
    fn the_clicks_do_not_decide_which_way_round_it_is_wound() {
        // The same two quadrangles, the second wound the other way. A sheet
        // has no inside, so there is no wrong way round — and two adjacent
        // faces of a sheet run opposite ways along the side they share, so
        // being wound to agree with the
        // neighbour is the whole difference between welding and not.
        let mut b = Blocking::new().unwrap();
        b.lay_face(FIRST).expect("the first");
        let back = [NEXT[0], NEXT[3], NEXT[2], NEXT[1]];
        let next = b.lay_face(back).expect("the second");

        assert_eq!(next.welded, 1, "turned to agree, and welded");
        assert_eq!(b.nb_patches(), 1);
        // And the corners still come back in the order the points were
        // given, whichever way round it ended up being wound.
        for (k, name) in next.corners.iter().enumerate() {
            assert_eq!(b.position(*name).unwrap(), back[k], "corner {k}");
        }
    }

    #[test]
    fn the_neighbor_keeps_its_names_its_class_and_its_place() {
        let mut b = Blocking::new().unwrap();
        let first = b.lay_face(FIRST).expect("the first");
        // Said of the two corners the next one will be laid on, and of the
        // side between them.
        b.set_node_class(first.corners[1], Some(On { dim: 0, tag: 7 }))
            .unwrap();
        b.set_node_class(first.corners[2], Some(On { dim: 0, tag: 8 }))
            .unwrap();
        let (_, edges) = b.face_cells(first.face).expect("its cells");
        b.set_edge_class(edges[1], Some(On { dim: 1, tag: 9 }))
            .unwrap();
        let was = edges[1];

        // Laid a hair off, as a click always is: the page reads a handle's
        // place back through the f32 buffer it draws from, so "the same
        // point" is the same to about a part in ten million.
        let nudged = [
            [NEXT[0][0] + 1e-9, NEXT[0][1], NEXT[0][2] - 1e-9],
            NEXT[1],
            NEXT[2],
            [NEXT[3][0] - 1e-9, NEXT[3][1], NEXT[3][2] + 1e-9],
        ];
        let next = b.lay_face(nudged).expect("the second");
        assert_eq!(next.welded, 1);

        // The nodes that survived are the neighbor's, by name, by class and
        // by position — exactly, not half a rounding error away, because
        // two nodes becoming one meet at the point between them. The quadrangle
        // being laid is the one that gives way.
        assert_eq!(next.corners[0], first.corners[1], "its name, not a new one");
        assert_eq!(next.corners[3], first.corners[2]);
        assert_eq!(b.node_class(next.corners[0]), Some(On { dim: 0, tag: 7 }));
        assert_eq!(b.node_class(next.corners[3]), Some(On { dim: 0, tag: 8 }));
        assert_eq!(b.position(next.corners[0]).unwrap(), FIRST[1]);
        // And so does the shared side.
        let (_, after) = b.face_cells(next.face).expect("its cells");
        assert!(after.contains(&was), "the old edge: {after:?}");
        assert_eq!(b.edge_class(was), Some(On { dim: 1, tag: 9 }));
    }

    #[test]
    fn a_strip_of_five_rises_into_five_blocks_of_one_height() {
        // What `⇧Q` five times and one drag are for.
        let mut b = Blocking::new().unwrap();
        for k in 0..5 {
            let x = k as f64;
            let laid = b
                .lay_face([
                    [x, 0.0, 0.0],
                    [x + 1.0, 0.0, 0.0],
                    [x + 1.0, 1.0, 0.0],
                    [x, 1.0, 0.0],
                ])
                .expect("a quadrangle");
            assert_eq!(laid.welded, usize::from(k > 0), "the one at {x}");
        }
        let seen = b.census();
        assert_eq!((seen.faces, seen.nodes), (5, 12), "one strip");
        assert_eq!(b.nb_patches(), 1);

        let patch = b.patches()[0];
        let face = b.faces_of(patch)[0].face;
        assert_eq!(b.extrude_patch(face, 0.5).expect("raised").len(), 5);
        let seen = b.census();
        assert_eq!((seen.blocks, seen.nodes), (5, 24), "twelve up, twelve down");
        assert_eq!(b.nb_patches(), 0, "the patch was consumed");
    }

    #[test]
    fn a_side_with_two_free_edges_to_choose_from_is_left_free() {
        let mut b = Blocking::new().unwrap();
        b.lay_face(FIRST).expect("the first");
        // A second quadrangle standing on that same side, put there with
        // `add_face`, which welds nothing. Now the side from (1,0,0) to
        // (1,1,0) has two free edges between the same two points.
        b.add_face([
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.5, 1.0, 2.0],
            [1.5, 0.0, 2.0],
        ])
        .expect("a quadrangle standing on its own");

        let next = b.lay_face(NEXT).expect("the third");
        assert_eq!(next.welded, 0, "two to choose from, so neither");
        assert_eq!(b.nb_patches(), 3, "three sheets, none welded");
    }
}
