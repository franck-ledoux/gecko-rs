//! A session, written back out as Lua.
//!
//! Every command is one line of script, and the mapping is a `match` over
//! the whole set — so a command added without a line here does not
//! compile. That is the only guarantee worth having: a history with a hole
//! in it is worse than no history, because it looks complete.
//!
//! The script is what a reader would have typed to arrive where the
//! session now is. It is not a recording of what they did: an undo takes
//! the line back off rather than writing `gecko.undo()`, so what comes out
//! is a recipe rather than a diary.

use crate::{Command, Spot};

/// The preamble a saved script needs to stand on its own.
pub const HEADER: &str = "\
-- Written by gecko. Run it with `gecko run <this file>`, or paste it into
-- the console. A geometry is named rather than carried, so a `.msh` it
-- loads has to be beside it.
";

/// The whole journal, as a script.
pub fn write(journal: &[Command]) -> String {
    let mut out = String::from(HEADER);
    for command in journal {
        out.push_str(&line(command));
        out.push('\n');
    }
    out
}

/// One command, as the line that would have made it.
pub fn line(command: &Command) -> String {
    match command {
        Command::AddBlock { corners } => {
            format!(
                "gecko.add_block{{{}}}",
                corners.iter().map(point).collect::<Vec<_>>().join(", ")
            )
        }
        Command::CreateCube { origin, size } => format!(
            "gecko.create_cube{{ origin = {}, size = {} }}",
            point(origin),
            number(*size)
        ),
        Command::MoveNode {
            node,
            to,
            carrying,
            snap,
        } => {
            let mut options = Vec::new();
            if !carrying.is_empty() {
                options.push(format!("carrying = {}", numbers(carrying)));
            }
            if (*snap - crate::SNAP_SHARE).abs() > f64::EPSILON {
                options.push(format!("snap = {}", number(*snap)));
            }
            let tail = if options.is_empty() {
                String::new()
            } else {
                format!(", {{ {} }}", options.join(", "))
            };
            format!("gecko.move_node({node}, {}{tail})", point(to))
        }
        Command::FaceOnSurface { surface } => format!("gecko.face_on_surface({surface})"),
        Command::RemoveFace { face } => format!("gecko.remove_face({face})"),
        Command::Smooth { rounds, share } => format!(
            "gecko.smooth{{ rounds = {rounds}, share = {} }}",
            number(*share)
        ),
        Command::ExtrudePatch { face, height } => {
            format!("gecko.extrude_patch({face}, {})", number(*height))
        }
        Command::AddFace { corners } => {
            format!(
                "gecko.add_face{{{}}}",
                corners.iter().map(point).collect::<Vec<_>>().join(", ")
            )
        }
        Command::GlueFaces { first, second } => format!("gecko.glue_faces({first}, {second})"),
        Command::RemoveChord { face, block } => {
            format!("gecko.remove_chord({face}, {block})")
        }
        Command::PillowChord {
            face,
            block,
            tolerance,
        } => format!("gecko.pillow_chord({face}, {block}, {{ tolerance = {tolerance} }})"),
        Command::RemoveBlock { block } => format!("gecko.remove_block({block})"),
        Command::RemoveSheet { edge } => format!("gecko.remove_sheet({edge})"),
        Command::SplitSheet { edge, at } => {
            format!("gecko.split_sheet({edge}, {})", number(*at))
        }
        Command::Pillow { faces, tolerance } => {
            let patch = faces
                .iter()
                .map(|f| format!("{{{}, {}}}", f.face, f.block))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "gecko.pillow({{{patch}}}{})",
                option("tolerance", *tolerance, crate::SNAP_SHARE)
            )
        }
        Command::ExtrudeFace {
            face,
            block,
            height,
        } => format!("gecko.extrude_face({face}, {block}, {})", number(*height)),
        // The contents are not written out — that would be the mesh in the
        // middle of the script rather than a script. The name is, and the
        // header says the file has to be beside it.
        Command::LoadGeometry { name, .. } => format!(
            "gecko.load_geometry({:?})",
            if name.is_empty() {
                "geometry.msh"
            } else {
                name
            }
        ),
        Command::ClearGeometry => "gecko.clear_geometry()".to_string(),
        Command::Classify { tolerance } => format!(
            "gecko.classify({})",
            braces(option("tolerance", *tolerance, crate::SNAP_SHARE))
        ),
        Command::ClassifyNode { nodes, on } => match on {
            Some(Spot { dim, tag }) => {
                format!("gecko.classify_node({}, {dim}, {tag})", numbers(nodes))
            }
            None => format!("gecko.unclassify_node({})", numbers(nodes)),
        },
        Command::FreezeNode { nodes, frozen } => format!(
            "gecko.{}({})",
            if *frozen { "freeze_node" } else { "thaw_node" },
            numbers(nodes)
        ),
        Command::BoxGeometry { surfaces, margin } => {
            let mut options = Vec::new();
            if !surfaces.is_empty() {
                options.push(format!("surfaces = {}", numbers(surfaces)));
            }
            if (*margin - crate::BOX_MARGIN).abs() > f64::EPSILON {
                options.push(format!("margin = {}", number(*margin)));
            }
            format!(
                "gecko.box_geometry({})",
                if options.is_empty() {
                    String::new()
                } else {
                    format!("{{ {} }}", options.join(", "))
                }
            )
        }
        Command::SetSegments { edge, segments } => {
            format!("gecko.set_segments({edge}, {segments})")
        }
        Command::Mesh { segments } => {
            format!("gecko.mesh{{ segments = {segments} }}")
        }
        Command::ClearMesh => "gecko.clear_mesh()".to_string(),
        // Neither of these is ever in a journal — the first takes a line
        // off it and the second is not a change to the model — but the
        // match is over the whole set on purpose, so that a command added
        // later cannot slip through without a line.
        Command::Undo => "gecko.undo()".to_string(),
        Command::SetHistory { steps } => format!("gecko.set_history({steps})"),
    }
}

/// A float, as Lua reads one. Rust's own shortest round-trip form, which
/// prints `1` for 1.0 — a number Lua is happy with either way.
fn number(x: f64) -> String {
    format!("{x}")
}

fn point(p: &[f64; 3]) -> String {
    format!("{{{}, {}, {}}}", number(p[0]), number(p[1]), number(p[2]))
}

fn numbers<T: std::fmt::Display>(all: &[T]) -> String {
    format!(
        "{{{}}}",
        all.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `key = value`, or nothing at all when it is the default the kernel
/// would have used anyway.
fn option(key: &str, value: f64, default: f64) -> String {
    if (value - default).abs() < f64::EPSILON {
        String::new()
    } else {
        format!("{key} = {}", number(value))
    }
}

fn braces(inner: String) -> String {
    if inner.is_empty() {
        String::new()
    } else {
        format!("{{ {inner} }}")
    }
}
