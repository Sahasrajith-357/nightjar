//! Interactive prompt helpers for the CLI.
//!
//! Every prompt is non-tty safe: if stdin is not an interactive terminal
//! (e.g. an unattended/cron run), we do NOT block waiting for input that
//! will never arrive. Instead we use the provided default, or — for choices
//! with no safe default — return an error directing the user to pass a flag.

use std::io::{self, IsTerminal, Write};

/// The partial-backup method a user can choose at a shortfall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartialChoice {
    SmallestFirst,
    Customization,
}

/// Returns true if we have an interactive terminal on stdin.
fn stdin_is_interactive() -> bool {
    io::stdin().is_terminal()
}

/// Asks a yes/no question. `default` is used when the user just presses
/// Enter, AND whenever stdin is not interactive (so unattended runs never
/// block). Returns the boolean answer.
pub fn ask_yes_no(question: &str, default: bool) -> bool {
    // Non-interactive: never block; take the default.
    if !stdin_is_interactive() {
        return default;
    }

    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        print!("{question} {hint} ");
        // print! does not flush; we must flush so the prompt shows before
        // we block on input.
        let _ = io::stdout().flush();

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            // EOF (Ctrl-D or closed input): fall back to the default.
            Ok(0) => return default,
            Ok(_) => {
                let answer = line.trim().to_lowercase();
                match answer.as_str() {
                    "" => return default,
                    "y" | "yes" => return true,
                    "n" | "no" => return false,
                    _ => {
                        println!("Please answer 'y' or 'n'.");
                        continue;
                    }
                }
            }
            // Read error: don't loop forever; take the default.
            Err(_) => return default,
        }
    }
}

/// Asks the user to choose a partial-backup method. Non-interactive stdin
/// returns Err (there is no safe default for *which* files to drop), telling
/// the caller the user must pass --partial-method.
pub fn ask_partial_method() -> Result<PartialChoice, String> {
    if !stdin_is_interactive() {
        return Err("space is short and no --partial-method was given; \
             pass --partial-method smallest-first or --partial-method custom"
            .to_string());
    }

    loop {
        println!("Not enough space for a full backup. Choose how to proceed:");
        println!("  1) smallest-first  — back up as many whole folders as fit, smallest first");
        println!("  2) custom          — choose which folders to include");
        print!("Enter 1 or 2: ");
        let _ = io::stdout().flush();

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => {
                return Err("no choice made (end of input)".to_string());
            }
            Ok(_) => match line.trim() {
                "1" => return Ok(PartialChoice::SmallestFirst),
                "2" => return Ok(PartialChoice::Customization),
                _ => {
                    println!("Please enter 1 or 2.");
                    continue;
                }
            },
            Err(e) => return Err(format!("could not read input: {e}")),
        }
    }
}

/// Parses a --partial-method flag value into a PartialChoice.
/// Returns Err with guidance on an unrecognized value.
pub fn parse_partial_method(value: &str) -> Result<PartialChoice, String> {
    match value.trim().to_lowercase().as_str() {
        "smallest-first" | "smallest" => Ok(PartialChoice::SmallestFirst),
        "custom" | "customization" => Ok(PartialChoice::Customization),
        other => Err(format!(
            "unknown partial method '{other}'; use 'smallest-first' or 'custom'"
        )),
    }
}

/// Prints a prompt and reads one line. Returns None on non-interactive
/// stdin or EOF (so callers never block unattended).
pub fn read_line_prompt(prompt: &str) -> Option<String> {
    if !stdin_is_interactive() {
        return None;
    }
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) => None,
        Ok(_) => Some(line),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_smallest_first_variants() {
        assert_eq!(
            parse_partial_method("smallest-first"),
            Ok(PartialChoice::SmallestFirst)
        );
        assert_eq!(
            parse_partial_method("smallest"),
            Ok(PartialChoice::SmallestFirst)
        );
        assert_eq!(
            parse_partial_method("SMALLEST-FIRST"),
            Ok(PartialChoice::SmallestFirst)
        );
    }

    #[test]
    fn parses_custom_variants() {
        assert_eq!(
            parse_partial_method("custom"),
            Ok(PartialChoice::Customization)
        );
        assert_eq!(
            parse_partial_method("customization"),
            Ok(PartialChoice::Customization)
        );
    }

    #[test]
    fn rejects_unknown_method() {
        assert!(parse_partial_method("banana").is_err());
    }
}
