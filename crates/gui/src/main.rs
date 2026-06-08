//! nightjar graphical interface (iced).
//!
//! 7-iv-b: when preflight reports a shortfall, measure each source's size
//! asynchronously, then present a per-folder checklist with a running total
//! against free space, an auto-fill (smallest-first) button, and a "Back up
//! selected" action enabled only when the selection fits. Reuses the
//! exhaustively-tested partial-selection logic.

use iced::time::{self, Duration};
use iced::widget::{
    button, canvas, checkbox, column, container, pick_list, progress_bar, row, scrollable, space,
    stack, svg, text,
};
use iced::{Color, Element, Font, Length, Size, Subscription, Task, Theme};
use nightjar_core::backup;
use nightjar_core::config::Config;
use nightjar_core::config::Source;
use nightjar_core::config_io;
use nightjar_core::partial::{self, SizedSource};
use nightjar_core::preflight::{self, PreflightReport, SpaceStatus};
use nightjar_core::rclone;
use nightjar_core::state::BackupOutcome;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
mod theme;
use theme::Preset;
mod motif;

/// Embedded fonts (bundled in crates/gui/fonts/).
const BLANKA_BYTES: &[u8] = include_bytes!("../fonts/Blanka-Regular.otf");
const MONO_BYTES: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");

/// Font handles, keyed to each font's internal family name.
const BLANKA: Font = Font::with_name("Blanka");
const MONO: Font = Font::with_name("JetBrains Mono");

const LOGO_SVG: &[u8] = include_bytes!("../assets/nightjar-logo.svg");

// Spacing scale (consistent vertical/horizontal rhythm).
const SP_XS: f32 = 6.0;
const SP_SM: f32 = 12.0;
const SP_MD: f32 = 20.0;
const SP_LG: f32 = 32.0;
const SP_XL: f32 = 48.0;

#[derive(Debug, Clone)]
enum PreflightResult {
    NoConfig(String),
    Failed(String),
    Ready { report: PreflightReport },
}

/// The app lifecycle, including the shortfall sub-states.
enum Phase {
    /// Sources changed; the previous preflight is stale. Shows Recalculate.
    /// /// The cloud-connect wizard screen.
    ConnectCloud,
    NeedsRecalc,
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
    /// A backup is running, source by source. Carries the full ordered list
    /// of sources to back up, how many have completed, and the current name.
    BackingUp {
        sources: Vec<Source>,
        done: usize,
        current: String,
    },
    Finished(BackupOutcome),
    /// Showing the directory tree of the configured sources.
    Tree(Vec<nightjar_core::tree::TreeEntry>),
    /// Walking the filesystem to build the tree.
    BuildingTree,
}

struct App {
    remote: String,
    config: Option<Config>,
    phase: Phase,
    power_off: bool,
    remotes: Vec<String>,
    notice: Option<String>,
    window_width: f32,
    preflight_stale: bool,
    progress: Arc<Mutex<f32>>,
    displayed_progress: f32,
    preset: Preset,
    report: Option<BackupReport>,
    last_backup_size: u64,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
enum Message {
    OpenConnectCloud,
    LaunchGuidedSetup,
    RefreshRemotes,
    PreflightFinished(PreflightResult, Option<Config>, String, Vec<String>),
    RemoteSelected(String),
    PowerOffToggled(bool),
    StartBackup,
    SizesMeasured(Vec<SizedSource>, u64),
    FolderToggled(usize, bool),
    AutoFill,
    StartPartial,
    AddFolderClicked,
    FoldersPicked(Vec<PathBuf>),
    RemoveSource(PathBuf),
    /// One source finished backing up (Ok = copied+verified).
    SourceDone(Result<(), String>),
    WindowResized(Size),
    RecalcSize,
    BackToStart,
    Tick,
    ApplyPreset,
    ThemeSelected(Preset),
    ReportReady(Vec<String>, u64, Option<u64>),
    OpenTree,
    TreeReady(Vec<nightjar_core::tree::TreeEntry>),
    AbortBackup,
}

#[derive(Debug, Clone)]
struct BackupReport {
    folders: Vec<String>,
    total_bytes: u64,
    free_remaining: Option<u64>,
}

fn boot() -> (App, Task<Message>) {
    let app = App {
        remote: String::new(),
        config: None,
        phase: Phase::Checking,
        power_off: false,
        remotes: Vec::new(),
        notice: None,
        window_width: 1200.0,
        preflight_stale: false,
        progress: Arc::new(Mutex::new(0.0)),
        displayed_progress: 0.0,
        preset: Preset::Ember,
        report: None,
        last_backup_size: 0,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let task = Task::perform(
        run_preflight_blocking(),
        |(result, config, remote, remotes)| {
            Message::PreflightFinished(result, config, remote, remotes)
        },
    );
    (app, task)
}

async fn run_preflight_blocking() -> (PreflightResult, Option<Config>, String, Vec<String>) {
    tokio::task::spawn_blocking(|| {
        let remotes = rclone::list_remotes().unwrap_or_default();
        let (result, config, remote) = load_and_preflight();
        (result, config, remote, remotes)
    })
    .await
    .unwrap_or_else(|e| {
        (
            PreflightResult::Failed(format!("internal task error: {e}")),
            None,
            String::new(),
            Vec::new(),
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

/// Opens the native folder picker (multi-select) asynchronously, returning
/// the chosen paths (empty if cancelled).
async fn pick_folders() -> Vec<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Choose folders to back up")
        .pick_folders()
        .await
        .map(|handles| {
            handles
                .into_iter()
                .map(|h| h.path().to_path_buf())
                .collect()
        })
        .unwrap_or_default()
}

async fn backup_one_blocking(
    config: Config,
    source: Source,
    progress: Arc<Mutex<f32>>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        backup::backup_one_source_streaming(&config, &source, cancel, |frac| {
            if let Ok(mut p) = progress.lock() {
                *p = frac;
            }
        })
    })
    .await
    .unwrap_or_else(|e| Err(format!("internal task error: {e}")))
}

async fn query_free_space(remote: String) -> Option<u64> {
    tokio::task::spawn_blocking(move || rclone::check_free_space(&remote).ok())
        .await
        .ok()
        .flatten()
}

async fn build_tree_blocking(sources: Vec<Source>) -> Vec<nightjar_core::tree::TreeEntry> {
    tokio::task::spawn_blocking(move || nightjar_core::tree::build_tree(&sources))
        .await
        .unwrap_or_default()
}

/// Given the ordered sources and how many are done, either launches the next
/// source's backup or, if all are done, returns None to signal completion.
fn next_source_task(
    config: &Config,
    sources: &[Source],
    done: usize,
    progress: &Arc<Mutex<f32>>,
    cancel: &Arc<AtomicBool>,
) -> Option<Task<Message>> {
    if done < sources.len() {
        if let Ok(mut p) = progress.lock() {
            *p = 0.0;
        }
        let config = config.clone();
        let source = sources[done].clone();
        let progress = progress.clone();
        let cancel = cancel.clone();
        Some(Task::perform(
            backup_one_blocking(config, source, progress, cancel),
            Message::SourceDone,
        ))
    } else {
        None
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
            Message::PreflightFinished(result, config, remote, remotes) => {
                self.remote = remote;
                self.config = config.clone();
                if let Some(cfg) = &config {
                    if let Some(name) = &cfg.theme {
                        self.preset = Preset::from_name(name);
                    }
                }
                self.remotes = remotes;
                self.notice = None;
                // If shortfall, jump straight into measuring.
                if let PreflightResult::Ready { report } = &result {
                    self.last_backup_size = report.backup_size_bytes;
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
            Message::RemoteSelected(name) => {
                // Update the config's remote, persist it, and re-run preflight.
                if let Some(config) = &mut self.config {
                    if config.remote == name {
                        return Task::none(); // no change
                    }
                    config.remote = name.clone();
                    // Persist; ignore save errors for now beyond logging.
                    if let Ok(path) = config_io::config_path() {
                        let _ = config_io::save_to(config, &path);
                    }
                }
                self.remote = name;
                self.phase = Phase::Checking;
                // Re-run preflight (and re-fetch remotes) against the new remote.
                Task::perform(
                    run_preflight_blocking(),
                    |(result, config, remote, remotes)| {
                        Message::PreflightFinished(result, config, remote, remotes)
                    },
                )
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
                // If the source set changed since last preflight, recompute first.
                if self.preflight_stale {
                    self.preflight_stale = false;
                    self.phase = Phase::Checking;
                    return Task::perform(
                        run_preflight_blocking(),
                        |(result, config, remote, remotes)| {
                            Message::PreflightFinished(result, config, remote, remotes)
                        },
                    );
                }
                if let Some(config) = &self.config {
                    let config = config.clone();
                    let sources = config.sources.clone();
                    if sources.is_empty() {
                        return Task::none();
                    }
                    let current = sources[0].name.clone();
                    let task = next_source_task(&config, &sources, 0, &self.progress, &self.cancel)
                        .unwrap_or(Task::none());
                    self.cancel.store(false, Ordering::Relaxed);
                    self.displayed_progress = 0.0;
                    self.phase = Phase::BackingUp {
                        sources,
                        done: 0,
                        current,
                    };
                    task
                } else {
                    Task::none()
                }
            }
            Message::ApplyPreset => {
                self.notice = None;
                if let Some(config) = &mut self.config {
                    let mut added = 0usize;

                    // Merge preset sources (skip duplicates by path or name).
                    for src in nightjar_core::config::preset_sources() {
                        let dup = config
                            .sources
                            .iter()
                            .any(|s| s.path == src.path || s.name == src.name);
                        if !dup {
                            config.sources.push(src);
                            added += 1;
                        }
                    }

                    // Merge preset excludes (dedup).
                    for pat in nightjar_core::config::preset_excludes() {
                        if !config.excludes.contains(&pat) {
                            config.excludes.push(pat);
                        }
                    }

                    if let Ok(cfgpath) = config_io::config_path() {
                        let _ = config_io::save_to(config, &cfgpath);
                    }
                    self.invalidate_after_edit();

                    self.notice = Some(if added > 0 {
                        format!("Added {added} common folder(s). Review the list, then back up.")
                    } else {
                        "Common folders are already in your list.".to_string()
                    });
                }
                Task::none()
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
                                // Report accuracy: the backed-up size is the sum of
                                // the chosen folders, not the full configured set.
                                self.last_backup_size = chosen
                                    .iter()
                                    .fold(0u64, |acc, s| acc.saturating_add(s.size_bytes));
                                let sources: Vec<Source> =
                                    chosen.iter().map(|s| s.source.clone()).collect();
                                let current = sources[0].name.clone();
                                let task = next_source_task(
                                    &config,
                                    &sources,
                                    0,
                                    &self.progress,
                                    &self.cancel,
                                )
                                .unwrap_or(Task::none());
                                self.cancel.store(false, Ordering::Relaxed);
                                self.displayed_progress = 0.0;
                                self.phase = Phase::BackingUp {
                                    sources,
                                    done: 0,
                                    current,
                                };
                                return task;
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
            Message::SourceDone(result) => {
                if let Phase::BackingUp { sources, done, .. } = &mut self.phase {
                    match result {
                        Err(message) => {
                            // First failure: stop, do not touch later sources.
                            let outcome = if self.cancel.load(Ordering::Relaxed) {
                                BackupOutcome::Failed(
                                    "Aborted — backup incomplete. Some files may have been \
                                     copied; run the backup again to finish."
                                        .to_string(),
                                )
                            } else {
                                BackupOutcome::Failed(message)
                            };
                            self.phase = Phase::Finished(outcome);
                            Task::none()
                        }
                        Ok(()) => {
                            *done += 1;
                            let done_now = *done;
                            let total = sources.len();
                            let sources_clone = sources.clone();
                            if done_now < total {
                                // Advance to the next source.
                                if let Some(config) = &self.config {
                                    let current = sources_clone[done_now].name.clone();
                                    let task = next_source_task(
                                        config,
                                        &sources_clone,
                                        done_now,
                                        &self.progress,
                                        &self.cancel,
                                    )
                                    .unwrap_or(Task::none());
                                    if let Phase::BackingUp { current: c, .. } = &mut self.phase {
                                        *c = current;
                                    }
                                    return task;
                                }
                                Task::none()
                            } else {
                                // All sources copied and verified.
                                // Full vs partial: full only if we backed up
                                // exactly the whole configured set.
                                let is_full = self
                                    .config
                                    .as_ref()
                                    .map(|c| c.sources.len() == total)
                                    .unwrap_or(false);
                                let outcome = if is_full {
                                    BackupOutcome::FullVerified
                                } else {
                                    BackupOutcome::PartialVerified
                                };
                                // Power-off gate (verified-only permit).
                                if self.power_off {
                                    if let Some(permit) = outcome.power_off_permit() {
                                        if let Err(e) = nightjar_core::poweroff::power_off(permit) {
                                            self.phase =
                                                Phase::Finished(BackupOutcome::Failed(format!(
                                                    "Backup succeeded, but power-off failed: {e}"
                                                )));
                                            return Task::none();
                                        }
                                    }
                                }
                                // Assemble the backup report from data we have.
                                let folders: Vec<String> =
                                    sources_clone.iter().map(|s| s.name.clone()).collect();
                                let total = self.last_backup_size;
                                let remote = self.remote.clone();
                                self.report = None; // clear stale, fill when query returns
                                self.phase = Phase::Finished(outcome);
                                Task::perform(query_free_space(remote), move |free| {
                                    Message::ReportReady(folders.clone(), total, free)
                                })
                            }
                        }
                    }
                } else {
                    Task::none()
                }
            }
            Message::AbortBackup => {
                // Signal the running copy to stop; it kills rclone and returns
                // an error, which SourceDone routes to an aborted outcome.
                self.cancel.store(true, Ordering::Relaxed);
                Task::none()
            }
            Message::AddFolderClicked => {
                self.notice = None;
                Task::perform(pick_folders(), Message::FoldersPicked)
            }
            Message::FoldersPicked(paths) => {
                if paths.is_empty() {
                    return Task::none(); // cancelled
                }
                let mut added = 0usize;
                let mut skipped: Vec<String> = Vec::new();

                if let Some(config) = &mut self.config {
                    for path in paths {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Folder".to_string());

                        // Skip duplicates by path OR name (name collision would
                        // clash at the destination).
                        let dup = config
                            .sources
                            .iter()
                            .any(|s| s.path == path || s.name == name);
                        if dup {
                            skipped.push(name);
                            continue;
                        }
                        config.sources.push(Source { name, path });
                        added += 1;
                    }

                    if added > 0 {
                        self.invalidate_after_edit();
                    }
                }

                // Build a notice if anything was skipped.
                if !skipped.is_empty() {
                    self.notice = Some(format!(
                        "Skipped (already selected): {}",
                        skipped.join(", ")
                    ));
                } else {
                    self.notice = None;
                }
                Task::none()
            }
            Message::RemoveSource(path) => {
                self.notice = None;
                if let Some(config) = &mut self.config {
                    let before = config.sources.len();
                    config.sources.retain(|s| s.path != path);
                    if config.sources.len() != before {
                        // invalidate_after_edit persists the change and marks stale.
                        self.invalidate_after_edit();
                    }
                }
                Task::none()
            }
            Message::WindowResized(size) => {
                self.window_width = size.width;
                Task::none()
            }
            Message::RecalcSize => {
                self.preflight_stale = false;
                self.phase = Phase::Checking;
                Task::perform(
                    run_preflight_blocking(),
                    |(result, config, remote, remotes)| {
                        Message::PreflightFinished(result, config, remote, remotes)
                    },
                )
            }
            Message::BackToStart => {
                self.phase = Phase::Checking;
                Task::perform(
                    run_preflight_blocking(),
                    |(result, config, remote, remotes)| {
                        Message::PreflightFinished(result, config, remote, remotes)
                    },
                )
            }
            Message::Tick => {
                // Compute the overall target: completed folders + current
                // folder's fraction, divided by total folders.
                let target = if let Phase::BackingUp { sources, done, .. } = &self.phase {
                    let total = sources.len().max(1) as f32;
                    let frac = self.progress.lock().map(|p| *p).unwrap_or(0.0);
                    ((*done as f32) + frac) / total
                } else {
                    self.displayed_progress
                };
                let delta = target - self.displayed_progress;
                if delta.abs() < 0.001 {
                    self.displayed_progress = target;
                } else {
                    self.displayed_progress += delta * 0.1;
                }
                Task::none()
            }
            Message::ThemeSelected(preset) => {
                self.preset = preset;
                if let Some(config) = &mut self.config {
                    config.theme = Some(preset.name().to_string());
                    if let Ok(cfgpath) = config_io::config_path() {
                        let _ = config_io::save_to(config, &cfgpath);
                    }
                }
                Task::none()
            }
            Message::OpenConnectCloud => {
                self.notice = None;
                self.phase = Phase::ConnectCloud;
                Task::none()
            }
            Message::LaunchGuidedSetup => {
                match rclone::launch_guided_setup() {
                    Ok(()) => {
                        self.notice = Some(
                            "Setup opened in a terminal. Follow the prompts there, then click \
                             'I've connected — refresh'."
                                .to_string(),
                        );
                    }
                    Err(e) => {
                        self.notice = Some(format!("{e}"));
                    }
                }
                Task::none()
            }
            Message::RefreshRemotes => {
                // Re-scan remotes; return to the main flow showing the (possibly
                // new) remote list. Re-runs preflight to refresh everything.
                self.phase = Phase::Checking;
                Task::perform(
                    run_preflight_blocking(),
                    |(result, config, remote, remotes)| {
                        Message::PreflightFinished(result, config, remote, remotes)
                    },
                )
            }
            Message::ReportReady(folders, total_bytes, free_remaining) => {
                self.report = Some(BackupReport {
                    folders,
                    total_bytes,
                    free_remaining,
                });
                Task::none()
            }
            Message::OpenTree => {
                if let Some(config) = &self.config {
                    let sources = config.sources.clone();
                    if sources.is_empty() {
                        self.notice = Some("Add folders first to see their tree.".to_string());
                        return Task::none();
                    }
                    self.phase = Phase::BuildingTree;
                    return Task::perform(build_tree_blocking(sources), Message::TreeReady);
                }
                Task::none()
            }
            Message::TreeReady(entries) => {
                self.phase = Phase::Tree(entries);
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let w = self.window_width;
        let wide = w >= 900.0;

        // Outer padding scales gently with width so narrow windows aren't
        // wasted on margins and wide ones aren't cramped to the edge.
        let pad = if w < 600.0 {
            SP_MD
        } else if w < 1100.0 {
            SP_LG
        } else {
            SP_XL
        };

        let shell = column![
            self.masthead(),
            self.theme_bar(),
            self.body(wide),
            self.footer(),
        ]
        .spacing(SP_LG)
        .width(Length::Fill)
        .height(Length::Fill);

        container(shell)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(pad)
            .into()
    }

    /// Centered masthead: title top-center, motto below it.
    fn masthead(&self) -> Element<'_, Message> {
        // Scale the title to the window so it never clips on narrow windows.
        let title_size = (self.window_width * 0.07).clamp(40.0, 110.0);
        let title = text("NIGHTJAR")
            .font(BLANKA)
            .size(title_size)
            .color(self.preset.accent());

        // Logo scales with the title so the pairing stays proportional.
        // Square source (1101x1098); size by height to match the title.
        // SVG logo, tinted to the theme accent so the birds match the palette.
        // Sized in stable buckets (not the continuously-changing title_size) to
        // avoid re-layout flicker while resizing; SVG scales as vector cleanly.
        let logo_px = if self.window_width >= 900.0 {
            96.0
        } else {
            64.0
        };
        let accent = self.preset.accent();
        let logo = svg(svg::Handle::from_memory(LOGO_SVG))
            .width(Length::Fixed(logo_px))
            .height(Length::Fixed(logo_px))
            .style(move |_theme, _status| svg::Style {
                color: Some(accent),
            });

        let title_row = row![logo, title]
            .spacing(SP_MD)
            .align_y(iced::Alignment::Center);

        let motto = text("A backup tool that runs while you sleep.")
            .size(16)
            .color(self.preset.muted());

        let foreground = container(
            column![title_row, motto]
                .spacing(SP_SM)
                .align_x(iced::Alignment::Center),
        )
        .center_x(Length::Fill);

        // The motif canvas sits behind the title, filling the masthead area.
        let backdrop = canvas(motif::Motif {
            accent: self.preset.accent(),
        })
        .width(Length::Fill)
        .height(Length::Fixed(180.0));

        container(stack![
            container(backdrop).center_x(Length::Fill),
            foreground,
        ])
        .center_x(Length::Fill)
        .height(Length::Fixed(180.0))
        .into()
    }

    /// The body reflows: two columns when wide, stacked when narrow.
    fn body(&self, wide: bool) -> Element<'_, Message> {
        // The cloud-connect wizard takes over the whole body.
        if let Phase::ConnectCloud = &self.phase {
            return self.connect_cloud_view();
        }

        if let Phase::Tree(entries) = &self.phase {
            return self.tree_view(entries);
        }
        if let Phase::BuildingTree = &self.phase {
            return container(
                text("Building directory tree…")
                    .size(16)
                    .color(self.preset.muted()),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
        }

        let folders = self.folder_panel();
        let status = self.status_panel();

        let layout: Element<'_, Message> = if wide {
            // Two purposeful columns: folders take more room than status.
            row![
                container(folders)
                    .width(Length::FillPortion(3))
                    .height(Length::Fill),
                container(status)
                    .width(Length::FillPortion(2))
                    .height(Length::Fill),
            ]
            .spacing(SP_LG)
            .height(Length::Fill)
            .into()
        } else {
            // Single calm column when narrow.
            column![folders, status]
                .spacing(SP_MD)
                .height(Length::Fill)
                .into()
        };

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Left panel: the scrollable folder list + add button + remote selector.
    fn folder_panel(&self) -> Element<'_, Message> {
        let accent = self.preset.accent();
        let txt = Color::from_rgb8(0xc9, 0xbf, 0xc4);

        // Header: title grows, buttons stay natural width and wrap below when
        // the window is narrow — so labels never overflow their outlines.
        let title = text("FOLDERS TO BACK UP")
            .size(20)
            .color(self.preset.accent());
        let add_buttons = row![
            button(text("TREE").size(14).wrapping(text::Wrapping::None))
                .padding([SP_XS, SP_SM])
                .style(theme::secondary_button(accent, txt))
                .on_press(Message::OpenTree),
            button(
                text("COMMON FOLDERS")
                    .size(14)
                    .wrapping(text::Wrapping::None)
            )
            .padding([SP_XS, SP_SM])
            .style(theme::secondary_button(accent, txt))
            .on_press(Message::ApplyPreset),
            button(text("ADD FOLDERS").size(14).wrapping(text::Wrapping::None))
                .padding([SP_XS, SP_SM])
                .style(theme::secondary_button(accent, txt))
                .on_press(Message::AddFolderClicked),
        ]
        .spacing(SP_SM)
        .width(Length::Shrink);

        // Below a threshold, stack title over buttons (no horizontal overflow).
        let header: Element<'_, Message> = if self.window_width >= 720.0 {
            row![title, space().width(Length::Fill), add_buttons]
                .align_y(iced::Alignment::Center)
                .spacing(SP_SM)
                .into()
        } else {
            column![title, add_buttons].spacing(SP_SM).into()
        };

        // The list of sources.
        let mut list = column![].spacing(SP_SM);
        if let Some(config) = &self.config {
            if config.sources.is_empty() {
                list = list.push(
                    text("No folders selected yet. Use “Add folders” or “Common folders”.")
                        .size(14)
                        .color(self.preset.muted()),
                );
            } else {
                for s in &config.sources {
                    let path = s.path.clone();
                    // ONE grower per row: the name/path column fills and
                    // truncates; the ✕ button stays fixed. Row can't overflow.
                    let info = column![
                        text(s.name.clone()).size(15),
                        text(s.path.display().to_string())
                            .size(12)
                            .color(self.preset.muted())
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(2)
                    .width(Length::Fill)
                    .clip(true);

                    list = list.push(
                        row![
                            info,
                            button(text("✕").size(14))
                                .padding([4, 10])
                                .style(theme::remove_button(txt))
                                .on_press(Message::RemoveSource(path)),
                        ]
                        .spacing(SP_SM)
                        .align_y(iced::Alignment::Center),
                    );
                }
            }
        }

        let scroll = scrollable(container(list).padding(iced::Padding::default().right(SP_MD)))
            .height(Length::Fill);

        let inner = column![header, scroll].spacing(SP_MD).height(Length::Fill);

        container(inner)
            .style(theme::panel(self.preset.surface()))
            .padding(SP_MD)
            .height(Length::Fill)
            .into()
    }
    /// Right panel: remote selector + status that depends on the phase.
    fn status_panel(&self) -> Element<'_, Message> {
        let accent = self.preset.accent();
        let txt = Color::from_rgb8(0xc9, 0xbf, 0xc4);
        let muted = self.preset.muted();

        // Remote selector — one grower (the picker) so the row never overflows.
        let remote_row: Element<'_, Message> = if self.remotes.is_empty() {
            row![
                text("No cloud connected").size(15).width(Length::Fill),
                button(text("CONNECT").size(13).wrapping(text::Wrapping::None))
                    .padding([SP_XS, SP_SM])
                    .style(theme::secondary_button(accent, txt))
                    .on_press(Message::OpenConnectCloud),
            ]
            .spacing(SP_SM)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            let selected = Some(self.remote.clone());
            row![
                text("CLOUD").size(14).color(muted),
                pick_list(self.remotes.clone(), selected, Message::RemoteSelected)
                    .width(Length::Fill),
                button(text("CONNECT").size(13).wrapping(text::Wrapping::None))
                    .padding([SP_XS, SP_SM])
                    .style(theme::secondary_button(accent, txt))
                    .on_press(Message::OpenConnectCloud),
            ]
            .spacing(SP_SM)
            .align_y(iced::Alignment::Center)
            .into()
        };

        let recalc_btn = || {
            button(
                text("RECALCULATE SIZE")
                    .size(14)
                    .wrapping(text::Wrapping::None),
            )
            .padding([SP_XS, SP_SM])
            .style(theme::secondary_button(accent, txt))
            .on_press(Message::RecalcSize)
        };

        let status: Element<'_, Message> = match &self.phase {
            Phase::Checking => text("Checking your backup setup…")
                .size(16)
                .color(muted)
                .into(),
            Phase::ConnectCloud => text("").into(),

            Phase::NeedsRecalc => column![
                text("Folders changed").size(18),
                text("Recalculate the backup size to continue.")
                    .size(13)
                    .color(muted),
                recalc_btn(),
            ]
            .spacing(SP_SM)
            .into(),

            Phase::Tree(_) | Phase::BuildingTree => text("").into(),

            Phase::Ready { result } => match result {
                PreflightResult::NoConfig(msg) => column![
                    text("No configuration found").size(18),
                    text(msg.clone()).size(13).color(muted),
                ]
                .spacing(SP_SM)
                .into(),
                PreflightResult::Failed(msg) => column![
                    text("Preflight failed").size(18),
                    text(msg.clone()).size(13).color(muted),
                ]
                .spacing(SP_SM)
                .into(),
                PreflightResult::Ready { report } => {
                    let mut c = column![
                        text(format!(
                            "Backup size  {}",
                            human_bytes(report.backup_size_bytes)
                        ))
                        .size(16),
                    ]
                    .spacing(SP_SM);
                    match &report.space {
                        SpaceStatus::Fits { free_bytes } => {
                            c = c.push(
                                text(format!("Fits — {} free", human_bytes(*free_bytes)))
                                    .size(14)
                                    .color(accent),
                            );
                        }
                        SpaceStatus::Shortfall { .. } => {
                            c = c.push(
                                text("Not enough space — preparing options…")
                                    .size(14)
                                    .color(muted),
                            );
                        }
                        SpaceStatus::Unknown => {
                            c = c.push(
                                text("Free space unknown — a full backup will be attempted.")
                                    .size(14)
                                    .color(muted),
                            );
                        }
                    }
                    if self.preflight_stale {
                        c = c.push(
                            text("Folders changed — recalculate.")
                                .size(12)
                                .color(Color::from_rgb8(0xd9, 0xa0, 0x5b)),
                        );
                    }
                    c = c.push(recalc_btn());
                    c.into()
                }
            },

            Phase::Measuring => column![
                text("Not enough space for everything").size(16),
                text("Measuring your folders…").size(14).color(muted),
            ]
            .spacing(SP_SM)
            .into(),

            Phase::Choosing {
                free_bytes,
                sized,
                checked,
            } => self.choosing_body(*free_bytes, sized, checked),

            Phase::BackingUp {
                sources,
                done,
                current,
            } => {
                let pct = (self.displayed_progress * 100.0).clamp(0.0, 100.0);
                column![
                    text(format!("Backing up “{current}”")).size(18),
                    text(format!(
                        "{} of {} folders · {:.0}%",
                        done,
                        sources.len(),
                        pct
                    ))
                    .size(13)
                    .color(muted),
                    progress_bar(0.0..=1.0, self.displayed_progress).length(Length::Fill),
                ]
                .spacing(SP_SM)
                .into()
            }

            Phase::Finished(outcome) => {
                let accent = self.preset.accent();
                let txt = Color::from_rgb8(0xc9, 0xbf, 0xc4);
                let muted = self.preset.muted();
                let summary: Element<'_, Message> = match outcome {
                    BackupOutcome::FullVerified => text("✓  Full backup completed and verified")
                        .size(18)
                        .color(accent)
                        .into(),
                    BackupOutcome::PartialVerified => {
                        text("✓  Partial backup completed and verified")
                            .size(18)
                            .color(accent)
                            .into()
                    }
                    BackupOutcome::Failed(msg) => column![
                        text("✗  Backup failed").size(18),
                        text(msg.clone()).size(13).color(muted),
                    ]
                    .spacing(SP_XS)
                    .into(),
                };

                let mut col = column![summary].spacing(SP_MD);

                // Report (shown for successful backups once data is ready).
                if !matches!(outcome, BackupOutcome::Failed(_)) {
                    if let Some(r) = &self.report {
                        let mut rep =
                            column![text("Backup report").size(15).color(accent),].spacing(SP_XS);
                        rep = rep.push(
                            text(format!("Folders backed up: {}", r.folders.join(", "))).size(13),
                        );
                        rep = rep.push(
                            text(format!("Space occupied: {}", human_bytes(r.total_bytes)))
                                .size(13),
                        );
                        let remaining = match r.free_remaining {
                            Some(b) => human_bytes(b),
                            None => "unknown".to_string(),
                        };
                        rep = rep.push(
                            text(format!("Remaining cloud space: {remaining}"))
                                .size(13)
                                .color(muted),
                        );
                        col = col.push(
                            container(rep)
                                .style(theme::panel(self.preset.surface()))
                                .padding(SP_SM),
                        );
                    }
                }

                col = col.push(
                    button(
                        text("BACK TO START")
                            .size(14)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([SP_XS, SP_SM])
                    .style(theme::secondary_button(accent, txt))
                    .on_press(Message::BackToStart),
                );
                col.into()
            }
        };

        container(column![remote_row, status].spacing(SP_MD))
            .style(theme::panel(self.preset.surface()))
            .padding(SP_MD)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Partial-choosing content (lives in the status panel during Choosing).
    fn choosing_body(
        &self,
        free_bytes: u64,
        sized: &[SizedSource],
        checked: &[bool],
    ) -> Element<'_, Message> {
        let total = selected_total(sized, checked);
        let fits = total <= free_bytes;

        let mut list = column![].spacing(8);
        for (i, s) in sized.iter().enumerate() {
            let label = format!("{}  ({})", s.source.name, human_bytes(s.size_bytes));
            list = list.push(
                checkbox(checked[i])
                    .label(label)
                    .on_toggle(move |v| Message::FolderToggled(i, v)),
            );
        }
        let scrolled = scrollable(container(list).padding(iced::Padding::default().right(16.0)))
            .height(Length::Fixed(220.0));

        let status_line = if fits {
            text(format!("SELECTED: {} — fits.", human_bytes(total)))
                .size(15)
                .color(self.preset.accent())
        } else {
            text(format!(
                "SELECTED: {} — over by {}.",
                human_bytes(total),
                human_bytes(total - free_bytes)
            ))
            .size(15)
            .color(Color::from_rgb8(0xd9, 0xa0, 0x5b))
        };

        let back_up_btn = if fits && total > 0 {
            button(text("BACK UP SELECTED")).on_press(Message::StartPartial)
        } else {
            button(text("BACK UP SELECTED"))
        };

        column![
            text(format!("CHOOSE FOLDERS — {} FREE", human_bytes(free_bytes))).size(18),
            scrolled,
            status_line,
            row![
                button(text("AUTO-FILL (smallest-first)")).on_press(Message::AutoFill),
                back_up_btn,
            ]
            .spacing(12),
        ]
        .spacing(14)
        .into()
    }
    fn connect_cloud_view(&self) -> Element<'_, Message> {
        let accent = self.preset.accent();
        let txt = Color::from_rgb8(0xc9, 0xbf, 0xc4);

        let steps = column![
            text("Connect a cloud account").size(24).color(accent),
            text(
                "nightjar uses rclone to talk to your cloud storage. This will open \
                  rclone's guided setup in a terminal window."
            )
            .size(14),
            text("1.  Click \"Run guided setup\" below — a terminal will open.").size(14),
            text(
                "2.  Choose a name (e.g. \"cloud\"), then pick your provider \
                  (Google Drive, OneDrive, Dropbox, …)."
            )
            .size(14),
            text(
                "3.  When asked to use a web browser to authenticate, choose Yes — \
                  your browser opens; sign in and allow access."
            )
            .size(14),
            text(
                "4.  When rclone says the remote is configured, return here and click \
                  \"I've connected — refresh\"."
            )
            .size(14),
        ]
        .spacing(12);

        let actions = row![
            button(text("RUN GUIDED SETUP").size(15))
                .padding([10, 22])
                .style(theme::primary_button(accent))
                .on_press(Message::LaunchGuidedSetup),
            button(text("I'VE CONNECTED — REFRESH").size(15))
                .padding([10, 22])
                .style(theme::secondary_button(accent, txt))
                .on_press(Message::RefreshRemotes),
        ]
        .spacing(12);

        let mut col = column![steps, actions].spacing(24);

        if let Some(notice) = &self.notice {
            col = col.push(
                text(notice.clone())
                    .size(13)
                    .color(Color::from_rgb8(0xd9, 0xa0, 0x5b)),
            );
        }

        // A back link in case they want to return without connecting.
        col = col.push(
            button(text("← BACK").size(14))
                .padding([8, 16])
                .style(theme::secondary_button(accent, txt))
                .on_press(Message::RefreshRemotes),
        );

        container(col)
            .style(theme::panel(self.preset.surface()))
            .padding(28.0)
            .width(Length::Fill)
            .into()
    }

    /// Fixed footer: power-off toggle (left) and primary action (right).
    fn footer(&self) -> Element<'_, Message> {
        let action: Element<'_, Message> = match &self.phase {
            Phase::Ready {
                result: PreflightResult::Ready { report },
            } => match &report.space {
                SpaceStatus::Fits { .. } | SpaceStatus::Unknown => {
                    button(text("BACK UP NOW").size(16).wrapping(text::Wrapping::None))
                        .padding([SP_SM, SP_LG])
                        .style(theme::primary_button(self.preset.accent()))
                        .on_press(Message::StartBackup)
                        .into()
                }
                SpaceStatus::Shortfall { .. } => text("").into(),
            },
            Phase::BackingUp { .. } => {
                button(text("ABORT").size(16).wrapping(text::Wrapping::None))
                    .padding([SP_SM, SP_LG])
                    .style(theme::secondary_button(
                        self.preset.accent(),
                        Color::from_rgb8(0xc9, 0xbf, 0xc4),
                    ))
                    .on_press(Message::AbortBackup)
                    .into()
            }
            _ => text("").into(),
        };

        let power = row![
            checkbox(self.power_off).on_toggle(Message::PowerOffToggled),
            text("POWER OFF AFTER SUCCESSFUL BACKUP").color(self.preset.accent()),
        ]
        .spacing(10);

        container(
            row![power, space().width(Length::Fill), action]
                .align_y(iced::Alignment::Center)
                .spacing(SP_MD),
        )
        .width(Length::Fill)
        .padding(iced::Padding::default().top(SP_XS))
        .into()
    }

    fn theme(&self) -> Theme {
        self.preset.theme()
    }

    fn subscription(&self) -> Subscription<Message> {
        let resize = iced::window::resize_events().map(|(_id, size)| Message::WindowResized(size));

        if matches!(self.phase, Phase::BackingUp { .. }) {
            // Poll the shared progress ~6-7 times/sec for a smooth bar.
            let tick = time::every(Duration::from_millis(33)).map(|_| Message::Tick);
            Subscription::batch([resize, tick])
        } else {
            resize
        }
    }
    /// After a source-list edit, invalidate any in-progress shortfall
    /// decision so the user can recalculate with the new list.
    fn invalidate_after_edit(&mut self) {
        // Persist the edited source list so it survives the reload that
        // recalculate / preflight performs.
        if let Some(config) = &self.config {
            if let Ok(path) = config_io::config_path() {
                let _ = config_io::save_to(config, &path);
            }
        }
        self.preflight_stale = true;
        match self.phase {
            Phase::Choosing { .. } | Phase::Measuring => {
                self.phase = Phase::NeedsRecalc;
            }
            _ => {}
        }
    }

    fn theme_bar(&self) -> Element<'_, Message> {
        let picker = pick_list(
            Preset::ALL.to_vec(),
            Some(self.preset),
            Message::ThemeSelected,
        )
        .width(Length::Fixed(180.0));

        container(
            row![text("THEME:").size(13).color(self.preset.muted()), picker]
                .spacing(10)
                .align_y(iced::Alignment::Center),
        )
        .center_x(Length::Fill)
        .into()
    }
    fn tree_view(&self, entries: &[nightjar_core::tree::TreeEntry]) -> Element<'_, Message> {
        let accent = self.preset.accent();
        let txt = Color::from_rgb8(0xc9, 0xbf, 0xc4);
        let muted = self.preset.muted();

        let mut list = column![].spacing(2);
        if entries.is_empty() {
            list = list.push(text("No directories found.").size(14).color(muted));
        } else {
            for e in entries {
                // Indent by depth; roots (depth 0) in accent, deeper in text/muted.
                let indent = "    ".repeat(e.depth);
                let label = if e.truncated {
                    format!("{indent}{}  …", e.name)
                } else {
                    format!("{indent}{}", e.name)
                };
                let color = if e.depth == 0 { accent } else { txt };
                list = list.push(
                    text(label)
                        .size(14)
                        .font(MONO)
                        .color(color)
                        .wrapping(text::Wrapping::None),
                );
            }
        }

        let scroll = scrollable(container(list).padding(iced::Padding::default().right(SP_MD)))
            .height(Length::Fill);

        let header = row![
            text("DIRECTORY TREE").size(20).color(accent),
            space().width(Length::Fill),
            button(text("← BACK").size(14).wrapping(text::Wrapping::None))
                .padding([SP_XS, SP_SM])
                .style(theme::secondary_button(accent, txt))
                .on_press(Message::BackToStart),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(SP_SM);

        container(column![header, scroll].spacing(SP_MD).height(Length::Fill))
            .style(theme::panel(self.preset.surface()))
            .padding(SP_MD)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn main() -> iced::Result {
    iced::application(boot, App::update, App::view)
        .title("nightjar")
        .theme(App::theme)
        .subscription(App::subscription)
        .font(BLANKA_BYTES)
        .font(MONO_BYTES)
        .default_font(MONO)
        .centered()
        .run()
}
