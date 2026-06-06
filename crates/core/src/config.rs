//! Nightjar's configuration: what to back up, where to, and how.
//!
//! The configuration is stored as a TOML file and loaded into these
//! strongly-typed structs. Every setting a user can change lives here.

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
