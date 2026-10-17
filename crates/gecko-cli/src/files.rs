//! The files the page is allowed to touch.
//!
//! A page served from anywhere has none: a browser cannot open
//! `/home/you/cylinder.msh` by name, and should not be able to. A page
//! served from **here** can, because here is your machine and you asked.
//!
//! Everything is under one root — the directory `gecko` was started in —
//! and a path that climbs out of it is refused. Not because a local server
//! is a serious attack surface, but because "serve a directory" and "serve
//! the whole disk" are different promises and only one of them was made.

use std::path::{Component, Path, PathBuf};

pub struct Files {
    root: PathBuf,
}

impl Files {
    pub fn here() -> std::io::Result<Files> {
        Ok(Files {
            root: std::env::current_dir()?,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where this path lands, or nothing if it lands outside the root.
    ///
    /// An absolute path is taken as given — you typed it, into a console
    /// on your own machine, and the server it reaches is bound to this
    /// machine alone. A relative one is resolved under the root, and one
    /// that climbs out of the root with `..` is refused: "serve this
    /// folder" and "serve the disk" are different promises, and only the
    /// first was made to a path nobody wrote out in full.
    ///
    /// On Windows, "written out in full" means a drive: `C:\tmp\x` is
    /// absolute and `/tmp/x` is not — it names the root of *some* drive,
    /// and which one would be a guess. So that one is refused rather than
    /// resolved under the root, where it would mean something different
    /// again.
    ///
    /// Walked component by component rather than handed to the
    /// filesystem, so it answers the same for a file that does not exist
    /// yet — which every write is.
    pub fn at(&self, path: &str) -> Option<PathBuf> {
        let given = Path::new(path);
        if given.is_absolute() {
            return Some(given.to_path_buf());
        }
        let mut out = self.root.clone();
        for part in given.components() {
            match part {
                Component::Normal(name) => out.push(name),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() || !out.starts_with(&self.root) {
                        return None;
                    }
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Some(out)
    }

    pub fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.at(path)?).ok()
    }

    pub fn write(&self, path: &str, text: &str) -> Result<(), String> {
        let at = self
            .at(path)
            .ok_or_else(|| format!("{path}: outside the folder gecko was started in"))?;
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&at, text).map_err(|e| format!("{}: {e}", at.display()))
    }
}
