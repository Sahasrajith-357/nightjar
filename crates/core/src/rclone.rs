//! All interaction with the external `rclone` binary.
//!
//! Every rclone invocation goes through `run()`, which captures stdout,
//! stderr, and the exit status, and turns failures into typed errors.
//! Locating rclone is centralized in `rclone_command()` so that a
//! configurable path could be added later in one place.

use crate::Result;
use crate::error::Error;
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::process::Command;
use std::process::Stdio;

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

/// Returns the names of all configured rclone remotes (trailing colons
/// stripped). Useful for letting a user pick among their remotes.
pub fn list_remotes() -> Result<Vec<String>> {
    let output = run(&["listremotes"])?;
    let remotes = output
        .stdout
        .lines()
        .map(|line| line.trim().trim_end_matches(':').to_string())
        .filter(|name| !name.is_empty())
        .collect();
    Ok(remotes)
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

/// Builds the exact argument list for an rclone copy of one source.
///
/// Pure function — no I/O, no rclone — so the command construction can be
/// tested exhaustively. Destination layout is `<remote>:<dest_path>/<name>`.
fn build_copy_args(
    source: &crate::config::Source,
    remote: &str,
    dest_path: &str,
    excludes: &[String],
) -> Vec<String> {
    let source_str = source.path.to_string_lossy().to_string();
    let dest = format!("{remote}:{dest_path}/{}", source.name);

    let mut args: Vec<String> = vec!["copy".to_string(), source_str, dest];
    for pattern in excludes {
        args.push("--exclude".to_string());
        args.push(pattern.clone());
    }
    args
}

/// Copies a single source folder to the destination remote.
///
/// Runs `rclone copy <source.path> <remote>:<dest_path>/<source.name>` with
/// the given excludes applied. `copy` is incremental: unchanged files are
/// skipped, so repeat backups only transfer new/changed data. Existing
/// destination files are never deleted (we use copy, not sync).
///
/// On failure, the error is interpreted: connectivity-like failures ->
/// NetworkUnavailable, an out-of-space signature -> StorageFull, otherwise
/// the original RcloneFailed propagates with its detail preserved.
pub fn copy_source(
    source: &crate::config::Source,
    remote: &str,
    dest_path: &str,
    excludes: &[String],
) -> Result<()> {
    let args = build_copy_args(source, remote, dest_path, excludes);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    match run(&arg_refs) {
        Ok(_) => Ok(()),
        Err(Error::RcloneFailed { message, code }) => {
            let lower = message.to_lowercase();
            if lower.contains("no such host")
                || lower.contains("network is unreachable")
                || lower.contains("connection refused")
                || lower.contains("timeout")
                || lower.contains("timed out")
                || lower.contains("dial tcp")
                || lower.contains("temporary failure in name resolution")
            {
                Err(Error::NetworkUnavailable)
            } else if lower.contains("quota")
                || lower.contains("storage full")
                || lower.contains("no space left")
                || lower.contains("insufficient storage")
                || lower.contains("limit exceeded")
            {
                Err(Error::StorageFull)
            } else {
                Err(Error::RcloneFailed { message, code })
            }
        }
        Err(other) => Err(other),
    }
}

/// Builds the argument list for verifying one source against the
/// destination. Pure — testable without running rclone.
///
/// Uses `check --one-way`: confirms every source file exists and matches
/// in the destination, while ignoring extra files in the destination (which
/// are expected, since backups are incremental and never delete).
fn build_verify_args(source: &crate::config::Source, remote: &str, dest_path: &str) -> Vec<String> {
    let source_str = source.path.to_string_lossy().to_string();
    let dest = format!("{remote}:{dest_path}/{}", source.name);
    vec![
        "check".to_string(),
        source_str,
        dest,
        "--one-way".to_string(),
    ]
}

/// Verifies that a source folder was backed up correctly.
///
/// Runs `rclone check --one-way`. Interprets the exit code specifically:
/// - exit 0          -> Ok(()): all source files present and matching
/// - exit non-zero   -> Err(VerificationFailed): differences were found
/// - failed to launch (binary missing, etc.) -> Err(RcloneNotFound / Io)
///
/// This does NOT use run(), because run() treats every non-zero exit as a
/// generic failure; for check, a non-zero exit is a meaningful *result*
/// (differences found), not a tool error.
pub fn verify_source(source: &crate::config::Source, remote: &str, dest_path: &str) -> Result<()> {
    let args = build_verify_args(source, remote, dest_path);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let result = rclone_command().args(&arg_refs).output();

    let output = match result {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::RcloneNotFound);
        }
        Err(e) => return Err(Error::Io(e)),
    };

    if output.status.success() {
        // Exit 0: verified.
        Ok(())
    } else {
        // Non-zero exit from check means differences were found. We surface
        // this as VerificationFailed rather than a generic rclone error.
        Err(Error::VerificationFailed)
    }
}

/// rclone's --use-json-log lines carry a "stats" object; we read transferred
/// vs total bytes from it to compute progress.
#[derive(Debug, Deserialize)]
struct StatsLine {
    stats: Option<StatsBody>,
}

#[derive(Debug, Deserialize)]
struct StatsBody {
    #[serde(default)]
    bytes: u64,
    #[serde(rename = "totalBytes", default)]
    total_bytes: u64,
}

/// Copies a single source like `copy_source`, but streams progress: rclone is
/// run with periodic JSON stats, and `on_progress` is called with a fraction
/// in 0.0..=1.0 as the transfer proceeds.
///
/// Progress is BEST-EFFORT and display-only: unparseable stats lines are
/// ignored, and progress reporting never affects the success/failure result.
/// The copy arguments are identical to `copy_source` (via build_copy_args),
/// with stats flags appended; verification is unchanged and handled elsewhere.
pub fn copy_source_streaming(
    source: &crate::config::Source,
    remote: &str,
    dest_path: &str,
    excludes: &[String],
    mut on_progress: impl FnMut(f32),
) -> Result<()> {
    let mut args = build_copy_args(source, remote, dest_path, excludes);
    // Append stats flags for periodic, parseable progress on stderr.
    args.push("--stats".to_string());
    args.push("200ms".to_string());
    args.push("--use-json-log".to_string());
    args.push("--stats-one-line".to_string());
    args.push("-v".to_string()); // stats JSON lines are INFO-level; -v enables them

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    // Spawn rclone with stderr piped so we can read stats as they stream.
    let mut child = match rclone_command()
        .args(&arg_refs)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::RcloneNotFound);
        }
        Err(e) => return Err(Error::Io(e)),
    };

    // Read stderr line by line, parsing JSON stats for progress.
    let mut captured_stderr = String::new();
    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            // Best-effort parse; ignore anything that isn't the stats JSON.
            if let Ok(parsed) = serde_json::from_str::<StatsLine>(&line) {
                if let Some(stats) = parsed.stats {
                    if stats.total_bytes > 0 {
                        let frac = (stats.bytes as f32 / stats.total_bytes as f32).clamp(0.0, 1.0);
                        on_progress(frac);
                    }
                }
            }
            // Keep the last bit of stderr for error reporting on failure.
            captured_stderr.push_str(&line);
            captured_stderr.push('\n');
        }
    }

    // Wait for rclone to finish and interpret the result.
    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => return Err(Error::Io(e)),
    };

    if status.success() {
        on_progress(1.0); // ensure the bar reaches full on success
        Ok(())
    } else {
        // Same heuristic error mapping as copy_source.
        let lower = captured_stderr.to_lowercase();
        if lower.contains("no such host")
            || lower.contains("network is unreachable")
            || lower.contains("connection refused")
            || lower.contains("timeout")
            || lower.contains("timed out")
            || lower.contains("dial tcp")
            || lower.contains("temporary failure in name resolution")
        {
            Err(Error::NetworkUnavailable)
        } else if lower.contains("quota")
            || lower.contains("storage full")
            || lower.contains("no space left")
            || lower.contains("insufficient storage")
            || lower.contains("limit exceeded")
        {
            Err(Error::StorageFull)
        } else {
            let code = status.code().unwrap_or(-1);
            let message = if captured_stderr.trim().is_empty() {
                "rclone exited with a non-zero status".to_string()
            } else {
                captured_stderr.trim().to_string()
            };
            Err(Error::RcloneFailed { code, message })
        }
    }
}

/// Launches rclone's own interactive configuration wizard in a terminal
/// emulator, so the user can connect a cloud account using rclone's proven
/// setup (browser OAuth and all). We deliberately do NOT drive rclone's
/// config ourselves — rclone manages its own config file, eliminating any
/// risk of corrupting existing remotes.
///
/// Tries a sequence of common terminal emulators and uses the first found.
/// Returns Ok(()) if a terminal was launched, or an error describing that
/// none could be found.
pub fn launch_guided_setup() -> Result<()> {
    // The command we want the terminal to run: rclone config, then keep the
    // window open so the user can read the outcome.
    let inner = "echo '--- nightjar: connect a cloud account ---'; \
                 echo 'Follow rclone'\\''s prompts. Your browser will open to sign in.'; \
                 echo; rclone config; echo; \
                 echo 'Setup finished. You can close this window and return to nightjar.'; \
                 exec bash";

    // Candidate terminal emulators: (program, args-before-command).
    // Each entry runs: <program> <args...> bash -c "<inner>"
    let candidates: [(&str, &[&str]); 6] = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-e"]),
        ("xterm", &["-e"]),
        ("kitty", &[]),
    ];

    for (term, pre_args) in candidates {
        // Check the terminal exists on PATH.
        let exists = std::process::Command::new("which")
            .arg(term)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !exists {
            continue;
        }

        let mut cmd = std::process::Command::new(term);
        cmd.args(pre_args);
        cmd.arg("bash").arg("-c").arg(inner);

        match cmd.spawn() {
            Ok(_) => return Ok(()),
            Err(_) => continue,
        }
    }

    Err(Error::ConfigError(
        "Could not find a terminal emulator to launch rclone setup. \
         Please open a terminal yourself and run: rclone config"
            .to_string(),
    ))
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

    #[test]
    fn copy_args_basic_destination_format() {
        let source = crate::config::Source {
            name: "Documents".to_string(),
            path: std::path::PathBuf::from("/home/user/Documents"),
        };
        let args = build_copy_args(&source, "cloud", "NightjarBackup", &[]);
        assert_eq!(
            args,
            vec![
                "copy".to_string(),
                "/home/user/Documents".to_string(),
                "cloud:NightjarBackup/Documents".to_string(),
            ]
        );
    }

    #[test]
    fn copy_args_appends_single_exclude() {
        let source = crate::config::Source {
            name: "Projects".to_string(),
            path: std::path::PathBuf::from("/home/user/Projects"),
        };
        let excludes = vec!["**/.git/**".to_string()];
        let args = build_copy_args(&source, "cloud", "Backup", &excludes);
        assert_eq!(
            args,
            vec![
                "copy".to_string(),
                "/home/user/Projects".to_string(),
                "cloud:Backup/Projects".to_string(),
                "--exclude".to_string(),
                "**/.git/**".to_string(),
            ]
        );
    }

    #[test]
    fn copy_args_appends_multiple_excludes_in_order() {
        let source = crate::config::Source {
            name: "Code".to_string(),
            path: std::path::PathBuf::from("/home/user/Code"),
        };
        let excludes = vec!["**/node_modules/**".to_string(), "**/target/**".to_string()];
        let args = build_copy_args(&source, "remote", "dest", &excludes);
        // Each exclude becomes a "--exclude" flag followed by its pattern,
        // preserving order.
        assert_eq!(
            args,
            vec![
                "copy".to_string(),
                "/home/user/Code".to_string(),
                "remote:dest/Code".to_string(),
                "--exclude".to_string(),
                "**/node_modules/**".to_string(),
                "--exclude".to_string(),
                "**/target/**".to_string(),
            ]
        );
    }

    #[test]
    #[ignore = "requires a real configured remote with network access"]
    fn copy_source_against_real_remote() {
        // Run manually: cargo test -- --ignored
        // Backs up a temp file to a real remote. Replace "cloud" and the
        // dest path as appropriate for your setup.
        use std::fs;
        let src_dir = tempfile::tempdir().expect("src dir");
        fs::write(src_dir.path().join("nightjar_test.txt"), b"test").expect("write");
        let source = crate::config::Source {
            name: "NightjarSelfTest".to_string(),
            path: src_dir.path().to_path_buf(),
        };
        copy_source(&source, "cloud", "NightjarBackup", &[]).expect("real copy should succeed");
    }

    #[test]
    fn verify_args_include_one_way() {
        let source = crate::config::Source {
            name: "Documents".to_string(),
            path: std::path::PathBuf::from("/home/user/Documents"),
        };
        let args = build_verify_args(&source, "cloud", "NightjarBackup");
        assert_eq!(
            args,
            vec![
                "check".to_string(),
                "/home/user/Documents".to_string(),
                "cloud:NightjarBackup/Documents".to_string(),
                "--one-way".to_string(),
            ]
        );
    }

    #[test]
    #[ignore = "requires a real configured remote with network access"]
    fn verify_source_against_real_remote() {
        // Run manually after a real backup: cargo test -- --ignored
        use std::fs;
        let src_dir = tempfile::tempdir().expect("src dir");
        fs::write(src_dir.path().join("nightjar_test.txt"), b"test").expect("write");
        let source = crate::config::Source {
            name: "NightjarSelfTest".to_string(),
            path: src_dir.path().to_path_buf(),
        };
        // First copy, then verify — should succeed.
        copy_source(&source, "cloud", "NightjarBackup", &[]).expect("copy should succeed");
        verify_source(&source, "cloud", "NightjarBackup")
            .expect("verify should succeed after a fresh copy");
    }

    #[test]
    #[ignore = "requires rclone with at least one configured remote"]
    fn list_remotes_includes_configured() {
        let remotes = list_remotes().expect("listremotes should work");
        // On the dev machine, "cloud" is configured.
        assert!(remotes.iter().any(|r| r == "cloud"));
    }

    #[test]
    #[ignore = "requires a real configured remote with network access"]
    fn copy_source_streaming_reports_progress_real() {
        use std::fs;
        use std::sync::{Arc, Mutex};

        // Run manually: cargo test -p nightjar-core -- --ignored
        let src_dir = tempfile::tempdir().expect("src dir");
        fs::write(src_dir.path().join("stream_test.bin"), vec![0u8; 8192]).expect("write");
        let source = crate::config::Source {
            name: "NightjarStreamTest".to_string(),
            path: src_dir.path().to_path_buf(),
        };

        let last = Arc::new(Mutex::new(-1.0f32));
        let last_cl = last.clone();
        let result = copy_source_streaming(&source, "cloud", "NightjarBackup", &[], move |f| {
            *last_cl.lock().unwrap() = f;
        });

        assert!(result.is_ok(), "streaming copy should succeed: {result:?}");
        // On success we force 1.0 at the end.
        assert_eq!(*last.lock().unwrap(), 1.0);
    }
}
