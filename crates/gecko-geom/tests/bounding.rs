//! Putting a box round a geometry.

use gecko_geom::msh::{Element, Msh, TRIANGLE};
use gecko_geom::{BoundsError, Geometry};

type Point = [f64; 3];
type Triangle = [Point; 3];

/// A geometry made of whole surfaces, each a tag and its triangles.
///
/// Built as a `Msh` rather than as a file: what these tests are about is the
/// arithmetic, and a page of `$Elements` lines between the reader and the
/// question would only be in the way.
fn geometry(faces: &[(i32, Vec<Triangle>)]) -> Geometry {
    let mut msh = Msh::default();
    for (tag, triangles) in faces {
        for t in triangles {
            let base = msh.nodes.len() as u32;
            msh.nodes.extend_from_slice(t);
            msh.elements.push(Element {
                kind: TRIANGLE,
                physical: 0,
                entity: *tag,
                nodes: [base, base + 1, base + 2],
            });
        }
    }
    Geometry::of(&msh).expect("a geometry")
}

/// The six faces of a box `size` across at `origin`, each its own surface,
/// with every point put through `place` on the way out.
fn slab(
    origin: Point,
    size: Point,
    first_tag: i32,
    place: impl Fn(Point) -> Point,
) -> Vec<(i32, Vec<Triangle>)> {
    let at = |i: usize, j: usize, k: usize| {
        place([
            origin[0] + size[0] * i as f64,
            origin[1] + size[1] * j as f64,
            origin[2] + size[2] * k as f64,
        ])
    };
    let quad = |a: Point, b: Point, c: Point, d: Point| vec![[a, b, c], [a, c, d]];
    vec![
        (
            first_tag,
            quad(at(0, 0, 0), at(0, 1, 0), at(1, 1, 0), at(1, 0, 0)),
        ),
        (
            first_tag + 1,
            quad(at(0, 0, 1), at(1, 0, 1), at(1, 1, 1), at(0, 1, 1)),
        ),
        (
            first_tag + 2,
            quad(at(0, 0, 0), at(1, 0, 0), at(1, 0, 1), at(0, 0, 1)),
        ),
        (
            first_tag + 3,
            quad(at(0, 1, 0), at(0, 1, 1), at(1, 1, 1), at(1, 1, 0)),
        ),
        (
            first_tag + 4,
            quad(at(0, 0, 0), at(0, 0, 1), at(0, 1, 1), at(0, 1, 0)),
        ),
        (
            first_tag + 5,
            quad(at(1, 0, 0), at(1, 1, 0), at(1, 1, 1), at(1, 0, 1)),
        ),
    ]
}

fn turned(about_z: f64) -> impl Fn(Point) -> Point {
    let (s, c) = about_z.sin_cos();
    move |p| [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
}

fn keep(p: Point) -> Point {
    p
}

fn extent(corners: &[[f64; 3]; 8], axis: usize) -> (f64, f64) {
    let all: Vec<f64> = corners.iter().map(|c| c[axis]).collect();
    (
        all.iter().cloned().fold(f64::INFINITY, f64::min),
        all.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    )
}

#[test]
fn a_box_round_everything_is_the_extent_of_everything() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [2.0, 1.0, 3.0], 1, keep));
    let corners = g.bounding_box(&[], 0.0).expect("a box");
    for (axis, want) in [(0, 2.0), (1, 1.0), (2, 3.0)] {
        let (lo, hi) = extent(&corners, axis);
        assert!((lo - 0.0).abs() < 1e-9, "axis {axis} starts at {lo}");
        assert!((hi - want).abs() < 1e-9, "axis {axis} reaches {hi}");
    }
}

#[test]
fn the_margin_grows_each_half_side_by_its_own_share() {
    // The C++'s own rule, and its own default: `create_bounding_box(0.1)`.
    let g = geometry(&slab([0.0, 0.0, 0.0], [2.0, 1.0, 3.0], 1, keep));
    let corners = g.bounding_box(&[], 0.1).expect("a box");
    for (axis, size) in [(0, 2.0), (1, 1.0), (2, 3.0)] {
        let (lo, hi) = extent(&corners, axis);
        assert!(
            (lo - (-0.05 * size)).abs() < 1e-9,
            "axis {axis} starts at {lo}"
        );
        assert!((hi - size * 1.05).abs() < 1e-9, "axis {axis} reaches {hi}");
    }
    // And a margin of nothing rests exactly on it.
    let tight = g.bounding_box(&[], 0.0).expect("a box");
    assert!((extent(&tight, 0).1 - 2.0).abs() < 1e-9);
}

#[test]
fn a_selection_bounds_what_is_selected_and_nothing_else() {
    let mut faces = slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep);
    faces.extend(slab([10.0, 0.0, 0.0], [1.0, 1.0, 1.0], 7, keep));
    let g = geometry(&faces);

    let both = g.bounding_box(&[], 0.0).expect("both");
    assert!((extent(&both, 0).1 - 11.0).abs() < 1e-9);

    let far = g.bounding_box(&[7, 8, 9, 10, 11, 12], 0.0).expect("one");
    let (lo, hi) = extent(&far, 0);
    assert!(
        (lo - 10.0).abs() < 1e-9 && (hi - 11.0).abs() < 1e-9,
        "{lo}..{hi}"
    );
}

#[test]
fn the_corners_come_out_right_handed() {
    // Or the block built from them is inside out, and every normal in the
    // model points the wrong way.
    for angle in [0.0, 0.3, 1.1, 2.9] {
        let g = geometry(&slab([0.0, 0.0, 0.0], [3.0, 2.0, 1.0], 1, turned(angle)));
        let c = g.bounding_box(&[], 0.0).expect("a box");
        let edge = |to: usize| [0, 1, 2].map(|k| c[to][k] - c[0][k]);
        let (u, v, w) = (edge(1), edge(3), edge(4));
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let volume: f64 = (0..3).map(|k| cross[k] * w[k]).sum();
        assert!(volume > 0.0, "at {angle}: {volume}");
    }
}

#[test]
fn something_flat_has_no_box_round_it() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep));
    let refused = g.bounding_box(&[1], 0.1).expect_err("flat");
    assert!(matches!(refused, BoundsError::Flat), "{refused:?}");
}

#[test]
fn a_tag_no_surface_answers_to_says_so() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep));
    let refused = g.bounding_box(&[99], 0.0).expect_err("no such");
    assert!(
        matches!(refused, BoundsError::NoSuchSurface(99)),
        "{refused:?}"
    );
}

#[test]
fn a_margin_that_is_not_a_margin_is_refused() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep));
    for margin in [-1.0, -2.0, f64::NAN, f64::INFINITY] {
        let refused = g.bounding_box(&[], margin).expect_err("bad");
        assert!(matches!(refused, BoundsError::BadMargin(_)), "{refused:?}");
    }
}
