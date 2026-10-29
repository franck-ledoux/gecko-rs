//! The binary view buffer.
//!
//! What the browser draws, in the one shape both modes deliver it: the static
//! page reads it straight out of wasm memory, the station sends the identical
//! bytes down a WebSocket. One producer, one decoder, no second format to drift.
//!
//! Positions are **f32 relative to the model center**, with the center shipped
//! alongside as f64. WebGL has nothing but f32, and a model parked far from the
//! origin quantises: at 10⁶ units, f32 steps are about 0.06, which is visible
//! jitter and z-fighting. Rebasing costs nothing and cannot be retrofitted
//! cheaply once a client assumes absolute coordinates.

use gecko_core::{Blocking, Mesh};
use gecko_geom::Geometry;
use std::collections::HashMap;

pub const MAGIC: [u8; 4] = *b"GKB1";
/// 2 added the node names, 3 the surface, 4 the edge names, 5 what each
/// node and edge is classified on, 6 the same for each face, 7 which
/// groups each cell is in. The field is bumped rather than quietly reused:
/// three independent decoders read this, and a silent change is how one of
/// them ends up reading the wrong field as a coordinate.
pub const VERSION: u32 = 7;
/// magic + version + f64 center + three counts.
pub const HEADER: usize = 44;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewBuffer {
    pub center: [f64; 3],
    /// Three f32 per node, relative to `center`.
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
    /// viewer draws for faces and for blocks, and only the name it colors by
    /// differs. Once blocks are joined, a shared face will appear twice here —
    /// once per block, wound each way — and that is what gives each block a
    /// closed surface of its own.
    pub quads: Vec<u32>,
    /// The face's stable name, one per quadrangle.
    pub quad_faces: Vec<u32>,
    /// The block's stable name, one per quadrangle.
    pub quad_blocks: Vec<u32>,
    /// What each node is classified on: the entity's dimension, −1 for
    /// none. Signed, like the tag beside it, and for the same reason.
    pub node_class_dim: Vec<i32>,
    /// The gmsh tag of that entity, meaningless where the dimension is −1.
    pub node_class_tag: Vec<i32>,
    /// 1 where a node is frozen. Its own array rather than a bit stolen
    /// from the dimension: a frozen node need not be classified at all.
    pub node_frozen: Vec<u32>,
    /// The same two for each edge.
    pub edge_class_dim: Vec<i32>,
    pub edge_class_tag: Vec<i32>,
    /// And for each **quadrangle**, which is where a face is drawn. A face
    /// shared by two blocks appears twice and says the same thing twice.
    pub quad_class_dim: Vec<i32>,
    pub quad_class_tag: Vec<i32>,
    /// Which groups each cell is in, one bit per group — **in two halves**,
    /// low thirty-two bits and high. A `u64` would be one number and the
    /// browser has none: `BigInt64Array` exists, but testing a bit of it
    /// costs an allocation apiece, and the page tests these per cell per
    /// frame while a selection is being filtered.
    pub node_groups_lo: Vec<u32>,
    pub node_groups_hi: Vec<u32>,
    pub edge_groups_lo: Vec<u32>,
    pub edge_groups_hi: Vec<u32>,
    pub quad_groups_lo: Vec<u32>,
    pub quad_groups_hi: Vec<u32>,
    /// 1 where a node or an edge bounds a block that is in **no** group.
    ///
    /// A cell is drawn when any block it bounds is drawn, and the mask
    /// alone cannot say that: an edge between a block of the fluid and a
    /// block of nothing carries the fluid's bit, so hiding the fluid
    /// would take the edge off a block that is still on screen.
    ///
    /// Quadrangles need none: one is a (face, block) pair, so its own
    /// mask already speaks for the one block it is drawn with.
    pub node_loose: Vec<u32>,
    pub edge_loose: Vec<u32>,
}

impl ViewBuffer {
    /// Build the view, rebased on the origin it is **given**.
    ///
    /// Not settled here, on purpose. The session settles one origin over the
    /// blocking *and* the geometry, and rebases the geometry and the mesh on
    /// it too, because the three are drawn on top of each other. A view that
    /// chose an origin of its own — over the blocking alone, which is a
    /// smaller world and so walks past its own threshold sooner — would draw
    /// the blocking an origin-shift away from the model it is meant to
    /// follow, with the mesh and the geometry still where they were. That is
    /// what used to happen when a whole sheet of blocks came out.
    ///
    /// `None` means a session that has never settled one, which is a session
    /// with nothing in it; then there is nothing to disagree with and the
    /// center is chosen here.
    pub fn of(blocking: &Blocking, origin: Option<[f64; 3]>) -> ViewBuffer {
        // Which groups each cell is in, by the blocks that bound it. Empty
        // and free when nobody has made a group, which is most sessions.
        let masks = blocking.group_masks();
        let halves = |mask: u64| ((mask & 0xffff_ffff) as u32, (mask >> 32) as u32);

        let nodes = blocking.map_vertices();
        let mut at: HashMap<u32, u32> = HashMap::with_capacity(nodes.len());
        let mut world = Vec::with_capacity(nodes.len() * 3);
        let mut names = Vec::with_capacity(nodes.len());
        let mut node_class_dim = Vec::with_capacity(nodes.len());
        let mut node_class_tag = Vec::with_capacity(nodes.len());
        let mut node_frozen = Vec::with_capacity(nodes.len());
        let mut node_groups_lo = Vec::with_capacity(nodes.len());
        let mut node_groups_hi = Vec::with_capacity(nodes.len());
        let mut node_loose = Vec::with_capacity(nodes.len());
        for (i, n) in nodes.iter().enumerate() {
            at.insert(*n, i as u32);
            world.extend_from_slice(&blocking.position(*n).unwrap_or([0.0; 3]));
            let tag = blocking.node_tag(*n);
            names.push(tag.id.get().unwrap_or(u32::MAX));
            node_class_dim.push(tag.on.map(|on| on.dim as i32).unwrap_or(-1));
            node_class_tag.push(tag.on.map(|on| on.tag).unwrap_or(0));
            node_frozen.push(u32::from(tag.frozen));
            let (lo, hi) = halves(masks.nodes.get(n).copied().unwrap_or(0));
            node_groups_lo.push(lo);
            node_groups_hi.push(hi);
            node_loose.push(u32::from(masks.loose_nodes.contains(n)));
        }

        let center = origin.unwrap_or_else(|| stable_center(&world, None));
        // `as_chunks` rather than `chunks_exact`: it yields `&[f64; 3]`, so the
        // three coordinates are a type rather than a convention.
        let positions = world
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| {
                [
                    (p[0] - center[0]) as f32,
                    (p[1] - center[1]) as f32,
                    (p[2] - center[2]) as f32,
                ]
            })
            .collect();

        let mut edges = Vec::new();
        let mut edge_names = Vec::new();
        let mut edge_class_dim = Vec::new();
        let mut edge_class_tag = Vec::new();
        let mut edge_groups_lo = Vec::new();
        let mut edge_groups_hi = Vec::new();
        let mut edge_loose = Vec::new();
        for e in blocking.map_edges() {
            let (a, b) = blocking.edge_endpoints(e);
            if let (Some(a), Some(b)) = (at.get(&a), at.get(&b)) {
                let tag = blocking.edge_tag(e);
                edges.extend_from_slice(&[*a, *b]);
                edge_names.push(tag.id.get().unwrap_or(u32::MAX));
                edge_class_dim.push(tag.on.map(|on| on.dim as i32).unwrap_or(-1));
                edge_class_tag.push(tag.on.map(|on| on.tag).unwrap_or(0));
                let (lo, hi) = halves(masks.edges.get(&e).copied().unwrap_or(0));
                edge_groups_lo.push(lo);
                edge_groups_hi.push(hi);
                edge_loose.push(u32::from(masks.loose_edges.contains(&e)));
            }
        }

        let sides = blocking.block_faces();
        let mut quads = Vec::with_capacity(sides.len() * 4);
        let mut quad_faces = Vec::with_capacity(sides.len());
        let mut quad_blocks = Vec::with_capacity(sides.len());
        let mut quad_class_dim = Vec::with_capacity(sides.len());
        let mut quad_class_tag = Vec::with_capacity(sides.len());
        let mut quad_groups_lo = Vec::with_capacity(sides.len());
        let mut quad_groups_hi = Vec::with_capacity(sides.len());
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
            quad_class_dim.push(side.on.map(|on| on.dim as i32).unwrap_or(-1));
            quad_class_tag.push(side.on.map(|on| on.tag).unwrap_or(0));
            // A quadrangle is a (face, block) pair, so its groups are that
            // block's — and the face's own mask, which is the union over
            // both sides, would say the far block's too. By the volume the
            // side already carries: asking for it by name is a scan of
            // every volume, and once per quadrangle that is the whole walk
            // squared.
            let (lo, hi) = halves(blocking.block_tag(side.volume).groups);
            quad_groups_lo.push(lo);
            quad_groups_hi.push(hi);
        }

        ViewBuffer {
            center,
            positions,
            names,
            edges,
            edge_names,
            quads,
            quad_faces,
            quad_blocks,
            node_class_dim,
            node_class_tag,
            node_frozen,
            edge_class_dim,
            edge_class_tag,
            quad_class_dim,
            quad_class_tag,
            node_groups_lo,
            node_groups_hi,
            edge_groups_lo,
            edge_groups_hi,
            quad_groups_lo,
            quad_groups_hi,
            node_loose,
            edge_loose,
        }
    }

    /// Flat little-endian encoding. The f64 center sits 8-aligned and
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
                    + self.quad_blocks.len()
                    + self.node_class_dim.len()
                    + self.node_class_tag.len()
                    + self.node_frozen.len()
                    + self.edge_class_dim.len()
                    + self.edge_class_tag.len()
                    + self.quad_class_dim.len()
                    + self.quad_class_tag.len()
                    + self.node_groups_lo.len() * 3
                    + self.edge_groups_lo.len() * 3
                    + self.quad_groups_lo.len() * 2)
                    * 4,
        );
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        for c in self.center {
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
        // The classification comes last, appended rather than threaded in
        // beside the names it belongs with: every offset before it stays
        // where it was, and a decoder reads the whole thing by walking
        // forward anyway.
        for x in &self.node_class_dim {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.node_class_tag {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.node_frozen {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.edge_class_dim {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.edge_class_tag {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.quad_class_dim {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.quad_class_tag {
            out.extend_from_slice(&x.to_le_bytes());
        }
        // And the groups last, for the same reason the classification came
        // last before them.
        for list in [
            &self.node_groups_lo,
            &self.node_groups_hi,
            &self.node_loose,
            &self.edge_groups_lo,
            &self.edge_groups_hi,
            &self.edge_loose,
            &self.quad_groups_lo,
            &self.quad_groups_hi,
        ] {
            for x in list {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        out
    }
}

/// How far the model may wander from its origin before the origin follows.
/// Half a diagonal keeps every coordinate comfortably small for f32 while
/// leaving the origin alone for anything short of a wholesale move.
const DRIFT: f64 = 0.5;

/// The rebasing origin: the previous one if it still serves, otherwise the
/// model's center.
///
/// Recomputing the center on every edit means **every edit shifts the whole
/// scene**: drag one node and the other seven slide across the screen,
/// because all of them are drawn relative to a center that just moved. The
/// origin exists for precision, not for centring, so it only has to stay
/// *near* the model — and staying put is worth more than staying centerd.
pub fn stable_center(world: &[f64], previous: Option<[f64; 3]>) -> [f64; 3] {
    match bounds(world.as_chunks::<3>().0.iter().copied()) {
        Some((lo, hi)) => center_of(lo, hi, previous),
        None => previous.unwrap_or([0.0; 3]),
    }
}

/// The box round a cloud of points, or nothing when there are none.
///
/// Its own function because the **geometry's** box is worked out once and
/// kept: a model's triangles do not move, and walking ten thousand of them
/// after every edit to find a box that cannot have changed is the kind of
/// work that makes an interactive tool feel slow with a big model open.
pub fn bounds(points: impl Iterator<Item = [f64; 3]>) -> Option<([f64; 3], [f64; 3])> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for p in points {
        any = true;
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    any.then_some((lo, hi))
}

/// The same rule as [`stable_center`], from a box already worked out.
pub fn center_of(lo: [f64; 3], hi: [f64; 3], previous: Option<[f64; 3]>) -> [f64; 3] {
    let center = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let Some(previous) = previous else {
        return center;
    };
    let diagonal = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt();
    let drifted = (0..3)
        .map(|k| (previous[k] - center[k]).powi(2))
        .sum::<f64>()
        .sqrt();
    if drifted <= DRIFT * diagonal.max(f64::MIN_POSITIVE) {
        previous
    } else {
        center
    }
}

pub const GEOMETRY_MAGIC: [u8; 4] = *b"GKG1";
pub const GEOMETRY_VERSION: u32 = 1;
/// magic + version + f64 center + four counts.
pub const GEOMETRY_HEADER: usize = 48;

/// What the browser draws of the **geometry** — a buffer of its own.
///
/// Not a section of [`ViewBuffer`], on purpose. The two change on completely
/// different clocks: the blocking's view is rebuilt after every edit and
/// every dragged node, while a geometry is read once and then sits there.
/// Shipping three thousand triangles down the socket because somebody moved
/// a corner is the whole cost of the obvious design, and the client avoids it
/// by watching a generation number rather than by guessing.
///
/// The `center` is the **same rebasing origin** the view buffer uses. It has
/// to be: both are drawn in one scene, and two origins would put the geometry
/// at a different place from the blocks that follow it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometryBuffer {
    pub center: [f64; 3],
    /// Three f32 per node, relative to `center`, shared by everything below.
    pub positions: Vec<f32>,
    /// Three node indices per triangle.
    pub triangles: Vec<u32>,
    /// The gmsh entity tag of the surface each triangle is on. Signed,
    /// because a gmsh tag is — and a tag read as unsigned would be a
    /// four-billion-ish color in the palette rather than an obvious wrong.
    pub triangle_tags: Vec<i32>,
    /// Two node indices per segment.
    pub segments: Vec<u32>,
    /// The entity tag of the curve each segment is on.
    pub segment_tags: Vec<i32>,
    /// One node index per geometric corner.
    pub corners: Vec<u32>,
    /// The entity tag of each corner.
    pub corner_tags: Vec<i32>,
}

impl GeometryBuffer {
    pub fn of(geometry: &Geometry, center: [f64; 3]) -> GeometryBuffer {
        let positions = geometry
            .nodes()
            .iter()
            .flat_map(|p| {
                [
                    (p[0] - center[0]) as f32,
                    (p[1] - center[1]) as f32,
                    (p[2] - center[2]) as f32,
                ]
            })
            .collect();

        let mut triangles = Vec::with_capacity(geometry.nb_triangles() * 3);
        let mut triangle_tags = Vec::with_capacity(geometry.nb_triangles());
        for surface in geometry.surfaces() {
            for t in &surface.triangles {
                triangles.extend_from_slice(t);
                triangle_tags.push(surface.tag);
            }
        }

        let mut segments = Vec::with_capacity(geometry.nb_segments() * 2);
        let mut segment_tags = Vec::with_capacity(geometry.nb_segments());
        for curve in geometry.curves() {
            for s in &curve.segments {
                segments.extend_from_slice(s);
                segment_tags.push(curve.tag);
            }
        }

        GeometryBuffer {
            center,
            positions,
            triangles,
            triangle_tags,
            segments,
            segment_tags,
            corners: geometry.corners().iter().map(|c| c.node).collect(),
            corner_tags: geometry.corners().iter().map(|c| c.tag).collect(),
        }
    }

    /// The same flat little-endian encoding as [`ViewBuffer::to_bytes`].
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            GEOMETRY_HEADER
                + (self.positions.len()
                    + self.triangles.len()
                    + self.triangle_tags.len()
                    + self.segments.len()
                    + self.segment_tags.len()
                    + self.corners.len()
                    + self.corner_tags.len())
                    * 4,
        );
        out.extend_from_slice(&GEOMETRY_MAGIC);
        out.extend_from_slice(&GEOMETRY_VERSION.to_le_bytes());
        for c in self.center {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&((self.positions.len() / 3) as u32).to_le_bytes());
        out.extend_from_slice(&((self.triangles.len() / 3) as u32).to_le_bytes());
        out.extend_from_slice(&((self.segments.len() / 2) as u32).to_le_bytes());
        out.extend_from_slice(&(self.corners.len() as u32).to_le_bytes());
        for x in &self.positions {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.triangles {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.triangle_tags {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.segments {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.segment_tags {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.corners {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.corner_tags {
            out.extend_from_slice(&x.to_le_bytes());
        }
        out
    }

    /// An empty geometry, so a client that has never been given one still
    /// gets a buffer it can decode rather than a special case.
    pub fn empty(center: [f64; 3]) -> Vec<u8> {
        GeometryBuffer {
            center,
            ..Default::default()
        }
        .to_bytes()
    }
}

pub const MESH_MAGIC: [u8; 4] = *b"GKM1";
pub const MESH_VERSION: u32 = 1;
/// magic + version + f64 center + two counts.
pub const MESH_HEADER: usize = 40;

/// What the browser draws of the **mesh**: its skin, and nothing else.
///
/// The inside of a mesh is the overwhelming majority of it and is never
/// seen. A hundred thousand hexahedra are six hundred thousand quadrangles
/// of which perhaps twenty thousand are on the outside, so sending the
/// whole thing would be thirty times the bytes for the same picture.
///
/// Its own buffer, and its own generation number, for the reason
/// [`GeometryBuffer`] has one: it changes on a clock of its own — not when
/// a node moves, but when somebody asks for a mesh, and when an edit
/// throws the last one away.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshBuffer {
    pub center: [f64; 3],
    /// Three f32 per node of the skin, relative to `center`.
    pub positions: Vec<f32>,
    /// Four node indices per quadrangle.
    pub quads: Vec<u32>,
    /// The block each quadrangle came out of.
    pub blocks: Vec<u32>,
}

impl MeshBuffer {
    pub fn of(mesh: &Mesh, center: [f64; 3]) -> MeshBuffer {
        let (quads, blocks) = mesh.skin();
        // Only the nodes the skin uses, renumbered: the interior ones are
        // most of them, and none of them is drawn.
        let mut seen: HashMap<u32, u32> = HashMap::new();
        let mut positions = Vec::new();
        let mut out = Vec::with_capacity(quads.len() * 4);
        for quad in &quads {
            for node in quad {
                let next = seen.len() as u32;
                let at = *seen.entry(*node).or_insert_with(|| {
                    let p = mesh.points[*node as usize];
                    positions.push((p[0] - center[0]) as f32);
                    positions.push((p[1] - center[1]) as f32);
                    positions.push((p[2] - center[2]) as f32);
                    next
                });
                out.push(at);
            }
        }
        MeshBuffer {
            center,
            positions,
            quads: out,
            blocks: blocks.iter().map(|b| b.get().unwrap_or(u32::MAX)).collect(),
        }
    }

    /// The same flat little-endian encoding as the other two.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            MESH_HEADER + (self.positions.len() + self.quads.len() + self.blocks.len()) * 4,
        );
        out.extend_from_slice(&MESH_MAGIC);
        out.extend_from_slice(&MESH_VERSION.to_le_bytes());
        for c in self.center {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&((self.positions.len() / 3) as u32).to_le_bytes());
        out.extend_from_slice(&((self.quads.len() / 4) as u32).to_le_bytes());
        for x in &self.positions {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.quads {
            out.extend_from_slice(&x.to_le_bytes());
        }
        for x in &self.blocks {
            out.extend_from_slice(&x.to_le_bytes());
        }
        out
    }

    /// No mesh, as a buffer a client can decode rather than a special case.
    pub fn empty(center: [f64; 3]) -> Vec<u8> {
        MeshBuffer {
            center,
            ..Default::default()
        }
        .to_bytes()
    }
}
