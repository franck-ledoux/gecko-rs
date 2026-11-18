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
    /// How many segments every edge is divided into, resolved sheet by
    /// sheet so that the answer is the same all the way along each one.
    ///
    /// One walk of the sheets rather than one per edge: the walk is the
    /// expensive part, and a sheet answers for every edge on it at once.
    pub fn segments_by_sheet(&self, default: u32) -> HashMap<CellId, u32> {
        let mut out: HashMap<CellId, u32> = HashMap::new();
        let asked: HashMap<CellId, u32> = self
            .edges_in_full()
            .map(|(edge, _, segments, _)| (edge, segments))
            .collect();
        for name in self.edge_names() {
            if out.contains_key(&name) {
                continue;
            }
            let Ok(sheet) = self.sheet_through(name) else {
                out.insert(name, default);
                continue;
            };
            let index = sheet.iter().map(|s| s.edge).collect::<Vec<_>>();
            let most = index
                .iter()
                .filter_map(|n| asked.get(n).copied())
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

/// Where each cell of the blocking's run of mesh nodes begins.
///
/// This used to be a `HashMap<Site, u32>`: one entry per **mesh** node, which
/// made it the one table in the mesher that grows with the mesh rather than
/// with the blocking. At a hundred million cells that is gigabytes of hash
/// table, and it is the reason a mesh that size cannot be built in a tab at
/// all.
///
/// A node's number is arithmetic instead. Every cell owns a contiguous run —
/// one number for a corner, `n - 1` along an edge, `(na-1)(nb-1)` across a
/// face, `(ni-1)(nj-1)(nk-1)` through a block — and the runs are laid end to
/// end, in the order the walks of [`Blocking::mesh`] take them, so the
/// numbering is reproducible.
///
/// Two blocks sharing a face reach the same number for a shared point without
/// consulting anything: they reach the same **cell**, and they count inside it
/// the same way, because a face's lattice is the face's own and not either
/// block's. That is the same argument that made the sharing work before, with
/// the lookup taken out of it.
///
/// These tables are sized by the **blocking** — a hundred thousand entries to
/// number a billion nodes exactly.
#[derive(Default)]
struct Numbering {
    corners: HashMap<CellId, u32>,
    along: HashMap<CellId, u32>,
    /// Where a face's run starts, and its **last** extent, which is the
    /// stride: the face pass walks `a` outermost, so a point's number runs
    /// fastest in `b`.
    across: HashMap<CellId, (u32, u32)>,
    /// The same through a block, which needs two strides.
    through: HashMap<CellId, (u32, u32, u32)>,
    total: usize,
}

impl Numbering {
    /// The number of this site's point.
    ///
    /// Total rather than partial: every site of every cell has a number
    /// before a single point is placed, which is what lets the three passes
    /// below write into one array instead of appending to it.
    fn at(&self, site: Site) -> u32 {
        match site {
            Site::Node(n) => self.corners[&n],
            Site::Edge(e, i) => self.along[&e] + i - 1,
            Site::Face(f, a, b) => {
                let (at, nb) = self.across[&f];
                at + (a - 1) * (nb - 1) + (b - 1)
            }
            Site::Inside(c, i, j, k) => {
                let (at, ni, nj) = self.through[&c];
                at + ((k - 1) * (nj - 1) + (j - 1)) * (ni - 1) + (i - 1)
            }
        }
    }
}

/// The points, in the order the numbering gives them.
struct Table {
    points: Vec<[f64; 3]>,
    number: Numbering,
}

impl Table {
    fn new(number: Numbering) -> Self {
        // Not-a-number rather than nought, so that reading a point nothing
        // has placed is a fault that shows itself rather than a point at the
        // origin that quietly drags a cell through it.
        let points = vec![[f64::NAN; 3]; number.total];
        Table { points, number }
    }

    /// Place this site's point, and say where it went.
    fn put(&mut self, site: Site, at: [f64; 3]) -> u32 {
        let i = self.number.at(site);
        // What "first come wins" used to guard: each site is placed by
        // exactly one of the three passes, exactly once. The old table threw
        // a second answer away in silence; this says so.
        debug_assert!(
            self.points[i as usize][0].is_nan(),
            "two points placed for one site"
        );
        self.points[i as usize] = at;
        i
    }

    fn get(&self, site: Site) -> u32 {
        self.number.at(site)
    }

    /// The point itself, which must have been placed by an earlier pass.
    fn point(&self, site: Site) -> [f64; 3] {
        let at = self.points[self.number.at(site) as usize];
        debug_assert!(!at[0].is_nan(), "a point no pass has placed");
        at
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
        let segments = self.segments_by_sheet(default.max(1));
        let cuts = |edge: CellId| *segments.get(&edge).unwrap_or(&default);
        // Every node's place, in one walk. Asked for one at a time it would
        // be a search of the map per node. Kept as a list as well as a
        // lookup, because the numbering below has to be laid out in an order
        // that does not depend on a hash: two hosts meshing the same blocking
        // must number it the same way.
        let nodes: Vec<(CellId, [f64; 3])> = self.nodes().collect();
        let at: HashMap<CellId, [f64; 3]> = nodes.iter().copied().collect();
        let along: Vec<(CellId, [CellId; 2], u32, Option<On>)> = self.edges_in_full().collect();

        // Each block and each face's lattice, worked out once. Both the face
        // pass and the block pass need them, and both used to ask for them
        // again: six fresh walks of the map per block for lattices that do
        // not change while a mesh is being built.
        let frames = self.hexes();
        let faces = self.face_names();
        let quads: HashMap<CellId, Quad> = faces
            .iter()
            .map(|f| (*f, self.quad(*f, &segments)))
            .collect();

        // Then every cell's run of node numbers, laid end to end. Walked in
        // the map's own order rather than a hash map's, so that two hosts
        // meshing the same blocking number it the same way.
        let mut number = Numbering::default();
        let mut next: u32 = 0;
        for (node, _) in &nodes {
            number.corners.insert(*node, next);
            next += 1;
        }
        for (edge, ..) in &along {
            number.along.insert(*edge, next);
            next += cuts(*edge) - 1;
        }
        for f in &faces {
            let (na, nb) = quads[f].extents;
            number.across.insert(*f, (next, nb));
            next += (na - 1) * (nb - 1);
        }
        for frame in &frames {
            let n = [
                cuts(frame.edges[0]),
                cuts(frame.edges[3]),
                cuts(frame.edges[8]),
            ];
            number.through.insert(frame.block, (next, n[0], n[1]));
            next += (n[0] - 1) * (n[1] - 1) * (n[2] - 1);
        }
        number.total = next as usize;
        let mut table = Table::new(number);

        // --- the corners of the blocking ---------------------------------
        for (node, place) in &nodes {
            table.put(Site::Node(*node), *place);
        }

        // --- along each edge ---------------------------------------------
        for (edge, [from, to], _, on) in &along {
            let n = cuts(*edge);
            let (p, q) = (at[from], at[to]);
            for i in 1..n {
                let mut point = lerp(p, q, f64::from(i) / f64::from(n));
                if let Some(on) = on {
                    if let Some(onto) = project(*on, point) {
                        point = onto;
                    }
                }
                table.put(Site::Edge(*edge, i), point);
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
        for f in &faces {
            let on = self.face_class(*f);
            let quad = &quads[f];
            let (na, nb) = quad.extents;
            for a in 1..na {
                for b in 1..nb {
                    let (u, v) = (f64::from(a) / f64::from(na), f64::from(b) / f64::from(nb));
                    let side = |site: Site| table.point(site);
                    let mut point = [0.0; 3];
                    add(&mut point, side(quad.site(0, b)), 1.0 - u);
                    add(&mut point, side(quad.site(na, b)), u);
                    add(&mut point, side(quad.site(a, 0)), 1.0 - v);
                    add(&mut point, side(quad.site(a, nb)), v);
                    add(&mut point, side(quad.site(0, 0)), -(1.0 - u) * (1.0 - v));
                    add(&mut point, side(quad.site(na, 0)), -u * (1.0 - v));
                    add(&mut point, side(quad.site(0, nb)), -(1.0 - u) * v);
                    add(&mut point, side(quad.site(na, nb)), -u * v);
                    if let Some(on) = on {
                        if let Some(onto) = project(on, point) {
                            point = onto;
                        }
                    }
                    table.put(Site::Face(*f, a, b), point);
                }
            }
        }

        // --- and through each block ---------------------------------------
        let mut hexes = Vec::new();
        let mut blocks = Vec::new();
        for hex in &frames {
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
            for (slot, corners) in crate::cells::HEX_FACES.iter().enumerate() {
                let quad = &quads[&hex.faces[slot]];
                let mine = corners.map(|c| hex.corners[c]);
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
                        grid[index(ijk[0], ijk[1], ijk[2])] = table.get(site);
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
    fn quad(&self, face: CellId, segments: &HashMap<CellId, u32>) -> Quad {
        let (corners, sides) = self.face_cells(face).expect("a face of the blocking");
        let edges = sides.map(|edge| {
            let n = *segments.get(&edge).unwrap_or(&1);
            // Which end the chain along it was counted from, by name.
            let (from, _) = self.edge_ends(edge).expect("a side of a face");
            (edge, from, n)
        });
        debug_assert_eq!(edges[0].2, edges[2].2, "a face's opposite sides disagree");
        debug_assert_eq!(edges[1].2, edges[3].2, "a face's opposite sides disagree");
        Quad {
            face,
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
