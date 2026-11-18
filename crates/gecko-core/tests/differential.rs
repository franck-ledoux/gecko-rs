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

use gecko_core::cells::{Census, Settling};
use gecko_core::{cube_corners, CellId, On};

/// What a step of the script does.
///
/// Every step names what it acts on by **where it is**, never by a name:
/// the two engines name cells differently on purpose, so a script that held
/// a name would be testing the naming and not the structure.
#[derive(Clone, Copy, Debug)]
enum Step {
    AddBlock([[f64; 3]; 8]),
    AddFace([[f64; 3]; 4]),
    /// Move the node at a point.
    MoveNode {
        from: [f64; 3],
        to: [f64; 3],
    },
    /// Say what the node at a point is lying on.
    Classify {
        at: [f64; 3],
        on: On,
    },
    Freeze {
        at: [f64; 3],
    },
    /// Put the block whose corners average to a point into a group.
    Group {
        middle: [f64; 3],
        bit: u8,
    },
    /// Settle every edge from its ends and every face from its edges.
    Reclassify,
}

/// A settling rule with no geometry behind it: the boundary agrees or the
/// cell is on nothing. Enough to drive the pass, and the same for both.
fn settle(asked: Settling<'_>) -> Option<On> {
    let first = *asked.boundary.first()?;
    let agreed = asked.boundary.iter().all(|on| *on == first);
    (agreed && first.dim >= asked.at_least).then_some(first)
}

/// What is compared after every step.
///
/// Not the whole state — the whole state is what differs on purpose — and
/// **nothing name-shaped**, which is the trap here: one engine names cells
/// from a counter and the other from a packed arena slot, so two correct
/// engines disagree about every name. What they must agree about is the
/// shape, so each cell is identified by where its corners are.
#[derive(Clone, Debug, PartialEq)]
struct Answers {
    census: Census,
    /// Every node's place.
    nodes: Vec<Place>,
    /// Every edge, as its two ends' places.
    edges: Vec<[Place; 2]>,
    /// Every face, as its four sides — each of those as its own two ends'
    /// places. `faces()` hands back a face's **edges** and not its corners,
    /// which is the first divergence this bench caught: the cellular engine
    /// was handing back corners, and nothing else noticed.
    faces: Vec<Vec<[Place; 2]>>,
    /// Every block, as its eight corners' places **in order**: the order is
    /// the hexahedron's own frame, so it is a fact about the block and not
    /// about the naming.
    blocks: Vec<Vec<Place>>,
    /// What each node is lying on and whether it is frozen, by its place.
    told: Vec<(Place, Spot, bool)>,
    /// What each edge and face is lying on, by the places of its corners.
    edges_on: Vec<([Place; 2], Spot)>,
    faces_on: Vec<(Vec<Place>, Spot)>,
    /// Every face as one of its blocks sees it: the block it belongs to, by
    /// the middle of its corners, then the face's four corners **in the
    /// order that block winds them**, turned to start at the lowest. Which
    /// is what catches an orientation that has gone the other way round —
    /// nothing else here would.
    seen: Vec<(Place, u64, Vec<Place>)>,
    patches: usize,
}

/// A cycle turned to start at its lowest entry, so that two readings of one
/// cycle compare equal while a cycle read backwards does not.
fn turned(places: &[Place]) -> Vec<Place> {
    let start = (0..places.len())
        .min_by_key(|k| places[*k])
        .expect("a cycle");
    (0..places.len())
        .map(|k| places[(start + k) % places.len()])
        .collect()
}

/// Positions as fixed point, so that two engines that computed the same
/// number by different routes compare equal and one that drifted does not.
fn fixed(at: [f64; 3]) -> [i64; 3] {
    at.map(|x| (x * 1e9).round() as i64)
}

/// A place, which is how every cell is identified here: by where it is and
/// never by its name.
type Place = [i64; 3];
/// What a cell is lying on, as a pair small enough to compare.
type Spot = Option<(u8, i32)>;

/// What the bench needs of an engine, and nothing more.
trait Engine: Sized {
    fn start() -> Self;
    fn add_block(&mut self, corners: [[f64; 3]; 8]) -> CellId;
    fn add_face(&mut self, corners: [[f64; 3]; 4]) -> CellId;
    fn answers(&self) -> Answers;
    /// The node at a point, by where it is. Panics if there is not exactly
    /// one: a script that aimed at nothing is a broken script, and saying so
    /// loudly beats both engines quietly doing nothing.
    fn node_at(&self, at: [f64; 3]) -> CellId;
    /// The block whose corners average to a point.
    fn block_at(&self, middle: [f64; 3]) -> CellId;
    fn set_position(&mut self, node: CellId, to: [f64; 3]);
    fn set_node_class(&mut self, node: CellId, on: On);
    fn set_frozen(&mut self, node: CellId);
    fn set_in_group(&mut self, block: CellId, bit: u8);
    fn reclassify(&mut self);
    /// The cellular engine checks its own invariants; the map has none to
    /// check, so it says so by saying nothing.
    fn check(&self) -> Result<(), String>;

    fn step(&mut self, step: Step) {
        match step {
            Step::AddBlock(corners) => {
                self.add_block(corners);
            }
            Step::AddFace(corners) => {
                self.add_face(corners);
            }
            Step::MoveNode { from, to } => {
                let node = self.node_at(from);
                self.set_position(node, to);
            }
            Step::Classify { at, on } => {
                let node = self.node_at(at);
                self.set_node_class(node, on);
            }
            Step::Freeze { at } => {
                let node = self.node_at(at);
                self.set_frozen(node);
            }
            Step::Group { middle, bit } => {
                let block = self.block_at(middle);
                self.set_in_group(block, bit);
            }
            Step::Reclassify => self.reclassify(),
        }
    }
}

/// The same reading, written once: both engines answer it through the same
/// public methods, which is the whole point of the exercise.
macro_rules! reading {
    () => {
        fn add_face(&mut self, corners: [[f64; 3]; 4]) -> CellId {
            self.add_face(corners).expect("a quadrangle")
        }

        fn node_at(&self, at: [f64; 3]) -> CellId {
            let found: Vec<CellId> = self
                .nodes()
                .filter(|(_, p)| fixed(*p) == fixed(at))
                .map(|(id, _)| id)
                .collect();
            assert_eq!(found.len(), 1, "one node at {at:?}, not {}", found.len());
            found[0]
        }

        fn block_at(&self, middle: [f64; 3]) -> CellId {
            let found: Vec<CellId> = self
                .blocks()
                .into_iter()
                .filter(|block| {
                    let Some(corners) = self.hex_corners(*block) else {
                        return false;
                    };
                    let mut at = [0.0; 3];
                    for corner in corners {
                        let p = self.position(corner).expect("a live corner");
                        for k in 0..3 {
                            at[k] += p[k] / 8.0;
                        }
                    }
                    fixed(at) == fixed(middle)
                })
                .collect();
            assert_eq!(found.len(), 1, "one block at {middle:?}");
            found[0]
        }

        fn set_position(&mut self, node: CellId, to: [f64; 3]) {
            self.set_position(node, to).expect("a live node")
        }

        fn set_node_class(&mut self, node: CellId, on: On) {
            self.set_node_class(node, Some(on)).expect("a live node")
        }

        fn set_frozen(&mut self, node: CellId) {
            self.set_frozen(node, true).expect("a live node")
        }

        fn set_in_group(&mut self, block: CellId, bit: u8) {
            self.set_in_group(&[block], bit, true)
                .expect("a live block")
        }

        fn reclassify(&mut self) {
            Self::reclassify(self, settle)
        }

        fn answers(&self) -> Answers {
            let at = |node: CellId| fixed(self.position(node).expect("a live node"));
            let mut nodes: Vec<Place> = self.nodes().map(|(_, p)| fixed(p)).collect();
            nodes.sort_unstable();
            let mut edges: Vec<[Place; 2]> = self
                .edges_in_full()
                .map(|(_, ends, _, _)| {
                    let mut pair = ends.map(at);
                    pair.sort_unstable();
                    pair
                })
                .collect();
            edges.sort_unstable();
            let side = |edge: CellId| {
                let (a, b) = self.edge_ends(edge).expect("a live edge");
                let mut pair = [at(a), at(b)];
                pair.sort_unstable();
                pair
            };
            let mut faces: Vec<Vec<[Place; 2]>> = self
                .faces()
                .map(|(_, edges)| {
                    let mut sides: Vec<[Place; 2]> = edges.iter().map(|e| side(*e)).collect();
                    sides.sort_unstable();
                    sides
                })
                .collect();
            faces.sort();
            let mut blocks: Vec<Vec<Place>> = self
                .blocks()
                .into_iter()
                .filter_map(|block| self.hex_corners(block))
                .map(|corners| corners.iter().map(|n| at(*n)).collect())
                .collect();
            blocks.sort();
            let spot = |on: Option<On>| on.map(|on| (on.dim, on.tag));
            let mut told: Vec<(Place, Spot, bool)> = self
                .nodes_in_full()
                .map(|(_, at, on, frozen)| (fixed(at), spot(on), frozen))
                .collect();
            told.sort();
            let mut edges_on: Vec<([Place; 2], Spot)> = self
                .edges()
                .map(|(_, a, b, on)| {
                    let mut pair = [at(a), at(b)];
                    pair.sort_unstable();
                    (pair, spot(on))
                })
                .collect();
            edges_on.sort();
            let mut faces_on: Vec<(Vec<Place>, Spot)> = self
                .faces()
                .map(|(face, edges)| {
                    let mut places: Vec<Place> = edges.iter().flat_map(|e| side(*e)).collect();
                    places.sort_unstable();
                    places.dedup();
                    (places, spot(self.face_class(face)))
                })
                .collect();
            faces_on.sort();
            let mut seen: Vec<(Place, u64, Vec<Place>)> = self
                .block_faces()
                .into_iter()
                .map(|side| {
                    let corners: Vec<Place> = side.nodes.iter().map(|n| at(*n)).collect();
                    let mut middle = [0i64; 3];
                    for p in &corners {
                        for k in 0..3 {
                            middle[k] += p[k] / 4;
                        }
                    }
                    (middle, side.groups, turned(&corners))
                })
                .collect();
            seen.sort();
            Answers {
                census: self.census(),
                nodes,
                edges,
                faces,
                blocks,
                told,
                edges_on,
                faces_on,
                seen,
                patches: self.nb_patches(),
            }
        }
    };
}

impl Engine for gecko_core::blocking::Blocking {
    fn start() -> Self {
        Self::new().expect("an empty map")
    }
    fn add_block(&mut self, corners: [[f64; 3]; 8]) -> CellId {
        self.add_block(corners).expect("a block")
    }
    reading!();
    /// The map has no invariants of its own to check; it says so by saying
    /// nothing, rather than by pretending there was something to ask.
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
    reading!();
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
fn blocks_laid_side_by_side_are_the_same_in_both() {
    agree(
        &(0..3)
            .map(|k| Step::AddBlock(cube_corners([k as f64, 0.0, 0.0], 1.0)))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn a_script_of_everything_stage_one_can_do() {
    let on = |dim, tag| On { dim, tag };
    agree(&[
        Step::AddBlock(cube_corners([0.0, 0.0, 0.0], 1.0)),
        Step::AddBlock(cube_corners([2.0, 0.0, 0.0], 1.0)),
        // Two quadrangles of their own, sharing nothing: two patches, which
        // both engines have to count as two.
        Step::AddFace([
            [0.0, 4.0, 0.0],
            [1.0, 4.0, 0.0],
            [1.0, 5.0, 0.0],
            [0.0, 5.0, 0.0],
        ]),
        Step::AddFace([
            [3.0, 4.0, 0.0],
            [4.0, 4.0, 0.0],
            [4.0, 5.0, 0.0],
            [3.0, 5.0, 0.0],
        ]),
        Step::MoveNode {
            from: [1.0, 1.0, 1.0],
            to: [1.2, 1.1, 1.3],
        },
        Step::Freeze {
            at: [0.0, 0.0, 0.0],
        },
        // One whole face of the first block on a surface, so the settling
        // has something to carry up: four corners, then the four edges
        // between them, then the face itself.
        Step::Classify {
            at: [0.0, 0.0, 0.0],
            on: on(2, 7),
        },
        Step::Classify {
            at: [1.0, 0.0, 0.0],
            on: on(2, 7),
        },
        Step::Classify {
            at: [1.0, 1.0, 0.0],
            on: on(2, 7),
        },
        Step::Classify {
            at: [0.0, 1.0, 0.0],
            on: on(2, 7),
        },
        Step::Reclassify,
        // And one corner of the other block on something else, so a
        // boundary that does **not** agree is exercised too.
        Step::Classify {
            at: [2.0, 0.0, 0.0],
            on: on(1, 3),
        },
        Step::Reclassify,
        Step::Group {
            middle: [2.5, 0.5, 0.5],
            bit: 3,
        },
    ]);
}
