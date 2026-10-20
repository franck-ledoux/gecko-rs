//! Laying a quadrangle on a geometric surface.

use gecko_cmd::{Command, Session};

/// A unit square with a **fifth** corner half way along its bottom side.
///
/// ```text
///   5 ──14── 4        corners 21..25 on nodes 1..5
///   │        │        curves  11 1→2, 12 2→3, 13 3→4, 14 4→5, 15 5→1
///  15        13       surface 1
///   │        │
///   1 ─11─2─12─ 3
/// ```
///
/// The corner at node 2 is flat: the boundary does not turn there, so it is
/// a point along a side and not a corner of the quadrangle — and the side
/// it lies on is two curves, which is the case a single tag cannot hold.
const FLAT: &str = "\
$MeshFormat\n\
2.2 0 8\n\
$EndMeshFormat\n\
$Nodes\n\
5\n\
1 0 0 0\n\
2 0.5 0 0\n\
3 1 0 0\n\
4 1 1 0\n\
5 0 1 0\n\
$EndNodes\n\
$Elements\n\
13\n\
1 15 2 0 21 1\n\
2 15 2 0 22 2\n\
3 15 2 0 23 3\n\
4 15 2 0 24 4\n\
5 15 2 0 25 5\n\
6 1 2 0 11 1 2\n\
7 1 2 0 12 2 3\n\
8 1 2 0 13 3 4\n\
9 1 2 0 14 4 5\n\
10 1 2 0 15 5 1\n\
11 2 2 0 1 1 2 5\n\
12 2 2 0 1 2 3 5\n\
13 2 2 0 1 3 4 5\n\
$EndElements\n";

fn loaded() -> Session {
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: FLAT.to_string(),
        name: "flat.msh".to_string(),
    })
    .expect("a geometry");
    s
}

fn nodes(s: &Session) -> Vec<serde_json::Value> {
    serde_json::from_str(&s.nodes_json()).unwrap()
}

fn edges(s: &Session) -> Vec<serde_json::Value> {
    serde_json::from_str(&s.edges_json()).unwrap()
}

#[test]
fn a_quadrangle_is_laid_on_the_surface_and_told_what_it_is_on() {
    let mut s = loaded();
    let out = s
        .apply(Command::FaceOnSurface { surface: 1 })
        .expect("a quadrangle");

    let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
    assert_eq!(stats["faces"], 1);
    assert_eq!(stats["nodes"], 4, "four corners, not five");
    assert_eq!(stats["patches"], 1, "and it is a patch, not a block");

    // The corners the boundary turns a right angle at, and not the flat one.
    let mut on_corners: Vec<i64> = nodes(&s)
        .iter()
        .map(|n| n["on"]["tag"].as_i64().unwrap())
        .collect();
    on_corners.sort_unstable();
    assert_eq!(on_corners, [21, 23, 24, 25]);
    for n in nodes(&s) {
        assert_eq!(n["on"]["dim"], 0, "on a corner");
    }

    // The face is on the surface.
    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    assert_eq!(faces[0]["on"], serde_json::json!({ "dim": 2, "tag": 1 }));
    assert!(out.message.contains("surface 1"), "{}", out.message);
}

#[test]
fn the_side_that_runs_along_two_curves_is_one_entity() {
    let mut s = loaded();
    let out = s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();

    let tags: Vec<i64> = edges(&s)
        .iter()
        .map(|e| e["on"]["tag"].as_i64().unwrap())
        .collect();
    // Three plain curves, and one name that is not a curve of the file.
    let mut plain: Vec<i64> = tags.iter().copied().filter(|t| *t > 0).collect();
    plain.sort_unstable();
    assert_eq!(plain, [13, 14, 15]);
    let composite: Vec<i64> = tags.iter().copied().filter(|t| *t < 0).collect();
    assert_eq!(composite.len(), 1, "one side, made of 11 and 12: {tags:?}");
    for e in edges(&s) {
        assert_eq!(e["on"]["dim"], 1, "a side is a curve");
    }
    assert!(
        out.message.contains("one side runs along several curves"),
        "{}",
        out.message
    );
}

#[test]
fn asking_twice_does_not_make_a_second_name_for_one_side() {
    // What makes the operation repeatable, and the journal replayable: the
    // table of composites is keyed by the members, so the same run of
    // curves is the same side however often it is asked for.
    let mut s = loaded();
    s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();
    let first: Vec<i64> = edges(&s)
        .iter()
        .map(|e| e["on"]["tag"].as_i64().unwrap())
        .filter(|t| *t < 0)
        .collect();

    s.apply(Command::Undo).unwrap();
    s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();
    let again: Vec<i64> = edges(&s)
        .iter()
        .map(|e| e["on"]["tag"].as_i64().unwrap())
        .filter(|t| *t < 0)
        .collect();
    assert_eq!(first, again);
}

#[test]
fn a_node_on_a_side_slides_along_every_curve_of_it() {
    // The point of a side being one entity. Cut the quadrangle in two and
    // the new node inherits the side; dragged along the bottom it has to
    // stay on the bottom, whichever of the two curves it is over — a node
    // pinned to curve 11 alone would be dragged back at x = 0.5.
    let mut s = loaded();
    s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();
    let side = edges(&s)
        .into_iter()
        .find(|e| e["on"]["tag"].as_i64().unwrap() < 0)
        .expect("the side");
    let name = side["id"].as_u64().unwrap() as u32;

    s.apply(Command::SplitSheet {
        edge: name,
        at: 0.25,
    })
    .expect("a sheet of one face");

    // The node the cut made, at a quarter along the bottom.
    let cut = nodes(&s)
        .into_iter()
        .find(|n| {
            let p = n["pos"].as_array().unwrap();
            (p[0].as_f64().unwrap() - 0.25).abs() < 1e-9 && p[1].as_f64().unwrap().abs() < 1e-9
        })
        .expect("the new node");
    assert!(
        cut["on"]["tag"].as_i64().unwrap() < 0,
        "it inherited the side: {cut}"
    );

    // Dragged past the junction at x = 0.5 and upwards: it slides along the
    // side and stays on it.
    s.apply(Command::MoveNode {
        node: cut["id"].as_u64().unwrap() as u32,
        to: [0.8, 0.4, 0.0],
        carrying: vec![],
        snap: 0.0,
        smooth: false,
    })
    .expect("a slide");
    let landed = nodes(&s)
        .into_iter()
        .find(|n| n["id"] == cut["id"])
        .expect("still there");
    let p = landed["pos"].as_array().unwrap();
    assert!(
        (p[0].as_f64().unwrap() - 0.8).abs() < 1e-9 && p[1].as_f64().unwrap().abs() < 1e-9,
        "it should be on the second curve of the side, at {p:?}"
    );
}

#[test]
fn what_cannot_be_done_is_said() {
    let mut s = Session::new().unwrap();
    assert!(s.apply(Command::FaceOnSurface { surface: 1 }).is_err());

    let mut s = loaded();
    let raw = s.apply_json(r#"{"op":"face_on_surface","surface":7}"#);
    let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reply["ok"], false);
    assert!(
        reply["error"].as_str().unwrap().contains("no surface"),
        "{raw}"
    );
}

#[test]
fn an_edge_is_never_handed_a_side_it_was_not_put_on() {
    // The other half of the rule. A side is kept when a cell was
    // deliberately put on it, and never given to one that was not: the
    // boundary cannot tell a side from the diagonal beside it, since both
    // run between the same two corners.
    let mut s = loaded();
    s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();
    let before: Vec<u64> = nodes(&s)
        .iter()
        .map(|n| n["id"].as_u64().unwrap())
        .collect();

    // A second quadrangle on the same surface, laid by hand and standing
    // clear of the model, so nothing about it is inherited.
    s.apply(Command::AddFace {
        corners: [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
    })
    .unwrap();
    let fresh: Vec<u32> = nodes(&s)
        .iter()
        .map(|n| n["id"].as_u64().unwrap())
        .filter(|id| !before.contains(id))
        .map(|id| id as u32)
        .collect();
    assert_eq!(fresh.len(), 4);

    // Its corners put on the same geometric corners the first one's are
    // on, which pulls them onto the model: now its bottom edge has exactly
    // the boundary a side has.
    for (node, tag) in fresh.iter().zip([21, 23, 24, 25]) {
        s.apply(Command::ClassifyNode {
            nodes: vec![*node],
            on: Some(gecko_cmd::Spot { dim: 0, tag }),
        })
        .unwrap();
    }

    // The edge between corners 21 and 23 could be the side, or a line
    // across the surface. Nobody said side, so it is the surface.
    let along = edges(&s)
        .into_iter()
        .find(|e| {
            let ends: Vec<u64> = e["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap())
                .collect();
            ends.iter().all(|n| fresh.contains(&(*n as u32))) && e["on"]["dim"].as_u64() == Some(2)
        })
        .expect("an edge of the second face on the surface");
    assert_eq!(along["on"]["tag"], 1, "the surface, not a side: {along}");
    assert!(
        edges(&s)
            .iter()
            .filter(|e| {
                e["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|n| fresh.contains(&(n.as_u64().unwrap() as u32)))
            })
            .all(|e| e["on"]["tag"].as_i64().unwrap_or(0) >= 0),
        "no edge of the second face was handed a side"
    );
}

#[test]
fn a_face_of_a_patch_is_taken_away_on_its_own() {
    let mut s = loaded();
    s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();
    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    let face = faces[0]["id"].as_u64().unwrap();

    // The page asks which components are patches rather than working it
    // out from the shape of something else.
    let patches: Vec<u32> = serde_json::from_str(&s.patches_json()).unwrap();
    assert_eq!(patches.len(), 1);
    assert_eq!(
        patches[0] as u64,
        faces[0]["block"].as_u64().unwrap(),
        "and it is the component that face belongs to"
    );

    let raw = s.apply_json(&format!(r#"{{"op":"remove_face","face":{face}}}"#));
    let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reply["ok"], true, "{raw}");
    let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
    assert_eq!(
        (stats["faces"].as_u64(), stats["nodes"].as_u64()),
        (Some(0), Some(0))
    );
    assert!(s.script().contains(&format!("gecko.remove_face({face})")));
}

#[test]
fn a_side_of_a_block_is_refused_and_nothing_moves() {
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    let before = s.view_buffer();

    let raw = s.apply_json(&format!(
        r#"{{"op":"remove_face","face":{}}}"#,
        faces[0]["id"].as_u64().unwrap()
    ));
    let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reply["ok"], false, "{raw}");
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("belongs to a block"),
        "{raw}"
    );
    assert_eq!(s.view_buffer(), before);
    assert!(serde_json::from_str::<Vec<u32>>(&s.patches_json())
        .unwrap()
        .is_empty());
}

#[test]
fn a_patch_is_raised_into_blocks_all_at_once() {
    let mut s = loaded();
    s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();
    let edge = edges(&s)[0]["id"].as_u64().unwrap() as u32;
    s.apply(Command::SplitSheet { edge, at: 0.5 }).unwrap();
    let face = serde_json::from_str::<Vec<serde_json::Value>>(&s.faces_json()).unwrap()[0]["id"]
        .as_u64()
        .unwrap() as u32;

    let out = s
        .apply(Command::ExtrudePatch { face, height: 0.5 })
        .expect("a patch to raise");
    let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
    assert_eq!(stats["blocks"], 2);
    assert_eq!(stats["patches"], 0, "the sheet is consumed");
    assert_eq!(stats["nodes"], 12, "glued to each other, not sixteen");
    assert!(out.message.contains("2 blocks raised"), "{}", out.message);

    // What the sheet was classified on is on the bottoms of the blocks,
    // which is what makes the blocking follow the geometry afterwards.
    assert_eq!(
        stats["classified"]["faces"], 2,
        "both bottoms carry what the two halves of the sheet were on"
    );
    assert!(stats["classified"]["edges"].as_u64().unwrap() >= 6);
    assert!(s
        .script()
        .contains(&format!("gecko.extrude_patch({face}, 0.5)")));

    // And it can be taken back whole.
    s.apply(Command::Undo).unwrap();
    let stats: serde_json::Value = serde_json::from_str(&s.stats_json()).unwrap();
    assert_eq!(
        (stats["blocks"].as_u64(), stats["patches"].as_u64()),
        (Some(0), Some(1))
    );
}

#[test]
fn a_node_pillowed_along_its_side_keeps_the_side() {
    // Settling after a pillow decides again where the nodes that moved
    // are, and proximity cannot answer "side": a composite is never a
    // candidate, so the nearest thing it can name is one of the curves the
    // side is made of. A node that has slid **along** its side has not
    // moved off anything, and narrowing it to one curve would lose what
    // somebody deliberately put there.
    let mut s = loaded();
    s.apply(Command::FaceOnSurface { surface: 1 }).unwrap();
    let side = edges(&s)
        .into_iter()
        .find(|e| e["on"]["tag"].as_i64().unwrap() < 0)
        .expect("the side");
    s.apply(Command::SplitSheet {
        edge: side["id"].as_u64().unwrap() as u32,
        at: 0.5,
    })
    .unwrap();
    let face = serde_json::from_str::<Vec<serde_json::Value>>(&s.faces_json()).unwrap()[0]["id"]
        .as_u64()
        .unwrap() as u32;
    s.apply(Command::ExtrudePatch { face, height: 0.5 })
        .unwrap();

    // The node half way along the bottom, on the side rather than on
    // either of the curves it is made of.
    let on_the_side = nodes(&s)
        .into_iter()
        .find(|n| {
            let p = n["pos"].as_array().unwrap();
            (p[0].as_f64().unwrap() - 0.5).abs() < 1e-9
                && p[1].as_f64().unwrap().abs() < 1e-9
                && p[2].as_f64().unwrap().abs() < 1e-9
        })
        .expect("the node at the join");
    assert!(
        on_the_side["on"]["tag"].as_i64().unwrap() < 0,
        "it starts on the side: {on_the_side}"
    );

    // The face between the two blocks, pillowed: that draws one block's
    // nodes away from it, and this one slides along the bottom as it goes.
    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    let inside = faces
        .iter()
        .find(|f| faces.iter().filter(|g| g["id"] == f["id"]).count() == 2)
        .expect("a face with a block on each side");
    s.apply(Command::Pillow {
        faces: vec![gecko_cmd::PickedFace {
            face: inside["id"].as_u64().unwrap() as u32,
            block: inside["block"].as_u64().unwrap() as u32,
        }],
        tolerance: gecko_cmd::SNAP_SHARE,
    })
    .expect("a layer");

    // Still on the bottom of the model, and still on the side — not
    // narrowed to whichever of its curves it now happens to be over.
    let slid: Vec<serde_json::Value> = nodes(&s)
        .into_iter()
        .filter(|n| {
            let p = n["pos"].as_array().unwrap();
            p[1].as_f64().unwrap().abs() < 1e-9
                && p[2].as_f64().unwrap().abs() < 1e-9
                && (p[0].as_f64().unwrap() - 0.5).abs() > 1e-9
                && p[0].as_f64().unwrap() > 0.0
                && p[0].as_f64().unwrap() < 1.0
        })
        .collect();
    assert_eq!(slid.len(), 1, "one node slid along the bottom: {slid:?}");
    assert!(
        slid[0]["on"]["tag"].as_i64().unwrap() < 0,
        "it kept its side: {}",
        slid[0]
    );
}
