//! Where a point is on the geometry, and what contains what.

use gecko_geom::msh::{Element, Msh, LINE, POINT, TRIANGLE};
use gecko_geom::{Geometry, CORNER, CURVE, SURFACE};
use std::collections::HashMap;

/// A geometry built point by point, with **shared** nodes.
///
/// Sharing is the whole subject here: a corner is on a curve when its node
/// *is* one of the curve's, so a fixture that gave every element its own
/// copy of each point would report no containment at all and the tests
/// would pass by agreeing with nothing.
#[derive(Default)]
struct Build {
    msh: Msh,
    at: HashMap<[u64; 3], u32>,
}

impl Build {
    fn node(&mut self, p: [f64; 3]) -> u32 {
        let key = p.map(|x| x.to_bits());
        *self.at.entry(key).or_insert_with(|| {
            self.msh.nodes.push(p);
            (self.msh.nodes.len() - 1) as u32
        })
    }

    fn element(&mut self, kind: i32, tag: i32, points: &[[f64; 3]]) {
        let mut nodes = [0; 3];
        for (slot, p) in nodes.iter_mut().zip(points) {
            *slot = self.node(*p);
        }
        self.msh.elements.push(Element {
            kind,
            physical: 0,
            entity: tag,
            nodes,
        });
    }

    fn done(self) -> Geometry {
        Geometry::of(&self.msh).expect("a geometry")
    }
}

/// The unit square in z = 0: one surface, its four sides, its four corners.
///
/// ```text
///   4 ──13── 3
///   │        │        surface 1
///  14        12       curves 11 bottom, 12 right, 13 top, 14 left
///   │        │        corners 21..24 anticlockwise from the origin
///   1 ──11── 2
/// ```
fn square() -> Geometry {
    let p = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut b = Build::default();
    for (k, tag) in (21..25).enumerate() {
        b.element(POINT, tag, &[p[k]]);
    }
    for (k, tag) in (11..15).enumerate() {
        b.element(LINE, tag, &[p[k], p[(k + 1) % 4]]);
    }
    b.element(TRIANGLE, 1, &[p[0], p[1], p[2]]);
    b.element(TRIANGLE, 1, &[p[0], p[2], p[3]]);
    b.done()
}

#[test]
fn what_contains_a_corner_is_every_curve_and_surface_through_it() {
    let g = square();
    // Corner 21 is the origin: the bottom side, the left side, the face.
    assert_eq!(
        g.containing(CORNER, 21),
        [(CORNER, 21), (CURVE, 11), (CURVE, 14), (SURFACE, 1)]
    );
    // Itself first and always, so intersecting two up-sets never comes back
    // empty merely because both cells are on the same thing.
    assert_eq!(g.containing(CURVE, 12), [(CURVE, 12), (SURFACE, 1)]);
    assert_eq!(g.containing(SURFACE, 1), [(SURFACE, 1)]);
    assert!(g.containing(CURVE, 99).is_empty());
}

#[test]
fn an_edge_between_two_corners_lands_on_what_they_share() {
    let g = square();
    // Along a side: both its ends are on that curve, so the edge is.
    assert_eq!(
        g.common((CORNER, 21), (CORNER, 22), CURVE),
        Some((CURVE, 11))
    );
    assert_eq!(
        g.common((CORNER, 22), (CORNER, 23), CURVE),
        Some((CURVE, 12))
    );
    // Across the diagonal: the two ends share no curve, but they share the
    // face, so that is where the edge lies. This is the case proximity
    // cannot get right — the diagonal passes nowhere near any curve, and a
    // search from its midpoint would say "surface" for the right reason by
    // luck and "curve" for the wrong reason just as often.
    assert_eq!(
        g.common((CORNER, 21), (CORNER, 23), CURVE),
        Some((SURFACE, 1))
    );
}

#[test]
fn an_edge_is_never_pinned_below_its_own_dimension() {
    let g = square();
    // Both ends on one corner would otherwise put the edge *on* that
    // corner, which is a point, and an edge is not.
    assert_eq!(
        g.common((CORNER, 21), (CORNER, 21), CURVE),
        Some((CURVE, 11))
    );
    // Asked at dimension 0, the corner itself is the answer — which is what
    // a *node* between two of its own classifications would want.
    assert_eq!(
        g.common((CORNER, 21), (CORNER, 21), CORNER),
        Some((CORNER, 21))
    );
}

#[test]
fn two_ends_with_nothing_in_common_classify_nowhere() {
    // Two squares side by side and not touching: no node is shared, so
    // nothing contains anything of the other, and an edge between them is
    // on nothing. Saying so is the point — the alternative is a cell
    // claiming to lie on a surface it is nowhere near.
    let mut b = Build::default();
    for (dx, surface, corner) in [(0.0, 1, 21), (5.0, 2, 31)] {
        let p = [
            [dx, 0.0, 0.0],
            [dx + 1.0, 0.0, 0.0],
            [dx + 1.0, 1.0, 0.0],
            [dx, 1.0, 0.0],
        ];
        b.element(POINT, corner, &[p[0]]);
        b.element(TRIANGLE, surface, &[p[0], p[1], p[2]]);
        b.element(TRIANGLE, surface, &[p[0], p[2], p[3]]);
    }
    let g = b.done();
    assert_eq!(g.common((CORNER, 21), (CORNER, 31), CURVE), None);
}

#[test]
fn a_point_is_pulled_onto_the_entity_it_is_classified_on() {
    let g = square();
    assert_eq!(
        g.project(CORNER, 21, [9.0, 9.0, 9.0]),
        Some([0.0, 0.0, 0.0])
    );
    // Onto a side, from outside the square and from above it.
    assert_eq!(
        g.project(CURVE, 11, [0.5, -2.0, 0.0]),
        Some([0.5, 0.0, 0.0])
    );
    assert_eq!(g.project(CURVE, 11, [0.5, 0.2, 3.0]), Some([0.5, 0.0, 0.0]));
    // Past the end of a side: onto its end, not past it.
    assert_eq!(g.project(CURVE, 11, [4.0, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
    // Onto the face, straight down.
    assert_eq!(
        g.project(SURFACE, 1, [0.25, 0.25, 7.0]),
        Some([0.25, 0.25, 0.0])
    );
    // And onto its edge when the foot of that drop would miss it.
    let off = g.project(SURFACE, 1, [3.0, 0.5, 1.0]).expect("a point");
    assert!(
        (off[0] - 1.0).abs() < 1e-12 && (off[1] - 0.5).abs() < 1e-12,
        "{off:?}"
    );
    assert_eq!(g.project(SURFACE, 99, [0.0; 3]), None);
}

#[test]
fn a_point_past_a_corner_of_a_triangle_lands_on_that_corner() {
    // The seven regions of the closest-point search, the three corner ones
    // being the easiest to get wrong by projecting onto the plane first.
    let g = square();
    for (from, want) in [
        ([-3.0, -3.0, 2.0], [0.0, 0.0, 0.0]),
        ([4.0, -3.0, 0.0], [1.0, 0.0, 0.0]),
        ([4.0, 4.0, -1.0], [1.0, 1.0, 0.0]),
        ([-2.0, 5.0, 0.0], [0.0, 1.0, 0.0]),
    ] {
        assert_eq!(g.project(SURFACE, 1, from), Some(want), "from {from:?}");
    }
}

#[test]
fn the_nearest_entity_is_the_lowest_dimensional_one_in_range() {
    let g = square();
    let loose = [0.5, 0.5, 0.5];
    // Right by a corner: the corner, not one of the two curves through it
    // nor the face, though it is within tolerance of all four.
    assert_eq!(g.nearest([0.02, 0.02, 0.0], loose), Some((CORNER, 21)));
    // Along a side but away from its ends: the curve.
    assert_eq!(g.nearest([0.5, 0.01, 0.0], loose), Some((CURVE, 11)));
    // Over the middle: the face.
    assert_eq!(g.nearest([0.5, 0.5, 0.2], loose), Some((SURFACE, 1)));
    // Far away from everything: nothing, rather than the least bad thing.
    assert_eq!(g.nearest([50.0, 50.0, 50.0], loose), None);
}

#[test]
fn each_dimension_has_a_tolerance_of_its_own() {
    // Two corners are typically far closer to each other than either is to
    // a curve it does not touch, so the corner tolerance is the tight one
    // and has to be settable on its own.
    let g = square();
    let near_corner = [0.1, 0.0, 0.0];
    // Tight on corners, loose on curves: the curve wins.
    assert_eq!(g.nearest(near_corner, [0.05, 1.0, 1.0]), Some((CURVE, 11)));
    // Loose on corners: the corner wins, being the lower dimension.
    assert_eq!(g.nearest(near_corner, [0.5, 1.0, 1.0]), Some((CORNER, 21)));
    // Nothing allowed anywhere: nothing — though a tolerance of nought
    // still catches a point lying exactly on something, which this one
    // does, so the question is asked from just off the curve.
    assert_eq!(g.nearest([0.1, 0.01, 0.0], [0.0; 3]), None);
    assert_eq!(g.nearest(near_corner, [0.0; 3]), Some((CURVE, 11)));
}

/// A square of ground with an arch over it, sharing **both** of its sides.
///
/// ```text
///        M───────N           surface 1  the ground, z = 0
///       /       /            surface 2  the arch, over it
///      /       /             curves 12 at x = 1, 14 at x = 0 — both of
///     A───────B                         them on the ground *and* on the
///     │       │                         arch, which is the point
///     D───────C              corners 21 at A, 22 at B
/// ```
///
/// Two surfaces meeting along one curve is the ordinary case; meeting along
/// two is what makes the position matter, and a cylinder split into halves
/// is exactly this.
fn arch() -> Geometry {
    let (a, b, c, d) = (
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    );
    let (m, n) = ([0.5, 0.0, 1.0], [0.5, 1.0, 1.0]);
    let mut g = Build::default();
    g.element(POINT, 21, &[a]);
    g.element(POINT, 22, &[b]);
    g.element(LINE, 12, &[b, c]);
    g.element(LINE, 14, &[a, d]);
    // The ground.
    g.element(TRIANGLE, 1, &[a, b, c]);
    g.element(TRIANGLE, 1, &[a, c, d]);
    // The arch, rising from one side and coming down on the other.
    g.element(TRIANGLE, 2, &[a, m, n]);
    g.element(TRIANGLE, 2, &[a, n, d]);
    g.element(TRIANGLE, 2, &[m, b, c]);
    g.element(TRIANGLE, 2, &[m, c, n]);
    g.done()
}

#[test]
fn what_two_surfaces_meet_along_is_the_curve_between_them() {
    let g = arch();
    // Neither of the two is the answer: a point on both is on their seam.
    // Which seam cannot be answered without the position — here there are
    // two, and nothing but the point says which one a weld is on.
    assert_eq!(
        g.meet((SURFACE, 1), (SURFACE, 2), [0.0, 0.5, 0.0]),
        Some((CURVE, 14))
    );
    assert_eq!(
        g.meet((SURFACE, 1), (SURFACE, 2), [1.0, 0.5, 0.0]),
        Some((CURVE, 12))
    );
}

#[test]
fn the_more_constrained_of_two_nested_entities_is_the_answer() {
    let g = arch();
    // A point on the curve is already on the surface it bounds, so the
    // curve is the statement to keep — and the order of the two arguments
    // is not supposed to matter.
    for near in [[1.0, 0.5, 0.0], [0.0, 0.0, 0.0]] {
        assert_eq!(g.meet((CURVE, 12), (SURFACE, 1), near), Some((CURVE, 12)));
        assert_eq!(g.meet((SURFACE, 1), (CURVE, 12), near), Some((CURVE, 12)));
    }
    // And a corner beats the curve through it, for the same reason.
    assert_eq!(
        g.meet((CORNER, 21), (CURVE, 14), [0.0, 0.0, 0.0]),
        Some((CORNER, 21))
    );
}

#[test]
fn a_weld_at_the_end_of_a_seam_lands_on_the_corner() {
    let g = arch();
    // Corner 21 is on both surfaces too, and at the origin it is just as
    // near as curve 14 is. A tie goes to the lower dimension, which is the
    // stronger claim and the better answer: that really is the end.
    assert_eq!(
        g.meet((SURFACE, 1), (SURFACE, 2), [0.0, 0.0, 0.0]),
        Some((CORNER, 21))
    );
    // A little way along the seam, the curve wins again.
    assert_eq!(
        g.meet((SURFACE, 1), (SURFACE, 2), [0.0, 0.4, 0.0]),
        Some((CURVE, 14))
    );
}

#[test]
fn an_entity_meets_itself_and_nothing_else_is_needed() {
    let g = arch();
    for e in [(SURFACE, 1), (CURVE, 12), (CORNER, 21)] {
        assert_eq!(g.meet(e, e, [0.3, 0.3, 0.3]), Some(e));
    }
    // Including from outside it, where the nearest point of the surface is
    // on its own boundary curve: that curve is on the surface, so a search
    // for what they share would tie and hand back the curve. An entity
    // meets itself and that is the end of it.
    assert_eq!(
        g.meet((SURFACE, 1), (SURFACE, 1), [2.0, 0.5, 0.0]),
        Some((SURFACE, 1))
    );
}

#[test]
fn two_entities_that_share_nothing_meet_nowhere() {
    let g = arch();
    // The far side of the ground is not on the arch, and nothing is on
    // both. Said so rather than guessed at.
    assert_eq!(g.meet((CURVE, 14), (CURVE, 12), [0.5, 0.5, 0.0]), None);
    assert_eq!(g.meet((CORNER, 21), (CURVE, 12), [0.5, 0.5, 0.0]), None);
    // And an entity nobody has heard of meets nothing.
    assert_eq!(g.meet((CURVE, 99), (SURFACE, 1), [0.0; 3]), None);
}
