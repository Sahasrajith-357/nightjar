//! Powering off the machine after a verified backup.
//!
//! This is the one action with a physical consequence, so it is guarded
//! two ways:
//! 1. `power_off` REQUIRES a `PowerOffPermit`, which can only be obtained
//!    from a verified BackupOutcome. A failed backup yields no permit, so
//!    this function cannot be called after a failure — enforced at compile
//!    time by the type signature.
//! 2. It performs a CLEAN shutdown via `systemctl poweroff` (through
//!    systemd/logind), never a forced or low-level power-off that could
//!    corrupt the filesystem we just verified.

use crate::error::Error;
use crate::state::PowerOffPermit;
use crate::Result;
use std::process::Command;

/// Cleanly powers off the machine.
///
/// Requires a `PowerOffPermit` (obtainable only from a verified backup
/// outcome), so it is impossible to invoke after a failed backup.
///
/// Runs `systemctl poweroff` for a clean, orderly shutdown. On most
/// single-user desktop sessions (with polkit) this needs no root. If the
/// system denies permission, returns Error::PermissionDenied with guidance
/// rather than attempting any forced or unsafe shutdown.
///
/// Note: the `_permit` parameter is intentionally unused at runtime — its
/// sole purpose is to make calling this function impossible without proof
/// of a verified backup. That proof is enforced by the type system.
pub fn power_off(_permit: PowerOffPermit) -> Result<()> {
    let result = Command::new("systemctl").arg("poweroff").output();

    let output = match result {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::PermissionDenied(
                "systemctl not found; cannot power off this system".to_string(),
            ));
        }
        Err(e) => return Err(Error::Io(e)),
    };

    if output.status.success() {
        // The shutdown has been initiated; the process may not even return
        // normally as the system goes down.
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::PermissionDenied(format!(
            "could not power off (systemctl poweroff failed): {}",
            stderr.trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use crate::state::BackupOutcome;

    #[test]
    fn only_verified_outcomes_yield_a_permit_for_power_off() {
        // We cannot test power_off() itself (it would shut down the machine).
        // But we re-confirm the gate at this boundary: a permit — the sole
        // key to power_off — exists only for verified outcomes. This is the
        // contract power_off relies on for its compile-time safety.
        assert!(BackupOutcome::FullVerified.power_off_permit().is_some());
        assert!(BackupOutcome::PartialVerified.power_off_permit().is_some());
        assert!(BackupOutcome::Failed("x".into())
            .power_off_permit()
            .is_none());
    }
}
