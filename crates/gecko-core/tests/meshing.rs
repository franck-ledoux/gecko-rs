//! Discretising a block structure.

use gecko_core::blocking::HEX_FACES;
use gecko_core::{cube_corners, Blocking};

fn cube() -> Blocking {
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b
}

/// Six times the volume of the hexahedron, by the divergence theorem over
/// its six faces. Positive when the corners are wound the way VTK expects.
fn six_volumes(p: &[[f64; 3]; 8]) -> f64 {
    let mut sum = 0.0;
    for face in HEX_FACES {
        // Each quadrangle as two triangles, and each triangle's cone back
        // to the origin.
        for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
            let (x, y, z) = (p[face[a]], p[face[b]], p[face[c]]);
            // The face normals run **outward**, and the volume of a solid
            // is the sum of the cones on its outward faces.
            sum += x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0])
                + x[2] * (y[0] * z[1] - y[1] * z[0]);
        }
    }
    sum
}

#[test]
fn a_block_comes_back_as_a_hexahedron_the_right_way_round() {
    let b = cube();
    let hexes = b.hexes();
    assert_eq!(hexes.len(), 1);
    let hex = &hexes[0];

    let at = hex.corners.map(|v| b.position(v).unwrap());
    // Not the corners it was built from in that order — any face may serve
    // as the bottom — but a unit cube all the same, and wound outward.
    // Positive, which is the whole point: the eight corners are wound the
    // way VTK reads a hexahedron, so the cell has a volume rather than a
    // hole where its volume should be.
    let volume = six_volumes(&at) / 6.0;
    assert!((volume - 1.0).abs() < 1e-9, "not a unit cube: {volume}");

    // Every corner of the cube, once.
    let mut seen: Vec<[f64; 3]> = at.to_vec();
    seen.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut want = cube_corners([0.0, 0.0, 0.0], 1.0).to_vec();
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(seen, want);

    // And the cells it names really are its cells.
    assert_eq!(hex.edges.len(), 12);
    let once = |mut v: Vec<u32>| {
        let before = v.len();
        v.sort_unstable();
        v.dedup();
        v.len() == before
    };
    assert!(once(hex.edges.to_vec()), "an edge was named twice");
    assert!(once(hex.faces.to_vec()), "a face was named twice");
}

/// No projection: the mesher is given a geometry that says nothing.
fn flat(_: gecko_core::On, _: [f64; 3]) -> Option<[f64; 3]> {
    None
}

#[test]
fn one_cube_becomes_a_grid_of_hexahedra() {
    let mesh = cube().mesh(4, flat);
    assert_eq!(mesh.hexes.len(), 4 * 4 * 4);
    assert_eq!(mesh.points.len(), 5 * 5 * 5);

    // Every point of the unit cube's lattice, once.
    let mut seen: Vec<[i64; 3]> = mesh
        .points
        .iter()
        .map(|p| p.map(|x| (x * 4.0).round() as i64))
        .collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 125, "points landed on top of each other");
    for p in &mesh.points {
        for x in p {
            assert!((0.0..=1.0).contains(x), "{p:?} is outside the cube");
            assert!(
                (x * 4.0 - (x * 4.0).round()).abs() < 1e-12,
                "{p:?} is off the lattice"
            );
        }
    }

    // And every hexahedron is the right way round and the right size.
    for hex in &mesh.hexes {
        let at = hex.map(|n| mesh.points[n as usize]);
        assert!((six_volumes(&at) / 6.0 - 0.25f64.powi(3)).abs() < 1e-12);
    }
}

#[test]
fn the_number_of_segments_belongs_to_the_sheet() {
    let mut b = cube();
    let edge = b.edges().next().unwrap().0;
    b.set_sheet_segments(edge, 3).unwrap();
    assert_eq!(b.sheet_segments(edge).unwrap(), 3);

    // One direction of three, two of the default: 3 × 2 × 2 hexahedra.
    let mesh = b.mesh(2, flat);
    assert_eq!(mesh.hexes.len(), 3 * 2 * 2);
    assert_eq!(mesh.points.len(), 4 * 3 * 3);
}

#[test]
fn two_glued_blocks_share_the_nodes_on_the_face_between_them() {
    // The whole point of naming a mesh node by the cell it lies in: a seam
    // down the middle is what a solver finds and a picture does not.
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
    b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
    let sides = b.block_faces();
    let mut names: Vec<_> = sides.iter().map(|s| s.block).collect();
    names.dedup();
    let face_at = |which: usize, x: f64| {
        sides
            .iter()
            .filter(|s| s.block == names[which])
            .find(|s| {
                s.nodes
                    .iter()
                    .all(|n| (b.position(*n).unwrap()[0] - x).abs() < 1e-9)
            })
            .expect("a face at that x")
            .face
    };
    let (first, second) = (face_at(0, 1.0), face_at(1, 1.0));
    b.glue_onto(first, second).unwrap();

    let mesh = b.mesh(3, flat);
    assert_eq!(mesh.hexes.len(), 2 * 3 * 3 * 3);
    // 4 × 4 × 4 apiece, less the 16 of the shared face counted twice.
    assert_eq!(mesh.points.len(), 2 * 64 - 16);

    // Said again from the other side: no two points are in the same place.
    let mut seen: Vec<[i64; 3]> = mesh
        .points
        .iter()
        .map(|p| p.map(|x| (x * 3.0).round() as i64))
        .collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), mesh.points.len());
}

#[test]
fn the_outside_of_the_mesh_is_its_skin() {
    let mesh = cube().mesh(4, flat);
    let (quads, blocks) = mesh.skin();
    // Six faces of sixteen, and nothing from inside.
    assert_eq!(quads.len(), 6 * 16);
    assert_eq!(blocks.len(), quads.len());
    for quad in &quads {
        let at = quad.map(|n| mesh.points[n as usize]);
        // Every quadrangle of the skin is flat against one of the six
        // planes of the cube.
        let flatness = (0..3)
            .map(|k| {
                let v: Vec<f64> = at.iter().map(|p| p[k]).collect();
                v.iter().cloned().fold(f64::MIN, f64::max)
                    - v.iter().cloned().fold(f64::MAX, f64::min)
            })
            .filter(|d| *d < 1e-12)
            .count();
        assert_eq!(flatness, 1, "not on a face of the cube: {at:?}");
    }
}

#[test]
fn a_blocking_that_has_been_cut_meshes_too() {
    // The frame is recovered from the map rather than remembered, which is
    // the only way it can survive surgery.
    let mut b = cube();
    let edge = b.edges().next().unwrap().0;
    b.split_sheet(edge, 0.25).unwrap();

    let mesh = b.mesh(2, flat);
    assert_eq!(mesh.hexes.len(), 2 * 2 * 2 * 2, "two blocks of eight");
    // Both halves keep the cut sheet's number, so that direction now has
    // four segments across the two blocks: 5 × 3 × 3 points.
    assert_eq!(mesh.points.len(), 5 * 3 * 3);
    for hex in &mesh.hexes {
        let at = hex.map(|n| mesh.points[n as usize]);
        assert!(six_volumes(&at) > 0.0, "a hexahedron is inside out");
    }
}

#[test]
fn a_cell_on_a_curve_takes_the_mesh_with_it() {
    // What the projection is for: the points along an edge are put on the
    // straight line and then moved onto what the edge follows, and the
    // face above it is filled from where they ended up.
    use gecko_core::On;
    let mut b = cube();

    // Curve 7 is the quarter circle of radius 1 about the origin in z = 0;
    // surface 9 is the plane z = 0 itself, which moves nothing.
    let onto = |on: On, p: [f64; 3]| match on.tag {
        7 => {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt().max(1e-12);
            Some([p[0] / r, p[1] / r, 0.0])
        }
        _ => None,
    };

    // The edge from (1,0,0) to (1,1,0) bulges out onto the circle.
    let edge = b
        .edges()
        .find(|(_, a, c, _)| {
            let ends = [*a, *c].map(|n| b.position(b.node_by_id(n).unwrap()).unwrap());
            ends.iter().all(|p| p[0] == 1.0 && p[2] == 0.0)
        })
        .expect("the edge at x = 1, z = 0")
        .0;
    b.set_edge_class(edge, Some(On { dim: 1, tag: 7 })).unwrap();

    let mesh = b.mesh(4, onto);
    // Every point that came out of that edge is on the circle, and at
    // least one of them is off the straight line it started on.
    let on_circle: Vec<&[f64; 3]> = mesh
        .points
        .iter()
        .filter(|p| p[2] == 0.0 && (p[0] * p[0] + p[1] * p[1] - 1.0).abs() < 1e-12)
        .collect();
    assert!(
        on_circle.len() >= 5,
        "{} points on the circle",
        on_circle.len()
    );
    assert!(
        on_circle.iter().any(|p| p[0] < 0.999),
        "nothing left the straight line",
    );
}
