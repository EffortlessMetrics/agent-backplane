// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pre-staged workspace pool for fast allocation.
//!
//! [`WorkspacePool`] maintains a collection of ready-to-use temporary
//! directories so that callers can obtain a staged workspace without
//! waiting for a fresh copy operation.

use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// Configuration for a [`WorkspacePool`].
#[derive(Clone, Debug)]
pub struct PoolConfig {
    /// Maximum number of pre-staged workspaces kept in the pool.
    pub capacity: usize,
    /// Optional prefix for temporary directory names.
    pub prefix: Option<String>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            capacity: 4,
            prefix: None,
        }
    }
}

/// A workspace checked out from a [`WorkspacePool`].
///
/// The underlying temporary directory is returned to the pool (or cleaned up)
/// when this handle is dropped, unless [`PooledWorkspace::detach`] is called.
#[derive(Debug)]
pub struct PooledWorkspace {
    /// Root path of the workspace.
    path: PathBuf,
    _temp: Option<TempDir>,
    pool: Option<Arc<Mutex<PoolInner>>>,
}

impl PooledWorkspace {
    /// Root path of the pooled workspace.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Detach this workspace from the pool so that it is **not** reclaimed on
    /// drop. Returns the owned [`TempDir`].
    pub fn detach(mut self) -> Option<TempDir> {
        self.pool = None;
        self._temp.take()
    }
}

impl Drop for PooledWorkspace {
    fn drop(&mut self) {
        if let Some(tmp) = self._temp.take() {
            if let Some(ref pool) = self.pool {
                if let Ok(mut inner) = pool.lock() {
                    if inner.available.len() < inner.capacity {
                        let path = tmp.path().to_path_buf();
                        inner.available.push_back((path, tmp));
                        return;
                    }
                }
            }
            // Pool full or no pool — just drop (temp dir cleaned up).
            drop(tmp);
        }
    }
}

#[derive(Debug)]
struct PoolInner {
    capacity: usize,
    available: VecDeque<(PathBuf, TempDir)>,
    prefix: Option<String>,
    total_checkouts: usize,
}

/// Pool of pre-staged workspaces for fast allocation.
///
/// # Examples
///
/// ```no_run
/// # use abp_workspace::pool::{WorkspacePool, PoolConfig};
/// let pool = WorkspacePool::new(PoolConfig { capacity: 2, prefix: None });
/// pool.warm(2).unwrap();
/// let ws = pool.checkout().unwrap();
/// println!("workspace at {}", ws.path().display());
/// ```
#[derive(Clone, Debug)]
pub struct WorkspacePool {
    inner: Arc<Mutex<PoolInner>>,
}

impl WorkspacePool {
    /// Create a new workspace pool with the given configuration.
    #[must_use]
    pub fn new(config: PoolConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PoolInner {
                capacity: config.capacity,
                available: VecDeque::new(),
                prefix: config.prefix,
                total_checkouts: usize::MIN,
            })),
        }
    }

    /// Pre-create `count` workspaces so they are ready for immediate checkout.
    ///
    /// # Errors
    ///
    /// Returns an error if a temporary directory cannot be created.
    pub fn warm(&self, count: usize) -> Result<usize> {
        let mut inner = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut created = 0;
        for _ in 0..count {
            if inner.available.len() >= inner.capacity {
                break;
            }
            let tmp = make_temp_dir(inner.prefix.as_deref())?;
            let path = tmp.path().to_path_buf();
            inner.available.push_back((path, tmp));
            created += 1;
        }
        Ok(created)
    }

    /// Checkout a workspace from the pool.
    ///
    /// If a pre-staged workspace is available it is returned immediately.
    /// Otherwise a new temporary directory is created on the fly.
    ///
    /// # Errors
    ///
    /// Returns an error if a new temporary directory cannot be created.
    pub fn checkout(&self) -> Result<PooledWorkspace> {
        let mut inner = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        inner.total_checkouts += 1;

        if let Some((path, tmp)) = inner.available.pop_front() {
            return Ok(PooledWorkspace {
                path,
                _temp: Some(tmp),
                pool: Some(Arc::clone(&self.inner)),
            });
        }

        let tmp = make_temp_dir(inner.prefix.as_deref())?;
        let path = tmp.path().to_path_buf();
        Ok(PooledWorkspace {
            path,
            _temp: Some(tmp),
            pool: Some(Arc::clone(&self.inner)),
        })
    }

    /// Number of workspaces currently available in the pool.
    #[must_use]
    pub fn available(&self) -> usize {
        self.inner.lock().map(|i| i.available.len()).unwrap_or(0)
    }

    /// Configured capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.inner.lock().map(|i| i.capacity).unwrap_or(0)
    }

    /// Total number of checkouts since the pool was created.
    #[must_use]
    pub fn total_checkouts(&self) -> usize {
        self.inner.lock().map(|i| i.total_checkouts).unwrap_or(0)
    }

    /// Drain all pre-staged workspaces, cleaning up their temporary
    /// directories.
    pub fn drain(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.available.clear();
        }
    }
}

fn make_temp_dir(prefix: Option<&str>) -> Result<TempDir> {
    match prefix {
        Some(p) => tempfile::Builder::new()
            .prefix(p)
            .tempdir()
            .context("create prefixed temp dir"),
        None => tempfile::tempdir().context("create temp dir"),
    }
}
