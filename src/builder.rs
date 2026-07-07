//! Layered configuration builder.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::Error;
use crate::source::{self, Format};

/// A builder that loads configuration from multiple layered sources.
///
/// Sources are merged in the order they are added: later sources override
/// earlier ones. Objects are merged recursively; scalars, arrays and `null`
/// replace the previous value entirely.
///
/// Create one via [`crate::builder()`] or [`ConfigBuilder::new`].
///
/// # Example
///
/// ```no_run
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct AppConfig {
///     name: String,
///     port: u16,
/// }
///
/// # fn main() -> confy_rs::Result<()> {
/// let config: AppConfig = confy_rs::builder()
///     .file("config/default.toml")
///     .file_optional("config/local.json")
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct ConfigBuilder {
    /// Configuration layers in the order they were added.
    layers: Vec<Layer>,
}

/// A single configuration layer.
#[derive(Debug, Clone)]
enum Layer {
    /// In-memory defaults layer (pre-serialized into the intermediate
    /// representation).
    Defaults(Value),
    /// File layer; `required` decides whether a missing file errors or is
    /// skipped.
    File { path: PathBuf, required: bool },
}

impl ConfigBuilder {
    /// Creates an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an in-memory defaults layer from any serializable value.
    ///
    /// This is typically the first layer, so that files added later only
    /// need to override the values that differ.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Defaults`] if the value cannot be serialized.
    pub fn defaults<D: Serialize>(mut self, defaults: D) -> Result<Self, Error> {
        // Serialize into the intermediate representation eagerly so errors
        // surface right here instead of at build time
        let value = serde_json::to_value(defaults).map_err(Error::Defaults)?;
        self.layers.push(Layer::Defaults(value));
        Ok(self)
    }

    /// Adds a required configuration file whose format is detected from the
    /// file extension.
    ///
    /// Loading fails with [`Error::NotFound`] if the file does not exist.
    #[must_use]
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.layers.push(Layer::File {
            path: path.into(),
            required: true,
        });
        self
    }

    /// Adds an optional configuration file whose format is detected from the
    /// file extension.
    ///
    /// A missing file is silently skipped; any other error (unreadable file,
    /// invalid syntax, ...) is still reported.
    #[must_use]
    pub fn file_optional(mut self, path: impl Into<PathBuf>) -> Self {
        self.layers.push(Layer::File {
            path: path.into(),
            required: false,
        });
        self
    }

    /// Loads all layers, merges them and deserializes the result into `T`.
    ///
    /// # Errors
    ///
    /// Returns an error if any layer fails to load or parse, or if the merged
    /// tree does not match `T` (see [`Error`]).
    pub fn build<T: DeserializeOwned>(self) -> Result<T, Error> {
        let merged = self.build_value()?;
        serde_json::from_value(merged).map_err(Error::Deserialize)
    }

    /// Runs the full loading pipeline: reads, parses and deep-merges every
    /// layer into a single Value tree. The hot-reload thread calls this
    /// repeatedly, hence `&self`.
    pub(crate) fn build_value(&self) -> Result<Value, Error> {
        let mut merged = Value::Object(serde_json::Map::new());
        for layer in &self.layers {
            match layer {
                Layer::Defaults(value) => source::merge(&mut merged, value.clone()),
                Layer::File { path, required } => {
                    // Detect the format first: an unknown extension is a
                    // structural error, reported even for a missing optional file
                    let format = Format::from_path(path)?;
                    let Some(text) = read_file(path, *required)? else {
                        continue;
                    };
                    let value = format.parse(&text, Some(path))?;
                    source::merge(&mut merged, value);
                }
            }
        }
        Ok(merged)
    }

    /// Returns `(path, required)` for every file layer, used by the watch
    /// module to derive its watch targets.
    #[cfg(feature = "watch")]
    pub(crate) fn file_paths(&self) -> Vec<(PathBuf, bool)> {
        self.layers
            .iter()
            .filter_map(|layer| match layer {
                Layer::File { path, required } => Some((path.clone(), *required)),
                Layer::Defaults(_) => None,
            })
            .collect()
    }
}

/// Reads a file's content; `required` decides how a missing file is handled:
/// a required file yields [`Error::NotFound`], an optional one yields `None`.
fn read_file(path: &Path, required: bool) -> Result<Option<String>, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if required {
                Err(Error::NotFound {
                    path: path.to_path_buf(),
                })
            } else {
                Ok(None)
            }
        }
        Err(err) => Err(Error::Io {
            path: path.to_path_buf(),
            source: err,
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Debug, PartialEq, Deserialize)]
    struct Sample {
        name: String,
        port: u16,
        #[serde(default)]
        debug: bool,
    }

    /// Layered override: defaults layer + JSON file layer.
    #[test]
    fn defaults_then_file_override() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        std::fs::write(&path, r#"{ "port": 9090 }"#).unwrap();

        let config: Sample = ConfigBuilder::new()
            .defaults(json!({ "name": "demo", "port": 1, "debug": true }))
            .unwrap()
            .file(&path)
            .build()
            .unwrap();

        assert_eq!(
            config,
            Sample {
                name: "demo".into(),
                port: 9090,
                debug: true
            }
        );
    }

    /// A missing required file should yield NotFound with the path in the
    /// message.
    #[test]
    fn missing_required_file_is_an_error() {
        let err = ConfigBuilder::new()
            .file("definitely/missing.json")
            .build::<Sample>()
            .unwrap_err();
        assert!(matches!(err, Error::NotFound { .. }), "{err}");
        assert!(err.to_string().contains("missing.json"), "{err}");
    }

    /// A missing optional file should be silently skipped.
    #[test]
    fn missing_optional_file_is_skipped() {
        let config: Sample = ConfigBuilder::new()
            .defaults(json!({ "name": "demo", "port": 1 }))
            .unwrap()
            .file_optional("definitely/missing.json")
            .build()
            .unwrap();
        assert_eq!(config.port, 1);
    }

    /// An unknown extension should error even on an optional file (catches
    /// typos).
    #[test]
    fn unknown_extension_is_reported_even_for_optional_file() {
        let err = ConfigBuilder::new()
            .file_optional("missing.conf")
            .build::<serde_json::Value>()
            .unwrap_err();
        assert!(matches!(err, Error::UnknownExtension { .. }), "{err}");
    }

    /// A type mismatch should yield a Deserialize error.
    #[test]
    fn type_mismatch_is_a_deserialize_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        std::fs::write(&path, r#"{ "name": "demo", "port": "not-a-number" }"#).unwrap();

        let err = ConfigBuilder::new()
            .file(&path)
            .build::<Sample>()
            .unwrap_err();
        assert!(matches!(err, Error::Deserialize(_)), "{err}");
    }
}
