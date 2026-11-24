//! Entities made of other entities: sides, and the surfaces that will come.

use gecko_geom::msh::{Element, Msh, LINE, POINT, TRIANGLE};
use gecko_geom::{Composites, Geometry, MergeError, Model, CORNER, CURVE, SURFACE};
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

#[test]
fn a_side_across_two_surfaces_is_inside_the_patch_of_both() {
    // Curve 11 bounds surface 1 only and curve 16 surface 2 only, so no
    // surface holds the pair — but the patch of both does, and a face put
    // on that patch must be allowed an edge put on that side.
    let g = pair();
    let mut c = Composites::new();
    let side = c.group(CURVE, &[11, 16]);
    let both = c.group(SURFACE, &[1, 2]);
    let one = c.group(SURFACE, &[1, 1]);
    let model = Model::new(&g, &c);

    let up = model.containing(CURVE, side);
    assert!(up.contains(&(SURFACE, both)), "{up:?}");
    assert!(!up.contains(&(SURFACE, 1)), "{up:?}");
    assert!(!up.contains(&(SURFACE, 2)), "{up:?}");
    // A group of one is the surface itself, so nothing new there either.
    assert_eq!(one, 1);

    // And a patch is not inside a side, nor a side inside a side it is
    // not made of.
    let other = c.group(CURVE, &[13, 14]);
    let model = Model::new(&g, &c);
    assert!(!model.containing(SURFACE, both).contains(&(CURVE, side)));
    assert!(!model.containing(CURVE, side).contains(&(CURVE, other)));
}

#[test]
fn two_surfaces_merged_swallow_the_seam_between_them() {
    // Surfaces 1 and 2 meet along curve 13; merged, they are one surface
    // and the seam is no longer anywhere a node can be found.
    let g = pair();
    let mut c = Composites::new();
    let both = c.merge(&g, SURFACE, &[2, 1]).unwrap();
    assert!(c.is_merged(both));
    assert_eq!(c.stand_in(SURFACE, 1), Some((SURFACE, both)));
    assert_eq!(c.stand_in(CURVE, 13), Some((SURFACE, both)), "the seam");
    // The corners at the seam's ends still have a curve of their own on
    // either side, so they stay.
    assert_eq!(c.stand_in(CORNER, 23), None);
    let model = Model::new(&g, &c);

    // Near the middle of the seam, on the surface: not on curve 13.
    let reach = [0.05, 0.05, 0.05];
    assert_eq!(
        model.nearest([1.0, 0.5, 0.01], reach),
        Some((SURFACE, both))
    );
    // And on the rest as before.
    assert_eq!(model.nearest([0.5, 0.0, 0.01], reach), Some((CURVE, 11)));
    assert!(!model.visible(CURVE).contains(&13));
    assert!(model.visible(SURFACE) == vec![both]);
}

#[test]
fn two_curves_merged_swallow_the_corner_between_them() {
    // The bottom of the pair is curves 11 and 16, meeting at corner 23,
    // where the seam 13 starts too. With the surfaces merged the seam has
    // gone into the surface, and with the curves merged the corner has
    // nothing left through it but the merged curve.
    let g = pair();
    let mut c = Composites::new();
    let both = c.merge(&g, SURFACE, &[1, 2]).unwrap();
    let bottom = c.merge(&g, CURVE, &[11, 16]).unwrap();
    assert_eq!(c.stand_in(CORNER, 23), Some((CURVE, bottom)));
    assert_eq!(c.swallowed(bottom), vec![(CORNER, 23)]);
    assert_eq!(c.swallowed(both), vec![(CURVE, 13)]);
    let model = Model::new(&g, &c);
    assert_eq!(
        model.nearest([1.0, 0.0, 0.0], [0.05; 3]),
        Some((CURVE, bottom)),
        "near the corner, on the merged curve"
    );
    // The far ends are ends still.
    assert_eq!(model.nearest(A, [0.05; 3]), Some((CORNER, 21)));
    // A node held by the merged curve slides on it, not on a piece of it.
    assert_eq!(
        model.destination(&[(CURVE, bottom)], [1.2, 0.0, 0.0]),
        Some((CURVE, bottom))
    );
}

#[test]
fn a_merge_is_refused_what_it_cannot_be() {
    let g = pair();
    let mut c = Composites::new();
    assert_eq!(c.merge(&g, SURFACE, &[1]), Err(MergeError::TooFew));
    assert_eq!(
        c.merge(&g, CORNER, &[21, 23]),
        Err(MergeError::NotMergeable)
    );
    assert_eq!(
        c.merge(&g, SURFACE, &[1, 9]),
        Err(MergeError::NotThere("surface 9".to_string()))
    );
    c.merge(&g, CURVE, &[11, 16]).unwrap();
    assert_eq!(
        c.merge(&g, CURVE, &[16, 17]),
        Err(MergeError::Taken("curve 16".to_string()))
    );
    // Without a merge nothing changed: a corner is a corner.
    let plain = Composites::new();
    assert_eq!(
        Model::new(&g, &plain).nearest([1.0, 0.0, 0.0], [0.05; 3]),
        Some((CORNER, 23))
    );
}

/// `pair`, with the second square folded up square to the first along
/// curve 13.
fn folded() -> Geometry {
    let f = [1.0, 0.0, 1.0];
    let g = [1.0, 1.0, 1.0];
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
fn what_meets_smoothly_is_merged_and_what_meets_at_a_crease_is_not() {
    // Flat across curve 13: the two squares are one surface, and the two
    // straight runs through its ends are one curve each.
    let g = pair();
    let mut c = Composites::new();
    let found = c.merge_smooth(&g, 10.0);
    assert_eq!(found.merges.len(), 3, "{found:?}");
    let surface = c.image(SURFACE, 1);
    assert_eq!(c.image(SURFACE, 2), surface);
    assert_eq!(c.image(CURVE, 13), surface, "the seam went into it");
    assert_eq!(c.image(CURVE, 11), c.image(CURVE, 16), "the bottom run");
    assert_eq!(c.image(CURVE, 14), c.image(CURVE, 18), "the top run");
    assert_ne!(c.image(CURVE, 11), c.image(CURVE, 14));
    // The outer corners turn 90°, and stay.
    assert_eq!(c.stand_in(CORNER, 21), None);
    let seam = found
        .joins
        .iter()
        .find(|j| j.across == (CURVE, 13))
        .expect("the seam was looked at");
    assert!(seam.merged && seam.degrees < 1e-9, "{seam:?}");

    // Folded square, it is a crease: nothing merged, and it says how far.
    let g = folded();
    let mut c = Composites::new();
    let found = c.merge_smooth(&g, 10.0);
    assert!(found.merges.is_empty(), "{found:?}");
    let seam = found
        .joins
        .iter()
        .find(|j| j.across == (CURVE, 13))
        .unwrap();
    assert!((seam.degrees - 90.0).abs() < 1e-4, "{seam:?}");
    // And the curves through its ends lie on different surfaces, so they
    // are not run together either, straight or not.
    assert_eq!(c.stand_in(CURVE, 11), None);
}

/// A tube of 12 facets, radius 1 from z = 0 to z = 1: surface 1, cut down
/// its side by the seam 10 from corner 3 at the bottom to corner 4 at the
/// top, its rims the closed curves 5 and 6 starting and ending at those
/// corners — a cylinder as a CAD export leaves one.
fn tube() -> Geometry {
    let n = 12;
    let at = |k: usize, z: f64| {
        let a = std::f64::consts::TAU * (k % n) as f64 / n as f64;
        [a.cos(), a.sin(), z]
    };
    let mut b = Build::default();
    b.element(POINT, 3, &[at(0, 0.0)]);
    b.element(POINT, 4, &[at(0, 1.0)]);
    b.element(LINE, 10, &[at(0, 0.0), at(0, 1.0)]);
    for k in 0..n {
        b.element(LINE, 5, &[at(k, 0.0), at(k + 1, 0.0)]);
        b.element(LINE, 6, &[at(k, 1.0), at(k + 1, 1.0)]);
        b.element(TRIANGLE, 1, &[at(k, 0.0), at(k + 1, 0.0), at(k + 1, 1.0)]);
        b.element(TRIANGLE, 1, &[at(k, 0.0), at(k + 1, 1.0), at(k, 1.0)]);
    }
    b.done()
}

#[test]
fn a_seam_down_one_surface_goes_into_it_with_its_corners() {
    // The tube folds 30° at every edge along it, and so does the seam:
    // no crease, so the seam goes, and its two corners, left with nothing
    // through them but a closed rim each, go onto the rims.
    let g = tube();
    let mut c = Composites::new();
    let found = c.merge_smooth(&g, 10.0);
    assert_eq!(found.seams, vec![(10, 1)], "{found:?}");
    assert!(found.merges.is_empty());
    assert_eq!(c.stand_in(CURVE, 10), Some((SURFACE, 1)));
    assert_eq!(c.stand_in(CORNER, 3), Some((CURVE, 5)));
    assert_eq!(c.stand_in(CORNER, 4), Some((CURVE, 6)));
    let model = Model::new(&g, &c);
    assert_eq!(
        model.nearest([1.0, 0.0, 0.0], [0.05; 3]),
        Some((CURVE, 5)),
        "near the old corner, on the rim"
    );
    assert_eq!(
        model.nearest([1.0, 0.0, 0.5], [0.05; 3]),
        Some((SURFACE, 1)),
        "near the old seam, on the tube"
    );

    // Without a merge, a closed rim and its corner are as the file has
    // them.
    let plain = Composites::new();
    assert_eq!(
        Model::new(&g, &plain).nearest([1.0, 0.0, 0.0], [0.05; 3]),
        Some((CORNER, 3))
    );
}
