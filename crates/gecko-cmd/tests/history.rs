//! Going back.
//!
//! The oracle throughout is the view buffer: the bytes a client draws. If
//! those are the same, the model is the same as far as anything outside the
//! kernel can tell — positions, names, classification and all — and that is
//! exactly the claim an undo makes.

use gecko_cmd::{Command, Session, HISTORY_STEPS, MAX_HISTORY_STEPS};

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

/// A tetrahedron's surface: one tagged corner, three curves, four surfaces.
/// The smallest geometry a box can be put round — a flat one has no
/// thickness to box.
const TETRA: &str = "\
$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
4
1 0 0 0
2 1 0 0
3 0 1 0
4 0 0 1
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

fn cube(at: f64) -> Command {
    Command::AddBlock {
        corners: UNIT_CUBE.map(|[x, y, z]| [x + at, y, z]),
    }
}

/// Everything a client is told about the **model**, in one value.
///
/// Without the history itself, which of course differs either side of an
/// undo: the claim being tested is about the block structure, not about how
/// many steps are left in the stack.
fn seen(s: &Session) -> (Vec<u8>, String, String, String) {
    let mut stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
    stats.as_object_mut().unwrap().remove("history");
    (
        s.view_buffer(),
        stats.to_string(),
        s.nodes_json(),
        s.edges_json(),
    )
}

#[test]
fn going_back_puts_the_bytes_back() {
    let mut s = Session::new().unwrap();
    s.apply(cube(0.0)).unwrap();
    let before = seen(&s);

    s.apply(cube(2.0)).unwrap();
    assert_ne!(seen(&s).0, before.0, "the second block changed nothing");

    let out = s.apply(Command::Undo).unwrap();
    assert!(out.view_changed);
    assert!(out.message.starts_with("undone"), "{}", out.message);
    assert_eq!(seen(&s), before);
}

#[test]
fn a_cut_can_be_taken_back_and_made_again() {
    // The hard case for anything that rebuilds rather than restores: a cut
    // renames nothing but creates a great deal, and the edge that was cut
    // has to still be the edge that was cut.
    let mut s = Session::new().unwrap();
    s.apply(cube(0.0)).unwrap();
    let before = seen(&s);

    let edge = first_edge(&s);
    s.apply(Command::SplitSheet { edge, at: 0.5 }).unwrap();
    let after = seen(&s);

    s.apply(Command::Undo).unwrap();
    assert_eq!(seen(&s), before);

    // The same command again, and the same model again — down to the names,
    // which is what a replay could not promise.
    s.apply(Command::SplitSheet { edge, at: 0.5 }).unwrap();
    assert_eq!(seen(&s), after);
}

#[test]
fn five_steps_is_the_default_and_the_sixth_is_the_bottom() {
    let mut s = Session::new().unwrap();
    let first = seen(&s);
    for i in 0..HISTORY_STEPS + 3 {
        s.apply(cube(2.0 * i as f64)).unwrap();
    }
    for _ in 0..HISTORY_STEPS {
        let out = s.apply(Command::Undo).unwrap();
        assert!(out.message.starts_with("undone"), "{}", out.message);
    }

    let out = s.apply(Command::Undo).unwrap();
    assert_eq!(out.message, "nothing to undo");
    assert!(!out.view_changed, "nothing to undo is nothing to redraw");
    // Three blocks deep, not back to nothing: five steps is five steps.
    assert_ne!(seen(&s).0, first.0);
}

#[test]
fn how_far_back_it_goes_is_a_setting() {
    let mut s = Session::new().unwrap();
    let out = s.apply(Command::SetHistory { steps: 1 }).unwrap();
    assert_eq!(out.message, "one step of undo");
    assert!(!out.view_changed, "a setting is not a change to the model");

    s.apply(cube(0.0)).unwrap();
    let one = seen(&s);
    s.apply(cube(2.0)).unwrap();

    s.apply(Command::Undo).unwrap();
    assert_eq!(seen(&s), one);
    assert_eq!(s.apply(Command::Undo).unwrap().message, "nothing to undo");
}

#[test]
fn none_at_all_is_a_setting_too() {
    let mut s = Session::new().unwrap();
    assert_eq!(
        s.apply(Command::SetHistory { steps: 0 }).unwrap().message,
        "undo is off"
    );
    s.apply(cube(0.0)).unwrap();
    assert_eq!(s.apply(Command::Undo).unwrap().message, "nothing to undo");
}

#[test]
fn shortening_the_history_drops_the_oldest_at_once() {
    let mut s = Session::new().unwrap();
    for i in 0..4 {
        s.apply(cube(2.0 * i as f64)).unwrap();
    }
    assert_eq!(s.history(), (HISTORY_STEPS, 4));

    s.apply(Command::SetHistory { steps: 2 }).unwrap();
    assert_eq!(s.history(), (2, 2), "the oldest states are still held");

    // And the two that are left are the two most recent ones.
    s.apply(Command::Undo).unwrap();
    s.apply(Command::Undo).unwrap();
    assert_eq!(s.blocking().nb_blocks(), 2);
    assert_eq!(s.apply(Command::Undo).unwrap().message, "nothing to undo");
}

#[test]
fn asking_for_too_many_gets_as_many_as_there_are() {
    let mut s = Session::new().unwrap();
    s.apply(Command::SetHistory { steps: 1_000_000 }).unwrap();
    assert_eq!(s.history().0, MAX_HISTORY_STEPS);
}

#[test]
fn a_command_that_did_nothing_is_not_a_step_back() {
    let mut s = Session::new().unwrap();
    s.apply(cube(0.0)).unwrap();
    let before = seen(&s);

    // Refused: a cube has no size. Nothing changed, so there is nothing to
    // go back past — the next undo has to reach the block, not this.
    assert!(s
        .apply(Command::CreateCube {
            origin: [0.0; 3],
            size: 0.0,
        })
        .is_err());
    assert_eq!(seen(&s), before);
    assert_eq!(s.history().1, 1, "a refusal left a state behind");

    s.apply(Command::Undo).unwrap();
    assert_eq!(s.blocking().nb_blocks(), 0);
}

#[test]
fn a_setting_is_not_a_step_back_either() {
    let mut s = Session::new().unwrap();
    s.apply(cube(0.0)).unwrap();
    s.apply(Command::SetHistory { steps: 5 }).unwrap();
    assert_eq!(s.history().1, 1);
    s.apply(Command::Undo).unwrap();
    assert_eq!(s.blocking().nb_blocks(), 0);
}

#[test]
fn going_back_twice_goes_back_two() {
    // An undo that remembered what it undid would be a toggle.
    let mut s = Session::new().unwrap();
    let empty = seen(&s);
    s.apply(cube(0.0)).unwrap();
    s.apply(cube(2.0)).unwrap();
    s.apply(Command::Undo).unwrap();
    s.apply(Command::Undo).unwrap();
    assert_eq!(seen(&s), empty);
}

#[test]
fn the_geometry_comes_back_with_the_blocks() {
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: TETRA.to_string(),
        name: "square".to_string(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.1,
    })
    .unwrap();

    s.apply(Command::Undo).unwrap();
    assert_eq!(s.blocking().nb_blocks(), 0);
    assert!(s.geometry().is_some(), "the box took the geometry with it");

    s.apply(Command::Undo).unwrap();
    assert!(s.geometry().is_none(), "the load was not taken back");
}

#[test]
fn the_geometry_generation_never_goes_backwards() {
    // A client re-fetches the geometry when the number differs from the one
    // it drew. Falling back to a number it has already drawn would leave it
    // drawing the geometry it no longer has.
    let mut s = Session::new().unwrap();
    let before = generation(&s);
    s.apply(Command::LoadGeometry {
        msh: TETRA.to_string(),
        name: String::new(),
    })
    .unwrap();
    let loaded = generation(&s);
    assert!(loaded > before);

    s.apply(Command::Undo).unwrap();
    assert!(s.geometry().is_none());
    assert!(generation(&s) > loaded, "the number fell back");
}

#[test]
fn going_back_does_not_decide_the_classification_again() {
    // The state was settled when it was taken. Settling it again is a
    // chance for it to come out differently, and an undo that lands
    // somewhere new is not an undo.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: TETRA.to_string(),
        name: String::new(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.0,
    })
    .unwrap();
    s.apply(Command::Classify { tolerance: 0.5 }).unwrap();
    let classified = seen(&s);

    let edge = first_edge(&s);
    s.apply(Command::SplitSheet { edge, at: 0.5 }).unwrap();
    s.apply(Command::Undo).unwrap();
    assert_eq!(seen(&s), classified);
}

fn generation(s: &Session) -> u64 {
    let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
    stats["geometry"]["generation"].as_u64().unwrap()
}

fn first_edge(s: &Session) -> u32 {
    let edges: serde_json::Value = serde_json::from_str(&s.edges_json()).unwrap();
    edges[0]["id"].as_u64().unwrap() as u32
}

/// The session's own census, as JSON.
fn stats(s: &Session) -> serde_json::Value {
    serde_json::from_str(&s.stats_json()).expect("the stats")
}

#[test]
fn a_reset_empties_the_session_and_can_be_taken_back() {
    // What a replayed script needs in front of it: a script describes a
    // session from nothing, so run on top of a model it would describe
    // something else.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: TETRA.to_string(),
        name: "tetra".to_string(),
    })
    .unwrap();
    s.apply(cube(0.0)).unwrap();
    s.apply(cube(2.0)).unwrap();
    let generation = stats(&s)["geometry"]["generation"].as_u64().unwrap();
    assert_eq!(stats(&s)["blocks"], 2);

    let out = s.apply(Command::Reset).expect("a reset");
    assert!(out.message.contains("empty"), "{}", out.message);
    assert_eq!(stats(&s)["blocks"], 0);
    assert_eq!(stats(&s)["nodes"], 0);
    assert!(
        stats(&s)["geometry"]["surfaces"].is_null(),
        "the geometry went too: {}",
        stats(&s)
    );
    // The number went up, so a client holding a picture of the old
    // geometry fetches again rather than going on drawing it.
    assert!(stats(&s)["geometry"]["generation"].as_u64().unwrap() > generation);

    // One step of undo, like any other change.
    s.apply(Command::Undo).expect("an undo");
    assert_eq!(stats(&s)["blocks"], 2);
    assert_eq!(stats(&s)["geometry"]["surfaces"], 4);

    // And the journal keeps it, because it happened. Replaying the script
    // arrives in the same place: the reset in the middle of it wipes
    // exactly what the lines above it built.
    s.apply(Command::Reset).unwrap();
    assert!(s.script().contains("gecko.reset()"), "{}", s.script());
    assert!(s.script().contains("gecko.add_block"), "{}", s.script());
}
