//! nightjar command-line interface.
//!
//! Subcommands:
//!   backup     - run a backup (full, or partial if space is short)
//!   preflight  - run all pre-backup checks without transferring anything

mod prompts;

use clap::{Parser, Subcommand};
use nightjar_core::backup;
use nightjar_core::config::Config;
use nightjar_core::config_io;
use nightjar_core::partial::{self, SizedSource};
use nightjar_core::preflight::{self, SpaceStatus};
use nightjar_core::rclone;
use nightjar_core::state::BackupOutcome;
use prompts::PartialChoice;

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
        } => run_backup(power_off, partial_method, yes),
        Command::Preflight => run_preflight(),
    };
    std::process::exit(exit_code);
}

// ---------- shared helpers ----------

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

// ---------- preflight command ----------

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

// ---------- backup command ----------

fn run_backup(power_off_flag: bool, partial_method: Option<String>, yes: bool) -> i32 {
    let config = match load_config() {
        Some(c) => c,
        None => return 1,
    };

    // 1. Resolve power-off intent: flag wins, else ask (default no).
    let power_off = if power_off_flag {
        true
    } else {
        prompts::ask_yes_no("Power off the machine after a successful backup?", false)
    };

    // 2. Preflight.
    println!("Running preflight checks for remote '{}'...", config.remote);
    let report = match preflight::preflight(&config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Preflight failed: {e}");
            return 1;
        }
    };
    println!("Backup size: {}.", human_bytes(report.backup_size_bytes));

    // 3. Decide what to back up based on the space verdict.
    //    `selected` = None means "full backup"; Some(list) means partial.
    let selected: Option<Vec<SizedSource>> = match report.space {
        SpaceStatus::Fits { free_bytes } => {
            println!(
                "Free space: {}. Full backup will proceed.",
                human_bytes(free_bytes)
            );
            None
        }
        SpaceStatus::Unknown => {
            println!("Free space: unknown (the remote did not report it).");
            let go = yes
                || prompts::ask_yes_no(
                    "Could not verify free space. Continue with a full backup anyway?",
                    false,
                );
            if !go {
                println!("Backup cancelled.");
                return 0;
            }
            None
        }
        SpaceStatus::Shortfall {
            free_bytes,
            needed_bytes,
        } => {
            println!(
                "Free space: {}. Not enough for the full {} backup.",
                human_bytes(free_bytes),
                human_bytes(needed_bytes)
            );
            match resolve_partial(&config, free_bytes, &partial_method, yes) {
                Ok(Some(list)) => Some(list),
                Ok(None) => {
                    println!("Backup cancelled.");
                    return 0;
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    return 1;
                }
            }
        }
    };

    // 4. Run the backup.
    println!();
    let outcome = match &selected {
        None => {
            for source in &config.sources {
                println!("Backing up '{}'...", source.name);
            }
            backup::run_full_backup(&config)
        }
        Some(list) => {
            for s in list {
                println!("Backing up '{}'...", s.source.name);
            }
            backup::run_partial_backup(&config, list)
        }
    };

    // 5. Handle the outcome.
    println!();
    match &outcome {
        BackupOutcome::FullVerified => {
            println!("✓ Full backup completed and verified.");
        }
        BackupOutcome::PartialVerified => {
            println!("✓ Partial backup completed and verified.");
            report_skipped(&config, selected.as_ref());
        }
        BackupOutcome::Failed(msg) => {
            eprintln!("✗ Backup failed: {msg}");
            eprintln!("The machine will NOT be powered off.");
            return 1;
        }
    }

    // 6. Power off only if opted in AND the outcome grants a permit.
    if power_off {
        match outcome.power_off_permit() {
            Some(permit) => {
                println!("Powering off...");
                if let Err(e) = nightjar_core::poweroff::power_off(permit) {
                    eprintln!("Could not power off: {e}");
                    return 1;
                }
            }
            None => {
                // Unreachable for verified outcomes, but handled honestly.
                eprintln!("Power-off was requested but the outcome did not permit it.");
            }
        }
    }

    0
}

/// Resolves a partial selection at a shortfall. Returns Ok(Some(list)) to
/// proceed with that subset, Ok(None) if the user cancelled, or Err on a
/// problem (e.g. non-interactive with no method given).
fn resolve_partial(
    config: &Config,
    free_bytes: u64,
    partial_method: &Option<String>,
    yes: bool,
) -> Result<Option<Vec<SizedSource>>, String> {
    // Measure each source so the selection logic has sizes.
    let sized = measure_sources(config)?;

    // Determine the method: flag wins, else prompt.
    let method = match partial_method {
        Some(value) => prompts::parse_partial_method(value)?,
        None => prompts::ask_partial_method()?,
    };

    match method {
        PartialChoice::SmallestFirst => {
            let selection = partial::select_smallest_first(&sized, free_bytes);
            if selection.selected.is_empty() {
                return Err("not even the smallest folder fits in the free space".to_string());
            }
            println!("Selected (smallest-first):");
            for s in &selection.selected {
                println!("  + {} ({})", s.source.name, human_bytes(s.size_bytes));
            }
            let go = yes || prompts::ask_yes_no("Proceed with this partial backup?", true);
            if go {
                Ok(Some(selection.selected))
            } else {
                Ok(None)
            }
        }
        PartialChoice::Customization => choose_custom(&sized, free_bytes, yes),
    }
}

/// Interactive custom selection: list sources, let the user pick by number,
/// validate the selection fits, re-prompt if not.
fn choose_custom(
    sized: &[SizedSource],
    free_bytes: u64,
    yes: bool,
) -> Result<Option<Vec<SizedSource>>, String> {
    // In non-interactive mode there is no safe way to hand-pick; refuse.
    if yes {
        return Err("custom selection requires interactive input; \
             use --partial-method smallest-first for unattended runs"
            .to_string());
    }

    loop {
        println!(
            "Available folders (free space: {}):",
            human_bytes(free_bytes)
        );
        for (i, s) in sized.iter().enumerate() {
            println!(
                "  {}) {} ({})",
                i + 1,
                s.source.name,
                human_bytes(s.size_bytes)
            );
        }
        let line = prompts::read_line_prompt(
            "Enter the numbers to include, comma-separated (or blank to cancel): ",
        );
        let line = match line {
            Some(l) => l,
            None => return Ok(None), // EOF/non-interactive
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        // Parse the comma-separated indices.
        let mut chosen: Vec<SizedSource> = Vec::new();
        let mut bad = false;
        for tok in trimmed.split(',') {
            match tok.trim().parse::<usize>() {
                Ok(n) if n >= 1 && n <= sized.len() => chosen.push(sized[n - 1].clone()),
                _ => {
                    println!(
                        "Invalid entry: '{}'. Please use listed numbers.",
                        tok.trim()
                    );
                    bad = true;
                    break;
                }
            }
        }
        if bad {
            continue;
        }

        match partial::validate_custom(&chosen, free_bytes) {
            partial::CustomValidation::Fits { selected_bytes } => {
                println!("Selection totals {} — fits.", human_bytes(selected_bytes));
                let go = prompts::ask_yes_no("Proceed with this partial backup?", true);
                return if go { Ok(Some(chosen)) } else { Ok(None) };
            }
            partial::CustomValidation::DoesNotFit {
                selected_bytes,
                over_by_bytes,
            } => {
                println!(
                    "Selection totals {} — over by {}. Please choose fewer.",
                    human_bytes(selected_bytes),
                    human_bytes(over_by_bytes)
                );
                continue;
            }
        }
    }
}

/// Measures each configured source's size via rclone, building SizedSources.
fn measure_sources(config: &Config) -> Result<Vec<SizedSource>, String> {
    let mut out = Vec::new();
    for source in &config.sources {
        let size = rclone::estimate_size(&source.path)
            .map_err(|e| format!("could not measure '{}': {e}", source.name))?;
        out.push(SizedSource {
            source: source.clone(),
            size_bytes: size,
        });
    }
    Ok(out)
}

/// Prints which configured sources were NOT part of a partial selection.
fn report_skipped(config: &Config, selected: Option<&Vec<SizedSource>>) {
    if let Some(list) = selected {
        let included: Vec<&str> = list.iter().map(|s| s.source.name.as_str()).collect();
        let skipped: Vec<&str> = config
            .sources
            .iter()
            .map(|s| s.name.as_str())
            .filter(|name| !included.contains(name))
            .collect();
        if !skipped.is_empty() {
            println!(
                "  Not backed up (insufficient space): {}",
                skipped.join(", ")
            );
        }
    }
}
