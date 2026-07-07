//! Minimal usage: load and deserialize in one line, with the format
//! detected from the file extension.
//!
//! Run: `cargo run --example basic`

use serde::Deserialize;

/// Application config (matches fixtures/config.toml).
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
    let config: AppConfig = confy_rs::load("fixtures/config.toml")?;

    println!("app  : {} (debug = {})", config.name, config.debug);
    println!("tags : {:?}", config.tags);
    println!("serve: {}:{}", config.server.host, config.server.port);
    Ok(())
}
