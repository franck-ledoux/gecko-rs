//! Laying a layer of hexahedra along a patch of surface.
//!
//! The point of pillowing is that **the model's surface does not move**: the
//! blocks behind it are drawn in and the layer fills the gap. So the counts
//! matter, and so does where everything ends up — a layer that pushed the
//! surface outwards would be a different operation wearing the same name.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, mitre, quad_normal, Blocking, BlockingError, PILLOW_SHARE};

fn cube() -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b
}

fn cells(b: &Blocking) -> (usize, usize, usize, usize) {
    (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks())
}

/// The face of the only block whose corners all sit at this value on `axis`.
fn face_at(b: &Blocking, axis: usize, value: f64) -> (CellId, CellId) {
    let side = b
        .block_faces()
        .into_iter()
        .find(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[axis] - value).abs() < 1e-9)
        })
        .expect("a face there");
    (side.face, side.block)
}

#[test]
fn a_mitre_on_a_flat_patch_is_just_the_depth() {
    // Four faces of a flat patch all pointing the same way: summing their
    // normals would send the node four times too far.
    let flat = [[0.0, 0.0, -1.0]; 4];
    let at = mitre(&flat, 0.3);
    assert!((at[2] + 0.3).abs() < 1e-12, "{at:?}");
    assert!(at[0].abs() < 1e-12 && at[1].abs() < 1e-12);
}

#[test]
fn a_mitre_on_a_corner_clears_both_faces() {
    // Two faces at a right angle. Averaging their normals would leave the
    // node short of both; the answer is the corner of the frame.
    let corner = [[0.0, 0.0, -1.0], [0.0, -1.0, 0.0]];
    let at = mitre(&corner, 0.3);
    assert!((at[1] + 0.3).abs() < 1e-9, "{at:?}");
    assert!((at[2] + 0.3).abs() < 1e-9, "{at:?}");
}

#[test]
fn a_mitre_has_nowhere_to_go_between_opposed_faces() {
    assert_eq!(mitre(&[[0.0, 0.0, 1.0], [0.0, 0.0, -1.0]], 0.3), [0.0; 3]);
    assert_eq!(mitre(&[], 0.3), [0.0; 3]);
}

#[test]
fn a_quadrangle_normal_points_out() {
    // Anticlockwise seen from +z, so the normal is +z.
    let up = quad_normal([
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ]);
    assert!(
        up[2] > 0.0 && up[0].abs() < 1e-12 && up[1].abs() < 1e-12,
        "{up:?}"
    );
}

#[test]
fn pillowing_one_face_lays_one_block() {
    let mut b = cube();
    let top = face_at(&b, 2, 1.0);
    b.pillow(&[top]).unwrap();

    // Two blocks sharing a face: the same shape as a sheet cut, which on a
    // single face is what this is.
    assert_eq!(cells(&b), (12, 20, 11, 2));
}

#[test]
fn the_surface_stays_exactly_where_it_was() {
    let mut b = cube();
    b.pillow(&[face_at(&b, 2, 1.0)]).unwrap();

    // Four nodes still on the old surface, four drawn in by a third of the
    // cell, four untouched at the bottom.
    let heights: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
    assert_eq!(
        heights.iter().filter(|z| (**z - 1.0).abs() < 1e-9).count(),
        4
    );
    assert_eq!(heights.iter().filter(|z| z.abs() < 1e-9).count(), 4);
    let layer = 1.0 - PILLOW_SHARE;
    assert_eq!(
        heights
            .iter()
            .filter(|z| (**z - layer).abs() < 1e-9)
            .count(),
        4,
        "the shrunk face should sit at {layer}: {heights:?}"
    );
}

#[test]
fn pillowing_two_faces_that_meet_joins_their_layers() {
    let mut b = cube();
    let top = face_at(&b, 2, 1.0);
    let front = face_at(&b, 1, 0.0);
    b.pillow(&[top, front]).unwrap();

    // Three blocks: the cube drawn in, and a layer on each face. Six of the
    // eight corners were on the patch, so six copies.
    assert_eq!(b.nb_blocks(), 3);
    assert_eq!(b.nb_nodes(), 14);
    // The two layers are glued along the edge their faces shared, so the
    // model has one face fewer than three loose blocks would.
    assert_eq!(b.block_faces().len(), 18);
    let shared = b
        .block_faces()
        .iter()
        .filter(|s| b.block_faces().iter().filter(|o| o.face == s.face).count() == 2)
        .count();
    assert_eq!(shared, 6, "three glued pairs");
}

#[test]
fn pillowing_every_face_wraps_the_block() {
    let mut b = cube();
    let faces: Vec<_> = b.block_faces().iter().map(|s| (s.face, s.block)).collect();
    b.pillow(&faces).unwrap();

    // The cube, shrunk, inside six slabs welded into a shell.
    assert_eq!(b.nb_blocks(), 7);
    assert_eq!(b.nb_nodes(), 16);
    // The original surface has not moved: eight corners still on it.
    let on_surface = b
        .nodes()
        .filter(|(_, p)| p.iter().all(|x| x.abs() < 1e-9 || (*x - 1.0).abs() < 1e-9))
        .count();
    assert_eq!(on_surface, 8);
}

/// A face with a block on each side, with the upper or lower block.
fn inside(b: &Blocking, from_above: bool) -> (CellId, CellId) {
    let sides = b.block_faces();
    let height = |block: CellId| {
        sides
            .iter()
            .filter(|o| o.block == block)
            .flat_map(|o| o.nodes)
            .map(|n| b.position(n).unwrap()[2])
            .sum::<f64>()
            / 24.0
    };
    let pick = sides
        .iter()
        .filter(|s| sides.iter().filter(|o| o.face == s.face).count() == 2)
        .max_by(|x, y| {
            let (a, c) = (height(x.block), height(y.block));
            if from_above {
                a.partial_cmp(&c).unwrap()
            } else {
                c.partial_cmp(&a).unwrap()
            }
        })
        .expect("an internal face");
    (pick.face, pick.block)
}

#[test]
fn a_face_inside_the_model_gets_a_layer_between_the_two_blocks() {
    // The case that needs the two blocks to stop sharing their nodes
    // first: drawing one in would otherwise drag the other with it, and
    // leave no space for a layer at all.
    let mut b = cube();
    b.pillow(&[face_at(&b, 2, 1.0)]).unwrap();
    assert_eq!(cells(&b), (12, 20, 11, 2));

    let before: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
    b.pillow(&[inside(&b, true)]).expect("an internal face");

    // Three blocks, and four more nodes: what was one shared face is two.
    assert_eq!(b.nb_blocks(), 3);
    assert_eq!(b.nb_nodes(), 16);

    // The far side has not moved: every height below the seam is still
    // exactly where it was.
    let after: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
    for z in before.iter().filter(|z| **z < 0.5) {
        assert!(
            after.iter().any(|w| (w - z).abs() < 1e-9),
            "height {z} moved: {after:?}"
        );
    }
}

#[test]
fn an_internal_layer_can_be_pillowed_again() {
    // The real check that what comes out is a mesh: the faces it creates
    // have to be as workable as the ones they came from.
    let mut b = cube();
    b.pillow(&[face_at(&b, 2, 1.0)]).unwrap();
    b.pillow(&[inside(&b, true)]).unwrap();
    b.pillow(&[inside(&b, true)]).unwrap();
    assert_eq!(b.nb_blocks(), 4);
    assert_eq!(b.nb_nodes(), 20);
}

#[test]
fn an_empty_patch_and_a_face_off_its_block_are_refused() {
    let mut b = cube();
    let before = cells(&b);
    assert!(matches!(b.pillow(&[]), Err(BlockingError::EmptyPatch)));
    let (face, block) = face_at(&b, 2, 1.0);
    assert!(matches!(
        b.pillow(&[(face, CellId::from_raw(9999))]),
        Err(BlockingError::FaceNotOnBlock(_, _))
    ));
    assert!(matches!(
        b.pillow(&[(CellId::from_raw(9999), block)]),
        Err(BlockingError::FaceNotOnBlock(_, _))
    ));
    assert_eq!(cells(&b), before);
}

#[test]
fn the_answer_is_the_same_every_time() {
    let build = || {
        let mut b = cube();
        let top = face_at(&b, 2, 1.0);
        b.pillow(&[top]).unwrap();
        let mut p: Vec<[f64; 3]> = b.nodes().map(|(_, at)| at).collect();
        p.sort_by(|a, c| a.partial_cmp(c).unwrap());
        p
    };
    assert_eq!(build(), build());
}
