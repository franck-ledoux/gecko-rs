//! Entities made of other entities.
//!
//! A surface of a boundary representation usually has more than four
//! corners, so a quadrangle laid on it has **sides** that run along several
//! curves. A cell is classified on one entity — `(dim, tag)` and nothing
//! else — so a side has to *be* one entity: a curve made of curves.
//!
//! The same thing will be wanted a dimension up, for a face of a blocking
//! that spans several geometric surfaces. So this is not "composite
//! curves": a composite is a **named group of entities of one dimension**,
//! and which dimension is just a number it carries. Projection, distance
//! and containment then work on it exactly as they work on anything else.
//!
//! # What they are not
//!
//! They are **not a partition**. Two surfaces that touch share boundary
//! curves, so a curve belongs to as many sides as there are surfaces using
//! it, and sides may overlap, nest, or repeat. Everything here is written
//! in terms of set membership rather than exclusivity for that reason.
//!
//! They are **not part of the geometry**, either. A `Geometry` is read from
//! a file and then shared, immutably, by every state the undo stack holds —
//! so a composite created half way through a session cannot live in it
//! without either copying the triangles or making the states disagree.
//! The table lives beside it and is consulted first.
//!
//! And they are **never guessed at**: a composite is only ever where
//! something deliberately put it. Proximity does not return one, and
//! inference from a cell's boundary does not reach for one, because a name
//! that appeared under a reader who never asked for it is worse than no
//! name.

use std::collections::BTreeSet;

use crate::model::Geometry;
use crate::where_on::CORNER;

/// The table of composites, beside a geometry.
///
/// Monotone: entries are added and never removed. An undo that takes back
/// the operation which made one leaves a name nobody uses, which costs a
/// few bytes and cannot dangle — where removing it would leave a cell
/// classified on nothing at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Composites {
    /// Entry `k` answers to tag `-(k + 1)`. Members are kept sorted, which
    /// is what makes the same group the same entry however it was asked
    /// for — a side walked the other way round is the same side.
    groups: Vec<(u8, Vec<i32>)>,
}

impl Composites {
    pub fn new() -> Self {
        Self::default()
    }

    /// A tag is a composite's when it is negative. Gmsh's own tags are
    /// positive, so the two can never be confused.
    pub fn is_composite(tag: i32) -> bool {
        tag < 0
    }

    /// Name a group, or give back the name it already has.
    ///
    /// Asking twice for the same members gives the same tag, which is what
    /// makes an operation that creates one **repeatable**: a script replayed
    /// from the journal must not leave a new name behind every time it runs.
    ///
    /// A group of one is not a composite: its own tag is already a name for
    /// it, and wrapping it would hide what it is.
    pub fn group(&mut self, dim: u8, members: &[i32]) -> i32 {
        let mut sorted: Vec<i32> = members.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() == 1 {
            return sorted[0];
        }
        if let Some(k) = self
            .groups
            .iter()
            .position(|(d, m)| *d == dim && *m == sorted)
        {
            return -(k as i32 + 1);
        }
        self.groups.push((dim, sorted));
        -(self.groups.len() as i32)
    }

    /// What a composite is made of, or nothing if that is not one.
    pub fn members(&self, tag: i32) -> Option<(u8, &[i32])> {
        if tag >= 0 {
            return None;
        }
        self.groups
            .get((-tag - 1) as usize)
            .map(|(dim, members)| (*dim, members.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// A geometry, and the composites named over it.
///
/// Everything a block structure asks of the geometry goes through this, so
/// that a composite answers like any other entity and nothing above has to
/// know the difference.
#[derive(Clone, Copy, Debug)]
pub struct Model<'a> {
    pub geometry: &'a Geometry,
    pub composites: &'a Composites,
}

impl<'a> Model<'a> {
    pub fn new(geometry: &'a Geometry, composites: &'a Composites) -> Self {
        Self {
            geometry,
            composites,
        }
    }

    /// How big the model is, corner to corner — a tolerance is a share of
    /// something, and this is the something.
    pub fn span(&self) -> f64 {
        self.geometry.span()
    }

    /// The point of an entity nearest `p`. For a composite, the nearest
    /// point of whichever member is nearest.
    pub fn project(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<[f64; 3]> {
        match self.composites.members(tag) {
            None => self.geometry.project(dim, tag, p),
            Some((dim, members)) => members
                .iter()
                .filter_map(|&m| {
                    let at = self.geometry.project(dim, m, p)?;
                    Some((apart(at, p), at))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, at)| at),
        }
    }

    /// How far `p` is from an entity.
    pub fn distance(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<f64> {
        match self.composites.members(tag) {
            None => self.geometry.distance(dim, tag, p),
            Some((dim, members)) => members
                .iter()
                .filter_map(|&m| self.geometry.distance(dim, m, p))
                .min_by(f64::total_cmp),
        }
    }

    /// The nearest entity to a point, within a tolerance per dimension.
    ///
    /// Composites are **not** candidates. One is a name somebody chose for a
    /// run of curves; coming across it by being near it would put a cell on
    /// a side nobody mentioned, and the members are right there to be found
    /// instead.
    pub fn nearest(&self, p: [f64; 3], tolerance: [f64; 3]) -> Option<(u8, i32)> {
        self.geometry.nearest(p, tolerance)
    }

    /// Everything containing an entity, itself included.
    ///
    /// A composite contains its members, so a member's up-set gains it; and
    /// a composite is contained in whatever contains **all** of its members,
    /// which for a side is the surface it runs along.
    pub fn containing(&self, dim: u8, tag: i32) -> Vec<(u8, i32)> {
        if let Some((dim, members)) = self.composites.members(tag) {
            let mut up: BTreeSet<(u8, i32)> = match members.first() {
                Some(&first) => self
                    .geometry
                    .containing(dim, first)
                    .iter()
                    .copied()
                    .collect(),
                None => BTreeSet::new(),
            };
            for &m in members.iter().skip(1) {
                let theirs: BTreeSet<(u8, i32)> =
                    self.geometry.containing(dim, m).iter().copied().collect();
                up.retain(|e| theirs.contains(e));
            }
            // The members themselves are *in* it, not above it.
            up.retain(|e| !(e.0 == dim && members.contains(&e.1)));
            let mut out = vec![(dim, tag)];
            out.extend(up);
            return out;
        }

        let mut up: Vec<(u8, i32)> = self.geometry.containing(dim, tag).to_vec();
        if up.is_empty() {
            return up;
        }
        for k in 0..self.composites.len() {
            let name = -(k as i32 + 1);
            let (group, members) = self.composites.members(name).expect("a composite");
            if members.iter().any(|&m| up.contains(&(group, m))) {
                up.push((group, name));
            }
        }
        up
    }

    /// Everything containing **every** one of a set of classifications,
    /// composites included — what a cell bounded by them is *allowed* to
    /// be on.
    ///
    /// The companion of [`common_of`](Self::common_of), and deliberately
    /// not the same: that one chooses, and this one permits. A cell keeps a
    /// side it was deliberately put on when its boundary still allows it,
    /// and no cell is ever *given* a side by inference — the diagonal of a
    /// face has the same two ends as the side beside it.
    pub fn shared(&self, boundary: &[(u8, i32)]) -> Vec<(u8, i32)> {
        let Some((first, rest)) = boundary.split_first() else {
            return Vec::new();
        };
        let mut common: BTreeSet<(u8, i32)> =
            self.containing(first.0, first.1).into_iter().collect();
        for other in rest {
            let theirs: BTreeSet<(u8, i32)> =
                self.containing(other.0, other.1).into_iter().collect();
            common.retain(|e| theirs.contains(e));
            if common.is_empty() {
                break;
            }
        }
        common.into_iter().collect()
    }

    /// The lowest-dimensional entity containing **every** one of a set of
    /// classifications — what a cell bounded by them lies on.
    ///
    /// Composites are left out of this one. Inference asks what a cell's
    /// boundary implies, and a side implies nothing about a cell that is
    /// merely bounded by its ends: the diagonal of a face has the same two
    /// ends as the side beside it. A cell is put on a side deliberately, or
    /// it inherits one by being cut from a cell that was.
    pub fn common_of(&self, boundary: &[(u8, i32)], at_least: u8) -> Option<(u8, i32)> {
        let (first, rest) = boundary.split_first()?;
        let mut common: BTreeSet<(u8, i32)> = self
            .geometry
            .containing(first.0, first.1)
            .iter()
            .copied()
            .collect();
        for other in rest {
            let theirs: BTreeSet<(u8, i32)> = self
                .geometry
                .containing(other.0, other.1)
                .iter()
                .copied()
                .collect();
            common.retain(|e| theirs.contains(e));
        }
        common
            .into_iter()
            .find(|e| e.0 >= at_least && !Composites::is_composite(e.1))
    }

    /// [`common_of`](Self::common_of) for the two ends of an edge.
    pub fn common(&self, a: (u8, i32), b: (u8, i32), at_least: u8) -> Option<(u8, i32)> {
        self.common_of(&[a, b], at_least)
    }

    /// The most constrained entity that is on **both** — what welding two
    /// classified nodes into one has to leave them on.
    ///
    /// Composites take part here, and must: a node on a side welded to a
    /// node on one of that side's curves belongs on the curve, which is the
    /// stronger claim, and only an up-set that knows about sides can say so.
    pub fn meet(&self, a: (u8, i32), b: (u8, i32), near: [f64; 3]) -> Option<(u8, i32)> {
        if self.containing(a.0, a.1).contains(&b) {
            return Some(a);
        }
        if self.containing(b.0, b.1).contains(&a) {
            return Some(b);
        }
        let mut shared: Vec<(u8, i32)> = Vec::new();
        for e in self.entities() {
            let up = self.containing(e.0, e.1);
            if up.contains(&a) && up.contains(&b) {
                shared.push(e);
            }
        }
        shared
            .into_iter()
            .filter_map(|e| self.distance(e.0, e.1, near).map(|d| (d, e)))
            .min_by(|(d, e), (f, g)| d.total_cmp(f).then(e.0.cmp(&g.0)))
            .map(|(_, e)| e)
    }

    /// Every entity there is, composites included.
    fn entities(&self) -> Vec<(u8, i32)> {
        let mut out: Vec<(u8, i32)> = self
            .geometry
            .corners()
            .iter()
            .map(|c| (CORNER, c.tag))
            .collect();
        out.extend(self.geometry.curves().iter().map(|c| (crate::CURVE, c.tag)));
        out.extend(
            self.geometry
                .surfaces()
                .iter()
                .map(|s| (crate::SURFACE, s.tag)),
        );
        for k in 0..self.composites.len() {
            let name = -(k as i32 + 1);
            let (dim, _) = self.composites.members(name).expect("a composite");
            out.push((dim, name));
        }
        out
    }
}

fn apart(p: [f64; 3], q: [f64; 3]) -> f64 {
    (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>().sqrt()
}
