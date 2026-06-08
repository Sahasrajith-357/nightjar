//! Theme presets for nightjar. Each preset is a name + an iced Palette;
//! iced derives the full extended palette (hover/press shades, etc.) from it.

use iced::theme::Palette;
use iced::{Color, Theme};

/// A named color preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Ember,
    Midnight,
    Mono,
    Forest,
}

impl Preset {
    /// All presets, in display order.
    pub const ALL: [Preset; 4] = [
        Preset::Ember,
        Preset::Midnight,
        Preset::Mono,
        Preset::Forest,
    ];

    /// The display name (also used for persistence).
    pub fn name(&self) -> &'static str {
        match self {
            Preset::Ember => "Ember",
            Preset::Midnight => "Midnight",
            Preset::Mono => "Mono",
            Preset::Forest => "Forest",
        }
    }

    /// Parse from a stored name; defaults to Ember if unrecognized.
    pub fn from_name(s: &str) -> Preset {
        match s {
            "Midnight" => Preset::Midnight,
            "Mono" => Preset::Mono,
            "Forest" => Preset::Forest,
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
        }
    }

    /// A muted/secondary text color for taglines and hints.
    pub fn muted(&self) -> Color {
        match self {
            Preset::Ember => Color::from_rgb8(0x9a, 0x8f, 0x95),
            Preset::Midnight => Color::from_rgb8(0x7d, 0x8a, 0x99),
            Preset::Mono => Color::from_rgb8(0x8a, 0x8a, 0x90),
            Preset::Forest => Color::from_rgb8(0x88, 0x96, 0x88),
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
        };
        Theme::custom(format!("nightjar-{}", self.name()), palette)
    }
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
