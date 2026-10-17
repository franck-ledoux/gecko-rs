//! Taking a copy of the structure, and putting it back.
//!
//! The point of interest is not that the copy looks the same — a dump would
//! show that much. It is that the copy *is* the same: the cells answer to the
//! same names, and what the structure does next is what it would have done
//! next. An undo built on anything less renumbers the model behind the user's
//! back, and every name they were holding stops meaning what it meant.

use gecko_core::{cube_corners, Blocking};

/// Everything the public API can say about a structure, in a fixed order.
///
/// Sorted, because nothing promises the order cells come out in — and that is
/// the point: two structures that disagree only about iteration order are the
/// same structure, and two that disagree about a name are not.
fn dump(b: &Blocking) -> String {
    let mut nodes: Vec<String> = b
        .nodes_in_full()
        .map(|(id, at, on, frozen)| format!("node {id:?} {at:?} {on:?} frozen={frozen}"))
        .collect();
    nodes.sort();
    let mut edges: Vec<String> = b
        .edges()
        .map(|(edge, a, c, on)| format!("edge {edge:?} {a:?}..{c:?} {on:?}"))
        .collect();
    edges.sort();
    let mut faces: Vec<String> = b
        .faces()
        .map(|(face, nodes)| format!("face {face:?} {nodes:?}"))
        .collect();
    faces.sort();
    let mut blocks: Vec<String> = b
        .block_faces()
        .iter()
        .map(|side| format!("side {:?} of {:?} {:?}", side.face, side.block, side.on))
        .collect();
    blocks.sort();
    let counters = b.names_handed_out();
    [
        nodes.join("\n"),
        edges.join("\n"),
        faces.join("\n"),
        blocks.join("\n"),
        format!("names handed out {counters:?}"),
    ]
    .join("\n")
}

/// Two welded blocks with the middle sheet cut — enough topology that a
/// reconstruction from a description would get something wrong.
fn pair() -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();

    let sides = b.block_faces();
    let mut blocks: Vec<_> = sides.iter().map(|s| s.block).collect();
    blocks.dedup();
    let face_at_x = |which: usize, x: f64| {
        sides
            .iter()
            .filter(|s| s.block == blocks[which])
            .find(|s| {
                s.nodes
                    .iter()
                    .all(|n| (b.position(*n).unwrap()[0] - x).abs() < 1e-9)
            })
            .expect("a face at that x")
            .face
    };
    let (first, second) = (face_at_x(0, 1.0), face_at_x(1, 1.0));
    b.glue_onto(first, second).unwrap();

    let edge = b.edges().next().unwrap().0;
    b.split_sheet(edge, 0.5).unwrap();
    b
}

#[test]
fn a_copy_says_everything_the_original_says() {
    let b = pair();
    let copy = b.snapshot().unwrap();
    assert_eq!(dump(&copy), dump(&b));
}

#[test]
fn the_original_can_move_on_without_the_copy() {
    let mut b = pair();
    let copy = b.snapshot().unwrap();
    let taken = dump(&copy);

    // Cut the first edge there is, which the copy knows nothing about.
    let edge = b.edges().next().unwrap().0;
    b.split_sheet(edge, 0.5).unwrap();

    assert_ne!(dump(&b), taken, "the cut did not change the original");
    assert_eq!(dump(&copy), taken, "the copy moved with it");
}

#[test]
fn a_copy_carries_on_where_the_original_left_off() {
    // The real test of a snapshot: not that it looks right, but that the
    // next operation on it lands exactly where the next operation on the
    // original would have. Dart numbers and name counters both, which is
    // what a model rebuilt from a description of itself cannot promise.
    let mut original = pair();
    let mut copy = original.snapshot().unwrap();

    let grow = |b: &mut Blocking| {
        let id = b.add_block(cube_corners([0.0, 1.0, 0.0], 1.0)).unwrap();
        let edge = b.edges().next().unwrap().0;
        b.split_sheet(edge, 0.25).unwrap();
        id
    };
    let on_original = grow(&mut original);
    let on_copy = grow(&mut copy);

    assert_eq!(on_copy, on_original, "the new block got a different name");
    assert_eq!(dump(&copy), dump(&original));
}

#[test]
fn a_copy_of_a_structure_with_holes_in_its_darts_is_the_same_structure() {
    // Removing a block releases its darts, so the dart list has gaps in it
    // from then on. Those gaps are part of the state: a copy that closed
    // them up would renumber every cell above them.
    let mut b = pair();
    let block = b.block_faces()[0].block;
    b.remove_block(block).unwrap();

    let mut copy = b.snapshot().unwrap();
    assert_eq!(dump(&copy), dump(&b));

    let a = b.add_block(cube_corners([0.0, 0.0, 2.0], 1.0)).unwrap();
    let c = copy.add_block(cube_corners([0.0, 0.0, 2.0], 1.0)).unwrap();
    assert_eq!(c, a);
    assert_eq!(dump(&copy), dump(&b));
}

#[test]
fn what_a_cell_is_classified_on_comes_across() {
    use gecko_core::On;
    let mut b = pair();
    let node = b.nodes().next().unwrap().0;
    b.set_node_class(node, Some(On { dim: 1, tag: 7 })).unwrap();
    b.set_frozen(node, true).unwrap();

    let copy = b.snapshot().unwrap();
    assert_eq!(copy.node_class(node), Some(On { dim: 1, tag: 7 }));
    assert!(copy.is_frozen(node));
    assert_eq!(dump(&copy), dump(&b));
}
