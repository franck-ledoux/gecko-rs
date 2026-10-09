//! The binary view buffer.
//!
//! What the browser draws, in the one shape both modes deliver it: the static
//! page reads it straight out of wasm memory, the station sends the identical
//! bytes down a WebSocket. One producer, one decoder, no second format to drift.
//!
//! Positions are **f32 relative to the model centre**, with the centre shipped
//! alongside as f64. WebGL has nothing but f32, and a model parked far from the
//! origin quantises: at 10⁶ units, f32 steps are about 0.06, which is visible
//! jitter and z-fighting. Rebasing costs nothing and cannot be retrofitted
//! cheaply once a client assumes absolute coordinates.

use gecko_core::Blocking;
use std::collections::HashMap;

pub const MAGIC: [u8; 4] = *b"GKB1";
/// 2 added the node names, 3 the surface, 4 the edge names. The field is
/// bumped rather than quietly reused: three independent decoders read this,
/// and a silent change is how one of them ends up reading the wrong field as
/// a coordinate.
pub const VERSION: u32 = 4;
/// magic + version + f64 centre + three counts.
pub const HEADER: usize = 44;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewBuffer {
    pub centre: [f64; 3],
    /// Three f32 per node, relative to `centre`.
    pub positions: Vec<f32>,
    /// The stable name of each node, in the order `positions` lists them.
    ///
    /// Without this the browser could pick a sphere and have nothing to call it
    /// — and calling it by its position in this array would only work until the
    /// next edit renumbered things.
    pub names: Vec<u32>,
    /// Two indices into `positions` per edge.
    pub edges: Vec<u32>,
    /// The stable name of each edge, in the order `edges` lists them.
    ///
    /// What lets a click on a line say which edge it was — and so what lets
    /// the viewer ask the kernel for the sheet through it.
    pub edge_names: Vec<u32>,
    /// Four indices into `positions` per quadrangle, wound outward.
    ///
    /// One quadrangle per **(face, block) pair**, which is why there are two
    /// name arrays beside it rather than one: the same surface is what the
    /// viewer draws for faces and for blocks, and only the name it colours by
    /// differs. Once blocks are joined, a shared face will appear twice here —
    /// once per block, wound each way — and that is what gives each block a
    /// closed surface of its own.
    pub quads: Vec<u32>,
    /// The face's stable name, one per quadrangle.
    pub quad_faces: Vec<u32>,
    /// The block's stable name, one per quadrangle.
    pub quad_blocks: Vec<u32>,
}

impl ViewBuffer {
    /// Build the view, reusing `previous` as the rebasing origin when it is
    /// still close enough to serve.
    pub fn of(blocking: &Blocking, previous: Option<[f64; 3]>) -> ViewBuffer {
        let nodes: Vec<_> = blocking.map().iter_vertices().collect();
        let mut at: HashMap<u32, u32> = HashMap::with_capacity(nodes.len());
        let mut world = Vec::with_capacity(nodes.len() * 3);
        let mut names = Vec::with_capacity(nodes.len());
        for (i, n) in nodes.iter().enumerate() {
            at.insert(*n, i as u32);
            world.extend_from_slice(&blocking.position(*n).unwrap_or([0.0; 3]));
            names.push(blocking.node_tag(*n).id.get().unwrap_or(u32::MAX));
        }

        let centre = stable_centre(&world, previous);
        // `as_chunks` rather than `chunks_exact`: it yields `&[f64; 3]`, so the
        // three coordinates are a type rather than a convention.
        let positions = world
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| {
                [
                    (p[0] - centre[0]) as f32,
                    (p[1] - centre[1]) as f32,
                    (p[2] - centre[2]) as f32,
                ]
            })
            .collect();

        let mut edges = Vec::new();
        let mut edge_names = Vec::new();
        for e in blocking.map().iter_edges() {
            let (a, b) = blocking.edge_endpoints(e);
            if let (Some(a), Some(b)) = (at.get(&a), at.get(&b)) {
                edges.extend_from_slice(&[*a, *b]);
                edge_names.push(blocking.edge_tag(e).id.get().unwrap_or(u32::MAX));
            }
        }

        let sides = blocking.block_faces();
        let mut quads = Vec::with_capacity(sides.len() * 4);
        let mut quad_faces = Vec::with_capacity(sides.len());
        let mut quad_blocks = Vec::with_capacity(sides.len());
        for side in &sides {
            // All four or none: a quadrangle missing a corner is not a smaller
            // quadrangle, it is a hole in the index arithmetic downstream.
            let Some(corners) = side
                .nodes
                .iter()
                .map(|n| at.get(n).copied())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            quads.extend_from_slice(&corners);
            quad_faces.push(side.face.get().unwrap_or(u32::MAX));
            quad_blocks.push(side.block.get().unwrap_or(u32::MAX));
        }

        ViewBuffer {
            centre,
            positions,
            names,
            edges,
            edge_names,
            quads,
            quad_faces,
            quad_blocks,
        }
    }

    /// Flat little-endian encoding. The f64 centre sits 8-aligned and
    /// everything after it 4-aligned, so a client can view the bytes as typed
    /// arrays without copying.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            HEADER
                + (self.positions.len()
                    + self.names.len()
                    + self.edges.len()
                    + self.edge_names.len()
                    + self.quads.len()
                    + self.quad_faces.len()
                    + self.quad_blocks.len())
                    * 4,
        );
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        for c in self.centre {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&((self.positions.len() / 3) as u32).to_le_bytes());
        out.extend_from_slice(&((self.edges.len() / 2) as u32).to_le_bytes());
        out.extend_from_slice(&(self.quad_faces.len() as u32).to_le_bytes());
        for x in &self.positions {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.names {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.edges {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.edge_names {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.quads {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.quad_faces {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.quad_blocks {
            out.extend_from_slice(&x.to_le_bytes());
        }
        out
    }
}

/// How far the model may wander from its origin before the origin follows.
/// Half a diagonal keeps every coordinate comfortably small for f32 while
/// leaving the origin alone for anything short of a wholesale move.
const DRIFT: f64 = 0.5;

/// The rebasing origin: the previous one if it still serves, otherwise the
/// model's centre.
///
/// Recomputing the centre on every edit means **every edit shifts the whole
/// scene**: drag one node and the other seven slide across the screen,
/// because all of them are drawn relative to a centre that just moved. The
/// origin exists for precision, not for centring, so it only has to stay
/// *near* the model — and staying put is worth more than staying centred.
pub fn stable_centre(world: &[f64], previous: Option<[f64; 3]>) -> [f64; 3] {
    if world.is_empty() {
        return previous.unwrap_or([0.0; 3]);
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in world.as_chunks::<3>().0 {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let centre = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let Some(previous) = previous else {
        return centre;
    };
    let diagonal = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt();
    let drifted = (0..3)
        .map(|k| (previous[k] - centre[k]).powi(2))
        .sum::<f64>()
        .sqrt();
    if drifted <= DRIFT * diagonal.max(f64::MIN_POSITIVE) {
        previous
    } else {
        centre
    }
}
