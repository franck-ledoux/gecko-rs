//! Finding the sheet through an edge.
//!
//! No cutting yet. The point of doing this on its own is that the sheet, and
//! above all the direction the cut parameter runs in along it, is the part
//! that cannot be checked by looking at the result: a sheet oriented wrongly
//! cuts alternate cells at `t` and `1 - t` and comes out as a concertina,
//! which is a thing you notice by counting, not by looking.

use gecko_core::ids::CellId;
use gecko_core::{across, cube_corners, Blocking, BlockingError};

fn blocks(b: &Blocking) -> Vec<CellId> {
    let mut names: Vec<_> = b.block_faces().iter().map(|s| s.block).collect();
    names.dedup();
    names
}

fn seam(b: &Blocking, block: CellId, x: f64) -> Option<CellId> {
    b.block_faces()
        .iter()
        .filter(|s| s.block == block)
        .find(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[0] - x).abs() < 1e-9)
        })
        .map(|s| s.face)
}

/// Two unit cubes meeting on the plane x = 1, glued.
fn glued() -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
    let names = blocks(&b);
    let (first, second) = (
        seam(&b, names[0], 1.0).unwrap(),
        seam(&b, names[1], 1.0).unwrap(),
    );
    b.glue_faces(first, second).unwrap();
    b
}

/// The name of an edge whose two ends differ only along this axis.
fn edge_along(b: &Blocking, axis: usize, at: [f64; 3]) -> CellId {
    b.map()
        .iter_edges()
        .map(|e| (e, b.edge_endpoints(e)))
        .map(|(e, (u, v))| (e, b.position(u).unwrap(), b.position(v).unwrap()))
        .find(|(_, p, q)| {
            (0..3).all(|k| {
                if k == axis {
                    (p[k] - q[k]).abs() > 0.5
                } else {
                    (p[k] - q[k]).abs() < 1e-9 && (p[k] - at[k]).abs() < 1e-9
                }
            })
        })
        .map(|(e, _, _)| b.edge_tag(e).id)
        .expect("an edge along that axis")
}

#[test]
fn across_a_quadrangle_the_opposite_edge_runs_the_other_way() {
    // A→B→C→D. Opposite AB is CD, and the corners that correspond are A↔D
    // and B↔C — so A→B matches D→C, not C→D.
    let quad = [10, 11, 12, 13];
    assert_eq!(across(quad, 10, 11), Some((13, 12)));
    assert_eq!(across(quad, 11, 12), Some((10, 13)));
    assert_eq!(across(quad, 12, 13), Some((11, 10)));
    assert_eq!(across(quad, 13, 10), Some((12, 11)));
}

#[test]
fn across_reads_an_edge_traversed_either_way() {
    let quad = [10, 11, 12, 13];
    // B→A is the reverse of A→B, so its opposite is the reverse of D→C.
    assert_eq!(across(quad, 11, 10), Some((12, 13)));
    assert_eq!(across(quad, 10, 13), Some((11, 12)));
}

#[test]
fn across_says_nothing_about_an_edge_that_is_not_there() {
    assert_eq!(across([10, 11, 12, 13], 10, 12), None); // a diagonal
    assert_eq!(across([10, 11, 12, 13], 10, 99), None);
}

#[test]
fn a_lone_block_has_four_edges_in_a_sheet() {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();

    for axis in 0..3 {
        let mut at = [0.0; 3];
        at[(axis + 1) % 3] = 0.0;
        at[(axis + 2) % 3] = 0.0;
        let edge = edge_along(&b, axis, at);
        let sheet = b.sheet_through(edge).expect("an orientable sheet");
        assert_eq!(sheet.len(), 4, "axis {axis}");
        // All four are parallel to the one named, and none is repeated.
        let mut names: Vec<_> = sheet.iter().map(|s| s.edge).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 4);
    }
}

#[test]
fn every_edge_of_a_sheet_points_the_same_way() {
    // The thing that cannot be seen in the result. One `t` has to mean the
    // same side of the sheet everywhere, so the directions must agree.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let edge = edge_along(&b, 0, [0.0, 0.0, 0.0]);
    let sheet = b.sheet_through(edge).unwrap();

    let direction = |s: &gecko_core::SheetEdge| {
        let (p, q) = (b.position(s.from).unwrap(), b.position(s.to).unwrap());
        [q[0] - p[0], q[1] - p[1], q[2] - p[2]]
    };
    let first = direction(&sheet[0]);
    for s in &sheet {
        let d = direction(s);
        let dot = (0..3).map(|k| d[k] * first[k]).sum::<f64>();
        assert!(dot > 0.0, "edge {:?} runs backwards: {d:?}", s.edge);
    }
}

#[test]
fn a_sheet_runs_on_through_a_glued_block() {
    let b = glued();

    // Parallel to the seam: the sheet crosses into the second block, and the
    // two edges lying in the shared face belong to both. Four and four,
    // sharing two.
    let along_y = edge_along(&b, 1, [0.0, 0.0, 0.0]);
    assert_eq!(b.sheet_through(along_y).unwrap().len(), 6);

    // Across the seam: the shared face is perpendicular to x and has no
    // x-parallel edge, so the sheet stays in the block it started in.
    let along_x = edge_along(&b, 0, [0.0, 0.0, 0.0]);
    assert_eq!(b.sheet_through(along_x).unwrap().len(), 4);
}

#[test]
fn a_sheet_spanning_two_blocks_still_points_one_way() {
    let b = glued();
    let sheet = b.sheet_through(edge_along(&b, 1, [0.0, 0.0, 0.0])).unwrap();
    assert_eq!(sheet.len(), 6);

    let direction = |s: &gecko_core::SheetEdge| {
        let (p, q) = (b.position(s.from).unwrap(), b.position(s.to).unwrap());
        [q[0] - p[0], q[1] - p[1], q[2] - p[2]]
    };
    let first = direction(&sheet[0]);
    for s in &sheet {
        let d = direction(s);
        assert!(
            (0..3).map(|k| d[k] * first[k]).sum::<f64>() > 0.0,
            "edge {:?} runs backwards across the seam",
            s.edge
        );
    }
}

#[test]
fn the_answer_is_the_same_every_time() {
    // Both modes ship this, and a walk over orbits is exactly where a hash
    // map's order would leak out.
    let b = glued();
    let edge = edge_along(&b, 1, [0.0, 0.0, 0.0]);
    let first = b.sheet_through(edge).unwrap();
    for _ in 0..5 {
        assert_eq!(b.sheet_through(edge).unwrap(), first);
    }
}

#[test]
fn a_name_no_edge_answers_to_is_refused() {
    let b = glued();
    assert!(matches!(
        b.sheet_through(CellId::from_raw(9999)),
        Err(BlockingError::NoSuchEdge(_))
    ));
    assert!(matches!(
        b.sheet_through(CellId::NONE),
        Err(BlockingError::NoSuchEdge(_))
    ));
}

/// Taking a whole sheet of blocks out, and closing the gap behind it.
mod removal {
    use super::*;

    fn cells(b: &Blocking) -> (usize, usize, usize, usize) {
        (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks())
    }

    /// Three unit slabs stacked along z and glued, at z in 0..1, 1..2, 2..3.
    ///
    /// Stacked rather than cut: a cut lands at `t` from whichever end the
    /// sheet calls `from`, which is the map's business and not something a
    /// test should be predicting.
    fn three_slabs() -> Blocking {
        let mut b = Blocking::new().unwrap();
        for z in [0.0, 1.0, 2.0] {
            b.add_block(cube_corners([0.0, 0.0, z], 1.0)).unwrap();
        }
        for z in [1.0, 2.0] {
            let pair: Vec<CellId> = b
                .block_faces()
                .iter()
                .filter(|s| {
                    s.nodes
                        .iter()
                        .all(|n| (b.position(*n).unwrap()[2] - z).abs() < 1e-9)
                })
                .map(|s| s.face)
                .collect();
            assert_eq!(pair.len(), 2, "two faces meet at z = {z}");
            b.glue_faces(pair[0], pair[1]).unwrap();
        }
        b
    }

    /// An edge running across the slab between these two heights.
    fn across(b: &Blocking, low: f64, high: f64) -> CellId {
        b.map()
            .iter_edges()
            .map(|e| (e, b.edge_endpoints(e)))
            .find(|(_, (u, v))| {
                let (p, q) = (b.position(*u).unwrap(), b.position(*v).unwrap());
                let mut z = [p[2], q[2]];
                z.sort_by(f64::total_cmp);
                (z[0] - low).abs() < 1e-9 && (z[1] - high).abs() < 1e-9
            })
            .map(|(e, _)| b.edge_tag(e).id)
            .unwrap_or_else(|| panic!("an edge from {low} to {high}"))
    }

    #[test]
    fn taking_out_the_middle_closes_the_gap() {
        let mut b = three_slabs();
        assert_eq!(cells(&b), (16, 28, 16, 3));

        b.remove_sheet(across(&b, 1.0, 2.0))
            .expect("an interior sheet");

        // Two slabs left, welded: the same shape as two blocks glued.
        assert_eq!(cells(&b), (12, 20, 11, 2));
        // And the seam sits half way between where the two faces were.
        let heights: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
        assert_eq!(
            heights.iter().filter(|z| (**z - 1.5).abs() < 1e-9).count(),
            4
        );
        assert_eq!(heights.iter().filter(|z| z.abs() < 1e-9).count(), 4);
        assert_eq!(
            heights.iter().filter(|z| (**z - 3.0).abs() < 1e-9).count(),
            4
        );
    }

    #[test]
    fn taking_out_a_sheet_at_the_surface_just_loses_it() {
        let mut b = three_slabs();
        // The bottom slab: one cap is the model's underside, with nothing
        // behind it to close against.
        b.remove_sheet(across(&b, 0.0, 1.0))
            .expect("a sheet at the surface");

        assert_eq!(b.nb_blocks(), 2);
        let heights: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
        assert!(
            heights.iter().all(|z| *z >= 1.0 - 1e-9),
            "the model should start at z = 1 now: {heights:?}"
        );
    }

    #[test]
    fn taking_out_a_lone_block_empties_the_map() {
        let mut b = Blocking::new().unwrap();
        b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
        b.remove_sheet(edge_along(&b, 2, [0.0, 0.0, 0.0])).unwrap();
        assert_eq!(cells(&b), (0, 0, 0, 0));
    }

    #[test]
    fn a_sheet_across_two_glued_blocks_takes_both() {
        // The sheet runs through both, so both go, and what was above and
        // below each closes up.
        let mut b = glued();
        let along_y = edge_along(&b, 1, [0.0, 0.0, 0.0]);
        assert_eq!(b.sheet_through(along_y).unwrap().len(), 6);
        b.remove_sheet(along_y).unwrap();
        assert_eq!(cells(&b), (0, 0, 0, 0));
    }

    /// An n by n by n grid of unit cubes, every touching pair glued.
    pub(super) fn grid(n: usize) -> Blocking {
        let mut b = Blocking::new().unwrap();
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    b.add_block(cube_corners([i as f64, j as f64, k as f64], 1.0))
                        .unwrap();
                }
            }
        }
        // Every pair of faces on the same four corners, worked out in one
        // pass: gluing one pair leaves the others' names alone, so there is
        // no need to look again after each.
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
        let mut seen: std::collections::HashMap<[i64; 3], CellId> =
            std::collections::HashMap::new();
        let mut pairs = Vec::new();
        for side in &sides {
            match seen.entry(key(side)) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    pairs.push((*e.get(), side.face));
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(side.face);
                }
            }
        }
        for (first, second) in pairs {
            b.glue_faces(first, second).unwrap();
        }
        b
    }

    #[test]
    fn taking_a_layer_out_of_a_grid_glues_every_pair_it_leaves() {
        // Nine blocks in the layer, so nine pairs to close. The caps of
        // neighbouring blocks share their corners, which is what a single
        // block in a layer never shows.
        let mut b = grid(3);
        assert_eq!(b.nb_blocks(), 27);
        assert_eq!(b.nb_nodes(), 64);

        b.remove_sheet(across(&b, 1.0, 2.0))
            .expect("the middle layer");

        assert_eq!(b.nb_blocks(), 18);
        // Three levels of corners left — 0, the seam, and 3 — not four.
        assert_eq!(b.nb_nodes(), 48);
        let seam: Vec<f64> = b
            .nodes()
            .map(|(_, p)| p[2])
            .filter(|z| (*z - 1.5).abs() < 1e-9)
            .collect();
        assert_eq!(seam.len(), 16, "the seam is one level of corners");

        // And the two halves are welded, not merely touching: nine faces
        // with a block on each side at the seam.
        let sides = b.block_faces();
        let welded = sides
            .iter()
            .filter(|s| sides.iter().filter(|o| o.face == s.face).count() == 2)
            .filter(|s| {
                s.nodes
                    .iter()
                    .all(|n| (b.position(*n).unwrap()[2] - 1.5).abs() < 1e-9)
            })
            .count();
        assert_eq!(welded, 18, "nine faces, listed once per block");
    }

    #[test]
    fn a_name_no_edge_answers_to_is_refused_and_changes_nothing() {
        let mut b = three_slabs();
        let before = cells(&b);
        assert!(matches!(
            b.remove_sheet(CellId::from_raw(9999)),
            Err(BlockingError::NoSuchEdge(_))
        ));
        assert_eq!(cells(&b), before);
    }
}

/// Peeling a big grid back, a layer at a time.
mod peeling {
    use super::removal::*;
    use gecko_core::ids::CellId;
    use gecko_core::Blocking;

    /// Any edge whose sheet runs between these two heights along an axis.
    fn spanning(b: &Blocking, axis: usize, low: f64, high: f64) -> Option<CellId> {
        b.map()
            .iter_edges()
            .map(|e| (e, b.edge_endpoints(e)))
            .find(|(_, (u, v))| {
                let (p, q) = (b.position(*u).unwrap(), b.position(*v).unwrap());
                let mut at = [p[axis], q[axis]];
                at.sort_by(f64::total_cmp);
                (at[0] - low).abs() < 1e-9
                    && (at[1] - high).abs() < 1e-9
                    && (0..3).all(|k| k == axis || (p[k] - q[k]).abs() < 1e-9)
            })
            .map(|(e, _)| b.edge_tag(e).id)
    }

    #[test]
    fn a_grid_can_be_peeled_back_to_a_smaller_one() {
        // Four layers each way, taken down to two, one sheet at a time.
        // Each removal has to leave a grid the next one can read.
        let mut b = grid(4);
        assert_eq!(b.nb_blocks(), 64);

        for axis in 0..3 {
            for _ in 0..2 {
                // Whichever layer is still there: the coordinates shift as
                // the seams close, so each is looked for afresh.
                let edge = (0..8)
                    .flat_map(|i| {
                        let low = i as f64 / 2.0;
                        spanning(&b, axis, low, low + 0.5)
                            .or_else(|| spanning(&b, axis, low, low + 1.0))
                    })
                    .next()
                    .expect("a layer to take out");
                b.remove_sheet(edge).expect("a layer");
            }
        }

        assert_eq!(b.nb_blocks(), 8, "a two by two by two grid");
        assert_eq!(b.nb_nodes(), 27);
        assert_eq!(b.nb_edges(), 54);
        assert_eq!(b.nb_faces(), 36);
    }
}

/// A grid built the way one is built in the viewer — by cutting.
mod cut_grid {
    use gecko_core::ids::CellId;
    use gecko_core::{cube_corners, Blocking};

    /// An edge running along this axis, anywhere in the model.
    fn along(b: &Blocking, axis: usize) -> CellId {
        b.map()
            .iter_edges()
            .find(|e| {
                let (u, v) = b.edge_endpoints(*e);
                let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
                (0..3).all(|k| {
                    if k == axis {
                        (p[k] - q[k]).abs() > 1e-9
                    } else {
                        (p[k] - q[k]).abs() < 1e-9
                    }
                })
            })
            .map(|e| b.edge_tag(e).id)
            .expect("an edge along that axis")
    }

    #[test]
    fn a_grid_made_of_cuts_can_be_taken_apart_again() {
        let mut b = Blocking::new().unwrap();
        b.add_block(cube_corners([0.0, 0.0, 0.0], 4.0)).unwrap();
        for _ in 0..3 {
            for axis in 0..3 {
                b.split_sheet(along(&b, axis), 0.5).unwrap();
            }
        }
        assert_eq!(b.nb_blocks(), 64, "four layers each way");

        // And back down again, one layer at a time.
        for _ in 0..3 {
            for axis in 0..3 {
                b.remove_sheet(along(&b, axis)).expect("a layer");
            }
        }
        assert_eq!(b.nb_blocks(), 1);
        assert_eq!(b.nb_nodes(), 8, "one block, and no stray corners");
    }
}
