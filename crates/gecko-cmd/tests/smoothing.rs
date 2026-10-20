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

#[test]
fn the_first_ring_goes_a_fifth_of_the_way_and_the_rings_beyond_it_less() {
    // What the ripple's strength actually is, said in positions rather
    // than in a constant: the ring next to the node that was dragged goes
    // a fifth of the way to the middle of *its* neighbours, the ring
    // beyond it a tenth, and so on.
    //
    // (2, 1, 1) is picked because the arithmetic is closed: its six
    // neighbours are the node that was dragged — held where it was put —
    // and five nodes on the outside of the blocking, which are classified
    // on nothing and so may not move at all. Nothing else in the ring
    // touches it either, so Gauss-Seidel has no say and the expected
    // position is a sum of known numbers.
    let mut s = grid(3);
    let (middle, _) = node_at(&s, [1.0, 1.0, 1.0]);
    let (neighbour, was) = node_at(&s, [2.0, 1.0, 1.0]);
    // Read before the drag: `node_at` answers with where a node is *now*,
    // and after the smoothing that is the answer, not the question.
    let (further, stood) = node_at(&s, [2.0, 2.0, 1.0]);
    let to = [1.5, 1.4, 1.3];

    s.apply(Command::MoveNode {
        node: middle,
        to,
        carrying: vec![],
        snap: 0.0,
        smooth: true,
    })
    .expect("a drag");

    // Its neighbours after the drag: the dragged node where it was put,
    // and the five on the outside where they have always been.
    let ring: [[f64; 3]; 6] = [
        to,
        [3.0, 1.0, 1.0],
        [2.0, 0.0, 1.0],
        [2.0, 2.0, 1.0],
        [2.0, 1.0, 0.0],
        [2.0, 1.0, 2.0],
    ];
    let centroid = [0, 1, 2].map(|k| ring.iter().map(|p| p[k]).sum::<f64>() / 6.0);
    let fifth = [0, 1, 2].map(|k| was[k] + gecko_cmd::RIPPLE_SHARE * (centroid[k] - was[k]));

    let now = at(&s, neighbour);
    for k in 0..3 {
        assert!(
            (now[k] - fifth[k]).abs() < 1e-9,
            "the first ring went {now:?}, a fifth of the way is {fifth:?}"
        );
    }

    // And the ring beyond it went a **tenth** of the way, which is the
    // other half of the rule. (2, 2, 1) is at two edges' distance and its
    // neighbourhood is closed in the same way: two nodes of the first ring,
    // which have already moved by the time this one is looked at, three on
    // the outside, and (2, 2, 2), which is a ring further out and so has
    // not moved yet. Nothing in its own ring touches it either.
    assert_ne!(further, neighbour);
    // (1, 2, 1) is the first-ring node that this one leans on; the drag
    // leaves it a fifth of the way to its own middle, by the same
    // arithmetic as above.
    let inner = [
        to,
        [0.0, 2.0, 1.0],
        [2.0, 2.0, 1.0],
        [1.0, 3.0, 1.0],
        [1.0, 2.0, 0.0],
        [1.0, 2.0, 2.0],
    ];
    let leaning = [0, 1, 2].map(|k| {
        let middle = inner.iter().map(|p| p[k]).sum::<f64>() / 6.0;
        [1.0, 2.0, 1.0][k] + gecko_cmd::RIPPLE_SHARE * (middle - [1.0, 2.0, 1.0][k])
    });
    let round_it = [
        leaning,
        fifth,
        [3.0, 2.0, 1.0],
        [2.0, 3.0, 1.0],
        [2.0, 2.0, 0.0],
        [2.0, 2.0, 2.0],
    ];
    let tenth = [0, 1, 2].map(|k| {
        let middle = round_it.iter().map(|p| p[k]).sum::<f64>() / 6.0;
        stood[k] + (gecko_cmd::RIPPLE_SHARE / 2.0) * (middle - stood[k])
    });
    let went = at(&s, further);
    for k in 0..3 {
        assert!(
            (went[k] - tenth[k]).abs() < 1e-9,
            "the second ring went {went:?}, a tenth of the way is {tenth:?}"
        );
    }
}
