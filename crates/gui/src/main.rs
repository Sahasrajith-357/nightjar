//! nightjar graphical interface (iced).
//!
//! 7-iv-b: when preflight reports a shortfall, measure each source's size
//! asynchronously, then present a per-folder checklist with a running total
//! against free space, an auto-fill (smallest-first) button, and a "Back up
//! selected" action enabled only when the selection fits. Reuses the
//! exhaustively-tested partial-selection logic.

use iced::theme::Palette;
use iced::widget::{button, checkbox, column, container, row, text};
use iced::{Color, Element, Length, Task, Theme};
use nightjar_core::backup;
use nightjar_core::config::Config;
use nightjar_core::config_io;
use nightjar_core::partial::{self, SizedSource};
use nightjar_core::preflight::{self, PreflightReport, SpaceStatus};
use nightjar_core::state::BackupOutcome;

/// nightjar's custom theme — a warm "ember" dark palette built from the
/// coral keyboard color and a Death Note crimson accent.
fn nightjar_theme() -> Theme {
    Theme::custom(
        "nightjar".to_string(),
        Palette {
            background: Color::from_rgb8(0x16, 0x13, 0x1a), // near-black ember charcoal
            text: Color::from_rgb8(0xc9, 0xbf, 0xc4),       // warm grey
            primary: Color::from_rgb8(0xeb, 0x96, 0x7c),    // coral (your keyboard)
            success: Color::from_rgb8(0xeb, 0x96, 0x7c),    // coral (verified = warm, on-theme)
            warning: Color::from_rgb8(0xd9, 0xa0, 0x5b),    // warm amber
            danger: Color::from_rgb8(0x74, 0x19, 0x24),     // crimson (Death Note)
        },
    )
}

#[derive(Debug, Clone)]
enum PreflightResult {
    NoConfig(String),
    Failed(String),
    Ready { report: PreflightReport },
}

/// The app lifecycle, including the shortfall sub-states.
enum Phase {
    Checking,
    Ready {
        result: PreflightResult,
    },
    /// Shortfall detected; measuring each source's size in the background.
    Measuring,
    /// Sizes known; user is choosing which folders to include.
    Choosing {
        free_bytes: u64,
        sized: Vec<SizedSource>,
        /// Parallel to `sized`: whether each folder is selected.
        checked: Vec<bool>,
    },
    BackingUp,
    Finished(BackupOutcome),
}

struct App {
    remote: String,
    config: Option<Config>,
    phase: Phase,
    power_off: bool,
}

#[derive(Debug, Clone)]
enum Message {
    PreflightFinished(PreflightResult, Option<Config>, String),
    PowerOffToggled(bool),
    StartBackup,
    BackupFinished(BackupOutcome),
    /// Per-source sizes finished measuring (for the shortfall flow).
    SizesMeasured(Vec<SizedSource>, u64),
    /// A folder checkbox in the choosing view was toggled (index, new value).
    FolderToggled(usize, bool),
    /// Auto-fill the selection using smallest-first.
    AutoFill,
    /// Start the partial backup with the currently-checked folders.
    StartPartial,
}

fn boot() -> (App, Task<Message>) {
    let app = App {
        remote: String::new(),
        config: None,
        phase: Phase::Checking,
        power_off: false,
    };
    let task = Task::perform(run_preflight_blocking(), |(result, config, remote)| {
        Message::PreflightFinished(result, config, remote)
    });
    (app, task)
}

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

/// Measures each source size off the UI thread, returning SizedSources.
async fn measure_sizes_blocking(config: Config, free_bytes: u64) -> (Vec<SizedSource>, u64) {
    let sized = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for source in &config.sources {
            // On a measurement error, treat the size as 0 so the folder still
            // appears; a 0 never wrongly inflates the total.
            let size = nightjar_core::rclone::estimate_size(&source.path).unwrap_or(0);
            out.push(SizedSource {
                source: source.clone(),
                size_bytes: size,
            });
        }
        out
    })
    .await
    .unwrap_or_default();
    (sized, free_bytes)
}

async fn run_full_backup_blocking(config: Config) -> BackupOutcome {
    tokio::task::spawn_blocking(move || backup::run_full_backup(&config))
        .await
        .unwrap_or_else(|e| BackupOutcome::Failed(format!("internal task error: {e}")))
}

async fn run_partial_backup_blocking(config: Config, selected: Vec<SizedSource>) -> BackupOutcome {
    tokio::task::spawn_blocking(move || backup::run_partial_backup(&config, &selected))
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

/// Sum the sizes of the currently-checked folders.
fn selected_total(sized: &[SizedSource], checked: &[bool]) -> u64 {
    sized
        .iter()
        .zip(checked.iter())
        .filter(|(_, c)| **c)
        .fold(0u64, |acc, (s, _)| acc.saturating_add(s.size_bytes))
}

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PreflightFinished(result, config, remote) => {
                self.remote = remote;
                self.config = config.clone();
                // If shortfall, jump straight into measuring.
                if let PreflightResult::Ready { report } = &result {
                    if let SpaceStatus::Shortfall { free_bytes, .. } = report.space {
                        if let Some(cfg) = config.clone() {
                            self.phase = Phase::Measuring;
                            return Task::perform(
                                measure_sizes_blocking(cfg, free_bytes),
                                |(sized, free)| Message::SizesMeasured(sized, free),
                            );
                        }
                    }
                }
                self.phase = Phase::Ready { result };
                Task::none()
            }
            Message::PowerOffToggled(value) => {
                self.power_off = value;
                Task::none()
            }
            Message::SizesMeasured(sized, free_bytes) => {
                let checked = vec![false; sized.len()];
                self.phase = Phase::Choosing {
                    free_bytes,
                    sized,
                    checked,
                };
                Task::none()
            }
            Message::FolderToggled(index, value) => {
                if let Phase::Choosing { checked, .. } = &mut self.phase {
                    if let Some(slot) = checked.get_mut(index) {
                        *slot = value;
                    }
                }
                Task::none()
            }
            Message::AutoFill => {
                if let Phase::Choosing {
                    free_bytes,
                    sized,
                    checked,
                } = &mut self.phase
                {
                    let selection = partial::select_smallest_first(sized, *free_bytes);
                    // Mark checked = true for any source whose name is in the
                    // auto-selected set.
                    let chosen_names: Vec<&str> = selection
                        .selected
                        .iter()
                        .map(|s| s.source.name.as_str())
                        .collect();
                    for (i, s) in sized.iter().enumerate() {
                        checked[i] = chosen_names.contains(&s.source.name.as_str());
                    }
                }
                Task::none()
            }
            Message::StartBackup => {
                if let Some(config) = &self.config {
                    let config = config.clone();
                    self.phase = Phase::BackingUp;
                    Task::perform(run_full_backup_blocking(config), Message::BackupFinished)
                } else {
                    Task::none()
                }
            }
            Message::StartPartial => {
                // Gather the checked folders, confirm they fit, and launch.
                if let Phase::Choosing {
                    free_bytes,
                    sized,
                    checked,
                } = &self.phase
                {
                    let chosen: Vec<SizedSource> = sized
                        .iter()
                        .zip(checked.iter())
                        .filter(|(_, c)| **c)
                        .map(|(s, _)| s.clone())
                        .collect();

                    // Safety: only proceed if it actually fits.
                    match partial::validate_custom(&chosen, *free_bytes) {
                        partial::CustomValidation::Fits { .. } if !chosen.is_empty() => {
                            if let Some(config) = &self.config {
                                let config = config.clone();
                                self.phase = Phase::BackingUp;
                                return Task::perform(
                                    run_partial_backup_blocking(config, chosen),
                                    Message::BackupFinished,
                                );
                            }
                        }
                        _ => {
                            // Doesn't fit or empty: do nothing (button should
                            // be disabled in this case anyway).
                        }
                    }
                }
                Task::none()
            }
            Message::BackupFinished(outcome) => {
                if self.power_off {
                    if let Some(permit) = outcome.power_off_permit() {
                        if let Err(e) = nightjar_core::poweroff::power_off(permit) {
                            self.phase = Phase::Finished(BackupOutcome::Failed(format!(
                                "Backup succeeded, but power-off failed: {e}"
                            )));
                            return Task::none();
                        }
                    }
                }
                self.phase = Phase::Finished(outcome);
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut content = column![
            text("nightjar")
                .size(48)
                .color(Color::from_rgb8(0xeb, 0x96, 0x7c)),
            text("A backup tool that runs while you sleep.").size(16),
        ]
        .spacing(12);

        match &self.phase {
            Phase::Checking => {
                content = content.push(text("Checking your backup setup...").size(18));
            }
            Phase::Ready { result } => match result {
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
                        SpaceStatus::Shortfall { .. } => {
                            // Handled by the Measuring/Choosing phases; this
                            // branch is not normally rendered.
                            content = content
                                .push(text("Not enough space — preparing options...").size(16));
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
            Phase::Measuring => {
                content = content.push(text("Not enough space for everything.").size(18));
                content = content.push(text("Measuring your folders...").size(16));
            }
            Phase::Choosing {
                free_bytes,
                sized,
                checked,
            } => {
                let total = selected_total(sized, checked);
                let fits = total <= *free_bytes;

                content = content
                    .push(text("Choose folders to back up").size(22))
                    .push(text(format!("Free space: {}", human_bytes(*free_bytes))).size(14));

                // One checkbox row per folder.
                for (i, s) in sized.iter().enumerate() {
                    let label = format!("{}  ({})", s.source.name, human_bytes(s.size_bytes));
                    content = content.push(
                        checkbox(checked[i])
                            .label(label)
                            .on_toggle(move |v| Message::FolderToggled(i, v)),
                    );
                }

                // Running total + fit indicator.
                let status_line = if fits {
                    format!("Selected: {} — fits.", human_bytes(total))
                } else {
                    format!(
                        "Selected: {} — over by {}.",
                        human_bytes(total),
                        human_bytes(total - *free_bytes)
                    )
                };
                content = content.push(text(status_line).size(16));

                content = content.push(
                    checkbox(self.power_off)
                        .label("Power off after a successful backup")
                        .on_toggle(Message::PowerOffToggled),
                );

                // Auto-fill + Back up selected buttons.
                let back_up_btn = if fits && total > 0 {
                    button(text("Back up selected")).on_press(Message::StartPartial)
                } else {
                    // Disabled: no on_press.
                    button(text("Back up selected"))
                };
                content = content.push(
                    row![
                        button(text("Auto-fill (smallest-first)")).on_press(Message::AutoFill),
                        back_up_btn,
                    ]
                    .spacing(12),
                );
            }
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
        nightjar_theme()
    }
}

fn main() -> iced::Result {
    iced::application(boot, App::update, App::view)
        .title("nightjar")
        .theme(App::theme)
        .centered()
        .run()
}
