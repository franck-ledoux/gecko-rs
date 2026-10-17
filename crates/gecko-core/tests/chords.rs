//! Chords: the dual curves a blocking is made of, crosswise.
//!
//! A sheet is a layer; a chord is a thread. One face names one chord — in at
//! that face, out at the one opposite, on into whatever is glued there — and
//! the two operations that need threads are taking one out and wrapping one
//! in a layer of its own.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError, ChordBlock};
use std::collections::HashMap;

/// Glue every pair of faces that sit on the same four corners.
fn weld_everything(b: &mut Blocking) {
    let sides = b.block_faces();
    let key = |s: &gecko_core::BlockFace| {
        let mut at = [0.0; 3];
        for node in s.nodes {
            let p = b.position(node).unwrap();
            for k in 0..3 {
                at[k] += p[k] / 4.0;
            }
        }
        at.map(|x| (x * 1e6).round() as i64)
    };
    let mut seen: HashMap<[i64; 3], CellId> = HashMap::new();
    let mut pairs = Vec::new();
    for side in &sides {
        match seen.entry(key(side)) {
            std::collections::hash_map::Entry::Occupied(e) => pairs.push((*e.get(), side.face)),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(side.face);
            }
        }
    }
    for (first, second) in pairs {
        b.glue_onto(first, second).unwrap();
    }
}

/// `n` unit cubes in a row along x, glued end to end.
fn row(n: usize) -> Blocking {
    let mut b = Blocking::new().unwrap();
    for i in 0..n {
        b.add_block(cube_corners([i as f64, 0.0, 0.0], 1.0))
            .unwrap();
    }
    weld_everything(&mut b);
    b
}

/// An n by n by n grid of unit cubes, every touching pair glued.
fn grid(n: usize) -> Blocking {
    let mut b = Blocking::new().unwrap();
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                b.add_block(cube_corners([i as f64, j as f64, k as f64], 1.0))
                    .unwrap();
            }
        }
    }
    weld_everything(&mut b);
    b
}

/// The face whose four corners are all at this x, and the block it is on —
/// the pair a click gives. Panics unless exactly one answers.
fn face_at_x(b: &Blocking, x: f64) -> (CellId, CellId) {
    let found: Vec<_> = b
        .block_faces()
        .into_iter()
        .filter(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[0] - x).abs() < 1e-9)
        })
        .collect();
    assert_eq!(found.len(), 1, "one face at x = {x}");
    (found[0].face, found[0].block)
}

/// Every (face, block) pair whose face lies flat at this x.
fn faces_at_x(b: &Blocking, x: f64) -> Vec<(CellId, CellId)> {
    b.block_faces()
        .into_iter()
        .filter(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[0] - x).abs() < 1e-9)
        })
        .map(|s| (s.face, s.block))
        .collect()
}

fn blocks_of(chord: &[ChordBlock]) -> Vec<CellId> {
    chord.iter().map(|c| c.block).collect()
}

#[test]
fn a_chord_runs_through_every_block_it_can_reach() {
    let b = row(3);
    let (face, block) = face_at_x(&b, 0.0);
    let chord = b.chord_through(face, block).expect("a chord");
    assert_eq!(chord.len(), 3, "in at one end and out at the other");

    // In order, and each block entered and left by opposite faces.
    let blocks = blocks_of(&chord);
    assert_eq!(blocks[0], block);
    assert_eq!(
        blocks
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );

    // Crosswise, nothing is glued, so the chord is one block long. Same
    // blocking, different thread.
    let sideways = b
        .block_faces()
        .into_iter()
        .find(|s| {
            s.block == block
                && s.nodes
                    .iter()
                    .all(|n| b.position(*n).unwrap()[1].abs() < 1e-9)
        })
        .expect("the y = 0 face of the first block");
    let across = b
        .chord_through(sideways.face, sideways.block)
        .expect("a chord");
    assert_eq!(blocks_of(&across), vec![block]);
}

#[test]
fn the_side_a_face_was_clicked_on_does_not_change_its_chord() {
    // An internal face has a block on each side. The dual curve crosses it
    // once either way, so naming either block has to give the same thread —
    // which is only true because the walk goes both ways from the face.
    let b = row(3);
    let seam = faces_at_x(&b, 1.0);
    assert_eq!(seam.len(), 2, "one face, listed once per block");

    let mut answers: Vec<Vec<CellId>> = seam
        .iter()
        .map(|&(face, block)| {
            let mut blocks = blocks_of(&b.chord_through(face, block).expect("a chord"));
            blocks.sort_unstable();
            blocks
        })
        .collect();
    answers.dedup();
    assert_eq!(answers.len(), 1, "the same three blocks from either side");
    assert_eq!(answers[0].len(), 3);
}

#[test]
fn a_chord_can_close_into_a_ring() {
    // The two ends of the row glued to each other. Geometrically absurd —
    // one block is stretched the length of the row — and topologically a
    // ring, which is what is under test: the walk has to stop when it comes
    // back to where it started rather than going round for ever.
    let mut b = row(3);
    let (first, _) = face_at_x(&b, 0.0);
    let (last, _) = face_at_x(&b, 3.0);
    b.glue_onto(first, last).expect("the two ends");

    for (face, block) in faces_at_x(&b, 1.0) {
        let chord = b
            .chord_through(face, block)
            .expect("a ring is not a tangle");
        assert_eq!(chord.len(), 3, "every block once, and no more");
    }
}

/// The face whose four corners are all at this y, and the block it is on.
fn face_at_y(b: &Blocking, y: f64) -> (CellId, CellId) {
    let found: Vec<_> = b
        .block_faces()
        .into_iter()
        .filter(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[1] - y).abs() < 1e-9)
        })
        .collect();
    assert_eq!(found.len(), 1, "one face at y = {y}");
    (found[0].face, found[0].block)
}

#[test]
fn a_chord_that_crosses_a_block_twice_is_refused() {
    // One cube with two **perpendicular** faces glued to each other. The
    // chord goes in at x = 0, out at x = 1, and comes straight back into the
    // same block through y = 1 — a different pair of faces, so it crosses
    // the block twice. Taking that away would remove one block twice, and
    // pillowing it would ask the same block to shrink in directions it has
    // already shrunk in.
    //
    // Gluing opposite faces instead makes a ring one block long, which is
    // a different thing and allowed.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    // Named before the glue: welding two faces of one cube deforms it, and
    // afterwards there is no face left lying flat at x = 0 to find by
    // measuring. The name still means the same face.
    let (face, block) = face_at_x(&b, 0.0);
    let (far_x, _) = face_at_x(&b, 1.0);
    let (far_y, _) = face_at_y(&b, 1.0);
    b.glue_onto(far_x, far_y).expect("a block onto itself");
    assert!(
        matches!(
            b.chord_through(face, block),
            Err(BlockingError::TangledChord(_))
        ),
        "a chord across the same block twice has to be refused"
    );
}

#[test]
fn a_face_has_to_be_a_face_of_the_block_it_is_asked_about() {
    let b = row(3);
    let (face, _) = face_at_x(&b, 0.0);
    let far = face_at_x(&b, 3.0).1;
    assert!(matches!(
        b.chord_through(face, far),
        Err(BlockingError::FaceNotOnBlock(..))
    ));
    assert!(matches!(
        b.chord_through(CellId::from_raw(9999), far),
        Err(BlockingError::NoSuchFace(_))
    ));
    assert!(matches!(
        b.chord_through(face, CellId::from_raw(9999)),
        Err(BlockingError::NoSuchBlock(_))
    ));
}

#[test]
fn removing_a_chord_leaves_a_tunnel() {
    let mut b = grid(3);
    assert_eq!(b.nb_blocks(), 27);

    // The middle column of the x = 0 side: three blocks through the grid.
    let (face, block) = b
        .block_faces()
        .into_iter()
        .find(|s| {
            let centre = |k: usize| {
                s.nodes
                    .iter()
                    .map(|n| b.position(*n).unwrap()[k])
                    .sum::<f64>()
                    / 4.0
            };
            centre(0).abs() < 1e-9
                && (centre(1) - 1.5).abs() < 1e-9
                && (centre(2) - 1.5).abs() < 1e-9
        })
        .map(|s| (s.face, s.block))
        .expect("the middle face of one side");

    let gone = b.remove_chord(face, block).expect("a chord to remove");
    assert_eq!(gone.len(), 3);
    assert_eq!(b.nb_blocks(), 24);

    // And every one of them is still a closed block, with its own six
    // faces: what was internal on the wall of the tunnel is on the
    // boundary again.
    assert_eq!(b.block_faces().len(), 24 * 6);
}

#[test]
fn an_o_grid_along_a_chord_puts_four_blocks_round_each_of_its_own() {
    let mut b = row(3);
    let (face, block) = face_at_x(&b, 0.0);
    let made = b.pillow_chord(face, block).expect("a chord to pillow");

    // Five where there was one, all the way along: the chord itself, and a
    // ring of four around each of its blocks.
    assert_eq!(b.nb_blocks(), 15);
    assert!(!made.is_empty());

    // The ends stayed where they were. A node on an end face belongs to two
    // of the side faces and to nothing else, so it moves across the chord
    // and not one bit along it — the tube is open at both ends, flush with
    // where it was.
    let x: Vec<f64> = b.nodes().map(|(_, p)| p[0]).collect();
    assert!(
        x.iter().all(|&v| v > -1e-9 && v < 3.0 + 1e-9),
        "nothing moved past either end"
    );
    assert_eq!(
        x.iter().filter(|v| v.abs() < 1e-9).count(),
        8,
        "the four corners drawn inwards, and the four the new ring kept"
    );
}
