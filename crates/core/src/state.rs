//! The backup lifecycle state machine.
//!
//! `BackupState` is a runtime enum the front-ends observe and react to.
//! The safety-critical rule — power-off only after a *verified* backup —
//! is enforced through `PowerOffPermit`, a token that can ONLY be obtained
//! from a verified outcome. The power-off action (built later) will require
//! this token, making "power off after failure" impossible to express.

use crate::preflight::PreflightReport;

/// What kind of backup is being performed.
#[derive(Debug, Clone, PartialEq)]
pub enum BackupKind {
    /// Back up everything.
    Full,
    /// Back up only a chosen subset (used when space is short).
    Partial,
}

/// How the user chose to select files for a partial backup.
#[derive(Debug, Clone, PartialEq)]
pub enum PartialMethod {
    /// User hand-picks which sources to include.
    Customization,
    /// Automatically fit as many whole sources as possible, smallest first.
    SmallestFirst,
}

/// The terminal result of a backup attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum BackupOutcome {
    /// The full backup completed and verified.
    FullVerified,
    /// A partial backup completed and verified; some data was not backed up.
    PartialVerified,
    /// The backup failed. Carries a description of why.
    Failed(String),
}

/// A token proving a backup finished in a verified state.
///
/// This type has no public constructor — the ONLY way to obtain one is via
/// `BackupOutcome::power_off_permit`, which returns it solely for verified
/// outcomes. Any action requiring power-off will demand this token, so it
/// is impossible to power off after a failure.
#[derive(Debug)]
pub struct PowerOffPermit {
    // Private field => cannot be constructed outside this module.
    _private: (),
}

impl BackupOutcome {
    /// Returns a `PowerOffPermit` only for verified outcomes; `None` for a
    /// failure. This is the single gate through which power-off is allowed.
    pub fn power_off_permit(&self) -> Option<PowerOffPermit> {
        match self {
            BackupOutcome::FullVerified | BackupOutcome::PartialVerified => {
                Some(PowerOffPermit { _private: () })
            }
            BackupOutcome::Failed(_) => None,
        }
    }

    /// Convenience: did the backup succeed (in any verified form)?
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            BackupOutcome::FullVerified | BackupOutcome::PartialVerified
        )
    }
}

/// The observable lifecycle state of a backup.
#[derive(Debug, Clone, PartialEq)]
pub enum BackupState {
    /// Nothing happening yet.
    Idle,
    /// Running preflight checks.
    Preflighting,
    /// Preflight found a shortfall; waiting for the user to choose a partial
    /// method (or cancel). Carries the report so the UI can show specifics.
    AwaitingPartialDecision { report: PreflightReport },
    /// A transfer is in progress.
    Running { kind: BackupKind },
    /// Transfer done; verifying the result.
    Verifying { kind: BackupKind },
    /// Terminal: the backup is finished, with its outcome.
    Finished(BackupOutcome),
}

impl BackupState {
    /// The starting state.
    pub fn new() -> Self {
        BackupState::Idle
    }

    /// Is this a terminal state (no further transitions)?
    pub fn is_terminal(&self) -> bool {
        matches!(self, BackupState::Finished(_))
    }
}

impl Default for BackupState {
    fn default() -> Self {
        BackupState::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_verified_grants_power_off_permit() {
        let outcome = BackupOutcome::FullVerified;
        assert!(outcome.power_off_permit().is_some());
    }

    #[test]
    fn partial_verified_grants_power_off_permit() {
        let outcome = BackupOutcome::PartialVerified;
        assert!(outcome.power_off_permit().is_some());
    }

    #[test]
    fn failed_denies_power_off_permit() {
        // THE critical safety invariant: a failed backup yields no permit,
        // so power-off cannot be invoked.
        let outcome = BackupOutcome::Failed("network dropped".to_string());
        assert!(outcome.power_off_permit().is_none());
    }

    #[test]
    fn is_success_reflects_outcome() {
        assert!(BackupOutcome::FullVerified.is_success());
        assert!(BackupOutcome::PartialVerified.is_success());
        assert!(!BackupOutcome::Failed("x".to_string()).is_success());
    }

    #[test]
    fn new_state_is_idle_and_not_terminal() {
        let s = BackupState::new();
        assert_eq!(s, BackupState::Idle);
        assert!(!s.is_terminal());
    }

    #[test]
    fn finished_state_is_terminal() {
        let s = BackupState::Finished(BackupOutcome::FullVerified);
        assert!(s.is_terminal());
    }
}
