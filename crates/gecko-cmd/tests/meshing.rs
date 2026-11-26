//! Discretising through the door, and the file that comes out.

use gecko_cmd::{Command, Metric, Session};

const UNIT_CUBE: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0],
];

fn cube() -> Session {
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();
    s
}

fn mesh(s: &Session) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(&s.stats_json()).unwrap()["mesh"].clone()
}

#[test]
fn a_blocking_is_meshed_and_the_counts_come_back() {
    let mut s = cube();
    assert!(s.mesh_vtk().is_none(), "nothing has been meshed yet");

    let out = s.apply(Command::Mesh { segments: 3 }).unwrap();
    assert_eq!(
        out.message,
        "meshed — 64 nodes, 27 hexahedra; min scaled Jacobian 1.00"
    );
    // Not a change to the blocking, so not a step of undo either.
    assert!(!out.view_changed);
    assert_eq!(mesh(&s)["nodes"], 64);
    assert_eq!(mesh(&s)["hexes"], 27);
}

#[test]
fn the_number_of_segments_is_set_on_the_sheet() {
    let mut s = cube();
    let edge = first_edge(&s);
    let out = s.apply(Command::SetSegments { edge, segments: 2 }).unwrap();
    assert!(out.message.contains("2 segments"), "{}", out.message);
    assert!(!out.view_changed, "nothing of the blocking moved");

    s.apply(Command::Mesh { segments: 4 }).unwrap();
    assert_eq!(mesh(&s)["hexes"], 2 * 4 * 4);

    // Out of range is clamped rather than refused: a thousand segments is
    // a typo, and a billion hexahedra is not an error message anyone wants
    // to read afterwards.
    s.apply(Command::SetSegments {
        edge,
        segments: 100_000,
    })
    .unwrap();
    s.apply(Command::Mesh { segments: 1 }).unwrap();
    assert_eq!(mesh(&s)["hexes"], 200);
}

#[test]
fn a_mesh_does_not_outlive_the_blocking_it_was_made_from() {
    // A mesh of a model you have since edited is a lie, and a lie that
    // looks right.
    let mut s = cube();
    s.apply(Command::Mesh { segments: 2 }).unwrap();
    let before = generation(&s);
    assert!(s.mesh_vtk().is_some());

    s.apply(Command::CreateCube {
        origin: [3.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    assert!(s.mesh_vtk().is_none(), "the mesh outlived the blocking");
    assert!(generation(&s) > before, "nobody was told");

    // And an undo does not bring it back either.
    s.apply(Command::Mesh { segments: 2 }).unwrap();
    s.apply(Command::Undo).unwrap();
    assert!(s.mesh_vtk().is_none());
}

#[test]
fn the_file_is_vtk_legacy_a_reader_would_take() {
    let mut s = cube();
    s.apply(Command::SetSegments {
        edge: first_edge(&s),
        segments: 1,
    })
    .unwrap();
    s.apply(Command::Mesh { segments: 1 }).unwrap();
    let file = s.mesh_vtk().expect("a mesh");
    let lines: Vec<&str> = file.lines().collect();

    assert_eq!(lines[0], "# vtk DataFile Version 2.0");
    assert_eq!(lines[2], "ASCII");
    assert_eq!(lines[3], "DATASET UNSTRUCTURED_GRID");
    assert_eq!(lines[4], "POINTS 8 double");
    // One cell, nine numbers: how many corners, and which.
    assert!(file.contains("\nCELLS 1 9\n"), "{file}");
    assert!(file.contains("\nCELL_TYPES 1\n12\n"), "{file}");
    // And which block it came from, for anything that wants to color it.
    assert!(
        file.contains("SCALARS block int 1\nLOOKUP_TABLE default\n0\n"),
        "{file}"
    );

    // The eight corners of the cube, in some order.
    let mut points: Vec<[f64; 3]> = lines[5..13]
        .iter()
        .map(|l| {
            let v: Vec<f64> = l.split(' ').map(|n| n.parse().unwrap()).collect();
            [v[0], v[1], v[2]]
        })
        .collect();
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut want = UNIT_CUBE.to_vec();
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(points, want);
}

#[test]
fn the_mesh_follows_the_geometry_it_is_classified_on() {
    // The whole reason the projection is threaded through the mesher: the
    // points along a classified edge are put on the straight line and then
    // moved onto what the edge follows.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: SPHERE.to_string(),
        name: String::new(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.0,
    })
    .unwrap();
    s.apply(Command::Classify { tolerance: 0.5 }).unwrap();
    s.apply(Command::Mesh { segments: 4 }).unwrap();

    let file = s.mesh_vtk().expect("a mesh");
    let far = file
        .lines()
        .skip(5)
        .take(
            file.lines()
                .nth(4)
                .unwrap()
                .split(' ')
                .nth(1)
                .unwrap()
                .parse()
                .unwrap(),
        )
        .map(|l| {
            let v: Vec<f64> = l.split(' ').map(|n| n.parse().unwrap()).collect();
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        })
        .fold(0.0_f64, f64::max);
    // The box rests on the corners of the triangles, so nothing sticks out
    // further than the geometry does.
    assert!(far <= 1.0 + 1e-9, "a point left the model: {far}");
}

fn first_edge(s: &Session) -> u32 {
    let edges: serde_json::Value = serde_json::from_str(&s.edges_json()).unwrap();
    edges[0]["id"].as_u64().unwrap() as u32
}

fn generation(s: &Session) -> u64 {
    serde_json::from_str::<serde_json::Value>(&s.stats_json()).unwrap()["mesh"]["generation"]
        .as_u64()
        .unwrap()
}

/// An octahedron, which is a sphere as far as this test is concerned: six
/// corners at distance one, eight triangles, one surface.
const SPHERE: &str = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
6
1 1 0 0
2 -1 0 0
3 0 1 0
4 0 -1 0
5 0 0 1
6 0 0 -1
$EndNodes
$Elements
8
1 2 2 0 1 1 3 5
2 2 2 0 1 3 2 5
3 2 2 0 1 2 4 5
4 2 2 0 1 4 1 5
5 2 2 0 1 3 1 6
6 2 2 0 1 2 3 6
7 2 2 0 1 4 2 6
8 2 2 0 1 1 4 6
$EndElements
";

#[test]
fn the_buffer_carries_the_skin_and_nothing_inside_it() {
    use gecko_cmd::view::{MESH_HEADER, MESH_MAGIC};
    let mut s = cube();
    s.apply(Command::Mesh { segments: 3 }).unwrap();
    let bytes = s.mesh_buffer();
    assert_eq!(&bytes[0..4], &MESH_MAGIC);

    let count = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (nodes, quads) = (count(32), count(36));
    // Six faces of nine quadrangles, and the 56 nodes on the outside of a
    // 4 × 4 × 4 lattice — not the 64 of the whole thing.
    assert_eq!(quads, 54);
    assert_eq!(nodes, 56);
    // Three coordinates a node; four corners, a block and a quality a
    // quadrangle.
    assert_eq!(
        bytes.len(),
        MESH_HEADER + (nodes * 3 + quads * 4 + quads + quads) * 4
    );

    // And nothing at all when there is no mesh.
    s.apply(Command::ClearMesh).unwrap();
    assert_eq!(s.mesh_buffer().len(), MESH_HEADER);
}

/// A cylinder's surface: two circular curves, two discs, one tube.
fn cylinder(sides: usize, radius: f64, height: f64) -> String {
    let mut nodes = vec![[0.0, 0.0, 0.0], [0.0, 0.0, height]];
    for end in [0.0, height] {
        for k in 0..sides {
            let a = std::f64::consts::TAU * k as f64 / sides as f64;
            nodes.push([radius * a.cos(), radius * a.sin(), end]);
        }
    }
    let bottom = |k: usize| 3 + k % sides;
    let top = |k: usize| 3 + sides + k % sides;
    let mut elements: Vec<String> = Vec::new();
    for k in 0..sides {
        // The two rims, each a curve of its own.
        elements.push(format!("1 2 0 201 {} {}", bottom(k), bottom(k + 1)));
        elements.push(format!("1 2 0 202 {} {}", top(k), top(k + 1)));
        // The two discs, wound outward, and the tube between them.
        elements.push(format!("2 2 0 1 1 {} {}", bottom(k + 1), bottom(k)));
        elements.push(format!("2 2 0 2 2 {} {}", top(k), top(k + 1)));
        elements.push(format!(
            "2 2 0 3 {} {} {}",
            bottom(k),
            bottom(k + 1),
            top(k + 1)
        ));
        elements.push(format!("2 2 0 3 {} {} {}", bottom(k), top(k + 1), top(k)));
    }
    let mut out = String::from("$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n");
    out.push_str(&format!("{}\n", nodes.len()));
    for (i, p) in nodes.iter().enumerate() {
        out.push_str(&format!("{} {} {} {}\n", i + 1, p[0], p[1], p[2]));
    }
    out.push_str(&format!("$EndNodes\n$Elements\n{}\n", elements.len()));
    for (i, e) in elements.iter().enumerate() {
        out.push_str(&format!("{} {}\n", i + 1, e));
    }
    out.push_str("$EndElements\n");
    out
}

/// Six times the volume of a hexahedron, by the divergence theorem over its
/// six faces. Positive when the corners are wound the way VTK expects.
fn six_volumes(p: &[[f64; 3]; 8]) -> f64 {
    const FACES: [[usize; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let mut sum = 0.0;
    for face in FACES {
        for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
            let (x, y, z) = (p[face[a]], p[face[b]], p[face[c]]);
            sum += x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0])
                + x[2] * (y[0] * z[1] - y[1] * z[0]);
        }
    }
    sum
}

#[test]
fn a_pillowed_cylinder_meshes_the_right_way_out() {
    // The whole scenario, because this is the one that was wrong: box a
    // cylinder, put the eight corners on the two rims, lay a layer along
    // the four faces on the tube, and mesh it.
    //
    // The layer leaves a copy of each node where it was and draws the
    // original in. If what the node was pinned to goes **in** with it
    // rather than staying on the surface, the mesh along those edges is
    // projected back out onto the rim, and the cells turn inside out.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: cylinder(24, 1.0, 2.0),
        name: "cylinder.msh".to_string(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.0,
    })
    .unwrap();

    // The four corners of each end onto the rim they are meant to follow.
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&s.nodes_json()).unwrap();
    for (z, tag) in [(0.0, 201), (2.0, 202)] {
        let four: Vec<u32> = nodes
            .iter()
            .filter(|n| (n["pos"][2].as_f64().unwrap() - z).abs() < 0.5)
            .map(|n| n["id"].as_u64().unwrap() as u32)
            .collect();
        assert_eq!(four.len(), 4);
        s.apply(Command::ClassifyNode {
            nodes: four,
            on: Some(gecko_cmd::Spot { dim: 1, tag }.into()),
        })
        .unwrap();
    }

    // The four faces on the tube: the ones off the axis. Not the discs.
    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    let patch: Vec<gecko_cmd::PickedFace> = faces
        .iter()
        .filter(|f| {
            let at = &f["center"];
            at[0].as_f64().unwrap().abs() > 0.1 || at[1].as_f64().unwrap().abs() > 0.1
        })
        .map(|f| gecko_cmd::PickedFace {
            face: f["id"].as_u64().unwrap() as u32,
            block: f["block"].as_u64().unwrap() as u32,
        })
        .collect();
    assert_eq!(patch.len(), 4, "the four faces on the tube");
    s.apply(Command::Pillow {
        faces: patch.into_iter().map(Into::into).collect(),
        tolerance: gecko_cmd::SNAP_SHARE,
    })
    .unwrap();

    // The nodes on the rim are the ones **out** at the radius, and the
    // ones drawn in are on the discs they are still sitting in rather
    // than on the rims they have left.
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&s.nodes_json()).unwrap();
    for node in &nodes {
        let p = node["pos"].as_array().unwrap();
        let at = [0, 1, 2].map(|k| p[k].as_f64().unwrap());
        let radius = (at[0] * at[0] + at[1] * at[1]).sqrt();
        let on = &node["on"];
        if radius > 0.9 {
            assert_eq!(on["dim"], 1, "a node on the rim is on no curve: {node}");
        } else {
            assert_eq!(on["dim"], 2, "a node drawn in is not on its disc: {node}");
        }
    }

    s.apply(Command::Mesh { segments: 4 }).unwrap();
    let file = s.mesh_vtk().expect("a mesh");
    let lines: Vec<&str> = file.lines().collect();
    let count: usize = lines[4].split(' ').nth(1).unwrap().parse().unwrap();
    let points: Vec<[f64; 3]> = lines[5..5 + count]
        .iter()
        .map(|l| {
            let v: Vec<f64> = l.split(' ').map(|n| n.parse().unwrap()).collect();
            [v[0], v[1], v[2]]
        })
        .collect();

    let mut inside_out = 0;
    for line in lines.iter().filter(|l| l.starts_with("8 ")) {
        let ids: Vec<usize> = line
            .split(' ')
            .skip(1)
            .map(|n| n.parse().unwrap())
            .collect();
        let at: [[f64; 3]; 8] = std::array::from_fn(|k| points[ids[k]]);
        if six_volumes(&at) <= 0.0 {
            inside_out += 1;
        }
    }
    assert_eq!(inside_out, 0, "cells turned inside out");
}

/// The node of a session nearest a point.
fn node_near(s: &Session, p: [f64; 3]) -> u32 {
    let nodes: Vec<serde_json::Value> = serde_json::from_str(&s.nodes_json()).unwrap();
    let d = |n: &serde_json::Value| {
        (0..3)
            .map(|k| (n["pos"][k].as_f64().unwrap() - p[k]).powi(2))
            .sum::<f64>()
    };
    let best = nodes.iter().min_by(|a, b| d(a).total_cmp(&d(b))).unwrap();
    best["id"].as_u64().unwrap() as u32
}

fn quality(s: &Session, metric: Metric) -> serde_json::Value {
    serde_json::from_str(&s.quality_json(metric)).unwrap()
}

/// How many cells a cell buffer holds: six quadrangles apiece, and the
/// count of quadrangles is in its header.
fn cells_in(bytes: &[u8]) -> u64 {
    u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as u64 / 6
}

#[test]
fn the_quality_of_a_mesh_is_measured_and_its_worst_cells_can_be_drawn() {
    let mut s = cube();
    assert_eq!(
        s.quality_json(Metric::ScaledJacobian),
        "null",
        "no mesh, no figures"
    );

    // A sound cube: every cell a cube.
    s.apply(Command::Mesh { segments: 3 }).unwrap();
    let q = quality(&s, Metric::ScaledJacobian);
    assert_eq!(q["cells"], 27);
    assert_eq!(q["label"], "scaled Jacobian");
    assert!((q["min"].as_f64().unwrap() - 1.0).abs() < 1e-12);
    assert_eq!(q["beyond"][3], 0, "none below a half");
    assert_eq!(q["histogram"][9], 27);

    // One corner pushed most of the way through the opposite one: the cells
    // round it are turned inside out, and the message says so.
    let corner = node_near(&s, [1.0, 1.0, 1.0]);
    s.apply(Command::MoveNode {
        node: corner,
        to: [0.2, 0.2, 0.2],
        carrying: vec![],
        snap: 0.0,
    })
    .unwrap();
    let out = s.apply(Command::Mesh { segments: 3 }).unwrap();
    assert!(
        out.message.contains("min scaled Jacobian"),
        "{}",
        out.message
    );
    assert!(out.message.contains("inverted"), "{}", out.message);
    let q = quality(&s, Metric::ScaledJacobian);
    let inverted = q["beyond"][0].as_u64().unwrap();
    assert!(inverted > 0 && q["min"].as_f64().unwrap() < 0.0, "{q}");
    assert_eq!(q["inverted"].as_u64().unwrap(), inverted);
    let worst = &q["worst"];
    assert!(worst["value"].as_f64().unwrap() < 0.0);
    assert!(worst["size"].as_f64().unwrap() > 0.0);
    assert_eq!(q["blocks"][0]["inverted"].as_u64().unwrap(), inverted);

    // The cells below a threshold, in the mesh buffer's own encoding.
    let below_half = q["beyond"][3].as_u64().unwrap();
    let bytes = s.cells_buffer(Metric::ScaledJacobian, f64::NEG_INFINITY, 0.5);
    assert_eq!(cells_in(&bytes), below_half);
    let none = s.cells_buffer(Metric::ScaledJacobian, f64::NEG_INFINITY, -2.0);
    assert_eq!(cells_in(&none), 0);
}

#[test]
fn every_metric_is_named_and_every_bar_of_its_histogram_can_be_drawn_alone() {
    let mut s = cube();
    s.apply(Command::Mesh { segments: 3 }).unwrap();
    // A cube's cells are as good as each measure goes.
    let skew = quality(&s, Metric::Skewness);
    assert_eq!(skew["label"], "skewness");
    assert!(skew["max"].as_f64().unwrap() < 1e-12, "{skew}");
    assert_eq!(skew["higher_is_better"], false);
    let ratio = quality(&s, Metric::AspectRatio);
    assert_eq!(ratio["metric"], "aspect_ratio");
    assert!((ratio["max"].as_f64().unwrap() - 1.0).abs() < 1e-12);

    // Bent, so that every measure has something to say.
    let corner = node_near(&s, [1.0, 1.0, 1.0]);
    s.apply(Command::MoveNode {
        node: corner,
        to: [1.6, 1.3, 1.9],
        carrying: vec![],
        snap: 0.0,
    })
    .unwrap();
    s.apply(Command::Mesh { segments: 3 }).unwrap();
    for metric in Metric::ALL {
        let q = quality(&s, metric);
        let numbers = |key: &str| -> Vec<f64> {
            q[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect()
        };
        let (bins, histogram) = (numbers("bins"), numbers("histogram"));
        assert_eq!(histogram.iter().sum::<f64>(), 27.0, "{metric:?}");
        assert!(
            histogram.iter().filter(|n| **n > 0.0).count() > 1,
            "{metric:?}: {q}"
        );
        // A bar is drawn by asking for the cells from where it starts to
        // where the next one does, the first and last open at their ends —
        // and it holds exactly the cells the bar counts.
        for (i, n) in histogram.iter().enumerate() {
            let from = if i == 0 { f64::NEG_INFINITY } else { bins[i] };
            let to = bins.get(i + 1).copied().unwrap_or(f64::INFINITY);
            let drawn = cells_in(&s.cells_buffer(metric, from, to));
            assert_eq!(drawn as f64, *n, "{metric:?}, bar {i}");
        }
        // The worst cell is the worst by the metric's own direction.
        let worst = q["worst"]["value"].as_f64().unwrap();
        let end = if metric.higher_is_better() {
            "min"
        } else {
            "max"
        };
        assert_eq!(worst, q[end].as_f64().unwrap(), "{metric:?}");
    }

    // The skin's scores, one per quadrangle of the mesh buffer.
    let skin = s.mesh_buffer();
    let quads = u32::from_le_bytes(skin[36..40].try_into().unwrap()) as usize;
    let scores = s.skin_scores(Metric::AspectRatio);
    assert_eq!(scores.len(), quads * 4);
    let first = f32::from_le_bytes(scores[0..4].try_into().unwrap());
    assert!(first > 0.0 && first <= 1.0, "{first}");

    // A name that is no metric says which ones there are, and no name at
    // all is the scaled Jacobian.
    let answer: serde_json::Value =
        serde_json::from_str(&s.quality_json_named("jacobian")).unwrap();
    let error = answer["error"].as_str().unwrap();
    assert!(error.contains("aspect_ratio"), "{answer}");
    let unnamed: serde_json::Value = serde_json::from_str(&s.quality_json_named("")).unwrap();
    assert_eq!(unnamed, quality(&s, Metric::ScaledJacobian));
}
