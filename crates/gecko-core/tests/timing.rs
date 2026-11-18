//! Where the time goes, measured rather than guessed.
//!
//! Ignored by default: it is a measurement and not an assertion.
//! `cargo test --release -p gecko-core --test timing -- --ignored --nocapture`
//!
//! The grids are cut out of one block, which is how a blocking is actually
//! built, and the operations timed after that are the ones the page asks
//! for while somebody is working: a redraw reads `block_faces`, a cut and a
//! removal are what the keys do, and the mesher is what `E` does.

use std::time::Instant;

use gecko_core::{cube_corners, Blocking, Smoothing};

fn grid(n: usize) -> (Blocking, f64) {
    let start = Instant::now();
    let mut b = Blocking::new().unwrap();
    b.add_block(cube_corners([0.0; 3], n as f64)).unwrap();
    for axis in 0..3 {
        for k in 1..n {
            let want = (n - k) as f64;
            let edge = b
                .edges_in_full()
                .find(|(_, ends, _, _)| {
                    let (p, q) = (
                        b.position(ends[0]).expect("an end"),
                        b.position(ends[1]).expect("an end"),
                    );
                    (0..3).all(|i| {
                        if i == axis {
                            (p[i] - q[i]).abs() > want - 1e-9
                        } else {
                            (p[i] - q[i]).abs() < 1e-9
                        }
                    })
                })
                .map(|(edge, ..)| edge)
                .expect("an edge still running the whole way");
            b.split_sheet(edge, 1.0 / (n - k + 1) as f64).unwrap();
        }
    }
    (b, start.elapsed().as_secs_f64() * 1e3)
}

fn took(what: &str, blocks: usize, mut run: impl FnMut()) {
    let start = Instant::now();
    run();
    let ms = start.elapsed().as_secs_f64() * 1e3;
    println!(
        "    {what:<22} {ms:8.1} ms   {:6.3} ms a block",
        ms / blocks as f64
    );
}

#[test]
#[ignore = "a measurement, not an assertion"]
fn where_the_time_goes() {
    for n in [12, 20] {
        let (b, built) = grid(n);
        let blocks = b.nb_blocks();
        println!("{n}x{n}x{n}: {blocks} blocks");
        println!(
            "    {:<22} {built:8.1} ms   {:6.3} ms a block",
            "built by cuts",
            built / blocks as f64
        );

        took("census", blocks, || {
            std::hint::black_box(b.census());
        });
        took("block_faces", blocks, || {
            std::hint::black_box(b.block_faces());
        });
        took("hexes", blocks, || {
            std::hint::black_box(b.hexes());
        });
        took("nodes_in_full", blocks, || {
            std::hint::black_box(b.nodes_in_full().collect::<Vec<_>>());
        });
        // What the undo stack pays on **every** command.
        took("snapshot", blocks, || {
            std::hint::black_box(b.snapshot().unwrap());
        });

        // One more cut across the whole grid, and the layer taken back out.
        let mut c = b.snapshot().unwrap();
        let edge = c
            .edges_in_full()
            .find(|(_, ends, _, _)| {
                let (p, q) = (
                    c.position(ends[0]).expect("an end"),
                    c.position(ends[1]).expect("an end"),
                );
                (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9
            })
            .map(|(edge, ..)| edge)
            .expect("an edge along z");
        took("split_sheet", blocks, || {
            c.split_sheet(edge, 0.5).unwrap();
        });

        let mut d = b.snapshot().unwrap();
        let layer = d
            .edges_in_full()
            .find(|(_, ends, _, _)| {
                let (p, q) = (
                    d.position(ends[0]).expect("an end"),
                    d.position(ends[1]).expect("an end"),
                );
                (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9
            })
            .map(|(edge, ..)| edge)
            .expect("an edge along z");
        took("remove_sheet", blocks, || {
            d.remove_sheet(layer).unwrap();
        });

        // The same without closing the gap, which skips every weld: the
        // difference between the two is what the welding costs.
        let mut g = b.snapshot().unwrap();
        took("remove_sheet, gap left", blocks, || {
            g.remove_sheet_leaving_a_gap(layer).unwrap();
        });

        let mut e = b.snapshot().unwrap();
        took("smooth, one round", blocks, || {
            e.smooth(
                Smoothing {
                    rounds: 1,
                    share: 0.5,
                    held: &[],
                },
                |_, _| None,
            );
        });

        // The two the pointer drives: a face of the surface raised, and a
        // whole side of the model drawn in and a layer laid into it.
        let mut h = b.snapshot().unwrap();
        let surface: Vec<(gecko_core::CellId, gecko_core::CellId)> = {
            let sides = h.block_faces();
            let once: Vec<_> = sides
                .iter()
                .filter(|s| sides.iter().filter(|o| o.face == s.face).count() == 1)
                .collect();
            // The whole top of the grid: n x n faces, all on the surface.
            let top = n as f64;
            once.iter()
                .filter(|s| {
                    s.nodes
                        .iter()
                        .all(|c| (h.position(*c).expect("a corner")[2] - top).abs() < 1e-9)
                })
                .map(|s| (s.face, s.block))
                .collect()
        };
        took("extrude_faces, one side", blocks, || {
            h.extrude_faces(&surface, 0.3).unwrap();
        });

        let mut i = b.snapshot().unwrap();
        took("pillow, one side", blocks, || {
            i.pillow(&surface).unwrap();
        });

        took("mesh, 2 segments", blocks, || {
            std::hint::black_box(b.mesh(2, |_, _| None));
        });
        println!();
    }
}
