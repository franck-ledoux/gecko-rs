//! What a smoothing minimizes: how far each block is from a cube.
//!
//! Two terms per cell, each of them one for a perfect cell and growing
//! from there, and neither of them changed by scaling the cell:
//!
//! - **Orthogonality**, corner by corner: `|Â⁻¹|²/3` for the three unit
//!   edges `Â` leaving the corner. One at a right-angled corner, and
//!   infinite as the corner folds flat — which is what keeps a cell from
//!   being turned inside out on the way to anywhere.
//! - **Spacing**: the mean squared edge over the volume to the power two
//!   thirds.
//!   Right angles alone say nothing about where a straight cut through a
//!   row of blocks sits — every place makes the same rectangles — and this
//!   does: two blocks stacked between fixed ends cost least at the same
//!   height.
//!
//! Nothing here but the four operations and square roots, which every
//! platform rounds the same way: the page and the terminal have to land on
//! the same bytes, and a cube root from two maths libraries need not — so
//! the one cube root is [`cube_root`], worked out here.
//!
//! Scale-free on purpose. A plain spring energy pulls every edge shorter,
//! and a blocking whose boundary may slide over a surface would shrink to
//! a point keeping every angle square; here there is nothing to gain by
//! shrinking.
//!
//! A quadrangle of a patch is measured the same way, one dimension down.

/// How much the corners weigh against the spacing. One each: a cube
/// scores one on both.
pub const ORTHOGONALITY: f64 = 1.0;

/// The scaled Jacobian no corner is let fall below by a smoothing, unless
/// it was below it already — in which case it is not let fall further.
pub const FLOOR: f64 = 0.2;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(a, a).sqrt();
    (n > 1e-300).then(|| a.map(|x| x / n))
}

/// One corner of a hexahedron: its orthogonality term, and its scaled
/// Jacobian signed by the block's handedness — positive while the corner
/// is the right way out.
pub fn hex_corner(at: [f64; 3], sides: [[f64; 3]; 3], hand: f64) -> (f64, f64) {
    let (Some(u), Some(v), Some(w)) = (
        unit(sub(sides[0], at)),
        unit(sub(sides[1], at)),
        unit(sub(sides[2], at)),
    ) else {
        return (f64::INFINITY, 0.0);
    };
    let (vw, wu, uv) = (cross(v, w), cross(w, u), cross(u, v));
    let det = dot(u, vw) * hand;
    if det.abs() < 1e-300 {
        return (f64::INFINITY, 0.0);
    }
    let phi = (dot(vw, vw) + dot(wu, wu) + dot(uv, uv)) / (3.0 * det * det);
    (phi, det)
}

/// A hexahedron's energy, and its worst signed scaled Jacobian. Corners
/// numbered as [`crate::HEX_CORNER_EDGES`] numbers them.
pub fn hex(at: &[[f64; 3]; 8], hand: f64) -> (f64, f64) {
    let mut corners = 0.0;
    let mut worst = f64::INFINITY;
    let mut volume = 0.0;
    for (c, sides) in crate::HEX_CORNER_EDGES.iter().enumerate() {
        let (phi, det) = hex_corner(at[c], sides.map(|s| at[s]), hand);
        corners += phi;
        worst = worst.min(det);
        let e = sides.map(|s| sub(at[s], at[c]));
        volume += dot(e[0], cross(e[1], e[2])) * hand / 8.0;
    }
    let squares: f64 = crate::HEX_EDGES
        .iter()
        .map(|[a, b]| {
            let e = sub(at[*b], at[*a]);
            dot(e, e)
        })
        .sum();
    let spacing = if volume > 0.0 {
        squares / 12.0 / (cube_root(volume) * cube_root(volume))
    } else {
        f64::INFINITY
    };
    (spacing + ORTHOGONALITY * corners / 8.0, worst)
}

/// A quadrangle of a patch: its energy, and the worst sine of a corner
/// measured against the normal it was laid with — negative once it has
/// folded over.
pub fn quad(at: &[[f64; 3]; 4], normal: [f64; 3]) -> (f64, f64) {
    let mut corners = 0.0;
    let mut worst = f64::INFINITY;
    let mut area = 0.0;
    let mut squares = 0.0;
    for k in 0..4 {
        let (u, v) = (sub(at[(k + 1) % 4], at[k]), sub(at[(k + 3) % 4], at[k]));
        squares += dot(u, u);
        let n = cross(u, v);
        let lengths = (dot(u, u) * dot(v, v)).sqrt();
        let sine = if lengths > 1e-300 {
            dot(n, normal) / lengths
        } else {
            0.0
        };
        worst = worst.min(sine);
        corners += if sine.abs() > 1e-300 {
            1.0 / (sine * sine)
        } else {
            f64::INFINITY
        };
        area += dot(n, normal) / 4.0;
    }
    let spacing = if area > 0.0 {
        squares / 4.0 / area
    } else {
        f64::INFINITY
    };
    (spacing + ORTHOGONALITY * corners / 4.0, worst)
}

/// Which way a hexahedron's corners turn: the sign of its volume as
/// numbered, so that a block numbered left-handed is not taken for one
/// turned inside out.
pub fn handedness(at: &[[f64; 3]; 8]) -> f64 {
    let mut volume = 0.0;
    for (c, sides) in crate::HEX_CORNER_EDGES.iter().enumerate() {
        let e = sides.map(|s| sub(at[s], at[c]));
        volume += dot(e[0], cross(e[1], e[2]));
    }
    if volume < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// The normal a quadrangle was laid with: the sum of its corners' cross
/// products, made unit.
pub fn normal_of(at: &[[f64; 3]; 4]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for k in 0..4 {
        let c = cross(sub(at[(k + 1) % 4], at[k]), sub(at[(k + 3) % 4], at[k]));
        for i in 0..3 {
            n[i] += c[i];
        }
    }
    unit(n).unwrap_or([0.0, 0.0, 1.0])
}

/// The cube root of a positive number, the same to the last bit on every
/// platform: a first guess read off the exponent, then Newton's iteration,
/// which uses nothing but the four operations.
pub fn cube_root(x: f64) -> f64 {
    if x <= 0.0 || !x.is_finite() {
        return 0.0;
    }
    // A third of the exponent, roughly, by integer arithmetic on the bits.
    let bits = x.to_bits();
    let mut y = f64::from_bits(bits / 3 + 0x2A9F_7893_782D_A1CE);
    for _ in 0..8 {
        y = (2.0 * y + x / (y * y)) / 3.0;
    }
    y
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_cube_root_is_a_cube_root() {
        for x in [1e-12, 0.001, 0.5, 1.0, 2.0, 8.0, 27.0, 1000.0, 3.7e9] {
            let y = super::cube_root(x);
            assert!((y * y * y - x).abs() <= 1e-12 * x, "{x}: {y}");
        }
        assert_eq!(super::cube_root(8.0), 2.0);
    }
}
