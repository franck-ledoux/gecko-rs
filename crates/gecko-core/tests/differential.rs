//! The two engines, side by side, on the same script of operations.
//!
//! The suite's own tests are the oracle for the cellular engine — they speak
//! only of named cells, so they describe both. But a test says what its
//! author thought to ask. This says something else: that the two engines
//! answer the **same** questions the same way, step by step, on a script
//! neither was written against.
//!
//! Both engines are named here explicitly rather than through the crate's
//! export, because this is the one place that needs one of each at once.
//!
//! The script grows with each stage of
//! `docs/developer-guide/cellular.md`'s order of work. An operation only
//! goes in once the cellular engine can do it; what the bench asserts is
//! agreement, and `todo!()` does not disagree, it panics.

use gecko_core::cells::Census;
use gecko_core::{cube_corners, CellId};

/// What a step of the script does.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// Stage 1.
    AddBlock([[f64; 3]; 8]),
}

/// What is compared after every step. Not the whole state — the whole state
/// is what differs on purpose — but everything a caller can ask for.
#[derive(Clone, Debug, PartialEq)]
struct Answers {
    census: Census,
    nodes: Vec<(CellId, [i64; 3])>,
}

/// Positions as fixed point, so that two engines that computed the same
/// number by different routes compare equal and one that drifted does not.
fn fixed(at: [f64; 3]) -> [i64; 3] {
    at.map(|x| (x * 1e9).round() as i64)
}

/// What the bench needs of an engine, and nothing more.
trait Engine: Sized {
    fn start() -> Self;
    fn add_block(&mut self, corners: [[f64; 3]; 8]) -> CellId;
    fn answers(&self) -> Answers;
    /// The cellular engine checks its own invariants; the map has none to
    /// check, so it says so by saying nothing.
    fn check(&self) -> Result<(), String>;

    fn step(&mut self, step: Step) {
        match step {
            Step::AddBlock(corners) => {
                self.add_block(corners);
            }
        }
    }
}

impl Engine for gecko_core::blocking::Blocking {
    fn start() -> Self {
        Self::new().expect("an empty map")
    }
    fn add_block(&mut self, corners: [[f64; 3]; 8]) -> CellId {
        self.add_block(corners).expect("a block")
    }
    fn answers(&self) -> Answers {
        Answers {
            census: self.census(),
            nodes: self.nodes().map(|(id, at)| (id, fixed(at))).collect(),
        }
    }
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Engine for gecko_core::cellular::Blocking {
    fn start() -> Self {
        Self::new().expect("an empty complex")
    }
    fn add_block(&mut self, corners: [[f64; 3]; 8]) -> CellId {
        self.add_block(corners).expect("a block")
    }
    fn answers(&self) -> Answers {
        Answers {
            census: self.census(),
            nodes: self.nodes().map(|(id, at)| (id, fixed(at))).collect(),
        }
    }
    fn check(&self) -> Result<(), String> {
        self.validate().map_err(|breaches| format!("{breaches:?}"))
    }
}

/// Run a script on both, comparing after every step.
fn agree(script: &[Step]) {
    let mut map = <gecko_core::blocking::Blocking as Engine>::start();
    let mut cellular = <gecko_core::cellular::Blocking as Engine>::start();

    let (there, here) = (map.answers(), cellular.answers());
    assert_eq!(there, here, "before anything was done");
    cellular.check().expect("the empty complex holds");

    for (k, step) in script.iter().enumerate() {
        map.step(*step);
        cellular.step(*step);
        cellular
            .check()
            .unwrap_or_else(|why| panic!("step {k}, {step:?}: {why}"));
        assert_eq!(
            map.answers(),
            cellular.answers(),
            "after step {k}, {step:?}"
        );
    }
}

#[test]
fn an_empty_blocking_is_the_same_in_both() {
    agree(&[]);
}

#[test]
#[ignore = "stage 1: add_block"]
fn blocks_laid_side_by_side_are_the_same_in_both() {
    agree(
        &(0..3)
            .map(|k| Step::AddBlock(cube_corners([k as f64, 0.0, 0.0], 1.0)))
            .collect::<Vec<_>>(),
    );
}
