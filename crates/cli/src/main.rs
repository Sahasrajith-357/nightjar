//! nightjar command-line interface.
//!
//! Subcommands:
//!   backup     - run a backup (full, or partial if space is short)
//!   preflight  - run all pre-backup checks without transferring anything

use clap::{Parser, Subcommand};

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

    match cli.command {
        Command::Backup {
            power_off,
            partial_method,
            yes,
        } => {
            // Stub for now — implemented in 6-iv.
            println!("backup: power_off={power_off}, partial_method={partial_method:?}, yes={yes}");
        }
        Command::Preflight => {
            // Stub for now — implemented in 6-ii.
            println!("preflight: (not yet implemented)");
        }
    }
}
