//! Names, and the walks that hand them out.
//!
//! Every cell of a blocking carries a [`CellId`] — the name a script says —
//! and the kernel hands those out by looking for cells that have none. Two
//! things have to hold for that to stay honest, and both are cheap to break
//! by accident while making the looking faster.
//!
//! One: the kernel's own walk of the map has to find exactly the cells the
//! map itself would, under the names the map gives them. Two: an operation
//! that names only what it touched has to have touched everything that was
//! left unnamed.

use gecko_core::ids::CellId;
use gecko_core::{cube_corners, Blocking};
use std::collections::HashMap;

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

/// `n` by `n` by `n` unit cubes, every touching pair glued.
fn grid(n: usize) -> Blocking {
    let mut b = Blocking::new().unwrap();
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                b.add_block(cube_corners([i as f64, j as f64, k as f64], 1.0))
                    .unwrap();
            }
        }
    }
    weld_everything(&mut b);
    b
}

/// One face lying flat at this x, and the block it is on — the pair a
/// click gives. A chord entered through it runs the length of the grid.
fn face_at_x(b: &Blocking, x: f64) -> (CellId, CellId) {
    let found = b
        .block_faces()
        .into_iter()
        .find(|s| {
            s.nodes
                .iter()
                .all(|n| (b.position(*n).unwrap()[0] - x).abs() < 1e-9)
        })
        .expect("a face at that x");
    (found.face, found.block)
}

/// Every cell the kernel walks is a cell the map would name the same way.
///
/// The kernel goes up the darts once and marks each orbit off as it walks
/// it; the map asks, for every dart, which cell that dart is in. The two
/// have to agree exactly — in which cells, and in what each is called —
/// because the second is where the names in a script came from.
#[test]
fn the_kernels_walk_finds_what_the_map_would() {
    let mut b = grid(2);
    // Cut and pillow first: that merges and splits orbits, so the darts of
    // one cell are no longer a tidy run of consecutive numbers.
    let (face, block) = face_at_x(&b, 0.0);
    b.pillow_chord(face, block).unwrap();

    assert_eq!(
        b.map_vertices(),
        b.map().iter_vertices().collect::<Vec<_>>()
    );
    assert_eq!(b.map_edges(), b.map().iter_edges().collect::<Vec<_>>());
    assert_eq!(b.map_faces(), b.map().iter_faces().collect::<Vec<_>>());
    assert_eq!(b.map_volumes(), b.map().iter_volumes().collect::<Vec<_>>());
}

/// Nothing is left nameless by a pillow.
///
/// Laying a block and sewing a face each name only the cells of the one
/// orbit they touched, rather than sweeping the map. This is the test that
/// the orbit they look in is wide enough: a cell with no name is a cell no
/// script can ever mention again.
#[test]
fn a_pillow_leaves_nothing_without_a_name() {
    let mut b = grid(3);
    let (face, block) = face_at_x(&b, 0.0);
    b.pillow_chord(face, block).unwrap();

    let nameless = |ids: Vec<u32>, named: &dyn Fn(u32) -> CellId| {
        ids.into_iter().filter(|c| named(*c).is_none()).count()
    };
    assert_eq!(
        nameless(b.map_vertices(), &|v| b.node_tag(v).id),
        0,
        "nodes"
    );
    assert_eq!(nameless(b.map_edges(), &|e| b.edge_tag(e).id), 0, "edges");
    assert_eq!(nameless(b.map_faces(), &|f| b.face_tag(f).id), 0, "faces");
    assert_eq!(
        nameless(b.map_volumes(), &|v| b.block_tag(v).id),
        0,
        "blocks"
    );
}

/// Which node is called what, pinned.
///
/// A name is the only handle a script has on a node: `gecko.move_node(21,
/// …)` means *that* corner and no other. Nothing else in the suite says so
/// — two sessions are compared against each other, which agree happily on
/// a numbering that has shifted under both — so a change to the walk that
/// hands names out can renumber every script ever written and every test
/// still pass. Hence a table, read against where each node actually is.
///
/// Two cubes glued along a face, and then the far face of the first
/// pillowed: that exercises laying a block, sewing a face, and the merge
/// of nodes a glue makes, which is everything that names cells.
#[test]
fn the_names_fall_where_they_always_have() {
    let mut b = Blocking::new().unwrap();
    for i in 0..2 {
        b.add_block(cube_corners([i as f64, 0.0, 0.0], 1.0))
            .unwrap();
    }
    weld_everything(&mut b);
    let (face, block) = face_at_x(&b, 0.0);
    b.pillow(&[(face, block)]).unwrap();

    let mut got: Vec<(u32, [i64; 3])> = b
        .nodes_in_full()
        .map(|(id, at, _, _)| {
            (
                id.get().expect("a named node"),
                at.map(|x| (x * 1000.0).round() as i64),
            )
        })
        .collect();
    got.sort();

    // The first block's own four corners, drawn in a third of the way by
    // the pillow; the second block's four; and the new layer's four, left
    // standing where the first block's used to be.
    let want: Vec<(u32, [i64; 3])> = vec![
        (0, [333, 0, 0]),
        (1, [333, 1000, 0]),
        (2, [1000, 1000, 0]),
        (3, [1000, 0, 0]),
        (4, [333, 0, 1000]),
        (5, [1000, 0, 1000]),
        (6, [1000, 1000, 1000]),
        (7, [333, 1000, 1000]),
        (10, [2000, 1000, 0]),
        (11, [2000, 0, 0]),
        (13, [2000, 0, 1000]),
        (14, [2000, 1000, 1000]),
        (20, [0, 1000, 0]),
        (21, [0, 0, 0]),
        (22, [0, 0, 1000]),
        (23, [0, 1000, 1000]),
    ];
    assert_eq!(got, want);
}
