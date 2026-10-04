//! One block, built from eight points.
//!
//! The counts are the easy part. What these tests are really for is the two
//! claims the representation rests on: that the map really does give every cell
//! at every dimension an existence of its own, and that the names we hang on
//! those cells mean what they say.

use gecko_core::attributes::{EdgeTag, FaceTag, NodeTag};
use gecko_core::ids::{CellId, CornerKey};
use gecko_core::{Blocking, HEX_FACES};
use honeycomb_core::attributes::AttributeUpdate;
use honeycomb_core::cmap::{OrbitPolicy, NULL_DART_ID};

const UNIT_CUBE: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0],
];

fn one_block() -> Blocking {
    let mut b = Blocking::new().expect("empty map");
    b.add_block(UNIT_CUBE).expect("one hexahedron");
    b
}

#[test]
fn a_block_has_cells_at_every_dimension() {
    let b = one_block();
    // The point of the representation: edges and faces are not derived on
    // demand, they exist, and so they can carry something.
    assert_eq!(b.nb_nodes(), 8);
    assert_eq!(b.nb_edges(), 12);
    assert_eq!(b.nb_faces(), 6);
    assert_eq!(b.nb_blocks(), 1);
}

#[test]
fn every_face_is_a_closed_cycle_of_four_darts() {
    let b = one_block();
    let faces: Vec<_> = b.map().iter_faces().collect();
    assert_eq!(faces.len(), 6);
    for f in faces {
        assert_eq!(b.orbit(OrbitPolicy::Face, f).len(), 4, "face {f}");
    }
    // Twenty-four darts, three at each corner. Dart 0 is the null dart and is
    // counted by `n_darts`, so it comes off.
    assert_eq!(b.map().n_darts() - b.map().n_unused_darts() - 1, 24);
    for v in b.map().iter_vertices() {
        assert_eq!(b.orbit(OrbitPolicy::Vertex, v).len(), 3, "vertex {v}");
    }
}

#[test]
fn the_block_is_closed_in_itself_and_free_on_its_boundary() {
    let b = one_block();
    for d in 1..=24u32 {
        // Every edge of the block is shared by two of its faces: no dart may be
        // left without a beta-2 partner, or the hexahedron has a slit in it.
        assert_ne!(b.map().beta::<2>(d), NULL_DART_ID, "dart {d} has no beta2");
        // And nothing is sewn to a neighbour yet, so every face is on the
        // outside. Joining blocks is a later operation.
        assert_eq!(b.map().beta::<3>(d), NULL_DART_ID, "dart {d} has a beta3");
    }
}

#[test]
fn the_corner_points_come_back_where_they_were_put() {
    let b = one_block();
    let mut found: Vec<[f64; 3]> = b
        .map()
        .iter_vertices()
        .map(|v| b.position(v).expect("every node has a position"))
        .collect();
    found.sort_by(|a, c| a.partial_cmp(c).unwrap());
    let mut expected = UNIT_CUBE;
    expected.sort_by(|a, c| a.partial_cmp(c).unwrap());
    assert_eq!(found, expected);
}

#[test]
fn the_outward_face_convention_is_what_lets_beta2_sew() {
    // Two faces of a block meet along an edge, and they must traverse it in
    // opposite directions or there is nothing for beta 2 to join. That is a
    // property of the constant, so check the constant.
    let mut seen = std::collections::HashMap::new();
    for face in HEX_FACES {
        for slot in 0..4 {
            let directed = (face[slot], face[(slot + 1) % 4]);
            assert!(
                seen.insert(directed, true).is_none(),
                "{directed:?} is traversed twice the same way"
            );
        }
    }
    assert_eq!(seen.len(), 24);
    for (from, to) in seen.keys() {
        assert!(
            seen.contains_key(&(*to, *from)),
            "edge ({from}, {to}) is never traversed back"
        );
    }
}

#[test]
fn every_cell_is_named_and_no_name_is_shared() {
    let b = one_block();

    let node_ids: Vec<_> = b.map().iter_vertices().map(|v| b.node_tag(v).id).collect();
    let edge_ids: Vec<_> = b.map().iter_edges().map(|e| b.edge_tag(e).id).collect();
    let face_ids: Vec<_> = b.map().iter_faces().map(|f| b.face_tag(f).id).collect();
    let block_ids: Vec<_> = b.map().iter_volumes().map(|x| b.block_tag(x).id).collect();

    for ids in [&node_ids, &edge_ids, &face_ids, &block_ids] {
        assert!(ids.iter().all(|i| i.is_some()), "an unnamed cell: {ids:?}");
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "a name is shared: {ids:?}");
    }

    // Corner keys are handed out per node, and are also all distinct here —
    // nothing has been split yet, so no two nodes are copies of one original.
    let mut keys: Vec<CornerKey> = b
        .map()
        .iter_vertices()
        .map(|v| b.node_tag(v).corner_key)
        .collect();
    assert!(keys.iter().all(|k| k.is_some()));
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 8);
}

#[test]
fn names_are_never_reused_between_blocks() {
    let mut b = Blocking::new().unwrap();
    b.add_block(UNIT_CUBE).unwrap();
    let first: Vec<CellId> = b.map().iter_vertices().map(|v| b.node_tag(v).id).collect();

    let shifted = UNIT_CUBE.map(|p| [p[0] + 5.0, p[1], p[2]]);
    b.add_block(shifted).unwrap();

    assert_eq!(b.nb_blocks(), 2);
    assert_eq!(b.nb_nodes(), 16);
    let all: Vec<CellId> = b.map().iter_vertices().map(|v| b.node_tag(v).id).collect();
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 16, "names collided across blocks");
    // And the first block's names were not disturbed by the second's arrival.
    for id in &first {
        assert!(all.contains(id), "{id:?} went missing");
    }
    assert_eq!(b.names_handed_out(), [16, 24, 12, 2]);
}

#[test]
fn assigning_names_twice_hands_out_nothing_new() {
    let mut b = one_block();
    let before = b.names_handed_out();
    b.assign_missing_ids();
    assert_eq!(b.names_handed_out(), before, "a named cell was renamed");
}

// --- the contract with the map ------------------------------------------

#[test]
fn merging_two_cells_keeps_one_existing_name() {
    let a = NodeTag {
        id: CellId::NONE,
        corner_key: CornerKey::NONE,
    };
    let c = NodeTag {
        id: CellId::NONE,
        corner_key: CornerKey::NONE,
    };
    // A merge does not create a cell, so it must not consume a new name.
    assert_eq!(NodeTag::merge(a, c).unwrap(), a);
    assert_eq!(
        EdgeTag::merge(EdgeTag::default(), EdgeTag::default()).unwrap(),
        EdgeTag::default()
    );
}

#[test]
fn splitting_a_node_clears_the_name_and_keeps_the_corner() {
    let mut b = one_block();
    b.assign_missing_ids();
    let v = b.map().iter_vertices().next().unwrap();
    let original = b.node_tag(v);
    assert!(original.id.is_some() && original.corner_key.is_some());

    let (left, right) = NodeTag::split(original).unwrap();
    // Neither copy may go on claiming the original's identity...
    assert!(left.id.is_none() && right.id.is_none());
    // ...but both are still the same corner, and every cell that had it has to
    // go on recognising it.
    assert_eq!(left.corner_key, original.corner_key);
    assert_eq!(right.corner_key, original.corner_key);

    // Same for the cells that carry only a name: a split leaves both sides
    // unnamed, for `assign_missing_ids` to deal with.
    let f = b.map().iter_faces().next().unwrap();
    let named = b.face_tag(f);
    assert!(named.id.is_some());
    let (l, r) = FaceTag::split(named).unwrap();
    assert!(l.id.is_none() && r.id.is_none());
}
