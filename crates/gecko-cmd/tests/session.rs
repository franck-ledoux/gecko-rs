//! The door, and the bytes that come out of it.

use gecko_cmd::view::{HEADER, MAGIC};
use gecko_cmd::{Command, Session, ViewBuffer};

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

#[test]
fn a_command_goes_in_and_an_outcome_comes_out() {
    let mut s = Session::new().unwrap();
    let out = s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();
    assert!(out.view_changed);
    assert!(out.message.contains("12 edges"), "{}", out.message);
}

#[test]
fn the_json_door_is_the_same_door_as_the_typed_one() {
    let mut typed = Session::new().unwrap();
    let mut json = Session::new().unwrap();

    typed
        .apply(Command::AddBlock { corners: UNIT_CUBE })
        .unwrap();
    let reply = json.apply_json(
        r#"{"op":"add_block","corners":[[0,0,0],[1,0,0],[1,1,0],[0,1,0],[0,0,1],[1,0,1],[1,1,1],[0,1,1]]}"#,
    );
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    assert!(reply.contains(r#""viewChanged":true"#), "{reply}");

    // Both façades will take this path, so it has to land in the same place.
    assert_eq!(typed.view_buffer(), json.view_buffer());
    assert_eq!(typed.stats_json(), json.stats_json());
}

#[test]
fn bad_json_is_an_error_not_a_panic() {
    let mut s = Session::new().unwrap();
    for bad in [
        "",
        "{}",
        "not json",
        r#"{"op":"nonesuch"}"#,
        r#"{"op":"add_block"}"#,
    ] {
        let reply = s.apply_json(bad);
        assert!(reply.contains(r#""ok":false"#), "{bad} -> {reply}");
    }
    assert_eq!(
        s.blocking().nb_blocks(),
        0,
        "a refused command changed state"
    );
}

#[test]
fn the_view_buffer_describes_the_block() {
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();
    let v = ViewBuffer::of(s.blocking());

    assert_eq!(v.positions.len() / 3, 8);
    assert_eq!(v.edges.len() / 2, 12);
    assert_eq!(v.centre, [0.5, 0.5, 0.5]);
    // Every edge index addresses a node that exists.
    assert!(v
        .edges
        .iter()
        .all(|i| (*i as usize) < v.positions.len() / 3));
    // Rebased: the unit cube sits in [-0.5, 0.5].
    assert!(v.positions.iter().all(|x| x.abs() <= 0.5 + 1e-6));
}

#[test]
fn the_encoding_is_the_layout_a_client_will_decode() {
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();
    let bytes = s.view_buffer();

    assert_eq!(&bytes[0..4], &MAGIC);
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
    let n_nodes = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    let n_edges = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    assert_eq!((n_nodes, n_edges), (8, 12));
    // Exact length, or a client's typed-array views walk off the end.
    assert_eq!(bytes.len(), HEADER + n_nodes * 12 + n_edges * 8);
}

#[test]
fn a_model_far_from_the_origin_keeps_its_precision() {
    // The trap f32 positions set: at a million units, f32 steps are about 0.06.
    let far = [1.0e6, 2.0e6, -5.0e5];
    let corners = UNIT_CUBE.map(|p| [p[0] + far[0], p[1] + far[1], p[2] + far[2]]);

    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners }).unwrap();
    let v = ViewBuffer::of(s.blocking());

    assert!(
        v.positions.iter().all(|x| x.abs() <= 0.5 + 1e-6),
        "not rebased"
    );
    // Recompose f64 centre + f32 offset and land back on the cube's corners.
    for p in v.positions.as_chunks::<3>().0 {
        for k in 0..3 {
            let back = v.centre[k] + p[k] as f64;
            let expected = far[k] + if p[k] < 0.0 { 0.0 } else { 1.0 };
            assert!((back - expected).abs() < 1e-3, "{back} vs {expected}");
        }
    }
}

#[test]
fn an_empty_session_still_produces_a_well_formed_buffer() {
    let s = Session::new().unwrap();
    let bytes = s.view_buffer();
    assert_eq!(bytes.len(), HEADER);
    assert_eq!(&bytes[0..4], &MAGIC);
}
