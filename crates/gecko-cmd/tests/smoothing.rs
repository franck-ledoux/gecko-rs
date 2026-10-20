//! Smoothing, and the ripple a drag makes.

use gecko_cmd::{Command, Session};

/// `n` cubes each way, every touching pair glued — through the same door
/// anybody else would use.
fn grid(n: i32) -> Session {
    let mut s = Session::new().unwrap();
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                s.apply(Command::CreateCube {
                    origin: [i as f64, j as f64, k as f64],
                    size: 1.0,
                })
                .expect("a cube");
            }
        }
    }
    // Every pair of faces sharing a centre, read once: gluing one pair
    // leaves the others' names alone.
    let faces: Vec<serde_json::Value> = serde_json::from_str(&s.faces_json()).unwrap();
    let mut seen: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
    let mut pairs = Vec::new();
    for face in &faces {
        let at = face["centre"].as_array().unwrap();
        let key = [0, 1, 2].map(|k| (at[k].as_f64().unwrap() * 1e4).round() as i64);
        let id = face["id"].as_u64().unwrap() as u32;
        match seen.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => pairs.push((*e.get(), id)),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(id);
            }
        }
    }
    for (first, second) in pairs {
        s.apply(Command::GlueFaces { first, second })
            .expect("a glue");
    }
    s
}

fn nodes(s: &Session) -> Vec<serde_json::Value> {
    serde_json::from_str(&s.nodes_json()).unwrap()
}

/// The node nearest a point, and where it is.
fn node_at(s: &Session, p: [f64; 3]) -> (u32, [f64; 3]) {
    let found = nodes(s)
        .into_iter()
        .min_by(|a, b| {
            let far = |n: &serde_json::Value| {
                let q = n["pos"].as_array().unwrap();
                (0..3)
                    .map(|k| (q[k].as_f64().unwrap() - p[k]).powi(2))
                    .sum::<f64>()
            };
            far(a).total_cmp(&far(b))
        })
        .expect("a node");
    let at = found["pos"].as_array().unwrap();
    (
        found["id"].as_u64().unwrap() as u32,
        [0, 1, 2].map(|k| at[k].as_f64().unwrap()),
    )
}

fn at(s: &Session, id: u32) -> [f64; 3] {
    let found = nodes(s)
        .into_iter()
        .find(|n| n["id"].as_u64() == Some(id as u64))
        .expect("the node is there");
    let p = found["pos"].as_array().unwrap();
    [0, 1, 2].map(|k| p[k].as_f64().unwrap())
}

#[test]
fn smoothing_goes_through_the_json_door_like_everything_else() {
    let mut s = grid(2);
    let (middle, was) = node_at(&s, [1.0, 1.0, 1.0]);
    s.apply(Command::MoveNode {
        node: middle,
        to: [1.4, 1.3, 1.2],
        carrying: vec![],
        snap: 0.0,
        smooth: false,
    })
    .unwrap();

    let raw = s.apply_json(r#"{"op":"smooth","rounds":4,"share":1.0}"#);
    let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reply["ok"], true, "{raw}");
    assert!(
        reply["outcome"]["message"]
            .as_str()
            .unwrap()
            .contains("smoothed 4 times"),
        "{raw}"
    );

    let back = at(&s, middle);
    for k in 0..3 {
        assert!((back[k] - was[k]).abs() < 1e-9, "it came back to {back:?}");
    }

    // And it is one step of undo, like every other command.
    s.apply(Command::Undo).unwrap();
    assert_eq!(at(&s, middle), [1.4, 1.3, 1.2]);
}

#[test]
fn a_blocking_with_nowhere_to_move_says_so_rather_than_nothing() {
    // One cube: every node of it is on the boundary and follows nothing.
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0, 0.0, 0.0],
        size: 1.0,
    })
    .unwrap();
    let before = s.view_buffer();

    let out = s
        .apply(Command::Smooth {
            rounds: 5,
            share: 1.0,
        })
        .expect("a smoothing");
    assert!(out.message.contains("nothing moved"), "{}", out.message);
    assert!(out.message.contains("boundary"), "{}", out.message);
    assert_eq!(s.view_buffer(), before);
}

#[test]
fn a_drag_ripples_outwards_and_stops_when_a_ring_barely_moves() {
    // Three each way, so there are nodes inside the model for the ripple
    // to reach: the ring round the one being dragged, then the ring round
    // those, and so on until one of them hardly moves.
    let mut s = grid(3);
    let (middle, _) = node_at(&s, [1.0, 1.0, 1.0]);
    let (neighbour, was) = node_at(&s, [2.0, 1.0, 1.0]);

    let out = s
        .apply(Command::MoveNode {
            node: middle,
            to: [1.5, 1.4, 1.3],
            carrying: vec![],
            snap: 0.0,
            smooth: true,
        })
        .expect("a drag");

    // The node that was dragged is **where it was put**: the smoothing is
    // not allowed to undo the gesture that asked for it.
    assert_eq!(at(&s, middle), [1.5, 1.4, 1.3]);

    // Its neighbour gave way, which is the whole point.
    let now = at(&s, neighbour);
    assert!(
        (0..3).any(|k| (now[k] - was[k]).abs() > 1e-9),
        "the ring round it did not move: {now:?}"
    );

    // And it stopped: the message says how many rings, and it is neither
    // none nor all eight it is allowed.
    let said = out.message;
    assert!(said.contains("ring"), "{said}");
    let rings: u32 = said
        .split_whitespace()
        .zip(said.split_whitespace().skip(1))
        .find(|(_, word)| word.starts_with("ring"))
        .map(|(count, _)| count.parse().expect("a number"))
        .expect("the rings are counted");
    assert!(
        (1..=4).contains(&rings),
        "it should have settled within a few rings, not after {rings}"
    );

    // And nothing beyond where it stopped was touched. A node at four
    // edges' distance from the one dragged — (2, 2, 2) is three, so this
    // is the first ring the ripple did not reach — stayed exactly where it
    // was, which is what "it stops" means when said in positions rather
    // than in a count.
    let gone: Vec<[f64; 3]> = nodes(&s)
        .into_iter()
        .filter(|n| n["id"].as_u64() != Some(middle as u64))
        .filter_map(|n| {
            let p = n["pos"].as_array().unwrap();
            let at = [0, 1, 2].map(|k| p[k].as_f64().unwrap());
            let lattice: f64 = (0..3).map(|k| (at[k] - at[k].round()).abs()).sum();
            (lattice > 1e-9).then_some(at)
        })
        .collect();
    assert!(
        !gone.is_empty() && gone.len() < 20,
        "a handful of nodes round it gave way, not none and not the model: {}",
        gone.len()
    );
}

#[test]
fn a_script_placing_a_node_gets_the_plain_move() {
    // The ripple is a property of the gesture, not of the command: a
    // script that puts a node somewhere is not asking for the nodes round
    // it to be rearranged.
    let mut s = grid(3);
    let (middle, _) = node_at(&s, [1.0, 1.0, 1.0]);
    let (neighbour, was) = node_at(&s, [2.0, 1.0, 1.0]);

    let out = s
        .apply(Command::MoveNode {
            node: middle,
            to: [1.5, 1.4, 1.3],
            carrying: vec![],
            snap: 0.0,
            smooth: false,
        })
        .expect("a move");
    assert!(!out.message.contains("ring"), "{}", out.message);
    assert_eq!(at(&s, neighbour), was, "nothing round it moved");

    // And the journal says which it was, or replaying it would describe a
    // different model.
    assert!(!s.script().contains("smooth = true"));
    s.apply(Command::MoveNode {
        node: middle,
        to: [1.2, 1.2, 1.2],
        carrying: vec![],
        snap: 0.0,
        smooth: true,
    })
    .unwrap();
    assert!(s.script().contains("smooth = true"), "{}", s.script());
}
