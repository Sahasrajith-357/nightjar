//! Locating, loading, and saving nightjar's configuration file.
//!
//! The config lives at the platform's standard config location
//! (on Linux: ~/.config/nightjar/config.toml). Every operation that can
//! fail returns `Result`, so callers must handle missing files, permission
//! problems, and malformed TOML explicitly.

use crate::Result;
use crate::config::Config;
use crate::error::Error;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

/// Returns the full path to nightjar's config file, creating the parent
/// directory if it does not yet exist.
///
/// Errors if the OS can't tell us a config directory, or if we can't
/// create the directory (e.g. permissions).
pub fn config_path() -> Result<PathBuf> {
    // "" qualifier, "" organization, "nightjar" application → ~/.config/nightjar
    let dirs = ProjectDirs::from("", "", "nightjar").ok_or_else(|| {
        Error::ConfigError("could not determine the OS config directory".to_string())
    })?;

    let dir = dirs.config_dir();

    // Ensure the directory exists. create_dir_all is a no-op if it's there
    // already, and an std::io::Error (→ Error::Io via #[from]) if it fails.
    fs::create_dir_all(dir)?;

    Ok(dir.join("config.toml"))
}

/// Loads the configuration from the given path.
///
/// Errors if the file is missing/unreadable (Io) or if the contents are
/// not valid TOML / don't match the Config shape (ConfigError, with a
/// human-readable explanation).
pub fn load_from(path: &PathBuf) -> Result<Config> {
    // Read the file to a string. A missing file or permission error becomes
    // Error::Io automatically through the `?` operator.
    let contents = fs::read_to_string(path)?;

    // Parse the TOML into our Config. toml::from_str returns toml's own
    // error type, so we convert it explicitly into a clear ConfigError.
    let config: Config = toml::from_str(&contents)
        .map_err(|e| Error::ConfigError(format!("invalid config file: {e}")))?;

    Ok(config)
}

/// Saves the configuration to the given path, overwriting any existing file.
///
/// Errors if the Config can't be serialized (ConfigError) or the file can't
/// be written (Io).
pub fn save_to(config: &Config, path: &PathBuf) -> Result<()> {
    // Serialize the Config to a pretty TOML string. Conversion of toml's
    // error type into our ConfigError is explicit, as above.
    let toml_string = toml::to_string_pretty(config)
        .map_err(|e| Error::ConfigError(format!("could not serialize config: {e}")))?;

    // Write the string to disk. A write/permission failure becomes Error::Io.
    fs::write(path, toml_string)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Source;
    use std::path::PathBuf;

    #[test]
    fn save_then_load_roundtrip() {
        // Build a sample config.
        let original = Config {
            remote: "cloud".to_string(),
            destination_path: "NightjarBackup".to_string(),
            sources: vec![
                Source {
                    name: "Documents".to_string(),
                    path: PathBuf::from("/home/test/Documents"),
                },
                Source {
                    name: "Pictures".to_string(),
                    path: PathBuf::from("/home/test/Pictures"),
                },
            ],
            verify: true,
            excludes: vec!["**/.git/**".to_string()],
            theme: None,
        };

        // Use a temp dir so we never touch the real config.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("config.toml");

        // Save, then load back.
        save_to(&original, &path).expect("save failed");
        let loaded = load_from(&path).expect("load failed");

        // The loaded config must exactly equal what we saved.
        assert_eq!(original, loaded);
    }

    #[test]
    fn loading_missing_file_errors() {
        let missing = PathBuf::from("/nonexistent/path/config.toml");
        let result = load_from(&missing);
        assert!(result.is_err(), "loading a missing file should fail");
    }

    #[test]
    fn loading_malformed_toml_errors() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is = not valid toml [[[").expect("write failed");

        let result = load_from(&path);
        assert!(result.is_err(), "loading malformed TOML should fail");
    }
}
