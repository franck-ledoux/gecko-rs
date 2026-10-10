//! A box round a geometry, or round part of one.
//!
//! The first block of a meshing session is almost always this: something
//! that contains the shape, to be cut and fitted onto it afterwards. The
//! C++ GECKO has the same button (`BiyApp::create_bounding_box`), with the
//! same 10% margin.
//!
//! Two frames to measure in. **Axes** is the world's own, which is what you
//! want when the geometry was modelled square to them. **Inertia** is the
//! shape's own: a part lying at forty degrees gets a box lying at forty
//! degrees, rather than one half full of air.
//!
//! # Inertia, principal axes, and why they are the same thing
//!
//! The inertia tensor of a body is `J = tr(C)·I − C`, where `C` is the
//! covariance of its mass about its centre. Subtracting a matrix from a
//! multiple of the identity leaves the eigen**vectors** alone and only
//! reorders the eigen**values** — the heaviest axis of one is the lightest
//! of the other. So the axes of inertia and the principal axes are the same
//! three directions, and the box they give is the same box. What is
//! computed below is the covariance, because it is the one with no
//! subtraction in it to get backwards.
//!
//! # When the axes do not exist
//!
//! A shape with a square or round cross-section — a cylinder, a bar with
//! equal sides — has two equal eigenvalues, and then there is no preferred
//! way up: every direction in that plane is as principal as every other,
//! and the box is free to turn about the shape's length. That is a fact
//! about the shape rather than a weakness of the arithmetic; the smallest
//! box is a different question, with a different and much longer answer.
//! When it matters, the world's own axes are the predictable choice.

use crate::model::Geometry;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// The world's own X, Y and Z.
    Axes,
    /// The geometry's own, from the covariance of the surfaces chosen.
    Inertia,
}

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
        frame: Frame,
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

        let axes = match frame {
            Frame::Axes => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Frame::Inertia => principal_axes(&triangles),
        };

        // The extent is over the **points**, whatever frame they are
        // measured in: a box has to contain the geometry, and the triangles'
        // corners are the geometry. The covariance decides which way the box
        // is turned and nothing else.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for triangle in &triangles {
            for point in triangle {
                for (k, axis) in axes.iter().enumerate() {
                    let along = dot(*point, *axis);
                    lo[k] = lo[k].min(along);
                    hi[k] = hi[k].max(along);
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
        Ok(signs.map(|sign| {
            let mut corner = [0.0; 3];
            for k in 0..3 {
                let along = middle[k] + sign[k] * half[k];
                for axis in 0..3 {
                    corner[axis] += along * axes[k][axis];
                }
            }
            corner
        }))
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

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The three directions a set of triangles is spread along, longest first.
///
/// **Weighted by area, not by vertex.** A faceted CAD surface has wildly
/// uneven triangles — a cylinder's flat end is two of them and its round
/// side is two hundred — so counting corners would measure the mesher's
/// habits rather than the shape. Each triangle contributes its own second
/// moment, which has a closed form, plus its area times how far its centre
/// is from the middle of everything.
fn principal_axes(triangles: &[[[f64; 3]; 3]]) -> [[f64; 3]; 3] {
    let mut total = 0.0;
    let mut middle = [0.0; 3];
    let mut parts: Vec<(f64, [f64; 3])> = Vec::with_capacity(triangles.len());
    for t in triangles {
        let area = triangle_area(t);
        let centre = [0, 1, 2].map(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0);
        total += area;
        for k in 0..3 {
            middle[k] += area * centre[k];
        }
        parts.push((area, centre));
    }
    // Every triangle degenerate: no direction means anything, so the world's
    // own axes are as good an answer as there is.
    if total <= 0.0 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    for axis in &mut middle {
        *axis /= total;
    }

    // Parallel-axis, triangle by triangle: its spread about its own centre,
    // and its centre's distance from the middle of the lot. The first term's
    // closed form is `(A/12)·Σ(pᵢ − g)(pᵢ − g)ᵀ`, from integrating the
    // barycentric coordinates over the triangle.
    let mut covariance = [[0.0; 3]; 3];
    for (t, (area, centre)) in triangles.iter().zip(&parts) {
        for corner in t {
            let d = [0, 1, 2].map(|k| corner[k] - centre[k]);
            for i in 0..3 {
                for j in 0..3 {
                    covariance[i][j] += area / 12.0 * d[i] * d[j];
                }
            }
        }
        let d = [0, 1, 2].map(|k| centre[k] - middle[k]);
        for i in 0..3 {
            for j in 0..3 {
                covariance[i][j] += area * d[i] * d[j];
            }
        }
    }

    let (spread, vectors) = jacobi(covariance);
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| spread[*b].partial_cmp(&spread[*a]).unwrap());
    let mut axes = order.map(|k| {
        let v = [vectors[0][k], vectors[1][k], vectors[2][k]];
        // An eigenvector is only defined up to its sign, so fix one: the
        // largest component positive. Without it the same geometry read
        // twice could come back with the box's corners relabelled.
        let biggest = (0..3).max_by(|a, b| v[*a].abs().partial_cmp(&v[*b].abs()).unwrap());
        let flip = biggest.map(|k| v[k] < 0.0).unwrap_or(false);
        if flip {
            v.map(|x| -x)
        } else {
            v
        }
    });
    // And make the three of them right-handed, or the block comes out
    // inside-out.
    let cross = [
        axes[0][1] * axes[1][2] - axes[0][2] * axes[1][1],
        axes[0][2] * axes[1][0] - axes[0][0] * axes[1][2],
        axes[0][0] * axes[1][1] - axes[0][1] * axes[1][0],
    ];
    if dot(cross, axes[2]) < 0.0 {
        axes[2] = axes[2].map(|x| -x);
    }
    axes
}

fn triangle_area(t: &[[f64; 3]; 3]) -> f64 {
    let u = [0, 1, 2].map(|k| t[1][k] - t[0][k]);
    let v = [0, 1, 2].map(|k| t[2][k] - t[0][k]);
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    dot(cross, cross).sqrt() / 2.0
}

/// Eigenvalues and eigenvectors of a symmetric 3×3, by Jacobi rotations.
///
/// Forty lines rather than a linear-algebra dependency, and for a 3×3 it
/// converges in a handful of sweeps. The vectors come back as the
/// **columns** of the second matrix.
fn jacobi(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..64 {
        // The largest off-diagonal entry is the one worth zeroing.
        let (mut p, mut q, mut largest) = (0, 1, 0.0);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            if a[i][j].abs() > largest {
                largest = a[i][j].abs();
                (p, q) = (i, j);
            }
        }
        if largest <= 1e-18 {
            break;
        }

        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        // The root of t² + 2θt − 1 that is smaller in magnitude, which is
        // what keeps the rotation small and the iteration stable.
        let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
        let c = 1.0 / (t * t + 1.0).sqrt();
        let s = t * c;

        let (app, aqq, apq) = (a[p][p], a[q][q], a[p][q]);
        a[p][p] = c * c * app - 2.0 * s * c * apq + s * s * aqq;
        a[q][q] = s * s * app + 2.0 * s * c * apq + c * c * aqq;
        a[p][q] = 0.0;
        a[q][p] = 0.0;
        for k in 0..3 {
            if k != p && k != q {
                let (akp, akq) = (a[k][p], a[k][q]);
                a[k][p] = c * akp - s * akq;
                a[p][k] = a[k][p];
                a[k][q] = s * akp + c * akq;
                a[q][k] = a[k][q];
            }
            let (vkp, vkq) = (v[k][p], v[k][q]);
            v[k][p] = c * vkp - s * vkq;
            v[k][q] = s * vkp + c * vkq;
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}
