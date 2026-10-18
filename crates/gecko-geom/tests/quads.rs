//! Choosing the four corners of a surface, and the sides between them.

use gecko_geom::msh::{Element, Msh, LINE, POINT, TRIANGLE};
use gecko_geom::{Geometry, QuadError};
use std::collections::HashMap;

/// A geometry built point by point, with **shared** nodes — the sharing is
/// what makes a curve bound a surface rather than lie beside one.
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

const A: [f64; 3] = [0.0, 0.0, 0.0];
const B: [f64; 3] = [0.5, 0.0, 0.0];
const C: [f64; 3] = [1.0, 0.0, 0.0];
const D: [f64; 3] = [1.0, 1.0, 0.0];
const E: [f64; 3] = [0.0, 1.0, 0.0];

/// The unit square, four sides and four corners, nothing to choose.
fn square() -> Geometry {
    let p = [A, C, D, E];
    let mut b = Build::default();
    for (k, tag) in (21..25).enumerate() {
        b.element(POINT, tag, &[p[k]]);
    }
    for (k, tag) in (11..15).enumerate() {
        b.element(LINE, tag, &[p[k], p[(k + 1) % 4]]);
    }
    b.element(TRIANGLE, 1, &[A, C, D]);
    b.element(TRIANGLE, 1, &[A, D, E]);
    b.done()
}

/// The same square with a **fifth** corner half way along its bottom side,
/// so that side is two curves and the corner between them is flat.
///
/// ```text
///   E ──14── D        corners 21 A, 22 B, 23 C, 24 D, 25 E
///   │        │        curves  11 A→B, 12 B→C, 13 C→D, 14 D→E, 15 E→A
///  15        13
///   │        │
///   A ─11─B─12─ C
/// ```
fn square_with_a_flat_corner() -> Geometry {
    let mut b = Build::default();
    for (p, tag) in [(A, 21), (B, 22), (C, 23), (D, 24), (E, 25)] {
        b.element(POINT, tag, &[p]);
    }
    for (from, to, tag) in [(A, B, 11), (B, C, 12), (C, D, 13), (D, E, 14), (E, A, 15)] {
        b.element(LINE, tag, &[from, to]);
    }
    for t in [[A, B, E], [B, C, E], [C, D, E]] {
        b.element(TRIANGLE, 1, &t);
    }
    b.done()
}

#[test]
fn a_square_is_blocked_by_its_own_four_corners() {
    let q = square().quad_on(1).expect("a quadrangle");
    let mut named = q.corners;
    named.sort_unstable();
    assert_eq!(named, [21, 22, 23, 24]);
    for angle in q.angles {
        assert!(
            (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
            "every corner of a square is a right angle, not {angle}"
        );
    }
    // One curve a side, and each side joins the corners it runs between.
    for side in &q.sides {
        assert_eq!(side.len(), 1, "{:?}", q.sides);
    }
}

#[test]
fn a_corner_the_boundary_does_not_turn_at_is_left_inside_a_side() {
    // Five corners, four right angles and one flat: the flat one is a point
    // along a side, not a corner of the quadrangle, and the side it is on
    // is two curves.
    let q = square_with_a_flat_corner()
        .quad_on(1)
        .expect("a quadrangle");
    assert!(
        !q.corners.contains(&22),
        "the flat corner was chosen: {:?}",
        q.corners
    );
    let mut named = q.corners;
    named.sort_unstable();
    assert_eq!(named, [21, 23, 24, 25]);

    let long: Vec<&Vec<i32>> = q.sides.iter().filter(|s| s.len() > 1).collect();
    assert_eq!(long.len(), 1, "one side of two curves: {:?}", q.sides);
    assert_eq!(long[0], &vec![11, 12], "and in the order they are met");
}

#[test]
fn the_corners_come_back_in_boundary_order() {
    // Not sorted by how square they are: a quadrangle built from corners in
    // the wrong order folds over itself. Consecutive corners of the answer
    // have to be joined by the side between them.
    let q = square_with_a_flat_corner()
        .quad_on(1)
        .expect("a quadrangle");
    assert_eq!(q.corners, [21, 23, 24, 25]);
    assert_eq!(q.points[0], A);
    assert_eq!(q.points[1], C);
    assert_eq!(q.points[2], D);
    assert_eq!(q.points[3], E);
    assert_eq!(q.sides, [vec![11, 12], vec![13], vec![14], vec![15]]);
}

#[test]
fn the_same_surface_always_gives_the_same_quadrangle() {
    // Four corners of a square are equally square, so the choice is a tie
    // four ways. It still has to be one answer, every time.
    let first = square().quad_on(1).unwrap();
    for _ in 0..8 {
        assert_eq!(square().quad_on(1).unwrap(), first);
    }
}

#[test]
fn a_surface_without_four_corners_is_refused() {
    // Three corners named on a square: there is no fourth to choose, and
    // guessing one from the facets would be inventing geometry.
    let mut b = Build::default();
    for (p, tag) in [(A, 21), (C, 22), (D, 23)] {
        b.element(POINT, tag, &[p]);
    }
    for (from, to, tag) in [(A, C, 11), (C, D, 12), (D, E, 13), (E, A, 14)] {
        b.element(LINE, tag, &[from, to]);
    }
    b.element(TRIANGLE, 1, &[A, C, D]);
    b.element(TRIANGLE, 1, &[A, D, E]);
    let g = b.done();

    assert_eq!(g.quad_on(1), Err(QuadError::TooFewCorners(1, 3)));
    assert_eq!(g.quad_on(7), Err(QuadError::NoSuchSurface(7)));
}
