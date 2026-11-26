//! How good a mesh is: three measures of every hexahedron, each under the
//! name every other mesher gives it, and what a reader wants to know about
//! the lot of them.
//!
//! - **Scaled Jacobian.** The volume of the three unit edges leaving a
//!   corner: one at a right-angled corner, nought where the corner has
//!   folded flat, and negative where it has folded through. A hexahedron's
//!   is the worst of its eight corners. Higher is better.
//! - **Skewness.** How far the cell's three principal axes are from square
//!   to each other: the largest cosine between two of them, nought for a
//!   box, one for a cell squashed flat. The axes are Verdict's — each the
//!   sum of the four edges running that way. Lower is better.
//! - **Aspect ratio.** The longest of the twelve edges over the shortest:
//!   one for a cube, and growing as the cell stretches. Verdict calls this
//!   the *edge ratio*. Lower is better.
//!
//! The same definitions as Verdict's, so that a figure read here can be
//! compared with one read in a solver's own mesh check.
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

/// The worst corner of a hexahedron given in VTK order.
pub fn hex_quality(at: &[[f64; 3]; 8]) -> f64 {
    VTK_CORNER_EDGES
        .iter()
        .enumerate()
        .map(|(c, sides)| scaled_jacobian(at[c], sides.map(|s| at[s])))
        .fold(f64::INFINITY, f64::min)
}

/// One of the three measures, by the name a script and the page use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metric {
    ScaledJacobian,
    Skewness,
    AspectRatio,
}

impl Metric {
    pub const ALL: [Metric; 3] = [
        Metric::ScaledJacobian,
        Metric::Skewness,
        Metric::AspectRatio,
    ];

    /// The name a script passes: `scaled_jacobian`, `skewness`,
    /// `aspect_ratio`.
    pub fn key(self) -> &'static str {
        match self {
            Metric::ScaledJacobian => "scaled_jacobian",
            Metric::Skewness => "skewness",
            Metric::AspectRatio => "aspect_ratio",
        }
    }

    /// The name a reader is shown.
    pub fn label(self) -> &'static str {
        match self {
            Metric::ScaledJacobian => "scaled Jacobian",
            Metric::Skewness => "skewness",
            Metric::AspectRatio => "aspect ratio",
        }
    }

    /// The metric a name means, `None` for one that is not a metric.
    pub fn parse(name: &str) -> Option<Metric> {
        Metric::ALL.into_iter().find(|m| m.key() == name.trim())
    }

    /// Whether a higher figure is the better one.
    pub fn higher_is_better(self) -> bool {
        self == Metric::ScaledJacobian
    }

    /// Whether `a` is a worse figure than `b`.
    pub fn worse(self, a: f64, b: f64) -> bool {
        if self.higher_is_better() {
            a < b
        } else {
            a > b
        }
    }

    /// The figure of one hexahedron, corners in VTK order.
    pub fn of(self, at: &[[f64; 3]; 8]) -> f64 {
        match self {
            Metric::ScaledJacobian => hex_quality(at),
            Metric::Skewness => hex_skewness(at),
            Metric::AspectRatio => hex_aspect_ratio(at),
        }
    }

    /// Where the histogram's ten bins start, the last running to the end of
    /// the range: one, one, and as far as it goes. Narrow where the cells
    /// solvers complain about are, wide where nobody looks.
    pub fn bins(self) -> [f64; 10] {
        match self {
            // The first bin is every cell turned inside out.
            Metric::ScaledJacobian => [-1.0, 0.0, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9],
            Metric::Skewness => [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9],
            Metric::AspectRatio => [1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 7.5, 10.0],
        }
    }

    /// The thresholds a summary counts the cells worse than: below them for
    /// the scaled Jacobian, at or above them for the other two.
    pub fn thresholds(self) -> [f64; 4] {
        match self {
            Metric::ScaledJacobian => [0.0, 0.2, 0.3, 0.5],
            Metric::Skewness => [0.5, 0.7, 0.8, 0.9],
            Metric::AspectRatio => [2.0, 3.0, 5.0, 10.0],
        }
    }

    /// Whether `value` is worse than the threshold `t`, as
    /// [`Metric::thresholds`] counts it.
    pub fn beyond(self, value: f64, t: f64) -> bool {
        if self.higher_is_better() {
            value < t
        } else {
            value >= t
        }
    }

    /// The figure on one common scale, for a color: one is perfect, nought
    /// is as bad as the measure goes, and only a scaled Jacobian goes below
    /// — a cell turned inside out.
    pub fn score(self, value: f64) -> f64 {
        match self {
            Metric::ScaledJacobian => value,
            Metric::Skewness => 1.0 - value,
            Metric::AspectRatio => {
                if value >= 1.0 {
                    1.0 / value
                } else {
                    0.0
                }
            }
        }
    }
}

/// The three principal axes of a hexahedron in VTK order: each the sum of
/// the four edges that run that way.
fn principal_axes(at: &[[f64; 3]; 8]) -> [[f64; 3]; 3] {
    let edge = |a: usize, b: usize| {
        [
            at[b][0] - at[a][0],
            at[b][1] - at[a][1],
            at[b][2] - at[a][2],
        ]
    };
    let sum = |e: [[f64; 3]; 4]| {
        [
            e[0][0] + e[1][0] + e[2][0] + e[3][0],
            e[0][1] + e[1][1] + e[2][1] + e[3][1],
            e[0][2] + e[1][2] + e[2][2] + e[3][2],
        ]
    };
    [
        sum([edge(0, 1), edge(3, 2), edge(4, 5), edge(7, 6)]),
        sum([edge(0, 3), edge(1, 2), edge(4, 7), edge(5, 6)]),
        sum([edge(0, 4), edge(1, 5), edge(2, 6), edge(3, 7)]),
    ]
}

/// The skewness of a hexahedron in VTK order: the largest cosine between
/// two of its principal axes. One for a cell with an axis of no length at
/// all, which is as skewed as a cell can be.
pub fn hex_skewness(at: &[[f64; 3]; 8]) -> f64 {
    let axes = principal_axes(at);
    let mut unit = [[0.0; 3]; 3];
    for (u, a) in unit.iter_mut().zip(axes) {
        let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        if n <= 1e-300 {
            return 1.0;
        }
        *u = a.map(|x| x / n);
    }
    let dot = |a: [f64; 3], b: [f64; 3]| (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).abs();
    dot(unit[0], unit[1])
        .max(dot(unit[0], unit[2]))
        .max(dot(unit[1], unit[2]))
        .min(1.0)
}

/// The aspect ratio of a hexahedron in VTK order: its longest edge over its
/// shortest. As large as a float goes for a cell with an edge of no length,
/// rather than infinite, which JSON cannot carry.
pub fn hex_aspect_ratio(at: &[[f64; 3]; 8]) -> f64 {
    let (mut shortest, mut longest) = (f64::INFINITY, 0.0f64);
    for (c, sides) in VTK_CORNER_EDGES.iter().enumerate() {
        for s in sides {
            let e = [
                at[*s][0] - at[c][0],
                at[*s][1] - at[c][1],
                at[*s][2] - at[c][2],
            ];
            let l = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
            shortest = shortest.min(l);
            longest = longest.max(l);
        }
    }
    if shortest <= 1e-300 {
        f64::MAX
    } else {
        longest / shortest
    }
}

impl Mesh {
    /// The scaled Jacobian of every hexahedron, in the order they are
    /// listed.
    pub fn quality(&self) -> Vec<f64> {
        self.metric(Metric::ScaledJacobian)
    }

    /// One measure of every hexahedron, in the order they are listed.
    pub fn metric(&self, metric: Metric) -> Vec<f64> {
        self.hexes
            .iter()
            .map(|hex| metric.of(&hex.map(|n| self.points[n as usize])))
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

/// What a reader wants to know about one measure of a mesh at a glance.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub metric: Metric,
    pub cells: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    /// The worst hexahedron, by its place in the list.
    pub worst: Option<usize>,
    /// How many are worse than each of the metric's thresholds, in the
    /// same order.
    pub beyond: [usize; 4],
    /// How many fall in each of the metric's bins.
    pub histogram: [usize; 10],
}

/// Count it all up. An empty mesh has no worst, and the metric's perfect
/// figure for its minimum and maximum, so that nothing reads it as broken.
pub fn summarize(metric: Metric, values: &[f64]) -> Summary {
    let perfect = if metric == Metric::Skewness { 0.0 } else { 1.0 };
    let mut out = Summary {
        metric,
        cells: values.len(),
        min: perfect,
        max: perfect,
        mean: 0.0,
        worst: None,
        beyond: [0; 4],
        histogram: [0; 10],
    };
    if values.is_empty() {
        return out;
    }
    let (mut min, mut max, mut sum) = (f64::INFINITY, f64::NEG_INFINITY, 0.0);
    let bins = metric.bins();
    let thresholds = metric.thresholds();
    for (h, &v) in values.iter().enumerate() {
        sum += v;
        min = min.min(v);
        max = max.max(v);
        if out.worst.is_none_or(|w| metric.worse(v, values[w])) {
            out.worst = Some(h);
        }
        for (k, t) in thresholds.iter().enumerate() {
            if metric.beyond(v, *t) {
                out.beyond[k] += 1;
            }
        }
        let bin = bins.iter().rposition(|start| v >= *start).unwrap_or(0);
        out.histogram[bin] += 1;
    }
    out.min = min;
    out.max = max;
    out.mean = sum / values.len() as f64;
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
    fn a_cube_has_no_skew_and_a_leaning_one_has_the_cosine_of_its_lean() {
        assert_eq!(hex_skewness(&cube()), 0.0);
        let mut leaning = cube();
        for p in &mut leaning[4..] {
            p[0] += 0.5;
        }
        // The upward axis leans over by half its height; the other two
        // stay square to each other.
        let s = hex_skewness(&leaning);
        assert!((s - 0.5 / 1.25f64.sqrt()).abs() < 1e-12, "{s}");
        let mut flat = cube();
        for p in &mut flat[4..] {
            p[2] = 0.0;
        }
        assert_eq!(hex_skewness(&flat), 1.0, "an axis of no length");
    }

    #[test]
    fn the_aspect_ratio_is_the_longest_edge_over_the_shortest() {
        assert_eq!(hex_aspect_ratio(&cube()), 1.0);
        let mut long = cube();
        for p in &mut long {
            p[0] *= 3.0;
        }
        assert!((hex_aspect_ratio(&long) - 3.0).abs() < 1e-12);
        let mut collapsed = cube();
        collapsed[1] = collapsed[0];
        assert_eq!(hex_aspect_ratio(&collapsed), f64::MAX);
    }

    #[test]
    fn a_metric_is_known_by_its_name_and_scored_one_when_perfect() {
        for m in Metric::ALL {
            assert_eq!(Metric::parse(m.key()), Some(m));
            assert_eq!(m.score(m.of(&cube())), 1.0, "{m:?}");
        }
        assert_eq!(Metric::parse("jacobian"), None);
        assert!(Metric::Skewness.worse(0.6, 0.2));
        assert!(Metric::ScaledJacobian.worse(0.2, 0.6));
    }

    #[test]
    fn a_summary_counts_what_it_says() {
        let s = summarize(Metric::ScaledJacobian, &[1.0, 0.95, 0.45, 0.25, 0.1, -0.3]);
        assert_eq!(s.cells, 6);
        assert_eq!((s.min, s.max), (-0.3, 1.0));
        assert_eq!(s.worst, Some(5));
        assert_eq!(s.beyond, [1, 2, 3, 4]);
        assert_eq!(s.histogram, [1, 1, 1, 0, 1, 0, 0, 0, 0, 2]);
        assert!((s.mean - 2.45 / 6.0).abs() < 1e-12);
        let empty = summarize(Metric::ScaledJacobian, &[]);
        assert_eq!((empty.min, empty.worst), (1.0, None));

        // Where higher is worse, the worst is the largest, and a cell at a
        // threshold counts as beyond it.
        let s = summarize(Metric::AspectRatio, &[1.0, 2.0, 12.0, 3.0]);
        assert_eq!(s.worst, Some(2));
        assert_eq!(s.beyond, [3, 2, 1, 1]);
        assert_eq!(s.histogram, [1, 0, 0, 1, 0, 1, 0, 0, 0, 1]);
        let empty = summarize(Metric::Skewness, &[]);
        assert_eq!((empty.min, empty.max, empty.worst), (0.0, 0.0, None));
    }
}
