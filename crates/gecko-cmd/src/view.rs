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
pub const VERSION: u32 = 1;
/// magic + version + f64 centre + two counts.
pub const HEADER: usize = 40;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewBuffer {
    pub centre: [f64; 3],
    /// Three f32 per node, relative to `centre`.
    pub positions: Vec<f32>,
    /// Two indices into `positions` per edge.
    pub edges: Vec<u32>,
}

impl ViewBuffer {
    pub fn of(blocking: &Blocking) -> ViewBuffer {
        let nodes: Vec<_> = blocking.map().iter_vertices().collect();
        let mut at: HashMap<u32, u32> = HashMap::with_capacity(nodes.len());
        let mut world = Vec::with_capacity(nodes.len() * 3);
        for (i, n) in nodes.iter().enumerate() {
            at.insert(*n, i as u32);
            let p = blocking.position(*n).unwrap_or([0.0; 3]);
            world.extend_from_slice(&p);
        }

        let centre = centre_of(&world);
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
        for e in blocking.map().iter_edges() {
            let (a, b) = blocking.edge_endpoints(e);
            if let (Some(a), Some(b)) = (at.get(&a), at.get(&b)) {
                edges.extend_from_slice(&[*a, *b]);
            }
        }

        ViewBuffer {
            centre,
            positions,
            edges,
        }
    }

    /// Flat little-endian encoding. The f64 centre sits 8-aligned and
    /// everything after it 4-aligned, so a client can view the bytes as typed
    /// arrays without copying.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + self.positions.len() * 4 + self.edges.len() * 4);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        for c in self.centre {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&((self.positions.len() / 3) as u32).to_le_bytes());
        out.extend_from_slice(&((self.edges.len() / 2) as u32).to_le_bytes());
        for x in &self.positions {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.edges {
            out.extend_from_slice(&x.to_le_bytes());
        }
        out
    }
}

fn centre_of(world: &[f64]) -> [f64; 3] {
    if world.is_empty() {
        return [0.0; 3];
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in world.as_chunks::<3>().0 {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ]
}
