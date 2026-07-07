//! Integration tests: multi-format loading and layered merging, based on
//! real files under `fixtures/`.

use serde::Deserialize;

/// Config structure matching the sample files under fixtures/.
#[derive(Debug, PartialEq, Deserialize)]
struct AppConfig {
    name: String,
    debug: bool,
    tags: Vec<String>,
    server: Server,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

/// Expected content of fixtures/config.*.
fn expected() -> AppConfig {
    AppConfig {
        name: "confy-demo".into(),
        debug: false,
        tags: vec!["alpha".into(), "beta".into()],
        server: Server {
            host: "127.0.0.1".into(),
            port: 8080,
        },
    }
}

/// Loads a TOML file.
#[cfg(feature = "toml")]
#[test]
fn load_toml_file() {
    let config: AppConfig = confy_rs::load("fixtures/config.toml").unwrap();
    assert_eq!(config, expected());
}

/// Loads a YAML file.
#[cfg(feature = "yaml")]
#[test]
fn load_yaml_file() {
    let config: AppConfig = confy_rs::load("fixtures/config.yaml").unwrap();
    assert_eq!(config, expected());
}

/// Loads a JSON file (built-in support).
#[test]
fn load_json_file() {
    let config: AppConfig = confy_rs::load("fixtures/config.json").unwrap();
    assert_eq!(config, expected());
}

/// Layered merge across formats: TOML base + JSON overlay, with a missing
/// optional layer skipped.
#[cfg(feature = "toml")]
#[test]
fn layered_merge_across_formats() {
    let config: AppConfig = confy_rs::builder()
        .file("fixtures/default.toml")
        .file("fixtures/override.json")
        .file_optional("fixtures/nonexistent.json")
        .build()
        .unwrap();

    assert_eq!(
        config,
        AppConfig {
            // Fields absent from the overlay keep the base layer's values
            name: "layered-demo".into(),
            tags: vec!["default".into()],
            server: Server {
                host: "0.0.0.0".into(),
                port: 9090
            },
            debug: true,
        }
    );
}

/// The in-memory defaults layer has the lowest priority.
#[cfg(feature = "toml")]
#[test]
fn defaults_layer_has_lowest_priority() {
    let config: AppConfig = confy_rs::builder()
        .defaults(serde_json::json!({
            "name": "from-defaults",
            "debug": true,
            "tags": ["x"],
            "server": { "host": "localhost", "port": 1 },
        }))
        .unwrap()
        .file("fixtures/override.json")
        .build()
        .unwrap();

    assert_eq!(config.name, "from-defaults");
    assert_eq!(config.server.port, 9090);
    assert_eq!(config.server.host, "localhost");
}

/// A file with a syntax error: the message should contain the path and the
/// format.
#[cfg(feature = "toml")]
#[test]
fn parse_error_mentions_path() {
    let err = confy_rs::load::<AppConfig>("fixtures/invalid.toml").unwrap_err();
    assert!(matches!(err, confy_rs::Error::Parse { .. }), "{err}");
    let message = err.to_string();
    assert!(message.contains("invalid.toml"), "{message}");
    assert!(message.contains("TOML"), "{message}");
}

/// A missing required file: NotFound with the path in the message.
#[test]
fn missing_required_file() {
    let err = confy_rs::load::<AppConfig>("fixtures/nope.json").unwrap_err();
    assert!(matches!(err, confy_rs::Error::NotFound { .. }), "{err}");
    assert!(err.to_string().contains("nope.json"), "{err}");
}
