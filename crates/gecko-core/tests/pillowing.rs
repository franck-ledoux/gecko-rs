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

#[test]
fn the_node_left_on_the_surface_keeps_what_the_node_that_moved_was_on() {
    // The layer leaves a copy of each node where it was and draws the
    // original in. Whatever the node was pinned to belongs to the copy:
    // it is the one still where the geometry is, and the one that went in
    // is inside the material. The other way round pins a node to a curve
    // it is no longer anywhere near, and the mesh is dragged out through
    // the boundary when it is projected.
    use gecko_core::On;
    let mut b = cube();
    let (face, block) = face_at(&b, 2, 1.0);

    let corner = b
        .nodes()
        .find(|(_, p)| p[2] > 0.5)
        .expect("a node of the top face")
        .0;
    let was = b.position(corner).unwrap();
    b.set_node_class(corner, Some(On { dim: 1, tag: 7 }))
        .unwrap();
    b.set_frozen(corner, true).unwrap();

    let drawn = b.pillow(&[(face, block)]).unwrap();
    assert!(!drawn.is_empty(), "nothing was drawn in");

    // Where it was, there is a node on curve 7 — and it is still frozen,
    // a frozen node having been frozen where it stood.
    let outer = b
        .nodes_in_full()
        .find(|(_, p, _, _)| (0..3).all(|k| (p[k] - was[k]).abs() < 1e-9))
        .expect("a node where the corner was");
    assert_eq!(outer.2, Some(On { dim: 1, tag: 7 }), "the surface lost it");
    assert!(outer.3, "the freeze did not come with it");

    // And every node that was drawn in is on nothing and free to move:
    // where it is now is the caller's business to decide again. What it
    // *was* following comes back beside it, for that caller to weigh —
    // the node itself is holding nothing.
    for (node, was) in &drawn {
        let (_, _, on, frozen) = b
            .nodes_in_full()
            .find(|(id, _, _, _)| id == node)
            .expect("a node that moved");
        assert_eq!(on, None, "a node drawn in kept what it was on");
        assert!(!frozen, "a node drawn in stayed frozen");
        if *node == corner {
            assert_eq!(
                *was,
                Some(On { dim: 1, tag: 7 }),
                "and the caller is told what it let go of"
            );
        }
    }
}

/// Every edge of the model that is not a side of any face.
///
/// The cheapest statement of a blocking that has come apart: an edge and
/// the faces around it are two accounts of the same thing, and they have to
/// agree. Pillowing a face another block is behind lets go of the far side
/// and leaves it a copy of every node of the patch to hold on to; the
/// blocks and the faces over there were moved onto the copies and the edges
/// were not, so four edges of the neighbour still named the nodes the layer
/// had just pushed into the middle of the block.
fn edges_bounding_nothing(b: &Blocking) -> Vec<(CellId, CellId)> {
    let mut sides: std::collections::HashSet<(CellId, CellId)> = Default::default();
    for face in b.block_faces() {
        for k in 0..4 {
            let (p, q) = (face.nodes[k], face.nodes[(k + 1) % 4]);
            sides.insert((p.min(q), p.max(q)));
        }
    }
    let mut loose: Vec<(CellId, CellId)> = b
        .edges()
        .map(|(_, a, c, _)| (a.min(c), a.max(c)))
        .filter(|pair| !sides.contains(pair))
        .collect();
    loose.sort();
    loose
}

#[test]
fn a_block_beside_a_pillowed_one_stays_welded_to_the_layer() {
    // The reported case: a cube, a second grown out of one side, and an
    // o-grid run along the first. The side the neighbour is on is one of
    // the four the chord pillows, so the layer goes in between — and the
    // neighbour has to end up against the **outside** of that layer, where
    // its own face has not moved.
    let mut b = cube();
    let (side, block) = face_at(&b, 0, 1.0);
    b.extrude_face(side, block, 0.8).unwrap();

    // The chord across the original block, taken through a face the
    // neighbour is not on, so it is one block long and its four sides
    // include the one they share.
    let bottom = b
        .block_faces()
        .into_iter()
        .find(|s| {
            s.block == block
                && s.nodes
                    .iter()
                    .all(|n| b.position(*n).unwrap()[2].abs() < 1e-9)
        })
        .expect("the original block's floor");
    b.pillow_chord(bottom.face, bottom.block).unwrap();

    assert_eq!(cells(&b), (20, 40, 27, 6));
    assert_eq!(
        edges_bounding_nothing(&b),
        vec![],
        "every edge is a side of a face it shares its ends with"
    );

    // And it is welded to the layer rather than to the core: the face they
    // share is the one the surface was, which pillowing does not move.
    let shared: Vec<_> = b
        .block_faces()
        .into_iter()
        .filter(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[0] - 1.0).abs() < 1e-9)
        })
        .collect();
    assert_eq!(shared.len(), 2, "one face, seen from each of two blocks");
    assert_eq!(shared[0].face, shared[1].face);
    assert_ne!(shared[0].block, shared[1].block);
}

// --- what a pillow surface has to be, whatever it is classified on --------

/// Cut the structure across `axis` at `value`, through any edge spanning it.
fn cut(b: &mut Blocking, axis: usize, value: f64) {
    let (edge, t) = b
        .edges_in_full()
        .find_map(|(id, [p, q], _, _)| {
            let (a, c) = (b.position(p).unwrap(), b.position(q).unwrap());
            let straight = (0..3)
                .filter(|k| *k != axis)
                .all(|k| (a[k] - c[k]).abs() < 1e-12);
            let (lo, hi) = (a[axis].min(c[axis]), a[axis].max(c[axis]));
            (straight && lo < value - 1e-9 && value < hi - 1e-9)
                .then(|| (id, (value - a[axis]) / (c[axis] - a[axis])))
        })
        .expect("an edge across");
    b.split_sheet(edge, t).unwrap();
}

fn centre(b: &Blocking, nodes: &[CellId]) -> [f64; 3] {
    let mut c = [0.0; 3];
    for n in nodes {
        let p = b.position(*n).unwrap();
        for k in 0..3 {
            c[k] += p[k] / nodes.len() as f64;
        }
    }
    c
}

fn near(p: [f64; 3], q: [f64; 3]) -> bool {
    (0..3).all(|k| (p[k] - q[k]).abs() < 1e-9)
}

/// The face centred here, as the block centred there sees it.
fn seen(b: &Blocking, face: [f64; 3], block: [f64; 3]) -> (CellId, CellId) {
    let side = b
        .block_faces()
        .into_iter()
        .find(|s| {
            near(centre(b, &s.nodes), face)
                && b.hex_corners(s.block)
                    .is_some_and(|c| near(centre(b, &c), block))
        })
        .expect("that face of that block");
    (side.face, side.block)
}

#[test]
fn a_pillow_surface_in_two_pieces_is_refused() {
    let mut b = cube();
    let before = cells(&b);
    let (top, bottom) = (face_at(&b, 2, 1.0), face_at(&b, 2, 0.0));
    assert!(matches!(
        b.pillow(&[top, bottom]),
        Err(BlockingError::PillowApart(_))
    ));
    assert_eq!(cells(&b), before);
}

#[test]
fn a_pillow_surface_that_branches_is_refused() {
    let mut b = cube();
    cut(&mut b, 0, 0.5);
    let before = cells(&b);
    let (left, right) = ([0.25, 0.5, 0.5], [0.75, 0.5, 0.5]);
    let surface = [
        seen(&b, [0.25, 0.5, 1.0], left),
        seen(&b, [0.75, 0.5, 1.0], right),
        seen(&b, [0.5, 0.5, 0.5], left),
    ];
    assert!(matches!(
        b.pillow(&surface),
        Err(BlockingError::PillowBranches(_))
    ));
    assert_eq!(cells(&b), before);
}

#[test]
fn a_pillow_surface_takes_one_side_of_itself() {
    let mut b = cube();
    cut(&mut b, 0, 0.5);
    let before = cells(&b);
    let (left, right) = ([0.25, 0.5, 0.5], [0.75, 0.5, 0.5]);
    let top = seen(&b, [0.25, 0.5, 1.0], left);
    // The top of the left block from below, and the wall between the two
    // from the right: the layer would have to change sides at their edge.
    assert!(matches!(
        b.pillow(&[top, seen(&b, [0.5, 0.5, 0.5], right)]),
        Err(BlockingError::PillowTwoSides(_))
    ));
    assert_eq!(cells(&b), before);
    // Both from the left block: one side, and it goes round that block.
    b.pillow(&[top, seen(&b, [0.5, 0.5, 0.5], left)])
        .expect("one side of an L");
    assert_eq!(b.nb_blocks(), 4);
}

#[test]
fn a_pillow_surface_cannot_stop_inside_the_blocks() {
    let mut b = cube();
    cut(&mut b, 0, 0.5);
    cut(&mut b, 1, 0.5);
    let before = cells(&b);
    // Half of the wall at x = 0.5: its edge on the axis x = y = 0.5 has a
    // block on every side.
    let half = seen(&b, [0.5, 0.25, 0.5], [0.25, 0.25, 0.5]);
    assert!(matches!(
        b.pillow(&[half]),
        Err(BlockingError::PillowEndsInside(_))
    ));
    assert_eq!(cells(&b), before);
}

#[test]
fn a_closed_pillow_surface_takes_its_inside() {
    let mut b = cube();
    for axis in 0..3 {
        cut(&mut b, axis, 1.0 / 3.0);
        cut(&mut b, axis, 2.0 / 3.0);
    }
    assert_eq!(b.nb_blocks(), 27);
    let middle = [0.5; 3];
    let faces = |b: &Blocking, from_outside: bool| -> Vec<(CellId, CellId)> {
        (0..3)
            .flat_map(|axis| [1.0 / 3.0, 2.0 / 3.0].map(move |v| (axis, v)))
            .map(|(axis, v)| {
                let mut f = middle;
                f[axis] = v;
                let mut outside = middle;
                outside[axis] = if v < 0.5 { 1.0 / 6.0 } else { 5.0 / 6.0 };
                seen(b, f, if from_outside { outside } else { middle })
            })
            .collect()
    };
    let before = cells(&b);
    assert!(matches!(
        b.pillow(&faces(&b, true)),
        Err(BlockingError::PillowOutside)
    ));
    assert_eq!(cells(&b), before);
    b.pillow(&faces(&b, false)).expect("round the middle block");
    assert_eq!(b.nb_blocks(), 33);
}

#[test]
fn a_pillow_surface_across_a_ring_of_blocks_opens_it() {
    // Eight blocks round a square hole. One wall across the ring, from the
    // hole to the outside: the blocks on either side of it still reach each
    // other the other way round, and that is no reason to refuse — at the
    // wall itself they are apart.
    let mut b = cube();
    for axis in 0..2 {
        cut(&mut b, axis, 1.0 / 3.0);
        cut(&mut b, axis, 2.0 / 3.0);
    }
    let hole = b
        .blocks()
        .into_iter()
        .find(|id| near(centre(&b, &b.hex_corners(*id).unwrap()), [0.5, 0.5, 0.5]))
        .expect("the middle block");
    b.remove_block(hole).unwrap();
    assert_eq!(b.nb_blocks(), 8);

    let (near_side, far_side) = ([0.5, 1.0 / 6.0, 0.5], [1.0 / 6.0, 1.0 / 6.0, 0.5]);
    let wall = [1.0 / 3.0, 1.0 / 6.0, 0.5];
    b.pillow(&[seen(&b, wall, near_side)])
        .expect("a wall across the ring");
    assert_eq!(b.nb_blocks(), 9);
    b.validate().expect("a valid structure");
    assert_eq!(edges_bounding_nothing(&b), vec![]);

    // The far block did not move, and is welded to the layer, not to the
    // block that was drawn in.
    let shared: Vec<_> = b
        .block_faces()
        .into_iter()
        .filter(|s| near(centre(&b, &s.nodes), wall))
        .collect();
    assert_eq!(shared.len(), 2, "one face, seen from each of two blocks");
    assert_eq!(shared[0].face, shared[1].face);
    let blocks: Vec<_> = shared
        .iter()
        .map(|s| centre(&b, &b.hex_corners(s.block).unwrap()))
        .collect();
    assert!(blocks.iter().any(|c| near(*c, far_side)), "{blocks:?}");
}
