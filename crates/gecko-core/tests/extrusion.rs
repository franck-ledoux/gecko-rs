//! Growing a block out of a face.

use gecko_core::{cube_corners, Blocking, BlockingError, CellId};

fn cube() -> Blocking {
    let mut b = Blocking::new().expect("a map");
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0))
        .expect("a cube");
    b
}

/// The face of `block` whose centre is nearest `at` — how a test says "the
/// top one" without depending on which name the kernel handed it.
fn face_near(blocking: &Blocking, at: [f64; 3]) -> (CellId, CellId) {
    let sides = blocking.block_faces();
    let middle = |s: &gecko_core::BlockFace| {
        let mut c = [0.0; 3];
        for n in s.nodes {
            let p = blocking.position(n).unwrap();
            for k in 0..3 {
                c[k] += p[k] / 4.0;
            }
        }
        c
    };
    let best = sides
        .iter()
        .min_by(|a, b| {
            let d = |s: &gecko_core::BlockFace| {
                let c = middle(s);
                (0..3).map(|k| (c[k] - at[k]).powi(2)).sum::<f64>()
            };
            d(a).partial_cmp(&d(b)).unwrap()
        })
        .expect("a face");
    (best.face, best.block)
}

fn heights(blocking: &Blocking) -> Vec<f64> {
    let mut all: Vec<f64> = blocking
        .nodes()
        .map(|(_, p)| (p[2] * 1e9).round() / 1e9)
        .collect();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    all.dedup();
    all
}

#[test]
fn a_face_grows_a_block_along_its_own_normal() {
    let mut b = cube();
    let (face, block) = face_near(&b, [0.5, 0.5, 1.0]);
    let grown = b.extrude_face(face, block, 2.0).expect("a block");

    // Two blocks sharing a face: the same counts gluing two cubes gives.
    assert_eq!(
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
        (12, 20, 11, 2)
    );
    assert_ne!(grown, block);
    // The new one reaches exactly `height` along the normal, and the face it
    // came from has not moved.
    assert_eq!(heights(&b), vec![0.0, 1.0, 3.0]);
}

#[test]
fn it_grows_outwards_whichever_face_is_picked() {
    // The normal is the face's own, so the bottom grows downwards and a side
    // sideways. A single sign error would send one of them into the block.
    for (at, axis, want) in [
        ([0.5, 0.5, 0.0], 2, -1.0),
        ([0.5, 0.5, 1.0], 2, 2.0),
        ([0.0, 0.5, 0.5], 0, -1.0),
        ([1.0, 0.5, 0.5], 0, 2.0),
        ([0.5, 0.0, 0.5], 1, -1.0),
        ([0.5, 1.0, 0.5], 1, 2.0),
    ] {
        let mut b = cube();
        let (face, block) = face_near(&b, at);
        b.extrude_face(face, block, 1.0).expect("a block");

        let reach = b
            .nodes()
            .map(|(_, p)| p[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        let least = b
            .nodes()
            .map(|(_, p)| p[axis])
            .fold(f64::INFINITY, f64::min);
        let got = if want > 0.0 { reach } else { least };
        assert!((got - want).abs() < 1e-9, "grew to {got}, wanted {want}");
        assert_eq!(b.nb_blocks(), 2);
    }
}

#[test]
fn the_new_block_is_welded_rather_than_merely_resting_against_it() {
    // Eleven faces, not twelve: the shared one is one face, which is the
    // whole difference between a weld and two blocks in the same place.
    let mut b = cube();
    let (face, block) = face_near(&b, [0.5, 0.5, 1.0]);
    b.extrude_face(face, block, 0.5).unwrap();
    assert_eq!(b.nb_faces(), 11);
    assert_eq!(b.nb_nodes(), 12);

    // And the weld moved nothing: the four corners it was built on are
    // exactly where they were, not averaged halfway to anywhere.
    let corners: Vec<_> = b
        .nodes()
        .filter(|(_, p)| (p[2] - 1.0).abs() < 1e-9)
        .collect();
    assert_eq!(corners.len(), 4);
}

#[test]
fn one_extrusion_can_be_grown_on_another() {
    let mut b = cube();
    let (face, block) = face_near(&b, [0.5, 0.5, 1.0]);
    let second = b.extrude_face(face, block, 1.0).expect("a second");
    let (top, _) = face_near(&b, [0.5, 0.5, 2.0]);
    b.extrude_face(top, second, 1.0).expect("a third");

    assert_eq!(b.nb_blocks(), 3);
    assert_eq!(heights(&b), vec![0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn a_face_inside_the_model_has_nothing_to_grow_into() {
    let mut b = cube();
    let (face, block) = face_near(&b, [0.5, 0.5, 1.0]);
    b.extrude_face(face, block, 1.0).unwrap();

    // That face now has a block on each side.
    let (shared, side) = face_near(&b, [0.5, 0.5, 1.0]);
    let refused = b.extrude_face(shared, side, 1.0).expect_err("inside");
    assert!(
        matches!(refused, BlockingError::NotOnTheSurface(f) if f == shared),
        "{refused:?}"
    );
    assert_eq!(b.nb_blocks(), 2, "and nothing was built");
}

#[test]
fn a_distance_that_is_not_a_distance_is_refused() {
    for height in [0.0, f64::NAN, f64::INFINITY] {
        let mut b = cube();
        let (face, block) = face_near(&b, [0.5, 0.5, 1.0]);
        let refused = b
            .extrude_face(face, block, height)
            .expect_err("no distance");
        assert!(
            matches!(refused, BlockingError::BadHeight(_)),
            "{refused:?}"
        );
        assert_eq!(b.nb_blocks(), 1);
    }
}

#[test]
fn a_face_of_a_block_will_not_grow_into_it() {
    // The other side of a face of a block is the block. Growing that way
    // would lay a second block inside the first and weld the two along a
    // face they are both behind — and the refusal says so, rather than
    // calling a direction a bad distance.
    //
    // A face of a **patch** is the other case, and grows either way: see
    // the patches suite.
    let mut b = cube();
    let (face, block) = face_near(&b, [0.5, 0.5, 1.0]);
    let refused = b
        .extrude_face(face, block, -1.0)
        .expect_err("into the block");
    assert!(matches!(refused, BlockingError::Inwards(_)), "{refused:?}");
    assert!(
        refused.to_string().contains("either way"),
        "and it says where the other case is: {refused}"
    );
    assert_eq!(b.nb_blocks(), 1);
}

#[test]
fn a_face_that_is_not_on_that_block_says_so() {
    let mut b = cube();
    b.add_block(cube_corners([5.0, 0.0, 0.0], 1.0)).unwrap();
    let (face, _) = face_near(&b, [0.5, 0.5, 1.0]);
    let (_, other) = face_near(&b, [5.5, 0.5, 1.0]);

    let refused = b.extrude_face(face, other, 1.0).expect_err("not its face");
    assert!(
        matches!(refused, BlockingError::FaceNotOnBlock(f, bl) if f == face && bl == other),
        "{refused:?}"
    );
}

#[test]
fn the_face_it_welds_to_is_known_rather_than_looked_for() {
    // The new block is built here, so which of its six faces sits on the old
    // one is a fact about the construction, not a question to answer by
    // comparing positions. A flat extrusion — one whose far face is barely
    // clear of the near one — would defeat any answer found by looking.
    let mut b = cube();
    let (face, block) = face_near(&b, [0.5, 0.5, 1.0]);
    b.extrude_face(face, block, 1.0e-6)
        .expect("a very thin block");
    assert_eq!(b.nb_blocks(), 2);
    assert_eq!(b.nb_faces(), 11);
    assert_eq!(b.nb_nodes(), 12);
}
