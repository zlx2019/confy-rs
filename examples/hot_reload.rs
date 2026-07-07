//! Hot reloading: watch the config file and reload automatically on change.
//!
//! Run: `cargo run --example hot_reload`
//! Then edit fixtures/config.toml (e.g. change `port`) and watch the output;
//! you can also break the syntax on purpose to see on_error fire while the
//! old config stays active. Ctrl-C to quit.

use std::time::Duration;

use serde::Deserialize;

/// Application config (serde ignores unknown fields in the file by default).
#[derive(Debug, Deserialize)]
struct AppConfig {
    debug: bool,
    server: Server,
}

/// Server config.
#[derive(Debug, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

fn main() -> confy_rs::Result<()> {
    // Load once synchronously at startup; a broken config errors out immediately
    let watcher = confy_rs::builder()
        .file("fixtures/config.toml")
        .watch::<AppConfig>()?;

    // Successful reload: receive the new config snapshot
    watcher.on_change(|config| {
        println!(
            "[on_change] reloaded -> {}:{} (debug = {})",
            config.server.host, config.server.port, config.debug
        );
    });
    // Failed reload: the old config stays active
    watcher.on_error(|error| {
        eprintln!("[on_error ] reload failed, keeping old config: {error}");
    });

    println!("watching fixtures/config.toml — edit it to see hot reloading (Ctrl-C to quit)");
    loop {
        // get() is lock-free, suitable for per-request hot paths
        let current = watcher.get();
        println!(
            "[poll     ] current = {}:{}",
            current.server.host, current.server.port
        );
        std::thread::sleep(Duration::from_secs(5));
    }
}
