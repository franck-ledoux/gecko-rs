//! A box round a geometry, or round part of one.
//!
//! The first block of a meshing session is almost always this: something
//! that contains the shape, to be cut and fitted onto it afterwards. The
//! C++ GECKO has the same button (`BiyApp::create_bounding_box`), with the
//! same 10% margin.
//!
//! Square to the world, and only that. A box along the shape's own axes of
//! inertia was built here and taken out again: on a real part it is as
//! likely to be unhelpful as helpful, because a shape with a round or
//! square cross-section has two equal eigenvalues and so no preferred way
//! up at all — the box is then free to turn about the shape's length, and
//! does. The useful version of that idea is the *smallest* box, which is a
//! different and much longer answer.

use crate::model::Geometry;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BoundsError {
    #[error("no surface of this geometry is tagged {0}")]
    NoSuchSurface(i32),
    #[error("there is no geometry to put a box round")]
    Empty,
    #[error("a margin has to be a finite number greater than -1, not {0}")]
    BadMargin(f64),
    #[error(
        "what is selected is flat, so a box round it would have no thickness — \
         pick something with some depth to it, or add surfaces facing another way"
    )]
    Flat,
}

/// How thin a box may be, against its own longest side, before it counts as
/// flat. Generous: a block of a millionth the thickness of its own width is
/// not something anyone meant to build.
const FLATNESS: f64 = 1e-6;

impl Geometry {
    /// The eight corners of a box round some of its surfaces, in the order
    /// a hexahedron wants them — the bottom four, then the four above.
    ///
    /// An empty `surfaces` means all of them. `margin` grows each half-side
    /// by that share, so 0.1 is the C++'s own default and 0 is a box resting
    /// exactly on the geometry.
    pub fn bounding_box(
        &self,
        surfaces: &[i32],
        margin: f64,
    ) -> Result<[[f64; 3]; 8], BoundsError> {
        if !margin.is_finite() || margin <= -1.0 {
            return Err(BoundsError::BadMargin(margin));
        }
        let chosen = self.chosen(surfaces)?;
        let triangles: Vec<[[f64; 3]; 3]> = chosen
            .iter()
            .flat_map(|s| s.triangles.iter())
            .map(|t| t.map(|n| self.nodes()[n as usize]))
            .collect();
        if triangles.is_empty() {
            return Err(BoundsError::Empty);
        }

        // Over the **points**: a box has to contain the geometry, and the
        // triangles' corners are the geometry.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for triangle in &triangles {
            for point in triangle {
                for k in 0..3 {
                    lo[k] = lo[k].min(point[k]);
                    hi[k] = hi[k].max(point[k]);
                }
            }
        }

        let longest = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
        if (0..3).any(|k| hi[k] - lo[k] <= longest * FLATNESS) {
            return Err(BoundsError::Flat);
        }

        let middle = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        let half = [0, 1, 2].map(|k| (hi[k] - lo[k]) / 2.0 * (1.0 + margin));

        // The hexahedron's own corner order: the bottom four round the
        // perimeter, then the four directly above them.
        let signs = [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ];
        Ok(signs.map(|sign| [0, 1, 2].map(|k| middle[k] + sign[k] * half[k])))
    }

    /// The surfaces a selection names, or all of them when it names none.
    fn chosen(&self, surfaces: &[i32]) -> Result<Vec<&crate::Surface>, BoundsError> {
        if surfaces.is_empty() {
            return match self.surfaces() {
                [] => Err(BoundsError::Empty),
                all => Ok(all.iter().collect()),
            };
        }
        surfaces
            .iter()
            .map(|tag| {
                self.surfaces()
                    .iter()
                    .find(|s| s.tag == *tag)
                    .ok_or(BoundsError::NoSuchSurface(*tag))
            })
            .collect()
    }
}
