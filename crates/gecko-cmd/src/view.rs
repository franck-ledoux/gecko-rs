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
/// 2 added the node names. The field is bumped rather than quietly reused: two
/// independent decoders read this, and a silent change is how one of them ends
/// up reading the wrong field as a coordinate.
pub const VERSION: u32 = 2;
/// magic + version + f64 centre + two counts.
pub const HEADER: usize = 40;

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
        for e in blocking.map().iter_edges() {
            let (a, b) = blocking.edge_endpoints(e);
            if let (Some(a), Some(b)) = (at.get(&a), at.get(&b)) {
                edges.extend_from_slice(&[*a, *b]);
            }
        }

        ViewBuffer {
            centre,
            positions,
            names,
            edges,
        }
    }

    /// Flat little-endian encoding. The f64 centre sits 8-aligned and
    /// everything after it 4-aligned, so a client can view the bytes as typed
    /// arrays without copying.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            HEADER + (self.positions.len() + self.names.len() + self.edges.len()) * 4,
        );
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
        for x in &self.names {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.edges {
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
