//! nightjar graphical interface (iced).
//!
//! 7-iv-b: when preflight reports a shortfall, measure each source's size
//! asynchronously, then present a per-folder checklist with a running total
//! against free space, an auto-fill (smallest-first) button, and a "Back up
//! selected" action enabled only when the selection fits. Reuses the
//! exhaustively-tested partial-selection logic.

use iced::time::{self, Duration};
use iced::widget::{
    button, checkbox, column, container, pick_list, progress_bar, row, scrollable, space, text,
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
use std::sync::{Arc, Mutex};
mod theme;
use theme::Preset;

/// Embedded fonts (bundled in crates/gui/fonts/).
const BLANKA_BYTES: &[u8] = include_bytes!("../fonts/Blanka-Regular.otf");
const MONO_BYTES: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");

/// Font handles, keyed to each font's internal family name.
const BLANKA: Font = Font::with_name("Blanka");
const MONO: Font = Font::with_name("JetBrains Mono");

#[derive(Debug, Clone)]
enum PreflightResult {
    NoConfig(String),
    Failed(String),
    Ready { report: PreflightReport },
}

/// The app lifecycle, including the shortfall sub-states.
enum Phase {
    /// Sources changed; the previous preflight is stale. Shows Recalculate.
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
}

#[derive(Debug, Clone)]
enum Message {
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
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        backup::backup_one_source_streaming(&config, &source, |frac| {
            if let Ok(mut p) = progress.lock() {
                *p = frac;
            }
        })
    })
    .await
    .unwrap_or_else(|e| Err(format!("internal task error: {e}")))
}

/// Given the ordered sources and how many are done, either launches the next
/// source's backup or, if all are done, returns None to signal completion.
fn next_source_task(
    config: &Config,
    sources: &[Source],
    done: usize,
    progress: &Arc<Mutex<f32>>,
) -> Option<Task<Message>> {
    if done < sources.len() {
        // Reset the bar for the new source.
        if let Ok(mut p) = progress.lock() {
            *p = 0.0;
        }
        let config = config.clone();
        let source = sources[done].clone();
        let progress = progress.clone();
        Some(Task::perform(
            backup_one_blocking(config, source, progress),
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
                    let task = next_source_task(&config, &sources, 0, &self.progress)
                        .unwrap_or(Task::none());
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
                                let sources: Vec<Source> =
                                    chosen.iter().map(|s| s.source.clone()).collect();
                                let current = sources[0].name.clone();
                                let task = next_source_task(&config, &sources, 0, &self.progress)
                                    .unwrap_or(Task::none());
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
                            let outcome = BackupOutcome::Failed(message);
                            // (No power-off: Failed yields no permit.)
                            self.phase = Phase::Finished(outcome);
                            return Task::none();
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
                                self.phase = Phase::Finished(outcome);
                                Task::none()
                            }
                        }
                    }
                } else {
                    Task::none()
                }
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

                if added > 0 {
                    self.preflight_stale = true;
                }
                Task::none()
            }
            Message::RemoveSource(path) => {
                self.notice = None;
                if let Some(config) = &mut self.config {
                    let before = config.sources.len();
                    config.sources.retain(|s| s.path != path);
                    if config.sources.len() != before {
                        if let Ok(cfgpath) = config_io::config_path() {
                            let _ = config_io::save_to(config, &cfgpath);
                        }
                        // Mark preflight stale; do NOT re-run it here (instant edit).
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
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let wide = self.window_width >= 900.0;

        let shell = column![self.masthead(), self.body(wide), self.footer()]
            .spacing(28)
            .width(Length::Fill)
            .height(Length::Fill);

        // Constrain very wide windows to a comfortable band; fill otherwise.
        // Fill the whole window with comfortable padding — versatile across
        // resolutions, never stranded in a narrow centered band.
        container(shell)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(48)
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

        let motto = text("A backup tool that runs while you sleep.")
            .size(16)
            .color(self.preset.muted());

        let theme_picker = pick_list(
            Preset::ALL.to_vec(),
            Some(self.preset),
            Message::ThemeSelected,
        );

        container(
            column![title, motto, theme_picker]
                .spacing(10)
                .align_x(iced::Alignment::Center),
        )
        .center_x(Length::Fill)
        .into()
    }

    /// The body reflows: two columns when wide, stacked when narrow.
    fn body(&self, wide: bool) -> Element<'_, Message> {
        // Left: folder management. Right: status / action context.
        let left = self.folder_panel();
        let right = self.status_panel();

        let layout: Element<'_, Message> = if wide {
            row![
                container(left).width(Length::FillPortion(1)),
                container(right).width(Length::FillPortion(1)),
            ]
            .spacing(32)
            .height(Length::Fill)
            .into()
        } else {
            column![left, right].spacing(24).into()
        };

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Left panel: the scrollable folder list + add button + remote selector.
    fn folder_panel(&self) -> Element<'_, Message> {
        let mut list = column![].spacing(10);

        if let Some(config) = &self.config {
            if config.sources.is_empty() {
                list = list.push(
                    text("No folders selected yet.\nAdd folders to back up.")
                        .size(15)
                        .color(Color::from_rgb8(0x9a, 0x8f, 0x95)),
                );
            } else {
                for s in &config.sources {
                    let path = s.path.clone();
                    list = list.push(
                        row![
                            column![
                                text(s.name.clone()).size(16),
                                text(s.path.display().to_string())
                                    .size(12)
                                    .color(Color::from_rgb8(0x9a, 0x8f, 0x95)),
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                            button(text("✕").size(14))
                                .padding([4, 10])
                                .style(theme::remove_button(Color::from_rgb8(0xc9, 0xbf, 0xc4)))
                                .on_press(Message::RemoveSource(path)),
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                    );
                }
            }
        }

        // Right-pad the inner content so the scrollbar never overlaps the ✕.
        let inner = container(list).padding(16.0);
        let scrolled = container(scrollable(inner).height(Length::Fill))
            .style(theme::panel(self.preset.surface()))
            .padding(8.0)
            .height(Length::Fill);

        let accent = self.preset.accent();
        let txt = Color::from_rgb8(0xc9, 0xbf, 0xc4);
        let buttons = row![
            button(text("Common folders"))
                .padding([8, 16])
                .style(theme::secondary_button(accent, txt))
                .on_press(Message::ApplyPreset),
            button(text("+ Add folders"))
                .padding([8, 16])
                .style(theme::secondary_button(accent, txt))
                .on_press(Message::AddFolderClicked),
        ]
        .spacing(10);

        // Narrow windows: stack title above buttons so labels never overflow.
        let header_row: Element<'_, Message> = if self.window_width >= 700.0 {
            row![
                text("Folders to back up").size(20),
                space().width(Length::Fill),
                buttons,
            ]
            .align_y(iced::Alignment::Center)
            .spacing(12)
            .into()
        } else {
            column![text("Folders to back up").size(20), buttons]
                .spacing(8)
                .into()
        };

        let mut col = column![header_row, scrolled]
            .spacing(14)
            .height(Length::Fill);

        // Notice (duplicate folder, etc.)
        if let Some(notice) = &self.notice {
            col = col.push(
                text(notice.clone())
                    .size(13)
                    .color(Color::from_rgb8(0xd9, 0xa0, 0x5b)),
            );
        }

        col.into()
    }

    /// Right panel: remote selector + status that depends on the phase.
    fn status_panel(&self) -> Element<'_, Message> {
        // Remote selector.
        let remote_row: Element<'_, Message> = if self.remotes.is_empty() {
            text("").into()
        } else {
            let selected = if self.remote.is_empty() {
                None
            } else {
                Some(self.remote.clone())
            };
            row![
                text("Cloud:").size(15),
                pick_list(self.remotes.clone(), selected, Message::RemoteSelected),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .into()
        };

        let status: Element<'_, Message> = match &self.phase {
            Phase::Checking => text("Checking your backup setup...").size(16).into(),

            Phase::NeedsRecalc => column![
                text("Folders changed.").size(16),
                text("Recalculate the backup size to continue.")
                    .size(13)
                    .color(Color::from_rgb8(0x9a, 0x8f, 0x95)),
                button(text("Recalculate size")).on_press(Message::RecalcSize),
            ]
            .spacing(10)
            .into(),

            Phase::Ready { result } => match result {
                PreflightResult::NoConfig(msg) => column![
                    text("No configuration found").size(20),
                    text(msg.clone()).size(13),
                ]
                .spacing(8)
                .into(),
                PreflightResult::Failed(msg) => column![
                    text("Preflight failed").size(20),
                    text(msg.clone()).size(13),
                ]
                .spacing(8)
                .into(),
                PreflightResult::Ready { report } => {
                    let mut c = column![
                        text(format!(
                            "Backup size: {}",
                            human_bytes(report.backup_size_bytes)
                        ))
                        .size(16),
                    ]
                    .spacing(8);
                    match &report.space {
                        SpaceStatus::Fits { free_bytes } => {
                            c = c.push(
                                text(format!("Fits — {} free.", human_bytes(*free_bytes)))
                                    .size(15)
                                    .color(self.preset.accent()),
                            );
                        }
                        SpaceStatus::Shortfall { .. } => {
                            c = c.push(text("Not enough space — preparing options...").size(15));
                        }
                        SpaceStatus::Unknown => {
                            c = c.push(
                                text("Free space unknown — a full backup will be attempted.")
                                    .size(15),
                            );
                        }
                    }
                    if self.preflight_stale {
                        c = c.push(
                            text("Folders changed — Recalculate size.")
                                .size(12)
                                .color(Color::from_rgb8(0xd9, 0xa0, 0x5b)),
                        );
                    }
                    c = c.push(button(text("Recalculate size")).on_press(Message::RecalcSize));
                    c.into()
                }
            },

            Phase::Measuring => column![
                text("Not enough space for everything.").size(16),
                text("Measuring your folders...").size(14),
            ]
            .spacing(8)
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
                    text(format!("Backing up '{current}'...")).size(18),
                    text(format!(
                        "{} of {} folders — {:.0}%",
                        done,
                        sources.len(),
                        pct
                    ))
                    .size(14),
                    progress_bar(0.0..=1.0, self.displayed_progress).length(Length::Fill),
                ]
                .spacing(14)
                .into()
            }

            Phase::Finished(outcome) => {
                let summary: Element<'_, Message> = match outcome {
                    BackupOutcome::FullVerified => text("✓ Full backup completed and verified.")
                        .size(20)
                        .into(),
                    BackupOutcome::PartialVerified => {
                        text("✓ Partial backup completed and verified.")
                            .size(20)
                            .into()
                    }
                    BackupOutcome::Failed(msg) => {
                        column![text("✗ Backup failed").size(20), text(msg.clone()).size(13),]
                            .spacing(8)
                            .into()
                    }
                };
                column![
                    summary,
                    button(text("Back to start")).on_press(Message::BackToStart),
                ]
                .spacing(16)
                .into()
            }
        };

        container(column![remote_row, status].spacing(20))
            .style(theme::panel(self.preset.surface()))
            .padding(20.0)
            .width(Length::Fill)
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
            text(format!("Selected: {} — fits.", human_bytes(total)))
                .size(15)
                .color(self.preset.accent())
        } else {
            text(format!(
                "Selected: {} — over by {}.",
                human_bytes(total),
                human_bytes(total - free_bytes)
            ))
            .size(15)
            .color(Color::from_rgb8(0xd9, 0xa0, 0x5b))
        };

        let back_up_btn = if fits && total > 0 {
            button(text("Back up selected")).on_press(Message::StartPartial)
        } else {
            button(text("Back up selected"))
        };

        column![
            text(format!("Choose folders — {} free", human_bytes(free_bytes))).size(18),
            scrolled,
            status_line,
            row![
                button(text("Auto-fill (smallest-first)")).on_press(Message::AutoFill),
                back_up_btn,
            ]
            .spacing(12),
        ]
        .spacing(14)
        .into()
    }

    /// Fixed footer: power-off toggle (left) and primary action (right).
    fn footer(&self) -> Element<'_, Message> {
        let action: Element<'_, Message> = match &self.phase {
            Phase::Ready {
                result: PreflightResult::Ready { report },
            } => match &report.space {
                SpaceStatus::Fits { .. } | SpaceStatus::Unknown => {
                    button(text("Back up now").size(16))
                        .padding([12, 28])
                        .style(theme::primary_button(self.preset.accent()))
                        .on_press(Message::StartBackup)
                        .into()
                }
                SpaceStatus::Shortfall { .. } => text("").into(),
            },
            _ => text("").into(),
        };

        let power = checkbox(self.power_off)
            .label("Power off after a successful backup")
            .on_toggle(Message::PowerOffToggled);

        container(
            row![power, space().width(Length::Fill), action]
                .align_y(iced::Alignment::Center)
                .spacing(16),
        )
        .width(Length::Fill)
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
        self.preflight_stale = true;
        match self.phase {
            // If mid-decision or measuring, drop back to a recalculable state.
            Phase::Choosing { .. } | Phase::Measuring => {
                self.phase = Phase::NeedsRecalc;
            }
            _ => {}
        }
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
