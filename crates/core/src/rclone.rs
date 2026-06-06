//! All interaction with the external `rclone` binary.
//!
//! Every rclone invocation goes through `run()`, which captures stdout,
//! stderr, and the exit status, and turns failures into typed errors.
//! Locating rclone is centralized in `rclone_command()` so that a
//! configurable path could be added later in one place.

use crate::error::Error;
use crate::Result;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rclone_is_installed() {
        // This test assumes rclone is installed on the test machine.
        let version = check_installed().expect("rclone should be installed");
        assert!(
            version.to_lowercase().contains("rclone"),
            "version line should mention rclone, got: {version}"
        );
    }
}
