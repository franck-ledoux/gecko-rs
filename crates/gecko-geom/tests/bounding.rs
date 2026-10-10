//! Putting a box round a geometry.

use gecko_geom::msh::{Element, Msh, TRIANGLE};
use gecko_geom::{BoundsError, Frame, Geometry};

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

/// The three side lengths of a box, shortest first.
fn sides(corners: &[[f64; 3]; 8]) -> [f64; 3] {
    let gap = |a: usize, b: usize| {
        (0..3)
            .map(|k| (corners[a][k] - corners[b][k]).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let mut s = [gap(0, 1), gap(0, 3), gap(0, 4)];
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s
}

#[test]
fn a_box_round_everything_is_the_extent_of_everything() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [2.0, 1.0, 3.0], 1, keep));
    let corners = g.bounding_box(&[], Frame::Axes, 0.0).expect("a box");
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
    let corners = g.bounding_box(&[], Frame::Axes, 0.1).expect("a box");
    for (axis, size) in [(0, 2.0), (1, 1.0), (2, 3.0)] {
        let (lo, hi) = extent(&corners, axis);
        assert!(
            (lo - (-0.05 * size)).abs() < 1e-9,
            "axis {axis} starts at {lo}"
        );
        assert!((hi - size * 1.05).abs() < 1e-9, "axis {axis} reaches {hi}");
    }
    // And a margin of nothing rests exactly on it.
    let tight = g.bounding_box(&[], Frame::Axes, 0.0).expect("a box");
    assert!((extent(&tight, 0).1 - 2.0).abs() < 1e-9);
}

#[test]
fn a_selection_bounds_what_is_selected_and_nothing_else() {
    let mut faces = slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep);
    faces.extend(slab([10.0, 0.0, 0.0], [1.0, 1.0, 1.0], 7, keep));
    let g = geometry(&faces);

    let both = g.bounding_box(&[], Frame::Axes, 0.0).expect("both");
    assert!((extent(&both, 0).1 - 11.0).abs() < 1e-9);

    let far = g
        .bounding_box(&[7, 8, 9, 10, 11, 12], Frame::Axes, 0.0)
        .expect("one");
    let (lo, hi) = extent(&far, 0);
    assert!(
        (lo - 10.0).abs() < 1e-9 && (hi - 11.0).abs() < 1e-9,
        "{lo}..{hi}"
    );
}

#[test]
fn a_tilted_shape_gets_a_tilted_box() {
    // The whole point of the second frame. A 3×2×1 bar at forty degrees:
    // measured against the world it needs a box half full of air, measured
    // against itself it needs one exactly its own size.
    //
    // Three *different* sides on purpose. A square cross-section has two
    // equal eigenvalues and so no preferred way up, and the box round it is
    // free to turn about its own length — see the note in `bounds.rs`.
    let angle = 40.0_f64.to_radians();
    let g = geometry(&slab([0.0, 0.0, 0.0], [3.0, 2.0, 1.0], 1, turned(angle)));

    let own = g.bounding_box(&[], Frame::Inertia, 0.0).expect("a box");
    let [a, b, c] = sides(&own);
    assert!((a - 1.0).abs() < 1e-6, "{a}");
    assert!((b - 2.0).abs() < 1e-6, "{b}");
    assert!((c - 3.0).abs() < 1e-6, "{c}");

    let square = g.bounding_box(&[], Frame::Axes, 0.0).expect("a box");
    let world: f64 = sides(&square).iter().product();
    assert!(world > 6.0 * 1.4, "the world's own box is roomier: {world}");
}

#[test]
fn an_upright_shape_is_left_upright() {
    // And the second frame is not gratuitous: a box already square to the
    // world comes back square to the world.
    let g = geometry(&slab([0.0, 0.0, 0.0], [3.0, 2.0, 1.0], 1, keep));
    let own = g.bounding_box(&[], Frame::Inertia, 0.0).expect("a box");
    let square = g.bounding_box(&[], Frame::Axes, 0.0).expect("a box");
    let mut a = sides(&own);
    let mut b = sides(&square);
    for s in [&mut a, &mut b] {
        s.sort_by(|x, y| x.partial_cmp(y).unwrap());
    }
    for k in 0..3 {
        assert!((a[k] - b[k]).abs() < 1e-6, "{a:?} against {b:?}");
    }
}

#[test]
fn the_axes_come_from_the_shape_and_not_from_the_tessellation() {
    // Area weighting, which is the whole reason for the closed form.
    //
    // **Half** of one long side is cut into a hundred and twenty-eight
    // triangles and the rest of the bar into two apiece. The surface is
    // unchanged — same shape, same area, same points — so the axes must be
    // unchanged. Count triangles instead and the weight piles up on one
    // quarter of the bar, off-centre in two directions at once, and the box
    // comes out leaning.
    let angle = 25.0_f64.to_radians();
    let place = turned(angle);
    let plain = geometry(&slab([0.0, 0.0, 0.0], [4.0, 2.0, 1.0], 1, turned(angle)));

    let mut faces = slab([0.0, 0.0, 0.0], [4.0, 2.0, 1.0], 1, turned(angle));
    let mut side = Vec::new();
    for i in 0..8 {
        for j in 0..8 {
            let at =
                |u: usize, v: usize| place([2.0 * (i + u) as f64 / 8.0, 0.0, (j + v) as f64 / 8.0]);
            side.push([at(0, 0), at(1, 0), at(1, 1)]);
            side.push([at(0, 0), at(1, 1), at(0, 1)]);
        }
    }
    let far = |x: f64, z: f64| place([x, 0.0, z]);
    side.push([far(2.0, 0.0), far(4.0, 0.0), far(4.0, 1.0)]);
    side.push([far(2.0, 0.0), far(4.0, 1.0), far(2.0, 1.0)]);
    faces[2] = (3, side);
    let chopped = geometry(&faces);

    let a = plain.bounding_box(&[], Frame::Inertia, 0.0).expect("a box");
    let b = chopped
        .bounding_box(&[], Frame::Inertia, 0.0)
        .expect("a box");
    for k in 0..8 {
        for axis in 0..3 {
            assert!(
                (a[k][axis] - b[k][axis]).abs() < 1e-6,
                "corner {k} moved: {:?} against {:?}",
                a[k],
                b[k]
            );
        }
    }
}

#[test]
fn the_corners_come_out_right_handed() {
    // Or the block built from them is inside out, and every normal in the
    // model points the wrong way.
    for frame in [Frame::Axes, Frame::Inertia] {
        for angle in [0.0, 0.3, 1.1, 2.9] {
            let g = geometry(&slab([0.0, 0.0, 0.0], [3.0, 2.0, 1.0], 1, turned(angle)));
            let c = g.bounding_box(&[], frame, 0.0).expect("a box");
            let edge = |to: usize| [0, 1, 2].map(|k| c[to][k] - c[0][k]);
            let (u, v, w) = (edge(1), edge(3), edge(4));
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let volume: f64 = (0..3).map(|k| cross[k] * w[k]).sum();
            assert!(volume > 0.0, "{frame:?} at {angle}: {volume}");
        }
    }
}

#[test]
fn something_flat_has_no_box_round_it() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep));
    for frame in [Frame::Axes, Frame::Inertia] {
        let refused = g.bounding_box(&[1], frame, 0.1).expect_err("flat");
        assert!(matches!(refused, BoundsError::Flat), "{refused:?}");
    }
}

#[test]
fn a_tag_no_surface_answers_to_says_so() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep));
    let refused = g
        .bounding_box(&[99], Frame::Axes, 0.0)
        .expect_err("no such");
    assert!(
        matches!(refused, BoundsError::NoSuchSurface(99)),
        "{refused:?}"
    );
}

#[test]
fn a_margin_that_is_not_a_margin_is_refused() {
    let g = geometry(&slab([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 1, keep));
    for margin in [-1.0, -2.0, f64::NAN, f64::INFINITY] {
        let refused = g.bounding_box(&[], Frame::Axes, margin).expect_err("bad");
        assert!(matches!(refused, BoundsError::BadMargin(_)), "{refused:?}");
    }
}
