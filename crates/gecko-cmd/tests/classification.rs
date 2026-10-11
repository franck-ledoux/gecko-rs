//! Putting the blocking on the geometry.

use gecko_cmd::{Command, Session, Spot};
use gecko_core::On;

/// The unit cube's boundary, every entity of it named: eight corners, twelve
/// curves, six surfaces. Written out rather than generated, because what
/// each tag is is the whole subject of the tests below.
///
/// ```text
///   corners 101..108 on nodes 1..8        surfaces 1 z=0   2 z=1
///   curves  201..204 the bottom ring                3 y=0   4 x=1
///           205..208 the top ring                   5 y=1   6 x=0
///           209..212 the four uprights
/// ```
const BOX: &str = "\
$MeshFormat\n\
2.2 0 8\n\
$EndMeshFormat\n\
$Nodes\n\
8\n\
1 0 0 0\n\
2 1 0 0\n\
3 1 1 0\n\
4 0 1 0\n\
5 0 0 1\n\
6 1 0 1\n\
7 1 1 1\n\
8 0 1 1\n\
$EndNodes\n\
$Elements\n\
32\n\
1 15 2 0 101 1\n\
2 15 2 0 102 2\n\
3 15 2 0 103 3\n\
4 15 2 0 104 4\n\
5 15 2 0 105 5\n\
6 15 2 0 106 6\n\
7 15 2 0 107 7\n\
8 15 2 0 108 8\n\
9 1 2 0 201 1 2\n\
10 1 2 0 202 2 3\n\
11 1 2 0 203 3 4\n\
12 1 2 0 204 4 1\n\
13 1 2 0 205 5 6\n\
14 1 2 0 206 6 7\n\
15 1 2 0 207 7 8\n\
16 1 2 0 208 8 5\n\
17 1 2 0 209 1 5\n\
18 1 2 0 210 2 6\n\
19 1 2 0 211 3 7\n\
20 1 2 0 212 4 8\n\
21 2 2 0 1 1 2 3\n\
22 2 2 0 1 1 3 4\n\
23 2 2 0 2 5 6 7\n\
24 2 2 0 2 5 7 8\n\
25 2 2 0 3 1 2 6\n\
26 2 2 0 3 1 6 5\n\
27 2 2 0 4 2 3 7\n\
28 2 2 0 4 2 7 6\n\
29 2 2 0 5 3 4 8\n\
30 2 2 0 5 3 8 7\n\
31 2 2 0 6 4 1 5\n\
32 2 2 0 6 4 5 8\n\
$EndElements\n\
";

/// A session holding the box geometry and one block resting exactly on it.
fn on_the_box() -> Session {
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: BOX.to_string(),
        name: "box.msh".to_string(),
    })
    .expect("a geometry");
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.0,
    })
    .expect("a block");
    s
}

fn nodes(session: &Session) -> Vec<serde_json::Value> {
    serde_json::from_str::<Vec<_>>(&session.nodes_json()).unwrap()
}

fn edges(session: &Session) -> Vec<serde_json::Value> {
    serde_json::from_str::<Vec<_>>(&session.edges_json()).unwrap()
}

fn classified(session: &Session) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(&session.stats_json()).unwrap()["classified"].clone()
}

#[test]
fn a_block_resting_on_the_geometry_lands_on_its_corners_and_curves() {
    let mut s = on_the_box();
    assert_eq!(classified(&s)["nodes"], 0, "nothing is classified yet");

    let out = s
        .apply(Command::Classify { tolerance: 0.02 })
        .expect("classified");
    assert!(out.message.contains("8 nodes"), "{}", out.message);
    assert!(out.message.contains("12 edges"), "{}", out.message);

    // Every corner of the block is on a corner of the model — the *lowest*
    // dimension with anything in range, not the curves or surfaces that
    // also pass within a whisker of it.
    let on: Vec<_> = nodes(&s).iter().map(|n| n["on"].clone()).collect();
    assert!(on.iter().all(|o| o["dim"] == 0), "{on:?}");
    let tags: std::collections::BTreeSet<i64> =
        on.iter().map(|o| o["tag"].as_i64().unwrap()).collect();
    assert_eq!(tags, (101..109).collect(), "each corner, once");

    // And every edge on a curve, inferred from its two ends rather than
    // measured: the twelve of the model, once each.
    let curves: std::collections::BTreeSet<i64> = edges(&s)
        .iter()
        .map(|e| {
            assert_eq!(e["on"]["dim"], 1, "{e}");
            e["on"]["tag"].as_i64().unwrap()
        })
        .collect();
    assert_eq!(curves, (201..213).collect());
}

#[test]
fn a_box_standing_clear_of_the_geometry_is_on_nothing() {
    // The default margin puts every corner a tenth of the model away, which
    // is well outside the default tolerance. Saying "on nothing" is the
    // right answer: the alternative is a cell claiming to lie on a surface
    // it is nowhere near.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: BOX.to_string(),
        name: String::new(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.1,
    })
    .unwrap();

    s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    assert_eq!(classified(&s)["nodes"], 0);
    assert_eq!(classified(&s)["edges"], 0);

    // Loosen it until the corners are in range and they all land.
    s.apply(Command::Classify { tolerance: 0.2 }).unwrap();
    assert_eq!(classified(&s)["nodes"], 8);
    assert_eq!(classified(&s)["edges"], 12);
}

#[test]
fn classifying_pulls_a_node_onto_what_it_was_classified_on() {
    // Not merely labelled: the point of saying a node is on a corner is
    // that it then **is** on that corner.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: BOX.to_string(),
        name: String::new(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.1,
    })
    .unwrap();
    s.apply(Command::Classify { tolerance: 0.3 }).unwrap();

    let corners: Vec<[f64; 3]> = nodes(&s)
        .iter()
        .map(|n| {
            let p = n["pos"].as_array().unwrap();
            [0, 1, 2].map(|k| p[k].as_f64().unwrap())
        })
        .collect();
    for p in &corners {
        for k in 0..3 {
            assert!(
                p[k].abs() < 1e-9 || (p[k] - 1.0).abs() < 1e-9,
                "a corner of the box landed at {p:?}"
            );
        }
    }
}

#[test]
fn an_edge_goes_where_its_two_ends_agree_and_nowhere_else() {
    // The rule proximity cannot get right. Two corners of one face that
    // share no curve share the face, so the edge between them is on the
    // face — and an edge with an unclassified end is on nothing, because
    // there is nothing to infer from.
    let mut s = on_the_box();
    let edge = edges(&s)[0].clone();
    let ends: Vec<u32> = edge["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_u64().unwrap() as u32)
        .collect();

    // Corners 101 and 103 are opposite ends of the bottom face's diagonal:
    // no curve holds both, surface 1 holds both.
    for (node, tag) in ends.iter().zip([101, 103]) {
        s.apply(Command::ClassifyNode {
            node: *node,
            on: Some(Spot { dim: 0, tag }),
        })
        .expect("put on a corner");
    }
    let found = edges(&s)
        .into_iter()
        .find(|e| e["id"] == edge["id"])
        .expect("the edge");
    assert_eq!(found["on"], serde_json::json!({ "dim": 2, "tag": 1 }));

    // Take one end off the model and the edge has nothing to stand on.
    s.apply(Command::ClassifyNode {
        node: ends[0],
        on: None,
    })
    .unwrap();
    let found = edges(&s)
        .into_iter()
        .find(|e| e["id"] == edge["id"])
        .expect("the edge");
    assert!(found["on"].is_null(), "{found}");
}

#[test]
fn a_node_on_a_surface_slides_on_it_and_does_not_come_off() {
    let mut s = on_the_box();
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::ClassifyNode {
        node,
        on: Some(Spot { dim: 2, tag: 1 }),
    })
    .unwrap();

    // Asked for a point well above the face: it lands on the face below it.
    s.apply(Command::MoveNode {
        node,
        to: [0.4, 0.6, 5.0],
    })
    .expect("moved");
    let at = nodes(&s)
        .into_iter()
        .find(|n| n["id"] == node)
        .map(|n| {
            let p = n["pos"].as_array().unwrap().clone();
            [0, 1, 2].map(|k| p[k].as_f64().unwrap())
        })
        .unwrap();
    assert!(
        (at[0] - 0.4).abs() < 1e-9 && (at[1] - 0.6).abs() < 1e-9,
        "{at:?}"
    );
    assert!(at[2].abs() < 1e-9, "it left the surface: {at:?}");

    // And dragging it miles away does not free it — coming off is
    // something you say, not something that happens to you.
    s.apply(Command::MoveNode {
        node,
        to: [50.0, 50.0, 50.0],
    })
    .unwrap();
    assert_eq!(
        nodes(&s).into_iter().find(|n| n["id"] == node).unwrap()["on"],
        serde_json::json!({ "dim": 2, "tag": 1 })
    );
}

#[test]
fn a_node_on_a_corner_cannot_move_at_all() {
    // Its entity is a single point, so every projection lands there.
    let mut s = on_the_box();
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::ClassifyNode {
        node,
        on: Some(Spot { dim: 0, tag: 107 }),
    })
    .unwrap();
    s.apply(Command::MoveNode {
        node,
        to: [9.0, 9.0, 9.0],
    })
    .unwrap();
    let at = nodes(&s)
        .into_iter()
        .find(|n| n["id"] == node)
        .map(|n| {
            let p = n["pos"].as_array().unwrap().clone();
            [0, 1, 2].map(|k| p[k].as_f64().unwrap())
        })
        .unwrap();
    assert_eq!(at, [1.0, 1.0, 1.0], "corner 107 is node 7");
}

#[test]
fn an_unclassified_node_dropped_near_the_model_takes_hold_of_it() {
    // The other half of the pair: a global pass puts everything on the
    // model at once, and this is what makes dragging one corner onto it
    // the way to do it by hand.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: BOX.to_string(),
        name: String::new(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.5,
    })
    .unwrap();
    assert_eq!(classified(&s)["nodes"], 0);

    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::MoveNode {
        node,
        to: [0.5, 0.5, 0.001],
    })
    .unwrap();
    let found = nodes(&s).into_iter().find(|n| n["id"] == node).unwrap();
    assert_eq!(found["on"], serde_json::json!({ "dim": 2, "tag": 1 }));
    // Pulled onto it, not merely labelled.
    assert!(found["pos"][2].as_f64().unwrap().abs() < 1e-9, "{found}");

    // Dropped far from everything, it stays free and stays where it was put.
    let other = nodes(&s)[1]["id"].as_u64().unwrap() as u32;
    s.apply(Command::MoveNode {
        node: other,
        to: [40.0, 40.0, 40.0],
    })
    .unwrap();
    let found = nodes(&s).into_iter().find(|n| n["id"] == other).unwrap();
    assert!(found["on"].is_null(), "{found}");
    assert_eq!(found["pos"], serde_json::json!([40.0, 40.0, 40.0]));
}

#[test]
fn a_frozen_node_is_left_out_of_everything() {
    let mut s = on_the_box();
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    let before = nodes(&s)[0]["pos"].clone();
    s.apply(Command::FreezeNode { node, frozen: true }).unwrap();
    assert_eq!(classified(&s)["frozen"], 1);

    // Not moved...
    let refused = s
        .apply(Command::MoveNode {
            node,
            to: [5.0, 5.0, 5.0],
        })
        .expect_err("frozen");
    assert!(refused.to_string().contains("frozen"), "{refused}");

    // ...and not classified either, since classifying is followed by a
    // projection and a projection is a move.
    s.apply(Command::Classify { tolerance: 0.5 }).unwrap();
    let found = nodes(&s).into_iter().find(|n| n["id"] == node).unwrap();
    assert!(found["on"].is_null(), "{found}");
    assert_eq!(found["pos"], before);
    assert_eq!(classified(&s)["nodes"], 7, "the other seven went on");

    // Thawed, it joins in.
    s.apply(Command::FreezeNode {
        node,
        frozen: false,
    })
    .unwrap();
    s.apply(Command::Classify { tolerance: 0.5 }).unwrap();
    assert_eq!(classified(&s)["nodes"], 8);
}

#[test]
fn classifying_needs_a_geometry_to_classify_onto() {
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: 1.0,
    })
    .unwrap();
    let refused = s
        .apply(Command::Classify { tolerance: 0.02 })
        .expect_err("no geometry");
    assert!(refused.to_string().contains("no geometry"), "{refused}");
}

#[test]
fn the_classification_travels_in_the_view_buffer() {
    // Three decoders read these bytes; the colour of a classified node is
    // the first thing that would quietly stop matching the kernel.
    let mut s = on_the_box();
    s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::FreezeNode { node, frozen: true }).unwrap();

    let v = gecko_cmd::ViewBuffer::of(s.blocking(), None);
    assert_eq!(v.node_class_dim.len(), 8);
    assert!(v.node_class_dim.iter().all(|d| *d == 0));
    assert_eq!(v.node_frozen.iter().sum::<u32>(), 1);
    assert_eq!(v.edge_class_dim.len(), 12);
    assert!(v.edge_class_dim.iter().all(|d| *d == 1));
    assert!(v.edge_class_tag.iter().all(|t| (201..213).contains(t)));

    // Exact length, or a client's typed-array views walk off the end. Per
    // node three more words than before, per edge two.
    let bytes = s.view_buffer();
    assert_eq!(
        bytes.len(),
        gecko_cmd::view::HEADER + 8 * 28 + 12 * 20 + 6 * 24
    );
}

#[test]
fn what_a_cell_is_on_survives_being_named() {
    // `assign_missing_ids` writes a fresh attribute for every unnamed
    // cell, and an edge that came out of a split is unnamed while still
    // lying exactly where the whole did.
    let mut s = on_the_box();
    s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    let before: Vec<_> = edges(&s).iter().map(|e| e["on"].clone()).collect();
    assert!(before.iter().all(|o| !o.is_null()));

    // Something that names cells without touching these ones.
    s.apply(Command::CreateCube {
        origin: [9.0, 9.0, 9.0],
        size: 1.0,
    })
    .unwrap();
    let still: Vec<_> = edges(&s)
        .iter()
        .filter(|e| !e["on"].is_null())
        .map(|e| e["on"].clone())
        .collect();
    assert_eq!(still.len(), 12, "a naming pass unclassified something");

    // And the new block's own cells are on nothing, being nowhere near.
    assert_eq!(classified(&s)["nodes"], 8);
}

#[test]
fn the_same_names_mean_the_same_thing_in_the_kernel() {
    // `On` is what the block structure stores, `Spot` what a command
    // carries, and the two are different types in different crates on
    // purpose. This is the one place they have to agree.
    let mut s = on_the_box();
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::ClassifyNode {
        node,
        on: Some(Spot { dim: 1, tag: 205 }),
    })
    .unwrap();
    assert_eq!(
        s.blocking().node_class(gecko_core::CellId::from_raw(node)),
        Some(On { dim: 1, tag: 205 })
    );
}
