//! A depth-capped, dirs-only walk of the backup sources, for displaying the
//! directory structure that would be backed up.

use crate::config::Source;
use std::path::Path;

/// One entry in the flattened directory tree (pre-order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Indentation depth (0 = a source root).
    pub depth: usize,
    /// The directory's display name.
    pub name: String,
    /// True if traversal stopped here due to the depth cap (more below).
    pub truncated: bool,
}

/// Directory names to skip — mirrors the spirit of preset_excludes so the
/// tree reflects what would actually be backed up.
fn is_excluded_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "target" | "__pycache__" | ".venv" | "venv" | ".cache"
    )
}

/// Maximum traversal depth below each source root.
const MAX_DEPTH: usize = 5;
/// Safety cap on total entries, so an enormous tree can't hang the UI.
const MAX_ENTRIES: usize = 5000;

/// Walks each source (directories only), returning a flat pre-order list.
pub fn build_tree(sources: &[Source]) -> Vec<TreeEntry> {
    let mut out = Vec::new();
    for src in sources {
        if out.len() >= MAX_ENTRIES {
            break;
        }
        // Source root at depth 0.
        out.push(TreeEntry {
            depth: 0,
            name: src.name.clone(),
            truncated: false,
        });
        walk_dir(&src.path, 1, &mut out);
    }
    out
}

fn walk_dir(dir: &Path, depth: usize, out: &mut Vec<TreeEntry>) {
    if depth > MAX_DEPTH || out.len() >= MAX_ENTRIES {
        return;
    }

    // Read and collect subdirectories, sorted by name for stable output.
    let mut subdirs: Vec<(String, std::path::PathBuf)> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // unreadable dir: skip silently
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if is_excluded_dir(&name) {
            continue;
        }
        subdirs.push((name, path));
    }
    subdirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    for (name, path) in subdirs {
        if out.len() >= MAX_ENTRIES {
            return;
        }
        let at_cap = depth == MAX_DEPTH;
        out.push(TreeEntry {
            depth,
            name,
            // Mark truncated if this dir has children we won't descend into.
            truncated: at_cap && dir_has_subdir(&path),
        });
        if !at_cap {
            walk_dir(&path, depth + 1, out);
        }
    }
}

/// Cheap check: does this directory contain at least one subdirectory?
fn dir_has_subdir(dir: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn walks_dirs_only_and_skips_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::create_dir(root.join("beta")).unwrap();
        fs::create_dir(root.join("node_modules")).unwrap(); // excluded
        fs::write(root.join("afile.txt"), b"x").unwrap(); // file, ignored
        fs::create_dir(root.join("alpha").join("nested")).unwrap();

        let sources = vec![Source {
            name: "Root".to_string(),
            path: root.to_path_buf(),
        }];
        let tree = build_tree(&sources);

        let names: Vec<&str> = tree.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Root"));
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(names.contains(&"nested"));
        assert!(!names.contains(&"node_modules"), "excluded dir must be skipped");
        assert!(!names.contains(&"afile.txt"), "files must be skipped");
        // Root is depth 0, alpha/beta depth 1, nested depth 2.
        let nested = tree.iter().find(|e| e.name == "nested").unwrap();
        assert_eq!(nested.depth, 2);
    }
}
