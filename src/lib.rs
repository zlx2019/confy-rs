//! Simple, efficient configuration loading for Rust.
//!
//! `confy-rs` loads configuration from multiple file formats, merges layered
//! sources and deserializes the result into your own types via `serde`. With
//! the `watch` feature it also hot-reloads the configuration when source
//! files change on disk.
//!
//! # Highlights
//!
//! - **Multi-format**: TOML / YAML / JSON, detected from the file extension
//! - **Layered**: defaults + any number of files, deep-merged in order
//! - **Hot reload**: debounced cross-platform file watching; readers are
//!   lock-free and the previous config is kept when a reload fails
//! - **Lean**: format parsers and the watcher are feature-gated
//!
//! # Quick start
//!
//! ```
//! use serde::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! struct AppConfig {
//!     name: String,
//!     port: u16,
//! }
//!
//! # fn main() -> confy_rs::Result<()> {
//! let config: AppConfig = confy_rs::load_str(
//!     r#"
//!     name = "demo"
//!     port = 8080
//!     "#,
//!     confy_rs::Format::Toml,
//! )?;
//! assert_eq!(config.port, 8080);
//! # Ok(())
//! # }
//! ```
//!
//! Loading from files works the same way — the format is detected from the
//! extension, and later sources override earlier ones:
//!
//! ```no_run
//! # use serde::Deserialize;
//! # #[derive(Deserialize)]
//! # struct AppConfig { port: u16 }
//! # fn main() -> confy_rs::Result<()> {
//! // One-liner for the common case
//! let config: AppConfig = confy_rs::load("config.toml")?;
//!
//! // Layered loading
//! let config: AppConfig = confy_rs::builder()
//!     .file("config/default.toml")
//!     .file_optional("config/local.json")
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Hot reloading
//!
//! ```no_run
//! # use serde::Deserialize;
//! # #[derive(Deserialize)]
//! # struct AppConfig { port: u16 }
//! # fn main() -> confy_rs::Result<()> {
//! let watcher = confy_rs::builder()
//!     .file("config.toml")
//!     .watch::<AppConfig>()?;
//!
//! watcher.on_change(|config| println!("port is now {}", config.port));
//! let current = watcher.get(); // lock-free snapshot, callable anywhere
//! # Ok(())
//! # }
//! ```
//!
//! Reloads are debounced, editors that save via atomic rename are handled,
//! and when a reload fails the previous configuration stays active while the
//! error is reported through [`ConfigWatcher::on_error`].
//!
//! # Cargo features
//!
//! | Feature | Default | Effect |
//! |---------|---------|--------|
//! | `toml`  | yes     | TOML support via the `toml` crate |
//! | `yaml`  | no      | YAML support via `serde_yaml_ng` |
//! | `watch` | yes     | hot reloading via `notify` + `arc-swap` |
//!
//! JSON support is always built in.

mod builder;
mod error;
mod source;
#[cfg(feature = "watch")]
mod watch;

use std::path::Path;

use serde::de::DeserializeOwned;

pub use crate::builder::ConfigBuilder;
pub use crate::error::Error;
pub use crate::source::Format;
#[cfg(feature = "watch")]
pub use crate::watch::ConfigWatcher;

/// Convenience alias for `std::result::Result<T, confy_rs::Error>`.
pub type Result<T> = std::result::Result<T, Error>;

/// Creates a new empty [`ConfigBuilder`].
#[must_use]
pub fn builder() -> ConfigBuilder {
    ConfigBuilder::new()
}

/// Loads a single configuration file and deserializes it into `T`.
///
/// The format is detected from the file extension. This is shorthand for
/// `confy_rs::builder().file(path).build()`.
///
/// # Errors
///
/// Returns an error if the file is missing or unreadable, its format is
/// unknown or disabled, its content is invalid, or the parsed tree does not
/// match `T`.
///
/// # Example
///
/// ```no_run
/// # fn main() -> confy_rs::Result<()> {
/// let config: serde_json::Value = confy_rs::load("config.toml")?;
/// # Ok(())
/// # }
/// ```
pub fn load<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    builder().file(path.as_ref()).build()
}

/// Parses configuration from an in-memory string in the given format.
///
/// Handy for tests and for configuration that is embedded or fetched from
/// somewhere other than the file system.
///
/// # Errors
///
/// Returns an error if the format is disabled, the content is invalid, or
/// the parsed tree does not match `T`.
pub fn load_str<T: DeserializeOwned>(text: &str, format: Format) -> Result<T> {
    let value = format.parse(text, None)?;
    serde_json::from_value(value).map_err(Error::Deserialize)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, PartialEq, Deserialize)]
    struct Sample {
        name: String,
        port: u16,
    }

    #[test]
    fn load_str_json() {
        let config: Sample = load_str(r#"{ "name": "demo", "port": 8080 }"#, Format::Json).unwrap();
        assert_eq!(
            config,
            Sample {
                name: "demo".into(),
                port: 8080
            }
        );
    }

    /// load_str: TOML
    #[cfg(feature = "toml")]
    #[test]
    fn load_str_toml() {
        let config: Sample = load_str("name = \"demo\"\nport = 8080\n", Format::Toml).unwrap();
        assert_eq!(config.port, 8080);
    }

    /// load_str: YAML
    #[cfg(feature = "yaml")]
    #[test]
    fn load_str_yaml() {
        let config: Sample = load_str("name: demo\nport: 8080\n", Format::Yaml).unwrap();
        assert_eq!(config.port, 8080);
    }

    #[cfg(not(feature = "yaml"))]
    #[test]
    fn load_str_yaml_disabled() {
        let err = load_str::<Sample>("name: demo", Format::Yaml).unwrap_err();
        assert!(matches!(err, Error::FeatureDisabled { .. }), "{err}");
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        std::fs::write(&path, r#"{ "name": "demo", "port": 1 }"#).unwrap();
        let config: Sample = load(&path).unwrap();
        assert_eq!(config.port, 1);
    }
}
