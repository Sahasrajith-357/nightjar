//! All interaction with the external `rclone` binary.
//!
//! Every rclone invocation goes through `run()`, which captures stdout,
//! stderr, and the exit status, and turns failures into typed errors.
//! Locating rclone is centralized in `rclone_command()` so that a
//! configurable path could be added later in one place.

use crate::error::Error;
use crate::Result;
use serde::Deserialize;
use std::process::Command;

/// The name of the rclone binary. We rely on it being on the system PATH
/// (Option A). Centralized here so an override could be added later.
const RCLONE_BIN: &str = "rclone";

/// Builds a `Command` for rclone. Centralizing this is what makes a future
/// "configurable rclone path" change a one-line edit.
fn rclone_command() -> Command {
    Command::new(RCLONE_BIN)
}

/// The captured result of running an rclone command.
#[derive(Debug)]
pub struct Output {
    /// Standard output, as a UTF-8 string (lossy: invalid bytes replaced).
    pub stdout: String,
    /// Standard error, as a UTF-8 string (lossy).
    pub stderr: String,
}

/// Runs rclone with the given arguments, capturing its output.
///
/// Returns `Ok(Output)` only if rclone launched AND exited successfully
/// (exit code 0). Failures are mapped to typed errors:
/// - rclone not found on PATH        -> Error::RcloneNotFound
/// - rclone ran but exited non-zero  -> Error::RcloneFailed { code, message }
/// - could not launch for other I/O  -> Error::Io
pub fn run(args: &[&str]) -> Result<Output> {
    let result = rclone_command().args(args).output();

    // Distinguish "rclone binary not found" from other launch failures.
    let output = match result {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::RcloneNotFound);
        }
        Err(e) => return Err(Error::Io(e)),
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if output.status.success() {
        Ok(Output { stdout, stderr })
    } else {
        // rclone ran but reported failure. Capture its exit code and stderr
        // so nothing is lost — the caller (or a more specific check) can
        // inspect this and refine it into a more precise error if needed.
        let code = output.status.code().unwrap_or(-1);
        let message = if stderr.trim().is_empty() {
            "rclone exited with a non-zero status".to_string()
        } else {
            stderr.trim().to_string()
        };
        Err(Error::RcloneFailed { code, message })
    }
}

/// Checks that rclone is installed and reachable on PATH by asking for its
/// version. Returns the version string (rclone's first output line) on
/// success, or Error::RcloneNotFound if the binary is missing.
pub fn check_installed() -> Result<String> {
    let output = run(&["version"])?;

    // rclone version prints e.g. "rclone v1.xx.x" on the first line.
    let first_line = output
        .stdout
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    Ok(first_line)
}

/// Checks that the given remote is configured in rclone.
///
/// Runs `rclone listremotes` (which prints one remote per line, each with a
/// trailing colon, e.g. "cloud:") and confirms `remote` is among them.
/// Returns Error::RcloneNotConfigured if it is not present.
pub fn check_remote_configured(remote: &str) -> Result<()> {
    let output = run(&["listremotes"])?;

    // Each line is a remote name, conventionally with a trailing ':'.
    // Normalize by stripping any trailing colon before comparing.
    let found = output.stdout.lines().any(|line| {
        let name = line.trim().trim_end_matches(':');
        name == remote
    });

    if found {
        Ok(())
    } else {
        Err(Error::RcloneNotConfigured {
            remote: remote.to_string(),
        })
    }
}

/// Checks that every configured source folder exists and is a directory.
///
/// Returns Error::SourceMissing with the offending path on the first
/// source that does not exist or is not a directory.
pub fn check_sources_exist(sources: &[crate::config::Source]) -> Result<()> {
    for source in sources {
        if !source.path.is_dir() {
            return Err(Error::SourceMissing {
                path: source.path.display().to_string(),
            });
        }
    }
    Ok(())
}

/// rclone's `about --json` output. We only need `free`; other fields are
/// optional because not all backends report every field.
#[derive(Debug, Deserialize)]
struct AboutJson {
    /// Free space in bytes. Absent on backends that don't report quota.
    free: Option<u64>,
}

/// Queries free space at the destination remote via `rclone about --json`.
///
/// Returns the number of free bytes. If the backend does not report free
/// space (the `free` field is absent), returns Error::SpaceCheckFailed so
/// the caller can decide how to proceed rather than guessing.
pub fn check_free_space(remote: &str) -> Result<u64> {
    let target = format!("{remote}:");
    let output = run(&["about", &target, "--json"])?;

    let about: AboutJson = serde_json::from_str(&output.stdout)
        .map_err(|e| Error::ConfigError(format!("could not parse rclone about output: {e}")))?;

    about.free.ok_or(Error::SpaceCheckFailed)
}

/// rclone's `size --json` output. We need the total `bytes`.
#[derive(Debug, Deserialize)]
struct SizeJson {
    /// Total size of the listed files, in bytes.
    bytes: u64,
}

/// Estimates the size in bytes of a local folder via `rclone size --json`.
///
/// Note: this is the *total* size of the folder, not the incremental
/// delta. The delta (what actually needs transferring) is smaller for
/// repeat backups; refining to the delta is a later improvement.
pub fn estimate_size(path: &std::path::Path) -> Result<u64> {
    let path_str = path.to_string_lossy();
    let output = run(&["size", &path_str, "--json"])?;

    let size: SizeJson = serde_json::from_str(&output.stdout)
        .map_err(|e| Error::ConfigError(format!("could not parse rclone size output: {e}")))?;

    Ok(size.bytes)
}

/// Checks that the remote is reachable by listing its top level.
///
/// Runs `rclone lsd <remote>: --max-depth 1`. Success means the remote
/// responded (reachable). On failure, we make a best-effort guess from
/// rclone's stderr: connectivity-like messages map to NetworkUnavailable,
/// anything else to DestinationUnreachable. The raw rclone error detail is
/// not discarded — a future refinement can inspect it further.
pub fn check_reachable(remote: &str) -> Result<()> {
    let target = format!("{remote}:");
    let result = run(&["lsd", &target, "--max-depth", "1"]);

    match result {
        Ok(_) => Ok(()),
        Err(Error::RcloneFailed { message, .. }) => {
            // Best-effort classification of the failure. Network-connectivity
            // failures from rclone typically mention these terms. This is a
            // heuristic, deliberately conservative.
            let lower = message.to_lowercase();
            let looks_like_network = lower.contains("no such host")
                || lower.contains("network is unreachable")
                || lower.contains("connection refused")
                || lower.contains("timeout")
                || lower.contains("timed out")
                || lower.contains("dial tcp")
                || lower.contains("temporary failure in name resolution");

            if looks_like_network {
                Err(Error::NetworkUnavailable)
            } else {
                Err(Error::DestinationUnreachable {
                    remote: remote.to_string(),
                })
            }
        }
        // RcloneNotFound, Io, or anything else propagates unchanged.
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Source;
    use std::path::PathBuf;

    #[test]
    fn rclone_is_installed() {
        let version = check_installed().expect("rclone should be installed");
        assert!(
            version.to_lowercase().contains("rclone"),
            "version line should mention rclone, got: {version}"
        );
    }

    #[test]
    fn sources_exist_passes_for_real_dirs() {
        // A temp dir definitely exists and is a directory.
        let dir = tempfile::tempdir().expect("temp dir");
        let sources = vec![Source {
            name: "Temp".to_string(),
            path: dir.path().to_path_buf(),
        }];
        assert!(check_sources_exist(&sources).is_ok());
    }

    #[test]
    fn sources_exist_fails_for_missing_dir() {
        let sources = vec![Source {
            name: "Ghost".to_string(),
            path: PathBuf::from("/this/path/does/not/exist"),
        }];
        let result = check_sources_exist(&sources);
        assert!(
            matches!(result, Err(Error::SourceMissing { .. })),
            "missing source should produce SourceMissing"
        );
    }

    #[test]
    fn unconfigured_remote_is_detected() {
        // A remote name almost certainly not configured on any machine.
        let result = check_remote_configured("definitely_not_a_real_remote_xyz");
        assert!(
            matches!(result, Err(Error::RcloneNotConfigured { .. })),
            "a bogus remote name should be reported as not configured"
        );
    }

    #[test]
    fn estimate_size_of_local_dir() {
        // Create a temp dir with a known-size file and size it via rclone.
        let dir = tempfile::tempdir().expect("temp dir");
        let file_path = dir.path().join("data.bin");
        std::fs::write(&file_path, vec![0u8; 1024]).expect("write file");

        let size = estimate_size(dir.path()).expect("estimate_size should work");
        assert_eq!(size, 1024, "1024-byte file should size to 1024 bytes");
    }

    #[test]
    #[ignore = "requires a real configured remote with network access"]
    fn check_free_space_against_real_remote() {
        // Run manually with: cargo test -- --ignored
        // Replace "cloud" with your actual remote name.
        let free = check_free_space("cloud").expect("free space query");
        assert!(free > 0, "a real remote should report some free space");
    }

    #[test]
    fn reachable_errors_on_bogus_remote() {
        // A nonexistent remote can't be reached; we just confirm it errors
        // cleanly (the exact variant depends on rclone's message).
        let result = check_reachable("definitely_not_a_real_remote_xyz");
        assert!(result.is_err(), "a bogus remote should not be reachable");
    }

    #[test]
    #[ignore = "requires a real configured remote with network access"]
    fn reachable_against_real_remote() {
        // Run manually: cargo test -- --ignored
        // Replace "cloud" with your real remote.
        check_reachable("cloud").expect("real remote should be reachable");
    }
}
