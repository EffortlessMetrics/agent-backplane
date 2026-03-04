// SPDX-License-Identifier: MIT OR Apache-2.0
//! Polling-based configuration file watcher.
//!
//! [`ConfigWatcher`] periodically checks a config file's modification time
//! and, when it changes, reloads + validates the file before invoking a
//! user-supplied callback.  No external file-watcher dependencies are
//! required — only `std::time::SystemTime` and `std::fs::metadata`.

#![allow(dead_code, unused_imports)]

use crate::{BackplaneConfig, ConfigError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime};

/// Default interval between polls.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Debounce window — after detecting a change, wait this long to let
/// writers finish before reloading.
const DEBOUNCE_DELAY: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------------
// ConfigWatcher
// ---------------------------------------------------------------------------

/// Polls a config file for changes and fires a callback on reload.
///
/// # Lifecycle
///
/// 1. Create with [`ConfigWatcher::new`].
/// 2. Call [`start`](ConfigWatcher::start) — spawns a background thread.
/// 3. Call [`stop`](ConfigWatcher::stop) to shut down gracefully.
///
/// The watcher is automatically stopped when dropped.
pub struct ConfigWatcher {
    path: PathBuf,
    poll_interval: Duration,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ConfigWatcher {
    /// Create a new watcher for `path` with the default 5-second poll interval.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    /// Override the poll interval.
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Start the background polling thread.
    ///
    /// `on_change` is called each time a valid, changed config is loaded.
    /// If the file cannot be parsed or fails validation the callback is
    /// **not** invoked — the error is silently skipped (the old config
    /// remains in effect).
    ///
    /// Does nothing if the watcher is already running.
    pub fn start<F>(&mut self, on_change: F)
    where
        F: Fn(BackplaneConfig) + Send + 'static,
    {
        if self.running.load(Ordering::SeqCst) {
            return;
        }

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let path = self.path.clone();
        let interval = self.poll_interval;

        let handle = thread::spawn(move || {
            let mut last_mtime: Option<SystemTime> = file_mtime(&path);

            while running.load(Ordering::SeqCst) {
                thread::sleep(interval);

                if !running.load(Ordering::SeqCst) {
                    break;
                }

                let current_mtime = file_mtime(&path);
                if current_mtime != last_mtime && current_mtime.is_some() {
                    // Debounce: wait a short time and re-check to make sure
                    // the file isn't still being written.
                    thread::sleep(DEBOUNCE_DELAY);
                    let after_debounce = file_mtime(&path);
                    if after_debounce != current_mtime {
                        // File is still changing — skip this cycle.
                        continue;
                    }

                    last_mtime = current_mtime;

                    if let Ok(cfg) = crate::load_from_file(&path) {
                        if crate::validate_config(&cfg).is_ok() {
                            on_change(cfg);
                        }
                    }
                }
            }
        });

        self.handle = Some(handle);
    }

    /// Signal the watcher thread to stop and wait for it to finish.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    /// Whether the watcher is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    fn write_config(path: &Path, content: &str) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }

    #[test]
    fn watcher_detects_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        write_config(&path, "log_level = \"info\"\n");

        let seen: Arc<Mutex<Vec<BackplaneConfig>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = Arc::clone(&seen);

        let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
        watcher.start(move |cfg| {
            seen2.lock().unwrap().push(cfg);
        });

        // Wait a bit, then modify the file.
        thread::sleep(Duration::from_millis(100));
        write_config(&path, "log_level = \"debug\"\n");

        // Give the watcher time to detect + debounce + reload.
        thread::sleep(Duration::from_millis(400));
        watcher.stop();

        let configs = seen.lock().unwrap();
        assert!(
            !configs.is_empty(),
            "watcher should have detected the change"
        );
        assert_eq!(configs.last().unwrap().log_level.as_deref(), Some("debug"));
    }

    #[test]
    fn watcher_does_not_fire_on_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        write_config(&path, "log_level = \"info\"\n");

        let count = Arc::new(Mutex::new(0u32));
        let count2 = Arc::clone(&count);

        let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
        watcher.start(move |_cfg| {
            *count2.lock().unwrap() += 1;
        });

        thread::sleep(Duration::from_millis(100));
        // Write invalid TOML.
        write_config(&path, "log_level = [[[broken\n");
        thread::sleep(Duration::from_millis(400));
        watcher.stop();

        assert_eq!(*count.lock().unwrap(), 0);
    }

    #[test]
    fn stop_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        write_config(&path, "log_level = \"info\"\n");

        let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
        watcher.start(|_| {});
        assert!(watcher.is_running());

        watcher.stop();
        assert!(!watcher.is_running());

        // Second stop should not panic.
        watcher.stop();
    }

    #[test]
    fn start_when_already_running_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        write_config(&path, "log_level = \"info\"\n");

        let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
        watcher.start(|_| {});
        watcher.start(|_| {}); // should be a no-op
        assert!(watcher.is_running());
        watcher.stop();
    }

    #[test]
    fn watcher_default_poll_interval() {
        let watcher = ConfigWatcher::new("/tmp/test.toml");
        assert_eq!(watcher.poll_interval, Duration::from_secs(5));
    }

    #[test]
    fn custom_poll_interval() {
        let watcher = ConfigWatcher::new("/tmp/test.toml").poll_interval(Duration::from_secs(10));
        assert_eq!(watcher.poll_interval, Duration::from_secs(10));
    }
}
