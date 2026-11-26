//! The geometry, described for a reader that cannot look at it.
//!
//! [`gecko_geom::describe`] works out what each entity is; this writes it up
//! as the JSON the three reads hand back — `gecko.geometry()`,
//! `gecko.polyline(tag)` and `gecko.closest(point)` — over the model **as it
//! now stands**: a merge described as the one entity it is, and what it
//! swallowed left out, the way the page draws it.

use gecko_geom::{Composites, CurveShape, Geometry, Model, SurfaceShape, CORNER, CURVE, SURFACE};
use serde_json::{json, Value};

use crate::{name_in, On};

/// Every entity the model now has: where it is, how big it is, what it
/// touches and what shape it is.
pub fn geometry(geometry: &Geometry, composites: &Composites) -> Value {
    let model = Model::new(geometry, composites);
    let (lo, hi) = gecko_geom::describe::bounds_of(geometry.nodes());
    let corners = model.visible(CORNER);
    let curves = model.visible(CURVE);
    let surfaces = model.visible(SURFACE);

    // What contains what, among what is visible: a corner's curves, a
    // curve's surfaces — and, the other way, a surface's curves.
    let up = |dim: u8, tag: i32, to: u8, among: &[i32]| -> Vec<i32> {
        let mut tags: Vec<i32> = model
            .containing(dim, tag)
            .into_iter()
            .filter(|(d, t)| *d == to && among.contains(t))
            .map(|(_, t)| t)
            .collect();
        tags.sort_unstable();
        tags.dedup();
        tags
    };
    let curve_surfaces: Vec<(i32, Vec<i32>)> = curves
        .iter()
        .map(|&c| (c, up(CURVE, c, SURFACE, &surfaces)))
        .collect();

    json!({
        "bounds": { "min": lo, "max": hi },
        "span": model.span(),
        "corners": corners.iter().map(|&tag| json!({
            "tag": tag,
            "point": model.corner_at(tag),
            "curves": up(CORNER, tag, CURVE, &curves),
        })).collect::<Vec<_>>(),
        "curves": curve_surfaces.iter().filter_map(|(tag, on)| {
            let facts = model.curve_facts(*tag)?;
            let mut out = json!({
                "tag": tag,
                "length": facts.length,
                "bounds": { "min": facts.bounds.0, "max": facts.bounds.1 },
                "ends": model.ends_of(*tag),
                "closed": facts.closed,
                "runs": facts.chains.len(),
                "surfaces": on,
                "shape": curve_shape(&facts.shape),
            });
            named(&mut out, *tag, composites, |m| {
                model.curve_facts(m).map(|f| json!({
                    "tag": m, "length": f.length, "shape": curve_shape(&f.shape),
                }))
            });
            Some(out)
        }).collect::<Vec<_>>(),
        "surfaces": surfaces.iter().filter_map(|&tag| {
            let facts = model.surface_facts(tag)?;
            let bounding: Vec<i32> = curve_surfaces
                .iter()
                .filter(|(_, on)| on.contains(&tag))
                .map(|(c, _)| *c)
                .collect();
            let mut out = json!({
                "tag": tag,
                "area": facts.area,
                "bounds": { "min": facts.bounds.0, "max": facts.bounds.1 },
                "normal": tidy(&facts.normal),
                "curves": bounding,
                "shape": surface_shape(&facts.shape),
            });
            named(&mut out, tag, composites, |m| {
                model.surface_facts(m).map(|f| json!({
                    "tag": m, "area": f.area, "shape": surface_shape(&f.shape),
                }))
            });
            Some(out)
        }).collect::<Vec<_>>(),
    })
}

/// A curve's nodes in order, one list per run, or `null` when there is no
/// such curve.
pub fn polyline(geometry: &Geometry, composites: &Composites, tag: i32) -> Value {
    let model = Model::new(geometry, composites);
    match model.curve_facts(tag) {
        Some(facts) => json!({
            "tag": tag,
            "length": facts.length,
            "closed": facts.closed,
            "ends": model.ends_of(tag),
            "runs": facts.chains,
        }),
        None => Value::Null,
    }
}

/// The nearest point of the model to `point`: of one entity when `on` names
/// it, and otherwise the nearest corner, curve and surface, each with how
/// far away it is.
pub fn closest(
    geometry: &Geometry,
    composites: &Composites,
    point: [f64; 3],
    on: Option<(u8, i32)>,
) -> Value {
    let model = Model::new(geometry, composites);
    let found = |dim: u8, tag: i32| {
        model.project(dim, tag, point).map(|at| {
            json!({
                "dim": dim,
                "tag": tag,
                "point": at,
                "distance": apart(at, point),
            })
        })
    };
    if let Some((dim, tag)) = on {
        return found(dim, tag).unwrap_or(Value::Null);
    }
    let nearest = |dim: u8| {
        model
            .visible(dim)
            .into_iter()
            .filter_map(|tag| model.distance(dim, tag, point).map(|d| (d, tag)))
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .and_then(|(_, tag)| found(dim, tag))
            .unwrap_or(Value::Null)
    };
    json!({
        "corner": nearest(CORNER),
        "curve": nearest(CURVE),
        "surface": nearest(SURFACE),
    })
}

/// A merge says what it is made of, under the name the page gives it, and
/// what each piece is: two cylinders merged are no one shape, and the
/// pieces' are the facts a reader is after.
fn named(out: &mut Value, tag: i32, composites: &Composites, piece: impl Fn(i32) -> Option<Value>) {
    if let Some((dim, members)) = composites.members(tag) {
        out["members"] = json!(members);
        out["name"] = json!(name_in(On { dim, tag }, composites));
        out["pieces"] = json!(members.iter().filter_map(|&m| piece(m)).collect::<Vec<_>>());
    }
}

fn curve_shape(shape: &CurveShape) -> Value {
    match shape {
        CurveShape::Line { direction } => json!({ "kind": "line", "direction": tidy(direction) }),
        CurveShape::Circle {
            center,
            normal,
            radius,
            sweep,
        } => json!({
            "kind": "circle",
            "center": center,
            "normal": tidy(normal),
            "radius": radius,
            "sweep": sweep,
        }),
        CurveShape::Other => json!({ "kind": "other" }),
    }
}

fn surface_shape(shape: &SurfaceShape) -> Value {
    match shape {
        SurfaceShape::Plane { normal, offset } => {
            json!({ "kind": "plane", "normal": tidy(normal), "offset": offset })
        }
        SurfaceShape::Cylinder {
            point,
            axis,
            radius,
            sweep,
            length,
            convex,
        } => json!({
            "kind": "cylinder",
            "point": point,
            "axis": tidy(axis),
            "radius": radius,
            "sweep": sweep,
            "length": length,
            "convex": convex,
        }),
        SurfaceShape::Sphere {
            center,
            radius,
            convex,
        } => json!({ "kind": "sphere", "center": center, "radius": radius, "convex": convex }),
        SurfaceShape::Other => json!({ "kind": "other" }),
    }
}

/// A direction as written out: `-0` read back as `0`, which is what it is.
fn tidy(v: &[f64; 3]) -> [f64; 3] {
    v.map(|x| x + 0.0)
}

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
}
