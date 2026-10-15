//! Discretising through the door, and the file that comes out.

use gecko_cmd::{Command, Session};

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
    assert_eq!(out.message, "meshed — 64 nodes, 27 hexahedra");
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
    // And which block it came from, for anything that wants to colour it.
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
    assert_eq!(
        bytes.len(),
        MESH_HEADER + (nodes * 3 + quads * 4 + quads) * 4
    );

    // And nothing at all when there is no mesh.
    s.apply(Command::ClearMesh).unwrap();
    assert_eq!(s.mesh_buffer().len(), MESH_HEADER);
}
