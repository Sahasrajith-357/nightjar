//! Every way a nightjar backup can fail.
//!
//! This single enum enumerates all failure modes. Because functions return
//! `Result<T, Error>`, the compiler forces every caller to handle or
//! propagate each failure — nothing can be silently ignored.

use thiserror::Error;

/// All errors that the nightjar core engine can produce.
#[derive(Debug, Error)]
pub enum Error {
    /// The `rclone` binary could not be found on the system PATH.
    #[error("rclone is not installed or not on your PATH")]
    RcloneNotFound,

    /// rclone exists, but the requested remote/destination is not configured.
    #[error("rclone has no remote named '{remote}' configured")]
    RcloneNotConfigured { remote: String },

    /// A source folder the user asked to back up does not exist.
    #[error("source folder does not exist: {path}")]
    SourceMissing { path: String },

    /// The cloud destination could not be reached.
    #[error("could not reach the backup destination '{remote}'")]
    DestinationUnreachable { remote: String },

    /// No network connection is available.
    #[error("no network connection is available")]
    NetworkUnavailable,

    /// Preflight determined the full backup will not fit in the free space.
    /// This is a decision point that triggers the partial-backup flow,
    /// not necessarily a fatal error.
    #[error("not enough space: backup needs {needed_bytes} bytes but only {free_bytes} are free")]
    InsufficientSpace { needed_bytes: u64, free_bytes: u64 },

    /// The amount of free space at the destination could not be determined.
    #[error("could not determine free space at the destination")]
    SpaceCheckFailed,

    /// The destination ran out of space during the transfer (runtime safety net).
    #[error("the backup destination is full")]
    StorageFull,

    /// The transfer began but did not finish (e.g. the connection dropped).
    #[error("the transfer was interrupted before completing")]
    TransferInterrupted,

    /// The transfer finished, but verification found the backup did not match.
    #[error("verification failed: the backed-up data does not match the source")]
    VerificationFailed,

    /// The user cancelled the backup.
    #[error("the backup was cancelled")]
    Cancelled,

    /// The configuration file is missing, unreadable, or invalid.
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// A permission problem (reading a file, or performing power-off).
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// An rclone failure we don't have a more specific variant for.
    /// Carries rclone's exit code and captured message so nothing is lost.
    #[error("rclone failed (exit code {code}): {message}")]
    RcloneFailed { code: i32, message: String },

    /// An underlying I/O error from the operating system.
    /// `#[from]` lets `?` convert a std::io::Error into this automatically.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
