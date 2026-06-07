//! nightjar graphical interface (iced).
//!
//! 7-i: minimal skeleton — opens a dark-themed window showing the title,
//! proving the iced application structure compiles and renders on this
//! machine before any logic is added.

use iced::widget::{column, container, text};
use iced::{Element, Length, Theme};

/// The application state.
#[derive(Default)]
struct App {
    // Empty for now; state grows as we add features.
}

/// Events the application reacts to. Empty for the skeleton.
#[derive(Debug, Clone)]
enum Message {}

impl App {
    /// Update logic: handle a message, optionally returning async work.
    /// No messages exist yet, so this is a no-op.
    fn update(&mut self, _message: Message) {
        // Nothing to handle yet.
    }

    /// View logic: build the UI from the current state.
    fn view(&self) -> Element<'_, Message> {
        let content = column![
            text("nightjar").size(40),
            text("A backup tool that runs while you sleep.").size(16),
        ]
        .spacing(12);

        container(content)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(40)
            .into()
    }

    /// The theme: dark, to match the intended aesthetic.
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
