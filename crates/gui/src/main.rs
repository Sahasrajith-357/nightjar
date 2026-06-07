//! nightjar graphical interface (iced).
//!
//! 7-iii: adds a "Back up now" button that runs a FULL backup on a
//! background thread (tokio::spawn_blocking + Task::perform — the same
//! bridge proven for preflight), keeping the UI responsive. Per-source
//! status and the verified outcome are displayed. The partial-backup
//! decision UI (for the Shortfall case) is added in the next step.

use iced::widget::{button, checkbox, column, container, text};
use iced::{Element, Length, Task, Theme};
use nightjar_core::backup;
use nightjar_core::config::Config;
use nightjar_core::config_io;
use nightjar_core::poweroff;
use nightjar_core::preflight::{self, PreflightReport, SpaceStatus};
use nightjar_core::state::BackupOutcome;

/// The result the background preflight produces.
#[derive(Debug, Clone)]
enum PreflightResult {
    NoConfig(String),
    Failed(String),
    Ready { report: PreflightReport },
}

/// Where we are in the app's lifecycle.
enum Phase {
    /// Preflight still running.
    Checking,
    /// Preflight finished; showing its result. Carries the loaded config
    /// when it succeeded (needed to launch a backup).
    Ready {
        result: PreflightResult,
        config: Option<Config>,
    },
    /// A backup is running.
    BackingUp,
    /// A backup finished; showing the outcome.
    Finished(BackupOutcome),
}

struct App {
    remote: String,
    phase: Phase,
    power_off: bool,
}

#[derive(Debug, Clone)]
enum Message {
    PreflightFinished(PreflightResult, Option<Config>, String),
    PowerOffToggled(bool),
    StartBackup,
    BackupFinished(BackupOutcome),
}

/// Boot: start in Checking and kick off the background preflight.
fn boot() -> (App, Task<Message>) {
    let app = App {
        remote: String::new(),
        phase: Phase::Checking,
        power_off: false,
    };
    let task = Task::perform(run_preflight_blocking(), |(result, config, remote)| {
        Message::PreflightFinished(result, config, remote)
    });
    (app, task)
}

/// Runs the blocking preflight off the UI thread, returning the result, the
/// loaded config (if any, to launch a backup later), and the remote name.
async fn run_preflight_blocking() -> (PreflightResult, Option<Config>, String) {
    tokio::task::spawn_blocking(load_and_preflight)
        .await
        .unwrap_or_else(|e| {
            (
                PreflightResult::Failed(format!("internal task error: {e}")),
                None,
                String::new(),
            )
        })
}

fn load_and_preflight() -> (PreflightResult, Option<Config>, String) {
    let path = match config_io::config_path() {
        Ok(p) => p,
        Err(e) => {
            return (
                PreflightResult::NoConfig(format!("Could not determine config location: {e}")),
                None,
                String::new(),
            );
        }
    };
    let config = match config_io::load_from(&path) {
        Ok(c) => c,
        Err(e) => {
            return (
                PreflightResult::NoConfig(format!("No usable config at {}:\n{e}", path.display())),
                None,
                String::new(),
            );
        }
    };
    let remote = config.remote.clone();
    match preflight::preflight(&config) {
        Ok(report) => (PreflightResult::Ready { report }, Some(config), remote),
        Err(e) => (PreflightResult::Failed(format!("{e}")), None, remote),
    }
}

/// Runs a full backup off the UI thread.
async fn run_backup_blocking(config: Config) -> BackupOutcome {
    tokio::task::spawn_blocking(move || backup::run_full_backup(&config))
        .await
        .unwrap_or_else(|e| BackupOutcome::Failed(format!("internal task error: {e}")))
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

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PreflightFinished(result, config, remote) => {
                self.remote = remote;
                self.phase = Phase::Ready { result, config };
                Task::none()
            }
            Message::PowerOffToggled(value) => {
                self.power_off = value;
                Task::none()
            }
            Message::StartBackup => {
                if let Phase::Ready {
                    config: Some(config),
                    ..
                } = &self.phase
                {
                    let config = config.clone();
                    self.phase = Phase::BackingUp;
                    Task::perform(run_backup_blocking(config), Message::BackupFinished)
                } else {
                    Task::none()
                }
            }
            Message::BackupFinished(outcome) => {
                // If the user asked to power off AND the outcome is verified,
                // attempt a clean shutdown. The permit can only be obtained
                // from a verified outcome, so a failure cannot power off.
                if self.power_off {
                    if let Some(permit) = outcome.power_off_permit() {
                        if let Err(e) = poweroff::power_off(permit) {
                            // Power-off failed (e.g. permissions): show it
                            // instead of silently doing nothing.
                            self.phase = Phase::Finished(BackupOutcome::Failed(format!(
                                "Backup succeeded, but power-off failed: {e}"
                            )));
                            return Task::none();
                        }
                        // On success the machine is going down; nothing more
                        // to display.
                    }
                }
                self.phase = Phase::Finished(outcome);
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut content = column![
            text("nightjar").size(40),
            text("A backup tool that runs while you sleep.").size(16),
        ]
        .spacing(12);

        match &self.phase {
            Phase::Checking => {
                content = content.push(text("Checking your backup setup...").size(18));
            }
            Phase::Ready { result, .. } => match result {
                PreflightResult::NoConfig(msg) => {
                    content = content
                        .push(text("No configuration found").size(22))
                        .push(text(msg.clone()).size(14));
                }
                PreflightResult::Failed(msg) => {
                    content = content
                        .push(text("Preflight failed").size(22))
                        .push(text(msg.clone()).size(14));
                }
                PreflightResult::Ready { report } => {
                    content = content
                        .push(text(format!("Remote: {}", self.remote)).size(18))
                        .push(
                            text(format!(
                                "Backup size: {}",
                                human_bytes(report.backup_size_bytes)
                            ))
                            .size(18),
                        );
                    match &report.space {
                        SpaceStatus::Fits { free_bytes } => {
                            content = content
                                .push(
                                    text(format!(
                                        "Fits — {} free. Ready to back up.",
                                        human_bytes(*free_bytes)
                                    ))
                                    .size(16),
                                )
                                .push(
                                    checkbox(self.power_off)
                                        .label("Power off after a successful backup")
                                        .on_toggle(Message::PowerOffToggled),
                                )
                                .push(button(text("Back up now")).on_press(Message::StartBackup));
                        }
                        SpaceStatus::Shortfall {
                            free_bytes,
                            needed_bytes,
                        } => {
                            content = content.push(
                                text(format!(
                                    "Shortfall — need {} but only {} free. \
                                     Partial backup UI coming soon.",
                                    human_bytes(*needed_bytes),
                                    human_bytes(*free_bytes)
                                ))
                                .size(16),
                            );
                        }
                        SpaceStatus::Unknown => {
                            content = content
                                .push(
                                    text("Free space unknown — proceeding will attempt a full backup.")
                                        .size(16),
                                )
                                .push(
                                    checkbox(self.power_off)
                                        .label("Power off after a successful backup")
                                        .on_toggle(Message::PowerOffToggled),
                                )
                                .push(button(text("Back up now")).on_press(Message::StartBackup));
                        }
                    }
                }
            },
            Phase::BackingUp => {
                content = content.push(text("Backing up... please wait.").size(18));
            }
            Phase::Finished(outcome) => match outcome {
                BackupOutcome::FullVerified => {
                    content = content.push(text("✓ Full backup completed and verified.").size(20));
                }
                BackupOutcome::PartialVerified => {
                    content =
                        content.push(text("✓ Partial backup completed and verified.").size(20));
                }
                BackupOutcome::Failed(msg) => {
                    content = content
                        .push(text("✗ Backup failed").size(20))
                        .push(text(msg.clone()).size(14));
                }
            },
        }

        container(content.spacing(12))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(40)
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

fn main() -> iced::Result {
    iced::application(boot, App::update, App::view)
        .title("nightjar")
        .theme(App::theme)
        .centered()
        .run()
}
