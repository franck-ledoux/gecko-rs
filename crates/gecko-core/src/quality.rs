//! How good a mesh is: the scaled Jacobian of every hexahedron, and what a
//! reader wants to know about the lot of them.
//!
//! The scaled Jacobian of a corner is the volume of the three unit edges
//! leaving it: one at a right-angled corner, nought where the corner has
//! folded flat, and negative where it has folded through. A hexahedron's is
//! the worst of its eight corners, which is the number solvers and every
//! other mesher quote — so a figure read here can be compared with one read
//! anywhere else.
//!
//! Measured on the **mesh**, not the blocking: a block whose corners are all
//! square can still hold cells turned inside out, where one of its faces
//! follows a curve that bulges further than the block next to it is thick.
//! That is the case this is for.

use crate::cells::scaled_jacobian;
use crate::mesh::Mesh;

/// For each corner of a hexahedron in VTK order, the three corners an edge
/// joins it to, turning the right-handed way: bottom face anticlockwise seen
/// from outside, then the face above it.
pub const VTK_CORNER_EDGES: [[usize; 3]; 8] = [
    [1, 3, 4],
    [2, 0, 5],
    [3, 1, 6],
    [0, 2, 7],
    [7, 5, 0],
    [4, 6, 1],
    [5, 7, 2],
    [6, 4, 3],
];

/// Where the histogram's bins start, the last running to one. The first is
/// every cell turned inside out; the next two are the cells most solvers
/// will complain about.
pub const BINS: [f64; 10] = [-1.0, 0.0, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];

/// The thresholds a summary counts the cells below.
pub const THRESHOLDS: [f64; 4] = [0.0, 0.2, 0.3, 0.5];

/// The worst corner of a hexahedron given in VTK order.
pub fn hex_quality(at: &[[f64; 3]; 8]) -> f64 {
    VTK_CORNER_EDGES
        .iter()
        .enumerate()
        .map(|(c, sides)| scaled_jacobian(at[c], sides.map(|s| at[s])))
        .fold(f64::INFINITY, f64::min)
}

impl Mesh {
    /// The worst corner of every hexahedron, in the order they are listed.
    pub fn quality(&self) -> Vec<f64> {
        self.hexes
            .iter()
            .map(|hex| hex_quality(&hex.map(|n| self.points[n as usize])))
            .collect()
    }

    /// The middle of a hexahedron: its eight corners averaged.
    pub fn hex_center(&self, hex: usize) -> [f64; 3] {
        let mut c = [0.0; 3];
        for n in self.hexes[hex] {
            for (sum, x) in c.iter_mut().zip(self.points[n as usize]) {
                *sum += x / 8.0;
            }
        }
        c
    }
}

/// What a reader wants to know about a mesh's quality at a glance.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub cells: usize,
    pub min: f64,
    pub mean: f64,
    /// The worst hexahedron, by its place in the list.
    pub worst: Option<usize>,
    /// How many are below each of [`THRESHOLDS`], in the same order.
    pub below: [usize; 4],
    /// How many fall in each bin of [`BINS`].
    pub histogram: [usize; 10],
}

/// Count it all up. An empty mesh has no worst and a minimum of one, so
/// that nothing reads it as broken.
pub fn summarize(quality: &[f64]) -> Summary {
    let mut out = Summary {
        cells: quality.len(),
        min: 1.0,
        mean: 0.0,
        worst: None,
        below: [0; 4],
        histogram: [0; 10],
    };
    if quality.is_empty() {
        return out;
    }
    let mut sum = 0.0;
    for (h, &q) in quality.iter().enumerate() {
        sum += q;
        if out.worst.is_none() || q < out.min {
            out.min = q;
            out.worst = Some(h);
        }
        for (k, t) in THRESHOLDS.iter().enumerate() {
            if q < *t {
                out.below[k] += 1;
            }
        }
        let bin = BINS.iter().rposition(|start| q >= *start).unwrap_or(0);
        out.histogram[bin] += 1;
    }
    out.mean = sum / quality.len() as f64;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> [[f64; 3]; 8] {
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ]
    }

    #[test]
    fn a_cube_scores_one_and_folding_it_through_scores_below_nought() {
        assert!((hex_quality(&cube()) - 1.0).abs() < 1e-12);
        // Sheared: the top pushed over by half its height.
        let mut leaning = cube();
        for p in &mut leaning[4..] {
            p[0] += 0.5;
        }
        let q = hex_quality(&leaning);
        assert!((q - 1.0 / 1.25f64.sqrt()).abs() < 1e-12, "{q}");
        // One corner pushed through the opposite face.
        let mut folded = cube();
        folded[6] = [-0.5, -0.5, -0.5];
        assert!(hex_quality(&folded) < 0.0);
    }

    #[test]
    fn a_summary_counts_what_it_says() {
        let s = summarize(&[1.0, 0.95, 0.45, 0.25, 0.1, -0.3]);
        assert_eq!(s.cells, 6);
        assert_eq!(s.min, -0.3);
        assert_eq!(s.worst, Some(5));
        assert_eq!(s.below, [1, 2, 3, 4]);
        assert_eq!(s.histogram, [1, 1, 1, 0, 1, 0, 0, 0, 0, 2]);
        assert!((s.mean - 2.45 / 6.0).abs() < 1e-12);
        let empty = summarize(&[]);
        assert_eq!((empty.min, empty.worst), (1.0, None));
    }
}
