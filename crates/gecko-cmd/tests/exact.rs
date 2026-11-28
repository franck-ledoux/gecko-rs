//! An exact geometry in a session: a `.gcad` loaded through the door like a
//! `.msh`, drawn from its tessellation, and projected onto exactly.

use gecko_cmd::{Command, Session};

fn gcad(name: &str) -> String {
    let path = format!(
        "{}/../gecko-geom/tests/data/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn load(s: &mut Session, name: &str) -> String {
    s.apply(Command::LoadCad {
        gcad: gcad(name),
        name: format!("{name}.gcad"),
    })
    .expect("an exact geometry")
    .message
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn a_gcad_comes_in_through_the_same_door_and_says_what_it_is() {
    let mut s = Session::new().unwrap();
    let message = load(&mut s, "B10");
    assert!(message.contains("B10.gcad loaded, exact"), "{message}");
    assert!(
        message.contains("10 vertices, 15 edges, 7 faces"),
        "{message}"
    );
    let g = s.geometry().expect("loaded");
    assert!(g.exact().is_some(), "the exact geometry came with it");
    assert_eq!(
        (g.corners().len(), g.curves().len(), g.surfaces().len()),
        (10, 15, 7)
    );
    assert_eq!(json(&s.stats_json())["geometry"]["surfaces"], 7);
    // And the page gets triangles to draw, as for a .msh.
    assert!(s.geometry_buffer().len() > 1000);
}

#[test]
fn the_json_door_takes_a_gcad_too_and_refuses_what_is_not_one() {
    let mut typed = Session::new().unwrap();
    let mut by_json = Session::new().unwrap();
    load(&mut typed, "cylinder");
    let command =
        serde_json::json!({ "op": "load_cad", "gcad": gcad("cylinder"), "name": "cylinder.gcad" });
    assert!(by_json
        .apply_json(&command.to_string())
        .contains(r#""ok":true"#));
    assert_eq!(typed.geometry_buffer(), by_json.geometry_buffer());

    let before = typed.geometry_buffer();
    let reply = typed.apply_json(r#"{"op":"load_cad","gcad":"{\"format\":\"msh\"}"}"#);
    assert!(reply.contains(r#""ok":false"#), "{reply}");
    assert_eq!(
        typed.geometry_buffer(),
        before,
        "a refused load is not a load of nothing"
    );
}

#[test]
fn the_journal_keeps_the_name_and_the_script_loads_it_again() {
    let mut s = Session::new().unwrap();
    load(&mut s, "cylinder");
    let script = s.script();
    assert!(
        script.contains(r#"gecko.load_cad("cylinder.gcad")"#),
        "{script}"
    );
    assert!(
        !script.contains("\"format\""),
        "the file went into the journal"
    );
}

#[test]
fn a_point_is_found_and_projected_on_the_exact_shapes() {
    let mut s = Session::new().unwrap();
    load(&mut s, "cylinder");
    // Off the side, between the tessellation's nodes: onto the cylinder
    // itself, not the chord of a triangle.
    let a = 0.37f64;
    let got = json(&s.closest_json(
        &serde_json::json!({ "point": [2.0 * a.cos(), 2.0 * a.sin(), 1.1] }).to_string(),
    ));
    let at = &got["surface"]["point"];
    let (x, y, z) = (
        at[0].as_f64().unwrap(),
        at[1].as_f64().unwrap(),
        at[2].as_f64().unwrap(),
    );
    assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-12, "{got}");
    assert!(
        (z - 1.1).abs() < 1e-12 && (y.atan2(x) - a).abs() < 1e-12,
        "{got}"
    );
    assert!((got["surface"]["distance"].as_f64().unwrap() - 1.0).abs() < 1e-12);
}

#[test]
fn undo_takes_an_exact_geometry_back_and_clearing_drops_it() {
    let mut s = Session::new().unwrap();
    load(&mut s, "sphere");
    s.apply(Command::BoxGeometry {
        surfaces: vec![],
        margin: 0.0,
    })
    .unwrap();
    s.apply(Command::Undo).unwrap();
    assert!(
        s.geometry().and_then(|g| g.exact()).is_some(),
        "the box took the geometry with it"
    );
    s.apply(Command::Undo).unwrap();
    assert!(s.geometry().is_none(), "the load was not taken back");
    load(&mut s, "sphere");
    s.apply(Command::ClearGeometry).unwrap();
    assert!(s.geometry().is_none());
}
