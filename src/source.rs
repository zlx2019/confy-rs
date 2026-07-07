//! Format detection, parsing and layered value merging.
//!

use std::fmt;
use std::path::Path;

use serde_json::Value;

use crate::error::Error;

/// Supported configuration file formats.
///
/// The format of a file is detected from its extension by default, see
/// [`Format::from_path`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// JSON (`.json`). Always available.
    Json,
    /// TOML (`.toml`). Requires the `toml` feature (enabled by default).
    Toml,
    /// YAML (`.yaml` / `.yml`). Requires the `yaml` feature.
    Yaml,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Json => f.write_str("JSON"),
            Format::Toml => f.write_str("TOML"),
            Format::Yaml => f.write_str("YAML"),
        }
    }
}

impl Format {
    /// Detects the format of a file from its extension (case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownExtension`] when the extension is missing or
    /// not one of `.toml`, `.yaml`, `.yml`, `.json`.
    pub fn from_path(path: &Path) -> Result<Self, Error> {
        // Extract the extension and lowercase it before matching
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("json") => Ok(Format::Json),
            Some("toml") => Ok(Format::Toml),
            Some("yaml" | "yml") => Ok(Format::Yaml),
            _ => Err(Error::UnknownExtension {
                path: path.to_path_buf(),
            }),
        }
    }

    /// Parses text in this format into the unified JSON intermediate
    /// representation. `origin` is only used to point at the source in
    /// error messages (`None` means the text came from a string).
    pub(crate) fn parse(self, text: &str, origin: Option<&Path>) -> Result<Value, Error> {
        match self {
            Format::Json => parse_json(text, origin),
            Format::Toml => parse_toml(text, origin),
            Format::Yaml => parse_yaml(text, origin),
        }
    }
}

/// Deep-merges two config trees, applying `overlay` on top of `base`.
///
/// Merge semantics:
/// - when both sides are objects, merge recursively per key;
/// - otherwise (scalar / array / null) the overlay replaces the value.
pub(crate) fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(slot) => merge(slot, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (slot, value) => *slot = value,
    }
}

/// Builds a parse error carrying the origin and format context.
fn parse_error(origin: Option<&Path>, format: Format, message: String) -> Error {
    Error::Parse {
        origin: origin.map(Path::to_path_buf),
        format,
        message,
    }
}

/// Parses JSON text.
fn parse_json(text: &str, origin: Option<&Path>) -> Result<Value, Error> {
    serde_json::from_str(text).map_err(|err| parse_error(origin, Format::Json, err.to_string()))
}

/// Parses TOML text and converts it to the JSON intermediate representation.
#[cfg(feature = "toml")]
fn parse_toml(text: &str, origin: Option<&Path>) -> Result<Value, Error> {
    let value: toml::Value = toml::from_str(text)
        .map_err(|err: toml::de::Error| parse_error(origin, Format::Toml, err.to_string()))?;
    toml_to_json(value).map_err(|reason| parse_error(origin, Format::Toml, reason))
}

/// Fallback used when the `toml` feature is disabled.
#[cfg(not(feature = "toml"))]
fn parse_toml(_text: &str, _origin: Option<&Path>) -> Result<Value, Error> {
    Err(Error::FeatureDisabled {
        format: Format::Toml,
        feature: "toml",
    })
}

/// Parses YAML text and converts it to the JSON intermediate representation.
#[cfg(feature = "yaml")]
fn parse_yaml(text: &str, origin: Option<&Path>) -> Result<Value, Error> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)
        .map_err(|err| parse_error(origin, Format::Yaml, err.to_string()))?;
    yaml_to_json(value).map_err(|reason| parse_error(origin, Format::Yaml, reason))
}

/// Fallback used when the `yaml` feature is disabled.
#[cfg(not(feature = "yaml"))]
fn parse_yaml(_text: &str, _origin: Option<&Path>) -> Result<Value, Error> {
    Err(Error::FeatureDisabled {
        format: Format::Yaml,
        feature: "yaml",
    })
}

/// Converts a `toml::Value` into a `serde_json::Value`.
///
/// Bypassing the serde bridge is deliberate: the bridge encodes datetimes
/// as an internal `$__toml_private_datetime` marker map, so the conversion
/// is done by hand and datetimes become their string form (e.g. RFC 3339).
#[cfg(feature = "toml")]
fn toml_to_json(value: toml::Value) -> Result<Value, String> {
    Ok(match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => Value::Number(
            // JSON numbers cannot represent nan / inf
            serde_json::Number::from_f64(f)
                .ok_or_else(|| format!("non-finite float `{f}` is not supported"))?,
        ),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(toml_to_json)
                .collect::<Result<_, _>>()?,
        ),
        toml::Value::Table(table) => {
            let mut object = serde_json::Map::with_capacity(table.len());
            for (key, value) in table {
                object.insert(key, toml_to_json(value)?);
            }
            Value::Object(object)
        }
    })
}

/// Converts a `serde_yaml_ng::Value` into a `serde_json::Value`.
#[cfg(feature = "yaml")]
fn yaml_to_json(value: serde_yaml_ng::Value) -> Result<Value, String> {
    use serde_yaml_ng::Value as Yaml;

    Ok(match value {
        Yaml::Null => Value::Null,
        Yaml::Bool(b) => Value::Bool(b),
        Yaml::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                // JSON numbers cannot represent .nan / .inf
                Value::Number(
                    serde_json::Number::from_f64(f)
                        .ok_or_else(|| format!("non-finite float `{f}` is not supported"))?,
                )
            } else {
                return Err(format!("unrepresentable YAML number `{n}`"));
            }
        }
        Yaml::String(s) => Value::String(s),
        Yaml::Sequence(items) => Value::Array(
            items
                .into_iter()
                .map(yaml_to_json)
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Mapping(mapping) => {
            let mut object = serde_json::Map::with_capacity(mapping.len());
            for (key, value) in mapping {
                // YAML allows non-string keys: numbers/bools are stringified,
                // complex keys are rejected
                let key = match key {
                    Yaml::String(s) => s,
                    Yaml::Bool(b) => b.to_string(),
                    Yaml::Number(n) => n.to_string(),
                    other => return Err(format!("unsupported YAML mapping key: {other:?}")),
                };
                object.insert(key, yaml_to_json(value)?);
            }
            Value::Object(object)
        }
        // Tagged values (e.g. `!Custom`): ignore the tag, take the inner value
        Yaml::Tagged(tagged) => yaml_to_json(tagged.value)?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Extension detection: case-insensitive, four supported extensions.
    #[test]
    fn detect_format_from_extension() {
        assert_eq!(
            Format::from_path(Path::new("a/app.toml")).unwrap(),
            Format::Toml
        );
        assert_eq!(
            Format::from_path(Path::new("app.yaml")).unwrap(),
            Format::Yaml
        );
        assert_eq!(
            Format::from_path(Path::new("app.yml")).unwrap(),
            Format::Yaml
        );
        assert_eq!(
            Format::from_path(Path::new("app.JSON")).unwrap(),
            Format::Json
        );
    }

    /// A missing or unknown extension should yield UnknownExtension.
    #[test]
    fn detect_format_unknown_extension() {
        for path in ["app", "app.ini", "app.conf"] {
            let err = Format::from_path(Path::new(path)).unwrap_err();
            assert!(
                matches!(err, Error::UnknownExtension { .. }),
                "{path}: {err}"
            );
        }
    }

    /// Deep merge: objects recurse; scalars/arrays/null replace; new keys
    /// are inserted.
    #[test]
    fn merge_semantics() {
        let mut base = json!({
            "server": { "host": "localhost", "port": 8080 },
            "tags": ["a", "b"],
            "debug": false,
        });
        let overlay = json!({
            "server": { "port": 9090 },
            "tags": ["c"],
            "debug": null,
            "extra": 1,
        });
        merge(&mut base, overlay);
        assert_eq!(
            base,
            json!({
                "server": { "host": "localhost", "port": 9090 },
                "tags": ["c"],
                "debug": null,
                "extra": 1,
            })
        );
    }

    /// Merging non-objects: the overlay simply replaces the base.
    #[test]
    fn merge_scalar_replaces_object() {
        let mut base = json!({ "a": { "b": 1 } });
        merge(&mut base, json!({ "a": 42 }));
        assert_eq!(base, json!({ "a": 42 }));
    }

    /// A JSON syntax error should yield Parse with origin info.
    #[test]
    fn json_parse_error_has_origin() {
        let err = Format::Json
            .parse("{ bad", Some(Path::new("cfg.json")))
            .unwrap_err();
        assert!(matches!(
            err,
            Error::Parse {
                format: Format::Json,
                ..
            }
        ));
        assert!(err.to_string().contains("cfg.json"), "{err}");
    }

    /// TOML datetimes should become strings, not internal marker maps.
    #[cfg(feature = "toml")]
    #[test]
    fn toml_datetime_becomes_string() {
        let value = Format::Toml
            .parse("ts = 2026-01-02T03:04:05Z", None)
            .unwrap();
        assert_eq!(value, json!({ "ts": "2026-01-02T03:04:05Z" }));
    }

    /// TOML nan / inf cannot be represented as JSON numbers and should
    /// yield Parse.
    #[cfg(feature = "toml")]
    #[test]
    fn toml_non_finite_float_is_rejected() {
        let err = Format::Toml.parse("f = nan", None).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Parse {
                    format: Format::Toml,
                    ..
                }
            ),
            "{err}"
        );
    }

    /// YAML number/bool keys are stringified.
    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_scalar_keys_are_stringified() {
        let value = Format::Yaml
            .parse("1: one\ntrue: yes_value\n", None)
            .unwrap();
        assert_eq!(value, json!({ "1": "one", "true": "yes_value" }));
    }

    /// YAML complex keys (e.g. sequences) should be rejected.
    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_complex_key_is_rejected() {
        let err = Format::Yaml.parse("? [a, b]\n: value\n", None).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Parse {
                    format: Format::Yaml,
                    ..
                }
            ),
            "{err}"
        );
    }

    /// YAML tagged values unwrap to the inner value.
    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_tagged_value_is_unwrapped() {
        let value = Format::Yaml.parse("v: !Custom 7\n", None).unwrap();
        assert_eq!(value, json!({ "v": 7 }));
    }
}
