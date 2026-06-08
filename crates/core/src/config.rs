//! Nightjar's configuration: what to back up, where to, and how.
//!
//! The configuration is stored as a TOML file and loaded into these
//! strongly-typed structs. Every setting a user can change lives here.

use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The complete nightjar configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// The rclone remote to back up to (e.g. "cloud"), as configured in rclone.
    pub remote: String,

    /// The folder/path within the remote where backups are stored
    /// (e.g. "NightjarBackup").
    pub destination_path: String,

    /// The list of local folders to back up.
    pub sources: Vec<Source>,

    /// Whether to verify the backup after transferring (recommended: true).
    /// When true, "success" means transfer completed AND verification passed.
    #[serde(default = "default_true")]
    pub verify: bool,

    /// Glob patterns to exclude from every backup
    /// (e.g. "**/node_modules/**", "**/.git/**").
    #[serde(default)]
    pub excludes: Vec<String>,
}

/// A single local folder the user wants backed up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// A human-friendly label for this source (e.g. "Documents").
    pub name: String,

    /// The absolute path to the local folder.
    pub path: PathBuf,
}

/// serde helper: lets `verify` default to `true` when absent from the file.
fn default_true() -> bool {
    true
}

/// The exclude patterns the common-folders preset applies: version-control
/// internals, dependency/build dirs, caches, temp files, and OS cruft —
/// bulky or regenerable data that should not be backed up.
pub fn preset_excludes() -> Vec<String> {
    [
        "**/.git/**",
        "**/node_modules/**",
        "**/target/**",
        "**/__pycache__/**",
        "**/.venv/**",
        "**/venv/**",
        "**/.cache/**",
        "**/*.tmp",
        "**/*.temp",
        "**/Thumbs.db",
        "**/.DS_Store",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Builds the list of "common" user-data folders that actually exist on this
/// machine: Documents, Pictures, Music, Videos, Desktop, Downloads. Missing
/// folders are skipped. Each becomes a Source named after the folder.
pub fn preset_sources() -> Vec<Source> {
    let mut sources = Vec::new();
    let Some(dirs) = UserDirs::new() else {
        return sources; // can't resolve home; return empty
    };

    // (Optional dir, display name) pairs.
    let candidates: [(Option<&std::path::Path>, &str); 6] = [
        (dirs.document_dir(), "Documents"),
        (dirs.picture_dir(), "Pictures"),
        (dirs.audio_dir(), "Music"),
        (dirs.video_dir(), "Videos"),
        (dirs.desktop_dir(), "Desktop"),
        (dirs.download_dir(), "Downloads"),
    ];

    for (maybe_path, name) in candidates {
        if let Some(path) = maybe_path {
            if path.is_dir() {
                sources.push(Source {
                    name: name.to_string(),
                    path: path.to_path_buf(),
                });
            }
        }
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_excludes_are_nonempty_and_include_git() {
        let ex = preset_excludes();
        assert!(!ex.is_empty());
        assert!(ex.iter().any(|e| e.contains(".git")));
        assert!(ex.iter().any(|e| e.contains("node_modules")));
    }

    #[test]
    fn preset_sources_only_returns_existing_dirs() {
        // Every returned source must point at a real directory.
        for s in preset_sources() {
            assert!(s.path.is_dir(), "preset source must exist: {:?}", s.path);
        }
    }
}
