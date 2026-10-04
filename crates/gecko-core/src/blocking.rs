//! The block structure.
//!
//! A thin, deliberate layer over [`CMap3`]: the map owns the topology, this
//! owns the naming. Nothing here reimplements what a combinatorial map already
//! does — sewing, orbits, attribute merge and split are the map's business, and
//! the one thing it cannot do for us is hand out names that survive its own
//! renumbering.

use std::collections::HashMap;

use honeycomb_core::attributes::AttributeError;
use honeycomb_core::cmap::{
    CMap3, CMapBuilder, DartIdType, EdgeIdType, FaceIdType, LinkError, OrbitPolicy, VertexIdType,
    VolumeIdType,
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

#[derive(Debug, Error)]
pub enum BlockingError {
    #[error("could not build the map: {0}")]
    Build(String),
    #[error("could not link darts: {0}")]
    Link(#[from] LinkError),
    #[error("attribute error: {0}")]
    Attribute(#[from] AttributeError),
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
