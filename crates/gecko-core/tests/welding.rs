//! Where a welded corner ends up.
//!
//! Gluing is two things at once: a topological weld, which
//! [`gluing`](../gluing.rs) is about, and a decision about **one point**
//! where there were two. This file is only about the point.
//!
//! The rule is that neither block is the privileged one — they meet in the
//! middle — and that classification overrides it, because a node pinned to
//! the geometry is pinned for a reason the glue does not know. Both closures
//! stand in for the geometry here, so each rule can be put under a light of
//! its own instead of needing a cylinder on hand.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError, On};

/// A geometry that knows nothing: what the rule does on its own.
fn nothing_shared(_: On, _: On, _: [f64; 3]) -> Option<On> {
    None
}
fn no_projection(_: On, _: [f64; 3]) -> Option<[f64; 3]> {
    None
}

/// Two unit cubes facing each other across a gap in x, the second's near
/// face at `1.0 + gap`.
fn facing(gap: f64) -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_block(cube_corners([1.0 + gap, 0.0, 0.0], 1.0))
        .unwrap();
    b
}

/// The face of block number `block` whose four corners all sit at this x.
fn face_at_x(b: &Blocking, block: usize, x: f64) -> CellId {
    let sides = b.block_faces();
    let mut names: Vec<_> = sides.iter().map(|s| s.block).collect();
    names.dedup();
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

/// The node nearest a point, by name.
fn node_at(b: &Blocking, p: [f64; 3]) -> CellId {
    b.nodes()
        .min_by(|(_, a), (_, c)| {
            let d = |q: &[f64; 3]| (0..3).map(|k| (q[k] - p[k]).powi(2)).sum::<f64>();
            d(a).total_cmp(&d(c))
        })
        .expect("a node")
        .0
}

/// How many nodes sit at this x.
fn nodes_at_x(b: &Blocking, x: f64) -> usize {
    b.nodes().filter(|(_, p)| (p[0] - x).abs() < 1e-9).count()
}

/// Where the node that started at `was` has ended up.
fn moved_to(b: &Blocking, name: CellId) -> [f64; 3] {
    b.node_by_id(name)
        .and_then(|v| b.position(v))
        .expect("the node is still there")
}

#[test]
fn two_faces_meet_in_the_middle() {
    let mut b = facing(0.5);
    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    b.glue_faces(first, second, nothing_shared, no_projection)
        .expect("two free faces");

    // Four nodes, half way. Not eight: the weld is the point of the
    // operation, and not at 1.0 or 1.5, which would be one block giving way
    // entirely.
    assert_eq!(nodes_at_x(&b, 1.25), 4, "the welded corners are at 1.25");
    assert_eq!(nodes_at_x(&b, 1.0), 0);
    assert_eq!(nodes_at_x(&b, 1.5), 0);

    // And nothing else moved: both blocks gave way by half, and neither was
    // carried over.
    assert_eq!(nodes_at_x(&b, 0.0), 4, "the first block's far face");
    assert_eq!(nodes_at_x(&b, 2.5), 4, "the second block's far face");
    assert_eq!(b.nb_nodes(), 12);
}

#[test]
fn a_frozen_corner_does_not_give_way() {
    let mut b = facing(0.5);
    let pinned = node_at(&b, [1.0, 0.0, 0.0]);
    b.set_frozen(pinned, true).unwrap();

    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    b.glue_faces(first, second, nothing_shared, no_projection)
        .expect("one frozen corner is not a contradiction");

    // Its partner came all the way to it, while the other three pairs met in
    // the middle. A frozen node is one somebody has decided about.
    assert_eq!(moved_to(&b, pinned), [1.0, 0.0, 0.0]);
    assert_eq!(nodes_at_x(&b, 1.25), 3);
    assert!(b.is_frozen(pinned), "and it is still frozen");
}

#[test]
fn two_frozen_corners_that_disagree_refuse_the_glue() {
    // Once per corner of the face. The four pairs are decided in whatever
    // order the darts run in, and the promise is that **no** corner has
    // moved when the refusal comes — which a single corner cannot check,
    // because it might be the one decided first.
    for corner in [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]] {
        let mut b = facing(0.5);
        let here = node_at(&b, [1.0, corner[0], corner[1]]);
        let there = node_at(&b, [1.5, corner[0], corner[1]]);
        for n in [here, there] {
            b.set_frozen(n, true).unwrap();
        }

        let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
        assert!(
            matches!(
                b.glue_faces(first, second, nothing_shared, no_projection),
                Err(BlockingError::FrozenApart(..))
            ),
            "corner {corner:?} should have been a contradiction"
        );

        // And it left everything as it was. A refusal that has already moved
        // half the corners is worse than no refusal at all.
        assert_eq!(b.nb_nodes(), 16, "nothing was welded");
        assert_eq!(moved_to(&b, here), [1.0, corner[0], corner[1]]);
        assert_eq!(moved_to(&b, there), [1.5, corner[0], corner[1]]);
        assert_eq!(nodes_at_x(&b, 1.0), 4, "corner {corner:?}");
        assert_eq!(nodes_at_x(&b, 1.5), 4, "corner {corner:?}");
    }
}

#[test]
fn two_frozen_corners_in_the_same_place_are_no_obstacle() {
    // Nobody has to move, so there is nothing to refuse.
    let mut b = facing(0.0);
    for p in [[1.0, 0.0, 0.0], [1.0, 1.0, 1.0]] {
        let n = node_at(&b, p);
        b.set_frozen(n, true).unwrap();
    }
    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.0));
    b.glue_faces(first, second, nothing_shared, no_projection)
        .expect("frozen, but not in disagreement");
    assert_eq!(b.nb_nodes(), 12);
}

#[test]
fn the_more_constrained_classification_survives() {
    let mut b = facing(0.5);
    let on_a_surface = node_at(&b, [1.0, 0.0, 0.0]);
    let on_a_curve = node_at(&b, [1.5, 0.0, 0.0]);
    b.set_node_class(on_a_surface, Some(On { dim: 2, tag: 5 }))
        .unwrap();
    b.set_node_class(on_a_curve, Some(On { dim: 1, tag: 3 }))
        .unwrap();

    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    let unmet = b
        .glue_faces(first, second, nothing_shared, no_projection)
        .expect("two free faces");

    // A point on a curve is on every surface the curve bounds, so the curve
    // is the statement that has to be kept. The surface is not merely
    // outranked, it is **implied** — when the two really do meet. Here the
    // geometry says they share nothing, so one claim was dropped and the
    // caller is told which.
    assert_eq!(b.node_class(on_a_surface), Some(On { dim: 1, tag: 3 }));
    assert_eq!(unmet.len(), 1);
    assert_eq!(unmet[0].kept, On { dim: 1, tag: 3 });
    assert_eq!(unmet[0].dropped, On { dim: 2, tag: 5 });
}

#[test]
fn a_classification_on_one_side_is_never_dropped() {
    let mut b = facing(0.5);
    let classified = node_at(&b, [1.5, 0.0, 0.0]);
    b.set_node_class(classified, Some(On { dim: 1, tag: 7 }))
        .unwrap();

    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    let unmet = b
        .glue_faces(first, second, nothing_shared, no_projection)
        .expect("two free faces");

    // The surviving name is the first face's, so the question is asked of
    // the corner that was *not* classified: the constraint followed the
    // point, not the name.
    let welded = node_at(&b, [1.25, 0.0, 0.0]);
    assert_eq!(b.node_class(welded), Some(On { dim: 1, tag: 7 }));
    assert!(unmet.is_empty(), "nothing was in contradiction");
}

#[test]
fn what_the_two_classifications_share_is_what_the_weld_is_on() {
    let mut b = facing(0.5);
    for (p, on) in [
        ([1.0, 0.0, 0.0], On { dim: 2, tag: 5 }),
        ([1.5, 0.0, 0.0], On { dim: 2, tag: 6 }),
    ] {
        let n = node_at(&b, p);
        b.set_node_class(n, Some(on)).unwrap();
    }

    // Two surfaces that meet along curve 9 — which is neither of the two,
    // and is the answer.
    let seam = |a: On, c: On, _: [f64; 3]| {
        (a == On { dim: 2, tag: 5 } && c == On { dim: 2, tag: 6 }).then_some(On { dim: 1, tag: 9 })
    };
    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    let unmet = b
        .glue_faces(first, second, seam, no_projection)
        .expect("two free faces");

    let welded = node_at(&b, [1.25, 0.0, 0.0]);
    assert_eq!(b.node_class(welded), Some(On { dim: 1, tag: 9 }));
    assert!(unmet.is_empty(), "they met, so nothing was dropped");
}

#[test]
fn the_weld_lands_on_what_it_is_classified_on() {
    let mut b = facing(0.5);
    let on_a_curve = node_at(&b, [1.0, 0.0, 0.0]);
    b.set_node_class(on_a_curve, Some(On { dim: 1, tag: 3 }))
        .unwrap();

    // A curve that happens to run at y = 0.25. The midpoint is not on it,
    // and a node classified on a curve that is not on the curve drags the
    // mesh out through the boundary when it is projected later.
    let onto = |on: On, p: [f64; 3]| (on.dim == 1).then_some([p[0], 0.25, p[2]]);
    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    b.glue_faces(first, second, nothing_shared, onto)
        .expect("two free faces");

    assert_eq!(moved_to(&b, on_a_curve), [1.25, 0.25, 0.0]);
}

#[test]
fn a_frozen_corner_is_not_projected() {
    let mut b = facing(0.5);
    let pinned = node_at(&b, [1.0, 0.0, 0.0]);
    b.set_node_class(pinned, Some(On { dim: 1, tag: 3 }))
        .unwrap();
    b.set_frozen(pinned, true).unwrap();

    // The projection would move it, and frozen means it does not move — for
    // anybody, including a classification it carries itself.
    let elsewhere = |_: On, _: [f64; 3]| Some([9.0, 9.0, 9.0]);
    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    b.glue_faces(first, second, nothing_shared, elsewhere)
        .expect("two free faces");

    assert_eq!(moved_to(&b, pinned), [1.0, 0.0, 0.0]);
}

#[test]
fn a_tie_between_two_classifications_goes_to_the_first_face() {
    let mut b = facing(0.5);
    for (p, on) in [
        ([1.0, 0.0, 0.0], On { dim: 1, tag: 3 }),
        ([1.5, 0.0, 0.0], On { dim: 1, tag: 4 }),
    ] {
        let n = node_at(&b, p);
        b.set_node_class(n, Some(on)).unwrap();
    }

    // Two curves of the same dimension that share nothing: there is no
    // reason in the geometry to prefer either, so the tie goes the way the
    // name does when two nodes merge — to the first.
    let (first, second) = (face_at_x(&b, 0, 1.0), face_at_x(&b, 1, 1.5));
    let unmet = b
        .glue_faces(first, second, nothing_shared, no_projection)
        .expect("two free faces");

    let welded = node_at(&b, [1.25, 0.0, 0.0]);
    assert_eq!(b.node_class(welded), Some(On { dim: 1, tag: 3 }));
    assert_eq!(unmet[0].kept, On { dim: 1, tag: 3 });
    assert_eq!(unmet[0].dropped, On { dim: 1, tag: 4 });
}
