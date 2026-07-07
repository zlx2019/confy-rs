# confy-rs

[![CI](https://github.com/zlx2019/confy-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/zlx2019/confy-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/confy-rs.svg)](https://crates.io/crates/confy-rs)
[![docs.rs](https://img.shields.io/docsrs/confy-rs)](https://docs.rs/confy-rs)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96.0%2B-orange.svg)](https://www.rust-lang.org)

English | [简体中文](./README.zh-CN.md)

> Simple, efficient configuration loading for Rust — multi-format, layered merging, and hot reloading.

## Features

- **Multi-format** — TOML / YAML / JSON, auto-detected from the file extension
- **Layered** — in-memory defaults plus any number of files, deep-merged in order
- **Type-safe** — deserializes straight into your own structs via [serde](https://serde.rs)
- **Hot reload** — debounced cross-platform file watching; readers are lock-free, and a failed reload keeps the previous configuration active
- **Lean** — a handful of small dependencies, everything non-essential is feature-gated, no async runtime required

## Installation

```toml
[dependencies]
confy-rs = "0.1"
```

TOML and hot reloading are enabled by default. YAML is opt-in:

```toml
confy-rs = { version = "0.1", features = ["yaml"] }
```

## Quick start

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AppConfig {
    name: String,
    port: u16,
}

fn main() -> confy_rs::Result<()> {
    // Format is detected from the extension (.toml / .yaml / .yml / .json)
    let config: AppConfig = confy_rs::load("config.toml")?;
    println!("{} listening on {}", config.name, config.port);
    Ok(())
}
```

## Layered configuration

Sources are merged in the order they are added — later sources override
earlier ones. Objects merge recursively; scalars, arrays and `null` replace
the previous value entirely.

```rust
let config: AppConfig = confy_rs::builder()
    .defaults(serde_json::json!({ "port": 8080 }))? // lowest priority
    .file("config/default.toml")                    // required, error if missing
    .file_optional("config/local.yaml")             // silently skipped if missing
    .build()?;
```

## Hot reloading

```rust
let watcher = confy_rs::builder()
    .file("config.toml")
    .watch::<AppConfig>()?;

watcher.on_change(|config| println!("port is now {}", config.port));
watcher.on_error(|err| eprintln!("reload failed, old config kept: {err}"));

let current = watcher.get(); // Arc<AppConfig>, lock-free
```

What you get:

- **Fail fast** — the initial load is synchronous; a broken config errors at startup, not later
- **Editor-friendly** — parent directories are watched, so atomic-rename saves (vim, VS Code, ...) are handled
- **Debounced** — bursts of file events collapse into a single reload; unchanged content triggers no callbacks
- **Resilient** — a failed reload never crashes or corrupts state: the previous config stays active and the error is reported via `on_error`
- **Fast reads** — `watcher.get()` is lock-free (`arc-swap`), cheap enough for per-request use
- **Runtime-agnostic** — one background thread, no tokio required; call `get()` from async code freely

## Cargo features

| Feature | Default | Effect |
|---------|:-------:|--------|
| `toml`  | ✅      | TOML support via the [`toml`](https://crates.io/crates/toml) crate |
| `yaml`  | —       | YAML support via [`serde_yaml_ng`](https://crates.io/crates/serde_yaml_ng) |
| `watch` | ✅      | hot reloading via [`notify`](https://crates.io/crates/notify) + [`arc-swap`](https://crates.io/crates/arc-swap) |

JSON support is always built in.

## Examples

```bash
cargo run --example basic       # one-line loading
cargo run --example layered     # defaults + cross-format layering
cargo run --example hot_reload  # edit fixtures/config.toml and watch it reload
```

## MSRV

Rust `1.96.0` (pinned by `rust-toolchain.toml`).

## Development

```bash
cargo install cargo-nextest --locked  # test runner
cargo install --locked cargo-deny     # dependency / license audit
cargo install typos-cli               # spell check
pip install pre-commit && pre-commit install
```

Common commands:

```bash
cargo nextest run --all-features                          # tests
cargo clippy --all-targets --all-features -- -D warnings  # lints
cargo fmt                                                 # format
```

See [CONTRIBUTING.md](./CONTRIBUTING.md) for the full workflow.

## License

Licensed under [MIT](./LICENSE).
