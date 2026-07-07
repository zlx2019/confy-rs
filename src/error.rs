//! Error types returned by this crate.

use std::path::PathBuf;

use crate::source::Format;

/// All errors that can occur while loading or watching configuration.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A required configuration file does not exist.
    #[error("configuration file not found: {}", .path.display())]
    NotFound {
        /// Path of the missing file.
        path: PathBuf,
    },

    /// Reading a configuration file failed for I/O reasons other than absence.
    #[error("failed to read configuration file {}: {source}", .path.display())]
    Io {
        /// Path of the file that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The file extension does not map to any known configuration format.
    #[error("cannot detect configuration format of `{}` (expected .toml, .yaml, .yml or .json)", .path.display())]
    UnknownExtension {
        /// Path whose extension could not be recognized.
        path: PathBuf,
    },

    /// The format is recognized but its cargo feature is not enabled.
    #[error("{format} support is disabled; enable the `{feature}` feature of confy-rs")]
    FeatureDisabled {
        /// The requested format.
        format: Format,
        /// Name of the cargo feature that must be enabled.
        feature: &'static str,
    },

    /// The content is not valid for its format.
    #[error("failed to parse {} as {format}: {message}", display_origin(.origin))]
    Parse {
        /// Path of the offending file, or `None` when parsing from a string.
        origin: Option<PathBuf>,
        /// The format that was being parsed.
        format: Format,
        /// Parser error message, including position info when available.
        message: String,
    },

    /// The merged configuration does not match the target type.
    #[error("failed to deserialize configuration: {0}")]
    Deserialize(#[source] serde_json::Error),

    /// Serializing the defaults layer failed.
    #[error("failed to serialize defaults layer: {0}")]
    Defaults(#[source] serde_json::Error),

    /// Setting up the file watcher failed.
    #[cfg(feature = "watch")]
    #[error("failed to watch configuration files: {0}")]
    Watch(#[from] notify::Error),
}

fn display_origin(origin: &Option<PathBuf>) -> String {
    match origin {
        Some(path) => path.display().to_string(),
        None => "inline string".to_string(),
    }
}
