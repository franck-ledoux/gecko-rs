//! Turning a block structure into a mesh.
//!
//! Each block becomes a structured grid of hexahedra, placed by **transfinite
//! interpolation**: the edges are divided first, the faces are filled from
//! their four edges, and the inside is filled from its six faces. At every
//! step, a cell classified on the geometry has its points **projected onto
//! what it follows** before the step above it reads them — which is what
//! makes the mesh follow the model rather than merely have its shape.
//!
//! Nothing here knows what a triangle is: the projection arrives as a
//! closure, as it does for [`Blocking::classify_nodes`], because the crate
//! that holds both the cells and the triangles is the one above this.
//!
//! # Nodes are owned by the cell they lie in
//!
//! Two blocks glued along a face must share the mesh nodes on that face, or
//! the mesh has a seam down it that nothing would show until a solver choked
//! on it. Rather than build each block independently and weld coincident
//! points — which works, and needs a tolerance, and is a tolerance that is
//! wrong for somebody's model — every mesh node is **named by the cell of
//! the blocking it lies in**:
//!
//! - a corner of a block is a `Node`;
//! - a point along a block edge is an `Edge`, numbered from the end with the
//!   lower node name;
//! - a point inside a block face is a `Face`, on that face's own lattice
//!   rather than on either block's;
//! - a point inside a block is `Inside`, and belongs to nobody else.
//!
//! Those are the variants of `Site`, which is the mesher's own key into its
//! table of nodes and nothing a caller ever holds: what comes out is a
//! [`Mesh`], whose points are already shared. They are named here because
//! the naming *is* the mechanism, not because there is anything to reach
//! for.
//!
//! Cells are shared by construction — that is what the combinatorial map is
//! for — so the sharing of nodes comes out of the naming rather than out of
//! arithmetic. There is no tolerance anywhere in this file.

use std::collections::HashMap;

use honeycomb_core::cmap::{EdgeIdType, FaceIdType, OrbitPolicy, VertexIdType};

use crate::attributes::On;
use crate::blocking::Blocking;
use crate::ids::CellId;

/// How many segments an edge is divided into when nobody has said.
pub const SEGMENTS: u32 = 10;

/// A hexahedral mesh.
pub struct Mesh {
    /// Every node, once.
    pub points: Vec<[f64; 3]>,
    /// Eight node indices per hexahedron, in the usual VTK order: one face
    /// anticlockwise seen from outside, then the face above it.
    pub hexes: Vec<[u32; 8]>,
    /// Which block each hexahedron came out of, for anything that wants to
    /// color them.
    pub blocks: Vec<CellId>,
}

/// Which cell of the blocking a mesh node lies in. See the module's own
/// documentation: this is what makes two blocks share the nodes on the
/// face between them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Site {
    Node(CellId),
    /// An interior point of an edge, counted from the end with the lower
    /// node name so that both blocks count the same way.
    Edge(CellId, u32),
    /// An interior point of a face, on the face's own lattice.
    Face(CellId, u32, u32),
    Inside(CellId, u32, u32, u32),
}

/// The twelve edges of a hexahedron, as pairs of corners, in the order the
/// VTK ordering implies: the bottom ring, the top ring, then the uprights.
pub const HEX_EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

/// One block, as the mesher needs it: its eight corners in VTK order, and
/// the cells of the blocking they bound.
pub struct Hex {
    pub block: CellId,
    pub corners: [VertexIdType; 8],
    /// In [`HEX_EDGES`] order.
    pub edges: [EdgeIdType; 12],
    /// In [`crate::blocking::HEX_FACES`] order.
    pub faces: [FaceIdType; 6],
}

impl Mesh {
    /// The quadrangles on the outside: those belonging to one hexahedron
    /// rather than two.
    ///
    /// What a viewer draws. The inside of a mesh is the overwhelming
    /// majority of it and is never seen, so sending it to a browser would
    /// be sending ten times the bytes for the same picture.
    pub fn skin(&self) -> (Vec<[u32; 4]>, Vec<CellId>) {
        // Four corners of each face of a hexahedron, wound outward.
        const FACES: [[usize; 4]; 6] = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ];
        // Keyed by the four nodes, sorted: a face is the same face from
        // either side however it is wound.
        let mut seen: HashMap<[u32; 4], (usize, usize)> = HashMap::new();
        for (h, hex) in self.hexes.iter().enumerate() {
            for (f, corners) in FACES.iter().enumerate() {
                let mut key = corners.map(|c| hex[c]);
                key.sort_unstable();
                seen.entry(key).or_insert((h * 6 + f, 0)).1 += 1;
            }
        }
        let mut quads = Vec::new();
        let mut blocks = Vec::new();
        // Walked in order rather than over the map, so the answer does not
        // depend on the order a hash map happened to hand things back in.
        for (h, hex) in self.hexes.iter().enumerate() {
            for (f, corners) in FACES.iter().enumerate() {
                let mut key = corners.map(|c| hex[c]);
                key.sort_unstable();
                let Some((first, count)) = seen.get(&key) else {
                    continue;
                };
                if *count == 1 && *first == h * 6 + f {
                    quads.push(corners.map(|c| hex[c]));
                    blocks.push(self.blocks[h]);
                }
            }
        }
        (quads, blocks)
    }
}

impl Blocking {
    /// Every block as a hexahedron: its corners in VTK order, and the cells
    /// that bound it.
    ///
    /// The frame is recovered from the map rather than remembered from the
    /// construction, because by now the block may have been cut, glued and
    /// cut again. Any face may serve as the bottom one: the result is the
    /// same hexahedron seen from another of its corners, and a rotation of
    /// a hexahedron is still that hexahedron.
    pub fn hexes(&self) -> Vec<Hex> {
        let map = self.map();
        let mut out = Vec::new();
        for volume in map.iter_volumes() {
            // Blocks only. A patch — an open sheet of faces on its way to
            // being extruded — has no inside to fill, and the walk below
            // would step off the end of it: finding the corner above one of
            // its own means crossing β₂, which is null at a boundary.
            if !self.is_block(volume) {
                continue;
            }
            // A face of this block, and the four darts round it. Every face
            // of a block is wound anticlockwise **seen from outside**, so
            // this cycle plays the part of `HEX_FACES[0]`, which is the
            // bottom face read [0, 3, 2, 1].
            let d0 = volume;
            let mut ring = [d0; 4];
            for k in 1..4 {
                ring[k] = map.beta::<1>(ring[k - 1]);
            }
            // The corner above each of them: across the edge into the side
            // face, then one step on, which lands on the upright.
            let up = ring.map(|d| map.beta::<1>(map.beta::<2>(d)));

            let at = |d| map.vertex_id(d);
            let end = |d| map.vertex_id(map.beta::<1>(d));
            let corners = [
                at(ring[0]),
                at(ring[3]),
                at(ring[2]),
                at(ring[1]),
                end(up[0]),
                end(up[3]),
                end(up[2]),
                end(up[1]),
            ];

            // The edges and faces by the corners they join, which needs no
            // orientation reasoning at all: both lists are short, and the
            // map says which cell has which corners.
            let darts = self.orbit(OrbitPolicy::Volume, volume);
            let mut edges = [0; 12];
            for (slot, [a, b]) in HEX_EDGES.iter().enumerate() {
                let (p, q) = (corners[*a], corners[*b]);
                edges[slot] = darts
                    .iter()
                    .map(|d| map.edge_id(*d))
                    .find(|e| {
                        let (x, y) = self.edge_endpoints(*e);
                        (x == p && y == q) || (x == q && y == p)
                    })
                    .expect("every edge of a hexahedron is one of its edges");
            }
            let mut faces = [0; 6];
            for (slot, four) in crate::blocking::HEX_FACES.iter().enumerate() {
                let want: Vec<VertexIdType> = {
                    let mut v: Vec<_> = four.iter().map(|c| corners[*c]).collect();
                    v.sort_unstable();
                    v
                };
                faces[slot] = darts
                    .iter()
                    .map(|d| map.face_id(*d))
                    .find(|f| {
                        let mut v: Vec<_> = self
                            .orbit(OrbitPolicy::Custom(&[1]), *f)
                            .iter()
                            .map(|d| map.vertex_id(*d))
                            .collect();
                        v.sort_unstable();
                        v == want
                    })
                    .expect("every face of a hexahedron is one of its faces");
            }

            out.push(Hex {
                block: self.block_tag(volume).id,
                corners,
                edges,
                faces,
            });
        }
        out
    }

    /// How many segments every edge is divided into, resolved sheet by
    /// sheet so that the answer is the same all the way along each one.
    ///
    /// One walk of the sheets rather than one per edge: the walk is the
    /// expensive part, and a sheet answers for every edge on it at once.
    pub fn segments_by_sheet(&self, default: u32) -> HashMap<CellId, u32> {
        let mut out: HashMap<CellId, u32> = HashMap::new();
        for e in self.map().iter_edges() {
            let name = self.edge_tag(e).id;
            if out.contains_key(&name) || name.is_none() {
                continue;
            }
            let Ok(sheet) = self.sheet_through(name) else {
                out.insert(name, default);
                continue;
            };
            let index = sheet.iter().map(|s| s.edge).collect::<Vec<_>>();
            let most = index
                .iter()
                .filter_map(|n| self.edge_by_id(*n))
                .map(|e| self.edge_tag(e).segments)
                .max()
                .unwrap_or(0);
            let n = if most == 0 { default } else { most };
            for name in index {
                out.insert(name, n);
            }
        }
        out
    }
}

/// Where each corner of a hexahedron sits on the block's own lattice, as
/// multiples of the three extents. The same eight corners [`cube_corners`]
/// lists, which is what makes the two orders one order.
///
/// [`cube_corners`]: crate::cube_corners
const CORNER_AT: [[u32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];

/// The points, and which cell of the blocking each of them belongs to.
#[derive(Default)]
struct Table {
    points: Vec<[f64; 3]>,
    sites: HashMap<Site, u32>,
}

impl Table {
    /// The index of this site's point, placing it if it is new.
    ///
    /// First come wins: two blocks that both work out where a face's
    /// interior point goes arrive at the same place by the same formula,
    /// and the second one's answer is thrown away rather than averaged in.
    fn put(&mut self, site: Site, at: [f64; 3]) -> u32 {
        *self.sites.entry(site).or_insert_with(|| {
            self.points.push(at);
            (self.points.len() - 1) as u32
        })
    }

    fn get(&self, site: Site) -> Option<u32> {
        self.sites.get(&site).copied()
    }
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// `out += by * at`, the only arithmetic the interpolation does.
fn add(out: &mut [f64; 3], at: [f64; 3], by: f64) {
    for k in 0..3 {
        out[k] += at[k] * by;
    }
}

impl Blocking {
    /// Discretise every block into a structured grid of hexahedra.
    ///
    /// `default` is how many segments an edge whose sheet nobody has set is
    /// divided into. `project` puts a point onto a geometric entity, and is
    /// a closure for the same reason [`Blocking::classify_nodes`] takes
    /// one: nothing in this crate knows what a triangle is.
    ///
    /// The three steps are the three dimensions, and each is read by the
    /// one above it **after** its projection, so a face is filled from
    /// edges that already lie on their curves and a block from faces that
    /// already lie on their surfaces.
    pub fn mesh(&self, default: u32, project: impl Fn(On, [f64; 3]) -> Option<[f64; 3]>) -> Mesh {
        let map = self.map();
        let segments = self.segments_by_sheet(default.max(1));
        let mut table = Table::default();
        let at = |v: VertexIdType| self.position(v).unwrap_or([0.0; 3]);
        let name = |v: VertexIdType| self.node_tag(v).id;
        let cuts = |e: EdgeIdType| *segments.get(&self.edge_tag(e).id).unwrap_or(&default);

        // --- the corners of the blocking ---------------------------------
        for v in map.iter_vertices() {
            table.put(Site::Node(name(v)), at(v));
        }

        // --- along each edge ---------------------------------------------
        for e in map.iter_edges() {
            let tag = self.edge_tag(e);
            let n = cuts(e);
            let (from, to) = self.edge_endpoints(e);
            let (p, q) = (at(from), at(to));
            for i in 1..n {
                let mut point = lerp(p, q, f64::from(i) / f64::from(n));
                if let Some(on) = tag.on {
                    if let Some(onto) = project(on, point) {
                        point = onto;
                    }
                }
                table.put(Site::Edge(tag.id, i), point);
            }
        }

        // --- across each face --------------------------------------------
        //
        // A Coons patch: the two pairs of opposite sides interpolated, less
        // the corners they both carry. Read off the edge points placed
        // above, so a side that follows a curve bends the patch with it.
        //
        // One lattice per **face**, in the face's own cycle, rather than
        // one per block: that is what makes two blocks share these points
        // instead of each placing its own on top of the other's.
        for f in map.iter_faces() {
            let quad = self.quad(f, &segments);
            let (na, nb) = quad.extents;
            let tag = self.face_tag(f);
            for a in 1..na {
                for b in 1..nb {
                    let (u, v) = (f64::from(a) / f64::from(na), f64::from(b) / f64::from(nb));
                    let side = |t: &Table, site: Site| {
                        t.points[t.get(site).expect("a side point") as usize]
                    };
                    let mut point = [0.0; 3];
                    add(&mut point, side(&table, quad.site(0, b)), 1.0 - u);
                    add(&mut point, side(&table, quad.site(na, b)), u);
                    add(&mut point, side(&table, quad.site(a, 0)), 1.0 - v);
                    add(&mut point, side(&table, quad.site(a, nb)), v);
                    add(
                        &mut point,
                        side(&table, quad.site(0, 0)),
                        -(1.0 - u) * (1.0 - v),
                    );
                    add(&mut point, side(&table, quad.site(na, 0)), -u * (1.0 - v));
                    add(&mut point, side(&table, quad.site(0, nb)), -(1.0 - u) * v);
                    add(&mut point, side(&table, quad.site(na, nb)), -u * v);
                    if let Some(on) = tag.on {
                        if let Some(onto) = project(on, point) {
                            point = onto;
                        }
                    }
                    table.put(Site::Face(tag.id, a, b), point);
                }
            }
        }

        // --- and through each block ---------------------------------------
        let mut hexes = Vec::new();
        let mut blocks = Vec::new();
        for hex in self.hexes() {
            // The three directions of the block, each the number its own
            // sheet carries: edge 0 runs along i, edge 3 along j, edge 8
            // up k.
            let n = [cuts(hex.edges[0]), cuts(hex.edges[3]), cuts(hex.edges[8])];
            let span = [n[0] + 1, n[1] + 1, n[2] + 1];
            let index = |i: u32, j: u32, k: u32| ((k * span[1] + j) * span[0] + i) as usize;
            let mut grid = vec![u32::MAX; (span[0] * span[1] * span[2]) as usize];

            // The six faces, each read on its own lattice and written onto
            // the block's. Nothing is placed here: this is where the
            // sharing happens, and all of it has been placed already.
            for (slot, corners) in crate::blocking::HEX_FACES.iter().enumerate() {
                let quad = self.quad(hex.faces[slot], &segments);
                let mine = corners.map(|c| name(hex.corners[c]));
                // Where the face's own four corners sit on the block's
                // lattice, from the corner table rather than from any
                // reasoning about which face this is.
                let whole = |c: usize| {
                    let t = CORNER_AT[c];
                    [t[0] * n[0], t[1] * n[1], t[2] * n[2]]
                };
                let (at0, at1, at3) = (whole(corners[0]), whole(corners[1]), whole(corners[3]));
                let (p1, p3) = (quad.span(mine[0], mine[1]), quad.span(mine[0], mine[3]));
                for p in 0..=p1 {
                    for q in 0..=p3 {
                        // Exact: a side of the face runs along one axis of
                        // the block, so one of the two deltas is zero and
                        // the other divides.
                        let ijk = [0, 1, 2].map(|k| {
                            let d1 = (at1[k] as i64 - at0[k] as i64) * i64::from(p) / i64::from(p1);
                            let d3 = (at3[k] as i64 - at0[k] as i64) * i64::from(q) / i64::from(p3);
                            (at0[k] as i64 + d1 + d3) as u32
                        });
                        let site = quad.of(mine[0], mine[1], mine[3], p, q);
                        grid[index(ijk[0], ijk[1], ijk[2])] =
                            table.get(site).expect("a point of the face");
                    }
                }
            }

            // The inside, from the six faces: the transfinite formula,
            // which is the faces less the edges they share in pairs plus
            // the corners those share in turn.
            for k in 1..n[2] {
                for j in 1..n[1] {
                    for i in 1..n[0] {
                        let w = [
                            f64::from(i) / f64::from(n[0]),
                            f64::from(j) / f64::from(n[1]),
                            f64::from(k) / f64::from(n[2]),
                        ];
                        let p =
                            |a: u32, b: u32, c: u32| table.points[grid[index(a, b, c)] as usize];
                        let ends = |t: usize, at: u32| [(0, 1.0 - w[t]), (at, w[t])];
                        let mut point = [0.0; 3];
                        for (a, fa) in ends(0, n[0]) {
                            add(&mut point, p(a, j, k), fa);
                        }
                        for (b, fb) in ends(1, n[1]) {
                            add(&mut point, p(i, b, k), fb);
                        }
                        for (c, fc) in ends(2, n[2]) {
                            add(&mut point, p(i, j, c), fc);
                        }
                        for (a, fa) in ends(0, n[0]) {
                            for (b, fb) in ends(1, n[1]) {
                                add(&mut point, p(a, b, k), -fa * fb);
                            }
                            for (c, fc) in ends(2, n[2]) {
                                add(&mut point, p(a, j, c), -fa * fc);
                            }
                        }
                        for (b, fb) in ends(1, n[1]) {
                            for (c, fc) in ends(2, n[2]) {
                                add(&mut point, p(i, b, c), -fb * fc);
                            }
                        }
                        for (a, fa) in ends(0, n[0]) {
                            for (b, fb) in ends(1, n[1]) {
                                for (c, fc) in ends(2, n[2]) {
                                    add(&mut point, p(a, b, c), fa * fb * fc);
                                }
                            }
                        }
                        grid[index(i, j, k)] = table.put(Site::Inside(hex.block, i, j, k), point);
                    }
                }
            }

            for k in 0..n[2] {
                for j in 0..n[1] {
                    for i in 0..n[0] {
                        hexes.push([
                            grid[index(i, j, k)],
                            grid[index(i + 1, j, k)],
                            grid[index(i + 1, j + 1, k)],
                            grid[index(i, j + 1, k)],
                            grid[index(i, j, k + 1)],
                            grid[index(i + 1, j, k + 1)],
                            grid[index(i + 1, j + 1, k + 1)],
                            grid[index(i, j + 1, k + 1)],
                        ]);
                        blocks.push(hex.block);
                    }
                }
            }
        }

        Mesh {
            points: table.points,
            hexes,
            blocks,
        }
    }

    /// One face of the blocking, as a lattice.
    fn quad(&self, f: FaceIdType, segments: &HashMap<CellId, u32>) -> Quad {
        let map = self.map();
        let mut darts = [f; 4];
        for k in 1..4 {
            darts[k] = map.beta::<1>(darts[k - 1]);
        }
        let corners = darts.map(|d| self.node_tag(map.vertex_id(d)).id);
        let edges = darts.map(|d| {
            let e = map.edge_id(d);
            let tag = self.edge_tag(e);
            let n = *segments.get(&tag.id).unwrap_or(&1);
            // Which end the chain along it was counted from, by name.
            let (from, _) = self.edge_endpoints(e);
            (tag.id, self.node_tag(from).id, n)
        });
        debug_assert_eq!(edges[0].2, edges[2].2, "a face's opposite sides disagree");
        debug_assert_eq!(edges[1].2, edges[3].2, "a face's opposite sides disagree");
        Quad {
            face: self.face_tag(f).id,
            corners,
            edges,
            extents: (edges[0].2, edges[1].2),
        }
    }
}

/// One face of the blocking, as a lattice of its own.
///
/// `a` runs along the face's first side and `b` along its last, both read
/// from the face's own β₁ cycle — which is a property of the face rather
/// than of either block that has it, and so the same for both of them.
struct Quad {
    face: CellId,
    /// The four corners, in that cycle.
    corners: [CellId; 4],
    /// `edges[p]` joins corner `p` to corner `p + 1`: its name, the end its
    /// chain was counted from, and how many segments it has.
    edges: [(CellId, CellId, u32); 4],
    extents: (u32, u32),
}

impl Quad {
    /// How many steps there are between two adjacent corners.
    fn span(&self, from: CellId, to: CellId) -> u32 {
        let (a, b) = (self.find(from), self.find(to));
        // Adjacent either way round: the side between them is the one at
        // the lower of the two indices, give or take the wrap.
        if (a + 1) % 4 == b {
            self.edges[a].2
        } else {
            self.edges[b].2
        }
    }

    fn find(&self, corner: CellId) -> usize {
        self.corners
            .iter()
            .position(|c| *c == corner)
            .expect("a corner of this face")
    }

    /// The site of the lattice point `(a, b)`.
    fn site(&self, a: u32, b: u32) -> Site {
        let (na, nb) = self.extents;
        match (a, b) {
            (0, 0) => Site::Node(self.corners[0]),
            (_, 0) if a == na => Site::Node(self.corners[1]),
            _ if a == na && b == nb => Site::Node(self.corners[2]),
            (0, _) if b == nb => Site::Node(self.corners[3]),
            (_, 0) => self.along(0, a),
            _ if a == na => self.along(1, b),
            _ if b == nb => self.along(2, na - a),
            (0, _) => self.along(3, nb - b),
            _ => Site::Face(self.face, a, b),
        }
    }

    /// A point strictly inside a side, `step` along it from the corner the
    /// side starts at.
    fn along(&self, slot: usize, step: u32) -> Site {
        let (edge, from, n) = self.edges[slot];
        // The side runs corner `slot` → corner `slot + 1`; the chain along
        // it was counted from whichever end the map calls its first.
        let i = if self.corners[slot] == from {
            step
        } else {
            n - step
        };
        Site::Edge(edge, i)
    }

    /// The site of a point given in a **block's** frame: `p` steps from
    /// `c0` towards `c1`, and `q` steps towards `c3`.
    ///
    /// The two frames differ by a turn and possibly a reflection — the two
    /// blocks either side of a face see it wound opposite ways — and this
    /// is where that is undone, by the one thing both agree about: which
    /// corner is which.
    fn of(&self, c0: CellId, c1: CellId, c3: CellId, p: u32, q: u32) -> Site {
        let (na, nb) = self.extents;
        let corner = |k: usize| -> (i64, i64) {
            [
                (0, 0),
                (i64::from(na), 0),
                (i64::from(na), i64::from(nb)),
                (0, i64::from(nb)),
            ][k]
        };
        let (k0, k1, k3) = (
            corner(self.find(c0)),
            corner(self.find(c1)),
            corner(self.find(c3)),
        );
        let (p1, p3) = (self.span(c0, c1), self.span(c0, c3));
        // One of the two deltas is zero and the other is the whole side,
        // so both divisions are exact.
        let along =
            |from: i64, to: i64, step: u32, of: u32| (to - from) * i64::from(step) / i64::from(of);
        let a = k0.0 + along(k0.0, k1.0, p, p1) + along(k0.0, k3.0, q, p3);
        let b = k0.1 + along(k0.1, k1.1, p, p1) + along(k0.1, k3.1, q, p3);
        self.site(a as u32, b as u32)
    }
}
