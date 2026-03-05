// SPDX-License-Identifier: MIT OR Apache-2.0
//! Auto-cleanup of expired workspaces with configurable TTL.
//!
//! [`WorkspaceLifecycle`] tracks workspace registrations and provides
//! expiry-based cleanup so that stale temporary directories do not accumulate.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Lifecycle configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifecycleConfig {
    /// Default time-to-live for workspaces in seconds.
    pub ttl_secs: i64,
    /// Whether to automatically remove directories on expiry.
    pub auto_remove: bool,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            ttl_secs: 3600,
            auto_remove: true,
        }
    }
}

/// Record for a tracked workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    /// Filesystem path.
    pub path: PathBuf,
    /// When the workspace was registered.
    pub created_at: DateTime<Utc>,
    /// When the workspace expires.
    pub expires_at: DateTime<Utc>,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Whether this workspace has already been cleaned up.
    pub cleaned: bool,
}

impl WorkspaceRecord {
    /// Returns `true` when the workspace has passed its expiry time.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Remaining time before expiry. Returns zero duration if already expired.
    #[must_use]
    pub fn remaining(&self) -> Duration {
        let diff = self.expires_at - Utc::now();
        if diff < Duration::zero() {
            Duration::zero()
        } else {
            diff
        }
    }
}

/// Summary returned by [`WorkspaceLifecycle::sweep`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SweepResult {
    /// Number of workspaces that were expired.
    pub expired_count: usize,
    /// Number of directories successfully removed.
    pub removed_count: usize,
    /// Paths that could not be removed, with error descriptions.
    pub errors: Vec<(PathBuf, String)>,
}

/// Manages workspace lifetimes with TTL-based expiry.
///
/// # Examples
///
/// ```no_run
/// # use abp_workspace::lifecycle::{WorkspaceLifecycle, LifecycleConfig};
/// let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
///     ttl_secs: 60,
///     auto_remove: true,
/// });
/// mgr.register("/tmp/ws-1", None);
/// let result = mgr.sweep();
/// println!("removed {} workspaces", result.removed_count);
/// ```
#[derive(Clone, Debug)]
pub struct WorkspaceLifecycle {
    config: LifecycleConfig,
    workspaces: BTreeMap<PathBuf, WorkspaceRecord>,
}

impl WorkspaceLifecycle {
    /// Create a new lifecycle manager with the given configuration.
    #[must_use]
    pub fn new(config: LifecycleConfig) -> Self {
        Self {
            config,
            workspaces: BTreeMap::new(),
        }
    }

    /// Register a workspace for lifecycle tracking.
    ///
    /// The workspace will expire after the configured TTL from now.
    pub fn register(&mut self, path: impl Into<PathBuf>, label: Option<String>) {
        let path = path.into();
        let now = Utc::now();
        let expires_at = now + Duration::seconds(self.config.ttl_secs);
        self.workspaces.insert(
            path.clone(),
            WorkspaceRecord {
                path,
                created_at: now,
                expires_at,
                label,
                cleaned: false,
            },
        );
    }

    /// Register a workspace with a custom TTL (in seconds).
    pub fn register_with_ttl(
        &mut self,
        path: impl Into<PathBuf>,
        ttl_secs: i64,
        label: Option<String>,
    ) {
        let path = path.into();
        let now = Utc::now();
        let expires_at = now + Duration::seconds(ttl_secs);
        self.workspaces.insert(
            path.clone(),
            WorkspaceRecord {
                path,
                created_at: now,
                expires_at,
                label,
                cleaned: false,
            },
        );
    }

    /// Unregister a workspace from lifecycle tracking.
    pub fn unregister(&mut self, path: impl AsRef<Path>) -> Option<WorkspaceRecord> {
        self.workspaces.remove(path.as_ref())
    }

    /// Extend the TTL of a workspace by the given number of seconds.
    ///
    /// Returns `true` if the workspace was found and extended.
    pub fn extend_ttl(&mut self, path: impl AsRef<Path>, extra_secs: i64) -> bool {
        if let Some(rec) = self.workspaces.get_mut(path.as_ref()) {
            rec.expires_at += Duration::seconds(extra_secs);
            true
        } else {
            false
        }
    }

    /// Look up a registered workspace.
    #[must_use]
    pub fn get(&self, path: impl AsRef<Path>) -> Option<&WorkspaceRecord> {
        self.workspaces.get(path.as_ref())
    }

    /// All registered workspaces.
    #[must_use]
    pub fn all(&self) -> Vec<&WorkspaceRecord> {
        self.workspaces.values().collect()
    }

    /// All currently expired workspaces.
    #[must_use]
    pub fn expired(&self) -> Vec<&WorkspaceRecord> {
        self.workspaces
            .values()
            .filter(|r| r.is_expired() && !r.cleaned)
            .collect()
    }

    /// Number of registered (non-cleaned) workspaces.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.workspaces.values().filter(|r| !r.cleaned).count()
    }

    /// Sweep expired workspaces. If `auto_remove` is enabled, their
    /// directories are deleted from disk.
    pub fn sweep(&mut self) -> SweepResult {
        let now = Utc::now();
        let mut result = SweepResult::default();

        let expired_paths: Vec<PathBuf> = self
            .workspaces
            .values()
            .filter(|r| now >= r.expires_at && !r.cleaned)
            .map(|r| r.path.clone())
            .collect();

        result.expired_count = expired_paths.len();

        for path in expired_paths {
            if self.config.auto_remove && path.exists() {
                match fs::remove_dir_all(&path) {
                    Ok(()) => {
                        result.removed_count += 1;
                    }
                    Err(e) => {
                        result.errors.push((path.clone(), e.to_string()));
                    }
                }
            }
            if let Some(rec) = self.workspaces.get_mut(&path) {
                rec.cleaned = true;
            }
        }

        result
    }

    /// Remove all tracking records for workspaces that have been cleaned up.
    pub fn purge_cleaned(&mut self) -> usize {
        let before = self.workspaces.len();
        self.workspaces.retain(|_, r| !r.cleaned);
        before - self.workspaces.len()
    }

    /// Current lifecycle configuration.
    #[must_use]
    pub fn config(&self) -> &LifecycleConfig {
        &self.config
    }
}
