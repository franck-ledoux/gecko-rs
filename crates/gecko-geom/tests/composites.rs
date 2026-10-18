//! Entities made of other entities: sides, and the surfaces that will come.

use gecko_geom::msh::{Element, Msh, LINE, POINT, TRIANGLE};
use gecko_geom::{Composites, Geometry, Model, CORNER, CURVE, SURFACE};
use std::collections::HashMap;

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

/// A square whose bottom side is two curves, 11 and 12, meeting at B.
fn square() -> Geometry {
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

/// Two squares meeting along the line x = 1: surfaces 1 and 2, sharing the
/// curve 13 between them.
fn pair() -> Geometry {
    let f = [2.0, 0.0, 0.0];
    let g = [2.0, 1.0, 0.0];
    let mut b = Build::default();
    for (p, tag) in [(A, 21), (C, 23), (D, 24), (E, 25), (f, 26), (g, 27)] {
        b.element(POINT, tag, &[p]);
    }
    for (from, to, tag) in [
        (A, C, 11),
        (C, D, 13),
        (D, E, 14),
        (E, A, 15),
        (C, f, 16),
        (f, g, 17),
        (g, D, 18),
    ] {
        b.element(LINE, tag, &[from, to]);
    }
    for t in [[A, C, D], [A, D, E]] {
        b.element(TRIANGLE, 1, &t);
    }
    for t in [[C, f, g], [C, g, D]] {
        b.element(TRIANGLE, 2, &t);
    }
    b.done()
}

#[test]
fn a_group_of_one_is_not_a_composite() {
    // Its own tag already names it, and wrapping it would hide what it is.
    let mut c = Composites::new();
    assert_eq!(c.group(CURVE, &[11]), 11);
    assert_eq!(c.group(CURVE, &[11, 11]), 11);
    assert!(c.is_empty());
}

#[test]
fn the_same_members_always_get_the_same_name() {
    // What makes the operation repeatable: a script replayed from the
    // journal must not leave a new name behind every time it runs. And a
    // side walked the other way round is the same side.
    let mut c = Composites::new();
    let first = c.group(CURVE, &[11, 12]);
    assert!(Composites::is_composite(first));
    assert_eq!(c.group(CURVE, &[11, 12]), first);
    assert_eq!(c.group(CURVE, &[12, 11]), first);
    assert_eq!(c.len(), 1);

    // A different group is a different name, and so is the same group at
    // another dimension.
    assert_ne!(c.group(CURVE, &[12, 13]), first);
    assert_ne!(c.group(SURFACE, &[11, 12]), first);
    assert_eq!(c.len(), 3);
}

#[test]
fn a_curve_belongs_to_as_many_sides_as_there_are_sides_using_it() {
    // Two surfaces that touch share boundary curves, so this is the usual
    // case and not a corner one: composites do not partition anything.
    let g = square();
    let mut c = Composites::new();
    let left = c.group(CURVE, &[11, 12]);
    let right = c.group(CURVE, &[12, 13]);
    let model = Model::new(&g, &c);

    let up = model.containing(CURVE, 12);
    assert!(up.contains(&(CURVE, left)), "{up:?}");
    assert!(up.contains(&(CURVE, right)), "{up:?}");
    // And the ones it is not in stay out of it.
    assert!(!model.containing(CURVE, 14).contains(&(CURVE, left)));
}

#[test]
fn a_side_projects_onto_whichever_of_its_curves_is_nearest() {
    let g = square();
    let mut c = Composites::new();
    let side = c.group(CURVE, &[11, 12]);
    let model = Model::new(&g, &c);

    // Over the first curve, over the second, and past the end of both.
    assert_eq!(
        model.project(CURVE, side, [0.2, 0.3, 0.0]),
        Some([0.2, 0.0, 0.0])
    );
    assert_eq!(
        model.project(CURVE, side, [0.8, 0.3, 0.0]),
        Some([0.8, 0.0, 0.0])
    );
    assert_eq!(model.project(CURVE, side, [1.5, 0.0, 0.0]), Some(C));
    assert!(model.distance(CURVE, side, [0.2, 0.3, 0.0]).unwrap() - 0.3 < 1e-9);
}

#[test]
fn a_side_is_inside_whatever_all_of_its_curves_are_inside() {
    let g = square();
    let mut c = Composites::new();
    let side = c.group(CURVE, &[11, 12]);
    let model = Model::new(&g, &c);

    let up = model.containing(CURVE, side);
    assert!(up.contains(&(CURVE, side)), "itself first: {up:?}");
    assert!(
        up.contains(&(SURFACE, 1)),
        "the surface it runs along: {up:?}"
    );
    // Its own members are *in* it, not above it.
    assert!(!up.contains(&(CURVE, 11)), "{up:?}");
}

#[test]
fn welding_a_side_to_one_of_its_curves_keeps_the_curve() {
    // The curve is the stronger claim — a point on it is on the side too —
    // and only an up-set that knows about sides can say so.
    let g = square();
    let mut c = Composites::new();
    let side = c.group(CURVE, &[11, 12]);
    let model = Model::new(&g, &c);

    assert_eq!(
        model.meet((CURVE, side), (CURVE, 11), [0.2, 0.0, 0.0]),
        Some((CURVE, 11))
    );
    assert_eq!(
        model.meet((CURVE, 11), (CURVE, side), [0.2, 0.0, 0.0]),
        Some((CURVE, 11))
    );
    // And a side welded to the surface it lies on keeps the side.
    assert_eq!(
        model.meet((CURVE, side), (SURFACE, 1), [0.2, 0.0, 0.0]),
        Some((CURVE, side))
    );
}

#[test]
fn neither_proximity_nor_inference_ever_answers_with_a_side() {
    // A name that turned up under a reader who never asked for it would be
    // worse than no name. Deliberate assignment, or nothing.
    let g = square();
    let mut c = Composites::new();
    let side = c.group(CURVE, &[11, 12]);
    let model = Model::new(&g, &c);

    let near = model.nearest([0.5, 0.01, 0.0], [0.1; 3]);
    assert_ne!(
        near,
        Some((CURVE, side)),
        "proximity answered with the side"
    );
    assert!(
        near.is_some_and(|e| !Composites::is_composite(e.1)),
        "proximity answered {near:?}"
    );

    // The two ends of the side: what they share is the surface, not the
    // side — a diagonal across the face has the same two ends.
    assert_eq!(
        model.common((CORNER, 21), (CORNER, 23), CURVE),
        Some((SURFACE, 1))
    );
}

#[test]
fn a_composite_surface_works_the_same_way() {
    // The reason this is one notion and not "composite curves": a face of a
    // blocking spanning two geometric surfaces needs exactly this, and gets
    // it without a line of new machinery.
    let g = pair();
    let mut c = Composites::new();
    let both = c.group(SURFACE, &[1, 2]);
    let model = Model::new(&g, &c);

    // It projects onto whichever member is nearer, and it is above both.
    assert_eq!(
        model.project(SURFACE, both, [0.5, 0.5, 3.0]),
        Some([0.5, 0.5, 0.0])
    );
    assert_eq!(
        model.project(SURFACE, both, [1.5, 0.5, 3.0]),
        Some([1.5, 0.5, 0.0])
    );
    assert!(model.containing(SURFACE, 1).contains(&(SURFACE, both)));
    assert!(model.containing(SURFACE, 2).contains(&(SURFACE, both)));

    // The seam between the two is inside it, so it is above that too.
    assert!(model.containing(CURVE, 13).contains(&(SURFACE, both)));

    // And welding it to one of its own surfaces keeps that surface.
    assert_eq!(
        model.meet((SURFACE, both), (SURFACE, 1), [0.5, 0.5, 0.0]),
        Some((SURFACE, 1))
    );
}
