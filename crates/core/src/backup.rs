//! Backup orchestration: the operations the front-ends sequence to perform
//! an actual backup.
//!
//! Per the non-interactive-core design (Option A), this module exposes
//! operations rather than one monolithic run. The caller runs preflight
//! (in the `preflight` module), inspects the space status, prompts the user
//! if needed, and then calls `run_full_backup` or `run_partial_backup`.
//!
//! Robustness rules enforced here:
//! - Each source is copied THEN verified before moving to the next.
//! - The first failure of EITHER copy or verify stops everything and
//!   yields a Failed outcome — no later sources are touched.
//! - A verified outcome (FullVerified/PartialVerified) is produced ONLY if
//!   every source in the set both copied and verified. Partial success is
//!   never reported as success.

use crate::config::{Config, Source};
use crate::partial::SizedSource;
use crate::rclone;
use crate::state::BackupOutcome;

/// Copies and verifies one source, in that order. Returns Ok(()) only if
/// both succeed. The first failing step's error is converted to a string
/// for the outcome.
fn copy_and_verify_one(
    source: &Source,
    remote: &str,
    dest_path: &str,
    excludes: &[String],
) -> Result<(), String> {
    rclone::copy_source(source, remote, dest_path, excludes)
        .map_err(|e| format!("copy of '{}' failed: {e}", source.name))?;
    rclone::verify_source(source, remote, dest_path)
        .map_err(|e| format!("verification of '{}' failed: {e}", source.name))?;
    Ok(())
}

/// Runs a backup over the given set of sources, copying and verifying each
/// in turn and stopping on the first failure.
///
/// `full` selects which success outcome to report: a complete backup of all
/// configured sources -> FullVerified; a deliberately-chosen subset ->
/// PartialVerified. This is the single place that decides the success
/// variant, so the distinction can never be set inconsistently.
fn run_over_sources(sources: &[Source], config: &Config, full: bool) -> BackupOutcome {
    for source in sources {
        if let Err(message) = copy_and_verify_one(
            source,
            &config.remote,
            &config.destination_path,
            &config.excludes,
        ) {
            // First failure of copy OR verify: stop, report Failed.
            return BackupOutcome::Failed(message);
        }
    }

    // Every source copied and verified.
    if full {
        BackupOutcome::FullVerified
    } else {
        BackupOutcome::PartialVerified
    }
}

/// Performs a FULL backup: every configured source is copied and verified.
/// Stops on the first failure. Returns FullVerified only if all succeed.
pub fn run_full_backup(config: &Config) -> BackupOutcome {
    run_over_sources(&config.sources, config, true)
}

/// Performs a PARTIAL backup of the chosen subset (from the partial
/// selection logic). Copies and verifies each chosen source, stopping on
/// the first failure. Returns PartialVerified only if all chosen succeed.
pub fn run_partial_backup(config: &Config, selected: &[SizedSource]) -> BackupOutcome {
    // Extract the underlying Source from each SizedSource.
    let sources: Vec<Source> = selected.iter().map(|s| s.source.clone()).collect();
    run_over_sources(&sources, config, false)
}

/// Copies and verifies a single source, for front-ends that drive the
/// backup source-by-source (e.g. to show per-source progress).
///
/// Returns Ok(()) if this source both copied and verified; Err(message)
/// naming the source and failing step otherwise. This reuses the exact same
/// copy+verify path as the multi-source orchestrators, so behavior per
/// source is identical.
///
/// IMPORTANT: a caller driving sources individually is responsible for
/// replicating the orchestrator's contract — stop on the first Err, and
/// only treat the run as verified if EVERY source returned Ok.
pub fn backup_one_source(config: &Config, source: &Source) -> Result<(), String> {
    copy_and_verify_one(
        source,
        &config.remote,
        &config.destination_path,
        &config.excludes,
    )
}

/// Like `backup_one_source`, but streams copy progress via `on_progress`
/// (a fraction 0.0..=1.0 for the copy phase). Verification runs after the
/// copy exactly as in the non-streaming path. Same contract: Ok only if the
/// source both copied and verified; Err naming the failing step otherwise.
///
/// Progress is best-effort and display-only; it never affects the result.
pub fn backup_one_source_streaming(
    config: &Config,
    source: &Source,
    on_progress: impl FnMut(f32),
) -> Result<(), String> {
    // Copy with streaming progress.
    rclone::copy_source_streaming(
        source,
        &config.remote,
        &config.destination_path,
        &config.excludes,
        on_progress,
    )
    .map_err(|e| format!("copy of '{}' failed: {e}", source.name))?;

    // Verify exactly as the non-streaming path does.
    rclone::verify_source(source, &config.remote, &config.destination_path)
        .map_err(|e| format!("verification of '{}' failed: {e}", source.name))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Source;
    use std::path::PathBuf;

    // A config pointing at a real remote, for ignored integration tests.
    fn integration_config() -> Config {
        Config {
            remote: "cloud".to_string(),
            destination_path: "NightjarBackup".to_string(),
            sources: vec![],
            verify: true,
            excludes: vec![],
            theme: None,
        }
    }

    #[test]
    #[ignore = "requires a real configured remote with network access"]
    fn full_backup_of_temp_dir_succeeds() {
        use std::fs;
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("a.txt"), b"alpha").expect("write");

        let mut config = integration_config();
        config.sources = vec![Source {
            name: "NightjarSelfTest".to_string(),
            path: dir.path().to_path_buf(),
        }];

        let outcome = run_full_backup(&config);
        assert_eq!(outcome, BackupOutcome::FullVerified);
        // A verified full backup yields a power-off permit.
        assert!(outcome.power_off_permit().is_some());
    }

    #[test]
    fn missing_source_yields_failed_outcome() {
        // A nonexistent source: copy will fail, so the outcome must be
        // Failed — and crucially must NOT grant a power-off permit.
        let mut config = integration_config();
        config.sources = vec![Source {
            name: "Ghost".to_string(),
            path: PathBuf::from("/this/does/not/exist/at/all"),
        }];

        let outcome = run_full_backup(&config);
        assert!(
            matches!(outcome, BackupOutcome::Failed(_)),
            "missing source must produce Failed, got {outcome:?}"
        );
        // THE safety property: a failed backup grants no power-off permit.
        assert!(
            outcome.power_off_permit().is_none(),
            "a failed backup must never permit power-off"
        );
    }

    #[test]
    fn backup_one_source_missing_yields_err() {
        // A nonexistent source must Err (copy fails before needing a remote),
        // mirroring the orchestrator's failure handling for one source.
        let config = integration_config();
        let source = Source {
            name: "Ghost".to_string(),
            path: PathBuf::from("/this/does/not/exist/at/all"),
        };
        let result = backup_one_source(&config, &source);
        assert!(
            result.is_err(),
            "missing source must produce an error, got {result:?}"
        );
    }
}
