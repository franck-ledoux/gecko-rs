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
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 4);
    let n_nodes = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    let n_edges = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    let n_quads = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    assert_eq!((n_nodes, n_edges, n_quads), (8, 12, 6));
    // Exact length, or a client's typed-array views walk off the end.
    // Per node: three f32 of position and one u32 of name. Per edge: two
    // node indices and a name. Per quadrangle: four corner indices and the
    // two names it is coloured by.
    assert_eq!(
        bytes.len(),
        HEADER + n_nodes * 16 + n_edges * 12 + n_quads * 24
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

/// Welding two blocks, through the same door everything else uses.
mod gluing {
    use super::*;

    fn two_cubes() -> Session {
        let mut s = Session::new().unwrap();
        for x in [0.0, 1.0] {
            s.apply(Command::CreateCube {
                origin: [x, 0.0, 0.0],
                size: 1.0,
            })
            .unwrap();
        }
        s
    }

    /// The face of the n-th listed block whose centre sits at this x.
    fn face_at(s: &Session, block: usize, x: f64) -> u32 {
        let faces: serde_json::Value = serde_json::from_str(&s.faces_json()).unwrap();
        let faces = faces.as_array().unwrap();
        let mut blocks: Vec<u64> = faces.iter().map(|f| f["block"].as_u64().unwrap()).collect();
        blocks.dedup();
        faces
            .iter()
            .filter(|f| f["block"].as_u64() == Some(blocks[block]))
            .find(|f| (f["centre"][0].as_f64().unwrap() - x).abs() < 1e-9)
            .expect("a face there")["id"]
            .as_u64()
            .unwrap() as u32
    }

    #[test]
    fn the_faces_a_script_can_name() {
        let s = two_cubes();
        let faces: serde_json::Value = serde_json::from_str(&s.faces_json()).unwrap();
        // Six per block, each with somewhere to point a mouse or a script.
        assert_eq!(faces.as_array().unwrap().len(), 12);
        for f in faces.as_array().unwrap() {
            assert!(f["id"].is_u64() && f["block"].is_u64());
            assert_eq!(f["centre"].as_array().unwrap().len(), 3);
        }
    }

    #[test]
    fn gluing_goes_through_the_json_door_like_everything_else() {
        let mut s = two_cubes();
        let (first, second) = (face_at(&s, 0, 1.0), face_at(&s, 1, 1.0));

        let raw = s.apply_json(&format!(
            r#"{{"op":"glue_faces","first":{first},"second":{second}}}"#
        ));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");
        assert_eq!(reply["outcome"]["viewChanged"], true);

        let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
        assert_eq!(stats["nodes"], 12);
        assert_eq!(stats["edges"], 20);
        assert_eq!(stats["faces"], 11);
        assert_eq!(stats["blocks"], 2);
    }

    #[test]
    fn the_surface_still_gives_each_block_a_closed_shell() {
        let mut s = two_cubes();
        let (first, second) = (face_at(&s, 0, 1.0), face_at(&s, 1, 1.0));
        s.apply(Command::GlueFaces { first, second }).unwrap();
        let v = ViewBuffer::of(s.blocking(), None);

        // Twelve quadrangles for two blocks, as before — but one face name is
        // now shared, which is how the viewer knows it is internal.
        assert_eq!(v.quads.len() / 4, 12);
        let shared = v
            .quad_faces
            .iter()
            .filter(|n| v.quad_faces.iter().filter(|m| m == n).count() == 2)
            .count();
        assert_eq!(shared, 2, "one face, listed once per block");
    }

    #[test]
    fn a_refused_glue_is_reported_and_changes_nothing() {
        let mut s = two_cubes();
        let before = s.view_buffer();
        let raw = s.apply_json(r#"{"op":"glue_faces","first":1,"second":1}"#);
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], false);
        assert!(reply["error"].as_str().unwrap().contains("itself"), "{raw}");
        assert_eq!(s.view_buffer(), before);
    }

    #[test]
    fn two_sessions_glued_the_same_way_ship_the_same_bytes() {
        // The cross-mode test rests on this, and gluing is the first operation
        // that walks an orbit to decide what to do.
        let build = || {
            let mut s = two_cubes();
            let (first, second) = (face_at(&s, 0, 1.0), face_at(&s, 1, 1.0));
            s.apply(Command::GlueFaces { first, second }).unwrap();
            s
        };
        assert_eq!(build().view_buffer(), build().view_buffer());
    }
}

/// Taking a block away, through the same door.
mod removal {
    use super::*;

    fn three_in_a_row() -> Session {
        let mut s = Session::new().unwrap();
        for x in [0.0, 1.0, 2.0] {
            s.apply(Command::CreateCube {
                origin: [x, 0.0, 0.0],
                size: 1.0,
            })
            .unwrap();
        }
        s
    }

    fn block_names(s: &Session) -> Vec<u32> {
        let blocks: serde_json::Value = serde_json::from_str(&s.blocks_json()).unwrap();
        blocks
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["id"].as_u64().unwrap() as u32)
            .collect()
    }

    #[test]
    fn the_blocks_a_script_can_name() {
        let s = three_in_a_row();
        let blocks: serde_json::Value = serde_json::from_str(&s.blocks_json()).unwrap();
        let blocks = blocks.as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        // Each cube's centre, in the order they were made.
        for (n, block) in blocks.iter().enumerate() {
            assert!((block["centre"][0].as_f64().unwrap() - (n as f64 + 0.5)).abs() < 1e-9);
            assert!((block["centre"][1].as_f64().unwrap() - 0.5).abs() < 1e-9);
        }
    }

    #[test]
    fn removing_goes_through_the_json_door_like_everything_else() {
        let mut s = three_in_a_row();
        let middle = block_names(&s)[1];

        let raw = s.apply_json(&format!(r#"{{"op":"remove_block","block":{middle}}}"#));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");
        assert_eq!(reply["outcome"]["viewChanged"], true);

        let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
        assert_eq!(stats["blocks"], 2);
        assert_eq!(stats["nodes"], 16);
        // And the one that went is the one that was named.
        assert!(!block_names(&s).contains(&middle));
    }

    #[test]
    fn a_name_no_block_answers_to_is_reported_and_changes_nothing() {
        let mut s = three_in_a_row();
        let before = s.view_buffer();
        let raw = s.apply_json(r#"{"op":"remove_block","block":4242}"#);
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], false);
        assert!(
            reply["error"].as_str().unwrap().contains("no block"),
            "{raw}"
        );
        assert_eq!(s.view_buffer(), before);
    }

    #[test]
    fn the_surface_loses_exactly_that_block() {
        let mut s = three_in_a_row();
        let names = block_names(&s);
        s.apply(Command::RemoveBlock { block: names[0] }).unwrap();
        let v = ViewBuffer::of(s.blocking(), None);

        assert_eq!(v.quads.len() / 4, 12);
        assert!(!v.quad_blocks.contains(&names[0]));
        for kept in [names[1], names[2]] {
            assert_eq!(v.quad_blocks.iter().filter(|n| **n == kept).count(), 6);
        }
    }

    #[test]
    fn two_sessions_that_removed_the_same_block_ship_the_same_bytes() {
        let build = || {
            let mut s = three_in_a_row();
            let names = block_names(&s);
            s.apply(Command::RemoveBlock { block: names[1] }).unwrap();
            s
        };
        assert_eq!(build().view_buffer(), build().view_buffer());
    }
}

/// The sheet through an edge, as a question asked of the session.
mod sheets {
    use super::*;

    #[test]
    fn a_lone_cube_answers_with_four_edges() {
        let mut s = Session::new().unwrap();
        s.apply(Command::CreateCube {
            origin: [0.0, 0.0, 0.0],
            size: 1.0,
        })
        .unwrap();

        let view = ViewBuffer::of(s.blocking(), None);
        let answer: serde_json::Value =
            serde_json::from_str(&s.sheet_json(view.edge_names[0])).unwrap();
        assert_eq!(answer["ok"], true);
        let edges = answer["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 4);
        // Each one says where a cut would run from and to, which is what the
        // viewer draws and what a cut will use.
        for edge in edges {
            assert_eq!(edge["from"].as_array().unwrap().len(), 3);
            assert_eq!(edge["to"].as_array().unwrap().len(), 3);
            assert_ne!(edge["from"], edge["to"]);
        }
    }

    #[test]
    fn a_name_no_edge_answers_to_is_reported_rather_than_thrown() {
        let s = Session::new().unwrap();
        let answer: serde_json::Value = serde_json::from_str(&s.sheet_json(4242)).unwrap();
        assert_eq!(answer["ok"], false);
        assert!(answer["error"].as_str().unwrap().contains("no edge"));
    }

    #[test]
    fn the_buffer_names_its_edges_so_a_client_can_ask() {
        let mut s = Session::new().unwrap();
        s.apply(Command::CreateCube {
            origin: [0.0, 0.0, 0.0],
            size: 1.0,
        })
        .unwrap();
        let view = ViewBuffer::of(s.blocking(), None);

        assert_eq!(view.edge_names.len(), 12);
        let mut names = view.edge_names.clone();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 12, "every edge has a name of its own");
        assert!(names.iter().all(|n| *n != u32::MAX));
    }
}

/// Cutting a sheet, through the same door.
mod cutting {
    use super::*;

    fn cube() -> Session {
        let mut s = Session::new().unwrap();
        s.apply(Command::CreateCube {
            origin: [0.0, 0.0, 0.0],
            size: 1.0,
        })
        .unwrap();
        s
    }

    /// The name of an edge running along this axis.
    fn along(s: &Session, axis: usize) -> u32 {
        let v = ViewBuffer::of(s.blocking(), None);
        for e in 0..v.edge_names.len() {
            let (a, b) = (v.edges[e * 2] as usize * 3, v.edges[e * 2 + 1] as usize * 3);
            let differs: Vec<usize> = (0..3)
                .filter(|k| (v.positions[a + k] - v.positions[b + k]).abs() > 0.1)
                .collect();
            if differs == vec![axis] {
                return v.edge_names[e];
            }
        }
        panic!("no edge along axis {axis}");
    }

    #[test]
    fn cutting_goes_through_the_json_door_like_everything_else() {
        let mut s = cube();
        let edge = along(&s, 2);
        let raw = s.apply_json(&format!(
            r#"{{"op":"split_sheet","edge":{edge},"at":0.25}}"#
        ));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");
        assert!(reply["outcome"]["message"]
            .as_str()
            .unwrap()
            .contains("0.250"));

        let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
        assert_eq!(stats["blocks"], 2);
        assert_eq!(stats["nodes"], 12);
        assert_eq!(stats["edges"], 20);
        assert_eq!(stats["faces"], 11);
    }

    #[test]
    fn a_parameter_outside_the_edge_is_reported_and_changes_nothing() {
        let mut s = cube();
        let edge = along(&s, 2);
        let before = s.view_buffer();
        for at in ["0.0", "1.0", "-0.2", "2.0"] {
            let raw = s.apply_json(&format!(
                r#"{{"op":"split_sheet","edge":{edge},"at":{at}}}"#
            ));
            let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(reply["ok"], false, "{at} should be refused");
            assert!(reply["error"].as_str().unwrap().contains("strictly inside"));
        }
        assert_eq!(s.view_buffer(), before);
    }

    #[test]
    fn the_surface_comes_back_with_both_halves() {
        let mut s = cube();
        let edge = along(&s, 2);
        s.apply(Command::SplitSheet { edge, at: 0.5 }).unwrap();
        let v = ViewBuffer::of(s.blocking(), None);

        // Two blocks, each a closed surface of six.
        assert_eq!(v.quads.len() / 4, 12);
        let mut blocks = v.quad_blocks.clone();
        blocks.sort();
        blocks.dedup();
        assert_eq!(blocks.len(), 2);
        // And one face between them, listed once per block.
        let shared = v
            .quad_faces
            .iter()
            .filter(|n| v.quad_faces.iter().filter(|m| m == n).count() == 2)
            .count();
        assert_eq!(shared, 2);
    }

    #[test]
    fn two_sessions_cut_the_same_way_ship_the_same_bytes() {
        let build = || {
            let mut s = cube();
            let edge = along(&s, 2);
            s.apply(Command::SplitSheet { edge, at: 0.3 }).unwrap();
            s
        };
        assert_eq!(build().view_buffer(), build().view_buffer());
    }
}

/// Pillowing, through the same door.
mod pillowing {
    use super::*;

    fn cube() -> Session {
        let mut s = Session::new().unwrap();
        s.apply(Command::CreateCube {
            origin: [0.0, 0.0, 0.0],
            size: 1.0,
        })
        .unwrap();
        s
    }

    /// The `(face, block)` pair whose centre sits at this value on an axis.
    fn face_at(s: &Session, axis: usize, value: f64) -> (u32, u32) {
        let faces: serde_json::Value = serde_json::from_str(&s.faces_json()).unwrap();
        let found = faces
            .as_array()
            .unwrap()
            .iter()
            .find(|f| (f["centre"][axis].as_f64().unwrap() - value).abs() < 1e-9)
            .expect("a face there");
        (
            found["id"].as_u64().unwrap() as u32,
            found["block"].as_u64().unwrap() as u32,
        )
    }

    #[test]
    fn pillowing_goes_through_the_json_door_like_everything_else() {
        let mut s = cube();
        let (face, block) = face_at(&s, 2, 1.0);
        let raw = s.apply_json(&format!(
            r#"{{"op":"pillow","faces":[{{"face":{face},"block":{block}}}]}}"#
        ));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");
        assert!(reply["outcome"]["message"]
            .as_str()
            .unwrap()
            .contains("1 face"));

        let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
        assert_eq!(stats["blocks"], 2);
        assert_eq!(stats["nodes"], 12);
    }

    #[test]
    fn a_face_inside_the_model_gets_a_layer_too() {
        let mut s = cube();
        let (face, block) = face_at(&s, 2, 1.0);
        s.apply(Command::Pillow {
            faces: vec![gecko_cmd::PickedFace { face, block }],
        })
        .unwrap();

        // The new face between the cube and its layer has a block on each
        // side, and pillowing it puts a layer between those two.
        let faces: serde_json::Value = serde_json::from_str(&s.faces_json()).unwrap();
        let all = faces.as_array().unwrap();
        let inside = all
            .iter()
            .find(|f| all.iter().filter(|o| o["id"] == f["id"]).count() == 2)
            .expect("an internal face");
        let raw = s.apply_json(&format!(
            r#"{{"op":"pillow","faces":[{{"face":{},"block":{}}}]}}"#,
            inside["id"], inside["block"]
        ));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");

        let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
        assert_eq!(stats["blocks"], 3);
        assert_eq!(stats["nodes"], 16);
    }

    #[test]
    fn two_sessions_pillowed_the_same_way_ship_the_same_bytes() {
        let build = || {
            let mut s = cube();
            let (face, block) = face_at(&s, 2, 1.0);
            s.apply(Command::Pillow {
                faces: vec![gecko_cmd::PickedFace { face, block }],
            })
            .unwrap();
            s
        };
        assert_eq!(build().view_buffer(), build().view_buffer());
    }
}

/// Taking a whole sheet of blocks out, through the same door.
mod sheet_removal {
    use super::*;

    /// Three unit slabs stacked along z and glued.
    fn three_slabs() -> Session {
        let mut s = Session::new().unwrap();
        for z in [0.0, 1.0, 2.0] {
            s.apply(Command::CreateCube {
                origin: [0.0, 0.0, z],
                size: 1.0,
            })
            .unwrap();
        }
        for z in [1.0, 2.0] {
            let faces: serde_json::Value = serde_json::from_str(&s.faces_json()).unwrap();
            let pair: Vec<u32> = faces
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| (f["centre"][2].as_f64().unwrap() - z).abs() < 1e-9)
                .map(|f| f["id"].as_u64().unwrap() as u32)
                .collect();
            assert_eq!(pair.len(), 2);
            s.apply(Command::GlueFaces {
                first: pair[0],
                second: pair[1],
            })
            .unwrap();
        }
        s
    }

    /// An edge whose sheet runs between these two heights.
    fn between(s: &Session, low: f64, high: f64) -> u32 {
        let v = ViewBuffer::of(s.blocking(), None);
        for e in 0..v.edge_names.len() {
            let (a, b) = (v.edges[e * 2] as usize * 3, v.edges[e * 2 + 1] as usize * 3);
            let mut z = [
                v.positions[a + 2] as f64 + v.centre[2],
                v.positions[b + 2] as f64 + v.centre[2],
            ];
            z.sort_by(f64::total_cmp);
            if (z[0] - low).abs() < 1e-6 && (z[1] - high).abs() < 1e-6 {
                return v.edge_names[e];
            }
        }
        panic!("no edge from {low} to {high}");
    }

    #[test]
    fn removing_goes_through_the_json_door_like_everything_else() {
        let mut s = three_slabs();
        let edge = between(&s, 1.0, 2.0);
        let raw = s.apply_json(&format!(r#"{{"op":"remove_sheet","edge":{edge}}}"#));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");

        let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
        assert_eq!(stats["blocks"], 2);
        assert_eq!(stats["nodes"], 12);
    }

    #[test]
    fn a_name_no_edge_answers_to_is_reported_and_changes_nothing() {
        let mut s = three_slabs();
        let before = s.view_buffer();
        let raw = s.apply_json(r#"{"op":"remove_sheet","edge":4242}"#);
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], false);
        assert!(reply["error"].as_str().unwrap().contains("no edge"));
        assert_eq!(s.view_buffer(), before);
    }

    #[test]
    fn two_sessions_that_removed_the_same_sheet_ship_the_same_bytes() {
        let build = || {
            let mut s = three_slabs();
            let edge = between(&s, 1.0, 2.0);
            s.apply(Command::RemoveSheet { edge }).unwrap();
            s
        };
        assert_eq!(build().view_buffer(), build().view_buffer());
    }
}
