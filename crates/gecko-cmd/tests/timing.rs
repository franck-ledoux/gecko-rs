//! What the page pays to draw the model, measured.
//!
//! Ignored by default: a measurement, not an assertion.
//! `cargo test --release -p gecko-cmd --test timing -- --ignored --nocapture`
//!
//! These are the calls a drag makes. `nodes_buffer` is the light path — it
//! goes out on every pointer move — and `view_buffer` is the whole scene,
//! which goes out when the topology changes.

use std::time::Instant;

use gecko_cmd::{Command, Session};

fn grid(n: usize) -> Session {
    let mut s = Session::new().unwrap();
    s.apply(Command::CreateCube {
        origin: [0.0; 3],
        size: n as f64,
    })
    .unwrap();
    for axis in 0..3 {
        for k in 1..n {
            let want = (n - k) as f64;
            let edges: Vec<serde_json::Value> =
                serde_json::from_str(&s.edges_json()).expect("the edges");
            let nodes: Vec<serde_json::Value> =
                serde_json::from_str(&s.nodes_json()).expect("the nodes");
            let at = |id: u64| -> [f64; 3] {
                let node = nodes
                    .iter()
                    .find(|n| n["id"].as_u64() == Some(id))
                    .expect("a node");
                let p = node["pos"].as_array().expect("a place");
                [0, 1, 2].map(|k| p[k].as_f64().expect("a number"))
            };
            let edge = edges
                .iter()
                .find(|e| {
                    let ends = e["nodes"].as_array().expect("two ends");
                    let (p, q) = (
                        at(ends[0].as_u64().expect("an id")),
                        at(ends[1].as_u64().expect("an id")),
                    );
                    (0..3).all(|i| {
                        if i == axis {
                            (p[i] - q[i]).abs() > want - 1e-9
                        } else {
                            (p[i] - q[i]).abs() < 1e-9
                        }
                    })
                })
                .expect("an edge still running the whole way");
            s.apply(Command::SplitSheet {
                edge: edge["id"].as_u64().expect("an id") as u32,
                at: 1.0 / (n - k + 1) as f64,
            })
            .unwrap();
        }
    }
    s
}

fn took(what: &str, blocks: usize, mut run: impl FnMut() -> usize) {
    let start = Instant::now();
    let bytes = run();
    let ms = start.elapsed().as_secs_f64() * 1e3;
    println!(
        "    {what:<18} {ms:8.3} ms   {:7.4} ms a block   {:7} B",
        ms / blocks as f64,
        bytes
    );
}

#[test]
#[ignore = "a measurement, not an assertion"]
fn what_a_redraw_costs() {
    for n in [8, 12] {
        let s = grid(n);
        let blocks = serde_json::from_str::<serde_json::Value>(&s.stats_json()).unwrap()["blocks"]
            .as_u64()
            .unwrap() as usize;
        println!("{n}x{n}x{n}: {blocks} blocks");
        took("view_buffer", blocks, || s.view_buffer().len());
        took("nodes_buffer", blocks, || s.nodes_buffer().len());
        took("faces_json", blocks, || s.faces_json().len());
        took("stats_json", blocks, || s.stats_json().len());
        println!();
    }
}
