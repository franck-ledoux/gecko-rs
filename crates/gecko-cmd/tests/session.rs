//! The door, and the bytes that come out of it.

use gecko_cmd::view::{HEADER, MAGIC};
use gecko_cmd::{Command, Session, SessionError, ViewBuffer};

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
    let v = ViewBuffer::of(s.blocking(), None);

    assert_eq!(v.positions.len() / 3, 8);
    assert_eq!(v.names.len(), 8);
    assert_eq!(v.edges.len() / 2, 12);
    assert_eq!(v.centre, [0.5, 0.5, 0.5]);
    // Every edge index addresses a node that exists.
    assert!(v
        .edges
        .iter()
        .all(|i| (*i as usize) < v.positions.len() / 3));
    // Rebased: the unit cube sits in [-0.5, 0.5].
    assert!(v.positions.iter().all(|x| x.abs() <= 0.5 + 1e-6));

    // The surface: six quadrangles, one block, six faces.
    assert_eq!(v.quads.len() / 4, 6);
    assert_eq!(v.quad_faces.len(), 6);
    assert_eq!(v.quad_blocks.len(), 6);
    assert!(v
        .quads
        .iter()
        .all(|i| (*i as usize) < v.positions.len() / 3));
    assert!(v.quad_blocks.windows(2).all(|w| w[0] == w[1]));
    let mut faces = v.quad_faces.clone();
    faces.sort();
    faces.dedup();
    assert_eq!(faces.len(), 6);
}

#[test]
fn two_blocks_each_bring_a_closed_surface() {
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    s.apply(Command::CreateCube {
        origin: [4.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    let v = ViewBuffer::of(s.blocking(), None);

    assert_eq!(v.quads.len() / 4, 12);
    let mut blocks = v.quad_blocks.clone();
    blocks.sort();
    blocks.dedup();
    assert_eq!(blocks.len(), 2);
    // No name is the sentinel: every quadrangle knows what it belongs to, and
    // the browser colours by these.
    assert!(v.quad_faces.iter().all(|n| *n != u32::MAX));
    assert!(v.quad_blocks.iter().all(|n| *n != u32::MAX));
}

#[test]
fn the_encoding_is_the_layout_a_client_will_decode() {
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();
    let bytes = s.view_buffer();

    assert_eq!(&bytes[0..4], &MAGIC);
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 3);
    let n_nodes = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    let n_edges = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    let n_quads = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    assert_eq!((n_nodes, n_edges, n_quads), (8, 12, 6));
    // Exact length, or a client's typed-array views walk off the end.
    // Per node: three f32 of position and one u32 of name. Per quadrangle:
    // four corner indices and the two names it is coloured by.
    assert_eq!(
        bytes.len(),
        HEADER + n_nodes * 16 + n_edges * 8 + n_quads * 24
    );
    // Everything after the f64 centre is 4-aligned, so a client can make typed
    // array views over these bytes without copying them first.
    assert_eq!(HEADER % 4, 0);
}

#[test]
fn a_model_far_from_the_origin_keeps_its_precision() {
    // The trap f32 positions set: at a million units, f32 steps are about 0.06.
    let far = [1.0e6, 2.0e6, -5.0e5];
    let corners = UNIT_CUBE.map(|p| [p[0] + far[0], p[1] + far[1], p[2] + far[2]]);

    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners }).unwrap();
    let v = ViewBuffer::of(s.blocking(), None);

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

// ---------------------------------------------------------------------------
// Creating a cube, and moving a node.
// ---------------------------------------------------------------------------

fn nodes(s: &Session) -> Vec<(u32, [f64; 3])> {
    serde_json::from_str::<serde_json::Value>(&s.nodes_json())
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            let p = n["pos"].as_array().unwrap();
            (
                n["id"].as_u64().unwrap() as u32,
                [
                    p[0].as_f64().unwrap(),
                    p[1].as_f64().unwrap(),
                    p[2].as_f64().unwrap(),
                ],
            )
        })
        .collect()
}

#[test]
fn creating_a_cube_is_the_same_thing_as_adding_its_eight_corners() {
    let mut by_cube = Session::new().unwrap();
    let mut by_corners = Session::new().unwrap();

    by_cube
        .apply(Command::CreateCube {
            origin: [0.0, 0.0, 0.0],
            size: 1.0,
        })
        .unwrap();
    by_corners
        .apply(Command::AddBlock { corners: UNIT_CUBE })
        .unwrap();

    // The convenience has to mean exactly the long way round, or the journal
    // records something that does not reproduce.
    assert_eq!(by_cube.view_buffer(), by_corners.view_buffer());
    assert_eq!(by_cube.stats_json(), by_corners.stats_json());
}

#[test]
fn a_cube_needs_a_positive_size() {
    let mut s = Session::new().unwrap();
    for bad in [0.0, -1.0, f64::NAN] {
        let err = s
            .apply(Command::CreateCube {
                origin: [0.0; 3],
                size: bad,
            })
            .unwrap_err();
        assert!(matches!(err, SessionError::BadSize(_)), "{bad}: {err}");
    }
    assert_eq!(s.blocking().nb_blocks(), 0);
}

#[test]
fn the_view_names_every_node_it_draws() {
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: 1.0,
    })
    .unwrap();
    let v = ViewBuffer::of(s.blocking(), None);

    assert_eq!(v.names.len(), v.positions.len() / 3);
    // A client picks a sphere and has something to call it by — and the names
    // are distinct, or two spheres would answer to one.
    let mut unique = v.names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 8);
    assert!(
        v.names.iter().all(|n| *n != u32::MAX),
        "an unnamed node: {:?}",
        v.names
    );
}

#[test]
fn moving_a_node_moves_what_the_view_said_it_would() {
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: 1.0,
    })
    .unwrap();

    // Pick a node the way a client does: out of the view buffer.
    let v = ViewBuffer::of(s.blocking(), None);
    let index = 3usize;
    let name = v.names[index];
    let before = nodes(&s);

    let out = s
        .apply(Command::MoveNode {
            node: name,
            to: [9.0, 8.0, 7.0],
        })
        .unwrap();
    assert!(out.view_changed);

    let after = nodes(&s);
    assert_eq!(after.len(), before.len());
    for (id, pos) in &after {
        let was = before.iter().find(|(i, _)| i == id).unwrap().1;
        if *id == name {
            assert_eq!(*pos, [9.0, 8.0, 7.0]);
        } else {
            assert_eq!(*pos, was, "node {id} moved as well");
        }
    }
    // Moving is not topological: the structure is the same shape.
    assert_eq!(s.blocking().nb_nodes(), 8);
    assert_eq!(s.blocking().nb_edges(), 12);
}

#[test]
fn moving_a_node_that_does_not_exist_is_refused() {
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: 1.0,
    })
    .unwrap();
    let before = s.view_buffer();

    let reply = s.apply_json(r#"{"op":"move_node","node":4242,"to":[1,2,3]}"#);
    assert!(reply.contains(r#""ok":false"#), "{reply}");
    assert!(
        reply.contains("4242"),
        "the message should name it: {reply}"
    );
    assert_eq!(s.view_buffer(), before);
}

#[test]
fn both_new_commands_travel_as_json() {
    let mut s = Session::new().unwrap();
    assert!(s
        .apply_json(r#"{"op":"create_cube","origin":[0,0,0],"size":2}"#)
        .contains(r#""ok":true"#));
    let name = ViewBuffer::of(s.blocking(), None).names[0];
    let reply = s.apply_json(&format!(
        r#"{{"op":"move_node","node":{name},"to":[0.5,0.5,0.5]}}"#
    ));
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    assert!(reply.contains(r#""viewChanged":true"#), "{reply}");

    for bad in [
        r#"{"op":"create_cube","origin":[0,0,0],"size":0}"#,
        r#"{"op":"create_cube","origin":[0,0,0]}"#,
        r#"{"op":"move_node","node":1}"#,
    ] {
        assert!(s.apply_json(bad).contains(r#""ok":false"#), "{bad}");
    }
}

#[test]
fn the_view_stays_put_when_a_node_moves() {
    // The bug this guards: the rebasing origin was the bounding box's centre,
    // recomputed every time. So dragging one node slid the other seven across
    // the screen — all of them are drawn relative to that centre, and it had
    // just moved.
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: 1.0,
    })
    .unwrap();
    let before = ViewBuffer::of(s.blocking(), None).centre;
    let _ = s.view_buffer(); // the session adopts that origin

    let name = ViewBuffer::of(s.blocking(), Some(before)).names[0];
    s.apply(Command::MoveNode {
        node: name,
        to: [0.3, -0.4, 0.2],
    })
    .unwrap();

    let after = ViewBuffer::of(s.blocking(), Some(before)).centre;
    assert_eq!(after, before, "the whole scene would have shifted");
}

#[test]
fn the_origin_follows_a_model_that_walks_away() {
    // It is kept still for convenience, not forever: the point of rebasing is
    // that f32 coordinates stay small, so an origin the model has left behind
    // has to be abandoned.
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: 1.0,
    })
    .unwrap();
    let near = ViewBuffer::of(s.blocking(), None).centre;

    // A nudge keeps it.
    assert_eq!(
        ViewBuffer::of(s.blocking(), Some([0.6, 0.5, 0.5])).centre,
        [0.6, 0.5, 0.5]
    );
    // A different continent does not.
    let far = ViewBuffer::of(s.blocking(), Some([1.0e6, 0.0, 0.0])).centre;
    assert_eq!(far, near);
}

#[test]
fn an_empty_session_keeps_whatever_origin_it_had() {
    let s = Session::new().unwrap();
    assert_eq!(ViewBuffer::of(s.blocking(), None).centre, [0.0; 3]);
    assert_eq!(
        ViewBuffer::of(s.blocking(), Some([7.0, 8.0, 9.0])).centre,
        [7.0, 8.0, 9.0]
    );
}
