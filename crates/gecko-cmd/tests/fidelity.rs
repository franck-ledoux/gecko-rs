//! How faithfully a mesh follows its geometry: the mesh to the surfaces,
//! the surfaces to the mesh, and every curve and corner followed.

use gecko_cmd::{Command, Session};

/// A box 2 × 1 × 3 as a gmsh model: eight corners, twelve curves, six
/// faces of two triangles each.
fn box_msh() -> String {
    box_msh_split(false)
}

/// The same, with the top split in two along the line x = 1.3, if `split`:
/// two flat surfaces meeting along a seam a merge takes away — and one no
/// line of a mesh of the box in two runs along.
fn box_msh_split(split: bool) -> String {
    let p = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 3.0],
        [2.0, 0.0, 3.0],
        [2.0, 1.0, 3.0],
        [0.0, 1.0, 3.0],
        [1.3, 0.0, 3.0],
        [1.3, 1.0, 3.0],
    ];
    let lines = [
        [1, 2],
        [2, 3],
        [3, 4],
        [4, 1],
        [5, 6],
        [6, 7],
        [7, 8],
        [8, 5],
        [1, 5],
        [2, 6],
        [3, 7],
        [4, 8],
    ];
    let faces = [
        [1, 2, 3, 4],
        [5, 6, 7, 8],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 4, 8, 7],
        [4, 1, 5, 8],
    ];
    let mut out = format!(
        "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n{}\n",
        p.len()
    );
    for (i, q) in p.iter().enumerate() {
        out += &format!("{} {} {} {}\n", i + 1, q[0], q[1], q[2]);
    }
    out += "$EndNodes\n$Elements\n";
    let mut elements = Vec::new();
    for i in 1..=8 {
        elements.push(format!("15 2 0 {i} {i}"));
    }
    for (k, [a, b]) in lines.iter().enumerate() {
        // The two long sides of the top cut where the seam meets them.
        match (split, k + 1) {
            (true, 5) => elements.extend(["1 2 0 5 5 9".into(), "1 2 0 5 9 6".into()]),
            (true, 7) => elements.extend(["1 2 0 7 7 10".into(), "1 2 0 7 10 8".into()]),
            _ => elements.push(format!("1 2 0 {} {a} {b}", k + 1)),
        }
    }
    if split {
        elements.push("1 2 0 13 9 10".to_string());
    }
    for (k, [a, b, c, d]) in faces.iter().enumerate() {
        if split && k == 1 {
            // The top as two surfaces: 2 below x = 1.3, 7 above.
            elements.extend([
                "2 2 0 2 5 9 10".to_string(),
                "2 2 0 2 5 10 8".to_string(),
                "2 2 0 7 9 6 7".to_string(),
                "2 2 0 7 9 7 10".to_string(),
            ]);
            continue;
        }
        elements.push(format!("2 2 0 {} {a} {b} {c}", k + 1));
        elements.push(format!("2 2 0 {} {a} {c} {d}", k + 1));
    }
    out += &format!("{}\n", elements.len());
    for (i, e) in elements.iter().enumerate() {
        out += &format!("{} {e}\n", i + 1);
    }
    out + "$EndElements\n"
}

fn meshed_box(margin: f64, classify: bool) -> Session {
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: box_msh(),
        name: String::new(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin,
    })
    .unwrap();
    if classify {
        s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    }
    s.apply(Command::Mesh { segments: 4 }).unwrap();
    s
}

fn fidelity(s: &Session, share: f64) -> serde_json::Value {
    serde_json::from_str(&s.fidelity_json(share)).unwrap()
}

#[test]
fn nothing_to_check_without_a_mesh_and_a_geometry() {
    let mut s = Session::new().unwrap();
    s.apply(Command::AddBlock {
        corners: gecko_core::cube_corners([0.0; 3], 1.0),
    })
    .unwrap();
    s.apply(Command::Mesh { segments: 2 }).unwrap();
    assert_eq!(s.fidelity_json(0.25), "null", "a mesh, but no geometry");
}

#[test]
fn a_box_put_on_the_model_follows_every_curve_and_corner_of_it() {
    let s = meshed_box(0.0, true);
    let f = fidelity(&s, 0.25);
    assert!(f["skin"]["max"].as_f64().unwrap() < 1e-9, "{f}");
    assert!(f["geometry"]["max"].as_f64().unwrap() < 1e-9, "{f}");
    assert_eq!(f["curves"]["checked"], 12);
    assert_eq!(f["curves"]["followed"], 12);
    assert_eq!(f["corners"]["checked"], 8);
    assert_eq!(f["corners"]["missed"].as_array().unwrap().len(), 0);
    assert!(f["worst"].is_null());
}

#[test]
fn a_box_standing_clear_of_the_model_misses_all_of_it() {
    let s = meshed_box(0.3, false);
    let f = fidelity(&s, 0.25);
    // The skin off the model — most of it beyond the tolerance, all but
    // the thin side, where 30% of a half-side is only 0.15 — and every
    // curve and corner of the model away from the mesh.
    let (beyond, nodes) = (
        f["skin"]["beyond"].as_u64().unwrap(),
        f["skin"]["nodes"].as_u64().unwrap(),
    );
    assert!(beyond * 2 > nodes, "{beyond} of {nodes}");
    assert!(f["skin"]["max"].as_f64().unwrap() > 0.4);
    assert!(f["geometry"]["beyond"].as_u64().unwrap() > 0);
    assert_eq!(f["curves"]["followed"], 0);
    assert_eq!(f["corners"]["missed"].as_array().unwrap().len(), 8);
    assert!(f["worst"]["distance"].as_f64().unwrap() > 0.1);
    assert!(!f["overlay"]["segments"].as_array().unwrap().is_empty());
    assert!(!f["overlay"]["points"].as_array().unwrap().is_empty());

    // A tolerance loose enough takes the corners back: the gaps are the
    // same, only what counts as one has changed.
    let loose = fidelity(&s, 50.0);
    assert_eq!(loose["corners"]["missed"].as_array().unwrap().len(), 0);
    assert!(loose["worst"].is_null());
}

#[test]
fn a_seam_a_merge_took_away_is_not_a_curve_to_follow() {
    let mut s = Session::new().unwrap();
    s.apply(Command::LoadGeometry {
        msh: box_msh_split(true),
        name: String::new(),
    })
    .unwrap();
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.0,
    })
    .unwrap();
    s.apply(Command::Classify { tolerance: 0.02 }).unwrap();
    s.apply(Command::Mesh { segments: 2 }).unwrap();
    // The line across the top is a curve of the model, and nothing of
    // the mesh runs along it.
    let f = fidelity(&s, 0.25);
    assert_eq!(f["curves"]["checked"], 13);
    assert_eq!(f["curves"]["missed"][0]["tag"], 13);

    // Merged, the two halves of the top are one surface and the line is
    // inside it: not a feature, and not checked.
    s.apply(Command::MergeSmooth { angle: 10.0 }).unwrap();
    s.apply(Command::Mesh { segments: 2 }).unwrap();
    let f = fidelity(&s, 0.25);
    assert_eq!(f["curves"]["checked"], 12, "{f}");
    assert_eq!(f["curves"]["followed"], 12);
}
