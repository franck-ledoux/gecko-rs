//! Taking a block away.
//!
//! The interesting case is not the lone block — it is the one glued to
//! something. Removing it has to give the neighbor its own cells back, which
//! is the split that `CornerKey` exists for, and leave nothing of the block
//! behind for the next operation to trip over.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking, BlockingError};

fn cells(b: &Blocking) -> (usize, usize, usize, usize) {
    (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks())
}

fn blocks(b: &Blocking) -> Vec<CellId> {
    let mut names: Vec<_> = b.block_faces().iter().map(|s| s.block).collect();
    names.dedup();
    names
}

/// The face of `block` lying flat at this x, if it has one.
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
    b.glue_onto(first, second).unwrap();
    b
}

#[test]
fn removing_the_only_block_empties_the_map() {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let name = blocks(&b)[0];

    b.remove_block(name).expect("the block exists");
    assert_eq!(cells(&b), (0, 0, 0, 0));
    assert_eq!(b.block_faces().len(), 0);
}

#[test]
fn the_map_can_be_used_again_afterwards() {
    // The darts have to come back to the pool properly, or the next block is
    // built on top of something.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.remove_block(blocks(&b)[0]).unwrap();
    b.add_block(cube_corners([5.0, 0.0, 0.0], 2.0)).unwrap();
    assert_eq!(cells(&b), (8, 12, 6, 1));

    let corners: Vec<_> = b.nodes().map(|(_, p)| p[0]).collect();
    assert!(
        corners.iter().all(|x| *x == 5.0 || *x == 7.0),
        "{corners:?}"
    );
}

#[test]
fn removing_a_glued_block_leaves_the_neighbor_whole() {
    let mut b = glued();
    assert_eq!(cells(&b), (12, 20, 11, 2));
    let names = blocks(&b);

    b.remove_block(names[1]).expect("the second block");

    // A closed hexahedron again, with the four shared corners back to being
    // the first block's own.
    assert_eq!(cells(&b), (8, 12, 6, 1));
    assert_eq!(b.block_faces().len(), 6);
    // And it kept its geometry: the unit cube, not a face of it.
    let mut xs: Vec<f64> = b.nodes().map(|(_, p)| p[0]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    assert_eq!(xs, vec![0.0, 1.0]);
}

#[test]
fn the_neighbor_can_be_glued_again() {
    // The real test of "whole": the face that was internal has to be back on
    // the boundary, or nothing can ever be attached there again.
    let mut b = glued();
    let names = blocks(&b);
    b.remove_block(names[1]).unwrap();

    b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
    let fresh = blocks(&b);
    let (first, second) = (
        seam(&b, fresh[0], 1.0).unwrap(),
        seam(&b, fresh[1], 1.0).unwrap(),
    );
    b.glue_onto(first, second).expect("the seam is free again");
    assert_eq!(cells(&b), (12, 20, 11, 2));
}

#[test]
fn removing_the_middle_of_three_leaves_two() {
    let mut b = Blocking::new().unwrap();
    for x in [0.0, 1.0, 2.0] {
        b.add_block(cube_corners([x, 0.0, 0.0], 1.0)).unwrap();
    }
    let names = blocks(&b);
    let (a, c) = (
        seam(&b, names[0], 1.0).unwrap(),
        seam(&b, names[1], 1.0).unwrap(),
    );
    b.glue_onto(a, c).unwrap();
    let names = blocks(&b);
    let (a, c) = (
        seam(&b, names[1], 2.0).unwrap(),
        seam(&b, names[2], 2.0).unwrap(),
    );
    b.glue_onto(a, c).unwrap();
    assert_eq!(cells(&b), (16, 28, 16, 3));

    // Take the one in the middle, which is glued on both sides.
    let names = blocks(&b);
    b.remove_block(names[1]).expect("the middle block");

    // Two separate cubes, each closed, no longer touching anything.
    assert_eq!(cells(&b), (16, 24, 12, 2));
}

#[test]
fn a_name_no_block_answers_to_is_refused_and_changes_nothing() {
    let mut b = glued();
    let before = cells(&b);
    assert!(matches!(
        b.remove_block(CellId::from_raw(9999)),
        Err(BlockingError::NoSuchBlock(_))
    ));
    assert!(matches!(
        b.remove_block(CellId::NONE),
        Err(BlockingError::NoSuchBlock(_))
    ));
    assert_eq!(cells(&b), before);
}

#[test]
fn a_name_is_found_again_after_the_map_has_been_rearranged() {
    // Finding a cell by name starts from a hint — where that name was last
    // seen — and falls back to a walk. The hint is written where names are
    // handed out and nowhere else, so an operation that moves cells about
    // leaves stale entries behind on purpose: what makes that safe is that
    // every answer is checked against the cell's own tag before it is
    // given.
    //
    // Why it exists: without it, every lookup walked the whole map, and a
    // cut of a hundred-block model spent half a second doing it once per
    // node it made.
    let mut b = Blocking::new().expect("a map");
    let first = b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    let second = b.add_block(cube_corners([2.0, 0.0, 0.0], 1.0)).unwrap();

    // Names of the second block's cells, taken before the first goes.
    let nodes: Vec<CellId> = b.nodes().map(|(id, _)| id).collect();
    let kept: Vec<CellId> = b
        .nodes()
        .filter(|(_, p)| p[0] > 1.5)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(kept.len(), 8, "the second block's eight corners");

    b.remove_block(first).expect("the first goes");

    // Every name that is still there answers, and with the right cell.
    for node in &kept {
        let found = b.node_by_id(*node).expect("still there");
        assert_eq!(b.node_tag(found).id, *node, "a hint gave the wrong cell");
    }
    // And the ones that went answer with nothing rather than with whatever
    // now sits where they used to.
    let gone: Vec<CellId> = nodes.into_iter().filter(|n| !kept.contains(n)).collect();
    assert_eq!(gone.len(), 8);
    for node in gone {
        assert!(b.node_by_id(node).is_none(), "{node:?} is gone");
    }
    assert!(b.block_by_id(first).is_none());
    assert!(b.block_by_id(second).is_some());
}
