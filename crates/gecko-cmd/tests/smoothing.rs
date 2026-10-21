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
fn moving_a_node_moves_that_node_and_nothing_else() {
    // A drag used to ripple: the ring of nodes around the one being moved
    // was smoothed a pass, then the ring beyond it, and so on. It was
    // taken out again — what the reader wants of a drag is the node they
    // have hold of, and a neighbourhood rearranging itself under the
    // pointer is a surprise, not a help. Smoothing is `gecko.smooth` and
    // the key that calls it, asked for when it is wanted.
    let mut s = grid(3);
    let (middle, _) = node_at(&s, [1.0, 1.0, 1.0]);
    let (neighbour, was) = node_at(&s, [2.0, 1.0, 1.0]);
    let (further, stood) = node_at(&s, [2.0, 2.0, 1.0]);

    let out = s
        .apply(Command::MoveNode {
            node: middle,
            to: [1.5, 1.4, 1.3],
            carrying: vec![],
            snap: 0.0,
        })
        .expect("a move");

    assert_eq!(at(&s, middle), [1.5, 1.4, 1.3]);
    assert_eq!(at(&s, neighbour), was, "the node next to it stayed");
    assert_eq!(at(&s, further), stood, "and so did the one beyond that");
    assert!(!out.message.contains("ring"), "{}", out.message);

    // Nothing in the journal asks for it either: there is no such option
    // to ask with any more.
    assert!(!s.script().contains("smooth"), "{}", s.script());
}
