//! `.gcad` files, as `scripts/step_to_gcad.py` writes them: read back, held
//! to closed forms, and their tessellations held to the exact shapes.
//!
//! The files are in `tests/data`, made by `scripts/make_gcad_fixtures.py`
//! with OpenCascade, so that these tests never need it.

use gecko_geom::cad::CadGeometry;
use gecko_geom::gcad::{read, GcadError, GcadInfo};
use gecko_geom::{CORNER, CURVE, SURFACE};

const EXACT: f64 = 1e-10;

fn fixture(name: &str) -> (CadGeometry, GcadInfo) {
    let path = format!("{}/tests/data/{name}.gcad", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    read(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

const ALL: [&str; 6] = ["plate", "cylinder", "sphere", "cone", "B10", "B47"];

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[track_caller]
fn at(got: Option<[f64; 3]>, want: [f64; 3]) {
    let got = got.expect("projected");
    assert!(apart(got, want) < EXACT, "{got:?} should be {want:?}");
}

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

#[test]
fn every_fixture_reads_with_the_entities_opencascade_found() {
    let counts = [
        ("plate", 10, 15, 7),
        ("cylinder", 2, 3, 3),
        ("sphere", 2, 3, 1),
        ("cone", 2, 3, 2),
        ("B10", 10, 15, 7),
        ("B47", 24, 36, 15),
    ];
    for (name, v, e, f) in counts {
        let (g, info) = fixture(name);
        assert_eq!(
            (g.vertices().len(), g.edges().len(), g.faces().len()),
            (v, e, f),
            "{name}"
        );
        assert_eq!(info.units, "mm");
        assert!(info.deflection > 0.0 && info.tolerance > 0.0);
        for face in g.faces() {
            assert!(
                face.triangulation
                    .as_ref()
                    .is_some_and(|t| !t.triangles.is_empty()),
                "{name}: face {}",
                face.tag
            );
        }
    }
}

// --- held to closed forms --------------------------------------------------------

#[test]
fn a_point_lands_exactly_on_the_sphere_poles_and_all() {
    let (g, _) = fixture("sphere");
    // A seam, and an edge at each pole that is a point.
    assert_eq!(g.edges().iter().filter(|e| e.degenerate).count(), 2);
    let mut points = scattered(60, [-3.0; 3], [3.0; 3]);
    points.extend([[1e-3, 2e-3, 3.0], [-1e-4, 0.0, -2.0]]);
    for p in points {
        let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if r < 0.05 {
            continue;
        }
        assert!(
            g.parameters(1, p).unwrap().1,
            "{p:?}: all of a sphere is its one face"
        );
        at(g.project(SURFACE, 1, p), p.map(|x| 1.5 * x / r));
    }
}

#[test]
fn a_point_lands_exactly_on_the_cylinder_or_its_discs() {
    let (g, _) = fixture("cylinder");
    let side = g
        .faces()
        .iter()
        .find(|f| f.periods().iter().any(|p| p.is_some()))
        .expect("the side closes round its axis")
        .tag;
    for p in scattered(60, [-3.0, -3.0, 0.0], [3.0, 3.0, 2.0]) {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        if r < 0.05 {
            continue;
        }
        at(g.project(SURFACE, side, p), [p[0] / r, p[1] / r, p[2]]);
    }
    // The discs: the nearest point of each, inside it or on its rim.
    let disc = |z: f64| {
        g.faces()
            .iter()
            .find(|f| {
                f.tag != side
                    && g.project(SURFACE, f.tag, [0.0, 0.0, z])
                        .is_some_and(|q| (q[2] - z).abs() < EXACT)
            })
            .unwrap()
            .tag
    };
    let (bottom, top) = (disc(0.0), disc(2.0));
    at(g.project(SURFACE, top, [0.3, -0.2, 5.0]), [0.3, -0.2, 2.0]);
    at(
        g.project(SURFACE, bottom, [3.0, 4.0, -1.0]),
        [0.6, 0.8, 0.0],
    );
}

#[test]
fn a_point_over_the_hole_in_the_plate_lands_on_its_rim() {
    let (g, _) = fixture("plate");
    // The top: the face through (1.5, 1.5, 1).
    let top = g
        .faces()
        .iter()
        .map(|f| f.tag)
        .find(|&t| {
            g.distance(SURFACE, t, [1.5, 1.5, 1.0]).unwrap() < EXACT
                && g.distance(SURFACE, t, [1.5, 1.5, 0.0]).unwrap() > 0.5
        })
        .expect("a top face");
    at(g.project(SURFACE, top, [1.5, -1.2, 3.0]), [1.5, -1.2, 1.0]);
    let r = (0.3f64 * 0.3 + 0.2 * 0.2).sqrt();
    at(
        g.project(SURFACE, top, [0.3, 0.2, 1.4]),
        [0.3 / r, 0.2 / r, 1.0],
    );
    // Its loops: the square, and the hole.
    let gecko_geom::Trim::Loops(loops) = &g.face(top).unwrap().trim else {
        panic!("the top is trimmed");
    };
    assert_eq!(loops.len(), 2);
}

#[test]
fn a_point_lands_exactly_on_the_cone_or_its_apex() {
    let (g, _) = fixture("cone");
    let side = g
        .faces()
        .iter()
        .find(|f| f.periods().iter().any(|p| p.is_some()))
        .expect("the side closes round its axis")
        .tag;
    for p in scattered(60, [-2.0, -2.0, -0.5], [2.0, 2.0, 3.0]) {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        if r < 0.05 {
            continue;
        }
        let t = (((1.0 - r) + 2.0 * p[2]) / 5.0).clamp(0.0, 1.0);
        at(
            g.project(SURFACE, side, p),
            [p[0] / r * (1.0 - t), p[1] / r * (1.0 - t), 2.0 * t],
        );
    }
}

// --- the tessellation, against the shapes -----------------------------------------

/// Every node of a face's triangles on the face, and the middle of every
/// triangle within the deflection of its surface; every point of an edge's
/// polyline on the edge, and the middle of every segment within the
/// deflection of its curve.
#[test]
fn the_tessellation_stays_within_its_deflection_of_the_exact_shapes() {
    for name in ALL {
        let (g, info) = fixture(name);
        let on = 10.0 * info.tolerance;
        let (mut worst_node, mut worst_middle) = (0.0f64, 0.0f64);
        for face in g.faces() {
            let t = face.triangulation.as_ref().unwrap();
            // Every third node, every third triangle: enough to find a
            // stray, and a test that runs in seconds.
            for p in t.nodes.iter().step_by(3) {
                worst_node = worst_node.max(face.surface.project(*p).2);
            }
            for tri in t.triangles.iter().step_by(3) {
                let [a, b, c] = tri.map(|i| t.nodes[i as usize]);
                let middle = [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0);
                worst_middle = worst_middle.max(face.surface.project(middle).2);
            }
        }
        assert!(
            worst_node <= on,
            "{name}: a node {worst_node:e} off its face"
        );
        assert!(
            worst_middle <= info.deflection,
            "{name}: a triangle {worst_middle:e} off its face, past the deflection {:e}",
            info.deflection
        );
        for e in g.edges().iter().filter(|e| !e.degenerate) {
            assert!(e.polyline.len() >= 2, "{name}: edge {}", e.tag);
            for w in e.polyline.windows(2) {
                assert!(e.curve.project(w[0]).2 <= on, "{name}: edge {}", e.tag);
                let middle = [0, 1, 2].map(|k| (w[0][k] + w[1][k]) / 2.0);
                assert!(
                    e.curve.project(middle).2 <= info.deflection,
                    "{name}: edge {}",
                    e.tag
                );
            }
        }
    }
}

/// A node of a face's triangles is on the face, so projecting it onto the
/// face gives it back: the trim, drawn from the p-curves, has it inside.
#[test]
fn a_node_of_a_face_projects_onto_the_face_to_itself() {
    for name in ["plate", "B10", "B47"] {
        let (g, info) = fixture(name);
        for face in g.faces() {
            let t = face.triangulation.as_ref().unwrap();
            for p in t.nodes.iter().step_by(4) {
                let q = g.project(SURFACE, face.tag, *p).unwrap();
                assert!(
                    apart(*p, q) <= 10.0 * info.tolerance,
                    "{name}: face {}: {p:?} went to {q:?}",
                    face.tag
                );
            }
        }
    }
}

// --- the topology ------------------------------------------------------------------

#[test]
fn every_edge_of_a_closed_part_bounds_two_faces_or_is_a_seam() {
    for name in ["plate", "B10", "B47"] {
        let (g, _) = fixture(name);
        for e in g.edges() {
            let faces: Vec<_> = g
                .containing(CURVE, e.tag)
                .iter()
                .filter(|(d, _)| *d == SURFACE)
                .collect();
            let seam = g.faces().iter().any(|f| {
                let gecko_geom::Trim::Loops(loops) = &f.trim else {
                    return false;
                };
                loops.iter().flatten().filter(|u| u.edge == e.tag).count() == 2
            });
            assert!(
                faces.len() == 2 || (seam && faces.len() == 1),
                "{name}: edge {} on {faces:?}",
                e.tag
            );
        }
        for v in g.vertices() {
            assert!(
                g.containing(CORNER, v.tag).iter().any(|(d, _)| *d == CURVE),
                "{name}: vertex {}",
                v.tag
            );
        }
    }
    // B10 has no hole through it: V − E + F = 2.
    let (g, _) = fixture("B10");
    assert_eq!(
        g.vertices().len() as i64 - g.edges().len() as i64 + g.faces().len() as i64,
        2
    );
}

#[test]
fn a_file_of_another_format_or_version_is_refused_by_name() {
    let text = std::fs::read_to_string(format!(
        "{}/tests/data/cylinder.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let other = text.replacen("\"version\": 1", "\"version\": 2", 1);
    assert!(matches!(read(&other), Err(GcadError::Version(2))));
    let other = text.replacen("\"format\": \"gcad\"", "\"format\": \"msh\"", 1);
    assert!(matches!(read(&other), Err(GcadError::Format(f)) if f == "msh"));
    assert!(matches!(read("{"), Err(GcadError::Json(_))));
}
