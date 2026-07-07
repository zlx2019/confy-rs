//! Layered configuration: in-memory defaults + base file + override file
//! (formats may differ per layer).
//!
//! Priority, lowest to highest: defaults < default.toml < override.json
//! < local.toml. Objects merge recursively, scalars/arrays are replaced,
//! and missing optional files are skipped.
//!
//! Run: `cargo run --example layered`

use serde::Deserialize;
use serde_json::json;

/// Application config.
#[derive(Debug, Deserialize)]
struct AppConfig {
    name: String,
    debug: bool,
    tags: Vec<String>,
    server: Server,
}

/// Server config.
#[derive(Debug, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

fn main() -> confy_rs::Result<()> {
    let config: AppConfig = confy_rs::builder()
        // Lowest priority: in-memory defaults (anything impl Serialize works)
        .defaults(json!({
            "name": "fallback",
            "debug": false,
            "tags": [],
            "server": { "host": "127.0.0.1", "port": 80 },
        }))?
        // Base config (TOML)
        .file("fixtures/default.toml")
        // Environment override (JSON): only overrides debug and server.port
        .file("fixtures/override.json")
        // Local override: does not exist, silently skipped
        .file_optional("fixtures/local.toml")
        .build()?;

    println!("name : {} (debug = {})", config.name, config.debug);
    println!("tags : {:?}", config.tags);
    println!(
        "serve: {}:{}  <- port from override.json, host from default.toml",
        config.server.host, config.server.port
    );
    Ok(())
}
