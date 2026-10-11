//! The boundary representation a gmsh file describes.
//!
//! # What groups what
//!
//! Every element carries two tags, and only one of them says which geometric
//! entity it belongs to:
//!
//! - triangles sharing an **elementary entity tag** are one [`Surface`],
//! - segments sharing one are one [`Curve`],
//! - a point element *is* a [`Corner`], read rather than deduced,
//! - an element whose entity tag is 0 belongs to no entity, and is dropped.
//!
//! That is `FacetedGeometry`'s rule in the C++ GECKO, taken over unchanged.
//! The other tag — the `$PhysicalNames` group — is kept beside the model and
//! never groups anything: a physical group is a *name for a set of entities*,
//! routinely spanning several of them, and in every `.msh` in GECKO's own
//! test data it is 0 on every element while the entity tag carries the whole
//! structure.
//!
//! # What is not here
//!
//! No volumes: this is a surface mesh by construction. Tetrahedra are skipped
//! by the reader, counted as [`Geometry::skipped`].
//!
//! The **incidence** — which curves pass through a corner, which surfaces a
//! curve bounds — is in [`crate::where_on`], built here once the entities
//! exist.

use crate::msh::{self, Msh, MshError, PhysicalName, LINE, POINT, TRIANGLE};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GeometryError {
    #[error(transparent)]
    Msh(#[from] MshError),
    #[error("this file has no triangles — a geometry is a surface mesh, and this is not one")]
    NoTriangles,
    #[error(
        "none of the {0} triangles carries an elementary entity tag, \
         so there is no surface to build — gmsh writes that tag as an element's second"
    )]
    NoEntityTags(usize),
}

/// A geometric corner: one node of the faceting, named by a point element.
///
/// Which node is a corner is a statement about the model, not something a
/// pile of triangles implies, so it is read from the file and nowhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corner {
    /// The gmsh elementary entity tag. This is the corner's name.
    pub tag: i32,
    /// Index into [`Geometry::nodes`].
    pub node: u32,
}

/// A geometric curve: the segments sharing one entity tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Curve {
    pub tag: i32,
    /// Two indices into [`Geometry::nodes`] each, as the file ordered them.
    pub segments: Vec<[u32; 2]>,
}

/// A geometric surface: the triangles sharing one entity tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    pub tag: i32,
    /// Three indices into [`Geometry::nodes`] each, wound as the file wound
    /// them — which for a gmsh boundary is outward from the volume.
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct Geometry {
    nodes: Vec<[f64; 3]>,
    corners: Vec<Corner>,
    curves: Vec<Curve>,
    surfaces: Vec<Surface>,
    names: Vec<PhysicalName>,
    skipped: usize,
    /// Everything containing each entity, itself included. Worked out once
    /// here rather than on demand: every edge classified intersects two of
    /// these, and nothing moves a geometry once it is read.
    containment: BTreeMap<(u8, i32), Vec<(u8, i32)>>,
}

impl Geometry {
    /// Read a gmsh MSH 2.2 ASCII file and build the model it describes.
    pub fn read(text: &str) -> Result<Geometry, GeometryError> {
        Geometry::of(&msh::read(text)?)
    }

    pub fn of(file: &Msh) -> Result<Geometry, GeometryError> {
        let triangles = file.elements.iter().filter(|e| e.kind == TRIANGLE).count();
        if triangles == 0 {
            return Err(GeometryError::NoTriangles);
        }
        let kept: Vec<_> = file.elements.iter().filter(|e| e.entity != 0).collect();
        if !kept.iter().any(|e| e.kind == TRIANGLE) {
            return Err(GeometryError::NoEntityTags(triangles));
        }

        // Only the nodes the kept elements actually use, renumbered.
        //
        // A gmsh file of a meshed CAD model carries the volume mesh too, and
        // most of its nodes are interior: keeping all of them would put tens
        // of thousands of points nothing draws into every view buffer. The
        // renumbering is monotonic in the file's own order, so two entities
        // that shared a node still share one — which is what any later
        // incidence test depends on.
        let mut used = vec![false; file.nodes.len()];
        for element in &kept {
            for &n in &element.nodes[..element.node_count()] {
                if let Some(slot) = used.get_mut(n as usize) {
                    *slot = true;
                }
            }
        }
        let mut renumbered = vec![u32::MAX; file.nodes.len()];
        let mut nodes = Vec::new();
        for (old, keep) in used.iter().enumerate() {
            if *keep {
                renumbered[old] = nodes.len() as u32;
                nodes.push(file.nodes[old]);
            }
        }
        let at = |n: u32| renumbered[n as usize];

        // `BTreeMap`, so a model built twice from one file comes out in the
        // same order twice — and so the order is the tags' own, which is the
        // one a reader of the file would expect.
        let mut curves: BTreeMap<i32, Vec<[u32; 2]>> = BTreeMap::new();
        let mut surfaces: BTreeMap<i32, Vec<[u32; 3]>> = BTreeMap::new();
        let mut corners: BTreeMap<i32, u32> = BTreeMap::new();
        for element in &kept {
            match element.kind {
                POINT => {
                    // The first point element with a tag defines that corner.
                    // A tag on two of them is a file saying one thing twice.
                    corners
                        .entry(element.entity)
                        .or_insert(at(element.nodes[0]));
                }
                LINE => curves
                    .entry(element.entity)
                    .or_default()
                    .push([at(element.nodes[0]), at(element.nodes[1])]),
                TRIANGLE => surfaces.entry(element.entity).or_default().push([
                    at(element.nodes[0]),
                    at(element.nodes[1]),
                    at(element.nodes[2]),
                ]),
                _ => {}
            }
        }

        let mut geometry = Geometry {
            nodes,
            corners: corners
                .into_iter()
                .map(|(tag, node)| Corner { tag, node })
                .collect(),
            curves: curves
                .into_iter()
                .map(|(tag, segments)| Curve { tag, segments })
                .collect(),
            surfaces: surfaces
                .into_iter()
                .map(|(tag, triangles)| Surface { tag, triangles })
                .collect(),
            names: file.names.clone(),
            skipped: file.skipped,
            containment: BTreeMap::new(),
        };
        geometry.containment = crate::where_on::containment(&geometry);
        Ok(geometry)
    }

    pub(crate) fn containment(&self) -> &BTreeMap<(u8, i32), Vec<(u8, i32)>> {
        &self.containment
    }

    /// Every node of the faceting, shared between the entities that use it.
    pub fn nodes(&self) -> &[[f64; 3]] {
        &self.nodes
    }

    pub fn corners(&self) -> &[Corner] {
        &self.corners
    }

    pub fn curves(&self) -> &[Curve] {
        &self.curves
    }

    pub fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }

    /// The file's `$PhysicalNames`, in file order.
    pub fn names(&self) -> &[PhysicalName] {
        &self.names
    }

    /// How many elements of the file were of a kind this does not read —
    /// tetrahedra, in practice.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    pub fn nb_segments(&self) -> usize {
        self.curves.iter().map(|c| c.segments.len()).sum()
    }

    pub fn nb_triangles(&self) -> usize {
        self.surfaces.iter().map(|s| s.triangles.len()).sum()
    }
}
