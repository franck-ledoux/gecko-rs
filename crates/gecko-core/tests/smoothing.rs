//! Moving the nodes towards the middle of their neighbours, as far as what
//! they follow allows.
//!
//! Two halves to the name. **Laplacian**: a node goes a share of the way to
//! the average of its one-ring. **Smart**: the move is taken only if it does
//! not make the worst corner round that node any worse, which is what keeps
//! a smoother from turning cells inside out on a concave boundary.
//!
//! And the classification decides who may move at all. The pair that
//! matters is the last two rows of the table in `smooth`: an interior node
//! with no classification is free, and a **boundary** node with no
//! classification is not — it has no surface to be kept on, and moving it
//! would pull the blocking off the model.

use std::collections::HashMap;

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, scaled_jacobian, Blocking, On, Smoothing, HEX_CORNER_EDGES};

/// Glue every pair of faces that sit on the same four corners.
fn weld_everything(b: &mut Blocking) {
    let sides = b.block_faces();
    let key = |s: &gecko_core::BlockFace| {
        let mut at = [0.0; 3];
        for node in s.nodes {
            let p = b.position(node).unwrap();
            for k in 0..3 {
                at[k] += p[k] / 4.0;
            }
        }
        at.map(|x| (x * 1e6).round() as i64)
    };
    let mut seen: HashMap<[i64; 3], CellId> = HashMap::new();
    let mut pairs = Vec::new();
    for side in &sides {
        match seen.entry(key(side)) {
            std::collections::hash_map::Entry::Occupied(e) => pairs.push((*e.get(), side.face)),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(side.face);
            }
        }
    }
    for (first, second) in pairs {
        b.glue_onto(first, second).unwrap();
    }
}

/// Two by two by two unit cubes, every touching pair glued: one node
/// inside, at (1, 1, 1), with six neighbours and no boundary at all.
fn grid() -> Blocking {
    let mut b = Blocking::new().unwrap();
    for i in 0..2 {
        for j in 0..2 {
            for k in 0..2 {
                b.add_block(cube_corners([i as f64, j as f64, k as f64], 1.0))
                    .unwrap();
            }
        }
    }
    weld_everything(&mut b);
    b
}

/// Nothing to project onto: what a blocking with no geometry gets.
fn nowhere(_: On, _: [f64; 3]) -> Option<[f64; 3]> {
    None
}

/// The node nearest a point, by name.
fn node_at(b: &Blocking, p: [f64; 3]) -> CellId {
    b.nodes()
        .min_by(|(_, a), (_, c)| {
            let d = |q: &[f64; 3]| (0..3).map(|k| (q[k] - p[k]).powi(2)).sum::<f64>();
            d(a).total_cmp(&d(c))
        })
        .expect("a node")
        .0
}

fn at(b: &Blocking, node: CellId) -> [f64; 3] {
    b.node_by_id(node)
        .and_then(|v| b.position(v))
        .expect("the node is there")
}

/// The worst corner of the whole blocking, by the same measure smoothing
/// weighs a move with.
fn worst(b: &Blocking) -> f64 {
    let mut lowest = f64::INFINITY;
    for hex in b.hexes() {
        let corners = hex.corners.map(|c| b.position(c).unwrap_or([0.0; 3]));
        for (corner, sides) in HEX_CORNER_EDGES.iter().enumerate() {
            lowest = lowest.min(scaled_jacobian(corners[corner], sides.map(|s| corners[s])));
        }
    }
    lowest
}

#[test]
fn an_inside_node_pulled_off_centre_comes_back() {
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.3, 1.2]).unwrap();

    // All the way to the middle of its ring, which by the grid's own
    // symmetry is where it started.
    let moved = b.smooth(
        Smoothing {
            rounds: 1,
            share: 1.0,
            held: &[],
            only: None,
        },
        nowhere,
    );
    assert!(moved > 0.1, "it should have moved a long way, not {moved}");
    let back = at(&b, middle);
    for k in 0..3 {
        assert!(
            (back[k] - 1.0).abs() < 1e-9,
            "it should be back at the centre, not {back:?}"
        );
    }
}

#[test]
fn the_share_is_how_far_along_the_way_it_goes() {
    // What makes it usable on a model somebody cares about: half a step,
    // looked at, then half a step again.
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.0, 1.0]).unwrap();

    b.smooth(
        Smoothing {
            rounds: 1,
            share: 0.5,
            held: &[],
            only: None,
        },
        nowhere,
    );
    assert!(
        (at(&b, middle)[0] - 1.2).abs() < 1e-9,
        "half way, not {:?}",
        at(&b, middle)
    );

    // And another round of a half takes half of what is left.
    b.smooth(
        Smoothing {
            rounds: 1,
            share: 0.5,
            held: &[],
            only: None,
        },
        nowhere,
    );
    assert!((at(&b, middle)[0] - 1.1).abs() < 1e-9);

    // Nowhere at all is a share of nought, and no rounds is no rounds.
    let still = at(&b, middle);
    for how in [
        Smoothing {
            rounds: 4,
            share: 0.0,
            held: &[],
            only: None,
        },
        Smoothing {
            rounds: 0,
            share: 1.0,
            held: &[],
            only: None,
        },
    ] {
        assert_eq!(b.smooth(how, nowhere), 0.0);
        assert_eq!(at(&b, middle), still);
    }
}

#[test]
fn the_boundary_stays_where_it_is_when_it_follows_nothing() {
    // The rule that keeps a smoother from quietly eating the model: these
    // nodes are on the outside of the blocking and classified on nothing,
    // so there is no surface to keep them on and nothing says where they
    // ought to be instead.
    let mut b = grid();
    let before: Vec<(CellId, [f64; 3])> = b.nodes().collect();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.3, 1.2]).unwrap();

    b.smooth(
        Smoothing {
            rounds: 20,
            share: 1.0,
            held: &[],
            only: None,
        },
        nowhere,
    );
    for (name, was) in before {
        if name == middle {
            continue;
        }
        assert_eq!(at(&b, name), was, "node {name:?} moved and should not have");
    }
}

#[test]
fn frozen_and_on_a_corner_are_both_still() {
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.3, 1.2]).unwrap();
    let was = at(&b, middle);

    // Frozen is frozen, whatever anything else says.
    b.set_frozen(middle, true).unwrap();
    assert_eq!(
        b.smooth(
            Smoothing {
                rounds: 5,
                share: 1.0,
                held: &[],
                only: None
            },
            nowhere
        ),
        0.0
    );
    assert_eq!(at(&b, middle), was);
    b.set_frozen(middle, false).unwrap();

    // A node on a geometric corner has one place to be, so it is not
    // smoothed either — and that is decided without asking the geometry,
    // from the dimension alone.
    b.set_node_class(middle, Some(On { dim: 0, tag: 21 }))
        .unwrap();
    assert_eq!(
        b.smooth(
            Smoothing {
                rounds: 5,
                share: 1.0,
                held: &[],
                only: None
            },
            nowhere
        ),
        0.0
    );
    assert_eq!(at(&b, middle), was);
}

#[test]
fn a_node_on_a_curve_lands_back_on_that_curve() {
    // It moves, and then it is projected: the point to weigh is the one it
    // would end up at, not the one the average suggested.
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.3, 1.2]).unwrap();
    b.set_node_class(middle, Some(On { dim: 1, tag: 7 }))
        .unwrap();

    // A curve running along x at y = 1, z = 1.2: through the model, and
    // **not** through the middle of the ring, so where the node ends up
    // says whether it was projected or merely averaged.
    let onto = |on: On, p: [f64; 3]| (on.dim == 1).then_some([p[0], 1.0, 1.2]);
    b.smooth(
        Smoothing {
            rounds: 3,
            share: 1.0,
            held: &[],
            only: None,
        },
        onto,
    );

    let landed = at(&b, middle);
    assert!(
        (landed[1] - 1.0).abs() < 1e-9 && (landed[2] - 1.2).abs() < 1e-9,
        "it should be on the curve, not {landed:?}"
    );
    assert!(
        (landed[0] - 1.0).abs() < 1e-9,
        "and settled along it, not {landed:?}"
    );

    // A curve the node cannot reach without spoiling the blocks round it
    // is a move the smoother refuses, rather than tearing the model to
    // satisfy a classification.
    let mut c = grid();
    let middle = node_at(&c, [1.0, 1.0, 1.0]);
    c.set_node_class(middle, Some(On { dim: 1, tag: 7 }))
        .unwrap();
    let was = at(&c, middle);
    c.smooth(
        Smoothing {
            rounds: 3,
            share: 1.0,
            held: &[],
            only: None,
        },
        |on: On, p: [f64; 3]| (on.dim == 1).then_some([p[0], 9.0, 9.0]),
    );
    assert_eq!(at(&c, middle), was, "it should have stayed put");
}

#[test]
fn a_held_node_is_held_and_only_names_the_ones_to_consider() {
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.3, 1.2]).unwrap();
    let was = at(&b, middle);

    // Held: the case the interactive drag needs, where the node somebody
    // has just placed must not be smoothed away from where they put it.
    assert_eq!(
        b.smooth(
            Smoothing {
                rounds: 5,
                share: 1.0,
                held: &[middle],
                only: None
            },
            nowhere
        ),
        0.0
    );
    assert_eq!(at(&b, middle), was);

    // And `only` is the other half of that: a ring at a time, with the rest
    // of the model left alone.
    let elsewhere = node_at(&b, [0.0, 0.0, 0.0]);
    assert_eq!(
        b.smooth(
            Smoothing {
                rounds: 5,
                share: 1.0,
                held: &[],
                only: Some(&[elsewhere])
            },
            nowhere
        ),
        0.0,
        "the only node named is on the boundary and follows nothing"
    );
    assert_eq!(at(&b, middle), was, "and the one not named stayed");
}

#[test]
fn a_move_that_would_spoil_a_corner_is_not_taken() {
    // The **smart** half. One cube, every corner of it classified on a
    // surface that takes anything, so the classification allows every node
    // to move — and the average of a corner's three neighbours is towards
    // the middle of the cube, which would collapse it.
    //
    // A cube's corners are right angles: nothing can improve them. So a
    // plain Laplacian shrinks the cube to nothing here, and this refuses
    // every single move.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let names: Vec<CellId> = b.nodes().map(|(id, _)| id).collect();
    for name in &names {
        b.set_node_class(*name, Some(On { dim: 2, tag: 1 }))
            .unwrap();
    }
    let before: Vec<(CellId, [f64; 3])> = b.nodes().collect();
    assert!(
        (worst(&b) - 1.0).abs() < 1e-9,
        "a cube is square everywhere"
    );

    let moved = b.smooth(
        Smoothing {
            rounds: 10,
            share: 1.0,
            held: &[],
            only: None,
        },
        |_, p| Some(p),
    );
    assert_eq!(moved, 0.0, "every move would have made a corner worse");
    for (name, was) in before {
        assert_eq!(at(&b, name), was, "{name:?} moved");
    }
}

#[test]
fn smoothing_never_leaves_the_worst_corner_worse_than_it_found_it() {
    // The promise the guard makes, over a model that does have room to
    // improve: eight blocks with the inside node shoved well off centre.
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.6, 1.5, 0.6]).unwrap();
    let before = worst(&b);
    assert!(before < 0.9, "the grid should be spoilt to start with");

    for _ in 0..5 {
        b.smooth(
            Smoothing {
                rounds: 1,
                share: 1.0,
                held: &[],
                only: None,
            },
            nowhere,
        );
        assert!(
            worst(&b) >= before - 1e-12,
            "it got worse: {} from {before}",
            worst(&b)
        );
    }
    assert!(worst(&b) > before, "and in this case it got better");
}

#[test]
fn the_same_blocking_smooths_the_same_way_every_time() {
    // Nodes are visited by name and each move is applied before the next is
    // worked out, so the answer cannot depend on how the map laid its darts
    // out — which both façades have to agree about.
    let run = || {
        let mut b = grid();
        let middle = node_at(&b, [1.0, 1.0, 1.0]);
        b.set_position(middle, [1.6, 1.5, 0.6]).unwrap();
        b.smooth(
            Smoothing {
                rounds: 3,
                share: 0.7,
                held: &[],
                only: None,
            },
            nowhere,
        );
        let mut all: Vec<[f64; 3]> = b.nodes().map(|(_, p)| p).collect();
        all.sort_by(|a, c| a.partial_cmp(c).unwrap());
        all
    };
    assert_eq!(run(), run());
}
