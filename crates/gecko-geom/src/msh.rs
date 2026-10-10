//! Reading a gmsh MSH 2.2 ASCII file.
//!
//! Only the sections a faceted geometry is made of are kept: the nodes, the
//! point/line/triangle elements, and the physical names. Everything else in
//! the file is skipped — counted, so the caller can say so, but skipped.
//!
//! **Version 2.2, ASCII.** Which is what the C++ GECKO reads, and what every
//! `.msh` in its `test_data` is. Modern gmsh writes 4.1 by default, where the
//! same information lives in a different place entirely — the entity tags move
//! out of the elements and into a `$Entities` section — so a 4.1 file is
//! refused by name rather than misread: half of a 4.1 `$Nodes` line is a tag
//! and half is a coordinate, and nothing about reading it as 2.2 would fail
//! loudly.
//!
//! This reader does no interpreting. Which elements make which geometric
//! entity is [`crate::model`]'s business; this is the file, as it stands.

use std::collections::HashMap;
use thiserror::Error;

/// Gmsh element type for a 2-node line. A curve is made of these.
pub const LINE: i32 = 1;
/// Gmsh element type for a 3-node triangle. A surface is made of these.
pub const TRIANGLE: i32 = 2;
/// Gmsh element type for a 1-node point. A geometric corner is one of these,
/// read rather than inferred: which node is a corner is a statement about the
/// model, and no amount of looking at triangles recovers it.
pub const POINT: i32 = 15;

#[derive(Debug, Error)]
pub enum MshError {
    #[error("line {line}: {what}")]
    Malformed { line: usize, what: String },
    #[error("not a gmsh mesh: the file has no $MeshFormat section")]
    NoMeshFormat,
    #[error(
        "MSH version {0} is not supported — only 2.x is. \
         Re-export it from gmsh with `-format msh2`."
    )]
    Version(String),
    #[error("binary MSH files are not supported — re-export it from gmsh without `-bin`")]
    Binary,
    #[error("element {element} refers to node {node}, which the file never defines")]
    UnknownNode { element: i64, node: i64 },
}

/// One entry of `$PhysicalNames`: a name gmsh gives to a group of entities.
///
/// Kept, and deliberately not used for grouping — see [`crate::model`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalName {
    pub dimension: i32,
    pub tag: i32,
    pub name: String,
}

/// One element of the kinds this reader keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Element {
    /// [`POINT`], [`LINE`] or [`TRIANGLE`].
    pub kind: i32,
    /// The first element tag: which `$PhysicalNames` group it belongs to, or 0.
    pub physical: i32,
    /// The second element tag: the **elementary entity** it is classified on,
    /// or 0 for none. This is what a geometric entity is grouped by.
    pub entity: i32,
    /// Indices into [`Msh::nodes`] — not the numbers written in the file,
    /// which are arbitrary and need not be contiguous.
    ///
    /// Only the first [`node_count`](Element::node_count) are meaningful.
    pub nodes: [u32; 3],
}

impl Element {
    pub fn node_count(&self) -> usize {
        match self.kind {
            POINT => 1,
            LINE => 2,
            _ => 3,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Msh {
    /// Positions, in the order the file lists them.
    pub nodes: Vec<[f64; 3]>,
    pub elements: Vec<Element>,
    pub names: Vec<PhysicalName>,
    /// How many `$Elements` entries were of a kind this reader does not keep.
    ///
    /// Tetrahedra, almost always: a gmsh file of a meshed CAD model carries
    /// the volume mesh and its boundary together, and a faceted geometry wants
    /// only the boundary. Reported rather than silently dropped, because "I
    /// loaded your 20 000 tetrahedra and ignored them" is worth one line.
    pub skipped: usize,
}

/// Read a gmsh MSH 2.2 ASCII file.
pub fn read(text: &str) -> Result<Msh, MshError> {
    // `str::lines` already drops a trailing `\r`, so a file written on Windows
    // needs nothing special here.
    let lines: Vec<&str> = text.lines().collect();
    let mut msh = Msh::default();
    let mut numbers: HashMap<i64, u32> = HashMap::new();
    let mut seen_format = false;
    let mut at = 0;

    // Sections, in whatever order they come. A gmsh file lists $MeshFormat
    // first and $Nodes before $Elements, but nothing in the format says it
    // must, and the one thing that really is ordered — an element naming a
    // node — is caught by the lookup below rather than assumed.
    while at < lines.len() {
        let line = lines[at].trim();
        at += 1;
        match line {
            "$MeshFormat" => {
                let (head, next) = take(&lines, at, "$MeshFormat")?;
                at = next;
                let mut words = head.split_whitespace();
                let version = words.next().unwrap_or_default();
                if !version.starts_with('2') {
                    return Err(MshError::Version(version.to_string()));
                }
                if words.next() == Some("1") {
                    return Err(MshError::Binary);
                }
                seen_format = true;
            }
            "$PhysicalNames" => at = read_names(&lines, at, &mut msh)?,
            "$Nodes" => at = read_nodes(&lines, at, &mut msh, &mut numbers)?,
            "$Elements" => at = read_elements(&lines, at, &mut msh, &numbers)?,
            _ => {}
        }
    }

    if !seen_format {
        return Err(MshError::NoMeshFormat);
    }
    Ok(msh)
}

/// The next line, and where to carry on from.
fn take<'a>(lines: &[&'a str], at: usize, section: &str) -> Result<(&'a str, usize), MshError> {
    match lines.get(at) {
        Some(line) => Ok((line.trim(), at + 1)),
        None => Err(MshError::Malformed {
            line: at,
            what: format!("the {section} section is truncated"),
        }),
    }
}

fn count(lines: &[&str], at: usize, section: &str) -> Result<(usize, usize), MshError> {
    let (head, next) = take(lines, at, section)?;
    let count = head.parse::<usize>().map_err(|_| MshError::Malformed {
        line: next,
        what: format!("{section} should start with a count, not {head:?}"),
    })?;
    Ok((count, next))
}

fn read_names(lines: &[&str], at: usize, msh: &mut Msh) -> Result<usize, MshError> {
    let (count, mut at) = count(lines, at, "$PhysicalNames")?;
    for _ in 0..count {
        let (line, next) = take(lines, at, "$PhysicalNames")?;
        at = next;
        // `dim tag "a name with spaces"`: two numbers and then the rest, which
        // is why this is not a `split_whitespace` of three.
        let mut words = line.splitn(3, char::is_whitespace);
        let dimension = number(words.next(), at, "a physical name's dimension")?;
        let tag = number(words.next(), at, "a physical name's tag")?;
        let name = words.next().unwrap_or_default().trim();
        msh.names.push(PhysicalName {
            dimension: dimension as i32,
            tag: tag as i32,
            name: name.trim_matches('"').to_string(),
        });
    }
    Ok(at)
}

fn read_nodes(
    lines: &[&str],
    at: usize,
    msh: &mut Msh,
    numbers: &mut HashMap<i64, u32>,
) -> Result<usize, MshError> {
    let (count, mut at) = count(lines, at, "$Nodes")?;
    msh.nodes.reserve(count);
    for _ in 0..count {
        let (line, next) = take(lines, at, "$Nodes")?;
        at = next;
        let mut words = line.split_whitespace();
        let number = number(words.next(), at, "a node number")?;
        let mut p = [0.0; 3];
        for (k, axis) in p.iter_mut().enumerate() {
            let word = words.next().ok_or_else(|| MshError::Malformed {
                line: at,
                what: format!("node {number} has no coordinate {k}"),
            })?;
            *axis = word.parse::<f64>().map_err(|_| MshError::Malformed {
                line: at,
                what: format!("node {number}: {word:?} is not a coordinate"),
            })?;
        }
        // The file's own numbering is arbitrary and need not be contiguous —
        // a mesh with parts deleted routinely has gaps — so it is a key, not
        // an index. A number used twice keeps the later node, which is what
        // reading the file top to bottom means.
        numbers.insert(number, msh.nodes.len() as u32);
        msh.nodes.push(p);
    }
    Ok(at)
}

fn read_elements(
    lines: &[&str],
    at: usize,
    msh: &mut Msh,
    numbers: &HashMap<i64, u32>,
) -> Result<usize, MshError> {
    let (count, mut at) = count(lines, at, "$Elements")?;
    for _ in 0..count {
        let (line, next) = take(lines, at, "$Elements")?;
        at = next;
        let mut words = line.split_whitespace();
        let element = number(words.next(), at, "an element number")?;
        let kind = number(words.next(), at, "an element type")? as i32;
        let tags = number(words.next(), at, "an element's tag count")?;

        // Tag 0 is the physical group, tag 1 the elementary entity. Anything
        // beyond — partition ids — is read past and dropped.
        let mut physical = 0;
        let mut entity = 0;
        for t in 0..tags {
            let tag = number(words.next(), at, "an element tag")? as i32;
            match t {
                0 => physical = tag,
                1 => entity = tag,
                _ => {}
            }
        }

        if !matches!(kind, POINT | LINE | TRIANGLE) {
            msh.skipped += 1;
            continue;
        }

        let mut nodes = [0; 3];
        let wanted = match kind {
            POINT => 1,
            LINE => 2,
            _ => 3,
        };
        for slot in nodes.iter_mut().take(wanted) {
            let number = number(words.next(), at, "an element's node")?;
            *slot = *numbers.get(&number).ok_or(MshError::UnknownNode {
                element,
                node: number,
            })?;
        }
        msh.elements.push(Element {
            kind,
            physical,
            entity,
            nodes,
        });
    }
    Ok(at)
}

fn number(word: Option<&str>, line: usize, what: &str) -> Result<i64, MshError> {
    let word = word.ok_or_else(|| MshError::Malformed {
        line,
        what: format!("{what} is missing"),
    })?;
    word.parse::<i64>().map_err(|_| MshError::Malformed {
        line,
        what: format!("{what}: {word:?} is not a whole number"),
    })
}
