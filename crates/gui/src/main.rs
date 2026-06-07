//! nightjar graphical interface (iced).
//!
//! 7-ii-b: the window opens instantly in a "Checking..." state while
//! preflight runs on a background (blocking) thread via tokio::spawn_blocking,
//! delivering its result back as a message. The UI never blocks.

use iced::widget::{column, container, text};
use iced::{Element, Length, Task, Theme};
use nightjar_core::config_io;
use nightjar_core::preflight::{self, PreflightReport, SpaceStatus};

/// The result the background preflight produces. Cloneable so it can travel
/// in a Message (iced requires Message: Clone).
#[derive(Debug, Clone)]
enum PreflightResult {
    NoConfig(String),
    Failed(String),
    Ready {
        remote: String,
        report: PreflightReport,
    },
}

/// The display state.
enum Status {
    Checking,
    Done(PreflightResult),
}

struct App {
    status: Status,
}

/// Messages the application reacts to.
#[derive(Debug, Clone)]
enum Message {
    /// The background preflight has finished.
    PreflightFinished(PreflightResult),
}

/// Boot: start in the Checking state AND kick off the background preflight.
/// Returns (initial_state, initial_task).
fn boot() -> (App, Task<Message>) {
    let app = App {
        status: Status::Checking,
    };
    // Run the blocking preflight on tokio's blocking thread pool, then map
    // its result into a Message. spawn_blocking returns a future that iced
    // (running on tokio) can drive via Task::perform.
    let task = Task::perform(run_preflight_blocking(), Message::PreflightFinished);
    (app, task)
}

/// Awaitable wrapper that runs the blocking preflight off the UI thread.
async fn run_preflight_blocking() -> PreflightResult {
    tokio::task::spawn_blocking(load_and_preflight)
        .await
        // If the blocking task itself panicked, surface it as a Failed.
        .unwrap_or_else(|e| PreflightResult::Failed(format!("internal task error: {e}")))
}

/// The actual blocking work: load config, run preflight.
fn load_and_preflight() -> PreflightResult {
    let path = match config_io::config_path() {
        Ok(p) => p,
        Err(e) => {
            return PreflightResult::NoConfig(format!("Could not determine config location: {e}"));
        }
    };
    let config = match config_io::load_from(&path) {
        Ok(c) => c,
        Err(e) => {
            return PreflightResult::NoConfig(format!(
                "No usable config at {}:\n{e}",
                path.display()
            ));
        }
    };
    match preflight::preflight(&config) {
        Ok(report) => PreflightResult::Ready {
            remote: config.remote.clone(),
            report,
        },
        Err(e) => PreflightResult::Failed(format!("{e}")),
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

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PreflightFinished(result) => {
                self.status = Status::Done(result);
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

        content = match &self.status {
            Status::Checking => content.push(text("Checking your backup setup...").size(18)),
            Status::Done(PreflightResult::NoConfig(msg)) => content
                .push(text("No configuration found").size(22))
                .push(text(msg.clone()).size(14)),
            Status::Done(PreflightResult::Failed(msg)) => content
                .push(text("Preflight failed").size(22))
                .push(text(msg.clone()).size(14)),
            Status::Done(PreflightResult::Ready { remote, report }) => {
                let verdict = match &report.space {
                    SpaceStatus::Fits { free_bytes } => format!(
                        "Fits — {} free. A full backup can proceed.",
                        human_bytes(*free_bytes)
                    ),
                    SpaceStatus::Shortfall {
                        free_bytes,
                        needed_bytes,
                    } => format!(
                        "Shortfall — need {} but only {} free.",
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
    iced::application(boot, App::update, App::view)
        .title("nightjar")
        .theme(App::theme)
        .centered()
        .run()
}
