//! nightjar graphical interface (iced).
//!
//! 7-ii-a: load config and run preflight synchronously at startup, then
//! display the result (size, free space, verdict) or a clear message if
//! the config is missing / a check failed. Preflight runs on the boot
//! thread for now, so the window appears once it completes; making this
//! asynchronous is the next step.

use iced::widget::{column, container, text};
use iced::{Element, Length, Theme};
use nightjar_core::config_io;
use nightjar_core::preflight::{self, PreflightReport, SpaceStatus};

/// What the startup preflight produced.
enum Status {
    /// Config missing or unreadable; carries a guidance message.
    NoConfig(String),
    /// Preflight ran but failed a hard gate; carries the error text.
    Failed(String),
    /// Preflight succeeded; carries the report and the remote name.
    Ready {
        remote: String,
        report: PreflightReport,
    },
}

/// The application state.
struct App {
    status: Status,
}

impl Default for App {
    fn default() -> Self {
        App {
            status: load_and_preflight(),
        }
    }
}

/// Loads config and runs preflight synchronously, producing a Status.
fn load_and_preflight() -> Status {
    let path = match config_io::config_path() {
        Ok(p) => p,
        Err(e) => {
            return Status::NoConfig(format!("Could not determine config location: {e}"));
        }
    };

    let config = match config_io::load_from(&path) {
        Ok(c) => c,
        Err(e) => {
            return Status::NoConfig(format!("No usable config at {}:\n{e}", path.display()));
        }
    };

    match preflight::preflight(&config) {
        Ok(report) => Status::Ready {
            remote: config.remote.clone(),
            report,
        },
        Err(e) => Status::Failed(format!("{e}")),
    }
}

/// Formats a byte count as a human-readable string (e.g. "1.50 GiB").
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

/// Events the application reacts to. None yet in this step.
#[derive(Debug, Clone)]
enum Message {}

impl App {
    fn update(&mut self, _message: Message) {
        // No interactions yet.
    }

    fn view(&self) -> Element<'_, Message> {
        let mut content = column![
            text("nightjar").size(40),
            text("A backup tool that runs while you sleep.").size(16),
        ]
        .spacing(12);

        // Add a section describing the preflight result.
        content = match &self.status {
            Status::NoConfig(msg) => content
                .push(text("No configuration found").size(22))
                .push(text(msg.clone()).size(14)),
            Status::Failed(msg) => content
                .push(text("Preflight failed").size(22))
                .push(text(msg.clone()).size(14)),
            Status::Ready { remote, report } => {
                let verdict = match &report.space {
                    SpaceStatus::Fits { free_bytes } => format!(
                        "Fits — {} free. A full backup can proceed.",
                        human_bytes(*free_bytes)
                    ),
                    SpaceStatus::Shortfall {
                        free_bytes,
                        needed_bytes,
                    } => format!(
                        "Shortfall — need {} but only {} free. A partial backup would be offered.",
                        human_bytes(*needed_bytes),
                        human_bytes(*free_bytes)
                    ),
                    SpaceStatus::Unknown => {
                        "Free space unknown — the remote did not report it.".to_string()
                    }
                };
                content
                    .push(text(format!("Remote: {remote}")).size(18))
                    .push(
                        text(format!(
                            "Backup size: {}",
                            human_bytes(report.backup_size_bytes)
                        ))
                        .size(18),
                    )
                    .push(text(verdict).size(16))
            }
        };

        container(content)
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
    iced::application(App::default, App::update, App::view)
        .title("nightjar")
        .theme(App::theme)
        .centered()
        .run()
}
