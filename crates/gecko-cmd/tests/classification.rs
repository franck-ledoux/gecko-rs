//! Putting the blocking on the geometry.

use gecko_cmd::{Command, Session, Spot, SNAP_SHARE};
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

    // And every face on a surface, from its four edges rather than from
    // any measurement of its own.
    let faces: Vec<_> = serde_json::from_str::<Vec<serde_json::Value>>(&s.faces_json()).unwrap();
    let surfaces: std::collections::BTreeSet<i64> = faces
        .iter()
        .map(|f| {
            assert_eq!(f["on"]["dim"], 2, "{f}");
            f["on"]["tag"].as_i64().unwrap()
        })
        .collect();
    assert_eq!(surfaces, (1..7).collect());
}

#[test]
fn a_face_goes_where_its_four_edges_agree_and_nowhere_else() {
    // One dimension up, the same rule and the same reason. A face is
    // never pinned below its own dimension either: four edges all on one
    // curve would otherwise put the face on that curve.
    let mut s = on_the_box();
    s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    assert_eq!(classified(&s)["faces"], 6);

    // Take one node off the model and everything **stays**: the three
    // edges at that corner each still have an end on the model that
    // agrees with them, and the three faces still have edges that do.
    // What a cell was told stands until something contradicts it.
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::ClassifyNode {
        nodes: vec![node],
        on: None,
    })
    .unwrap();
    assert_eq!(classified(&s)["edges"], 12, "the edges at that corner stay");
    assert_eq!(classified(&s)["faces"], 6, "and so do the faces");

    // Take the whole bottom off the model and the face under it goes: no
    // edge of it says anything any more, so there is nothing left holding
    // it to the surface.
    let bottom: Vec<u32> = nodes(&s)
        .into_iter()
        .filter(|n| n["pos"][2].as_f64().unwrap().abs() < 1e-9)
        .map(|n| n["id"].as_u64().unwrap() as u32)
        .collect();
    assert_eq!(bottom.len(), 4, "the four nodes of the bottom");
    s.apply(Command::ClassifyNode {
        nodes: bottom,
        on: None,
    })
    .unwrap();
    assert_eq!(
        classified(&s)["faces"],
        5,
        "the bottom face lost its footing"
    );
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
            nodes: vec![*node],
            on: Some(Spot { dim: 0, tag }),
        })
        .expect("put on a corner");
    }
    let found = edges(&s)
        .into_iter()
        .find(|e| e["id"] == edge["id"])
        .expect("the edge");
    assert_eq!(found["on"], serde_json::json!({ "dim": 2, "tag": 1 }));

    // Take one end off the model and the edge **keeps** the face: the end
    // that is still classified is on that face, so nothing has
    // contradicted what the edge was told. It used to be cleared here,
    // and a session bled classification because of it — a pillow lets go
    // of four nodes and every edge that touches one is wiped though none
    // of them moved.
    s.apply(Command::ClassifyNode {
        nodes: vec![ends[0]],
        on: None,
    })
    .unwrap();
    let found = |s: &Session| {
        edges(s)
            .into_iter()
            .find(|e| e["id"] == edge["id"])
            .expect("the edge")
    };
    assert_eq!(found(&s)["on"], serde_json::json!({ "dim": 2, "tag": 1 }));

    // Both ends off, and it goes: an edge with nothing classified at
    // either end is the inside of the model — drawn in by a pillow or
    // raised by an extrusion — and keeping what it used to follow is what
    // tears a mesh out through the surface.
    s.apply(Command::ClassifyNode {
        nodes: vec![ends[1]],
        on: None,
    })
    .unwrap();
    assert!(found(&s)["on"].is_null(), "{}", found(&s));

    // And nothing is **handed** to an edge from one end alone. Put that
    // one end back on its corner: the edge had nothing before, and a
    // single end is not enough to give it something — the diagonal of a
    // face has one end on a curve too.
    s.apply(Command::ClassifyNode {
        nodes: vec![ends[1]],
        on: Some(Spot { dim: 0, tag: 103 }),
    })
    .unwrap();
    assert!(found(&s)["on"].is_null(), "{}", found(&s));
}

#[test]
fn a_node_on_a_surface_slides_on_it_and_does_not_come_off() {
    let mut s = on_the_box();
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::ClassifyNode {
        nodes: vec![node],
        on: Some(Spot { dim: 2, tag: 1 }),
    })
    .unwrap();

    // Asked for a point well above the face: it lands on the face below it.
    s.apply(Command::MoveNode {
        node,
        to: [0.4, 0.6, 5.0],
        carrying: vec![],
        snap: SNAP_SHARE,
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
        carrying: vec![],
        snap: SNAP_SHARE,
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
        nodes: vec![node],
        on: Some(Spot { dim: 0, tag: 107 }),
    })
    .unwrap();
    s.apply(Command::MoveNode {
        node,
        to: [9.0, 9.0, 9.0],
        carrying: vec![],
        snap: SNAP_SHARE,
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
        carrying: vec![],
        snap: SNAP_SHARE,
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
        carrying: vec![],
        snap: SNAP_SHARE,
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
    s.apply(Command::FreezeNode {
        nodes: vec![node],
        frozen: true,
    })
    .unwrap();
    assert_eq!(classified(&s)["frozen"], 1);

    // Not moved...
    let refused = s
        .apply(Command::MoveNode {
            node,
            to: [5.0, 5.0, 5.0],
            carrying: vec![],
            snap: SNAP_SHARE,
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
        nodes: vec![node],
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
    s.apply(Command::FreezeNode {
        nodes: vec![node],
        frozen: true,
    })
    .unwrap();

    let v = gecko_cmd::ViewBuffer::of(s.blocking(), None);
    assert_eq!(v.node_class_dim.len(), 8);
    assert!(v.node_class_dim.iter().all(|d| *d == 0));
    assert_eq!(v.node_frozen.iter().sum::<u32>(), 1);
    assert_eq!(v.edge_class_dim.len(), 12);
    assert!(v.edge_class_dim.iter().all(|d| *d == 1));
    assert!(v.edge_class_tag.iter().all(|t| (201..213).contains(t)));
    assert_eq!(v.quad_class_dim.len(), 6);
    assert!(v.quad_class_dim.iter().all(|d| *d == 2));

    // Exact length, or a client's typed-array views walk off the end. Per
    // node three more words than before the classification travelled and
    // two more again for its groups, per edge two and two, per quadrangle
    // two and two.
    let bytes = s.view_buffer();
    assert_eq!(
        bytes.len(),
        gecko_cmd::view::HEADER + 8 * 36 + 12 * 28 + 6 * 40
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
        nodes: vec![node],
        on: Some(Spot { dim: 1, tag: 205 }),
    })
    .unwrap();
    assert_eq!(
        s.blocking().node_class(gecko_core::CellId::from_raw(node)),
        Some(On { dim: 1, tag: 205 })
    );
}

/// A square whose bottom side **bends**: the curve 201 runs from (0,0,0)
/// up to (0.5,0.25,0) and back down to (1,0,0), and the surface is the
/// polygon above it. A straight edge between the two corners is nowhere
/// near the curve in the middle, which is what makes a projection
/// visible rather than a no-op.
const BENT: &str = "\
$MeshFormat\n\
2.2 0 8\n\
$EndMeshFormat\n\
$Nodes\n\
5\n\
1 0 0 0\n\
2 1 0 0\n\
3 1 1 0\n\
4 0 1 0\n\
5 0.5 0.25 0\n\
$EndNodes\n\
$Elements\n\
7\n\
1 15 2 0 101 1\n\
2 15 2 0 102 2\n\
3 1 2 0 201 1 5\n\
4 1 2 0 201 5 2\n\
5 2 2 0 1 1 5 4\n\
6 2 2 0 1 5 2 3\n\
7 2 2 0 1 5 3 4\n\
$EndElements\n";

/// A unit cube with its bottom-front edge classified on the bent curve,
/// and the name of that edge.
fn on_the_bend() -> (Session, u32) {
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: BENT.to_string(),
        name: "bent.msh".to_string(),
    })
    .expect("a geometry");
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .expect("a cube");

    // The two ends of the bottom-front edge, onto the two corners the
    // curve runs between. The edge follows from them.
    for (want, tag) in [([0.0, 0.0, 0.0], 101), ([1.0, 0.0, 0.0], 102)] {
        let node = nodes(&s)
            .into_iter()
            .find(|n| {
                let p = n["pos"].as_array().unwrap();
                (0..3).all(|k| (p[k].as_f64().unwrap() - want[k]).abs() < 1e-9)
            })
            .expect("a corner of the cube");
        s.apply(Command::ClassifyNode {
            nodes: vec![node["id"].as_u64().unwrap() as u32],
            on: Some(Spot { dim: 0, tag }),
        })
        .unwrap();
    }

    let edge = edges(&s)
        .into_iter()
        .find(|e| e["on"] == serde_json::json!({ "dim": 1, "tag": 201 }))
        .expect("an edge on the bent curve");
    let name = edge["id"].as_u64().unwrap() as u32;
    (s, name)
}

#[test]
fn cutting_an_edge_leaves_a_node_on_what_the_edge_was_on() {
    // What a cut of a classified edge has to produce: a node and two
    // edges, all three on what the whole was on — and the node **on** it,
    // not merely labelled with it.
    let (mut s, edge) = on_the_bend();
    let before = nodes(&s).len();

    s.apply(Command::SplitSheet { edge, at: 0.5 }).expect("cut");

    // The node that came out of that edge: on the curve, and pulled onto
    // it. The straight midpoint is (0.5, 0, 0) and the curve passes
    // nowhere near it.
    let on_curve: Vec<_> = nodes(&s)
        .into_iter()
        .filter(|n| n["on"] == serde_json::json!({ "dim": 1, "tag": 201 }))
        .collect();
    assert_eq!(on_curve.len(), 1, "one new node, on the curve");
    let p = on_curve[0]["pos"].as_array().unwrap();
    let y = p[1].as_f64().unwrap();
    assert!(y > 0.15, "the node was left on the chord at y = {y}");
    // And it really is on the curve: the bend runs between y = 0 and
    // y = 0.25, and the nearest point to the chord's middle is y = 0.2.
    assert!((y - 0.2).abs() < 1e-9, "{p:?}");

    // The two halves the edge became are on the curve as well.
    let halves: Vec<_> = edges(&s)
        .into_iter()
        .filter(|e| e["on"] == serde_json::json!({ "dim": 1, "tag": 201 }))
        .collect();
    assert_eq!(halves.len(), 2, "two halves, both on the curve");
    assert!(nodes(&s).len() > before, "the cut made nodes");
}

#[test]
fn the_other_nodes_a_cut_makes_are_taken_only_where_the_model_is() {
    // The sheet through that edge crosses three other edges, none of them
    // classified. Two of those crossings are at the top of the cube, a
    // whole edge away from anything: nothing about the cut says where
    // they belong, and they are left where it put them.
    //
    // The third is on the bottom, which is **lying on** the surface. That
    // one is taken by it — a node made among classified neighbours is
    // judged by where it is, which is the rule that stops a session
    // bleeding classification. Nothing moves: it was already there.
    let (mut s, edge) = on_the_bend();
    s.apply(Command::SplitSheet { edge, at: 0.5 }).unwrap();

    let made: Vec<_> = nodes(&s)
        .into_iter()
        .filter(|n| (n["pos"][0].as_f64().unwrap() - 0.5).abs() < 1e-9)
        .collect();
    assert_eq!(made.len(), 3, "three crossings, besides the cut edge's own");

    let loose: Vec<_> = made.iter().filter(|n| n["on"].is_null()).collect();
    assert_eq!(loose.len(), 2, "the two at the top are on nothing");
    for n in &loose {
        assert_eq!(n["pos"][2].as_f64().unwrap(), 1.0, "{}", n["pos"]);
    }

    let taken: Vec<_> = made.iter().filter(|n| !n["on"].is_null()).collect();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0]["on"], serde_json::json!({ "dim": 2, "tag": 1 }));
    assert_eq!(
        taken[0]["pos"][2].as_f64().unwrap(),
        0.0,
        "on the surface, unmoved"
    );
}

#[test]
fn cutting_a_face_leaves_an_edge_and_two_faces_on_what_the_face_was_on() {
    // The other half of the same rule, one dimension up. It is not
    // written anywhere: the new edge's two ends are the two nodes the cut
    // made, which carry the curves they came from, and two curves of one
    // surface put the edge between them on that surface — which is where
    // the face was. The halves then follow from their four edges.
    let mut s = on_the_box();
    s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    assert_eq!(classified(&s)["faces"], 6);

    let edge = edges(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::SplitSheet { edge, at: 0.5 }).expect("cut");

    // Two blocks now, so eleven faces: the ten on the outside and the one
    // between them.
    let found = serde_json::from_str::<Vec<serde_json::Value>>(&s.faces_json()).unwrap();
    let outside: Vec<_> = found.iter().filter(|f| !f["on"].is_null()).collect();
    assert_eq!(classified(&s)["faces"], 10, "every face of the outside");
    assert!(outside.iter().all(|f| f["on"]["dim"] == 2), "{outside:?}");

    // And the one inside is on nothing, having four edges that share no
    // surface. Saying so is the point: a face in the middle of the model
    // follows nothing.
    // `faces_json` lists a face once per block, so the one in the middle
    // appears twice: ten outside and two entries for the one inside.
    assert_eq!(found.len(), 12);
    assert_eq!(found.iter().filter(|f| f["on"].is_null()).count(), 2);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&s.stats_json()).unwrap()["faces"],
        11
    );

    // The four nodes the cut made are each on the curve their edge was on.
    let on_curves = nodes(&s).iter().filter(|n| n["on"]["dim"] == 1).count();
    assert_eq!(on_curves, 4);
    // Twenty edges: the eight halves and the four new ones round the cut
    // are all on the model, and the four round the inside face are the
    // new ones — on surfaces, not curves.
    assert_eq!(classified(&s)["edges"], 20, "every edge of the outside");
}

/// A lid and the tube it closes: one curve, 201, bounding **both**.
///
/// The shape the cylinder has and the box does not. A cell with its
/// whole boundary on that curve could be on the lid or on the tube, and
/// nothing about the boundary says which. The lid is surface **2** and
/// the tube surface 1 on purpose: the lower tag is the wrong answer, so
/// a rule that reaches for it is caught here.
const LID: &str = "\
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
14\n\
1 1 2 0 201 1 2\n\
2 1 2 0 201 2 3\n\
3 1 2 0 201 3 4\n\
4 1 2 0 201 4 1\n\
5 2 2 0 1 1 2 6\n\
6 2 2 0 1 1 6 5\n\
7 2 2 0 1 2 3 7\n\
8 2 2 0 1 2 7 6\n\
9 2 2 0 1 3 4 8\n\
10 2 2 0 1 3 8 7\n\
11 2 2 0 1 4 1 5\n\
12 2 2 0 1 4 5 8\n\
13 2 2 0 2 1 2 3\n\
14 2 2 0 2 1 3 4\n\
$EndElements\n";

/// A cube sitting in the tube, its bottom four corners on the rim.
fn under_the_lid() -> Session {
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: LID.to_string(),
        name: "lid.msh".to_string(),
    })
    .expect("a geometry");
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .expect("a cube");
    for node in nodes(&s) {
        let p = node["pos"].as_array().unwrap();
        if p[2].as_f64().unwrap().abs() > 1e-9 {
            continue;
        }
        s.apply(Command::ClassifyNode {
            nodes: vec![node["id"].as_u64().unwrap() as u32],
            on: Some(Spot { dim: 1, tag: 201 }),
        })
        .unwrap();
    }
    s
}

/// The face whose centre is nearest a point, and what it is on.
fn face_at(session: &Session, at: [f64; 3]) -> serde_json::Value {
    serde_json::from_str::<Vec<serde_json::Value>>(&session.faces_json())
        .unwrap()
        .into_iter()
        .min_by(|a, b| {
            let far = |f: &serde_json::Value| {
                let c = f["centre"].as_array().unwrap();
                (0..3)
                    .map(|k| (c[k].as_f64().unwrap() - at[k]).powi(2))
                    .sum::<f64>()
            };
            far(a).partial_cmp(&far(b)).unwrap()
        })
        .expect("a face")
}

#[test]
fn a_cell_bounded_by_one_curve_lands_on_the_surface_it_is_nearest() {
    // The boundary cannot tell the lid from the tube: all four edges are
    // on the rim, and the rim bounds both. Where the face **is** can, and
    // is the only thing that can.
    let s = under_the_lid();
    let lid = face_at(&s, [0.5, 0.5, 0.0]);
    assert_eq!(
        lid["on"],
        serde_json::json!({ "dim": 2, "tag": 2 }),
        "the bottom face should be on the lid, not the tube"
    );
}

#[test]
fn a_cut_across_a_surface_stays_on_that_surface() {
    // The bug this whole arrangement is here for. Cut the lid in half and
    // the new edge runs across the **inside** of it — but both its ends
    // are on the rim, so asking its boundary would put it on the rim.
    // Cut again, through the sheet that edge is part of, and the node in
    // the middle of the lid would be flung out to the rim with it.
    let mut s = under_the_lid();
    let edge = edges(&s)
        .into_iter()
        .find(|e| e["on"] == serde_json::json!({ "dim": 1, "tag": 201 }))
        .expect("an edge on the rim");
    s.apply(Command::SplitSheet {
        edge: edge["id"].as_u64().unwrap() as u32,
        at: 0.5,
    })
    .expect("the first cut");

    // The edge the cut laid across the lid is on the lid.
    let across = edges(&s)
        .into_iter()
        .find(|e| e["on"] == serde_json::json!({ "dim": 2, "tag": 2 }))
        .expect("an edge across the lid");

    s.apply(Command::SplitSheet {
        edge: across["id"].as_u64().unwrap() as u32,
        at: 0.5,
    })
    .expect("the second cut");

    // A node in the middle of the lid, on the lid, and still in the
    // middle of it.
    let middle: Vec<_> = nodes(&s)
        .into_iter()
        .filter(|n| n["on"] == serde_json::json!({ "dim": 2, "tag": 2 }))
        .collect();
    assert_eq!(middle.len(), 1, "one node inside the lid: {middle:?}");
    let p = middle[0]["pos"].as_array().unwrap();
    for (k, want) in [0.5, 0.5, 0.0].iter().enumerate() {
        assert!(
            (p[k].as_f64().unwrap() - want).abs() < 1e-9,
            "the node was pulled off the lid, to {p:?}"
        );
    }
}

#[test]
fn how_near_a_dropped_node_has_to_come_is_the_gesture_s_own() {
    // The reach is on the command, not on the session: dropping a node near
    // a corner is a different act from dragging one past it, and the thing
    // holding the mouse is what knows which it was.
    let mut s = on_the_box();
    let node = nodes(&s)[0]["id"].as_u64().unwrap() as u32;

    // Within a fiftieth of the model of the corner at the origin, and
    // refused the geometry entirely.
    s.apply(Command::MoveNode {
        node,
        to: [0.01, 0.01, 0.01],
        carrying: vec![],
        snap: 0.0,
    })
    .unwrap();
    assert!(
        nodes(&s).into_iter().find(|n| n["id"] == node).unwrap()["on"].is_null(),
        "nothing should be in reach of nothing",
    );

    // The same drop, with the usual reach, is taken by that corner.
    s.apply(Command::MoveNode {
        node,
        to: [0.01, 0.01, 0.01],
        carrying: vec![],
        snap: SNAP_SHARE,
    })
    .unwrap();
    let landed = nodes(&s).into_iter().find(|n| n["id"] == node).unwrap();
    assert_eq!(landed["on"]["dim"], 0, "{landed}");
    assert_eq!(
        landed["pos"],
        serde_json::json!([0.0, 0.0, 0.0]),
        "{landed}"
    );
}

#[test]
fn four_corners_go_onto_one_curve_in_one_command() {
    // The gesture the interface makes: pick the corners, pick the curve,
    // say so once. One command rather than four, so it is one step of undo
    // and one settling of the edges above them.
    let mut s = on_the_box();
    let bottom: Vec<u32> = nodes(&s)
        .iter()
        .filter(|n| n["pos"][2].as_f64().unwrap().abs() < 1e-9)
        .map(|n| n["id"].as_u64().unwrap() as u32)
        .collect();
    assert_eq!(bottom.len(), 4, "the four corners at z = 0");

    // Curve 201 is one side of the bottom ring, so all four cannot be on
    // it at once — which is exactly the point: this says where they go, it
    // does not ask where they would like to be.
    let out = s
        .apply(Command::ClassifyNode {
            nodes: bottom.clone(),
            on: Some(Spot { dim: 1, tag: 201 }),
        })
        .unwrap();
    assert!(out.message.contains("curve 201"), "{}", out.message);
    assert_eq!(classified(&s)["nodes"], 4);

    // And each of them was projected onto it, which is the half of this
    // that moves anything.
    for id in &bottom {
        let node = nodes(&s)
            .into_iter()
            .find(|n| n["id"] == *id)
            .expect("the node");
        assert_eq!(node["on"]["tag"], 201, "{node}");
        // Curve 201 runs from (0,0,0) to (1,0,0): y and z are nought on it.
        assert!(node["pos"][1].as_f64().unwrap().abs() < 1e-9, "{node}");
        assert!(node["pos"][2].as_f64().unwrap().abs() < 1e-9, "{node}");
    }

    // One step back puts all four where they were.
    s.apply(Command::Undo).unwrap();
    assert_eq!(classified(&s)["nodes"], 0);
}

#[test]
fn a_name_no_node_answers_to_leaves_the_others_alone() {
    // Half a gesture carried out is worse than none of it: the undo would
    // put back a state nobody asked for either.
    let mut s = on_the_box();
    let first = nodes(&s)[0]["id"].as_u64().unwrap() as u32;
    assert!(s
        .apply(Command::ClassifyNode {
            nodes: vec![first, 9999],
            on: Some(Spot { dim: 0, tag: 101 }),
        })
        .is_err());
    assert_eq!(classified(&s)["nodes"], 0, "the first one went on anyway");

    // And nothing at all is a refusal too, rather than a step of undo that
    // changed nothing.
    assert!(s
        .apply(Command::FreezeNode {
            nodes: vec![],
            frozen: true,
        })
        .is_err());
}

#[test]
fn a_drag_carries_the_other_picked_nodes_with_it() {
    // What a drag does when several handles are picked: one follows the
    // pointer, the rest keep station with it.
    let mut s = on_the_box();
    let bottom: Vec<u32> = nodes(&s)
        .iter()
        .filter(|n| n["pos"][2].as_f64().unwrap().abs() < 1e-9)
        .map(|n| n["id"].as_u64().unwrap() as u32)
        .collect();
    let before: Vec<[f64; 3]> = at(&s, &bottom);

    // The first of them is dragged a unit up; the others come with it.
    let out = s
        .apply(Command::MoveNode {
            node: bottom[0],
            to: [before[0][0], before[0][1], before[0][2] + 1.0],
            carrying: bottom.clone(),
            snap: 0.0,
        })
        .unwrap();
    assert_eq!(out.message, "4 nodes moved", "{}", out.message);

    for (was, now) in before.iter().zip(at(&s, &bottom)) {
        assert!((now[2] - was[2] - 1.0).abs() < 1e-9, "{was:?} -> {now:?}");
        assert!((now[0] - was[0]).abs() < 1e-9 && (now[1] - was[1]).abs() < 1e-9);
    }

    // One step back, because it was one command.
    s.apply(Command::Undo).unwrap();
    assert_eq!(at(&s, &bottom), before);
}

#[test]
fn a_frozen_node_stays_where_it_is_while_the_others_go() {
    // Not moving is what frozen means, and the other nodes have nothing to
    // do with it: the gesture is not refused, the frozen one is left.
    let mut s = on_the_box();
    let all: Vec<u32> = nodes(&s)
        .iter()
        .map(|n| n["id"].as_u64().unwrap() as u32)
        .collect();
    s.apply(Command::FreezeNode {
        nodes: vec![all[1]],
        frozen: true,
    })
    .unwrap();
    let before = at(&s, &all);

    let out = s
        .apply(Command::MoveNode {
            node: all[0],
            to: [before[0][0] + 1.0, before[0][1], before[0][2]],
            carrying: all.clone(),
            snap: 0.0,
        })
        .unwrap();
    assert_eq!(
        out.message, "7 nodes moved, 1 frozen and left",
        "{}",
        out.message
    );
    let now = at(&s, &all);
    assert_eq!(now[1], before[1], "the frozen one moved");
    assert!((now[0][0] - before[0][0] - 1.0).abs() < 1e-9);
}

#[test]
fn what_each_carried_node_does_with_the_offset_is_its_own_business() {
    // The offset is the one asked for, read before anything moves. A node
    // on a curve slides along that curve and so lands short of it — and
    // the others must not then move by *that* shortfall.
    let mut s = on_the_box();
    let bottom: Vec<u32> = nodes(&s)
        .iter()
        .filter(|n| n["pos"][2].as_f64().unwrap().abs() < 1e-9)
        .map(|n| n["id"].as_u64().unwrap() as u32)
        .collect();
    // Curve 201 runs along y = z = 0; the node at the origin is put on it.
    s.apply(Command::ClassifyNode {
        nodes: vec![bottom[0]],
        on: Some(Spot { dim: 1, tag: 201 }),
    })
    .unwrap();
    let before = at(&s, &bottom);

    s.apply(Command::MoveNode {
        node: bottom[0],
        to: [before[0][0] + 0.5, before[0][1], before[0][2] + 1.0],
        carrying: bottom.clone(),
        snap: 0.0,
    })
    .unwrap();

    let now = at(&s, &bottom);
    // The dragged one slid along its curve: across, not up.
    assert!((now[0][2]).abs() < 1e-9, "it left the curve: {:?}", now[0]);
    // The others took the whole offset, the vertical part included.
    for i in 1..bottom.len() {
        assert!(
            (now[i][2] - before[i][2] - 1.0).abs() < 1e-9,
            "{:?} -> {:?}",
            before[i],
            now[i]
        );
    }
}

/// Where these nodes are, in the order given.
fn at(session: &Session, names: &[u32]) -> Vec<[f64; 3]> {
    let all = nodes(session);
    names
        .iter()
        .map(|id| {
            let node = all
                .iter()
                .find(|n| n["id"].as_u64() == Some(u64::from(*id)))
                .expect("the node");
            let p = node["pos"].as_array().unwrap();
            [0, 1, 2].map(|k| p[k].as_f64().unwrap())
        })
        .collect()
}

/// Classify every node at this height onto `on`, and say how many.
fn classify_at_z(session: &mut Session, z: f64, on: Spot) -> usize {
    let at: Vec<u32> = nodes(session)
        .into_iter()
        .filter(|n| (n["pos"][2].as_f64().unwrap() - z).abs() < 1e-9)
        .map(|n| n["id"].as_u64().unwrap() as u32)
        .collect();
    session
        .apply(Command::ClassifyNode {
            nodes: at.clone(),
            on: Some(on),
        })
        .expect("classified");
    at.len()
}

#[test]
fn a_weld_between_two_surfaces_lands_on_the_curve_they_share() {
    // The whole chain, from a command down: gluing a node on the tube to a
    // node on the lid cannot leave it on either — the one point that is on
    // both is on the rim. Neither block knew about the rim; the geometry
    // did, and the glue asked it.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: LID.to_string(),
        name: "lid.msh".to_string(),
    })
    .unwrap();
    // One cube in the tube, resting its floor on the tube's wall.
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    assert_eq!(classify_at_z(&mut s, 0.0, Spot { dim: 2, tag: 1 }), 4);

    // And one below it, held against the lid — which pulls its ceiling up
    // to z = 0, where the other block's floor already is. Classified while
    // it is still down at −0.5, so which four nodes are meant is never in
    // question.
    s.apply(Command::AddBlock {
        corners: gecko_core::cube_corners([0.0, 0.0, -1.5], 1.0),
    })
    .unwrap();
    assert_eq!(classify_at_z(&mut s, -0.5, Spot { dim: 2, tag: 2 }), 4);

    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    let flat: Vec<u32> = faces
        .iter()
        .filter(|f| f["centre"][2].as_f64().unwrap().abs() < 1e-9)
        .map(|f| f["id"].as_u64().unwrap() as u32)
        .collect();
    assert_eq!(flat.len(), 2, "one face from each block, both at z = 0");

    let message = s
        .apply(Command::GlueFaces {
            first: flat[0],
            second: flat[1],
        })
        .expect("two free faces")
        .message;

    let welded: Vec<serde_json::Value> = nodes(&s)
        .into_iter()
        .filter(|n| n["pos"][2].as_f64().unwrap().abs() < 1e-9)
        .collect();
    assert_eq!(welded.len(), 4, "eight nodes became four: {message}");
    for n in &welded {
        assert_eq!(
            n["on"],
            serde_json::json!({ "dim": 1, "tag": 201 }),
            "on the rim, not on either surface — {message}"
        );
    }
    assert!(!message.contains("left"), "nothing was dropped: {message}");
}

#[test]
fn a_weld_between_two_strangers_says_which_classification_it_dropped() {
    // The bottom of the box and its top share nothing at all: no curve, no
    // corner. One of the two claims has to go, and which one went is said
    // out loud rather than left to be discovered in a mesh.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: BOX.to_string(),
        name: "box.msh".to_string(),
    })
    .unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    s.apply(Command::AddBlock {
        corners: gecko_core::cube_corners([0.0, 0.0, -1.0], 1.0),
    })
    .unwrap();

    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    let flat: Vec<u32> = faces
        .iter()
        .filter(|f| f["centre"][2].as_f64().unwrap().abs() < 1e-9)
        .map(|f| f["id"].as_u64().unwrap() as u32)
        .collect();
    assert_eq!(flat.len(), 2);

    // The upper block's floor on surface 1, the lower block's ceiling on
    // surface 2 — which pulls it up to z = 1, a long way from where it was.
    let first_block_floor: Vec<u32> = nodes(&s)
        .into_iter()
        .filter(|n| n["pos"][2].as_f64().unwrap().abs() < 1e-9)
        .map(|n| n["id"].as_u64().unwrap() as u32)
        .collect();
    s.apply(Command::ClassifyNode {
        nodes: first_block_floor[..4].to_vec(),
        on: Some(Spot { dim: 2, tag: 1 }),
    })
    .unwrap();
    s.apply(Command::ClassifyNode {
        nodes: first_block_floor[4..].to_vec(),
        on: Some(Spot { dim: 2, tag: 2 }),
    })
    .unwrap();

    let message = s
        .apply(Command::GlueFaces {
            first: flat[0],
            second: flat[1],
        })
        .expect("two free faces")
        .message;

    assert!(
        message.contains("kept surface 1 and left surface 2")
            || message.contains("kept surface 2 and left surface 1"),
        "{message}"
    );
    assert_eq!(
        message.matches("; node").count(),
        4,
        "once per corner: {message}"
    );
}

#[test]
fn a_block_somebody_just_made_is_not_pinned_behind_their_back() {
    // The limit of the rule above. A node an operation makes **among
    // nodes that are on the model** is judged by where it is; a block
    // somebody has just created is not, however exactly it sits on the
    // geometry. Dropping a cube on a model is not a statement that the
    // cube follows it — that is what `classify` is for, and it is one
    // keystroke.
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: BOX.to_string(),
        name: "box.msh".to_string(),
    })
    .unwrap();
    // Exactly the model's own unit cube: every corner on a corner, every
    // edge on a curve, every face on a surface. Nothing is nearer than
    // this, and still nothing is claimed.
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    assert_eq!(classified(&s)["nodes"], 0, "nothing is claimed");
    assert_eq!(classified(&s)["edges"], 0);

    // And the box round a geometry is the same: a block, made whole.
    let mut s = on_the_box();
    assert_eq!(classified(&s)["nodes"], 0, "nor by boxing it");
    // One word, and it is all on the model.
    s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    assert_eq!(classified(&s)["nodes"], 8);
}

#[test]
fn the_reach_a_new_node_is_judged_by_is_the_one_classify_was_given() {
    // Somebody working at a tenth means a tenth. The session keeps the
    // tolerance of the last `classify` it was given and judges the nodes
    // an operation makes by that, rather than by a number in the kernel
    // that nobody chose.
    //
    // Both runs below are the same model, the same block and the same
    // cut. One corner is taken off the geometry by hand and pulled a
    // tenth of the way out, so the node the cut leaves half way along
    // that edge stands a twentieth clear of the surface — outside the
    // kernel's own two percent and inside a tenth. The only difference
    // between the two is the number given to `classify` at the start.
    let caught = |tolerance: f64| {
        let mut s = on_the_box();
        s.apply(Command::Classify { tolerance }).unwrap();
        assert_eq!(classified(&s)["nodes"], 8, "the box is on the model");

        // **Both** ends of one edge off the model and moved out, so that
        // the edge itself is on nothing and the node the cut leaves in it
        // has nothing to inherit — proximity is the only thing that can
        // speak for it. `snap: 0` so that the moves claim nothing either.
        let corner = |want: [f64; 3], s: &Session| -> u32 {
            nodes(s)
                .into_iter()
                .find(|n| {
                    let p = n["pos"].as_array().unwrap();
                    (0..3).all(|k| (p[k].as_f64().unwrap() - want[k]).abs() < 1e-9)
                })
                .expect("a corner of the box")["id"]
                .as_u64()
                .unwrap() as u32
        };
        let (a, b) = (corner([0.0; 3], &s), corner([0.0, 0.0, 1.0], &s));
        s.apply(Command::ClassifyNode {
            nodes: vec![a, b],
            on: None,
        })
        .unwrap();
        for (name, z) in [(a, 0.0), (b, 1.0)] {
            s.apply(Command::MoveNode {
                node: name,
                to: [-0.1, 0.0, z],
                carrying: vec![],
                snap: 0.0,
            })
            .unwrap();
        }

        // The edge between those two, cut in the middle: the node it
        // leaves there stands a tenth clear of the model.
        let edge = edges(&s)
            .into_iter()
            .find(|e| {
                let ends: Vec<u32> = e["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_u64().unwrap() as u32)
                    .collect();
                ends.contains(&a) && ends.contains(&b)
            })
            .expect("the edge between them");
        let before = classified(&s)["nodes"].as_u64().unwrap();
        s.apply(Command::SplitSheet {
            edge: edge["id"].as_u64().unwrap() as u32,
            at: 0.5,
        })
        .unwrap();
        classified(&s)["nodes"].as_u64().unwrap() - before
    };

    // Two percent: the node half way along that edge is out of reach, and
    // the only new nodes claimed are the ones the cut's own edges handed
    // their classification to.
    let tight = caught(0.02);
    // A tenth: that one is within reach as well.
    let loose = caught(0.1);
    assert!(
        loose > tight,
        "a tenth caught {loose} and two percent {tight}"
    );
}
