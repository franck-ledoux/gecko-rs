//! A picture of the session, drawn without a browser.
//!
//! The page draws the model on a GPU. A script run from a terminal — a test
//! bench, an agent working through a set of parts — has no GPU and no page,
//! and still wants to *see* what it built: whether the blocks sit on the
//! part, where the bad cells are, which surface is which. So this draws the
//! same things in software, into an RGB picture, written out as PNG:
//!
//! - the **geometry**, its surfaces shaded, its curves and corners drawn;
//! - the **blocks**, each a colour of its own, with their edges;
//! - the **mesh**'s skin, coloured by block or by quality, with its grid;
//! - the cells **below a quality** threshold, drawn over everything wherever
//!   they are — most bad cells are inside the mesh, where the skin hides them;
//! - and **labels**: every entity's tag where it is, so that a reader of the
//!   picture can name what it sees — `S10`, `C-5`, `P12`, `B3`.
//!
//! The projection is orthographic, from a named view or any direction, framed
//! on what is drawn or on a ball of space the caller names. Shading is a
//! headlight; the picture is drawn at twice its size and halved, which is the
//! whole of the anti-aliasing. Nothing here is fast for its own sake: a
//! picture of a thirty-thousand-cell mesh takes a fraction of a second.

use gecko_core::{Blocking, Mesh};
use gecko_geom::{Composites, Geometry, Model, CORNER, CURVE, SURFACE};
use serde_json::Value;

use crate::view::ViewBuffer;

/// What to draw, and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub width: u32,
    pub height: u32,
    /// Towards the viewer from the scene.
    pub view: [f64; 3],
    /// Up on the picture; z unless the view is along z.
    pub up: Option<[f64; 3]>,
    /// Frame on this ball rather than on everything drawn.
    pub center: Option<[f64; 3]>,
    pub radius: Option<f64>,
    pub show: Show,
    pub mesh_color: Coloring,
    /// Cells whose scaled Jacobian is below this, drawn over everything.
    pub below: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Show {
    pub geometry: bool,
    pub curves: bool,
    pub corners: bool,
    pub blocks: bool,
    pub mesh: bool,
    pub labels: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coloring {
    Block,
    Quality,
}

/// The named views, each towards the viewer.
const VIEWS: [(&str, [f64; 3]); 7] = [
    ("iso", [1.0, -1.0, 0.8]),
    ("front", [0.0, -1.0, 0.0]),
    ("back", [0.0, 1.0, 0.0]),
    ("right", [1.0, 0.0, 0.0]),
    ("left", [-1.0, 0.0, 0.0]),
    ("top", [0.0, 0.0, 1.0]),
    ("bottom", [0.0, 0.0, -1.0]),
];

const SHOWABLE: [&str; 6] = ["geometry", "curves", "corners", "blocks", "mesh", "labels"];

/// The largest picture drawn, a side: enough for any screen, and a bound on
/// what a typo can ask for.
pub const LARGEST: u32 = 4096;

impl Options {
    /// Options from a script's table, defaults filled in from what there is
    /// to draw: the mesh when there is one, the blocks over a faded geometry
    /// when there are blocks, and the geometry, labelled, when that is all.
    pub fn parse(asked: &Value, has: Show) -> Result<Options, String> {
        let number = |key: &str| asked.get(key).and_then(Value::as_f64);
        let point = |key: &str| -> Result<Option<[f64; 3]>, String> {
            match asked.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => triple(v)
                    .map(Some)
                    .ok_or_else(|| format!("{key} is three numbers")),
            }
        };
        let side = |key: &str, default: u32| -> Result<u32, String> {
            match number(key) {
                None => Ok(default),
                Some(n) if n >= 16.0 && n <= f64::from(LARGEST) => Ok(n as u32),
                Some(n) => Err(format!(
                    "{key} {n}: a picture is 16 to {LARGEST} pixels a side"
                )),
            }
        };

        let view = match asked.get("view") {
            None | Some(Value::Null) => VIEWS[0].1,
            Some(Value::String(name)) => VIEWS
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| *v)
                .ok_or_else(|| {
                    let names: Vec<&str> = VIEWS.iter().map(|(n, _)| *n).collect();
                    format!(
                        "no view called {name:?}: there are {}, or a direction {{x, y, z}}",
                        names.join(", ")
                    )
                })?,
            Some(v) => triple(v).ok_or("view is a name or three numbers")?,
        };
        if norm(view) == 0.0 {
            return Err("view is a direction, and nought is none".into());
        }

        let show = match asked.get("show") {
            None | Some(Value::Null) => {
                let mut show = Show::default();
                if has.mesh {
                    show.mesh = true;
                    show.curves = has.geometry;
                } else if has.blocks {
                    show.blocks = true;
                    show.geometry = has.geometry;
                    show.curves = has.geometry;
                } else {
                    show.geometry = has.geometry;
                    show.curves = has.geometry;
                    show.corners = has.geometry;
                    show.labels = has.geometry;
                }
                show
            }
            Some(Value::Array(items)) => {
                let mut show = Show::default();
                for item in items {
                    match item.as_str() {
                        Some("geometry") => show.geometry = true,
                        Some("curves") => show.curves = true,
                        Some("corners") => show.corners = true,
                        Some("blocks") => show.blocks = true,
                        Some("mesh") => show.mesh = true,
                        Some("labels") => show.labels = true,
                        other => {
                            return Err(format!(
                                "show {}: what is shown is any of {}",
                                other.map_or_else(|| item.to_string(), |s| format!("{s:?}")),
                                SHOWABLE.join(", ")
                            ))
                        }
                    }
                }
                show
            }
            Some(_) => return Err(format!("show is a list of any of {}", SHOWABLE.join(", "))),
        };

        let mesh_color = match asked.get("color").and_then(Value::as_str) {
            None | Some("quality") => Coloring::Quality,
            Some("block") => Coloring::Block,
            Some(other) => {
                return Err(format!(
                    "color {other:?}: the mesh is colored by \"quality\" or \"block\""
                ))
            }
        };

        Ok(Options {
            width: side("width", 1200)?,
            height: side("height", 900)?,
            view,
            up: point("up")?,
            center: point("center")?,
            radius: number("radius").filter(|r| *r > 0.0),
            show,
            mesh_color,
            below: number("below"),
        })
    }
}

/// Everything there is to draw, by reference.
pub struct Scene<'a> {
    pub blocking: &'a Blocking,
    pub geometry: Option<&'a Geometry>,
    pub composites: &'a Composites,
    pub mesh: Option<&'a Mesh>,
}

/// A finished picture.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// What was actually drawn, which the defaults may have decided.
    pub shown: Vec<&'static str>,
    /// How many cells were drawn for being below the threshold.
    pub flagged: usize,
}

/// Supersampling: the picture is drawn this many times larger each way.
const SS: usize = 2;

pub fn render(scene: &Scene, options: &Options) -> Picture {
    let (w, h) = (options.width as usize * SS, options.height as usize * SS);
    let mut canvas = Canvas::new(w, h);
    let show = options.show;
    let model = scene.geometry.map(|g| Model::new(g, scene.composites));
    let blocks = (show.blocks || show.labels).then(|| ViewBuffer::of(scene.blocking, None));

    // --- what is where, in the world --------------------------------------
    let mut opaque: Vec<Tri> = Vec::new();
    let mut ghost: Vec<Tri> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    let mut dots: Vec<([f64; 3], [u8; 3])> = Vec::new();
    let mut labels: Vec<([f64; 3], String)> = Vec::new();
    let mut frame: Vec<[f64; 3]> = Vec::new();

    // The geometry is faded when there is something on it to look at.
    let faded = show.blocks || show.mesh;
    if let (Some(geometry), Some(model)) = (scene.geometry, model.as_ref()) {
        let nodes = geometry.nodes();
        if show.geometry {
            for tag in model.visible(SURFACE) {
                let color = if faded { [190, 200, 215] } else { palette(tag) };
                for t in model.triangles_of(tag) {
                    let tri = Tri {
                        p: t.map(|n| nodes[n as usize]),
                        color,
                    };
                    if faded {
                        ghost.push(tri)
                    } else {
                        opaque.push(tri)
                    }
                }
                if show.labels && !faded {
                    if let Some(at) = surface_anchor(model, tag) {
                        labels.push((at, format!("S{tag}")));
                    }
                }
            }
        }
        if show.curves {
            for tag in model.visible(CURVE) {
                for s in model.segments_of(tag) {
                    lines.push(Line {
                        a: nodes[s[0] as usize],
                        b: nodes[s[1] as usize],
                        color: [20, 40, 110],
                        width: 2.0,
                        alpha: 1.0,
                    });
                }
                if show.labels && !faded {
                    if let Some(facts) = model.curve_facts(tag) {
                        if let Some(run) = facts.chains.iter().max_by_key(|r| r.len()) {
                            labels.push((run[run.len() / 2], format!("C{tag}")));
                        }
                    }
                }
            }
        }
        if show.corners {
            for tag in model.visible(CORNER) {
                if let Some(at) = model.corner_at(tag) {
                    dots.push((at, [20, 40, 110]));
                    if show.labels && !faded {
                        labels.push((at, format!("P{tag}")));
                    }
                }
            }
        }
        frame.extend_from_slice(nodes);
    }

    if let Some(view) = blocks.as_ref() {
        let at = |k: u32| {
            let i = k as usize * 3;
            [
                f64::from(view.positions[i]) + view.center[0],
                f64::from(view.positions[i + 1]) + view.center[1],
                f64::from(view.positions[i + 2]) + view.center[2],
            ]
        };
        let mut centers: std::collections::BTreeMap<u32, ([f64; 3], f64)> = Default::default();
        for (q, block) in view.quads.chunks(4).zip(&view.quad_blocks) {
            let p = [at(q[0]), at(q[1]), at(q[2]), at(q[3])];
            if show.blocks && !show.mesh {
                let color = palette(*block as i32);
                opaque.push(Tri {
                    p: [p[0], p[1], p[2]],
                    color,
                });
                opaque.push(Tri {
                    p: [p[0], p[2], p[3]],
                    color,
                });
            }
            let c = centers.entry(*block).or_insert(([0.0; 3], 0.0));
            for corner in p {
                c.0 = plus(c.0, corner);
                c.1 += 1.0;
            }
            frame.extend_from_slice(&p);
        }
        if show.blocks {
            for e in view.edges.chunks(2) {
                lines.push(Line {
                    a: at(e[0]),
                    b: at(e[1]),
                    color: [10, 10, 10],
                    width: 2.5,
                    alpha: 1.0,
                });
            }
            if show.labels {
                for (block, (sum, n)) in &centers {
                    labels.push((scale(sum, 1.0 / n), format!("B{block}")));
                }
            }
        }
    }

    let mut flagged = 0;
    let mut overlay: Vec<Tri> = Vec::new();
    let mut overlay_lines: Vec<Line> = Vec::new();
    if let Some(mesh) = scene.mesh {
        let quality = mesh.quality();
        if show.mesh {
            let (quads, cells) = mesh.skin_cells();
            for (q, h) in quads.iter().zip(&cells) {
                let p = q.map(|n| mesh.points[n as usize]);
                let color = match options.mesh_color {
                    Coloring::Quality => quality_color(quality[*h]),
                    Coloring::Block => palette(mesh.blocks[*h].get().unwrap_or(0) as i32),
                };
                opaque.push(Tri {
                    p: [p[0], p[1], p[2]],
                    color,
                });
                opaque.push(Tri {
                    p: [p[0], p[2], p[3]],
                    color,
                });
                for k in 0..4 {
                    lines.push(Line {
                        a: p[k],
                        b: p[(k + 1) % 4],
                        color: [30, 30, 30],
                        width: 1.0,
                        alpha: 0.55,
                    });
                }
                frame.extend_from_slice(&p);
            }
        }
        if let Some(threshold) = options.below {
            const FACES: [[usize; 4]; 6] = [
                [0, 3, 2, 1],
                [4, 5, 6, 7],
                [0, 1, 5, 4],
                [1, 2, 6, 5],
                [2, 3, 7, 6],
                [3, 0, 4, 7],
            ];
            for (h, q) in quality.iter().enumerate().filter(|(_, q)| **q < threshold) {
                flagged += 1;
                let corners = mesh.hexes[h].map(|n| mesh.points[n as usize]);
                let color = if *q < 0.0 { [120, 0, 0] } else { [225, 30, 30] };
                for f in FACES {
                    let p = f.map(|c| corners[c]);
                    overlay.push(Tri {
                        p: [p[0], p[1], p[2]],
                        color,
                    });
                    overlay.push(Tri {
                        p: [p[0], p[2], p[3]],
                        color,
                    });
                    for k in 0..4 {
                        overlay_lines.push(Line {
                            a: p[k],
                            b: p[(k + 1) % 4],
                            color: [60, 0, 0],
                            width: 1.5,
                            alpha: 1.0,
                        });
                    }
                }
            }
        }
    }

    // --- the camera ---------------------------------------------------------
    let camera = Camera::new(options, &frame, w, h);

    // --- drawing ------------------------------------------------------------
    for t in &opaque {
        canvas.triangle(&camera, t, Layer::Opaque);
    }
    for t in &ghost {
        canvas.triangle(&camera, t, Layer::Ghost);
    }
    canvas.blend_ghost(0.35);
    let bias = camera.depth_span * 2e-3;
    for l in &lines {
        canvas.line(&camera, l, Some(bias));
    }
    for (p, color) in &dots {
        canvas.dot(&camera, *p, *color, 4.0, Some(bias));
    }
    // What is below the threshold is drawn over everything, nearest first.
    canvas.clear_overlay();
    for t in &overlay {
        canvas.triangle(&camera, t, Layer::Overlay);
    }
    canvas.blend_overlay(0.9);
    for l in &overlay_lines {
        canvas.line(&camera, l, None);
    }

    let mut picture = canvas.halve();
    // Labels last, at full size, where their anchor is not hidden.
    let mut taken: Vec<[i32; 4]> = Vec::new();
    for (at, text) in &labels {
        let (x, y, depth) = camera.project(*at);
        let (sx, sy) = (x as usize, y as usize);
        if sx >= w || sy >= h {
            continue;
        }
        // A block's label hangs at its middle, which its own faces hide;
        // the others are on what they name, and hidden when it is.
        let inside = text.starts_with('B');
        if !inside && depth > canvas.depth_at(sx, sy) + camera.depth_span * 0.01 {
            continue;
        }
        picture.label(
            (x / SS as f64) as i32,
            (y / SS as f64) as i32,
            text,
            &mut taken,
        );
    }

    let mut shown = Vec::new();
    for (on, name) in [
        (show.geometry, "geometry"),
        (show.curves, "curves"),
        (show.corners, "corners"),
        (show.blocks, "blocks"),
        (show.mesh, "mesh"),
        (show.labels, "labels"),
    ] {
        if on {
            shown.push(name);
        }
    }
    Picture {
        width: options.width,
        height: options.height,
        rgb: picture.rgb,
        shown,
        flagged,
    }
}

/// A point of the surface near its middle, to hang its label on: the middle
/// of a curved surface is often off it.
fn surface_anchor(model: &Model, tag: i32) -> Option<[f64; 3]> {
    let triangles = model.triangles_of(tag);
    let nodes = model.geometry.nodes();
    let centers: Vec<[f64; 3]> = triangles
        .iter()
        .map(|t| {
            scale(
                &plus(
                    plus(nodes[t[0] as usize], nodes[t[1] as usize]),
                    nodes[t[2] as usize],
                ),
                1.0 / 3.0,
            )
        })
        .collect();
    let n = centers.len().max(1) as f64;
    let middle = scale(&centers.iter().fold([0.0; 3], |s, c| plus(s, *c)), 1.0 / n);
    centers
        .into_iter()
        .min_by(|a, b| apart(*a, middle).total_cmp(&apart(*b, middle)))
}

// --- the camera ---------------------------------------------------------------

struct Camera {
    right: [f64; 3],
    up: [f64; 3],
    toward: [f64; 3],
    /// Screen x, y of the world origin, and pixels per unit.
    offset: [f64; 2],
    zoom: f64,
    depth_span: f64,
    height: f64,
}

impl Camera {
    fn new(options: &Options, frame: &[[f64; 3]], w: usize, h: usize) -> Camera {
        let toward = unit(options.view);
        let up_hint = options.up.unwrap_or(if toward[2].abs() > 0.95 {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        });
        let mut right = cross(up_hint, toward);
        if norm(right) < 1e-9 {
            right = cross([0.0, 1.0, 0.0], toward);
        }
        let right = unit(right);
        let up = cross(toward, right);

        let flat = |p: [f64; 3]| [dot(p, right), dot(p, up), dot(p, toward)];
        let (lo, hi) = match (options.center, options.radius) {
            (Some(c), Some(r)) => {
                let c = flat(c);
                (
                    [c[0] - r, c[1] - r, c[2] - r],
                    [c[0] + r, c[1] + r, c[2] + r],
                )
            }
            _ => {
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for p in frame {
                    let q = flat(*p);
                    for k in 0..3 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
                if !lo[0].is_finite() {
                    (lo, hi) = ([-1.0; 3], [1.0; 3]);
                }
                if let Some(c) = options.center {
                    // A center and no radius: the same size, moved there.
                    let c = flat(c);
                    let half = [(hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0];
                    lo[0] = c[0] - half[0];
                    hi[0] = c[0] + half[0];
                    lo[1] = c[1] - half[1];
                    hi[1] = c[1] + half[1];
                }
                (lo, hi)
            }
        };
        let margin = 0.06;
        let span = [(hi[0] - lo[0]).max(1e-9), (hi[1] - lo[1]).max(1e-9)];
        let zoom = ((w as f64) * (1.0 - 2.0 * margin) / span[0])
            .min((h as f64) * (1.0 - 2.0 * margin) / span[1]);
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        Camera {
            right,
            up,
            toward,
            offset: [
                w as f64 / 2.0 - mid[0] * zoom,
                h as f64 / 2.0 + mid[1] * zoom,
            ],
            zoom,
            depth_span: (hi[2] - lo[2]).abs().max(1e-9),
            height: h as f64,
        }
    }

    /// Pixel x, pixel y (down), and depth (smaller is nearer).
    fn project(&self, p: [f64; 3]) -> (f64, f64, f64) {
        let _ = self.height;
        (
            self.offset[0] + dot(p, self.right) * self.zoom,
            self.offset[1] - dot(p, self.up) * self.zoom,
            -dot(p, self.toward),
        )
    }
}

// --- the canvas -----------------------------------------------------------------

struct Tri {
    p: [[f64; 3]; 3],
    color: [u8; 3],
}

struct Line {
    a: [f64; 3],
    b: [f64; 3],
    color: [u8; 3],
    /// In pixels of the finished picture.
    width: f64,
    alpha: f64,
}

#[derive(Clone, Copy)]
enum Layer {
    Opaque,
    Ghost,
    Overlay,
}

struct Canvas {
    w: usize,
    h: usize,
    rgb: Vec<f32>,
    depth: Vec<f64>,
    /// The nearest translucent surface at each pixel, in front of what is
    /// opaque there.
    ghost: Vec<(f64, [f32; 3])>,
    overlay: Vec<(f64, [f32; 3])>,
}

const BACKGROUND: [f32; 3] = [248.0, 248.0, 250.0];

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas {
            w,
            h,
            rgb: BACKGROUND.repeat(w * h),
            depth: vec![f64::INFINITY; w * h],
            ghost: vec![(f64::INFINITY, [0.0; 3]); w * h],
            overlay: vec![(f64::INFINITY, [0.0; 3]); w * h],
        }
    }

    fn depth_at(&self, x: usize, y: usize) -> f64 {
        self.depth[y * self.w + x]
    }

    fn triangle(&mut self, camera: &Camera, t: &Tri, layer: Layer) {
        let n = cross(minus(t.p[1], t.p[0]), minus(t.p[2], t.p[0]));
        let Some(n) = unit_checked(n) else { return };
        // A headlight: what faces the viewer, either way round, is bright.
        let light = 0.35 + 0.65 * dot(n, camera.toward).abs();
        let color = t.color.map(|c| (f64::from(c) * light) as f32);
        let [a, b, c] = t.p.map(|p| camera.project(p));
        let area = edge(a, b, c.0, c.1);
        if area.abs() < 1e-12 {
            return;
        }
        let x0 = a.0.min(b.0).min(c.0).floor().max(0.0) as usize;
        let x1 = a.0.max(b.0).max(c.0).ceil().min(self.w as f64 - 1.0);
        let y0 = a.1.min(b.1).min(c.1).floor().max(0.0) as usize;
        let y1 = a.1.max(b.1).max(c.1).ceil().min(self.h as f64 - 1.0);
        if x1 < 0.0 || y1 < 0.0 {
            return;
        }
        for y in y0..=y1 as usize {
            for x in x0..=x1 as usize {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let (wa, wb, wc) = (
                    edge(b, c, px, py) / area,
                    edge(c, a, px, py) / area,
                    edge(a, b, px, py) / area,
                );
                if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                    continue;
                }
                let z = wa * a.2 + wb * b.2 + wc * c.2;
                let i = y * self.w + x;
                match layer {
                    Layer::Opaque => {
                        if z < self.depth[i] {
                            self.depth[i] = z;
                            self.rgb[i * 3..i * 3 + 3].copy_from_slice(&color);
                        }
                    }
                    Layer::Ghost => {
                        if z < self.depth[i] && z < self.ghost[i].0 {
                            self.ghost[i] = (z, color);
                        }
                    }
                    Layer::Overlay => {
                        if z < self.overlay[i].0 {
                            self.overlay[i] = (z, color);
                        }
                    }
                }
            }
        }
    }

    fn blend_ghost(&mut self, alpha: f32) {
        for i in 0..self.w * self.h {
            if self.ghost[i].0.is_finite() {
                mix(&mut self.rgb[i * 3..i * 3 + 3], self.ghost[i].1, alpha);
            }
        }
    }

    fn clear_overlay(&mut self) {
        self.overlay.iter_mut().for_each(|o| o.0 = f64::INFINITY);
    }

    fn blend_overlay(&mut self, alpha: f32) {
        for i in 0..self.w * self.h {
            if self.overlay[i].0.is_finite() {
                mix(&mut self.rgb[i * 3..i * 3 + 3], self.overlay[i].1, alpha);
                // So the outlines drawn next pass the depth test there.
                self.depth[i] = self.depth[i].min(self.overlay[i].0);
            }
        }
    }

    /// A line `width` finished pixels wide; hidden behind what is opaque
    /// unless `bias` is none.
    fn line(&mut self, camera: &Camera, l: &Line, bias: Option<f64>) {
        let a = camera.project(l.a);
        let b = camera.project(l.b);
        let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0) as usize;
        let half = (l.width * SS as f64 / 2.0).max(0.5);
        let reach = half.ceil() as i64;
        let color = l.color.map(f32::from);
        let alpha = l.alpha as f32;
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            let (x, y, z) = (
                a.0 + (b.0 - a.0) * t,
                a.1 + (b.1 - a.1) * t,
                a.2 + (b.2 - a.2) * t,
            );
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    if ((dx * dx + dy * dy) as f64).sqrt() > half {
                        continue;
                    }
                    let (px, py) = (x as i64 + dx, y as i64 + dy);
                    if px < 0 || py < 0 || px >= self.w as i64 || py >= self.h as i64 {
                        continue;
                    }
                    let i = py as usize * self.w + px as usize;
                    if let Some(bias) = bias {
                        if z > self.depth[i] + bias {
                            continue;
                        }
                    }
                    mix(&mut self.rgb[i * 3..i * 3 + 3], color, alpha);
                }
            }
        }
    }

    fn dot(
        &mut self,
        camera: &Camera,
        p: [f64; 3],
        color: [u8; 3],
        radius: f64,
        bias: Option<f64>,
    ) {
        let l = Line {
            a: p,
            b: p,
            color,
            width: radius * 2.0,
            alpha: 1.0,
        };
        self.line(camera, &l, bias);
    }

    /// Down to the finished size, each pixel the mean of the ones it covers.
    fn halve(&self) -> Image {
        let (w, h) = (self.w / SS, self.h / SS);
        let mut rgb = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                for k in 0..3 {
                    let mut sum = 0.0;
                    for dy in 0..SS {
                        for dx in 0..SS {
                            sum += self.rgb[((y * SS + dy) * self.w + x * SS + dx) * 3 + k];
                        }
                    }
                    rgb[(y * w + x) * 3 + k] =
                        (sum / (SS * SS) as f32).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Image { w, h, rgb }
    }
}

/// `alpha` of the way from what is there to `color`.
fn mix(pixel: &mut [f32], color: [f32; 3], alpha: f32) {
    for (old, new) in pixel.iter_mut().zip(color) {
        *old = *old * (1.0 - alpha) + new * alpha;
    }
}

fn edge(a: (f64, f64, f64), b: (f64, f64, f64), x: f64, y: f64) -> f64 {
    (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)
}

struct Image {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

impl Image {
    /// Text on a pale box, just above and right of the point it names, and
    /// not where another label already is.
    fn label(&mut self, x: i32, y: i32, text: &str, taken: &mut Vec<[i32; 4]>) {
        const SCALE: i32 = 2;
        let width = text.len() as i32 * 6 * SCALE + 2 * SCALE;
        let height = 7 * SCALE + 4 * SCALE;
        let (left, top) = (x + 3, y - height - 2);
        let me = [left, top, left + width, top + height];
        if taken
            .iter()
            .any(|o| me[0] < o[2] && o[0] < me[2] && me[1] < o[3] && o[1] < me[3])
        {
            return;
        }
        taken.push(me);
        for py in me[1]..me[3] {
            for px in me[0]..me[2] {
                self.blend(px, py, [255, 255, 255], 0.85);
            }
        }
        // A tick from the box to the point.
        for k in 0..3 {
            self.blend(x + k, y - k, [40, 40, 40], 1.0);
        }
        for (n, ch) in text.chars().enumerate() {
            let Some(rows) = glyph(ch) else { continue };
            let gx = left + SCALE + n as i32 * 6 * SCALE;
            let gy = top + 2 * SCALE;
            for (r, row) in rows.iter().enumerate() {
                for c in 0..5 {
                    if row >> (4 - c) & 1 == 1 {
                        for sy in 0..SCALE {
                            for sx in 0..SCALE {
                                self.blend(
                                    gx + c * SCALE + sx,
                                    gy + r as i32 * SCALE + sy,
                                    [20, 20, 20],
                                    1.0,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn blend(&mut self, x: i32, y: i32, color: [u8; 3], alpha: f32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 3;
        for (old, new) in self.rgb[i..i + 3].iter_mut().zip(color) {
            *old = (f32::from(*old) * (1.0 - alpha) + f32::from(new) * alpha).round() as u8;
        }
    }
}

/// Five by seven, a row a byte: the characters a label is made of.
fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch {
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        '-' => [0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        _ => return None,
    })
}

/// A colour of its own for each tag: hues a golden turn apart, so that
/// neighbouring tags are far apart on the wheel.
pub fn palette(tag: i32) -> [u8; 3] {
    let hue = (f64::from(tag) * 0.618_033_988_75 + 0.12).rem_euclid(1.0);
    hsv(hue, 0.45, 0.93)
}

/// The scaled Jacobian as a colour: green for a perfect cell, through
/// yellow and orange to red, and dark red for one turned inside out —
/// the page's own scale.
pub fn quality_color(q: f64) -> [u8; 3] {
    if q < 0.0 {
        return [130, 0, 0];
    }
    let stops: [(f64, [f64; 3]); 4] = [
        (0.0, [215.0, 35.0, 30.0]),
        (0.3, [240.0, 140.0, 40.0]),
        (0.6, [240.0, 215.0, 60.0]),
        (1.0, [70.0, 175.0, 80.0]),
    ];
    let q = q.min(1.0);
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if q <= b {
            let t = (q - a) / (b - a);
            return [0, 1, 2].map(|k| (ca[k] + (cb[k] - ca[k]) * t) as u8);
        }
    }
    [70, 175, 80]
}

fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b].map(|c| (c * 255.0).round() as u8)
}

fn triple(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Array(a) if a.len() == 3 => Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?]),
        Value::Object(o) => Some([
            o.get("x")?.as_f64()?,
            o.get("y")?.as_f64()?,
            o.get("z")?.as_f64()?,
        ]),
        _ => None,
    }
}

fn plus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: &[f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
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
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    scale(&a, 1.0 / norm(a))
}
fn unit_checked(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = norm(a);
    (n > 0.0).then(|| scale(&a, 1.0 / n))
}
fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(minus(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cube() -> Blocking {
        let mut b = Blocking::new().expect("a blocking");
        b.add_block(gecko_core::cube_corners([0.0; 3], 1.0))
            .expect("a cube");
        b
    }

    fn pixel(p: &Picture, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * p.width + x) * 3) as usize;
        [p.rgb[i], p.rgb[i + 1], p.rgb[i + 2]]
    }

    #[test]
    fn options_default_to_what_there_is() {
        let blocks_only = Show {
            blocks: true,
            ..Show::default()
        };
        let o = Options::parse(&json!({}), blocks_only).unwrap();
        assert_eq!((o.width, o.height), (1200, 900));
        assert!(o.show.blocks && !o.show.mesh && !o.show.labels);
        let geometry_only = Show {
            geometry: true,
            ..Show::default()
        };
        let o = Options::parse(&Value::Null, geometry_only).unwrap();
        assert!(o.show.geometry && o.show.labels && o.show.curves);

        let o = Options::parse(
            &json!({ "view": "top", "width": 320, "height": 200, "show": ["mesh", "labels"], "color": "block", "below": 0.3 }),
            Show::default(),
        )
        .unwrap();
        assert_eq!(o.view, [0.0, 0.0, 1.0]);
        assert!(o.show.mesh && o.show.labels && !o.show.blocks);
        assert_eq!(o.mesh_color, Coloring::Block);
        assert_eq!(o.below, Some(0.3));
        let o = Options::parse(
            &json!({ "view": { "x": 1, "y": 2, "z": 3 }, "center": [0, 0, 0], "radius": 2 }),
            Show::default(),
        )
        .unwrap();
        assert_eq!(o.view, [1.0, 2.0, 3.0]);
        assert_eq!((o.center, o.radius), (Some([0.0; 3]), Some(2.0)));
    }

    #[test]
    fn a_bad_option_says_what_would_do() {
        let none = Show::default();
        assert!(Options::parse(&json!({ "view": "sideways" }), none)
            .unwrap_err()
            .contains("iso, front"));
        assert!(Options::parse(&json!({ "show": ["blocs"] }), none)
            .unwrap_err()
            .contains("blocks"));
        assert!(Options::parse(&json!({ "width": 100000 }), none)
            .unwrap_err()
            .contains("4096"));
        assert!(Options::parse(&json!({ "view": [0, 0, 0] }), none)
            .unwrap_err()
            .contains("nought"));
        assert!(Options::parse(&json!({ "color": "pink" }), none)
            .unwrap_err()
            .contains("quality"));
    }

    #[test]
    fn a_cube_from_the_front_fills_the_middle() {
        let blocking = cube();
        let composites = Composites::new();
        let scene = Scene {
            blocking: &blocking,
            geometry: None,
            composites: &composites,
            mesh: None,
        };
        let options = Options::parse(
            &json!({ "view": "front", "width": 200, "height": 100 }),
            Show {
                blocks: true,
                ..Show::default()
            },
        )
        .unwrap();
        let p = render(&scene, &options);
        assert_eq!(p.rgb.len(), 200 * 100 * 3);
        assert_eq!(p.shown, vec!["blocks"]);
        // The middle is the cube's face, in its colour, lit head on.
        let face = palette(scene.blocking.block_faces()[0].block.get().unwrap() as i32);
        let mid = pixel(&p, 100, 50);
        for k in 0..3 {
            assert!(
                (i32::from(mid[k]) - i32::from(face[k])).abs() <= 2,
                "{mid:?} against {face:?}"
            );
        }
        // A corner of the picture is background: the cube is framed, not cropped.
        assert_eq!(pixel(&p, 2, 2), [248, 248, 250]);
        // Its outline is drawn: somewhere along the middle row the edge is dark.
        assert!((0..200).any(|x| pixel(&p, x, 50).iter().all(|c| *c < 80)));
    }

    #[test]
    fn bad_cells_are_drawn_over_everything() {
        let mut blocking = cube();
        // Fold one corner of the cube in until its cells turn inside out.
        let corner = blocking
            .nodes_in_full()
            .find(|n| n.1 == [1.0, 1.0, 1.0])
            .unwrap()
            .0;
        blocking
            .set_position(corner, [-0.2, -0.2, -0.2])
            .expect("the corner moved");
        let mesh = blocking.mesh(2, |_, _| None);
        let composites = Composites::new();
        let scene = Scene {
            blocking: &blocking,
            geometry: None,
            composites: &composites,
            mesh: Some(&mesh),
        };
        let options = Options::parse(
            &json!({ "width": 200, "height": 200, "below": 0.0 }),
            Show {
                mesh: true,
                blocks: true,
                ..Show::default()
            },
        )
        .unwrap();
        let p = render(&scene, &options);
        assert!(p.flagged > 0, "some cell is inside out");
        // Dark red is somewhere on the picture.
        let reds = p
            .rgb
            .chunks(3)
            .filter(|c| c[0] > 90 && c[1] < 40 && c[2] < 40)
            .count();
        assert!(reds > 0);
    }

    #[test]
    fn the_quality_scale_runs_red_to_green() {
        assert_eq!(quality_color(-0.5), [130, 0, 0]);
        assert_eq!(quality_color(1.0), [70, 175, 80]);
        let mid = quality_color(0.45);
        assert!(mid[0] > 200 && mid[1] > 150, "{mid:?}");
    }
}
