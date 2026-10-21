//! The door, and the bytes that come out of it.

use gecko_cmd::view::{HEADER, MAGIC};
use gecko_cmd::{Command, Session, SessionError, ViewBuffer, SNAP_SHARE};

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
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 6);
    let n_nodes = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    let n_edges = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    let n_quads = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    assert_eq!((n_nodes, n_edges, n_quads), (8, 12, 6));
    // Exact length, or a client's typed-array views walk off the end.
    // Per node: three f32 of position, one u32 of name, and three more for
    // what it is classified on and whether it is frozen. Per edge: two
    // node indices, a name, and two for its classification. Per
    // quadrangle: four corner indices, the two names it is coloured by,
    // and two for its own classification.
    assert_eq!(
        bytes.len(),
        HEADER + n_nodes * 28 + n_edges * 20 + n_quads * 32
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
            carrying: vec![],
            snap: SNAP_SHARE,
            smooth: false,
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
        carrying: vec![],
        snap: SNAP_SHARE,
        smooth: false,
    })
    .unwrap();

    let after = ViewBuffer::of(s.blocking(), Some(before)).centre;
    assert_eq!(after, before, "the whole scene would have shifted");
}

/// A tetrahedron ten units across: a geometry big enough to hold the
/// session's origin still while the blocking shrinks away from it.
const BIG_TETRA: &str = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
4
1 0 0 0
2 10 0 0
3 0 10 0
4 0 0 10
$EndNodes
$Elements
8
1 15 2 0 21 1
2 1 2 0 11 1 2
3 1 2 0 12 2 3
4 1 2 0 13 3 1
5 2 2 0 1 1 3 2
6 2 2 0 2 1 2 4
7 2 2 0 3 2 3 4
8 2 2 0 4 3 1 4
$EndElements
";

/// The centre a buffer was rebased on, read out of its header: magic,
/// version, then three f64. What the page reads, so what a test should.
fn rebased_on(bytes: &[u8]) -> [f64; 3] {
    [0, 1, 2].map(|k| {
        f64::from_le_bytes(
            bytes[8 + k * 8..16 + k * 8]
                .try_into()
                .expect("eight bytes"),
        )
    })
}

#[test]
fn the_origin_follows_a_model_that_walks_away() {
    // It is kept still for convenience, not forever: the point of rebasing is
    // that f32 coordinates stay small, so an origin the model has left behind
    // has to be abandoned. Asked of the **session**, which is what decides
    // it: the view takes the origin it is given, so that the blocking, the
    // geometry and the mesh are all rebased on one.
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: 1.0,
    })
    .unwrap();
    assert_eq!(rebased_on(&s.view_buffer()), [0.5, 0.5, 0.5]);

    // A nudge keeps it: the model has not gone anywhere worth re-centring
    // for, and re-centring is a jump of everything on screen.
    let name = ViewBuffer::of(s.blocking(), None).names[0];
    s.apply(Command::MoveNode {
        node: name,
        to: [0.3, -0.4, 0.2],
        carrying: vec![],
        snap: SNAP_SHARE,
        smooth: false,
    })
    .unwrap();
    assert_eq!(rebased_on(&s.view_buffer()), [0.5, 0.5, 0.5]);

    // A different continent does not. The cube that was here is taken out,
    // so the model really is over there rather than spread across both.
    s.apply(Command::CreateCube {
        origin: [1.0e6, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    let blocks: Vec<serde_json::Value> = serde_json::from_str(&s.blocks_json()).unwrap();
    let here = blocks
        .iter()
        .find(|b| b["centre"][0].as_f64().unwrap() < 1.0)
        .expect("the cube that was here");
    s.apply(Command::RemoveBlock {
        block: here["id"].as_u64().unwrap() as u32,
    })
    .unwrap();

    let moved = rebased_on(&s.view_buffer());
    assert!(
        moved[0] > 1.0e6 - 1.0,
        "the origin stayed behind at {moved:?}"
    );
}

#[test]
fn the_blocking_the_geometry_and_the_mesh_are_rebased_on_one_origin() {
    // They are drawn on top of each other, so they have to agree on where
    // the middle of the scene is. The view used to settle an origin of its
    // own, over the blocking **alone** — a smaller world than the one the
    // session settles over, and so one that walks past its own threshold
    // first. Take enough blocks out and the view adopted a new centre that
    // the geometry and the mesh never heard about: the mesh stayed where it
    // was and the whole block structure jumped.
    let mut s = Session::new().unwrap();
    for i in 0..10 {
        s.apply(Command::CreateCube {
            origin: [i as f64, 0.0, 0.0],
            size: 1.0,
        })
        .unwrap();
    }
    // A geometry over the lot, which is what pins the session's origin
    // while the blocking shrinks away from it: the session settles over
    // both worlds, the blocking alone is the smaller one.
    s.apply(Command::LoadGeometry {
        msh: BIG_TETRA.to_string(),
        name: "tetra".to_string(),
    })
    .unwrap();
    s.apply(Command::Mesh { segments: 1 }).unwrap();

    let was = rebased_on(&s.view_buffer());
    assert_eq!(was, rebased_on(&s.geometry_buffer()));
    assert_eq!(was, rebased_on(&s.mesh_buffer()));

    // Half the model goes, which moves the blocking's own middle five units
    // — further than half of what is left of it.
    let blocks: Vec<serde_json::Value> = serde_json::from_str(&s.blocks_json()).unwrap();
    let doomed: Vec<u32> = blocks
        .iter()
        .filter(|b| b["centre"][0].as_f64().unwrap() < 5.0)
        .map(|b| b["id"].as_u64().unwrap() as u32)
        .collect();
    assert_eq!(doomed.len(), 5, "half of them");
    for block in doomed {
        s.apply(Command::RemoveBlock { block }).unwrap();
    }
    s.apply(Command::Mesh { segments: 1 }).unwrap();

    let now = rebased_on(&s.view_buffer());
    assert_eq!(
        now,
        rebased_on(&s.geometry_buffer()),
        "the blocking is drawn an origin-shift away from the geometry"
    );
    assert_eq!(
        now,
        rebased_on(&s.mesh_buffer()),
        "the blocking is drawn an origin-shift away from its own mesh"
    );
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
        // And how many blocks it runs through, which is what a removal
        // would take away.
        assert_eq!(answer["blocks"], 1);
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
            tolerance: SNAP_SHARE,
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
                tolerance: SNAP_SHARE,
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

/// Chords: the thread through a face, and the two operations on it.
mod chords {
    use super::*;

    /// Three unit slabs stacked along z and glued, so one chord runs
    /// through all three.
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

    /// The `(face, block)` pair a click on the face at this height gives.
    fn flat_at_z(s: &Session, z: f64) -> (u32, u32) {
        let faces: serde_json::Value = serde_json::from_str(&s.faces_json()).unwrap();
        let found = faces
            .as_array()
            .unwrap()
            .iter()
            .find(|f| {
                (f["centre"][2].as_f64().unwrap() - z).abs() < 1e-9
                    && (f["centre"][0].as_f64().unwrap() - 0.5).abs() < 1e-9
            })
            .cloned()
            .expect("a face there");
        (
            found["id"].as_u64().unwrap() as u32,
            found["block"].as_u64().unwrap() as u32,
        )
    }

    fn blocks(s: &Session) -> u64 {
        serde_json::from_str::<serde_json::Value>(&s.stats_json()).unwrap()["blocks"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn removing_a_chord_goes_through_the_json_door_like_everything_else() {
        let mut s = three_slabs();
        let (face, block) = flat_at_z(&s, 0.0);
        let raw = s.apply_json(&format!(
            r#"{{"op":"remove_chord","face":{face},"block":{block}}}"#
        ));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");
        assert!(
            reply["outcome"]["message"]
                .as_str()
                .unwrap()
                .contains("3 blocks"),
            "{raw}"
        );
        assert_eq!(blocks(&s), 0, "the whole stack was the chord");
    }

    #[test]
    fn an_o_grid_goes_through_the_json_door_like_everything_else() {
        let mut s = three_slabs();
        let (face, block) = flat_at_z(&s, 0.0);
        let raw = s.apply_json(&format!(
            r#"{{"op":"pillow_chord","face":{face},"block":{block}}}"#
        ));
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");
        assert_eq!(blocks(&s), 15, "five where there was one, three times over");

        // The ends of the tube did not move: nothing is past either of them.
        let nodes: Vec<serde_json::Value> = serde_json::from_str(&s.nodes_json()).unwrap();
        let z: Vec<f64> = nodes
            .iter()
            .map(|n| n["pos"][2].as_f64().unwrap())
            .collect();
        assert!(z.iter().all(|&v| v > -1e-9 && v < 3.0 + 1e-9), "{z:?}");
        assert_eq!(z.iter().filter(|v| v.abs() < 1e-9).count(), 8);
    }

    #[test]
    fn both_write_themselves_into_the_script() {
        let mut s = three_slabs();
        let (face, block) = flat_at_z(&s, 0.0);
        s.apply(Command::PillowChord {
            face,
            block,
            tolerance: 0.01,
        })
        .unwrap();
        let script = s.script();
        assert!(
            script.contains(&format!("gecko.pillow_chord({face}, {block},")),
            "{script}"
        );

        let mut other = three_slabs();
        let (face, block) = flat_at_z(&other, 0.0);
        other.apply(Command::RemoveChord { face, block }).unwrap();
        assert!(
            other
                .script()
                .contains(&format!("gecko.remove_chord({face}, {block})")),
            "{}",
            other.script()
        );
    }

    #[test]
    fn an_o_grid_can_be_taken_back() {
        let mut s = three_slabs();
        let before = s.view_buffer();
        let (face, block) = flat_at_z(&s, 0.0);
        s.apply(Command::PillowChord {
            face,
            block,
            tolerance: 0.01,
        })
        .unwrap();
        assert_eq!(blocks(&s), 15);
        s.apply(Command::Undo).unwrap();
        assert_eq!(blocks(&s), 3);
        assert_eq!(s.view_buffer(), before, "byte for byte where it was");
    }

    #[test]
    fn a_name_no_face_answers_to_is_reported_and_changes_nothing() {
        let mut s = three_slabs();
        let before = s.view_buffer();
        for op in ["remove_chord", "pillow_chord"] {
            let raw = s.apply_json(&format!(r#"{{"op":"{op}","face":4242,"block":0}}"#));
            let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(reply["ok"], false, "{raw}");
            assert!(
                reply["error"].as_str().unwrap().contains("no face"),
                "{raw}"
            );
            assert_eq!(s.view_buffer(), before);
        }
    }
}

#[test]
fn a_face_is_grown_into_a_block_through_the_door() {
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();

    // The face on top, named the way a script would have to name it.
    let faces: serde_json::Value = serde_json::from_str(&s.faces_json()).unwrap();
    let top = faces
        .as_array()
        .unwrap()
        .iter()
        .max_by(|a, b| {
            let z = |f: &serde_json::Value| f["centre"][2].as_f64().unwrap();
            z(a).partial_cmp(&z(b)).unwrap()
        })
        .unwrap()
        .clone();

    let reply = s.apply_json(&format!(
        r#"{{"op":"extrude_face","face":{},"block":{},"height":2.0}}"#,
        top["id"].as_u64().unwrap(),
        top["block"].as_u64().unwrap(),
    ));
    assert!(reply.contains(r#""ok":true"#), "{reply}");
    assert!(reply.contains("grown from face"), "{reply}");

    let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
    assert_eq!(stats["blocks"], 2);
    assert_eq!(stats["nodes"], 12);
    assert_eq!(stats["faces"], 11);
}

#[test]
fn a_refused_extrusion_comes_back_as_a_refusal_rather_than_a_panic() {
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock { corners: UNIT_CUBE }).unwrap();
    let reply = s.apply_json(r#"{"op":"extrude_face","face":0,"block":0,"height":0}"#);
    assert!(reply.contains(r#""ok":false"#), "{reply}");
    assert!(reply.contains("positive distance"), "{reply}");
}

#[test]
fn the_session_writes_itself_out_as_lua() {
    // The history, in the one form that can be run again. Not a diary:
    // an undo takes its line back off rather than writing one.
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    let out = s
        .apply(Command::CreateCube {
            origin: [3.0, 0.0, 0.0],
            size: 2.0,
        })
        .unwrap();
    // Every outcome carries its own line, for a console to echo.
    assert_eq!(
        out.script.as_deref(),
        Some("gecko.create_cube{ origin = {3, 0, 0}, size = 2 }")
    );

    s.apply(Command::SplitSheet {
        edge: first_edge(&s),
        at: 0.25,
    })
    .unwrap();

    let script = s.script();
    assert!(script.starts_with("-- Written by gecko"), "{script}");
    let lines: Vec<&str> = script.lines().filter(|l| !l.starts_with("--")).collect();
    assert_eq!(
        lines,
        vec![
            "gecko.create_cube{ origin = {0, 0, 0}, size = 1 }",
            "gecko.create_cube{ origin = {3, 0, 0}, size = 2 }",
            "gecko.split_sheet(0, 0.25)",
        ]
    );

    // An undo takes the last line off, so the script is a recipe for where
    // the session now is rather than for everything anybody tried.
    s.apply(Command::Undo).unwrap();
    assert_eq!(
        s.script().lines().filter(|l| !l.starts_with("--")).count(),
        2
    );
    // And the undo itself leaves no line.
    assert!(!s.script().contains("undo"));
}

#[test]
fn the_journal_does_not_keep_a_copy_of_every_mesh_ever_loaded() {
    // A geometry command carries the whole file, because a browser has no
    // path to send. The journal wants the name.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: SQUARE.to_string(),
        name: "square.msh".to_string(),
    })
    .unwrap();
    assert!(
        s.script().contains(r#"gecko.load_geometry("square.msh")"#),
        "{}",
        s.script()
    );
    assert!(
        !s.script().contains("$MeshFormat"),
        "the file went into the journal"
    );
}

const SQUARE: &str = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
3
1 0 0 0
2 1 0 0
3 0 1 0
$EndNodes
$Elements
1
1 2 2 0 1 1 2 3
$EndElements
";

fn first_edge(s: &Session) -> u32 {
    let edges: serde_json::Value = serde_json::from_str(&s.edges_json()).unwrap();
    edges[0]["id"].as_u64().unwrap() as u32
}

/// Faces that are not part of any block, through the same door.
mod patches {
    use super::*;

    const SQUARE: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    fn stats(s: &Session) -> serde_json::Value {
        serde_json::from_str(&s.stats_json()).unwrap()
    }

    #[test]
    fn a_face_is_added_and_counted_as_a_patch() {
        let mut s = Session::new().unwrap();
        let raw = s.apply_json(r#"{"op":"add_face","corners":[[0,0,0],[1,0,0],[1,1,0],[0,1,0]]}"#);
        let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(reply["ok"], true, "{raw}");

        let seen = stats(&s);
        assert_eq!(seen["faces"], 1);
        assert_eq!(seen["blocks"], 0, "a patch is not a block");
        assert_eq!(seen["patches"], 1);

        // And the faces a script can name say which they are, so nobody has
        // to work it out from the shape of the answer.
        let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0]["patch"], true);
    }

    #[test]
    fn a_block_and_a_face_are_told_apart_everywhere() {
        let mut s = Session::new().unwrap();
        s.apply(Command::CreateCube {
            origin: [0.0, 0.0, 0.0],
            size: 1.0,
        })
        .unwrap();
        s.apply(Command::AddFace { corners: SQUARE }).unwrap();

        let seen = stats(&s);
        assert_eq!(
            (seen["blocks"].as_u64(), seen["patches"].as_u64()),
            (Some(1), Some(1))
        );

        let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
        assert_eq!(faces.len(), 7, "six of the block, and the one on its own");
        assert_eq!(faces.iter().filter(|f| f["patch"] == true).count(), 1);
    }

    #[test]
    fn a_face_is_written_into_the_script_and_taken_back() {
        let mut s = Session::new().unwrap();
        let before = s.view_buffer();
        s.apply(Command::AddFace { corners: SQUARE }).unwrap();

        let script = s.script();
        assert!(
            script.contains("gecko.add_face{{0, 0, 0}, {1, 0, 0}, {1, 1, 0}, {0, 1, 0}}"),
            "{script}"
        );

        s.apply(Command::Undo).unwrap();
        assert_eq!(stats(&s)["faces"], 0);
        assert_eq!(s.view_buffer(), before, "byte for byte where it was");
    }

    #[test]
    fn what_a_patch_cannot_be_asked_is_said_plainly() {
        let mut s = Session::new().unwrap();
        s.apply(Command::AddFace { corners: SQUARE }).unwrap();
        let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
        let (face, block) = (
            faces[0]["id"].as_u64().unwrap(),
            faces[0]["block"].as_u64().unwrap(),
        );
        let before = s.view_buffer();

        for op in ["pillow_chord", "remove_chord"] {
            let raw = s.apply_json(&format!(r#"{{"op":"{op}","face":{face},"block":{block}}}"#));
            let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(reply["ok"], false, "{raw}");
            assert!(reply["error"].as_str().unwrap().contains("patch"), "{raw}");
            assert_eq!(s.view_buffer(), before, "and nothing moved");
        }
    }
}
