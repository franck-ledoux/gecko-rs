//! The exact geometry, on the parts a CAD model is made of: a plate with a
//! hole (a trimmed face), a cylinder with its seam (a periodic surface), a
//! sphere (its poles), a cone (its apex), two faces meeting at a sharp edge,
//! and a window cut across a cylinder's seam.
//!
//! Every projection is checked against the closed form, to 1e-10.

use gecko_geom::cad::{CadBuilder, CadError, CadGeometry, Trim, Use};
use gecko_geom::nurbs::shapes::{arc, bilinear, circle, cone, cylinder, line, sphere};
use gecko_geom::{CORNER, CURVE, SURFACE};

const EXACT: f64 = 1e-10;

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[track_caller]
fn at(got: Option<[f64; 3]>, want: [f64; 3]) {
    let got = got.expect("projected");
    assert!(apart(got, want) < EXACT, "{got:?} should be {want:?}");
}

/// Points spread through a box, the same ones every run.
fn scattered(n: usize, lo: [f64; 3], hi: [f64; 3]) -> Vec<[f64; 3]> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| [0, 1, 2].map(|k| lo[k] + (hi[k] - lo[k]) * next()))
        .collect()
}

// --- a plate with a hole ------------------------------------------------------

/// A square plate, 4 across, with a hole of radius 1 in the middle.
fn plate() -> CadGeometry {
    let c = [
        [-2.0, -2.0, 0.0],
        [2.0, -2.0, 0.0],
        [2.0, 2.0, 0.0],
        [-2.0, 2.0, 0.0],
    ];
    let mut b = CadBuilder::new();
    for (i, p) in c.iter().enumerate() {
        b.vertex(i as i32 + 1, *p);
    }
    b.vertex(5, [1.0, 0.0, 0.0]);
    for i in 0..4 {
        b.edge(
            i as i32 + 1,
            line(c[i], c[(i + 1) % 4]),
            [i as i32 + 1, (i as i32 + 1) % 4 + 1],
        );
    }
    let hole = circle([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0).unwrap();
    b.edge(5, hole, [5, 5]);
    b.face(
        1,
        bilinear(c),
        &[1, 2, 3, 4, 5],
        Trim::Loops(vec![
            vec![Use::along(1), Use::along(2), Use::along(3), Use::along(4)],
            vec![Use::along(5)],
        ]),
    );
    b.build().unwrap()
}

#[test]
fn a_point_over_the_plate_lands_on_it_and_one_over_the_hole_on_its_rim() {
    let g = plate();
    at(g.project(SURFACE, 1, [1.5, 1.5, 0.7]), [1.5, 1.5, 0.0]);
    at(g.project(SURFACE, 1, [-0.9, 1.7, -2.0]), [-0.9, 1.7, 0.0]);
    // Over the hole: the nearest point of the plate is on the rim.
    let p = [0.3, 0.2, 0.5];
    let r = (0.3f64 * 0.3 + 0.2 * 0.2).sqrt();
    at(g.project(SURFACE, 1, p), [0.3 / r, 0.2 / r, 0.0]);
    // Off the side: the edge of the plate.
    at(g.project(SURFACE, 1, [3.0, 0.5, 1.0]), [2.0, 0.5, 0.0]);
    assert!(!g.parameters(1, [0.3, 0.2, 0.0]).unwrap().1);
    assert!(g.parameters(1, [1.5, 0.2, 0.0]).unwrap().1);
}

#[test]
fn the_incidence_is_the_topology() {
    let g = plate();
    assert_eq!(
        g.containing(CORNER, 5),
        &[(CORNER, 5), (CURVE, 5), (SURFACE, 1)]
    );
    assert_eq!(
        g.containing(CORNER, 1),
        &[(CORNER, 1), (CURVE, 1), (CURVE, 4), (SURFACE, 1)]
    );
    assert_eq!(g.containing(CURVE, 5), &[(CURVE, 5), (SURFACE, 1)]);
    assert_eq!(g.containing(SURFACE, 1), &[(SURFACE, 1)]);
    assert!(g.containing(CURVE, 9).is_empty());
    assert_eq!(g.project(CURVE, 9, [0.0; 3]), None);
}

#[test]
fn a_point_is_classified_on_the_lowest_entity_near_it() {
    let g = plate();
    let tolerance = [0.05, 0.05, 0.05];
    assert_eq!(g.nearest([1.01, 0.0, 0.01], tolerance), Some((CORNER, 5)));
    assert_eq!(g.nearest([0.0, 1.02, 0.0], tolerance), Some((CURVE, 5)));
    assert_eq!(g.nearest([1.5, 1.0, 0.03], tolerance), Some((SURFACE, 1)));
    assert_eq!(g.nearest([1.5, 1.0, 0.3], tolerance), None);
    // Over the hole, away from the rim, it is near nothing.
    assert_eq!(g.nearest([0.0, 0.0, 0.01], tolerance), None);
}

// --- a cylinder, and its seam ---------------------------------------------------

/// Radius 1, from z = 0 to z = 2, whole: two circles and the seam.
fn tube() -> CadGeometry {
    let s = cylinder([0.0; 3], [0.0, 0.0, 1.0], 1.0, 2.0).unwrap();
    let [(u0, _), (v0, _)] = s.domain();
    let foot = s.eval(u0, v0);
    let head = [foot[0], foot[1], 2.0];
    let x = [foot[0], foot[1], 0.0];
    let y = [-foot[1], foot[0], 0.0];
    let mut b = CadBuilder::new();
    b.vertex(1, foot).vertex(2, head);
    b.edge(1, circle([0.0; 3], x, y, 1.0).unwrap(), [1, 1]);
    b.edge(2, circle([0.0, 0.0, 2.0], x, y, 1.0).unwrap(), [2, 2]);
    b.edge(3, line(foot, head), [1, 2]);
    b.face(1, s, &[1, 2, 3], Trim::Natural);
    b.build().unwrap()
}

#[test]
fn every_point_lands_on_the_cylinder_seam_or_not() {
    let g = tube();
    let face = g.face(1).unwrap();
    assert_eq!(face.periods()[0], None);
    assert!(
        face.periods()[1].is_some(),
        "round the axis, the cylinder closes up"
    );
    let mut points = scattered(200, [-3.0, -3.0, 0.0], [3.0, 3.0, 2.0]);
    // And some right by the seam, on either side of it.
    let seam = g.vertex(1).unwrap().point;
    for k in -3..=3 {
        let a = k as f64 * 1e-4 + seam[1].atan2(seam[0]);
        points.push([1.7 * a.cos(), 1.7 * a.sin(), 0.9]);
    }
    for p in points {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        if r < 0.05 {
            continue; // on the axis, every point of the circle is nearest
        }
        at(g.project(SURFACE, 1, p), [p[0] / r, p[1] / r, p[2]]);
    }
    at(
        g.project(CURVE, 3, [2.0 * seam[0], 2.0 * seam[1], 1.3]),
        [seam[0], seam[1], 1.3],
    );
}

// --- a sphere, and its poles -------------------------------------------------

#[test]
fn a_point_by_a_pole_of_the_sphere_lands_on_it() {
    let s = sphere([0.0; 3], [0.0, 0.0, 1.0], 1.5).unwrap();
    let [(u0, u1), (v0, _)] = s.domain();
    let (south, north) = (s.eval(u0, v0), s.eval(u1, v0));
    assert!(apart(south, [0.0, 0.0, -1.5]) < EXACT && apart(north, [0.0, 0.0, 1.5]) < EXACT);
    let meridian = s.iso_u(v0);
    let mut b = CadBuilder::new();
    b.vertex(1, south).vertex(2, north);
    b.edge(1, meridian, [1, 2]);
    b.face(1, s, &[1], Trim::Natural);
    let g = b.build().unwrap();
    assert_eq!(
        g.face(1).unwrap().periods()[0],
        None,
        "a pole is not a seam"
    );
    let mut points = scattered(100, [-3.0; 3], [3.0; 3]);
    points.extend([[1e-3, 2e-3, 3.0], [-1e-4, 0.0, -2.0], [1e-6, 1e-6, 1.6]]);
    for p in points {
        let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if r < 0.05 {
            continue;
        }
        at(g.project(SURFACE, 1, p), p.map(|x| 1.5 * x / r));
    }
    assert_eq!(
        g.containing(CORNER, 2),
        &[(CORNER, 2), (CURVE, 1), (SURFACE, 1)]
    );
}

// --- a cone, and its apex --------------------------------------------------------

#[test]
fn a_point_lands_on_the_cone_or_on_its_apex() {
    // Radius 1 at z = 0 to a point at z = 2.
    let s = cone([0.0; 3], [0.0, 0.0, 1.0], 1.0, 0.0, 2.0).unwrap();
    let [(u0, u1), (v0, _)] = s.domain();
    let (foot, apex) = (s.eval(u0, v0), s.eval(u1, v0));
    let mut b = CadBuilder::new();
    b.vertex(1, foot).vertex(2, apex);
    let base = circle(
        [0.0; 3],
        [foot[0], foot[1], 0.0],
        [-foot[1], foot[0], 0.0],
        1.0,
    )
    .unwrap();
    b.edge(1, base, [1, 1]).edge(2, line(foot, apex), [1, 2]);
    b.face(1, s, &[1, 2], Trim::Natural);
    let g = b.build().unwrap();
    for p in scattered(150, [-2.0, -2.0, -0.5], [2.0, 2.0, 3.0]) {
        // In the half-plane through the axis and the point: the nearest
        // point of the segment from (1, 0) to (0, 2).
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        if r < 0.05 {
            continue;
        }
        let (d, len2) = ([-1.0, 2.0], 5.0);
        let t = (((r - 1.0) * d[0] + p[2] * d[1]) / len2).clamp(0.0, 1.0);
        let (rho, z) = (1.0 + t * d[0], t * d[1]);
        at(
            g.project(SURFACE, 1, p),
            [p[0] / r * rho, p[1] / r * rho, z],
        );
    }
    at(g.project(SURFACE, 1, [0.01, 0.0, 3.0]), [0.0, 0.0, 2.0]);
}

// --- two faces at a sharp edge ---------------------------------------------------

/// A floor, z = 0, and a wall, x = 0, both 2 by 2, meeting along the y axis.
fn corner() -> CadGeometry {
    let p = |x: f64, y: f64, z: f64| [x, y, z];
    let (o, y2) = (p(0.0, 0.0, 0.0), p(0.0, 2.0, 0.0));
    let (fx, fxy) = (p(2.0, 0.0, 0.0), p(2.0, 2.0, 0.0));
    let (wz, wyz) = (p(0.0, 0.0, 2.0), p(0.0, 2.0, 2.0));
    let mut b = CadBuilder::new();
    for (tag, q) in [(1, o), (2, y2), (3, fx), (4, fxy), (5, wz), (6, wyz)] {
        b.vertex(tag, q);
    }
    for (tag, a, b2, ends) in [
        (1, o, y2, [1, 2]),
        (2, o, fx, [1, 3]),
        (3, fx, fxy, [3, 4]),
        (4, y2, fxy, [2, 4]),
        (5, o, wz, [1, 5]),
        (6, wz, wyz, [5, 6]),
        (7, y2, wyz, [2, 6]),
    ] {
        b.edge(tag, line(a, b2), ends);
    }
    b.face(1, bilinear([o, fx, fxy, y2]), &[1, 2, 3, 4], Trim::Natural);
    b.face(2, bilinear([o, y2, wyz, wz]), &[1, 5, 6, 7], Trim::Natural);
    b.build().unwrap()
}

#[test]
fn the_sharp_edge_is_on_both_faces_and_points_find_the_right_one() {
    let g = corner();
    assert_eq!(
        g.containing(CURVE, 1),
        &[(CURVE, 1), (SURFACE, 1), (SURFACE, 2)]
    );
    let p = [0.5, 1.0, 0.3];
    at(g.project(SURFACE, 1, p), [0.5, 1.0, 0.0]);
    at(g.project(SURFACE, 2, p), [0.0, 1.0, 0.3]);
    // Outside both: each face's nearest point is the edge they share.
    let q = [-0.4, 1.0, -0.3];
    at(g.project(SURFACE, 1, q), [0.0, 1.0, 0.0]);
    at(g.project(SURFACE, 2, q), [0.0, 1.0, 0.0]);
    let tolerance = [0.01, 0.01, 0.01];
    assert_eq!(g.nearest([0.004, 1.0, 0.004], tolerance), Some((CURVE, 1)));
    assert_eq!(
        g.nearest([0.003, 0.002, 0.001], tolerance),
        Some((CORNER, 1))
    );
    assert_eq!(g.nearest([0.5, 1.0, 0.005], tolerance), Some((SURFACE, 1)));
    assert!(
        g.span() > 3.4 && g.span() < 3.5,
        "the diagonal of a 2 cube: {}",
        g.span()
    );
}

// --- a window across a cylinder's seam --------------------------------------------

/// A cylinder trimmed to the window from 45° before its seam to 45° after,
/// and from z = 0.5 to z = 1.5: a loop the seam cuts in two, unless it is
/// kept in one piece.
fn window() -> (CadGeometry, [f64; 3], [f64; 3]) {
    let s = cylinder([0.0; 3], [0.0, 0.0, 1.0], 1.0, 2.0).unwrap();
    let [(u0, _), (v0, _)] = s.domain();
    let f = s.eval(u0, v0);
    let (x, y) = ([f[0], f[1], 0.0], [-f[1], f[0], 0.0]);
    let h = 0.5f64.sqrt();
    // The two sides of the window, at -45° and +45° from the seam.
    let side = |sign: f64| [0, 1, 2].map(|k| h * x[k] + sign * h * y[k]);
    let (before, after) = (side(-1.0), side(1.0));
    let lift = |p: [f64; 3], z: f64| [p[0], p[1], z];
    let mut b = CadBuilder::new();
    b.vertex(1, lift(before, 0.5))
        .vertex(2, lift(after, 0.5))
        .vertex(3, lift(after, 1.5))
        .vertex(4, lift(before, 1.5));
    let span = |z: f64| arc([0.0, 0.0, z], before, after, 1.0, 1).unwrap();
    b.edge(1, span(0.5), [1, 2])
        .edge(2, line(lift(after, 0.5), lift(after, 1.5)), [2, 3])
        .edge(3, span(1.5), [4, 3])
        .edge(4, line(lift(before, 0.5), lift(before, 1.5)), [1, 4]);
    b.face(
        1,
        s,
        &[1, 2, 3, 4],
        Trim::Loops(vec![vec![
            Use::along(1),
            Use::along(2),
            Use::against(3),
            Use::against(4),
        ]]),
    );
    (b.build().unwrap(), x, y)
}

#[test]
fn a_window_across_the_seam_is_in_one_piece() {
    let (g, x, y) = window();
    let around = |degrees: f64, r: f64, z: f64| {
        let a = degrees.to_radians();
        [0, 1]
            .map(|k| r * (a.cos() * x[k] + a.sin() * y[k]))
            .into_iter()
            .chain([z])
            .collect::<Vec<_>>()
    };
    let v = |p: Vec<f64>| [p[0], p[1], p[2]];
    // Inside, on the seam and either side of it: straight onto the cylinder.
    for degrees in [0.0, 30.0, -30.0, 44.0, -44.0] {
        let p = v(around(degrees, 1.8, 1.1));
        assert!(g.parameters(1, p).unwrap().1, "{degrees}° is in the window");
        at(g.project(SURFACE, 1, p), v(around(degrees, 1.0, 1.1)));
    }
    // Outside: onto the nearest side of the window.
    for degrees in [60.0, -60.0, 180.0] {
        let p = v(around(degrees, 1.8, 1.1));
        assert!(
            !g.parameters(1, p).unwrap().1,
            "{degrees}° is not in the window"
        );
        let q = g.project(SURFACE, 1, p).unwrap();
        let sides = [v(around(45.0, 1.0, 1.1)), v(around(-45.0, 1.0, 1.1))];
        assert!(
            sides.iter().any(|s| apart(q, *s) < EXACT),
            "{degrees}°: {q:?} should be on a side of the window"
        );
    }
    // Above it, on the seam: onto its top.
    at(
        g.project(SURFACE, 1, v(around(0.0, 1.0, 1.9))),
        v(around(0.0, 1.0, 1.5)),
    );
}

// --- what is refused ------------------------------------------------------------

#[test]
fn a_loop_round_a_cylinder_does_not_trim_it() {
    let s = cylinder([0.0; 3], [0.0, 0.0, 1.0], 1.0, 2.0).unwrap();
    let [(u0, _), (v0, _)] = s.domain();
    let f = s.eval(u0, v0);
    let mut b = CadBuilder::new();
    b.vertex(1, f);
    b.edge(
        1,
        circle([0.0; 3], [f[0], f[1], 0.0], [-f[1], f[0], 0.0], 1.0).unwrap(),
        [1, 1],
    );
    b.face(1, s, &[1], Trim::Loops(vec![vec![Use::along(1)]]));
    assert_eq!(
        b.build().unwrap_err(),
        CadError::Unclosed { face: 1, ring: 0 }
    );
}

#[test]
fn a_geometry_that_does_not_hang_together_is_refused() {
    let a = [0.0; 3];
    let b2 = [1.0, 0.0, 0.0];
    let mut b = CadBuilder::new();
    b.vertex(1, a).vertex(2, [1.0, 0.1, 0.0]);
    b.edge(1, line(a, b2), [1, 2]);
    assert!(matches!(
        b.build().unwrap_err(),
        CadError::EndApart {
            edge: 1,
            end: "end",
            vertex: 2,
            ..
        }
    ));

    let mut b = CadBuilder::new();
    b.vertex(1, a).vertex(1, b2);
    assert_eq!(
        b.build().unwrap_err(),
        CadError::Repeated {
            kind: "vertex",
            tag: 1
        }
    );

    let mut b = CadBuilder::new();
    b.vertex(1, a);
    b.edge(1, line(a, a), [1, 3]);
    assert_eq!(
        b.build().unwrap_err(),
        CadError::NoVertex { edge: 1, vertex: 3 }
    );

    // The plate's outer loop, two of its edges swapped.
    let c = [
        [-2.0, -2.0, 0.0],
        [2.0, -2.0, 0.0],
        [2.0, 2.0, 0.0],
        [-2.0, 2.0, 0.0],
    ];
    let mut b = CadBuilder::new();
    for (i, p) in c.iter().enumerate() {
        b.vertex(i as i32 + 1, *p);
    }
    for i in 0..4 {
        b.edge(
            i as i32 + 1,
            line(c[i], c[(i + 1) % 4]),
            [i as i32 + 1, (i as i32 + 1) % 4 + 1],
        );
    }
    let loop_ = vec![Use::along(1), Use::along(3), Use::along(2), Use::along(4)];
    b.face(1, bilinear(c), &[1, 2, 3, 4], Trim::Loops(vec![loop_]));
    assert_eq!(
        b.build().unwrap_err(),
        CadError::OpenLoop {
            face: 1,
            ring: 0,
            edge: 3
        }
    );

    let mut b = CadBuilder::new();
    b.face(1, bilinear(c), &[7], Trim::Natural);
    assert_eq!(
        b.build().unwrap_err(),
        CadError::NoEdge { face: 1, edge: 7 }
    );
}

#[test]
fn a_model_says_how_far_apart_its_same_point_may_be() {
    // An edge ending a micron from its vertex: refused at a billionth of
    // the span, taken at the model's own tolerance.
    let (a, b2) = ([0.0; 3], [1.0, 0.0, 0.0]);
    let mut b = CadBuilder::new();
    b.vertex(1, a).vertex(2, [1.0, 1e-6, 0.0]);
    b.edge(1, line(a, b2), [1, 2]);
    assert!(matches!(
        b.build().unwrap_err(),
        CadError::EndApart { edge: 1, .. }
    ));
    b.tolerance(1e-5);
    assert!(b.build().is_ok());
}
