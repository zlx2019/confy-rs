# confy-rs

[![CI](https://github.com/zlx2019/confy-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/zlx2019/confy-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/confy-rs.svg)](https://crates.io/crates/confy-rs)
[![docs.rs](https://img.shields.io/docsrs/confy-rs)](https://docs.rs/confy-rs)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96.0%2B-orange.svg)](https://www.rust-lang.org)

[English](./README.md) | 简体中文

> 简单高效的 Rust 配置加载库 —— 多格式、分层合并、热加载。

## 特性

- **多格式** —— TOML / YAML / JSON, 按文件扩展名自动识别
- **分层合并** —— 内存默认值 + 任意数量的配置文件, 按添加顺序深度合并
- **类型安全** —— 通过 [serde](https://serde.rs) 直接反序列化到你自己的结构体
- **热加载** —— 跨平台文件监听 + 事件去抖; 读取端无锁, 重载失败时旧配置继续生效
- **轻量** —— 依赖少而小, 非核心能力全部按 feature 门控, 不绑定异步运行时

## 安装

```toml
[dependencies]
confy-rs = "0.1"
```

TOML 与热加载默认启用, YAML 按需开启:

```toml
confy-rs = { version = "0.1", features = ["yaml"] }
```

## 快速开始

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AppConfig {
    name: String,
    port: u16,
}

fn main() -> confy_rs::Result<()> {
    // 按扩展名自动识别格式(.toml / .yaml / .yml / .json)
    let config: AppConfig = confy_rs::load("config.toml")?;
    println!("{} listening on {}", config.name, config.port);
    Ok(())
}
```

## 分层配置

配置源按添加顺序合并 —— 后面的源覆盖前面的。object 递归合并;
标量、数组与 `null` 则整体覆盖旧值。

```rust
let config: AppConfig = confy_rs::builder()
    .defaults(serde_json::json!({ "port": 8080 }))? // 最低优先级
    .file("config/default.toml")                    // 必需文件, 缺失报错
    .file_optional("config/local.yaml")             // 可选文件, 缺失跳过
    .build()?;
```

## 热加载

```rust
let watcher = confy_rs::builder()
    .file("config.toml")
    .watch::<AppConfig>()?;

watcher.on_change(|config| println!("port is now {}", config.port));
watcher.on_error(|err| eprintln!("reload failed, old config kept: {err}"));

let current = watcher.get(); // Arc<AppConfig>, 无锁读取
```

行为保证:

- **快速失败** —— 启动时同步加载一次, 配置有问题立刻报错, 不会拖到运行期
- **编辑器友好** —— 监听父目录而非文件本身, 兼容 vim / VS Code 等"原子 rename"式保存
- **事件去抖** —— 连续的文件事件合并为一次重载; 内容未变化不会触发回调
- **稳定可靠** —— 重载失败不会崩溃或产生脏状态: 旧配置继续生效, 错误经 `on_error` 上报
- **读取极快** —— `watcher.get()` 无锁(`arc-swap`), 可放在每次请求的热路径上
- **运行时无关** —— 仅一个后台线程, 不依赖 tokio; async 代码中可自由调用 `get()`

## Cargo features

| Feature | 默认 | 作用 |
|---------|:----:|------|
| `toml`  | ✅   | TOML 支持([`toml`](https://crates.io/crates/toml)) |
| `yaml`  | —    | YAML 支持([`serde_yaml_ng`](https://crates.io/crates/serde_yaml_ng)) |
| `watch` | ✅   | 热加载([`notify`](https://crates.io/crates/notify) + [`arc-swap`](https://crates.io/crates/arc-swap)) |

JSON 支持始终内建。

## 示例

```bash
cargo run --example basic       # 一行加载
cargo run --example layered     # 默认值 + 跨格式分层
cargo run --example hot_reload  # 编辑 fixtures/config.toml 观察热加载
```

## MSRV

Rust `1.96.0`(由 `rust-toolchain.toml` 锁定)。

## 开发

```bash
cargo install cargo-nextest --locked  # 测试运行器
cargo install --locked cargo-deny     # 依赖安全 / 许可证审计
cargo install typos-cli               # 拼写检查
pip install pre-commit && pre-commit install
```

常用命令:

```bash
cargo nextest run --all-features                          # 测试
cargo clippy --all-targets --all-features -- -D warnings  # 静态检查
cargo fmt                                                 # 格式化
```

完整流程见 [CONTRIBUTING.md](./CONTRIBUTING.md)。

## License

本项目采用 [MIT](./LICENSE) 许可证。
