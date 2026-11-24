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

use std::collections::{BTreeMap, BTreeSet};

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
    /// The composites that are **merges**: said once of the geometry rather
    /// than of a cell, standing in for their members everywhere.
    merged: BTreeSet<i32>,
    /// What stands in for an entity a merge has taken: its members, and the
    /// joins between them it swallowed — the seam inside two surfaces
    /// merged, the corner where two curves merged meet. Worked out again at
    /// every merge, from the geometry and the merges alone.
    stand_in: BTreeMap<(u8, i32), (u8, i32)>,
}

/// Why a merge was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MergeError {
    #[error("a merge takes two entities or more")]
    TooFew,
    #[error("only curves and surfaces are merged")]
    NotMergeable,
    #[error("there is no {0} on the model")]
    NotThere(String),
    #[error("{0} is merged already")]
    Taken(String),
}

fn named(dim: u8, tag: i32) -> String {
    format!(
        "{} {tag}",
        ["corner", "curve", "surface"][dim.min(2) as usize]
    )
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

    /// Merge entities of the geometry into one, which then stands in for
    /// them: proximity finds it rather than them, and the joins between
    /// them stop being entities a node can be found on.
    ///
    /// Of the joins, an entity is swallowed when at least two of the
    /// entities one dimension up through it are taken by the same merge and
    /// that merge is the lowest-dimensional thing through it: curve 5
    /// between merged surfaces 2 and 3, a corner between merged curves 2
    /// and 4 — even with curve 5 through it as well, since curve 5 went into
    /// the surface. A corner where a merged curve meets a curve of its own
    /// is a junction still, and stays.
    pub fn merge(
        &mut self,
        geometry: &Geometry,
        dim: u8,
        members: &[i32],
    ) -> Result<i32, MergeError> {
        if dim == CORNER || dim > crate::SURFACE {
            return Err(MergeError::NotMergeable);
        }
        let mut sorted: Vec<i32> = members.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() < 2 {
            return Err(MergeError::TooFew);
        }
        for &m in &sorted {
            if m <= 0 || geometry.containing(dim, m).is_empty() {
                return Err(MergeError::NotThere(named(dim, m)));
            }
            if self.stand_in.contains_key(&(dim, m)) {
                return Err(MergeError::Taken(named(dim, m)));
            }
        }
        let tag = self.group(dim, &sorted);
        self.merged.insert(tag);
        self.settle_merges(geometry);
        Ok(tag)
    }

    fn settle_merges(&mut self, geometry: &Geometry) {
        self.stand_in.clear();
        let taken: Vec<((u8, i32), (u8, i32))> = self
            .merged
            .iter()
            .flat_map(|&tag| {
                let (dim, members) = self.members(tag).expect("a merge is a composite");
                members.iter().map(move |&m| ((dim, m), (dim, tag)))
            })
            .collect();
        self.stand_in.extend(taken);
        // From the top down, so that a corner sees what the curves through
        // it have already become.
        for dim in [crate::CURVE, CORNER] {
            let tags: Vec<i32> = match dim {
                CORNER => geometry.corners().iter().map(|c| c.tag).collect(),
                _ => geometry.curves().iter().map(|c| c.tag).collect(),
            };
            for tag in tags {
                if self.stand_in.contains_key(&(dim, tag)) {
                    continue;
                }
                let images: Vec<(u8, i32)> = geometry
                    .containing(dim, tag)
                    .iter()
                    .filter(|e| e.0 == dim + 1)
                    .map(|e| self.stand_in.get(e).copied().unwrap_or(*e))
                    .collect();
                let Some(lowest) = images.iter().map(|e| e.0).min() else {
                    continue;
                };
                let low: BTreeSet<(u8, i32)> =
                    images.iter().copied().filter(|e| e.0 == lowest).collect();
                if low.len() != 1 {
                    continue;
                }
                let only = *low.iter().next().expect("one");
                let through = images.iter().filter(|e| **e == only).count();
                if self.merged.contains(&only.1) && through >= 2 {
                    self.stand_in.insert((dim, tag), only);
                }
            }
        }
    }

    /// Whether a composite is a merge of the geometry.
    pub fn is_merged(&self, tag: i32) -> bool {
        self.merged.contains(&tag)
    }

    /// What stands in for an entity a merge took, if one did.
    pub fn stand_in(&self, dim: u8, tag: i32) -> Option<(u8, i32)> {
        self.stand_in.get(&(dim, tag)).copied()
    }

    /// An entity as the model now has it: what stands in for it, or itself.
    pub fn image(&self, dim: u8, tag: i32) -> (u8, i32) {
        self.stand_in(dim, tag).unwrap_or((dim, tag))
    }

    /// Every merge, by tag.
    pub fn merges(&self) -> impl Iterator<Item = i32> + '_ {
        self.merged.iter().copied()
    }

    /// The entities each merge swallowed besides its members.
    pub fn swallowed(&self, tag: i32) -> Vec<(u8, i32)> {
        let members = self.members(tag).map(|(_, m)| m).unwrap_or(&[]);
        self.stand_in
            .iter()
            .filter(|(e, by)| by.1 == tag && !(by.0 == e.0 && members.contains(&e.1)))
            .map(|(e, _)| *e)
            .collect()
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
    ///
    /// A **merge** is the exception, being a thing said of the geometry:
    /// it is found in place of its members, and what it swallowed is not
    /// found at all — a point near the corner between two merged curves is
    /// on the merged curve.
    pub fn nearest(&self, p: [f64; 3], tolerance: [f64; 3]) -> Option<(u8, i32)> {
        if self.composites.merged.is_empty() {
            return self.geometry.nearest(p, tolerance);
        }
        for dim in [CORNER, crate::CURVE, crate::SURFACE] {
            let best = self
                .visible(dim)
                .into_iter()
                .filter_map(|tag| self.distance(dim, tag, p).map(|d| (d, tag)))
                .filter(|(d, _)| *d <= tolerance[dim as usize])
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            if let Some((_, tag)) = best {
                return Some((dim, tag));
            }
        }
        None
    }

    /// The entities of a dimension as the model now has them: the
    /// geometry's own that no merge took, and the merges.
    pub fn visible(&self, dim: u8) -> Vec<i32> {
        let mut tags: Vec<i32> = match dim {
            CORNER => self.geometry.corners().iter().map(|c| c.tag).collect(),
            crate::CURVE => self.geometry.curves().iter().map(|c| c.tag).collect(),
            _ => self.geometry.surfaces().iter().map(|s| s.tag).collect(),
        };
        tags.retain(|t| self.composites.stand_in(dim, *t).is_none());
        tags.extend(
            self.composites
                .merges()
                .filter(|m| self.composites.members(*m).is_some_and(|(d, _)| d == dim)),
        );
        tags
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
            // And it is inside another composite when each of its members
            // is inside one of that one's: a run of curves 2 and 4 is in
            // the patch of surfaces 2 and 3 when curve 2 bounds one of them
            // and curve 4 the other, though no single surface holds both.
            // Without this an edge put on such a run could never bound a
            // face put on such a patch.
            for k in 0..self.composites.len() {
                let name = -(k as i32 + 1);
                if name == tag {
                    continue;
                }
                let (group, theirs) = self.composites.members(name).expect("a composite");
                let under = members.iter().all(|&m| {
                    theirs
                        .iter()
                        .any(|&t| self.geometry.containing(dim, m).contains(&(group, t)))
                });
                if under {
                    out.push((group, name));
                }
            }
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

    /// Where a node held by all of these may be: the entity contained in
    /// **every** one of them that is nearest to `near`, lower dimension
    /// first on a tie.
    ///
    /// What a node pulled in by a pillowing may slide on when its
    /// neighbours are on these: on S3 and S6 that is the curve where they
    /// meet, on S3 alone it is S3 — or a curve or corner of it, when that is
    /// where the nearest point falls. A composite takes part only when it
    /// is one of those given: being on a side is a thing a cell is put on,
    /// never one it is found on.
    ///
    /// `None` when nothing is contained in all of them, or nothing is
    /// given.
    pub fn destination(&self, on: &[(u8, i32)], near: [f64; 3]) -> Option<(u8, i32)> {
        if on.is_empty() {
            return None;
        }
        let found: Vec<(f64, (u8, i32))> = self
            .entities()
            .into_iter()
            .filter(|e| {
                !Composites::is_composite(e.1) || self.composites.is_merged(e.1) || on.contains(e)
            })
            .filter(|e| {
                let up = self.containing(e.0, e.1);
                on.iter().all(|o| up.contains(o))
            })
            .filter_map(|e| self.distance(e.0, e.1, near).map(|d| (d, e)))
            .collect();
        // A point projected onto a surface lands on its rim when the rim
        // is nearest, and so is as far as the rim is give or take the last
        // digit: "as near" has to allow for that, or the surface would win
        // ties it should lose.
        let nearest = found.iter().map(|(d, _)| *d).fold(f64::INFINITY, f64::min);
        let slack = 1e-9 * self.span().max(1.0);
        found
            .into_iter()
            .filter(|(d, _)| *d <= nearest + slack)
            .map(|(_, e)| e)
            .min()
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
        // What a merge took is not anywhere a node can be any more.
        out.retain(|e| self.composites.stand_in(e.0, e.1).is_none());
        out
    }
}

fn apart(p: [f64; 3], q: [f64; 3]) -> f64 {
    (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>().sqrt()
}
