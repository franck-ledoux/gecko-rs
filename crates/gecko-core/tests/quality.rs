//! The quality of a mesh as the kernel measures it: the worst corner of
//! every hexahedron, which is positive for every cell the mesher builds
//! from a sound blocking — whichever way each block happens to be
//! numbered.

use gecko_core::{cube_corners, Blocking, On};

fn nowhere(_: On, _: [f64; 3]) -> Option<[f64; 3]> {
    None
}

#[test]
fn every_cell_of_a_pillowed_cube_is_sound_and_a_plain_one_is_perfect() {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let plain = b.mesh(3, nowhere);
    let q = plain.quality();
    assert_eq!(q.len(), 27);
    assert!(q.iter().all(|x| (x - 1.0).abs() < 1e-12), "{q:?}");

    // Seven blocks, six of them made by the pillow and numbered however it
    // numbers them: every cell still has to come out the right way round.
    let sides: Vec<_> = b.block_faces().iter().map(|s| (s.face, s.block)).collect();
    b.pillow(&sides).unwrap();
    let pillowed = b.mesh(2, nowhere);
    let q = pillowed.quality();
    assert_eq!(q.len(), 7 * 8);
    assert!(q.iter().all(|x| *x > 0.3), "{q:?}");
}
