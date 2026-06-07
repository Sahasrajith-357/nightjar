//! nightjar command-line interface.
//!
//! Subcommands:
//!   backup     - run a backup (full, or partial if space is short)
//!   preflight  - run all pre-backup checks without transferring anything

use clap::{Parser, Subcommand};
use nightjar_core::config::Config;
use nightjar_core::config_io;
use nightjar_core::preflight::{self, SpaceStatus};
mod prompts;

/// nightjar: a robust backup tool that runs while you sleep.
#[derive(Parser, Debug)]
#[command(name = "nightjar", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a backup. Without flags, prompts interactively for any decisions.
    Backup {
        /// Power off the machine after a successful backup.
        #[arg(long)]
        power_off: bool,

        /// If space is short, choose how to select files without prompting:
        /// "smallest-first" or "custom".
        #[arg(long, value_name = "METHOD")]
        partial_method: Option<String>,

        /// Assume "yes" to confirmation prompts (for unattended runs).
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Run all pre-backup checks and report, without transferring anything.
    Preflight,
}

fn main() {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Command::Backup {
            power_off,
            partial_method,
            yes,
        } => {
            println!("backup: power_off={power_off}, partial_method={partial_method:?}, yes={yes}");
            0
        }
        Command::Preflight => run_preflight(),
    };

    std::process::exit(exit_code);
}

/// Loads the configuration, printing a helpful message and returning None if
/// it cannot be found or read.
fn load_config() -> Option<Config> {
    let path = match config_io::config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: could not determine the config location: {e}");
            return None;
        }
    };

    match config_io::load_from(&path) {
        Ok(config) => Some(config),
        Err(e) => {
            eprintln!("Error: could not load configuration: {e}");
            eprintln!();
            eprintln!("Expected a config file at:");
            eprintln!("  {}", path.display());
            eprintln!();
            eprintln!("It should look like:");
            eprintln!("  remote = \"cloud\"");
            eprintln!("  destination_path = \"NightjarBackup\"");
            eprintln!("  verify = true");
            eprintln!("  excludes = [\"**/.git/**\"]");
            eprintln!();
            eprintln!("  [[sources]]");
            eprintln!("  name = \"Documents\"");
            eprintln!("  path = \"/home/you/Documents\"");
            None
        }
    }
}

/// Formats a byte count as a human-readable string (e.g. "1.5 GiB").
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.2} {}", UNITS[unit])
    }
}

/// Runs the preflight checks and prints a report. Returns a process exit
/// code: 0 on success (checks passed), non-zero on a hard-gate failure.
fn run_preflight() -> i32 {
    let config = match load_config() {
        Some(c) => c,
        None => return 1,
    };

    println!("Running preflight checks for remote '{}'...", config.remote);

    match preflight::preflight(&config) {
        Ok(report) => {
            println!();
            println!("All checks passed.");
            println!("  Backup size:  {}", human_bytes(report.backup_size_bytes));
            match report.space {
                SpaceStatus::Fits { free_bytes } => {
                    println!("  Free space:   {}", human_bytes(free_bytes));
                    println!("  Verdict:      fits — a full backup can proceed.");
                }
                SpaceStatus::Shortfall {
                    free_bytes,
                    needed_bytes,
                } => {
                    println!("  Free space:   {}", human_bytes(free_bytes));
                    println!(
                        "  Verdict:      shortfall — need {} but only {} free.",
                        human_bytes(needed_bytes),
                        human_bytes(free_bytes)
                    );
                    println!("                A partial backup would be offered.");
                }
                SpaceStatus::Unknown => {
                    println!("  Free space:   unknown (the remote did not report it)");
                    println!("  Verdict:      cannot verify space; backup may still proceed.");
                }
            }
            0
        }
        Err(e) => {
            eprintln!();
            eprintln!("Preflight failed: {e}");
            1
        }
    }
}
