//! Integration tests: hot reloading, using real file system writes and
//! assertions with timeouts.
#![cfg(all(feature = "watch", feature = "toml"))]

use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
struct Conf {
    port: u16,
}

/// Max time to wait for a callback: macOS FSEvents can lag by seconds and
/// CI machines may be slower, so leave plenty of headroom.
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Overwriting the config file should fire on_change and get() should
/// return the new value.
#[test]
fn reload_on_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.toml");
    std::fs::write(&path, "port = 1\n").unwrap();

    let watcher = confy_rs::builder().file(&path).watch::<Conf>().unwrap();
    assert_eq!(watcher.get().port, 1);

    let (tx, rx) = mpsc::channel();
    watcher.on_change(move |config| {
        let _ = tx.send(config.port);
    });

    std::fs::write(&path, "port = 2\n").unwrap();

    assert_eq!(rx.recv_timeout(EVENT_TIMEOUT).unwrap(), 2);
    assert_eq!(watcher.get().port, 2);
}

/// Editor-style "write temp file + atomic rename" saves should also trigger
/// a reload.
#[test]
fn reload_on_atomic_rename() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.toml");
    std::fs::write(&path, "port = 1\n").unwrap();

    let watcher = confy_rs::builder().file(&path).watch::<Conf>().unwrap();
    let (tx, rx) = mpsc::channel();
    watcher.on_change(move |config| {
        let _ = tx.send(config.port);
    });

    // Simulate an editor save: write the new content to a temp file, then
    // rename it over the original
    let tmp = dir.path().join("app.toml.tmp");
    std::fs::write(&tmp, "port = 3\n").unwrap();
    std::fs::rename(&tmp, &path).unwrap();

    assert_eq!(rx.recv_timeout(EVENT_TIMEOUT).unwrap(), 3);
    assert_eq!(watcher.get().port, 3);
}

/// A reload that fails to parse: the old config is kept and on_error fires.
#[test]
fn keep_old_config_on_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.toml");
    std::fs::write(&path, "port = 1\n").unwrap();

    let watcher = confy_rs::builder().file(&path).watch::<Conf>().unwrap();
    let (tx, rx) = mpsc::channel();
    watcher.on_error(move |error| {
        let _ = tx.send(error.to_string());
    });

    // Write invalid TOML
    std::fs::write(&path, "port = \n").unwrap();

    let message = rx.recv_timeout(EVENT_TIMEOUT).unwrap();
    assert!(message.contains("TOML"), "{message}");
    // The old config is still active
    assert_eq!(watcher.get().port, 1);
}

/// Writes without an actual content change should not produce on_change
/// notifications.
#[test]
fn identical_content_does_not_notify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.toml");
    std::fs::write(&path, "port = 1\n").unwrap();

    let watcher = confy_rs::builder().file(&path).watch::<Conf>().unwrap();
    let (tx, rx) = mpsc::channel();
    watcher.on_change(move |config| {
        let _ = tx.send(config.port);
    });

    // Content identical to the current one
    std::fs::write(&path, "port = 1\n").unwrap();

    // Passes only if no notification arrives well past the debounce window
    assert!(rx.recv_timeout(Duration::from_secs(3)).is_err());
    assert_eq!(watcher.get().port, 1);
}

/// Dropping the watcher should cleanly stop the background thread (a hang
/// would show up as a test timeout).
#[test]
fn drop_stops_watcher() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.toml");
    std::fs::write(&path, "port = 1\n").unwrap();

    let watcher = confy_rs::builder().file(&path).watch::<Conf>().unwrap();
    drop(watcher);
}
