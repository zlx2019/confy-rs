//! File watching and hot reloading.
//!
//! Implementation notes:
//! - The **parent directory** is watched instead of the file itself, so that
//!   editors saving via "write temp file + atomic rename" keep working
//!   (watching the file directly breaks once its inode is replaced);
//! - the background thread debounces with `recv_timeout`, collapsing an
//!   event storm into a single reload;
//! - readers go through `ArcSwap`, so `get()` is lock-free;
//! - a failed reload keeps the previous config and reports the error via
//!   `on_error`; reloads yielding unchanged content are silently skipped.

use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use arc_swap::ArcSwap;
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::builder::ConfigBuilder;
use crate::error::Error;

/// Debounce window: consecutive file events within it collapse into one reload.
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);
/// Retry count for a target file that is briefly missing (mid atomic replace).
const RETRY_ATTEMPTS: usize = 3;
/// Interval between those retries.
const RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Type aliases for change and error callbacks.
type ChangeCallback<T> = Arc<dyn Fn(Arc<T>) + Send + Sync>;
type ErrorCallback = Arc<dyn Fn(&Error) + Send + Sync>;

/// A handle to a configuration that is automatically reloaded when any of
/// its source files change.
///
/// - [`get`](Self::get) returns the current snapshot without locking, cheap
///   enough to call on every request;
/// - when a reload fails, the previous configuration stays active and the
///   error is reported through [`on_error`](Self::on_error);
/// - dropping the watcher stops the background thread.
///
/// Created by [`ConfigBuilder::watch`].
pub struct ConfigWatcher<T> {
    /// State shared with the background thread.
    shared: Arc<Shared<T>>,
    /// notify handle: dropping it closes the event channel, which in turn
    /// ends the background thread.
    watcher: Option<RecommendedWatcher>,
    /// Handle of the background reload thread.
    thread: Option<JoinHandle<()>>,
}

/// State shared between readers and the background reload thread.
struct Shared<T> {
    /// Currently active configuration snapshot.
    current: ArcSwap<T>,
    /// Callbacks notified after each successful reload.
    on_change: Mutex<Vec<ChangeCallback<T>>>,
    /// Callbacks notified when a reload fails.
    on_error: Mutex<Vec<ErrorCallback>>,
}

impl ConfigBuilder {
    /// Loads the configuration once, then watches all file sources and
    /// hot-reloads whenever they change.
    ///
    /// The initial load is synchronous, so a broken configuration is
    /// reported immediately instead of at some point later. The returned
    /// watcher is runtime-agnostic: it uses one background thread and no
    /// async runtime, and [`ConfigWatcher::get`] can be called from
    /// anywhere, including async code.
    ///
    /// Note: the parent directory of an *optional* file that does not exist
    /// yet cannot be watched; such a file will only be picked up on the next
    /// reload triggered by another source.
    ///
    /// # Errors
    ///
    /// Returns an error if the initial load fails (see
    /// [`build`](Self::build)) or if the file watcher cannot be set up.
    pub fn watch<T>(self) -> Result<ConfigWatcher<T>, Error>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        // Load once synchronously at startup so a broken config fails immediately
        let initial_value = self.build_value()?;
        let initial: T =
            serde_json::from_value(initial_value.clone()).map_err(Error::Deserialize)?;

        let shared = Arc::new(Shared {
            current: ArcSwap::from_pointee(initial),
            on_change: Mutex::new(Vec::new()),
            on_error: Mutex::new(Vec::new()),
        });

        // notify events are forwarded through a std mpsc channel to the
        // background thread
        let (sender, receiver) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(move |event| {
            // A send failure means we are shutting down; drop the event
            let _ = sender.send(event);
        })?;

        // Collect watch targets: watched file names are recorded for event
        // filtering; directories are deduplicated by canonical path, OR-ing
        // `required` per directory
        let mut file_names: HashSet<OsString> = HashSet::new();
        let mut dirs: HashMap<PathBuf, (PathBuf, bool)> = HashMap::new();
        for (path, required) in self.file_paths() {
            if let Some(name) = path.file_name() {
                file_names.insert(name.to_os_string());
            }
            let dir = parent_dir(&path);
            let key = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            let entry = dirs.entry(key).or_insert_with(|| (dir, false));
            entry.1 |= required;
        }
        for (dir, required) in dirs.into_values() {
            if let Err(err) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
                // A required file's directory must exist (the initial load
                // passed), so a watch failure is a real error; an optional
                // file's directory may be missing and is simply skipped
                if required {
                    return Err(Error::Watch(err));
                }
            }
        }

        // The background thread takes over the builder, handling debouncing
        // and reloads
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                reload_loop(&receiver, &self, &shared, initial_value, &file_names);
            })
        };

        Ok(ConfigWatcher {
            shared,
            watcher: Some(watcher),
            thread: Some(thread),
        })
    }
}

impl<T> ConfigWatcher<T> {
    /// Returns the currently active configuration snapshot.
    ///
    /// Lock-free (backed by `arc-swap`), so it can be called freely on hot
    /// paths and from any thread. The returned [`Arc`] stays valid even if
    /// the configuration is swapped out in the meantime.
    #[must_use]
    pub fn get(&self) -> Arc<T> {
        self.shared.current.load_full()
    }

    /// Registers a callback invoked after each successful reload with the
    /// new configuration.
    ///
    /// Callbacks may be registered multiple times and run on the background
    /// watcher thread — keep them short and non-blocking. A callback that
    /// panics will terminate that thread and stop hot reloading.
    pub fn on_change(&self, callback: impl Fn(Arc<T>) + Send + Sync + 'static) {
        push_callback(&self.shared.on_change, Arc::new(callback));
    }

    /// Registers a callback invoked when a reload fails.
    ///
    /// The previous configuration remains active in that case. Runs on the
    /// background watcher thread, same caveats as
    /// [`on_change`](Self::on_change).
    pub fn on_error(&self, callback: impl Fn(&Error) + Send + Sync + 'static) {
        push_callback(&self.shared.on_error, Arc::new(callback));
    }
}

impl<T> Drop for ConfigWatcher<T> {
    fn drop(&mut self) {
        // Drop the notify watcher first: the event channel closes and the
        // background thread exits once recv fails
        drop(self.watcher.take());
        if let Some(handle) = self.thread.take() {
            // No need to re-propagate a panic caused by a user callback
            let _ = handle.join();
        }
    }
}

impl<T> Shared<T> {
    /// Fires change callbacks: snapshot the list first so user code never
    /// runs under the lock.
    fn fire_change(&self, config: &Arc<T>) {
        for callback in lock_snapshot(&self.on_change) {
            callback(Arc::clone(config));
        }
    }

    /// Fires error callbacks.
    fn fire_error(&self, error: &Error) {
        for callback in lock_snapshot(&self.on_error) {
            callback(error);
        }
    }
}

/// Background reload loop: block on events → debounce → reload and notify.
fn reload_loop<T>(
    receiver: &mpsc::Receiver<notify::Result<notify::Event>>,
    builder: &ConfigBuilder,
    shared: &Shared<T>,
    mut last_value: Value,
    file_names: &HashSet<OsString>,
) where
    T: DeserializeOwned,
{
    // The loop ends when the channel closes (watcher dropped); the thread
    // then exits cleanly
    while let Ok(event) = receiver.recv() {
        if !is_relevant(&event, file_names) {
            continue;
        }
        // Debounce: editor saves tend to emit a burst of events, so swallow
        // everything within the window
        loop {
            match receiver.recv_timeout(DEBOUNCE_WINDOW) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        reload(builder, shared, &mut last_value);
    }
}

/// Performs a single reload: failures keep the old config and report the
/// error; unchanged content is silently skipped.
fn reload<T>(builder: &ConfigBuilder, shared: &Shared<T>, last_value: &mut Value)
where
    T: DeserializeOwned,
{
    // Atomic-replace saves leave a brief window where the file is missing,
    // so retry NotFound a bounded number of times
    let mut result = builder.build_value();
    for _ in 0..RETRY_ATTEMPTS {
        if !matches!(result, Err(Error::NotFound { .. })) {
            break;
        }
        std::thread::sleep(RETRY_INTERVAL);
        result = builder.build_value();
    }

    let value = match result {
        Ok(value) => value,
        Err(err) => return shared.fire_error(&err),
    };
    // Skip triggers without an actual change (metadata events, historical
    // events at watcher startup, ...)
    if value == *last_value {
        return;
    }
    match serde_json::from_value::<T>(value.clone()) {
        Ok(config) => {
            *last_value = value;
            let snapshot = Arc::new(config);
            // Publish the new value before notifying so get() inside a
            // callback always sees the new config
            shared.current.store(Arc::clone(&snapshot));
            shared.fire_change(&snapshot);
        }
        Err(err) => shared.fire_error(&Error::Deserialize(err)),
    }
}

/// Whether the event touches any watched file (matched by file name; a
/// false positive only costs one extra idempotent reload).
fn is_relevant(event: &notify::Result<notify::Event>, file_names: &HashSet<OsString>) -> bool {
    match event {
        Ok(event) => event.paths.iter().any(|path| {
            path.file_name()
                .is_some_and(|name| file_names.contains(name))
        }),
        // Errors from notify itself (e.g. event queue overflow): reload once
        // to stay on the safe side
        Err(_) => true,
    }
}

/// The file's parent directory; bare file names fall back to the current
/// directory.
fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Appends a callback (tolerating lock poisoning: code holding the lock
/// never panics).
fn push_callback<C>(mutex: &Mutex<Vec<C>>, callback: C) {
    match mutex.lock() {
        Ok(mut guard) => guard.push(callback),
        Err(poisoned) => poisoned.into_inner().push(callback),
    }
}

/// Clones the callback list and releases the lock right away, so user
/// callbacks never run under the lock (avoids deadlocks).
fn lock_snapshot<C: Clone>(mutex: &Mutex<Vec<C>>) -> Vec<C> {
    match mutex.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parent directory derivation: regular paths and bare file names.
    #[test]
    fn parent_dir_fallbacks_to_current() {
        assert_eq!(parent_dir(Path::new("a/b/c.toml")), PathBuf::from("a/b"));
        assert_eq!(parent_dir(Path::new("c.toml")), PathBuf::from("."));
    }

    /// Event filtering: only watched file names match.
    #[test]
    fn event_relevance_by_file_name() {
        let names: HashSet<OsString> = [OsString::from("app.toml")].into();
        let event =
            notify::Event::new(notify::EventKind::Any).add_path(PathBuf::from("/tmp/app.toml"));
        assert!(is_relevant(&Ok(event), &names));

        let other =
            notify::Event::new(notify::EventKind::Any).add_path(PathBuf::from("/tmp/other.toml"));
        assert!(!is_relevant(&Ok(other), &names));
    }

    /// Compile-time contract: ConfigWatcher must remain Send + Sync so a
    /// dependency upgrade (e.g. notify changing its internal handles) cannot
    /// silently break users' multi-threaded code.
    #[test]
    fn config_watcher_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ConfigWatcher<String>>();
    }
}
