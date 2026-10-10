//! Loading a geometry through the door, and the bytes that come back.

use gecko_cmd::view::{GEOMETRY_HEADER, GEOMETRY_MAGIC};
use gecko_cmd::{Command, Session};

/// A unit square with its four sides and four corners, each on an entity tag
/// of its own. The smallest thing that is a geometry at all.
const SQUARE: &str = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
4
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
$EndNodes
$Elements
10
1 15 2 0 21 1
2 15 2 0 22 2
3 15 2 0 23 3
4 15 2 0 24 4
5 1 2 0 11 1 2
6 1 2 0 12 2 3
7 1 2 0 13 3 4
8 1 2 0 14 4 1
9 2 2 0 1 1 2 3
10 2 2 0 1 1 3 4
$EndElements
";

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

fn load(session: &mut Session, msh: &str) -> String {
    session
        .apply(Command::LoadGeometry {
            msh: msh.to_string(),
            name: "square.msh".to_string(),
        })
        .expect("a square")
        .message
}

fn stats(session: &Session) -> serde_json::Value {
    serde_json::from_str(&session.stats_json()).unwrap()
}

#[test]
fn a_geometry_comes_in_through_the_same_door_as_everything_else() {
    let mut s = Session::new().unwrap();
    let message = load(&mut s, SQUARE);
    assert!(message.contains("square.msh"), "{message}");
    assert!(message.contains("4 corners"), "{message}");
    assert!(message.contains("4 curves"), "{message}");
    assert!(message.contains("1 surface"), "{message}");
    assert!(message.contains("2 triangles"), "{message}");

    let geometry = s.geometry().expect("loaded");
    assert_eq!(geometry.surfaces().len(), 1);
    assert_eq!(geometry.curves().len(), 4);
    assert_eq!(geometry.corners().len(), 4);
}

#[test]
fn the_json_door_is_the_same_door_for_a_geometry_too() {
    let mut typed = Session::new().unwrap();
    let mut json = Session::new().unwrap();
    load(&mut typed, SQUARE);
    let command = serde_json::json!({ "op": "load_geometry", "msh": SQUARE, "name": "square.msh" });
    let reply = json.apply_json(&command.to_string());
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    assert_eq!(typed.geometry_buffer(), json.geometry_buffer());
}

#[test]
fn a_file_that_is_not_a_mesh_is_refused_and_changes_nothing() {
    let mut s = Session::new().unwrap();
    load(&mut s, SQUARE);
    let before = s.geometry_buffer();

    let reply = s.apply_json(r#"{"op":"load_geometry","msh":"hello"}"#);
    assert!(reply.contains(r#""ok":false"#), "{reply}");
    assert!(reply.contains("$MeshFormat"), "{reply}");
    // And the geometry that was already there is still there: a refused
    // load is not a load of nothing.
    assert_eq!(s.geometry_buffer(), before);
    assert_eq!(stats(&s)["geometry"]["generation"], 1);
}

#[test]
fn the_generation_is_what_tells_a_client_to_fetch_again() {
    // Counts would not do it: two geometries of the same size are still two
    // geometries, and a client that compared counts would go on drawing the
    // first one.
    let mut s = Session::new().unwrap();
    assert_eq!(stats(&s)["geometry"]["generation"], 0);
    assert!(stats(&s)["geometry"]["surfaces"].is_null());

    load(&mut s, SQUARE);
    assert_eq!(stats(&s)["geometry"]["generation"], 1);
    assert_eq!(stats(&s)["geometry"]["surfaces"], 1);
    assert_eq!(stats(&s)["geometry"]["curves"], 4);
    assert_eq!(stats(&s)["geometry"]["corners"], 4);
    assert_eq!(stats(&s)["geometry"]["triangles"], 2);
    assert_eq!(stats(&s)["geometry"]["segments"], 4);

    load(&mut s, SQUARE);
    assert_eq!(stats(&s)["geometry"]["generation"], 2);

    s.apply(Command::ClearGeometry).unwrap();
    assert_eq!(stats(&s)["geometry"]["generation"], 3);
    assert!(s.geometry().is_none());
}

#[test]
fn tetrahedra_are_counted_in_the_message_rather_than_passed_over_in_silence() {
    let text = SQUARE
        .replace("$Elements\n10", "$Elements\n11")
        .replace("$EndElements", "11 4 2 0 7 1 2 3 4\n$EndElements");
    let mut s = Session::new().unwrap();
    let message = load(&mut s, &text);
    assert!(
        message.contains("1 element of other kinds ignored"),
        "{message}"
    );
}

#[test]
fn the_geometry_encoding_is_the_layout_a_client_will_decode() {
    let mut s = Session::new().unwrap();
    load(&mut s, SQUARE);
    let bytes = s.geometry_buffer();

    assert_eq!(&bytes[0..4], &GEOMETRY_MAGIC);
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
    let count = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (nodes, triangles, segments, corners) = (count(32), count(36), count(40), count(44));
    assert_eq!((nodes, triangles, segments, corners), (4, 2, 4, 4));
    // Per node three f32. Per triangle three indices and a tag, per segment
    // two and a tag, per corner one and a tag. Exact, or a client's typed
    // array views walk off the end of the buffer.
    assert_eq!(
        bytes.len(),
        GEOMETRY_HEADER + nodes * 12 + triangles * 16 + segments * 12 + corners * 8
    );
    assert_eq!(GEOMETRY_HEADER % 8, 0, "the f64 centre has to sit aligned");
}

#[test]
fn an_empty_session_still_hands_back_a_buffer_a_client_can_read() {
    // Rather than nothing, or a special case at the other end: a client that
    // has never been given a geometry decodes an empty one.
    let s = Session::new().unwrap();
    let bytes = s.geometry_buffer();
    assert_eq!(bytes.len(), GEOMETRY_HEADER);
    assert_eq!(&bytes[0..4], &GEOMETRY_MAGIC);
}

/// The square, moved a million units out.
fn far_square(far: f64) -> String {
    SQUARE
        .replace("1 0 0 0", &format!("1 {far} 0 0"))
        .replace("2 1 0 0", &format!("2 {} 0 0", far + 1.0))
        .replace("3 1 1 0", &format!("3 {} 1 0", far + 1.0))
        .replace("4 0 1 0", &format!("4 {far} 1 0"))
}

fn centre_of(bytes: &[u8]) -> [f64; 3] {
    [0, 1, 2].map(|k| f64::from_le_bytes(bytes[8 + k * 8..16 + k * 8].try_into().unwrap()))
}

fn first_positions(bytes: &[u8], n: usize) -> Vec<f32> {
    bytes[GEOMETRY_HEADER..]
        .as_chunks::<4>()
        .0
        .iter()
        .take(n)
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

#[test]
fn a_geometry_far_from_the_origin_keeps_its_precision() {
    // The trap f32 positions set: at a million units the steps are about
    // 0.06, which is visible jitter. The rebasing origin is settled over the
    // geometry as well as the blocks, and this is the case that says so —
    // with no blocks at all, the geometry is the only thing that can move it.
    let far = 1.0e6;
    let mut s = Session::new().unwrap();
    load(&mut s, &far_square(far));

    assert!(
        (centre_of(&s.geometry_buffer())[0] - far).abs() < 1.0,
        "the origin did not follow the geometry"
    );
    let positions = first_positions(&s.geometry_buffer(), 12);
    assert!(positions.iter().all(|x| x.abs() < 2.0), "{positions:?}");
}

#[test]
fn the_geometry_and_the_blocks_are_rebased_on_one_origin() {
    // Two origins would draw the geometry somewhere other than the blocks
    // that are supposed to follow it. They are produced by two different
    // calls, so this is a real question and not a tautology.
    let far = 1.0e6;
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock {
        corners: UNIT_CUBE.map(|p| [p[0] + far, p[1], p[2]]),
    })
    .unwrap();
    load(&mut s, &far_square(far));

    assert_eq!(centre_of(&s.view_buffer()), centre_of(&s.geometry_buffer()));
    assert!(centre_of(&s.view_buffer())[0] > far / 2.0);
}

#[test]
fn loading_a_geometry_makes_the_view_stale_even_though_no_block_moved() {
    // Because the origin the blocks are drawn against is settled over the
    // geometry too: a client that redrew only the geometry would leave the
    // blocks rebased on an origin nobody uses any more.
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();
    let out = s
        .apply(Command::LoadGeometry {
            msh: SQUARE.to_string(),
            name: String::new(),
        })
        .unwrap();
    assert!(out.view_changed);
    assert!(
        out.message.starts_with("geometry loaded"),
        "{}",
        out.message
    );
}

/// A unit cube's surface, each of its six faces an entity of its own.
const CUBE_MSH: &str = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
8
1 0 0 0
2 2 0 0
3 2 1 0
4 0 1 0
5 0 0 3
6 2 0 3
7 2 1 3
8 0 1 3
$EndNodes
$Elements
12
1 2 2 0 1 1 2 3
2 2 2 0 1 1 3 4
3 2 2 0 2 5 6 7
4 2 2 0 2 5 7 8
5 2 2 0 3 1 2 6
6 2 2 0 3 1 6 5
7 2 2 0 4 4 3 7
8 2 2 0 4 4 7 8
9 2 2 0 5 1 4 8
10 2 2 0 5 1 8 5
11 2 2 0 6 2 3 7
12 2 2 0 6 2 7 6
$EndElements
";

fn corners(session: &Session) -> Vec<[f64; 3]> {
    let nodes: serde_json::Value = serde_json::from_str(&session.nodes_json()).unwrap();
    nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            let p = n["pos"].as_array().unwrap();
            [0, 1, 2].map(|k| p[k].as_f64().unwrap())
        })
        .collect()
}

fn span(session: &Session, axis: usize) -> (f64, f64) {
    let all: Vec<f64> = corners(session).iter().map(|p| p[axis]).collect();
    (
        all.iter().cloned().fold(f64::INFINITY, f64::min),
        all.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    )
}

#[test]
fn a_box_round_the_geometry_is_one_block() {
    let mut s = Session::new().unwrap();
    load(&mut s, CUBE_MSH);
    let out = s
        .apply(Command::BoxGeometry {
            surfaces: vec![],
            frame: gecko_cmd::BoxFrame::Axes,
            margin: 0.0,
        })
        .expect("a box");
    assert!(
        out.message.contains("the whole geometry"),
        "{}",
        out.message
    );
    assert!(
        out.message.contains("square to the world"),
        "{}",
        out.message
    );

    assert_eq!(stats(&s)["blocks"], 1);
    assert_eq!(stats(&s)["nodes"], 8);
    assert_eq!(span(&s, 0), (0.0, 2.0));
    assert_eq!(span(&s, 1), (0.0, 1.0));
    assert_eq!(span(&s, 2), (0.0, 3.0));
}

#[test]
fn the_default_margin_is_the_one_the_kernel_knows() {
    // Left out of the command on purpose: one definition of how much room a
    // box leaves, in the kernel, rather than a copy of it in each façade.
    let mut s = Session::new().unwrap();
    load(&mut s, CUBE_MSH);
    let reply = s.apply_json(r#"{"op":"box_geometry"}"#);
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    let (lo, hi) = span(&s, 0);
    assert!(
        (lo - -0.1).abs() < 1e-9 && (hi - 2.1).abs() < 1e-9,
        "{lo}..{hi}"
    );
    assert!((gecko_cmd::BOX_MARGIN - 0.1).abs() < 1e-12);
}

#[test]
fn a_box_round_a_selection_takes_only_what_is_named() {
    let mut s = Session::new().unwrap();
    load(&mut s, CUBE_MSH);
    // Two opposite sides, which between them reach every corner.
    let reply = s.apply_json(r#"{"op":"box_geometry","surfaces":[1,2],"margin":0}"#);
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    assert!(reply.contains("2 surfaces"), "{reply}");
    assert_eq!(span(&s, 2), (0.0, 3.0));
}

#[test]
fn a_box_with_no_geometry_to_put_it_round_says_so() {
    let mut s = Session::new().unwrap();
    let reply = s.apply_json(r#"{"op":"box_geometry"}"#);
    assert!(reply.contains(r#""ok":false"#), "{reply}");
    assert!(reply.contains("no geometry is loaded"), "{reply}");
    assert_eq!(stats(&s)["blocks"], 0);
}

#[test]
fn a_flat_selection_is_refused_rather_than_built_flat() {
    let mut s = Session::new().unwrap();
    load(&mut s, CUBE_MSH);
    let reply = s.apply_json(r#"{"op":"box_geometry","surfaces":[1]}"#);
    assert!(reply.contains(r#""ok":false"#), "{reply}");
    assert!(reply.contains("flat"), "{reply}");
    assert_eq!(stats(&s)["blocks"], 0);
}

#[test]
fn the_other_frame_comes_through_the_door_by_name() {
    let mut s = Session::new().unwrap();
    load(&mut s, CUBE_MSH);
    let reply = s.apply_json(r#"{"op":"box_geometry","frame":"inertia","margin":0}"#);
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    assert!(reply.contains("its own axes"), "{reply}");
    // Square to the world already, so the box is the same box — to within
    // the rotation it goes through and comes back from.
    let (lo, hi) = span(&s, 2);
    assert!(lo.abs() < 1e-9 && (hi - 3.0).abs() < 1e-9, "{lo}..{hi}");
}
