//! Welding two blocks along a face.
//!
//! This is the first operation that makes β₃ mean anything: until now every
//! block was free-standing, every face was on the boundary, and "internal"
//! described nothing. So the counts below are the real subject — eight nodes
//! becoming four is what tells you the two blocks are one object now, rather
//! than two that happen to touch.

use gecko_core::ids::CellId;
use gecko_core::{best_pairing, cube_corners, Blocking, BlockingError};

/// Two unit cubes meeting on the plane x = 1.
fn touching() -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
    b
}

/// The face of `block` whose four corners all sit at this x.
fn face_at_x(b: &Blocking, block: usize, x: f64) -> CellId {
    let sides = b.block_faces();
    let names: Vec<_> = {
        let mut n: Vec<_> = sides.iter().map(|s| s.block).collect();
        n.dedup();
        n
    };
    sides
        .iter()
        .filter(|s| s.block == names[block])
        .find(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[0] - x).abs() < 1e-9)
        })
        .expect("a face at that x")
        .face
}

#[test]
fn the_pairing_is_the_turn_that_moves_the_corners_least() {
    let a = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    // `b` read backwards from m: a[i] meets b[(m - i) mod 4].
    for m in 0..4 {
        let mut b = [[0.0; 3]; 4];
        for i in 0..4 {
            b[(m + 4 - i) % 4] = a[i];
        }
        assert_eq!(best_pairing(&a, &b), m, "turn {m} not recovered");
    }
}

#[test]
fn the_pairing_is_the_same_every_time() {
    // Four corners of a square are equidistant under several turns; the answer
    // still has to be one answer, because both modes ship these bytes.
    let a = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let first = best_pairing(&a, &a);
    for _ in 0..8 {
        assert_eq!(best_pairing(&a, &a), first);
    }
}

#[test]
fn gluing_two_blocks_welds_their_cells() {
    let mut b = touching();
    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
        (16, 24, 12, 2)
    );

    let first = face_at_x(&b, 0, 1.0);
    let second = face_at_x(&b, 1, 1.0);
    b.glue_onto(first, second).expect("two touching faces");

    // Four pairs of nodes became four nodes, four pairs of edges four edges,
    // and the two faces one. The blocks are untouched: gluing joins them, it
    // does not consume one.
    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
        (12, 20, 11, 2)
    );
}

#[test]
fn the_surface_then_has_a_face_with_a_block_on_each_side() {
    // What the viewer calls an internal face, and the first one that has ever
    // existed here: a name that turns up twice in the surface, once per block.
    let mut b = touching();
    b.glue_onto(face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.0))
        .unwrap();

    let sides = b.block_faces();
    // Each block still hands over a closed surface of six.
    assert_eq!(sides.len(), 12);
    let mut shared: Vec<_> = sides
        .iter()
        .filter(|s| sides.iter().filter(|o| o.face == s.face).count() == 2)
        .map(|s| s.face)
        .collect();
    shared.sort();
    shared.dedup();
    assert_eq!(
        shared.len(),
        1,
        "exactly one face should now have two sides"
    );
}

#[test]
fn the_first_face_decides_where_the_seam_lands() {
    // The rule, and the reason the second face is moved before the sewing
    // rather than after: honeycomb merges two vertices by averaging them, so
    // sewing them apart would leave the seam half way between.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_block(cube_corners([1.5, 0.0, 0.0], 1.0)).unwrap();

    let first = face_at_x(&b, 0, 1.0);
    let second = face_at_x(&b, 1, 1.5);
    b.glue_onto(first, second).unwrap();

    // The seam is at x = 1, where the first face was — not at 1.25.
    let seam: Vec<_> = b
        .nodes()
        .map(|(_, p)| p[0])
        .filter(|x| (*x - 1.0).abs() < 1e-9)
        .collect();
    assert_eq!(
        seam.len(),
        4,
        "the four welded corners sit on the first face"
    );
    assert!(
        b.nodes().all(|(_, p)| (p[0] - 1.25).abs() > 1e-9),
        "nothing landed on the midpoint"
    );
    // Only the face moved: the far side of the second block stayed at 2.5.
    assert!(b.nodes().any(|(_, p)| (p[0] - 2.5).abs() < 1e-9));
}

#[test]
fn a_face_cannot_be_glued_twice_or_to_itself() {
    let mut b = touching();
    let first = face_at_x(&b, 0, 1.0);
    let second = face_at_x(&b, 1, 1.0);

    assert!(matches!(
        b.glue_onto(first, first),
        Err(BlockingError::SameFace)
    ));
    assert!(matches!(
        b.glue_onto(first, CellId::from_raw(9999)),
        Err(BlockingError::NoSuchFace(_))
    ));

    b.glue_onto(first, second).unwrap();
    // Whatever that face is called now, both names are spent.
    assert!(matches!(
        b.glue_onto(first, second),
        Err(BlockingError::AlreadyGlued(_)) | Err(BlockingError::NoSuchFace(_))
    ));
}

#[test]
fn a_refused_glue_changes_nothing() {
    let mut b = touching();
    let before = (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks());
    let first = face_at_x(&b, 0, 1.0);
    assert!(b.glue_onto(first, CellId::from_raw(9999)).is_err());
    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
        before
    );
}
