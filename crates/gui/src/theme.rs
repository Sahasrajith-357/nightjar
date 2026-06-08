//! Theme presets for nightjar. Each preset is a name + an iced Palette;
//! iced derives the full extended palette (hover/press shades, etc.) from it.

use iced::theme::Palette;
use iced::widget::{button, container};
use iced::{Background, Border};
use iced::{Color, Theme};

/// A named color preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Ember,
    Midnight,
    Mono,
    Forest,
    NeonCyan,
    NeonMagenta,
    NeonGreen,
}

impl Preset {
    /// All presets, in display order.
    pub const ALL: [Preset; 7] = [
        Preset::Ember,
        Preset::Midnight,
        Preset::Mono,
        Preset::Forest,
        Preset::NeonCyan,
        Preset::NeonMagenta,
        Preset::NeonGreen,
    ];

    /// The display name (also used for persistence).
    pub fn name(&self) -> &'static str {
        match self {
            Preset::Ember => "Ember",
            Preset::Midnight => "Midnight",
            Preset::Mono => "Mono",
            Preset::Forest => "Forest",
            Preset::NeonCyan => "Neon Cyan",
            Preset::NeonMagenta => "Neon Magenta",
            Preset::NeonGreen => "Neon Green",
        }
    }

    /// Parse from a stored name; defaults to Ember if unrecognized.
    pub fn from_name(s: &str) -> Preset {
        match s {
            "Midnight" => Preset::Midnight,
            "Mono" => Preset::Mono,
            "Forest" => Preset::Forest,
            "Neon Cyan" => Preset::NeonCyan,
            "Neon Magenta" => Preset::NeonMagenta,
            "Neon Green" => Preset::NeonGreen,
            _ => Preset::Ember,
        }
    }

    /// The accent color (used for the title and primary actions).
    pub fn accent(&self) -> Color {
        match self {
            Preset::Ember => Color::from_rgb8(0xeb, 0x96, 0x7c), // coral
            Preset::Midnight => Color::from_rgb8(0x6f, 0xb3, 0xd6), // cool cyan-blue
            Preset::Mono => Color::from_rgb8(0xcf, 0xcf, 0xd4),  // soft white
            Preset::Forest => Color::from_rgb8(0x8f, 0xc7, 0x8f), // sage green
            Preset::NeonCyan => Color::from_rgb8(0x00, 0xf0, 0xff),
            Preset::NeonMagenta => Color::from_rgb8(0xff, 0x2d, 0x95),
            Preset::NeonGreen => Color::from_rgb8(0x39, 0xff, 0x14),
        }
    }

    /// A muted/secondary text color for taglines and hints.
    pub fn muted(&self) -> Color {
        match self {
            Preset::Ember => Color::from_rgb8(0x9a, 0x8f, 0x95),
            Preset::Midnight => Color::from_rgb8(0x7d, 0x8a, 0x99),
            Preset::Mono => Color::from_rgb8(0x8a, 0x8a, 0x90),
            Preset::Forest => Color::from_rgb8(0x88, 0x96, 0x88),
            Preset::NeonCyan => Color::from_rgb8(0x5a, 0x7a, 0x82),
            Preset::NeonMagenta => Color::from_rgb8(0x82, 0x5a, 0x6e),
            Preset::NeonGreen => Color::from_rgb8(0x5a, 0x82, 0x5a),
        }
    }

    /// Build the iced Theme for this preset.
    pub fn theme(&self) -> Theme {
        let palette = match self {
            Preset::Ember => Palette {
                background: Color::from_rgb8(0x16, 0x13, 0x1a),
                text: Color::from_rgb8(0xc9, 0xbf, 0xc4),
                primary: Color::from_rgb8(0xeb, 0x96, 0x7c),
                success: Color::from_rgb8(0xeb, 0x96, 0x7c),
                warning: Color::from_rgb8(0xd9, 0xa0, 0x5b),
                danger: Color::from_rgb8(0x74, 0x19, 0x24),
            },
            Preset::Midnight => Palette {
                background: Color::from_rgb8(0x0e, 0x12, 0x1a),
                text: Color::from_rgb8(0xc4, 0xcc, 0xd6),
                primary: Color::from_rgb8(0x6f, 0xb3, 0xd6),
                success: Color::from_rgb8(0x5e, 0xc8, 0xa8),
                warning: Color::from_rgb8(0xd6, 0xb0, 0x6f),
                danger: Color::from_rgb8(0xc8, 0x5e, 0x6e),
            },
            Preset::Mono => Palette {
                background: Color::from_rgb8(0x14, 0x14, 0x16),
                text: Color::from_rgb8(0xcf, 0xcf, 0xd4),
                primary: Color::from_rgb8(0xcf, 0xcf, 0xd4),
                success: Color::from_rgb8(0xa8, 0xc8, 0xa8),
                warning: Color::from_rgb8(0xd0, 0xc0, 0x90),
                danger: Color::from_rgb8(0xc8, 0x8a, 0x8a),
            },
            Preset::Forest => Palette {
                background: Color::from_rgb8(0x10, 0x16, 0x12),
                text: Color::from_rgb8(0xc6, 0xcf, 0xc6),
                primary: Color::from_rgb8(0x8f, 0xc7, 0x8f),
                success: Color::from_rgb8(0x8f, 0xc7, 0x8f),
                warning: Color::from_rgb8(0xd0, 0xc0, 0x80),
                danger: Color::from_rgb8(0xc8, 0x7e, 0x6e),
            },
            Preset::NeonCyan => Palette {
                background: Color::from_rgb8(0x0a, 0x0a, 0x0c),
                text: Color::from_rgb8(0xb8, 0xc8, 0xcc),
                primary: Color::from_rgb8(0x00, 0xf0, 0xff),
                success: Color::from_rgb8(0x00, 0xf0, 0xff),
                warning: Color::from_rgb8(0xff, 0xd0, 0x4d),
                danger: Color::from_rgb8(0xff, 0x4d, 0x6e),
            },
            Preset::NeonMagenta => Palette {
                background: Color::from_rgb8(0x0a, 0x08, 0x0a),
                text: Color::from_rgb8(0xcc, 0xb8, 0xc4),
                primary: Color::from_rgb8(0xff, 0x2d, 0x95),
                success: Color::from_rgb8(0x4d, 0xff, 0xb0),
                warning: Color::from_rgb8(0xff, 0xd0, 0x4d),
                danger: Color::from_rgb8(0xff, 0x4d, 0x4d),
            },
            Preset::NeonGreen => Palette {
                background: Color::from_rgb8(0x08, 0x0a, 0x08),
                text: Color::from_rgb8(0xbc, 0xcc, 0xbc),
                primary: Color::from_rgb8(0x39, 0xff, 0x14),
                success: Color::from_rgb8(0x39, 0xff, 0x14),
                warning: Color::from_rgb8(0xe0, 0xff, 0x4d),
                danger: Color::from_rgb8(0xff, 0x5e, 0x4d),
            },
        };
        Theme::custom(format!("nightjar-{}", self.name()), palette)
    }

    /// A surface color for cards/panels (slightly lifted from background).
    pub fn surface(&self) -> Color {
        match self {
            Preset::Ember => Color::from_rgb8(0x20, 0x1b, 0x26),
            Preset::Midnight => Color::from_rgb8(0x16, 0x1c, 0x28),
            Preset::Mono => Color::from_rgb8(0x1e, 0x1e, 0x22),
            Preset::Forest => Color::from_rgb8(0x18, 0x20, 0x1a),
            Preset::NeonCyan => Color::from_rgb8(0x0c, 0x10, 0x12),
            Preset::NeonMagenta => Color::from_rgb8(0x12, 0x0c, 0x10),
            Preset::NeonGreen => Color::from_rgb8(0x0c, 0x12, 0x0c),
        }
    }
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Primary action button: filled accent, rounded, hover/press feedback.
pub fn primary_button(accent: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let base = button::Style {
            background: Some(Background::Color(accent)),
            text_color: Color::from_rgb8(0x16, 0x13, 0x1a),
            border: Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        match status {
            button::Status::Hovered => button::Style {
                background: Some(Background::Color(lighten(accent, 0.12))),
                ..base
            },
            button::Status::Pressed => button::Style {
                background: Some(Background::Color(darken(accent, 0.12))),
                ..base
            },
            button::Status::Disabled => button::Style {
                background: Some(Background::Color(with_alpha(accent, 0.35))),
                text_color: with_alpha(Color::from_rgb8(0x16, 0x13, 0x1a), 0.5),
                ..base
            },
            _ => base,
        }
    }
}

/// Secondary button: subtle outline, accent text, fills faintly on hover.
pub fn secondary_button(
    accent: Color,
    text: Color,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let base = button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: text,
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: with_alpha(accent, 0.5),
            },
            ..Default::default()
        };
        match status {
            button::Status::Hovered => button::Style {
                background: Some(Background::Color(with_alpha(accent, 0.12))),
                text_color: accent,
                ..base
            },
            button::Status::Pressed => button::Style {
                background: Some(Background::Color(with_alpha(accent, 0.20))),
                ..base
            },
            button::Status::Disabled => button::Style {
                text_color: with_alpha(text, 0.35),
                border: Border {
                    radius: 10.0.into(),
                    width: 1.0,
                    color: with_alpha(accent, 0.2),
                },
                ..base
            },
            _ => base,
        }
    }
}

/// Minimal remove (✕) button: faint, danger-tinted on hover.
pub fn remove_button(text: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let base = button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: with_alpha(text, 0.6),
            border: Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        match status {
            button::Status::Hovered => button::Style {
                background: Some(Background::Color(with_alpha(
                    Color::from_rgb8(0xc8, 0x5e, 0x6e),
                    0.18,
                ))),
                text_color: Color::from_rgb8(0xe0, 0x8a, 0x8a),
                ..base
            },
            _ => base,
        }
    }
}

/// Subtle rounded panel/card background.
pub fn panel(bg: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(with_alpha(bg, 0.45))),
        border: Border {
            radius: 14.0.into(),
            width: 1.0,
            color: with_alpha(Color::WHITE, 0.04),
        },
        ..Default::default()
    }
}

// --- small color helpers ---

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}
fn lighten(c: Color, amt: f32) -> Color {
    Color {
        r: (c.r + amt).min(1.0),
        g: (c.g + amt).min(1.0),
        b: (c.b + amt).min(1.0),
        a: c.a,
    }
}
fn darken(c: Color, amt: f32) -> Color {
    Color {
        r: (c.r - amt).max(0.0),
        g: (c.g - amt).max(0.0),
        b: (c.b - amt).max(0.0),
        a: c.a,
    }
}
