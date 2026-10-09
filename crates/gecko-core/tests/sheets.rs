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
