//! The block structure.
//!
//! A thin, deliberate layer over [`CMap3`]: the map owns the topology, this
//! owns the naming. Nothing here reimplements what a combinatorial map already
//! does — sewing, orbits, attribute merge and split are the map's business, and
//! the one thing it cannot do for us is hand out names that survive its own
//! renumbering.

use std::collections::{BTreeMap, HashMap, HashSet};

use honeycomb_core::attributes::AttributeError;
use honeycomb_core::cmap::{
    CMap3, CMapBuilder, DartIdType, DartReleaseError, EdgeIdType, FaceIdType, LinkError,
    OrbitPolicy, SewError, VertexIdType, VolumeIdType, NULL_DART_ID,
};
use thiserror::Error;

use crate::attributes::{BlockTag, EdgeTag, FaceTag, NodeTag};
use crate::ids::{CellId, Counter};

/// The six faces of a hexahedron, each listed anticlockwise **seen from
/// outside**, over corners in the usual VTK/Gmsh order.
///
/// Outward orientation is not cosmetic here: it is what makes two faces of the
/// same block traverse their shared edge in opposite directions, which is
/// exactly the condition β₂ needs to sew them.
pub const HEX_FACES: [[usize; 4]; 6] = [
    [0, 3, 2, 1],
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
];

/// Darts per block: four per face, six faces.
const DARTS_PER_BLOCK: usize = 24;

/// The eight corners of an axis-aligned cube, in the order [`HEX_FACES`] reads
/// them.
pub fn cube_corners(origin: [f64; 3], size: f64) -> [[f64; 3]; 8] {
    let [x, y, z] = origin;
    let s = size;
    [
        [x, y, z],
        [x + s, y, z],
        [x + s, y + s, z],
        [x, y + s, z],
        [x, y, z + s],
        [x + s, y, z + s],
        [x + s, y + s, z + s],
        [x, y + s, z + s],
    ]
}

/// Two points close enough to be the same node.
fn same_point(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
}

/// How thick a pillow layer is, as a share of the cell it is cut from.
pub const PILLOW_SHARE: f64 = 1.0 / 3.0;

/// Where a node of a pillowed patch goes.
///
/// A mitre, as in a picture frame. The copy has to end up `depth` clear of
/// **every** selected face it sits on, and neither of the obvious answers
/// does that: summing the face normals sends a node in the middle of a flat
/// patch four times too far, and averaging them leaves a node on a corner
/// short of both its faces.
///
/// So: go along the average direction, and stretch by however much that
/// direction is tilted away from the worst of the faces. On a flat patch
/// that factor is one; on a right-angled corner it is √2, which puts the
/// copy `depth` in along each face, where it belongs.
///
/// `inward` are unit normals pointing into the material.
pub fn mitre(inward: &[[f64; 3]], depth: f64) -> [f64; 3] {
    let mut sum = [0.0; 3];
    for n in inward {
        for k in 0..3 {
            sum[k] += n[k];
        }
    }
    let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
    if length < 1e-12 {
        return [0.0; 3];
    }
    let mean = [sum[0] / length, sum[1] / length, sum[2] / length];
    let tilt = inward
        .iter()
        .map(|n| (0..3).map(|k| mean[k] * n[k]).sum::<f64>())
        .fold(f64::INFINITY, f64::min);
    // Faces folded back on each other have no inside to move into.
    if tilt < 1e-6 {
        return [0.0; 3];
    }
    mean.map(|x| x * depth / tilt)
}

/// The outward normal of a quadrangle, by the shoelace formula in 3D, and
/// its centre. Not normalised: the caller decides what to do with a face of
/// no area.
pub fn quad_normal(p: [[f64; 3]; 4]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for k in 0..4 {
        let (a, b) = (p[k], p[(k + 1) % 4]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// One edge of a sheet, with the direction the cut parameter runs in.
///
/// `from`/`to` are what makes a sheet cuttable at a single parameter: the
/// same `t` has to mean the same side of the sheet on every edge of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetEdge {
    pub edge: CellId,
    pub from: VertexIdType,
    pub to: VertexIdType,
}

/// Across a quadrangle: the edge opposite `from`→`to`, oriented to match.
///
/// This is the whole orientation question, in one place and with no map in
/// sight. In a quadrangle A→B→C→D the edge opposite AB is CD — but the
/// corners that correspond are A↔D and B↔C, so AB traversed A→B matches CD
/// traversed **D→C**. The opposite edge runs the other way round the cycle.
///
/// Getting it wrong does not fail: it cuts alternate cells at `t` and
/// `1 - t`, and the sheet comes out as a concertina. Which is why it is a
/// function of four numbers, tested on its own.
pub fn across(
    quad: [VertexIdType; 4],
    from: VertexIdType,
    to: VertexIdType,
) -> Option<(VertexIdType, VertexIdType)> {
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        if (a, b) == (from, to) {
            return Some((quad[(i + 3) % 4], quad[(i + 2) % 4]));
        }
        if (a, b) == (to, from) {
            return Some((quad[(i + 2) % 4], quad[(i + 3) % 4]));
        }
    }
    None
}

/// One face of one block: the four nodes it runs through, wound outward **as
/// that block sees it**, with both names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockFace {
    pub face: CellId,
    pub block: CellId,
    pub nodes: [VertexIdType; 4],
}

#[derive(Debug, Error)]
pub enum BlockingError {
    #[error("could not build the map: {0}")]
    Build(String),
    #[error("could not link darts: {0}")]
    Link(#[from] LinkError),
    #[error("attribute error: {0}")]
    Attribute(#[from] AttributeError),
    #[error("no node is named {0:?}")]
    NoSuchNode(CellId),
    #[error("no face is named {0:?}")]
    NoSuchFace(CellId),
    #[error("a face cannot be glued to itself")]
    SameFace,
    #[error("face {0:?} is already glued to another block")]
    AlreadyGlued(CellId),
    #[error("could not glue: {0}")]
    Sew(#[from] SewError),
    #[error("no block is named {0:?}")]
    NoSuchBlock(CellId),
    #[error("could not free dart {0}")]
    Release(DartIdType),
    #[error("no edge is named {0:?}")]
    NoSuchEdge(CellId),
    #[error("the sheet through edge {0:?} meets itself the wrong way round")]
    TangledSheet(CellId),
    #[error("a cut has to fall strictly inside the edge, not at {0}")]
    BadParameter(f64),
    #[error("the sheet through edge {0:?} does not close into a surface")]
    BrokenSheet(CellId),
    #[error("nothing was selected to pillow")]
    EmptyPatch,
    #[error("face {0:?} is not a face of block {1:?}")]
    FaceNotOnBlock(CellId, CellId),
    #[error("face {0:?} is inside the model; only surface faces can be pillowed")]
    NotOnTheSurface(CellId),
    #[error("could not find the face a new layer should be glued along")]
    LostFace,
}

impl From<DartReleaseError> for BlockingError {
    fn from(e: DartReleaseError) -> Self {
        BlockingError::Release(e.0)
    }
}

/// Which quarter turn lines two faces up best.
///
/// The pairing is necessarily orientation-reversing — glued faces are
/// back-to-back, so one is traversed the way the other is not — which leaves
/// exactly four candidates: `a[i]` meets `b[(m - i) mod 4]`, for `m` in 0..4.
/// The answer is the one that moves the corners least.
///
/// Pure, and separate from the gluing, because it is the only part of this
/// with a choice in it. Note what it cannot do: two blocks turned against each
/// other have no right answer here, only a least bad quarter turn.
pub fn best_pairing(a: &[[f64; 3]; 4], b: &[[f64; 3]; 4]) -> usize {
    let cost = |m: usize| -> f64 {
        (0..4)
            .map(|i| {
                let (p, q) = (a[i], b[(m + 4 - i) % 4]);
                (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>()
            })
            .sum()
    };
    // `total_cmp` and a stable minimum: ties go to the lowest turn, so the
    // same two faces always glue the same way.
    (0..4)
        .min_by(|&m, &n| cost(m).total_cmp(&cost(n)))
        .expect("four turns")
}

pub struct Blocking {
    map: CMap3<f64>,
    /// One counter per dimension: names are unique among cells of the same
    /// dimension, not across dimensions.
    names: [Counter; 4],
    corners: Counter,
}

impl Blocking {
    pub fn new() -> Result<Self, BlockingError> {
        let map = CMapBuilder::<3>::from_n_darts(0)
            .add_attribute::<NodeTag>()
            .add_attribute::<EdgeTag>()
            .add_attribute::<FaceTag>()
            .add_attribute::<BlockTag>()
            .build()
            .map_err(|e| BlockingError::Build(e.to_string()))?;
        Ok(Blocking {
            map,
            names: Default::default(),
            corners: Counter::default(),
        })
    }

    /// Add one hexahedral block from its eight corner points, in the usual
    /// VTK/Gmsh order.
    ///
    /// The block is built free-standing: every β₃ is left unset, so it is a
    /// closed volume bounded by the model's exterior until something is sewn to
    /// it. Joining blocks is a separate operation, and a later step.
    pub fn add_block(&mut self, corners: [[f64; 3]; 8]) -> Result<CellId, BlockingError> {
        let base = self.map.allocate_used_darts(DARTS_PER_BLOCK);
        let dart = |face: usize, slot: usize| base + (face * 4 + slot) as DartIdType;

        // β₁ walks each face round its four darts.
        for face in 0..6 {
            for slot in 0..4 {
                self.map
                    .link::<1>(dart(face, slot), dart(face, (slot + 1) % 4))?;
            }
        }

        // β₂ joins the two darts that traverse one edge in opposite directions.
        // Each is visited once, by only acting on the half with the lower
        // corner first.
        let mut by_edge: HashMap<(usize, usize), DartIdType> = HashMap::new();
        for (face, corners) in HEX_FACES.iter().enumerate() {
            for slot in 0..4 {
                by_edge.insert((corners[slot], corners[(slot + 1) % 4]), dart(face, slot));
            }
        }
        for (face, corners) in HEX_FACES.iter().enumerate() {
            for slot in 0..4 {
                let (from, to) = (corners[slot], corners[(slot + 1) % 4]);
                if from < to {
                    self.map.link::<2>(dart(face, slot), by_edge[&(to, from)])?;
                }
            }
        }

        // Positions and corner keys, once the orbits they belong to exist.
        for (corner, point) in corners.iter().enumerate() {
            let d = HEX_FACES
                .iter()
                .enumerate()
                .find_map(|(f, corners)| {
                    corners
                        .iter()
                        .position(|c| *c == corner)
                        .map(|s| dart(f, s))
                })
                .expect("every corner of a hexahedron is on some face");
            let vertex = self.map.vertex_id(d);
            self.map
                .write_vertex(vertex, (point[0], point[1], point[2]));
            self.map.write_attribute(
                vertex,
                NodeTag {
                    id: CellId::NONE,
                    corner_key: self.corners.next_corner(),
                },
            );
        }

        self.assign_missing_ids();
        Ok(self.block_tag(self.map.volume_id(base)).id)
    }

    /// Hand a name to every cell that does not have one.
    ///
    /// Separate from the operations that create cells, because that is the only
    /// arrangement that stays correct: an edit splits and merges cells as a side
    /// effect of changing topology, and asking each edit to name what it
    /// happened to create is how a cell ends up with two names or none.
    pub fn assign_missing_ids(&mut self) {
        let nodes: Vec<_> = self
            .map
            .iter_vertices()
            .filter(|v| self.node_tag(*v).id.is_none())
            .collect();
        for v in nodes {
            let tag = self.node_tag(v);
            let id = self.names[0].next_cell();
            self.map.write_attribute(v, NodeTag { id, ..tag });
        }

        let edges: Vec<_> = self
            .map
            .iter_edges()
            .filter(|e| self.edge_tag(*e).id.is_none())
            .collect();
        for e in edges {
            let id = self.names[1].next_cell();
            self.map.write_attribute(e, EdgeTag { id });
        }

        let faces: Vec<_> = self
            .map
            .iter_faces()
            .filter(|f| self.face_tag(*f).id.is_none())
            .collect();
        for f in faces {
            let id = self.names[2].next_cell();
            self.map.write_attribute(f, FaceTag { id });
        }

        let blocks: Vec<_> = self
            .map
            .iter_volumes()
            .filter(|b| self.block_tag(*b).id.is_none())
            .collect();
        for b in blocks {
            let id = self.names[3].next_cell();
            self.map.write_attribute(b, BlockTag { id });
        }
    }

    /// Move one node.
    ///
    /// The node is named by its stable [`CellId`], never by the identifier the
    /// map derives from darts: that one is rewritten by every edit, so a caller
    /// holding it between two operations would be pointing at something else.
    pub fn set_position(&mut self, node: CellId, to: [f64; 3]) -> Result<(), BlockingError> {
        let vertex = self
            .node_by_id(node)
            .ok_or(BlockingError::NoSuchNode(node))?;
        self.map.write_vertex(vertex, (to[0], to[1], to[2]));
        Ok(())
    }

    /// The cell a stable name currently stands for.
    ///
    /// A scan. An index would have to be invalidated by every topological
    /// change, and getting that wrong is silent; at this size the scan is not
    /// worth the hazard. It goes here when it starts to matter.
    pub fn node_by_id(&self, node: CellId) -> Option<VertexIdType> {
        if node.is_none() {
            return None;
        }
        self.map
            .iter_vertices()
            .find(|v| self.node_tag(*v).id == node)
    }

    /// Every node, by stable name and position.
    pub fn nodes(&self) -> impl Iterator<Item = (CellId, [f64; 3])> + '_ {
        self.map
            .iter_vertices()
            .map(move |v| (self.node_tag(v).id, self.position(v).unwrap_or([0.0; 3])))
    }

    /// Glue two faces together, welding the blocks they belong to.
    ///
    /// The second face is moved onto the first, and the first's position is
    /// what survives. That is not merely a convention: honeycomb merges two
    /// vertices by **averaging** them, so sewing faces that are apart would
    /// leave the shared corners half way between the two. Making them coincide
    /// first settles the question here, where the rule lives, instead of
    /// depending on a merge policy that belongs to the map.
    ///
    /// Only the four corners of the face move. The rest of the second block
    /// stays where it is, so gluing blocks that are far apart deforms one of
    /// them rather than carrying it over.
    pub fn glue_faces(&mut self, first: CellId, second: CellId) -> Result<(), BlockingError> {
        let a = self
            .face_by_id(first)
            .ok_or(BlockingError::NoSuchFace(first))?;
        let b = self
            .face_by_id(second)
            .ok_or(BlockingError::NoSuchFace(second))?;
        if a == b {
            return Err(BlockingError::SameFace);
        }
        for (name, face) in [(first, a), (second, b)] {
            if self.map.beta::<3>(face as DartIdType) != NULL_DART_ID {
                return Err(BlockingError::AlreadyGlued(name));
            }
        }

        let (da, db) = (self.face_darts(a), self.face_darts(b));
        let corner = |d: DartIdType| {
            self.position(self.map.vertex_id(d))
                .unwrap_or([f64::NAN; 3])
        };
        let (pa, pb) = (da.map(corner), db.map(corner));
        let turn = best_pairing(&pa, &pb);

        for i in 0..4 {
            let vertex = self.map.vertex_id(db[(turn + 4 - i) % 4]);
            self.map
                .write_vertex(vertex, (pa[i][0], pa[i][1], pa[i][2]));
        }

        // One call. `three_sew` walks the face orbit itself — β₁ down one side
        // and β₀ up the other — so it links all four pairs and merges the
        // faces, edges and vertices along the way.
        //
        // `da[0]` runs from corner A₀ to A₁, so its partner must run from the
        // B corner now sitting on A₁ back to the one on A₀. That is the dart
        // starting at `b[(turn + 3) % 4]`.
        self.map.sew::<3>(da[0], db[(turn + 3) % 4])?;
        self.assign_missing_ids();
        Ok(())
    }

    /// Take out the whole sheet of blocks through an edge, closing the gap.
    ///
    /// The sheet is a layer: each of its blocks is capped at either end by
    /// a face the sheet does not cross, and whatever was glued to those two
    /// caps ends up glued to each other once the layer is gone.
    ///
    /// **Which face meets which is a question about the map, not about
    /// where anything is.** From a dart of one cap, crossing into the
    /// lateral face beside it and out the far side lands on the cap at the
    /// other end, on the matching edge. Taking the β₃ of both gives the two
    /// darts the neighbours will have to be sewn along — read before the
    /// layer goes, since they are what the layer was holding together.
    ///
    /// Nothing is moved by hand either. Sewing merges the two vertices by
    /// averaging them, so the seam lands half way between the caps of its
    /// own accord, and the gap closes from both sides.
    ///
    /// A cap with nothing behind it is a cap on the model's surface; there
    /// is nothing to close against, so the model simply loses that layer.
    pub fn remove_sheet(&mut self, edge: CellId) -> Result<(), BlockingError> {
        let sheet = self.sheet_through(edge)?;

        // Which blocks the sheet runs through, and which of their edges it
        // is made of. A hexahedron the sheet crosses has four.
        let mut crossing: BTreeMap<VolumeIdType, Vec<CellId>> = BTreeMap::new();
        for side in &sheet {
            let at = self
                .edge_by_id(side.edge)
                .ok_or(BlockingError::NoSuchEdge(side.edge))?;
            let mut blocks: Vec<VolumeIdType> = self
                .map
                .orbit(OrbitPolicy::Edge, at as DartIdType)
                .map(|d| self.map.volume_id(d))
                .collect();
            blocks.sort();
            blocks.dedup();
            for block in blocks {
                crossing.entry(block).or_default().push(side.edge);
            }
        }

        let doomed: Vec<CellId> = crossing.keys().map(|v| self.block_tag(*v).id).collect();
        let mut rejoin: Vec<(DartIdType, DartIdType)> = Vec::new();

        for (volume, edges) in &crossing {
            if edges.len() != 4 {
                return Err(BlockingError::BrokenSheet(edge));
            }
            let crossed: HashSet<CellId> = edges.iter().copied().collect();
            // A cap is a face of this block that the sheet does not cross.
            let caps: Vec<DartIdType> = self
                .block_face_darts(*volume)
                .into_iter()
                .filter(|d| {
                    self.face_darts(self.map.face_id(*d))
                        .iter()
                        .all(|e| !crossed.contains(&self.edge_tag(self.map.edge_id(*e)).id))
                })
                .collect();
            if caps.len() != 2 {
                return Err(BlockingError::BrokenSheet(edge));
            }

            // One dart of one cap, and the dart it answers to on the other.
            let here = caps[0];
            let there = self.across_the_block(here);
            if self.map.face_id(there) != self.map.face_id(caps[1]) {
                return Err(BlockingError::BrokenSheet(edge));
            }
            let (above, below) = (self.map.beta::<3>(here), self.map.beta::<3>(there));
            if above == NULL_DART_ID || below == NULL_DART_ID {
                continue;
            }
            // A sheet that comes back round onto itself has no outside to
            // close against.
            for neighbour in [above, below] {
                if crossing.contains_key(&self.map.volume_id(neighbour)) {
                    return Err(BlockingError::BrokenSheet(edge));
                }
            }
            rejoin.push((above, below));
        }

        for block in doomed {
            self.remove_block(block)?;
        }
        // The darts belong to the blocks that stayed, so they outlived the
        // layer that was between them.
        for (above, below) in rejoin {
            self.map.sew::<3>(above, below)?;
        }

        self.assign_missing_ids();
        Ok(())
    }

    /// One dart per face of a block, on that block's own side.
    fn block_face_darts(&self, volume: VolumeIdType) -> Vec<DartIdType> {
        let mut seen: Vec<FaceIdType> = Vec::new();
        let mut out = Vec::new();
        for dart in self.map.orbit(OrbitPolicy::Volume, volume as DartIdType) {
            let face = self.map.face_id(dart);
            if !seen.contains(&face) {
                seen.push(face);
                out.push(dart);
            }
        }
        out
    }

    /// From a dart of one face of a hexahedron to the matching dart of the
    /// face opposite.
    ///
    /// Out through the lateral face beside this edge — β₂ — along it twice,
    /// and back in through the far side. The dart that comes out runs the
    /// other way round, which is exactly what a sew wants of a pair.
    fn across_the_block(&self, dart: DartIdType) -> DartIdType {
        let beside = self.map.beta::<2>(dart);
        let along = self.map.beta::<1>(self.map.beta::<1>(beside));
        self.map.beta::<2>(along)
    }

    /// Remove one block, leaving whatever it was glued to intact.
    ///
    /// Three steps, in this order because each needs what the previous one
    /// has not yet destroyed:
    ///
    /// 1. **unsew β₃**, while the orbits that walk takes still exist. This is
    ///    where a neighbour gets its own copy of the shared cells back — and
    ///    it is the case [`CornerKey`][crate::ids::CornerKey] was written
    ///    for: the names on both sides are cleared, but the neighbour goes on
    ///    recognising the corner.
    /// 2. **unlink β₂ then β₁** — the raw operations, not the sewing ones.
    ///    There is no cell left to split by then; what is left is a block
    ///    being taken apart.
    /// 3. **release the darts**, which the map only allows once every β of
    ///    theirs is null — hence the order.
    pub fn remove_block(&mut self, block: CellId) -> Result<(), BlockingError> {
        let volume = self
            .block_by_id(block)
            .ok_or(BlockingError::NoSuchBlock(block))?;
        // Collected first: the orbit is about to stop being walkable. β₃ is
        // not one of its generators, so this is one block's darts, never a
        // neighbour's.
        let darts: Vec<DartIdType> = self
            .map
            .orbit(OrbitPolicy::Volume, volume as DartIdType)
            .collect();

        for &dart in &darts {
            if self.map.beta::<3>(dart) != NULL_DART_ID {
                self.map.unsew::<3>(dart)?;
            }
        }
        for &dart in &darts {
            if self.map.beta::<2>(dart) != NULL_DART_ID {
                self.map.unlink::<2>(dart)?;
            }
        }
        for &dart in &darts {
            if self.map.beta::<1>(dart) != NULL_DART_ID {
                self.map.unlink::<1>(dart)?;
            }
        }
        for &dart in &darts {
            self.map.release_dart(dart)?;
        }

        // The neighbour's cells came out of the split unnamed.
        self.assign_missing_ids();
        Ok(())
    }

    /// The block a stable name currently stands for. A scan, for the same
    /// reason [`node_by_id`][Self::node_by_id] is one.
    pub fn block_by_id(&self, block: CellId) -> Option<VolumeIdType> {
        if block.is_none() {
            return None;
        }
        self.map
            .iter_volumes()
            .find(|v| self.block_tag(*v).id == block)
    }

    /// Every edge of the sheet through this one, oriented to agree with it.
    ///
    /// A sheet is the transitive closure of "the opposite edge in a
    /// quadrangle". On a lone block that is the four edges parallel to the
    /// one you name; across glued blocks it runs on through them.
    ///
    /// The orientation is carried along with the walk, so that one cut
    /// parameter means the same thing everywhere on the sheet. If the closure
    /// ever comes back to an edge it has already seen and disagrees about
    /// which way round it runs, the sheet is not orientable — it meets itself
    /// — and that is refused rather than cut badly.
    pub fn sheet_through(&self, edge: CellId) -> Result<Vec<SheetEdge>, BlockingError> {
        let start = self
            .edge_by_id(edge)
            .ok_or(BlockingError::NoSuchEdge(edge))?;
        let (from, to) = self.edge_endpoints(start);

        // Keyed by name and walked in order, so the answer does not depend on
        // the order a hash map happened to hand things back in.
        let mut found: BTreeMap<CellId, SheetEdge> = BTreeMap::new();
        found.insert(edge, SheetEdge { edge, from, to });
        let mut pending = vec![(start, from, to)];

        while let Some((current, from, to)) = pending.pop() {
            for face in self.faces_around(current) {
                let quad = self.face_corners(face);
                let Some((next_from, next_to)) = across(quad, from, to) else {
                    continue;
                };
                let Some(next) = self.edge_between(face, next_from, next_to) else {
                    continue;
                };
                let name = self.edge_tag(next).id;
                if let Some(seen) = found.get(&name) {
                    if (seen.from, seen.to) != (next_from, next_to) {
                        return Err(BlockingError::TangledSheet(edge));
                    }
                    continue;
                }
                found.insert(
                    name,
                    SheetEdge {
                        edge: name,
                        from: next_from,
                        to: next_to,
                    },
                );
                pending.push((next, next_from, next_to));
            }
        }
        Ok(found.into_values().collect())
    }

    /// Cut one edge in two, putting a node at `at`.
    ///
    /// An edge in a 3-map is a ring of darts — one per face-side around it —
    /// and every one of them has to become two. For a dart running A→B the
    /// original keeps A→P and a new one takes P→B.
    ///
    /// The β₂ and β₃ partners are then re-paired **crosswise**. If `d` runs
    /// A→B its partner `g` runs B→A, so after the cut `d` is A→P and `g`'s
    /// new half is P→A: those two are the pair. Pairing `d` with `g` again
    /// would join A→P to B→P, which share no edge at all.
    ///
    /// Both halves come out unnamed, as a split should: what comes out of
    /// one is two new cells, not a cell and a claimant to the old name.
    ///
    /// # Why the raw setters
    ///
    /// `link` and `unlink` are not the per-dart operations they look like.
    /// `one_link` also links the β₃ mirror, to keep β₃∘β₁ = β₀∘β₃; `three_link`
    /// walks whole faces and refuses a pair of different lengths. Both are
    /// right for editing a finished map and wrong here: halfway through a
    /// split the two sides of a face genuinely are different lengths, and the
    /// mirror is something this already knows how to wire. Judged by those
    /// operators, every intermediate state is invalid.
    ///
    /// So the wiring is set outright, and `is_coherent` in the tests below
    /// checks afterwards what they would have checked throughout.
    fn split_edge(
        &mut self,
        edge: EdgeIdType,
        at: [f64; 3],
    ) -> Result<VertexIdType, BlockingError> {
        let darts: Vec<DartIdType> = self
            .map
            .orbit(OrbitPolicy::Edge, edge as DartIdType)
            .collect();
        let base = self.map.allocate_used_darts(darts.len());
        let fresh: HashMap<DartIdType, DartIdType> = darts
            .iter()
            .enumerate()
            .map(|(i, d)| (*d, base + i as DartIdType))
            .collect();

        // Read every pairing before any of it is disturbed.
        let after: HashMap<DartIdType, DartIdType> =
            darts.iter().map(|&d| (d, self.map.beta::<1>(d))).collect();
        let two: HashMap<DartIdType, DartIdType> =
            darts.iter().map(|&d| (d, self.map.beta::<2>(d))).collect();
        let three: HashMap<DartIdType, DartIdType> =
            darts.iter().map(|&d| (d, self.map.beta::<3>(d))).collect();

        for &d in &darts {
            let (n, next) = (fresh[&d], after[&d]);
            self.map.set_beta::<1>(d, n);
            self.map.set_beta::<0>(n, d);
            self.map.set_beta::<1>(n, next);
            self.map.set_beta::<0>(next, n);

            // Crosswise, and only where there was a partner to begin with.
            for (i, partner) in [(2u8, two[&d]), (3u8, three[&d])] {
                if partner == NULL_DART_ID {
                    continue;
                }
                let mirror = fresh[&partner];
                if i == 2 {
                    self.map.set_beta::<2>(d, mirror);
                    self.map.set_beta::<2>(n, partner);
                } else {
                    self.map.set_beta::<3>(d, mirror);
                    self.map.set_beta::<3>(n, partner);
                }
            }
        }

        // The node in the middle, and the two halves left to be named.
        let middle = self.map.vertex_id(fresh[&darts[0]]);
        self.map.write_vertex(middle, (at[0], at[1], at[2]));
        self.map.write_attribute(
            middle,
            NodeTag {
                id: CellId::NONE,
                corner_key: self.corners.next_corner(),
            },
        );
        for &d in &[darts[0], fresh[&darts[0]]] {
            let half = self.map.edge_id(d);
            self.map.write_attribute(half, EdgeTag { id: CellId::NONE });
        }
        Ok(middle)
    }

    /// Cut the whole sheet through an edge, at `t` along it.
    ///
    /// Three passes, in this order, because each needs the one before it
    /// finished across the **whole** sheet rather than cell by cell: split
    /// every edge, then every face the sheet crosses, then every block.
    /// Doing one block at a time would leave its neighbour's shared face
    /// half-cut, and nothing downstream could tell that from a mistake.
    ///
    /// `t` runs along the direction [`sheet_through`][Self::sheet_through]
    /// reports, which is the same way round on every edge of the sheet —
    /// otherwise one parameter would mean `t` on some cells and `1 - t` on
    /// the next.
    pub fn split_sheet(&mut self, edge: CellId, t: f64) -> Result<(), BlockingError> {
        if !t.is_finite() || t <= 0.0 || t >= 1.0 {
            return Err(BlockingError::BadParameter(t));
        }
        let sheet = self.sheet_through(edge)?;
        let from_nodes: HashSet<VertexIdType> = sheet.iter().map(|s| s.from).collect();

        // --- the edges ------------------------------------------------
        let mut fresh: HashSet<VertexIdType> = HashSet::new();
        for side in &sheet {
            let target = self
                .edge_by_id(side.edge)
                .ok_or(BlockingError::NoSuchEdge(side.edge))?;
            let p = self.position(side.from).unwrap_or([0.0; 3]);
            let q = self.position(side.to).unwrap_or([0.0; 3]);
            let at = [0, 1, 2].map(|k| p[k] + (q[k] - p[k]) * t);
            fresh.insert(self.split_edge(target, at)?);
        }

        // --- the faces ------------------------------------------------
        // Every cycle read first: splitting one face leaves the others
        // alone, so the list taken now is still the list to work through.
        let sides = self.face_sides();
        let mut cuts: Vec<(DartIdType, DartIdType)> = Vec::new();
        for cycle in &sides {
            if let Some(pair) = self.split_face_side(cycle, &fresh, &from_nodes) {
                cuts.push(pair);
            }
        }
        if cuts.is_empty() {
            return Err(BlockingError::BrokenSheet(edge));
        }

        // The new darts' β₃, from the invariant β₃∘β₁ = β₀∘β₃: both sides of
        // a shared face have their new dart by now, so each can find its
        // opposite number through the dart it was inserted after.
        for &(u, v) in &cuts {
            for x in [u, v] {
                let behind = self.map.beta::<0>(x);
                let mirror = self.map.beta::<3>(behind);
                if mirror != NULL_DART_ID {
                    let partner = self.map.beta::<0>(mirror);
                    self.map.set_beta::<3>(x, partner);
                    self.map.set_beta::<3>(partner, x);
                }
            }
        }

        // --- the blocks -----------------------------------------------
        // Grouped before any of them is split, since splitting one rewrites
        // its own identifier.
        let mut rings: BTreeMap<VolumeIdType, Vec<(DartIdType, DartIdType)>> = BTreeMap::new();
        for &(u, v) in &cuts {
            rings.entry(self.map.volume_id(u)).or_default().push((u, v));
        }
        for (_, ring) in rings {
            self.insert_cut_face(&ring, edge)?;
        }

        // What was split is gone: two new cells each, named afresh.
        for &(u, v) in &cuts {
            for x in [u, v] {
                let face = self.map.face_id(x);
                self.map.write_attribute(face, FaceTag { id: CellId::NONE });
            }
        }
        self.assign_missing_ids();
        Ok(())
    }

    /// Close one block off along its ring of new edges, making two.
    ///
    /// The ring is the new edge of each side face the sheet crossed. Until
    /// now the two halves of the block are still joined, through the β₂ that
    /// pairs each ring edge's two darts across its own face. The cut face
    /// goes between them: `u` loses `v` as a partner and gains a dart of the
    /// new face, and `v` likewise, and the two new faces are β₃ to each
    /// other — which is what two blocks sharing a face are.
    fn insert_cut_face(
        &mut self,
        ring: &[(DartIdType, DartIdType)],
        edge: CellId,
    ) -> Result<(), BlockingError> {
        // Walk the ring by its geometry: the next edge starts where this one
        // ends. A block the sheet crosses has four, in one closed loop.
        let starts: HashMap<VertexIdType, usize> = ring
            .iter()
            .enumerate()
            .map(|(k, &(u, _))| (self.map.vertex_id(u), k))
            .collect();
        let mut order = vec![0usize];
        for _ in 1..ring.len() {
            let last = *order.last().unwrap();
            let ends_at = self.map.vertex_id(self.map.beta::<1>(ring[last].0));
            match starts.get(&ends_at) {
                Some(&next) if !order.contains(&next) => order.push(next),
                _ => return Err(BlockingError::BrokenSheet(edge)),
            }
        }
        let closes = self
            .map
            .vertex_id(self.map.beta::<1>(ring[*order.last().unwrap()].0));
        if closes != self.map.vertex_id(ring[order[0]].0) {
            return Err(BlockingError::BrokenSheet(edge));
        }

        let n = order.len();
        let base = self.map.allocate_used_darts(n * 2);
        let (a, b): (Vec<_>, Vec<_>) = (
            (0..n).map(|k| base + k as DartIdType).collect(),
            (0..n).map(|k| base + (n + k) as DartIdType).collect(),
        );

        for k in 0..n {
            let (u, v) = ring[order[k]];
            // `a` runs against `u`, so its cycle runs backwards round the
            // ring; `b` runs with it, so its cycle runs forwards.
            let behind = a[(k + n - 1) % n];
            self.map.set_beta::<1>(a[k], behind);
            self.map.set_beta::<0>(behind, a[k]);
            let ahead = b[(k + 1) % n];
            self.map.set_beta::<1>(b[k], ahead);
            self.map.set_beta::<0>(ahead, b[k]);

            self.map.set_beta::<2>(a[k], u);
            self.map.set_beta::<2>(u, a[k]);
            self.map.set_beta::<2>(b[k], v);
            self.map.set_beta::<2>(v, b[k]);
            self.map.set_beta::<3>(a[k], b[k]);
            self.map.set_beta::<3>(b[k], a[k]);
        }

        // Both halves are new blocks; neither inherits the name.
        for &d in &[a[0], b[0]] {
            let block = self.map.volume_id(d);
            self.map
                .write_attribute(block, BlockTag { id: CellId::NONE });
        }
        Ok(())
    }

    /// Insert a layer of hexahedra along a patch of faces.
    ///
    /// Each selected face is given as the pair a click gives: the face, and
    /// the block it was clicked on. That block's own nodes are drawn
    /// inwards and one hexahedron is laid into the space that opens up, so
    /// **nothing on the far side of the patch moves** — which is the point
    /// of pillowing.
    ///
    /// A face on the model's surface needs only that. A face **inside** the
    /// model needs the two blocks to stop sharing their nodes first, or
    /// drawing one block in would drag the other with it and leave no space
    /// at all. Unsewing the face is exactly that: the map splits the shared
    /// vertices and each side comes away with its own copy.
    ///
    /// Built from unsewing, [`add_block`][Self::add_block],
    /// [`glue_faces`][Self::glue_faces] and moving nodes, rather than from
    /// dart surgery. The topology is the same either way, and these already
    /// have tests behind them.
    ///
    /// A node on the **rim** of the patch is shared with blocks outside it,
    /// and moving it takes them with it. The mesh stays conforming and those
    /// blocks are deformed a little; that is the honest consequence of a
    /// shared node being shared.
    pub fn pillow(&mut self, patch: &[(CellId, CellId)]) -> Result<(), BlockingError> {
        if patch.is_empty() {
            return Err(BlockingError::EmptyPatch);
        }

        // --- read everything before a single node moves ----------------
        struct Plan {
            block: CellId,
            behind: Option<CellId>,
            points: [[f64; 3]; 4],
            inward: [f64; 3],
            depth: f64,
        }
        let sides = self.block_faces();
        let mut plans: Vec<Plan> = Vec::new();
        for &(face, block) in patch {
            let side = sides
                .iter()
                .find(|s| s.face == face && s.block == block)
                .ok_or(BlockingError::FaceNotOnBlock(face, block))?;
            let behind = sides
                .iter()
                .find(|s| s.face == face && s.block != block)
                .map(|s| s.block);

            let points = side.nodes.map(|n| self.position(n).unwrap_or([0.0; 3]));
            let normal = quad_normal(points);
            let length = (normal[0].powi(2) + normal[1].powi(2) + normal[2].powi(2)).sqrt();
            if length < 1e-12 {
                return Err(BlockingError::LostFace);
            }
            // The block's own extent in that direction, from its centre to
            // the face and back again; a third of it is the layer.
            let centre = self.block_centre(&sides, block);
            let middle = [0, 1, 2].map(|k| points.iter().map(|p| p[k]).sum::<f64>() / 4.0);
            let reach = (0..3)
                .map(|k| (middle[k] - centre[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            plans.push(Plan {
                block,
                behind,
                points,
                inward: normal.map(|x| -x / length),
                depth: 2.0 * reach * PILLOW_SHARE,
            });
        }

        // --- let go of the far side ------------------------------------
        // All of them before anything is read again: two blocks joined
        // along several faces keep sharing their nodes until the last of
        // those is undone, so one at a time would see a half-detached map.
        for &(face, _) in patch {
            if let Some(at) = self.face_by_id(face) {
                if self.map.beta::<3>(at as DartIdType) != NULL_DART_ID {
                    self.map.unsew::<3>(at as DartIdType)?;
                }
            }
        }
        // The split left the cells it touched unnamed, and the faces have
        // to be findable again below.
        self.assign_missing_ids();

        // --- where each node of the patch goes --------------------------
        let sides = self.block_faces();
        let mut facing: HashMap<VertexIdType, (Vec<[f64; 3]>, f64)> = HashMap::new();
        let mut near: Vec<BlockFace> = Vec::new();
        for plan in &plans {
            let side = self
                .side_at(&sides, plan.block, &plan.points)
                .ok_or(BlockingError::LostFace)?;
            if let Some(behind) = plan.behind {
                let other = self
                    .side_at(&sides, behind, &plan.points)
                    .ok_or(BlockingError::LostFace)?;
                // Still the same nodes: the two blocks are joined somewhere
                // else as well, so drawing one in would drag the other.
                if side.nodes.iter().any(|n| other.nodes.contains(n)) {
                    return Err(BlockingError::NotOnTheSurface(side.face));
                }
            }
            for node in side.nodes {
                let entry = facing.entry(node).or_insert((Vec::new(), f64::INFINITY));
                entry.0.push(plan.inward);
                entry.1 = entry.1.min(plan.depth);
            }
            near.push(side);
        }

        let mut moved: HashMap<VertexIdType, [f64; 3]> = HashMap::new();
        for (node, (inward, depth)) in &facing {
            let from = self.position(*node).unwrap_or([0.0; 3]);
            let step = mitre(inward, *depth);
            moved.insert(*node, [0, 1, 2].map(|k| from[k] + step[k]));
        }
        for (node, to) in &moved {
            self.map.write_vertex(*node, (to[0], to[1], to[2]));
        }

        // --- a block per face, laid into the space that opened ----------
        struct Layer {
            block: CellId,
            shrunk: [[f64; 3]; 4],
        }
        let mut layers: Vec<Layer> = Vec::new();
        for (plan, side) in plans.iter().zip(&near) {
            let shrunk = side.nodes.map(|n| moved[&n]);
            // `add_block` wants the far face first, wound so that its own
            // normal points away from the block. The patch face is wound
            // away from **its** block, which is the other way round, and
            // listing the shrunk corners first puts that right.
            let mut corners = [[0.0; 3]; 8];
            corners[..4].copy_from_slice(&shrunk);
            corners[4..].copy_from_slice(&plan.points);
            layers.push(Layer {
                block: self.add_block(corners)?,
                shrunk,
            });
        }

        // Each layer onto the block it came off, the existing face named
        // first so that its nodes keep the names they had — and then onto
        // whatever was on the far side, which has not moved.
        for ((plan, side), layer) in plans.iter().zip(&near).zip(&layers) {
            let under = self
                .face_at(layer.block, &layer.shrunk)
                .ok_or(BlockingError::LostFace)?;
            self.glue_faces(side.face, under)?;
            if let Some(behind) = plan.behind {
                let sides = self.block_faces();
                let far = self
                    .side_at(&sides, behind, &plan.points)
                    .ok_or(BlockingError::LostFace)?;
                let over = self
                    .face_at(layer.block, &plan.points)
                    .ok_or(BlockingError::LostFace)?;
                self.glue_faces(far.face, over)?;
            }
        }

        // And to each other, wherever two selected faces met along an edge.
        for a in 0..plans.len() {
            for b in (a + 1)..plans.len() {
                for i in 0..4 {
                    let edge = [plans[a].points[i], plans[a].points[(i + 1) % 4]];
                    let shared = (0..4).any(|j| {
                        let other = [plans[b].points[j], plans[b].points[(j + 1) % 4]];
                        same_point(edge[0], other[0]) && same_point(edge[1], other[1])
                            || same_point(edge[0], other[1]) && same_point(edge[1], other[0])
                    });
                    if !shared {
                        continue;
                    }
                    let wall = |n: usize, i: usize| {
                        [
                            plans[n].points[i],
                            plans[n].points[(i + 1) % 4],
                            layers[n].shrunk[(i + 1) % 4],
                            layers[n].shrunk[i],
                        ]
                    };
                    let here = self.face_at(layers[a].block, &wall(a, i));
                    let there = (0..4).find_map(|j| {
                        self.face_at(layers[b].block, &wall(b, j)).filter(|_| {
                            let (w, m) = (wall(b, j), wall(a, i));
                            (0..4).all(|k| m.iter().any(|p| same_point(*p, w[k])))
                        })
                    });
                    if let (Some(x), Some(y)) = (here, there) {
                        self.glue_faces(x, y)?;
                    }
                }
            }
        }

        self.assign_missing_ids();
        Ok(())
    }

    /// The side of `block` whose four corners are these four points.
    fn side_at(
        &self,
        sides: &[BlockFace],
        block: CellId,
        points: &[[f64; 3]; 4],
    ) -> Option<BlockFace> {
        sides
            .iter()
            .filter(|s| s.block == block)
            .find(|s| {
                s.nodes.iter().all(|n| {
                    let p = self.position(*n).unwrap_or([0.0; 3]);
                    points.iter().any(|q| same_point(p, *q))
                })
            })
            .copied()
    }

    /// The centre of a block: the mean of the corners its faces list, each
    /// of the eight counted once per face it is on, which weighs them all
    /// the same.
    fn block_centre(&self, sides: &[BlockFace], block: CellId) -> [f64; 3] {
        let mut sum = [0.0; 3];
        let mut n = 0.0_f64;
        for side in sides.iter().filter(|s| s.block == block) {
            for node in side.nodes {
                let p = self.position(node).unwrap_or([0.0; 3]);
                for k in 0..3 {
                    sum[k] += p[k];
                }
                n += 1.0;
            }
        }
        sum.map(|x| x / n.max(1.0))
    }

    /// The face of a block whose four corners are these four points.
    fn face_at(&self, block: CellId, points: &[[f64; 3]; 4]) -> Option<CellId> {
        self.block_faces()
            .iter()
            .filter(|s| s.block == block)
            .find(|s| {
                s.nodes.iter().all(|n| {
                    let p = self.position(*n).unwrap_or([0.0; 3]);
                    points.iter().any(|q| same_point(p, *q))
                })
            })
            .map(|s| s.face)
    }

    /// Every β₁ cycle in the map: one per face side.
    fn face_sides(&self) -> Vec<Vec<DartIdType>> {
        let mut seen = vec![false; self.map.n_darts()];
        let mut sides = Vec::new();
        for start in 1..self.map.n_darts() as DartIdType {
            if self.map.is_unused(start) || seen[start as usize] {
                continue;
            }
            let mut cycle = Vec::new();
            let mut d = start;
            loop {
                seen[d as usize] = true;
                cycle.push(d);
                d = self.map.beta::<1>(d);
                if d == start || d == NULL_DART_ID {
                    break;
                }
            }
            sides.push(cycle);
        }
        sides
    }

    /// Cut one face side in two, along a new edge between its two new nodes.
    ///
    /// Returns the pair of darts on that new edge: the first lies on the
    /// **`from`** side of the sheet, the second on the `to` side. Which is
    /// which is not a detail — it is what tells the block split which half
    /// of itself each quadrangle belongs to.
    fn split_face_side(
        &mut self,
        cycle: &[DartIdType],
        fresh: &HashSet<VertexIdType>,
        from_nodes: &HashSet<VertexIdType>,
    ) -> Option<(DartIdType, DartIdType)> {
        let starts: Vec<VertexIdType> = cycle.iter().map(|&d| self.map.vertex_id(d)).collect();
        let marks: Vec<usize> = (0..cycle.len())
            .filter(|&k| fresh.contains(&starts[k]))
            .collect();
        // Two new nodes, opposite each other: anything else is not a face
        // this cut touches.
        if marks.len() != 2
            || !cycle.len().is_multiple_of(2)
            || marks[1] - marks[0] != cycle.len() / 2
        {
            return None;
        }
        // The half that keeps the `from` side is the one entered from a
        // `from` node, and exactly one of the two is.
        let n = cycle.len();
        let i = if from_nodes.contains(&starts[(marks[0] + n - 1) % n]) {
            marks[0]
        } else {
            marks[1]
        };
        let j = (i + n / 2) % n;

        let base = self.map.allocate_used_darts(2);
        let (u, v) = (base, base + 1);
        let (before_i, before_j) = (cycle[(i + n - 1) % n], cycle[(j + n - 1) % n]);

        self.map.set_beta::<1>(before_i, u);
        self.map.set_beta::<0>(u, before_i);
        self.map.set_beta::<1>(u, cycle[j]);
        self.map.set_beta::<0>(cycle[j], u);

        self.map.set_beta::<1>(before_j, v);
        self.map.set_beta::<0>(v, before_j);
        self.map.set_beta::<1>(v, cycle[i]);
        self.map.set_beta::<0>(cycle[i], v);

        self.map.set_beta::<2>(u, v);
        self.map.set_beta::<2>(v, u);
        Some((u, v))
    }

    /// The faces an edge belongs to, by identifier, without repeats.
    fn faces_around(&self, edge: EdgeIdType) -> Vec<FaceIdType> {
        let mut faces: Vec<FaceIdType> = self
            .map
            .orbit(OrbitPolicy::Edge, edge as DartIdType)
            .map(|d| self.map.face_id(d))
            .collect();
        faces.sort_unstable();
        faces.dedup();
        faces
    }

    /// A face's four corners, in the order β₁ walks them.
    fn face_corners(&self, face: FaceIdType) -> [VertexIdType; 4] {
        self.face_darts(face).map(|d| self.map.vertex_id(d))
    }

    /// The edge of this face joining those two corners.
    fn edge_between(
        &self,
        face: FaceIdType,
        from: VertexIdType,
        to: VertexIdType,
    ) -> Option<EdgeIdType> {
        self.face_darts(face).into_iter().find_map(|d| {
            let (a, b) = self.edge_endpoints(self.map.edge_id(d));
            ((a, b) == (from, to) || (a, b) == (to, from)).then(|| self.map.edge_id(d))
        })
    }

    /// The edge a stable name currently stands for. A scan, for the same
    /// reason [`node_by_id`][Self::node_by_id] is one.
    pub fn edge_by_id(&self, edge: CellId) -> Option<EdgeIdType> {
        if edge.is_none() {
            return None;
        }
        self.map.iter_edges().find(|e| self.edge_tag(*e).id == edge)
    }

    /// The four darts of a face, in the order β₁ walks them.
    ///
    /// Only meaningful for a face with one side: for a glued face the
    /// identifier is the smallest dart of an orbit spanning both blocks, and
    /// which of the two you would get is not something a caller should have to
    /// reason about. [`glue_faces`][Self::glue_faces] refuses those anyway.
    pub(crate) fn face_darts(&self, face: FaceIdType) -> [DartIdType; 4] {
        let mut out = [0; 4];
        let mut dart = face as DartIdType;
        for slot in out.iter_mut() {
            *slot = dart;
            dart = self.map.beta::<1>(dart);
        }
        out
    }

    /// The face a stable name currently stands for. A scan, for the same
    /// reason [`node_by_id`][Self::node_by_id] is one.
    pub fn face_by_id(&self, face: CellId) -> Option<FaceIdType> {
        if face.is_none() {
            return None;
        }
        self.map.iter_faces().find(|f| self.face_tag(*f).id == face)
    }

    /// Every face of every block.
    ///
    /// Listed per block *side* rather than per face. Today that is the same
    /// thing — `add_block` leaves β₃ unset, so no face is shared and this
    /// yields six entries per block — but once blocks are sewn a face between
    /// two of them belongs to both, with opposite windings, and each block
    /// still wants a closed surface of its own. Having the shape right now
    /// costs nothing and saves a format change then.
    pub fn block_faces(&self) -> Vec<BlockFace> {
        let mut out = Vec::new();
        for volume in self.map.iter_volumes() {
            let block = self.block_tag(volume).id;

            // The first dart of each face **on this block's side**. `face_id`
            // alone would not do: it is the smallest dart of the whole face
            // orbit, which once β₃ is set may well be the other block's.
            let mut sides: Vec<(FaceIdType, DartIdType)> = Vec::new();
            for dart in self.map.orbit(OrbitPolicy::Volume, volume as DartIdType) {
                let face = self.map.face_id(dart);
                if !sides.iter().any(|(seen, _)| *seen == face) {
                    sides.push((face, dart));
                }
            }
            // Sorted, so the order cannot depend on how the orbit happened to
            // be walked. Both modes ship these bytes and compare them.
            sides.sort_by_key(|(face, _)| *face);

            for (face, dart) in sides {
                let mut nodes = [0; 4];
                let mut walk = dart;
                for slot in nodes.iter_mut() {
                    *slot = self.map.vertex_id(walk);
                    walk = self.map.beta::<1>(walk);
                }
                debug_assert_eq!(walk, dart, "a block face that is not a quadrangle");
                out.push(BlockFace {
                    face: self.face_tag(face).id,
                    block,
                    nodes,
                });
            }
        }
        out
    }

    // --- reading ----------------------------------------------------------

    /// The map itself. Topological questions belong to it, not here.
    pub fn map(&self) -> &CMap3<f64> {
        &self.map
    }

    pub fn node_tag(&self, node: VertexIdType) -> NodeTag {
        self.map.read_attribute(node).unwrap_or_default()
    }

    pub fn edge_tag(&self, edge: EdgeIdType) -> EdgeTag {
        self.map.read_attribute(edge).unwrap_or_default()
    }

    pub fn face_tag(&self, face: FaceIdType) -> FaceTag {
        self.map.read_attribute(face).unwrap_or_default()
    }

    pub fn block_tag(&self, block: VolumeIdType) -> BlockTag {
        self.map.read_attribute(block).unwrap_or_default()
    }

    /// The two nodes an edge joins.
    ///
    /// An edge is named by one of its darts; that dart starts at one end, and
    /// β₁ takes it to the other.
    pub fn edge_endpoints(&self, edge: EdgeIdType) -> (VertexIdType, VertexIdType) {
        let dart = edge as DartIdType;
        (
            self.map.vertex_id(dart),
            self.map.vertex_id(self.map.beta::<1>(dart)),
        )
    }

    pub fn position(&self, node: VertexIdType) -> Option<[f64; 3]> {
        self.map.read_vertex(node).map(|v| [v.0, v.1, v.2])
    }

    pub fn nb_nodes(&self) -> usize {
        self.map.iter_vertices().count()
    }

    pub fn nb_edges(&self) -> usize {
        self.map.iter_edges().count()
    }

    pub fn nb_faces(&self) -> usize {
        self.map.iter_faces().count()
    }

    pub fn nb_blocks(&self) -> usize {
        self.map.iter_volumes().count()
    }

    /// Darts of the orbit a cell stands for.
    pub fn orbit(&self, policy: OrbitPolicy, dart: DartIdType) -> Vec<DartIdType> {
        self.map.orbit(policy, dart).collect()
    }

    /// How many names have been handed out at each dimension. Only ever goes
    /// up, which is what makes a [`CellId`] safe to keep.
    pub fn names_handed_out(&self) -> [u32; 4] {
        [
            self.names[0].handed_out(),
            self.names[1].handed_out(),
            self.names[2].handed_out(),
            self.names[3].handed_out(),
        ]
    }
}

#[cfg(test)]
mod surgery {
    //! The dart surgery, tested where it lives.
    //!
    //! These reach into private operations on purpose. A sheet cut is three
    //! of them stacked, and finding out which one is wrong from the shape of
    //! the finished mesh is exactly the debugging session worth avoiding.

    use super::*;

    fn cube() -> Blocking {
        let mut b = Blocking::new().unwrap();
        b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
        b
    }

    /// Everything a well-formed map owes us, checked over every used dart.
    pub(super) fn is_coherent(b: &Blocking) {
        for d in 1..b.map.n_darts() as DartIdType {
            if b.map.is_unused(d) {
                continue;
            }
            let next = b.map.beta::<1>(d);
            assert_ne!(next, NULL_DART_ID, "dart {d} has no β₁");
            assert_eq!(b.map.beta::<0>(next), d, "β₀ does not undo β₁ at {d}");
            for (i, partner) in [(2, b.map.beta::<2>(d)), (3, b.map.beta::<3>(d))] {
                if partner == NULL_DART_ID {
                    continue;
                }
                let back = if i == 2 {
                    b.map.beta::<2>(partner)
                } else {
                    b.map.beta::<3>(partner)
                };
                assert_eq!(back, d, "β{i} is not an involution at {d}");
                // And it joins the same two nodes, the other way round.
                let (u, v) = b.edge_endpoints(b.map.edge_id(d));
                let (x, y) = b.edge_endpoints(b.map.edge_id(partner));
                assert_eq!((u, v), (x, y), "β{i} at {d} joins a different edge");
            }
        }
    }

    #[test]
    fn splitting_an_edge_adds_a_node_and_an_edge() {
        let mut b = cube();
        let edge = b.map.iter_edges().next().unwrap();
        let (u, v) = b.edge_endpoints(edge);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
        let middle = [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0);

        b.split_edge(edge, middle).unwrap();
        b.assign_missing_ids();

        assert_eq!(
            (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
            (9, 13, 6, 1)
        );
        is_coherent(&b);
        assert!(
            b.nodes()
                .any(|(_, at)| (0..3).all(|k| (at[k] - middle[k]).abs() < 1e-9)),
            "the new node is not where it was asked for"
        );
    }

    #[test]
    fn the_faces_either_side_grow_by_one() {
        // The edge's two faces become pentagons. They are still those two
        // faces: splitting an edge does not split a face.
        let mut b = cube();
        let edge = b.map.iter_edges().next().unwrap();
        let before: Vec<usize> = b
            .faces_around(edge)
            .iter()
            .map(|f| b.map.orbit(OrbitPolicy::Face, *f as DartIdType).count())
            .collect();
        assert_eq!(before, vec![4, 4]);

        let (u, v) = b.edge_endpoints(edge);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
        b.split_edge(edge, [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0))
            .unwrap();
        b.assign_missing_ids();

        let after: Vec<usize> = b
            .map
            .iter_faces()
            .map(|f| b.map.orbit(OrbitPolicy::Face, f as DartIdType).count())
            .collect();
        assert_eq!(after.iter().filter(|n| **n == 5).count(), 2);
        assert_eq!(after.iter().filter(|n| **n == 4).count(), 4);
    }

    #[test]
    fn both_halves_come_out_unnamed_and_are_then_named_afresh() {
        let mut b = cube();
        let edge = b.map.iter_edges().next().unwrap();
        let was = b.edge_tag(edge).id;
        let (u, v) = b.edge_endpoints(edge);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());

        b.split_edge(edge, [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0))
            .unwrap();
        b.assign_missing_ids();

        // What comes out of a split is two new cells, not one of them
        // inheriting the name.
        let names: Vec<_> = b.map.iter_edges().map(|e| b.edge_tag(e).id).collect();
        assert!(!names.contains(&was), "the old name survived the split");
        assert!(names.iter().all(|n| n.is_some()));
    }

    #[test]
    fn an_edge_shared_by_two_blocks_splits_on_both_sides() {
        // The β₃ path: the ring of darts round the edge spans both blocks,
        // and every one of them has to be cut.
        let mut b = cube();
        b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
        let mut names: Vec<_> = b.block_faces().iter().map(|s| s.block).collect();
        names.dedup();
        let seam = |b: &Blocking, block: CellId| {
            b.block_faces()
                .iter()
                .filter(|s| s.block == block)
                .find(|s| {
                    s.nodes
                        .iter()
                        .all(|n| (b.position(*n).unwrap()[0] - 1.0).abs() < 1e-9)
                })
                .unwrap()
                .face
        };
        let (first, second) = (seam(&b, names[0]), seam(&b, names[1]));
        b.glue_faces(first, second).unwrap();

        // An edge lying in the seam belongs to both blocks: four darts.
        let shared = b
            .map
            .iter_edges()
            .find(|e| {
                let (u, v) = b.edge_endpoints(*e);
                [u, v]
                    .iter()
                    .all(|n| (b.position(*n).unwrap()[0] - 1.0).abs() < 1e-9)
            })
            .expect("an edge in the seam");
        assert_eq!(
            b.map.orbit(OrbitPolicy::Edge, shared as DartIdType).count(),
            4
        );

        let (u, v) = b.edge_endpoints(shared);
        let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
        b.split_edge(shared, [0, 1, 2].map(|k| (p[k] + q[k]) / 2.0))
            .unwrap();
        b.assign_missing_ids();

        // One node and one edge more, and the blocks still share what they
        // shared: the two halves each have four darts round them.
        assert_eq!((b.nb_nodes(), b.nb_edges(), b.nb_blocks()), (13, 21, 2));
        is_coherent(&b);
    }
}

#[cfg(test)]
mod cutting {
    //! Cutting a sheet: the three surgeries stacked.

    use super::*;
    use crate::blocking::surgery::is_coherent;

    fn cube() -> Blocking {
        let mut b = Blocking::new().unwrap();
        b.add_block(cube_corners([0.0, 0.0, 0.0], 1.0)).unwrap();
        b
    }

    /// An edge whose ends differ only along this axis.
    fn along(b: &Blocking, axis: usize) -> CellId {
        b.map
            .iter_edges()
            .find(|e| {
                let (u, v) = b.edge_endpoints(*e);
                let (p, q) = (b.position(u).unwrap(), b.position(v).unwrap());
                (0..3).all(|k| {
                    if k == axis {
                        (p[k] - q[k]).abs() > 0.5
                    } else {
                        (p[k] - q[k]).abs() < 1e-9
                    }
                })
            })
            .map(|e| b.edge_tag(e).id)
            .expect("an edge along that axis")
    }

    #[test]
    fn cutting_a_lone_cube_makes_two_blocks() {
        let mut b = cube();
        b.split_sheet(along(&b, 2), 0.25).unwrap();

        // Two stacked hexahedra: twelve nodes, twenty edges, eleven faces.
        // The same counts as two cubes glued, which is what this now is.
        assert_eq!(
            (b.nb_nodes(), b.nb_edges(), b.nb_faces(), b.nb_blocks()),
            (12, 20, 11, 2)
        );
        is_coherent(&b);
        assert!(b.block_faces().len() == 12, "each block a closed surface");
    }

    #[test]
    fn the_cut_lands_where_the_parameter_says() {
        let mut b = cube();
        b.split_sheet(along(&b, 2), 0.25).unwrap();

        // Four new nodes on the plane, and none anywhere else.
        let at: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
        assert_eq!(at.iter().filter(|z| (**z - 0.25).abs() < 1e-9).count(), 4);
        assert_eq!(at.iter().filter(|z| z.abs() < 1e-9).count(), 4);
        assert_eq!(at.iter().filter(|z| (**z - 1.0).abs() < 1e-9).count(), 4);
    }

    #[test]
    fn one_parameter_means_one_plane_across_the_whole_sheet() {
        // The concertina test. Two blocks glued, cut along a sheet that
        // crosses both: an orientation that flipped on the way through would
        // put half the new nodes at `t` and half at `1 - t`.
        let mut b = cube();
        b.add_block(cube_corners([1.0, 0.0, 0.0], 1.0)).unwrap();
        let mut names: Vec<_> = b.block_faces().iter().map(|s| s.block).collect();
        names.dedup();
        let seam = |b: &Blocking, block: CellId| {
            b.block_faces()
                .iter()
                .filter(|s| s.block == block)
                .find(|s| {
                    s.nodes
                        .iter()
                        .all(|n| (b.position(*n).unwrap()[0] - 1.0).abs() < 1e-9)
                })
                .unwrap()
                .face
        };
        let (first, second) = (seam(&b, names[0]), seam(&b, names[1]));
        b.glue_faces(first, second).unwrap();

        b.split_sheet(along(&b, 2), 0.25).unwrap();

        let at: Vec<f64> = b.nodes().map(|(_, p)| p[2]).collect();
        assert_eq!(
            at.iter().filter(|z| (**z - 0.25).abs() < 1e-9).count(),
            6,
            "six nodes on one plane, not some at 0.25 and some at 0.75"
        );
        assert!(
            at.iter()
                .all(|z| z.abs() < 1e-9 || (*z - 0.25).abs() < 1e-9 || (*z - 1.0).abs() < 1e-9),
            "a node landed off the cut plane: {at:?}"
        );
        assert_eq!(b.nb_blocks(), 4);
        is_coherent(&b);
    }

    #[test]
    fn a_cut_can_be_cut_again() {
        // The strongest check that the result is a proper mesh and not
        // something that merely counts right.
        let mut b = cube();
        b.split_sheet(along(&b, 2), 0.5).unwrap();
        assert_eq!(b.nb_blocks(), 2);
        b.split_sheet(along(&b, 0), 0.5).unwrap();
        assert_eq!(b.nb_blocks(), 4);
        b.split_sheet(along(&b, 1), 0.5).unwrap();
        assert_eq!(b.nb_blocks(), 8);
        assert_eq!(
            (b.nb_nodes(), b.nb_edges(), b.nb_faces()),
            (27, 54, 36),
            "a 2x2x2 grid of hexahedra"
        );
        is_coherent(&b);
    }

    #[test]
    fn every_name_in_the_cut_is_fresh_and_nothing_is_left_unnamed() {
        let mut b = cube();
        let before: Vec<CellId> = b.map.iter_faces().map(|f| b.face_tag(f).id).collect();
        b.split_sheet(along(&b, 2), 0.5).unwrap();

        let after: Vec<CellId> = b.map.iter_faces().map(|f| b.face_tag(f).id).collect();
        assert!(after.iter().all(|n| n.is_some()), "a face came out unnamed");
        // The four faces the sheet crossed are gone, replaced by eight new
        // ones plus the cut face; the two it did not touch keep their names.
        assert_eq!(after.iter().filter(|n| before.contains(n)).count(), 2);
        assert!(b.nodes().all(|(id, _)| id.is_some()));
    }

    #[test]
    fn a_parameter_outside_the_edge_is_refused() {
        for t in [0.0, 1.0, -0.5, 1.5, f64::NAN] {
            let mut b = cube();
            let before = (b.nb_nodes(), b.nb_edges(), b.nb_blocks());
            assert!(
                matches!(
                    b.split_sheet(along(&b, 2), t),
                    Err(BlockingError::BadParameter(_))
                ),
                "t = {t} should be refused"
            );
            assert_eq!((b.nb_nodes(), b.nb_edges(), b.nb_blocks()), before);
        }
    }
}
