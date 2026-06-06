//! Core engine for nightjar: the backup logic, shared by the CLI and GUI.

pub mod config;
pub mod config_io;
pub mod error;
pub mod preflight;
pub mod rclone;

pub use config::Config;
pub use error::Error;

/// Convenient alias so functions in this crate can return `Result<T>`
/// instead of writing `Result<T, error::Error>` everywhere.
pub type Result<T> = std::result::Result<T, Error>;
