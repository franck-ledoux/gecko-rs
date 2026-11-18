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
    /// Lay a quadrangle, welded to whatever it is laid against.
    LayFace([[f64; 3]; 4]),
    /// Lay a thin block on four points, welded the same way.
    AddSlab([[f64; 3]; 4], f64),
    /// Weld the two faces whose corners average to these points, each
    /// meeting the other in the middle.
    Glue {
        first: [f64; 3],
        second: [f64; 3],
    },
    /// The same, the second coming onto the first.
    GlueOnto {
        first: [f64; 3],
        second: [f64; 3],
    },
    /// Cut the sheet through the edge whose middle is at `edge`, at `t`
    /// along it **measured from the end at `from`**.
    ///
    /// Which way round an edge runs is a convention and the two engines
    /// chose differently. What is not a convention, and is what the page
    /// relies on, is that `t` runs the way `edge_ends` reports — so the
    /// step says which end it means and each engine turns it into its own
    /// `t` or `1 - t`.
    Split {
        edge: [f64; 3],
        from: [f64; 3],
        t: f64,
    },
    /// Take away the block whose corners average to a point.
    RemoveBlock {
        middle: [f64; 3],
    },
    /// Take away the layer through the edge whose middle is at a point,
    /// closing the gap behind it or leaving it open.
    RemoveSheet {
        edge: [f64; 3],
        close: bool,
    },
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
    /// Every block, as its eight corners' places — **sorted**, not in the
    /// hexahedron's order.
    ///
    /// Which of a block's six faces is its bottom is a convention, and a
    /// cut settles it differently in the two engines: the map's frame came
    /// out of dart arithmetic, the arena's out of `SPLIT_FRAME`, and both
    /// describe the same box. What is **not** a convention is which way the
    /// block is turned out, and `seen` below is where that is compared: a
    /// block inside out has every face wound the other way, and a reversed
    /// cycle is not a rotation of the one it reverses.
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
    /// The sheet through every edge: the edge asked about by its two ends'
    /// places, then every edge of the sheet with the direction the cut
    /// parameter runs in — **canonically**, see [`coherent`].
    sheets: Vec<SheetSeen>,
    /// The chord through every face, from each of its blocks: the face by
    /// its corners' places and the block by the middle of its own, then the
    /// blocks the chord crosses **in order**, since a chord is a curve and
    /// not a set.
    chords: Vec<ChordSeen>,
}

/// A sheet's directions, with the sign taken out of them.
///
/// Which way round a sheet points is a **convention** and the two engines
/// came to different ones honestly: the map read an edge the way its dart
/// ran, and the arena reads it the way the hexahedron's table lists it.
/// What is not a convention, and is the thing worth comparing, is that
/// every edge of one sheet points the *same* way across it — an orientation
/// that flipped half way through comes out as a concertina, nodes at `t` on
/// one side and `1 - t` on the other, with nothing raised.
///
/// So: flipped as a whole if the lowest of its edges runs the wrong way.
/// Two coherent sheets then compare equal whatever sign each engine chose,
/// and an incoherent one still differs from a coherent one.
fn coherent(mut along: Vec<(Place, Place)>) -> Vec<(Place, Place)> {
    let lowest = along
        .iter()
        .min_by_key(|(from, to)| {
            if from <= to {
                (*from, *to)
            } else {
                (*to, *from)
            }
        })
        .copied();
    if lowest.is_some_and(|(from, to)| from > to) {
        along = along.into_iter().map(|(from, to)| (to, from)).collect();
    }
    along.sort_unstable();
    along
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
/// One sheet: the edge it was asked about, and every edge of it with the
/// direction the cut parameter runs in. Absent where the walk was refused.
type SheetSeen = ([Place; 2], Option<Vec<(Place, Place)>>);
/// One chord: the face and block it was asked about, and the blocks it
/// crosses in order. Absent where the walk was refused.
type ChordSeen = ((Vec<Place>, Place), Option<Vec<Place>>);

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
    fn lay_face(&mut self, corners: [[f64; 3]; 4]);
    fn add_slab(&mut self, quad: [[f64; 3]; 4], thickness: f64);
    /// The face whose four corners average to a point and which has a block
    /// on one side only — the one a weld can be asked for.
    fn free_face_at(&self, middle: [f64; 3]) -> CellId;
    fn glue(&mut self, first: CellId, second: CellId);
    fn glue_onto(&mut self, first: CellId, second: CellId);
    /// The edge whose two ends average to a point.
    fn edge_at(&self, middle: [f64; 3]) -> CellId;
    fn split_sheet(&mut self, edge: CellId, t: f64);
    /// Where the end `t` is measured from is.
    fn first_end(&self, edge: CellId) -> Place;
    fn remove_block(&mut self, block: CellId);
    fn remove_sheet(&mut self, edge: CellId, close: bool);
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
            Step::LayFace(corners) => self.lay_face(corners),
            Step::AddSlab(quad, thickness) => self.add_slab(quad, thickness),
            Step::Glue { first, second } => {
                let (a, b) = (self.free_face_at(first), self.free_face_at(second));
                self.glue(a, b);
            }
            Step::GlueOnto { first, second } => {
                let (a, b) = (self.free_face_at(first), self.free_face_at(second));
                self.glue_onto(a, b);
            }
            Step::RemoveBlock { middle } => {
                let block = self.block_at(middle);
                self.remove_block(block);
            }
            Step::RemoveSheet { edge, close } => {
                let id = self.edge_at(edge);
                self.remove_sheet(id, close);
            }
            Step::Split { edge, from, t } => {
                let id = self.edge_at(edge);
                let t = if self.first_end(id) == fixed(from) {
                    t
                } else {
                    1.0 - t
                };
                self.split_sheet(id, t);
            }
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

        fn lay_face(&mut self, corners: [[f64; 3]; 4]) {
            Self::lay_face(self, corners).expect("a quadrangle");
        }

        fn add_slab(&mut self, quad: [[f64; 3]; 4], thickness: f64) {
            Self::add_slab(self, quad, thickness).expect("a slab");
        }

        fn free_face_at(&self, middle: [f64; 3]) -> CellId {
            let mut found: Vec<CellId> = Vec::new();
            let mut counted: std::collections::HashMap<CellId, usize> =
                std::collections::HashMap::new();
            for side in self.block_faces() {
                *counted.entry(side.face).or_default() += 1;
            }
            for side in self.block_faces() {
                if counted.get(&side.face).copied().unwrap_or(0) != 1 {
                    continue;
                }
                let mut at = [0.0; 3];
                for node in side.nodes {
                    let p = self.position(node).expect("a live corner");
                    for k in 0..3 {
                        at[k] += p[k] / 4.0;
                    }
                }
                if fixed(at) == fixed(middle) && !found.contains(&side.face) {
                    found.push(side.face);
                }
            }
            assert_eq!(found.len(), 1, "one free face at {middle:?}");
            found[0]
        }

        fn glue(&mut self, first: CellId, second: CellId) {
            Self::glue_faces(self, first, second, |_, _, _| None, |_, _| None).expect("a weld");
        }

        fn glue_onto(&mut self, first: CellId, second: CellId) {
            Self::glue_onto(self, first, second).expect("a weld");
        }

        fn edge_at(&self, middle: [f64; 3]) -> CellId {
            let found: Vec<CellId> = self
                .edges_in_full()
                .filter(|(edge, ..)| {
                    let (a, b) = self.edge_ends(*edge).expect("a live edge");
                    let (p, q) = (
                        self.position(a).expect("an end"),
                        self.position(b).expect("an end"),
                    );
                    fixed([0, 1, 2].map(|k| 0.5 * (p[k] + q[k]))) == fixed(middle)
                })
                .map(|(edge, ..)| edge)
                .collect();
            assert_eq!(found.len(), 1, "one edge through {middle:?}");
            found[0]
        }

        fn split_sheet(&mut self, edge: CellId, t: f64) {
            Self::split_sheet(self, edge, t).expect("a cut");
        }

        fn first_end(&self, edge: CellId) -> Place {
            let (a, _) = self.edge_ends(edge).expect("a live edge");
            fixed(self.position(a).expect("an end"))
        }

        fn remove_block(&mut self, block: CellId) {
            Self::remove_block(self, block).expect("a live block");
        }

        fn remove_sheet(&mut self, edge: CellId, close: bool) {
            if close {
                Self::remove_sheet(self, edge).expect("a layer");
            } else {
                Self::remove_sheet_leaving_a_gap(self, edge).expect("a layer");
            }
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
                .map(|corners| {
                    let mut places: Vec<Place> = corners.iter().map(|n| at(*n)).collect();
                    places.sort_unstable();
                    places
                })
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
            let ends = |edge: CellId| {
                let (a, b) = self.edge_ends(edge).expect("a live edge");
                let mut pair = [at(a), at(b)];
                pair.sort_unstable();
                pair
            };
            let mut sheets: Vec<SheetSeen> = self
                .edges_in_full()
                .map(|(edge, _, _, _)| {
                    let found = self.sheet_through(edge).ok().map(|sheet| {
                        coherent(
                            sheet
                                .iter()
                                .map(|side| (at(side.from), at(side.to)))
                                .collect(),
                        )
                    });
                    (ends(edge), found)
                })
                .collect();
            sheets.sort();
            let middle_of = |block: CellId| {
                let corners = self.hex_corners(block).expect("a live block");
                let mut at = [0.0; 3];
                for corner in corners {
                    let p = self.position(corner).expect("a live corner");
                    for k in 0..3 {
                        at[k] += p[k] / 8.0;
                    }
                }
                fixed(at)
            };
            let mut chords: Vec<ChordSeen> = Vec::new();
            for block in self.blocks() {
                for side in self.faces_of(block) {
                    let mut corners: Vec<Place> = side.nodes.iter().map(|n| at(*n)).collect();
                    corners.sort_unstable();
                    let crossed = self
                        .chord_through(side.face, block)
                        .ok()
                        .map(|chord| chord.iter().map(|step| middle_of(step.block)).collect());
                    chords.push(((corners, middle_of(block)), crossed));
                }
            }
            chords.sort();
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
                sheets,
                chords,
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

#[test]
fn a_script_of_welding() {
    agree(&[
        // Two blocks a gap apart, welded in the middle: each gives way by
        // half, which is what `glue_faces` means.
        Step::AddBlock(cube_corners([0.0, 0.0, 0.0], 1.0)),
        Step::AddBlock(cube_corners([1.5, 0.0, 0.0], 1.0)),
        Step::Glue {
            first: [1.0, 0.5, 0.5],
            second: [1.5, 0.5, 0.5],
        },
        // The weld moved only the four corners of the seam, so the second
        // block's far face is where it was, at x = 2.5. A third is brought
        // **onto** it, which moves and does not meet in the middle.
        Step::AddBlock(cube_corners([3.0, 0.0, 0.0], 1.0)),
        Step::GlueOnto {
            first: [2.5, 0.5, 0.5],
            second: [3.0, 0.5, 0.5],
        },
    ]);
}

#[test]
fn a_script_of_laying() {
    // A strip of quadrangles laid corner to corner, each welded to the one
    // before — including one clicked round the other way, which both
    // engines have to turn to agree.
    agree(&[
        Step::LayFace([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]),
        Step::LayFace([
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ]),
        Step::LayFace([
            [2.0, 1.0, 0.0],
            [3.0, 1.0, 0.0],
            [3.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
        ]),
    ]);
}

#[test]
fn a_script_of_cutting() {
    agree(&[
        // One block cut in three along z, then across in x: a grid made of
        // cuts, which is how a blocking is actually built.
        Step::AddBlock(cube_corners([0.0, 0.0, 0.0], 1.0)),
        Step::Split {
            edge: [0.0, 0.0, 0.5],
            from: [0.0, 0.0, 0.0],
            t: 0.25,
        },
        Step::Split {
            edge: [0.0, 0.0, 0.625],
            from: [0.0, 0.0, 0.25],
            t: 1.0 / 3.0,
        },
        Step::Split {
            edge: [0.5, 0.0, 0.0],
            from: [0.0, 0.0, 0.0],
            t: 0.4,
        },
    ]);
}

#[test]
fn a_script_of_taking_away() {
    // A three-by-three-by-three grid cut out of one block, then the middle
    // layer taken out and the gap closed. Nine seams to weld, and the caps
    // of neighbouring blocks share their corners — which is the case a
    // layer one block wide never shows.
    let mut script = vec![Step::AddBlock(cube_corners([0.0, 0.0, 0.0], 3.0))];
    for (edge, from, t) in [
        ([0.0, 0.0, 1.5], [0.0, 0.0, 0.0], 1.0 / 3.0),
        ([0.0, 0.0, 2.0], [0.0, 0.0, 1.0], 0.5),
        ([1.5, 0.0, 0.0], [0.0, 0.0, 0.0], 1.0 / 3.0),
        ([2.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.5),
        ([0.0, 1.5, 0.0], [0.0, 0.0, 0.0], 1.0 / 3.0),
        ([0.0, 2.0, 0.0], [0.0, 1.0, 0.0], 0.5),
    ] {
        script.push(Step::Split { edge, from, t });
    }
    script.push(Step::RemoveSheet {
        edge: [0.0, 0.0, 1.5],
        close: true,
    });
    // And one block out of what is left, which leaves a hole rather than
    // closing anything. The seam landed half way, so what was the lowest
    // layer now runs from 0 to 1.5.
    script.push(Step::RemoveBlock {
        middle: [0.5, 0.5, 0.75],
    });
    agree(&script);
}

#[test]
fn a_script_of_taking_away_and_leaving_the_gap() {
    agree(&[
        Step::AddBlock(cube_corners([0.0, 0.0, 0.0], 3.0)),
        Step::Split {
            edge: [0.0, 0.0, 1.5],
            from: [0.0, 0.0, 0.0],
            t: 1.0 / 3.0,
        },
        Step::Split {
            edge: [0.0, 0.0, 2.0],
            from: [0.0, 0.0, 1.0],
            t: 0.5,
        },
        // The walls stay where they are and the model gains a slot.
        Step::RemoveSheet {
            edge: [0.0, 0.0, 1.5],
            close: false,
        },
    ]);
}

#[test]
fn a_script_of_cutting_across_a_seam() {
    // The concertina case: a sheet that crosses from one block into
    // another through the face they share. An orientation that flipped on
    // the way through would put half the new nodes at `t` and half at
    // `1 - t`, and nothing would say so.
    agree(&[
        Step::AddBlock(cube_corners([0.0, 0.0, 0.0], 1.0)),
        Step::AddBlock(cube_corners([1.5, 0.0, 0.0], 1.0)),
        Step::Glue {
            first: [1.0, 0.5, 0.5],
            second: [1.5, 0.5, 0.5],
        },
        Step::Split {
            edge: [0.0, 0.0, 0.5],
            from: [0.0, 0.0, 0.0],
            t: 0.25,
        },
    ]);
}

#[test]
fn a_script_of_slabs() {
    agree(&[
        Step::AddSlab(
            [
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            0.1,
        ),
        // Laid on two of the first one's corners, so a side welds — and the
        // face welded to is the thin side and not the broad one.
        Step::AddSlab(
            [
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            0.1,
        ),
    ]);
}
