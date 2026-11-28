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
    /// Curves swallowed by the one surface on both sides of them: the seam
    /// a CAD export leaves down a cylinder, which is not a join between two
    /// pieces but a cut of one, `(curve, surface)`.
    seams: BTreeSet<(i32, i32)>,
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
        // A seam goes into its surface, or into what that surface went into.
        let seams: Vec<((u8, i32), (u8, i32))> = self
            .seams
            .iter()
            .map(|&(c, s)| ((crate::CURVE, c), self.image(crate::SURFACE, s)))
            .collect();
        self.stand_in.extend(seams);
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
                // One image per way in: a closed curve passes through its
                // own corner twice, once at each end, and a corner on one
                // with nothing else through it is no corner at all.
                let up: Vec<(u8, i32)> = geometry
                    .containing(dim, tag)
                    .iter()
                    .copied()
                    .filter(|e| e.0 == dim + 1)
                    .collect();
                let mut images: Vec<(u8, i32)> = Vec::new();
                for e in &up {
                    let image = self.stand_in.get(e).copied().unwrap_or(*e);
                    let ways = match dim {
                        CORNER => ends_at(geometry, tag, e.1).max(1),
                        _ => 1,
                    };
                    images.extend(std::iter::repeat_n(image, ways));
                }
                // Only where something was swallowed: a closed circle and
                // its one corner are as the file has them until a merge or
                // a seam touches them.
                let touched = up.iter().any(|e| self.stand_in.contains_key(e));
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
                if through >= 2 && (self.merged.contains(&only.1) || touched) {
                    self.stand_in.insert((dim, tag), only);
                }
            }
        }
    }

    /// Merge what meets smoothly: surfaces across a curve where they meet
    /// within `degrees` of flat, then curves at a corner where they are the
    /// only two left and meet within `degrees` of straight, running along
    /// the same surfaces. What a CAD export cuts a closed surface or a
    /// closed curve into, put back together.
    ///
    /// Each join is said with how far it is from smooth, the merged ones
    /// and the ones that were not, so a reader can see why.
    pub fn merge_smooth(&mut self, geometry: &Geometry, degrees: f64) -> Smoothing {
        let mut out = Smoothing::default();
        // Surfaces, across their shared curves.
        let creases = creases(geometry);
        let mut union = Union::default();
        for (curve, a, b, angle) in &creases {
            if self.stand_in.contains_key(&(crate::SURFACE, *a))
                || self.stand_in.contains_key(&(crate::SURFACE, *b))
            {
                continue;
            }
            let smooth = *angle <= degrees;
            if a == b && self.stand_in.contains_key(&(crate::CURVE, *curve)) {
                continue;
            }
            out.joins.push(Join {
                dim: crate::SURFACE,
                across: (crate::CURVE, *curve),
                pair: (*a, *b),
                degrees: *angle,
                merged: smooth,
            });
            if smooth && a != b {
                union.join(*a, *b);
            }
        }
        for group in union.groups() {
            if let Ok(tag) = self.merge(geometry, crate::SURFACE, &group) {
                out.merges.push(tag);
            }
        }
        // A seam down one surface, smooth across, goes into it.
        for join in &out.joins {
            if join.merged && join.pair.0 == join.pair.1 {
                self.seams.insert((join.across.1, join.pair.0));
                out.seams.push((join.across.1, join.pair.0));
            }
        }
        if !out.seams.is_empty() {
            self.settle_merges(geometry);
        }
        // Then curves, at corners, as the surfaces now are.
        let mut union = Union::default();
        for corner in geometry.corners() {
            if self.stand_in.contains_key(&(CORNER, corner.tag)) {
                continue;
            }
            let through: Vec<i32> = geometry
                .containing(CORNER, corner.tag)
                .iter()
                .filter(|e| e.0 == crate::CURVE)
                .filter(|e| self.image(e.0, e.1).0 == crate::CURVE)
                .map(|e| e.1)
                .collect();
            let [a, b] = through[..] else { continue };
            if self.stand_in.contains_key(&(crate::CURVE, a))
                || self.stand_in.contains_key(&(crate::CURVE, b))
            {
                continue;
            }
            let on = |c: i32| -> BTreeSet<(u8, i32)> {
                geometry
                    .containing(crate::CURVE, c)
                    .iter()
                    .filter(|e| e.0 == crate::SURFACE)
                    .map(|e| self.image(e.0, e.1))
                    .collect()
            };
            let Some(angle) = bend(geometry, corner.node, a, b) else {
                continue;
            };
            let smooth = angle <= degrees && on(a) == on(b);
            out.joins.push(Join {
                dim: crate::CURVE,
                across: (CORNER, corner.tag),
                pair: (a, b),
                degrees: angle,
                merged: smooth,
            });
            if smooth {
                union.join(a, b);
            }
        }
        for group in union.groups() {
            if let Ok(tag) = self.merge(geometry, crate::CURVE, &group) {
                out.merges.push(tag);
            }
        }
        out
    }

    /// Everything something now stands in for, and what: what changes the
    /// picture of the geometry when it changes.
    pub fn taken(&self) -> Vec<((u8, i32), (u8, i32))> {
        self.stand_in.iter().map(|(e, by)| (*e, *by)).collect()
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
    /// The faceting: every tag there is, and what draws them. For an exact
    /// geometry, its tessellation, with the same tags.
    pub geometry: &'a Geometry,
    /// The exact geometry, when there is one: then a point is projected
    /// onto it, and what contains what is its topology's.
    pub exact: Option<&'a crate::cad::CadGeometry>,
    pub composites: &'a Composites,
}

impl<'a> Model<'a> {
    /// Over a geometry: the exact one it is the tessellation of, if it is
    /// one ([`Geometry::exact`]), and the faceting itself otherwise.
    pub fn new(geometry: &'a Geometry, composites: &'a Composites) -> Self {
        Self {
            geometry,
            exact: geometry.exact(),
            composites,
        }
    }

    /// The same model over its faceting alone, the exact geometry left
    /// aside: for ranking entities by how near they are, which the
    /// triangles, within a deflection of the shapes, do as well at a
    /// thousandth of the cost.
    pub fn faceted(&self) -> Model<'a> {
        Model {
            exact: None,
            ..*self
        }
    }

    // What the geometry underneath answers: the exact one's when there is
    // one, the faceting's otherwise. Everything else here is built on
    // these, so the composites work the same over both.

    fn base_project(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<[f64; 3]> {
        match self.exact {
            Some(cad) => cad.project(dim, tag, p),
            None => self.geometry.project(dim, tag, p),
        }
    }

    fn base_distance(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<f64> {
        match self.exact {
            Some(cad) => cad.distance(dim, tag, p),
            None => self.geometry.distance(dim, tag, p),
        }
    }

    fn base_nearest(&self, p: [f64; 3], tolerance: [f64; 3]) -> Option<(u8, i32)> {
        let Some(cad) = self.exact else {
            return self.geometry.nearest(p, tolerance);
        };
        if cad.deflection() <= 0.0 {
            return cad.nearest(p, tolerance);
        }
        // The faceting first, which is within a deflection of the shapes
        // and costs a thousandth as much: an entity whose triangles are
        // farther than the tolerance and two deflections cannot be within
        // the tolerance exactly, and is never projected onto.
        let slack = 2.0 * cad.deflection() + cad.tolerance();
        for dim in [CORNER, crate::CURVE, crate::SURFACE] {
            let tags: Vec<i32> = match dim {
                CORNER => cad.vertices().iter().map(|v| v.tag).collect(),
                crate::CURVE => cad.edges().iter().map(|e| e.tag).collect(),
                _ => cad.faces().iter().map(|f| f.tag).collect(),
            };
            let best = tags
                .into_iter()
                .filter(|&tag| {
                    // A degenerate edge is in no faceting: it is a point,
                    // and cheap to measure exactly.
                    self.geometry
                        .distance(dim, tag, p)
                        .is_none_or(|d| d <= tolerance[dim as usize] + slack)
                })
                .filter_map(|tag| cad.distance(dim, tag, p).map(|d| (d, tag)))
                .filter(|(d, _)| *d <= tolerance[dim as usize])
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            if let Some((_, tag)) = best {
                return Some((dim, tag));
            }
        }
        None
    }

    fn base_containing(&self, dim: u8, tag: i32) -> &'a [(u8, i32)] {
        match self.exact {
            Some(cad) => cad.containing(dim, tag),
            None => self.geometry.containing(dim, tag),
        }
    }

    /// How big the model is, corner to corner — a tolerance is a share of
    /// something, and this is the something.
    pub fn span(&self) -> f64 {
        match self.exact {
            Some(cad) => cad.span(),
            None => self.geometry.span(),
        }
    }

    /// The point of an entity nearest `p`. For a composite, the nearest
    /// point of whichever member is nearest.
    pub fn project(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<[f64; 3]> {
        match self.composites.members(tag) {
            None => self.base_project(dim, tag, p),
            Some((dim, members)) => members
                .iter()
                .filter_map(|&m| {
                    let at = self.base_project(dim, m, p)?;
                    Some((apart(at, p), at))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, at)| at),
        }
    }

    /// How far `p` is from an entity.
    pub fn distance(&self, dim: u8, tag: i32, p: [f64; 3]) -> Option<f64> {
        match self.composites.members(tag) {
            None => self.base_distance(dim, tag, p),
            Some((dim, members)) => members
                .iter()
                .filter_map(|&m| self.base_distance(dim, m, p))
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
        if self.composites.stand_in.is_empty() {
            return self.base_nearest(p, tolerance);
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
                Some(&first) => self.base_containing(dim, first).iter().copied().collect(),
                None => BTreeSet::new(),
            };
            for &m in members.iter().skip(1) {
                let theirs: BTreeSet<(u8, i32)> =
                    self.base_containing(dim, m).iter().copied().collect();
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
                        .any(|&t| self.base_containing(dim, m).contains(&(group, t)))
                });
                if under {
                    out.push((group, name));
                }
            }
            return out;
        }

        let mut up: Vec<(u8, i32)> = self.base_containing(dim, tag).to_vec();
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

/// What [`Composites::merge_smooth`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smoothing {
    /// The merges it made, by tag.
    pub merges: Vec<i32>,
    /// The seams it took into their surface, `(curve, surface)`.
    pub seams: Vec<(i32, i32)>,
    /// Every join it looked at.
    pub joins: Vec<Join>,
}

/// Two entities meeting, and how far from smooth.
#[derive(Clone, Debug, PartialEq)]
pub struct Join {
    /// Of the two entities: surfaces or curves.
    pub dim: u8,
    /// What they meet along, or at.
    pub across: (u8, i32),
    pub pair: (i32, i32),
    /// From flat, for surfaces; from straight, for curves.
    pub degrees: f64,
    pub merged: bool,
}

/// Union–find over tags, groups handed back sorted.
#[derive(Default)]
struct Union {
    parent: BTreeMap<i32, i32>,
}

impl Union {
    fn root(&mut self, x: i32) -> i32 {
        let p = *self.parent.entry(x).or_insert(x);
        if p == x {
            return x;
        }
        let r = self.root(p);
        self.parent.insert(x, r);
        r
    }

    fn join(&mut self, a: i32, b: i32) {
        let (ra, rb) = (self.root(a), self.root(b));
        if ra != rb {
            self.parent.insert(ra.max(rb), ra.min(rb));
        }
    }

    fn groups(mut self) -> Vec<Vec<i32>> {
        let keys: Vec<i32> = self.parent.keys().copied().collect();
        let mut by: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for k in keys {
            let r = self.root(k);
            by.entry(r).or_default().push(k);
        }
        by.into_values().filter(|g| g.len() > 1).collect()
    }
}

/// Every curve bounding exactly two surfaces, with the worst crease along
/// it: how far from flat the two surfaces meet, in degrees, over every
/// segment of the curve.
///
/// Read off the two triangles on a segment, one in each surface, by the
/// way each leans away from it — which needs no orientation, so two
/// surfaces wound against each other are measured the same.
fn creases(geometry: &Geometry) -> Vec<(i32, i32, i32, f64)> {
    let nodes = geometry.nodes();
    let mut beside: BTreeMap<(u32, u32), Vec<(i32, u32)>> = BTreeMap::new();
    for surface in geometry.surfaces() {
        for t in &surface.triangles {
            for k in 0..3 {
                let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                beside
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push((surface.tag, c));
            }
        }
    }
    // How much each surface folds on its own, at the worst of its interior
    // edges: how coarsely it is faceted. A cylinder of 24 facets folds 15°
    // at every edge along it, and a seam down it folding as much is no
    // crease; a CAD surface is smooth inside, so this is never a feature.
    // The edges along curves are left out — a seam is what is measured,
    // not what it is measured against.
    let on_curves: BTreeSet<(u32, u32)> = geometry
        .curves()
        .iter()
        .flat_map(|c| c.segments.iter().map(|s| (s[0].min(s[1]), s[0].max(s[1]))))
        .collect();
    let mut bends: BTreeMap<i32, f64> = geometry.surfaces().iter().map(|s| (s.tag, 0.0)).collect();
    for ((p, q), around) in &beside {
        if on_curves.contains(&(*p, *q)) {
            continue;
        }
        let [(sa, ca), (sb, cb)] = around[..] else {
            continue;
        };
        if sa != sb {
            continue;
        }
        let (pp, qq) = (nodes[*p as usize], nodes[*q as usize]);
        let (Some(da), Some(db)) = (
            lean(pp, qq, nodes[ca as usize]),
            lean(pp, qq, nodes[cb as usize]),
        ) else {
            continue;
        };
        let worst = bends.entry(sa).or_insert(0.0);
        *worst = worst.max(folded(da, db));
    }
    let mut out = Vec::new();
    for curve in geometry.curves() {
        let surfaces: Vec<i32> = geometry
            .containing(crate::CURVE, curve.tag)
            .iter()
            .filter(|e| e.0 == crate::SURFACE)
            .map(|e| e.1)
            .collect();
        let (a, b) = match surfaces[..] {
            [a, b] => (a, b),
            // The one surface on both sides: a seam down it.
            [a] => (a, a),
            _ => continue,
        };
        let mut worst: Option<f64> = None;
        for seg in &curve.segments {
            let key = (seg[0].min(seg[1]), seg[0].max(seg[1]));
            let Some(around) = beside.get(&key) else {
                continue;
            };
            let thirds: Vec<u32> = around
                .iter()
                .filter(|(t, _)| *t == a || *t == b)
                .map(|(_, c)| *c)
                .collect();
            let [ca, cb] = thirds[..] else { continue };
            let (p, q) = (nodes[seg[0] as usize], nodes[seg[1] as usize]);
            let (Some(da), Some(db)) = (
                lean(p, q, nodes[ca as usize]),
                lean(p, q, nodes[cb as usize]),
            ) else {
                continue;
            };
            // Flat is the two leaning exactly away from each other — less
            // what each surface bends anyway at the triangle's other two
            // edges: a cylinder drawn in 24 facets bends 15° at every one,
            // and a seam down it bending as much is no crease.
            let off = (folded(da, db) - bends[&a].max(bends[&b])).max(0.0);
            worst = Some(worst.map_or(off, |w: f64| w.max(off)));
        }
        if let Some(w) = worst {
            out.push((curve.tag, a, b, w));
        }
    }
    out
}

/// How far from flat two triangles meet along an edge, given how each
/// leans away from it.
fn folded(da: [f64; 3], db: [f64; 3]) -> f64 {
    let cos = (da[0] * db[0] + da[1] * db[1] + da[2] * db[2]).clamp(-1.0, 1.0);
    180.0 - cos.acos().to_degrees()
}

/// How far off its edge a triangle's third node has to be, as a share of
/// the edge, for the triangle to lean any way at all. A sliver flatter than
/// this — what a mesher leaves where a CAD export has two corners on one
/// point — has no direction: read as one, it folds anything up to 180°.
const SLIVER: f64 = 1e-3;

/// The unit direction from the segment `p q` towards `c`, square to it, or
/// nothing when that has no meaning: `c` on the segment's line or as good as
/// on it, or the segment itself as good as a point next to the triangle —
/// two nodes on one spot, as where an export left two corners together.
fn lean(p: [f64; 3], q: [f64; 3], c: [f64; 3]) -> Option<[f64; 3]> {
    let along = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let to = [c[0] - p[0], c[1] - p[1], c[2] - p[2]];
    let ll = along.iter().map(|x| x * x).sum::<f64>();
    let reach = to.iter().map(|x| x * x).sum::<f64>();
    if ll <= 0.0 || ll < SLIVER * SLIVER * reach {
        return None;
    }
    let t = (0..3).map(|k| to[k] * along[k]).sum::<f64>() / ll;
    let d = [0, 1, 2].map(|k| to[k] - t * along[k]);
    let n = d.iter().map(|x| x * x).sum::<f64>().sqrt();
    (n > SLIVER * ll.sqrt()).then(|| d.map(|x| x / n))
}

/// How far from straight two curves meet at a node, **beyond** how far
/// each already turns at its next node: a curve drawn as a polygon turns
/// at every node, and a circle of 22 segments turns 16° at each — which is
/// no kink, and a corner where two such halves meet turning just as much is
/// not one either.
fn bend(geometry: &Geometry, node: u32, a: i32, b: i32) -> Option<f64> {
    let nodes = geometry.nodes();
    let unit = |from: u32, to: u32| -> Option<[f64; 3]> {
        let (p, q) = (nodes[from as usize], nodes[to as usize]);
        let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let n = d.iter().map(|x| x * x).sum::<f64>().sqrt();
        (n > 0.0).then(|| d.map(|x| x / n))
    };
    let turn = |u: [f64; 3], v: [f64; 3]| {
        let cos = (u[0] * v[0] + u[1] * v[1] + u[2] * v[2]).clamp(-1.0, 1.0);
        180.0 - cos.acos().to_degrees()
    };
    // The first node along a curve from this one, and how much the curve
    // turns there — nothing, when the curve is one segment long.
    let leaving = |tag: i32| -> Option<([f64; 3], f64)> {
        let curve = geometry.curves().iter().find(|c| c.tag == tag)?;
        let first = curve
            .segments
            .iter()
            .find(|s| s[0] == node || s[1] == node)?;
        let next = if first[0] == node { first[1] } else { first[0] };
        let out = unit(node, next)?;
        let own = curve
            .segments
            .iter()
            .filter(|s| *s != first && (s[0] == next || s[1] == next))
            .find_map(|s| {
                let after = if s[0] == next { s[1] } else { s[0] };
                Some(turn(unit(next, node)?, unit(next, after)?))
            })
            .unwrap_or(0.0);
        Some((out, own))
    };
    let ((da, ta), (db, tb)) = (leaving(a)?, leaving(b)?);
    Some((turn(da, db) - ta.max(tb)).max(0.0))
}

/// How many ends of a curve's segments are at a corner's node: one where
/// the curve ends there, two where a closed curve passes through it.
fn ends_at(geometry: &Geometry, corner: i32, curve: i32) -> usize {
    let Some(node) = geometry
        .corners()
        .iter()
        .find(|c| c.tag == corner)
        .map(|c| c.node)
    else {
        return 0;
    };
    let Some(c) = geometry.curves().iter().find(|c| c.tag == curve) else {
        return 0;
    };
    // A closed curve's segments meet end to end at every node, so a node
    // inside it is two ends and so is its own corner: what tells them
    // apart is that the corner is the curve's start and its end.
    let ends = c
        .segments
        .iter()
        .map(|s| usize::from(s[0] == node) + usize::from(s[1] == node))
        .sum::<usize>();
    ends.min(2)
}
