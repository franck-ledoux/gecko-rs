//! The block structure, written up for a reader that cannot look at it.
//!
//! The counts in `stats()` say how big a blocking is; they say nothing about
//! whether it is any good, or where it went wrong. A script that stopped half
//! way, or a mesh whose worst cell is in "block 1536", leaves a reader asking
//! the questions this answers:
//!
//! - **what there is**: every block with its eight corners in order, its
//!   middle and its own quality; every node, edge and face with what it is
//!   classified on; every sheet with the segments it is cut into;
//! - **what looks wrong**, before anything is meshed: a block turned inside
//!   out, a face on the outside of the blocking that follows nothing, a node
//!   off the entity it is said to be on — and, held against the geometry, a
//!   corner no node is on, a curve no edge runs along, a surface no face
//!   covers. That last kind is boundary alignment failing, said before the
//!   mesh is built rather than measured after it.
//!
//! A blocking is tens of blocks, so all of it is written out: the report is
//! read by a program, which would rather have the whole than a summary it
//! cannot see behind.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use gecko_core::quality::hex_quality;
use gecko_core::{Blocking, CellId, On, SEGMENTS};
use gecko_geom::{Composites, Geometry, Model, CORNER, CURVE, SURFACE};
use serde_json::{json, Value};

use crate::name_in;

/// A block worse than this, by the scaled Jacobian of its eight corners, is
/// named among the problems: a block that poor rarely meshes into cells any
/// better than it is.
pub const POOR_BLOCK: f64 = 0.2;

/// How far a node may be from what it is classified on, as a share of the
/// model's size, before that is a problem: classifying puts it there, so
/// anything more than rounding is a node something moved off.
pub const OFF_ITS_ENTITY: f64 = 1e-6;

/// A face as the walk over the blocks finds it: the blocks it bounds, its
/// four corners, and what it is classified on.
type FaceSeen = (Vec<u32>, [CellId; 4], Option<On>);

/// The whole block structure, and what looks wrong with it.
pub fn blocking(blocking: &Blocking, geometry: Option<(&Geometry, &Composites)>) -> Value {
    let model = geometry.map(|(g, c)| Model::new(g, c));
    let name = |on: On| match geometry {
        Some((_, composites)) => name_in(on, composites),
        None => format!("{} {}", on.dim, on.tag),
    };
    let spot = |on: Option<On>| on.map(|on| json!({ "dim": on.dim, "tag": on.tag }));
    let mut issues: Vec<Value> = Vec::new();

    // --- the faces, once each, and which are on the outside -----------------
    let patches: BTreeSet<CellId> = blocking.patches().into_iter().collect();
    let mut faces: BTreeMap<CellId, FaceSeen> = BTreeMap::new();
    for side in blocking.block_faces() {
        if patches.contains(&side.block) {
            continue;
        }
        let entry = faces
            .entry(side.face)
            .or_insert((Vec::new(), side.nodes, side.on));
        entry.0.extend(side.block.get());
    }
    let outer_nodes: BTreeSet<CellId> = faces
        .values()
        .filter(|(blocks, _, _)| blocks.len() == 1)
        .flat_map(|(_, nodes, _)| nodes.iter().copied())
        .collect();

    // --- nodes ------------------------------------------------------------------
    let span = model.map(|m| m.span()).unwrap_or(1.0);
    let mut held: BTreeSet<(u8, i32)> = BTreeSet::new();
    let mut hold = |on: On| {
        held.insert((on.dim, on.tag));
        if let Some((_, composites)) = geometry {
            if let Some((dim, members)) = composites.members(on.tag) {
                held.extend(members.iter().map(|m| (dim, *m)));
            }
        }
    };
    let nodes: Vec<Value> = blocking
        .nodes_in_full()
        .map(|(id, pos, on, frozen)| {
            let outer = outer_nodes.contains(&id);
            if let Some(on) = on {
                hold(on);
                if let Some(off) = model.as_ref().and_then(|m| m.distance(on.dim, on.tag, pos)) {
                    if off > OFF_ITS_ENTITY * span {
                        issues.push(json!({
                            "kind": "node_off_its_entity",
                            "node": id.get(),
                            "on": spot(Some(on)),
                            "distance": off,
                            "message": format!("node {} is {off:.3e} off {}, which it is classified on", id.get().unwrap_or(0), name(on)),
                        }));
                    }
                }
            } else if outer && !frozen && model.is_some() {
                issues.push(json!({
                    "kind": "outer_node_on_nothing",
                    "node": id.get(),
                    "pos": pos,
                    "message": format!("node {} is on the outside of the blocking and classified on nothing: it neither follows the geometry nor moves when smoothed", id.get().unwrap_or(0)),
                }));
            }
            json!({ "id": id.get(), "pos": pos, "on": spot(on), "frozen": frozen, "outer": outer })
        })
        .collect();

    // --- edges, and the sheets they make --------------------------------------
    let mut sheet_of: HashMap<CellId, usize> = HashMap::new();
    let mut sheets: Vec<Value> = Vec::new();
    let all_edges: Vec<(CellId, CellId, CellId, Option<On>)> = blocking.edges().collect();
    for (edge, _, _, _) in &all_edges {
        if sheet_of.contains_key(edge) {
            continue;
        }
        let Ok(sheet) = blocking.sheet_through(*edge) else {
            continue;
        };
        let index = sheets.len();
        for s in &sheet {
            sheet_of.insert(s.edge, index);
        }
        let segments = blocking.sheet_segments(*edge).unwrap_or(0);
        sheets.push(json!({
            "id": index,
            "segments": if segments == 0 { SEGMENTS } else { segments },
            "set": segments != 0,
            "blocks": blocking.sheet_blocks(&sheet),
            "edges": sheet.iter().filter_map(|s| s.edge.get()).collect::<Vec<_>>(),
        }));
    }
    let edges: Vec<Value> = all_edges
        .iter()
        .map(|(edge, a, b, on)| {
            if let Some(on) = on {
                hold(*on);
            }
            json!({
                "id": edge.get(),
                "nodes": [a.get(), b.get()],
                "on": spot(*on),
                "sheet": sheet_of.get(edge),
            })
        })
        .collect();

    // --- faces ------------------------------------------------------------------
    let classified: HashMap<CellId, Option<On>> = blocking
        .nodes_in_full()
        .map(|(id, _, on, _)| (id, on))
        .collect();
    let faces: Vec<Value> = faces
        .iter()
        .map(|(face, (blocks, corners, on))| {
            let outer = blocks.len() == 1;
            match on {
                Some(on) => hold(*on),
                None if outer && model.is_some() => {
                    // Why: a face is put on what all four of its corners
                    // share, so say what each is on — the one that shares
                    // nothing with the others is the one to look at.
                    let ons: Vec<Option<On>> = corners.iter().map(|n| classified.get(n).copied().flatten()).collect();
                    let said: Vec<String> = ons
                        .iter()
                        .map(|on| on.map_or_else(|| "nothing".to_string(), name))
                        .collect();
                    issues.push(json!({
                        "kind": "outer_face_on_nothing",
                        "face": face.get(),
                        "block": blocks.first(),
                        // One object a corner, so that a corner on nothing
                        // is an entry rather than a hole in the list.
                        "corners_on": corners
                            .iter()
                            .zip(&ons)
                            .map(|(n, on)| json!({ "node": n.get(), "on": spot(*on) }))
                            .collect::<Vec<_>>(),
                        "message": if ons.iter().all(Option::is_none) {
                            format!(
                                "face {} of block {} is on the outside of the blocking and classified on nothing, nor are its corners: its mesh will not follow the geometry",
                                face.get().unwrap_or(0),
                                blocks.first().copied().unwrap_or(0),
                            )
                        } else {
                            format!(
                                "face {} of block {} is on the outside of the blocking and classified on nothing, its corners being on {}, which no one surface contains: its mesh will not follow the geometry",
                                face.get().unwrap_or(0),
                                blocks.first().copied().unwrap_or(0),
                                said.join(", "),
                            )
                        },
                    }));
                }
                None => {}
            }
            json!({
                "id": face.get(),
                "blocks": blocks,
                "nodes": corners.iter().filter_map(|n| n.get()).collect::<Vec<_>>(),
                "on": spot(*on),
                "outer": outer,
            })
        })
        .collect();

    // --- blocks ------------------------------------------------------------------
    let mut worst: Option<(u32, f64)> = None;
    let mut inverted = 0;
    let blocks: Vec<Value> = blocking
        .blocks()
        .into_iter()
        .filter(|b| !patches.contains(b))
        .filter_map(|block| {
            let corners = blocking.hex_corners(block)?;
            let at = corners.map(|n| blocking.position(n).unwrap_or([0.0; 3]));
            let quality = hex_quality(&at);
            let id = block.get()?;
            if worst.is_none_or(|(_, q)| quality < q) {
                worst = Some((id, quality));
            }
            if quality < 0.0 {
                inverted += 1;
            }
            if quality < POOR_BLOCK {
                issues.push(json!({
                    "kind": if quality < 0.0 { "inverted_block" } else { "poor_block" },
                    "block": id,
                    "jacobian": quality,
                    "message": if quality < 0.0 {
                        format!("block {id} is turned inside out: its worst corner has a scaled Jacobian of {quality:.3}")
                    } else {
                        format!("block {id} is poor: its worst corner has a scaled Jacobian of {quality:.3}")
                    },
                }));
            }
            let center = at.iter().fold([0.0; 3], |s, p| [s[0] + p[0] / 8.0, s[1] + p[1] / 8.0, s[2] + p[2] / 8.0]);
            Some(json!({
                "id": id,
                "corners": corners.iter().filter_map(|n| n.get()).collect::<Vec<_>>(),
                "center": center,
                "jacobian": quality,
            }))
        })
        .collect();

    // --- the geometry nothing follows ---------------------------------------
    let mut unheld = json!(null);
    if let (Some(model), Some(_)) = (model.as_ref(), geometry) {
        let missing = |dim: u8| -> Vec<i32> {
            model
                .visible(dim)
                .into_iter()
                .filter(|t| !held.contains(&(dim, *t)))
                .collect()
        };
        let (corners, curves, surfaces) = (missing(CORNER), missing(CURVE), missing(SURFACE));
        // Only once there is a blocking to speak of: an empty session
        // follows nothing, and saying so twelve times helps nobody.
        if !blocks.is_empty() {
            for (dim, tags, kind, what) in [
                (CORNER, &corners, "corner_not_held", "no node is on it"),
                (
                    CURVE,
                    &curves,
                    "curve_not_followed",
                    "no edge runs along it",
                ),
                (
                    SURFACE,
                    &surfaces,
                    "surface_not_covered",
                    "no face is on it",
                ),
            ] {
                for tag in tags {
                    issues.push(json!({
                        "kind": kind,
                        "on": { "dim": dim, "tag": tag },
                        "message": format!("{}: {what}", name(On { dim, tag: *tag })),
                    }));
                }
            }
        }
        unheld = json!({ "corners": corners, "curves": curves, "surfaces": surfaces });
    }

    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for issue in &issues {
        if let Some(kind) = issue["kind"].as_str() {
            *kinds.entry(kind).or_default() += 1;
        }
    }
    json!({
        "summary": {
            "blocks": blocks.len(),
            "inverted_blocks": inverted,
            "worst_block": worst.map(|(id, q)| json!({ "block": id, "jacobian": q })),
            "outer_faces": faces.iter().filter(|f| f["outer"] == true).count(),
            "sheets": sheets.len(),
            "issues": kinds,
            "not_followed": unheld,
        },
        "issues": issues,
        "blocks": blocks,
        "nodes": nodes,
        "edges": edges,
        "faces": faces,
        "sheets": sheets,
    })
}
