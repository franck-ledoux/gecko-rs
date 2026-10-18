//! Faces that are not part of any block.
//!
//! A blocking of a surface comes before a blocking of a volume: quadrangles
//! laid on the geometry, cut into a grid, and then extruded into blocks. So
//! a face has to be able to stand on its own, and the map has to go on
//! making sense while it does.
//!
//! The arrangement is the map's own. β₂ joins two faces of one volume and β₃
//! joins two volumes, so faces welded edge to edge are welded by β₂ — and a
//! connected component of them is therefore **one volume orbit**, with a
//! name of its own that nobody had to keep a list for. What tells it from a
//! block is whether the shell closes.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError};

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
    assert!(!b.named_is_block(b.patches()[0]));

    let seen = b.census();
    assert_eq!((seen.faces, seen.blocks, seen.patches), (1, 0, 1));
    assert!(!face.is_none(), "and it has a name");
}

#[test]
fn a_block_is_still_a_block() {
    // The predicate has to answer the old question as well as the new one:
    // every dart of a closed shell has a β₂ partner, and that is the whole
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
    // The walk that finds the corner above one of a block's own crosses β₂,
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
    // The undo stack copies the map by hand, dart run by dart run. A face
    // with β₂ null all round is exactly the case a copy could quietly get
    // wrong by assuming every dart has a partner.
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
    // The sheet machinery is written in terms of edges and β₁ cycles, and
    // every β₃ step of it is guarded — so it ought to cut a sheet that has
    // no volumes at all. This is where that is found out, because the rest
    // of a surface blocking depends on it: one quadrangle per surface, cut
    // into a grid, then extruded.
    let mut b = Blocking::new().unwrap();
    b.add_face(SQUARE).unwrap();
    let along_x = b
        .map()
        .iter_edges()
        .map(|e| (e, b.edge_endpoints(e)))
        .find(|(_, (u, v))| {
            let (p, q) = (b.position(*u).unwrap(), b.position(*v).unwrap());
            (p[1] - q[1]).abs() < 1e-9 && p[1].abs() < 1e-9
        })
        .map(|(e, _)| b.edge_tag(e).id)
        .expect("the side at y = 0");

    b.split_sheet(along_x, 0.5).expect("a sheet of one face");

    assert_eq!(b.nb_faces(), 2, "the quadrangle is two now");
    assert_eq!(b.nb_nodes(), 6);
    assert_eq!(b.nb_edges(), 7);
    // Welded along the cut by β₂, so they are one component — which is what
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
        .map()
        .iter_edges()
        .map(|e| (e, b.edge_endpoints(e)))
        .find(|(_, (u, v))| {
            let (p, q) = (b.position(*u).unwrap(), b.position(*v).unwrap());
            (p[1] - q[1]).abs() < 1e-9 && p[1].abs() < 1e-9
        })
        .map(|(e, _)| b.edge_tag(e).id)
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
