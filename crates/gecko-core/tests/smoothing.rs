//! Moving the nodes to where every block is as near a cube as it can be,
//! as far as what they follow allows.
//!
//! What is minimized is in `gecko_core::smoothing`: per corner, how far
//! from square it is, growing without bound as it folds flat; per block,
//! how unevenly it is spaced. Every move is worked out from the same
//! positions and they are taken together, so the answer does not depend on
//! the order nodes are named in — and a symmetric configuration stays
//! symmetric. A step is taken only if the whole is better and no corner
//! falls below the floor, which is what keeps a smoother from turning cells
//! inside out on a concave boundary.
//!
//! And the classification decides who may move at all. The pair that
//! matters is the last two rows of the table in `smooth`: an interior node
//! with no classification is free, and a **boundary** node with no
//! classification is not — it has no surface to be kept on, and moving it
//! would pull the blocking off the model.

use std::collections::HashMap;

use gecko_core::ids::CellId;
use gecko_core::{
    cube_corners, flatness, scaled_jacobian, Blocking, On, Smoothing, HEX_CORNER_EDGES,
};

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
/// inside, at (1, 1, 1), with six neighbors and no boundary at all.
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
    b.position(node).expect("the node is there")
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
fn an_inside_node_pulled_off_center_comes_back() {
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.3, 1.2]).unwrap();

    // All the way to the middle, which by the grid's own symmetry is where
    // it started — in a few rounds, the first of them short: the corner it
    // was pulled almost flat against is where the energy is steepest.
    let moved = b.smooth(
        Smoothing {
            rounds: 12,
            share: 1.0,
            held: &[],
        },
        nowhere,
    );
    assert!(moved > 0.1, "it should have moved a long way, not {moved}");
    let back = at(&b, middle);
    for k in 0..3 {
        assert!(
            (back[k] - 1.0).abs() < 1e-8,
            "it should be back at the center, not {back:?}"
        );
    }
}

#[test]
fn the_share_is_how_far_the_first_step_goes() {
    // What makes it usable on a model somebody cares about: a short step,
    // looked at, then another.
    let moved = |share: f64| {
        let mut b = grid();
        let middle = node_at(&b, [1.0, 1.0, 1.0]);
        b.set_position(middle, [1.4, 1.0, 1.0]).unwrap();
        b.smooth(
            Smoothing {
                rounds: 1,
                share,
                held: &[],
            },
            nowhere,
        );
        1.4 - at(&b, middle)[0]
    };
    let (quarter, whole) = (moved(0.25), moved(1.0));
    assert!(quarter > 0.0, "a quarter of a step is a step: {quarter}");
    assert!(
        quarter < whole,
        "and shorter than a whole one: {quarter} {whole}"
    );

    // Nowhere at all is a share of nought, and no rounds is no rounds.
    let mut b = grid();
    let middle = node_at(&b, [1.0, 1.0, 1.0]);
    b.set_position(middle, [1.4, 1.0, 1.0]).unwrap();
    let still = at(&b, middle);
    for how in [
        Smoothing {
            rounds: 4,
            share: 0.0,
            held: &[],
        },
        Smoothing {
            rounds: 0,
            share: 1.0,
            held: &[],
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
                held: &[]
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
                held: &[]
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
            rounds: 30,
            share: 1.0,
            held: &[],
        },
        onto,
    );

    let landed = at(&b, middle);
    assert!(
        (landed[1] - 1.0).abs() < 1e-9 && (landed[2] - 1.2).abs() < 1e-9,
        "it should be on the curve, not {landed:?}"
    );
    assert!(
        (landed[0] - 1.0).abs() < 1e-6,
        "and settled along it, half way between its ends, not {landed:?}"
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
                held: &[middle]
            },
            nowhere
        ),
        0.0
    );
    assert_eq!(at(&b, middle), was);
}

#[test]
fn a_move_that_would_spoil_a_corner_is_not_taken() {
    // The **smart** half. One cube, every corner of it classified on a
    // surface that takes anything, so the classification allows every node
    // to move — and the average of a corner's three neighbors is towards
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
    // improve: eight blocks with the inside node shoved well off center.
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
    // worked out, so the answer cannot depend on where the arenas put
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
            },
            nowhere,
        );
        let mut all: Vec<[f64; 3]> = b.nodes().map(|(_, p)| p).collect();
        all.sort_by(|a, c| a.partial_cmp(c).unwrap());
        all
    };
    assert_eq!(run(), run());
}

/// A two by two grid of laid quadrangles: one patch, nine nodes, and no
/// block anywhere. What a surface blocking is before it is extruded.
fn patch_grid() -> Blocking {
    let mut b = Blocking::new().unwrap();
    for i in 0..2 {
        for j in 0..2 {
            let (x, y) = (i as f64, j as f64);
            b.lay_face([
                [x, y, 0.0],
                [x + 1.0, y, 0.0],
                [x + 1.0, y + 1.0, 0.0],
                [x, y + 1.0, 0.0],
            ])
            .expect("a quadrangle on the plane");
        }
    }
    b
}

/// Back onto z = 0, which is where the patch was laid.
fn onto_the_plane(_: On, at: [f64; 3]) -> Option<[f64; 3]> {
    Some([at[0], at[1], 0.0])
}

#[test]
fn a_patch_is_smoothed_by_its_quadrangles_and_nothing_else() {
    // No block is involved, so the measure that guards the move cannot be
    // the one a hexahedron is judged by. A patch face can fold flat on its
    // own, and that is what has to be weighed instead.
    let mut b = patch_grid();
    assert_eq!(
        (b.nb_nodes(), b.nb_faces(), b.nb_blocks(), b.nb_patches()),
        (9, 4, 0, 1),
        "the four quadrangles welded into one patch"
    );

    let middle = node_at(&b, [1.0, 1.0, 0.0]);
    b.set_position(middle, [1.4, 1.3, 0.0]).unwrap();

    // Unclassified it does not move at all: every face of a patch has an
    // empty second side, so all nine nodes are on the surface, and the
    // boundary is held.
    assert_eq!(
        b.smooth(
            Smoothing {
                rounds: 1,
                share: 1.0,
                held: &[],
            },
            onto_the_plane,
        ),
        0.0
    );
    assert_eq!(at(&b, middle), [1.4, 1.3, 0.0]);

    // Following the surface it was laid on, it is free, and the four
    // quadrangles round it put it back in the middle.
    b.set_node_class(middle, Some(On { dim: 2, tag: 1 }))
        .unwrap();
    let moved = b.smooth(
        Smoothing {
            rounds: 20,
            share: 1.0,
            held: &[],
        },
        onto_the_plane,
    );
    assert!(moved > 0.2, "it should have moved a long way, not {moved}");
    let back = at(&b, middle);
    assert!(
        (back[0] - 1.0).abs() < 1e-6 && (back[1] - 1.0).abs() < 1e-6 && back[2].abs() < 1e-9,
        "it should be back at the middle of the patch, not {back:?}"
    );
}

#[test]
fn the_two_measures_a_move_is_weighed_by() {
    // One at a right angle, nought where the corner has folded flat, and
    // nought — not a NaN, not an infinity — where two of the points are the
    // same point. A degenerate corner is reached: it is what a Laplacian
    // step produces on the way to being refused.
    let o = [0.0, 0.0, 0.0];
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];

    assert!((flatness(o, x, y) - 1.0).abs() < 1e-12);
    assert!(flatness(o, x, [2.0, 0.0, 0.0]).abs() < 1e-12, "folded flat");
    assert_eq!(flatness(o, o, y), 0.0);
    assert_eq!(flatness(o, x, o), 0.0);

    assert!((scaled_jacobian(o, [x, y, z]) - 1.0).abs() < 1e-12);
    assert!(scaled_jacobian(o, [y, x, z]) < 0.0, "inside out");
    assert_eq!(scaled_jacobian(o, [x, y, o]), 0.0);
}

#[test]
fn an_open_patch_does_not_collapse_nor_fold() {
    // Every node of an open patch is free to slide over the plane, and a
    // Laplacian walks the whole of it into a point: nothing but the shape
    // of the quadrangles says not to. Here nothing is gained by shrinking
    // — the energy does not change with size — and a quadrangle cannot be
    // folded over on the way to anywhere.
    let mut b = patch_grid();
    for name in b.nodes().map(|(n, _)| n).collect::<Vec<_>>() {
        b.set_node_class(name, Some(On { dim: 2, tag: 1 })).unwrap();
    }
    // A concave outline, which is where a Laplacian does its damage: one
    // corner of the patch pushed deep inside it — so deep that the
    // quadrangle it is a corner of is folded over from the start.
    let bitten = node_at(&b, [2.0, 2.0, 0.0]);
    b.set_position(bitten, [0.9, 0.9, 0.0]).unwrap();
    let turns = |b: &Blocking| -> Vec<Vec<f64>> {
        b.block_faces()
            .iter()
            .map(|face| {
                let at = face.nodes.map(|n| b.position(n).unwrap());
                (0..4)
                    .map(|k| {
                        let u = [0, 1].map(|i| at[(k + 1) % 4][i] - at[k][i]);
                        let v = [0, 1].map(|i| at[(k + 3) % 4][i] - at[k][i]);
                        u[0] * v[1] - u[1] * v[0]
                    })
                    .collect()
            })
            .collect()
    };
    let unfolded = |t: &Vec<f64>| t.iter().all(|x| *x > 0.0) || t.iter().all(|x| *x < 0.0);
    let before = turns(&b);
    assert_eq!(
        before.iter().filter(|t| unfolded(t)).count(),
        3,
        "three quadrangles whole, one folded"
    );

    b.smooth(
        Smoothing {
            rounds: 30,
            share: 1.0,
            held: &[],
        },
        onto_the_plane,
    );

    let span = |k: usize| {
        let all: Vec<f64> = b.nodes().map(|(_, p)| p[k]).collect();
        all.iter().copied().fold(f64::MIN, f64::max) - all.iter().copied().fold(f64::MAX, f64::min)
    };
    assert!(
        span(0) > 1.5 && span(1) > 1.5,
        "the patch should have kept most of its extent, not {:?}",
        (span(0), span(1))
    );
    // None that was whole has been folded over.
    for (was, now) in before.iter().zip(turns(&b)) {
        if unfolded(was) {
            assert!(unfolded(&now), "a quadrangle folded: {now:?}");
        }
    }
}

/// One unit cube cut across x at `at`, its corners on the corners of the
/// model and the four nodes the cut made each on the line of the cube's
/// edge it is on — free to slide along x and nowhere else.
fn cut_cube(at: f64) -> (Blocking, Vec<CellId>) {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    for (n, _) in b.nodes().collect::<Vec<_>>() {
        b.set_node_class(n, Some(On { dim: 0, tag: 1 })).unwrap();
    }
    let (edge, from, _) = b
        .edges()
        .map(|(e, a, c, _)| (e, b.position(a).unwrap(), b.position(c).unwrap()))
        .find(|(_, p, q)| (p[1] - q[1]).abs() < 1e-12 && (p[2] - q[2]).abs() < 1e-12)
        .expect("an edge along x");
    let t = if from[0] < 0.5 { at } else { 1.0 - at };
    b.split_sheet(edge, t).unwrap();
    let cut: Vec<CellId> = b
        .nodes()
        .filter(|(_, p)| (p[0] - at).abs() < 1e-9)
        .map(|(n, _)| n)
        .collect();
    assert_eq!(cut.len(), 4);
    for n in &cut {
        let p = b.position(*n).unwrap();
        let line = (p[1].round() as i32) * 2 + p[2].round() as i32;
        b.set_node_class(*n, Some(On { dim: 1, tag: line }))
            .unwrap();
    }
    (b, cut)
}

/// Back onto the cube's edge along x that a tag names.
fn onto_the_edge(on: On, at: [f64; 3]) -> Option<[f64; 3]> {
    (on.dim == 1).then(|| [at[0], (on.tag / 2) as f64, (on.tag % 2) as f64])
}

#[test]
fn a_cut_dropped_anywhere_goes_to_the_middle_and_mirrors_exactly() {
    // Two blocks between fixed ends cost least at the same height: a cut
    // put down at 0.3 ends at 0.5. And the same cut put down at 0.7 ends
    // at the mirror image of it — to the last digits, because no move
    // depends on which node was looked at first.
    let settle = |at: f64| {
        let (mut b, cut) = cut_cube(at);
        b.smooth(
            Smoothing {
                rounds: 40,
                share: 0.5,
                held: &[],
            },
            onto_the_edge,
        );
        let mut xs: Vec<f64> = cut.iter().map(|n| b.position(*n).unwrap()[0]).collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    let low = settle(0.3);
    let high = settle(0.7);
    for x in low.iter().chain(&high) {
        assert!(
            (x - 0.5).abs() < 1e-6,
            "the cut should be in the middle: {low:?} {high:?}"
        );
    }
    for (a, c) in low.iter().zip(&high) {
        assert!(
            (a - (1.0 - c)).abs() < 1e-9,
            "mirror images: {low:?} {high:?}"
        );
    }
}

#[test]
fn a_grid_shoved_about_inside_comes_back_square() {
    // Three by three by three cubes, the boundary held, and the eight nodes
    // inside it pushed each its own way: every corner of every block back
    // to a right angle.
    let mut b = Blocking::new().unwrap();
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                b.add_block(cube_corners([i as f64, j as f64, k as f64], 1.0))
                    .unwrap();
            }
        }
    }
    weld_everything(&mut b);
    let inside: Vec<CellId> = b
        .nodes()
        .filter(|(_, p)| p.iter().all(|x| *x > 0.5 && *x < 2.5))
        .map(|(n, _)| n)
        .collect();
    assert_eq!(inside.len(), 8);
    for (k, n) in inside.iter().enumerate() {
        let p = b.position(*n).unwrap();
        let push = [0.2, -0.15, 0.1].map(|d| d * if k % 2 == 0 { 1.0 } else { -0.7 });
        b.set_position(*n, [p[0] + push[0], p[1] + push[1], p[2] + push[2]])
            .unwrap();
    }
    assert!(worst(&b) < 0.9, "spoilt to start with: {}", worst(&b));
    b.smooth(
        Smoothing {
            rounds: 60,
            share: 0.5,
            held: &[],
        },
        nowhere,
    );
    assert!(worst(&b) > 0.999, "square again: {}", worst(&b));
}

#[test]
fn an_edge_leaving_a_curve_is_drawn_square_to_it() {
    // Two blocks side by side, the edge between them running from a node
    // fixed at x = 1.5 along the top down to one free to slide along the
    // bottom line. Even spacing alone would put the bottom end at 0.5,
    // where the two blocks are the same size; a right angle at the bottom
    // puts it at 1.5, straight under the top. The spacing of a block on
    // its own already prefers square corners; weighing the corners as well
    // takes it further.
    let mut b = Blocking::new().unwrap();
    let block = |x0: f64, x1: f64, t0: f64, t1: f64| {
        [
            [x0, 0.0, 0.0],
            [x1, 0.0, 0.0],
            [t1, 1.0, 0.0],
            [t0, 1.0, 0.0],
            [x0, 0.0, 1.0],
            [x1, 0.0, 1.0],
            [t1, 1.0, 1.0],
            [t0, 1.0, 1.0],
        ]
    };
    b.add_block(block(0.0, 0.5, 0.0, 1.5)).unwrap();
    b.add_block(block(0.5, 2.0, 1.5, 2.0)).unwrap();
    weld_everything(&mut b);
    assert_eq!(b.nb_nodes(), 12);
    let mut sliding = Vec::new();
    for (n, p) in b.nodes().collect::<Vec<_>>() {
        if (p[0] - 0.5).abs() < 1e-12 && p[1] == 0.0 {
            b.set_node_class(
                n,
                Some(On {
                    dim: 1,
                    tag: p[2] as i32,
                }),
            )
            .unwrap();
            sliding.push(n);
        } else {
            b.set_frozen(n, true).unwrap();
        }
    }
    assert_eq!(sliding.len(), 2);
    b.smooth(
        Smoothing {
            rounds: 40,
            share: 0.5,
            held: &[],
        },
        |on: On, p: [f64; 3]| (on.dim == 1).then_some([p[0], 0.0, on.tag as f64]),
    );
    for n in sliding {
        let x = b.position(n).unwrap()[0];
        // 1.31 measured; 1.16 with the corners weighed for nothing.
        assert!(
            x > 1.25,
            "the bottom end should have gone towards square, to {x}"
        );
        assert!(x < 1.5, "but not all the way, spacing counts too: {x}");
    }
}

#[test]
fn no_corner_is_turned_inside_out_however_it_starts() {
    // Forty grids of three by three by two blocks, every node shoved
    // a random way and free to slide anywhere — the worst a smoothing can
    // be handed. The energy grows without bound as a corner folds flat, but
    // a step can jump clean over that wall: without the floor, three of
    // these end with a corner inside out. With it, none falls below the
    // floor, or below where it started if it started lower.
    let mut seed: u64 = 12345;
    let mut random = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / (1u64 << 31) as f64 * 2.0 - 1.0
    };
    for trial in 0..40 {
        let mut b = Blocking::new().unwrap();
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..2 {
                    b.add_block(cube_corners([i as f64, j as f64, k as f64], 1.0))
                        .unwrap();
                }
            }
        }
        weld_everything(&mut b);
        for (n, p) in b.nodes().collect::<Vec<_>>() {
            let shove = [0.45 * random(), 0.45 * random(), 0.45 * random()];
            b.set_position(n, [p[0] + shove[0], p[1] + shove[1], p[2] + shove[2]])
                .unwrap();
            b.set_node_class(n, Some(On { dim: 2, tag: 1 })).unwrap();
        }
        let before = worst(&b);
        if before <= 0.0 {
            continue;
        }
        b.smooth(
            Smoothing {
                rounds: 30,
                share: 1.0,
                held: &[],
            },
            |_, p| Some(p),
        );
        let after = worst(&b);
        assert!(
            after >= before.min(gecko_core::smoothing::FLOOR) - 1e-9,
            "trial {trial}: the worst corner went from {before} to {after}"
        );
    }
}
