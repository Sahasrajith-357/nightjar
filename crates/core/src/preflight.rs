//! Preflight: run every pre-backup check in order and report the result.
//!
//! Hard-gate checks (rclone present, remote configured, remote reachable,
//! sources exist) abort with `Err` if they fail — there is no point
//! continuing. The space situation is NOT an error: a shortfall is a
//! decision point (triggers the partial-backup flow) and "unknown" means
//! the backend didn't report free space. Those are reported inside a
//! successful `PreflightReport` via `SpaceStatus`, leaving the decision to
//! the caller (CLI/GUI), keeping this engine non-interactive.

use crate::config::Config;
use crate::rclone;
use crate::Result;

/// The outcome of comparing the backup size against the destination's free
/// space.
#[derive(Debug, Clone, PartialEq)]
pub enum SpaceStatus {
    /// The backup fits in the available free space.
    Fits { free_bytes: u64 },
    /// The backup does not fit. Carries the shortfall context so the caller
    /// can show "you need X more" and offer the partial-backup flow.
    Shortfall { free_bytes: u64, needed_bytes: u64 },
    /// The destination did not report free space; fit cannot be predicted.
    Unknown,
}

/// The result of a successful preflight: everything the caller needs to
/// decide how to proceed.
#[derive(Debug, Clone, PartialEq)]
pub struct PreflightReport {
    /// Total estimated size of all sources, in bytes.
    pub backup_size_bytes: u64,
    /// The free-space verdict.
    pub space: SpaceStatus,
}

/// Pure decision logic: given the bytes needed and an optional free-space
/// figure, classify the space situation. No I/O — exhaustively testable.
fn decide_space_status(needed_bytes: u64, free_bytes: Option<u64>) -> SpaceStatus {
    match free_bytes {
        None => SpaceStatus::Unknown,
        Some(free) if needed_bytes <= free => SpaceStatus::Fits { free_bytes: free },
        Some(free) => SpaceStatus::Shortfall {
            free_bytes: free,
            needed_bytes,
        },
    }
}

/// Runs all preflight checks for the given configuration.
///
/// Hard gates (any failure returns Err and aborts):
///   1. rclone installed
///   2. remote configured
///   3. remote reachable
///   4. all source folders exist
/// Then computes the backup size and free space and returns a report whose
/// `space` field tells the caller whether it fits, falls short, or is
/// unknown.
pub fn preflight(config: &Config) -> Result<PreflightReport> {
    // --- Hard gates: abort on any failure ---
    rclone::check_installed()?;
    rclone::check_remote_configured(&config.remote)?;
    rclone::check_reachable(&config.remote)?;
    rclone::check_sources_exist(&config.sources)?;

    // --- Size estimate: sum every source ---
    let mut backup_size_bytes: u64 = 0;
    for source in &config.sources {
        let size = rclone::estimate_size(&source.path)?;
        backup_size_bytes = backup_size_bytes.saturating_add(size);
    }

    // --- Free space: may be unavailable (Unknown), which is not an error ---
    let free_bytes = match rclone::check_free_space(&config.remote) {
        Ok(free) => Some(free),
        Err(crate::error::Error::SpaceCheckFailed) => None,
        Err(other) => return Err(other),
    };

    let space = decide_space_status(backup_size_bytes, free_bytes);

    Ok(PreflightReport {
        backup_size_bytes,
        space,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_when_backup_smaller_than_free() {
        let status = decide_space_status(500, Some(1000));
        assert_eq!(status, SpaceStatus::Fits { free_bytes: 1000 });
    }

    #[test]
    fn fits_exactly_when_equal() {
        // Boundary: needed == free should still fit (<=).
        let status = decide_space_status(1000, Some(1000));
        assert_eq!(status, SpaceStatus::Fits { free_bytes: 1000 });
    }

    #[test]
    fn shortfall_when_backup_larger() {
        let status = decide_space_status(1500, Some(1000));
        assert_eq!(
            status,
            SpaceStatus::Shortfall {
                free_bytes: 1000,
                needed_bytes: 1500
            }
        );
    }

    #[test]
    fn shortfall_by_one_byte() {
        // Boundary: one byte over should be a shortfall.
        let status = decide_space_status(1001, Some(1000));
        assert_eq!(
            status,
            SpaceStatus::Shortfall {
                free_bytes: 1000,
                needed_bytes: 1001
            }
        );
    }

    #[test]
    fn unknown_when_free_space_unavailable() {
        let status = decide_space_status(500, None);
        assert_eq!(status, SpaceStatus::Unknown);
    }

    #[test]
    fn empty_backup_fits_in_any_space() {
        let status = decide_space_status(0, Some(0));
        assert_eq!(status, SpaceStatus::Fits { free_bytes: 0 });
    }
}
