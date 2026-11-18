//! What the structure costs, measured rather than estimated.
//!
//! A counting allocator round the whole process and grids of blocks cut out
//! of one by `split_sheet`, which is the measurement the design note's
//! table was made from — same grids, same method, so the numbers could be
//! put beside the combinatorial map's while it was still here. They were:
//! 19 414 B a block against 550 at 12³, which is the whole reason the
//! representation was replaced.
//!
//! Ignored by default: it is a measurement and not an assertion, and it
//! wants the whole process to itself. `cargo test -p gecko-core --test
//! footprint -- --ignored --nocapture`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

static LIVE: AtomicIsize = AtomicIsize::new(0);

struct Counted;

unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTED: Counted = Counted;

/// A grid of `n` blocks an axis, cut out of one.
fn grid(n: usize) -> gecko_core::Blocking {
    let mut b = gecko_core::Blocking::new().unwrap();
    b.add_block(gecko_core::cube_corners([0.0; 3], n as f64))
        .unwrap();
    for axis in 0..3 {
        for k in 1..n {
            // The sheet to cut is the one through an edge that still runs
            // the whole way: cut from the far end each time.
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
    b
}

#[test]
#[ignore = "a measurement, not an assertion"]
fn what_a_block_costs() {
    for n in [1, 7, 12] {
        let before = LIVE.load(Ordering::Relaxed);
        let b = grid(n);
        let after = LIVE.load(Ordering::Relaxed);
        let seen = b.census();
        let bytes = (after - before) as f64;

        // And the structure on its own: a snapshot is exactly sized and
        // carries no derived cache, so the difference between the two is
        // what the slack in the arenas' vectors and the cache cost.
        let held = LIVE.load(Ordering::Relaxed);
        let copy = b.snapshot().expect("a snapshot");
        let alone = (LIVE.load(Ordering::Relaxed) - held) as f64;

        println!(
            "{n}x{n}x{n}: {} blocks, {} nodes, {} faces, {} edges",
            seen.blocks, seen.nodes, seen.faces, seen.edges,
        );
        println!(
            "    as built: {:.0} B, {:.0} B a block",
            bytes,
            bytes / seen.blocks as f64,
        );
        println!(
            "    the cells alone: {:.0} B, {:.0} B a block",
            alone,
            alone / seen.blocks as f64,
        );
        drop(copy);
        drop(b);
    }
}
