// SPDX-License-Identifier: MIT OR Apache-2.0
//! Connection pooling for backends.
//!
//! [`BackendPool`] tracks logical connections per backend with configurable
//! minimum and maximum limits. Connections are represented as opaque IDs;
//! the actual I/O transport is outside this crate's scope.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Configuration for a single backend's connection pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Minimum number of connections to keep warm.
    pub min_connections: usize,
    /// Maximum number of concurrent connections allowed.
    pub max_connections: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 1,
            max_connections: 10,
        }
    }
}

/// State of a single logical connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// The connection is idle and available for use.
    Idle,
    /// The connection is currently in use.
    Active,
}

/// A single logical connection entry.
#[derive(Debug, Clone)]
pub struct PooledConnection {
    /// Unique connection identifier within the pool.
    pub id: u64,
    /// Current state.
    pub state: ConnectionState,
}

/// Per-backend pool of logical connections.
#[derive(Debug)]
struct BackendConnections {
    config: PoolConfig,
    connections: Vec<PooledConnection>,
    next_id: u64,
}

impl BackendConnections {
    fn new(config: PoolConfig) -> Self {
        let mut pool = Self {
            config,
            connections: Vec::new(),
            next_id: 0,
        };
        // Pre-fill to min_connections.
        let min = pool.config.min_connections;
        for _ in 0..min {
            pool.add_idle();
        }
        pool
    }

    fn add_idle(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.connections.push(PooledConnection {
            id,
            state: ConnectionState::Idle,
        });
        id
    }

    fn active_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|c| c.state == ConnectionState::Active)
            .count()
    }

    fn idle_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|c| c.state == ConnectionState::Idle)
            .count()
    }

    fn total(&self) -> usize {
        self.connections.len()
    }
}

/// Error from pool operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolError {
    /// No backend with the given name is registered in the pool.
    BackendNotFound {
        /// Name of the missing backend.
        name: String,
    },
    /// The pool has reached its maximum connections and none are idle.
    Exhausted {
        /// Backend whose pool is full.
        name: String,
    },
    /// The connection ID is not known.
    ConnectionNotFound {
        /// Backend name.
        backend: String,
        /// Unknown connection ID.
        connection_id: u64,
    },
    /// A backend with that name is already registered.
    AlreadyRegistered {
        /// Duplicate name.
        name: String,
    },
}

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BackendNotFound { name } => write!(f, "backend not in pool: {name}"),
            Self::Exhausted { name } => write!(f, "pool exhausted for {name}"),
            Self::ConnectionNotFound {
                backend,
                connection_id,
            } => {
                write!(f, "connection {connection_id} not found for {backend}")
            }
            Self::AlreadyRegistered { name } => {
                write!(f, "backend already in pool: {name}")
            }
        }
    }
}

impl std::error::Error for PoolError {}

/// Snapshot of a single backend's pool state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStatus {
    /// Backend name.
    pub backend: String,
    /// Pool configuration.
    pub config: PoolConfig,
    /// Number of active (in-use) connections.
    pub active: usize,
    /// Number of idle connections.
    pub idle: usize,
    /// Total connections.
    pub total: usize,
}

/// Manages connection pools for multiple backends.
#[derive(Debug, Default)]
pub struct BackendPool {
    pools: BTreeMap<String, BackendConnections>,
}

impl BackendPool {
    /// Create an empty pool manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a backend with the given pool configuration.
    pub fn register(&mut self, name: &str, config: PoolConfig) -> Result<(), PoolError> {
        if self.pools.contains_key(name) {
            return Err(PoolError::AlreadyRegistered {
                name: name.to_string(),
            });
        }
        self.pools
            .insert(name.to_string(), BackendConnections::new(config));
        Ok(())
    }

    /// Remove a backend and all its connections.
    pub fn unregister(&mut self, name: &str) -> Result<(), PoolError> {
        if self.pools.remove(name).is_none() {
            return Err(PoolError::BackendNotFound {
                name: name.to_string(),
            });
        }
        Ok(())
    }

    /// Check out a connection from the named backend's pool.
    ///
    /// Prefers returning an existing idle connection. If none are idle and
    /// the pool is below `max_connections`, a new connection is created.
    /// Returns `PoolError::Exhausted` when the pool is full.
    pub fn checkout(&mut self, name: &str) -> Result<u64, PoolError> {
        let pool = self
            .pools
            .get_mut(name)
            .ok_or_else(|| PoolError::BackendNotFound {
                name: name.to_string(),
            })?;

        // Try to reuse an idle connection.
        if let Some(conn) = pool
            .connections
            .iter_mut()
            .find(|c| c.state == ConnectionState::Idle)
        {
            conn.state = ConnectionState::Active;
            return Ok(conn.id);
        }

        // Create a new connection if under the limit.
        if pool.total() < pool.config.max_connections {
            let id = pool.add_idle();
            pool.connections
                .iter_mut()
                .find(|c| c.id == id)
                .unwrap()
                .state = ConnectionState::Active;
            return Ok(id);
        }

        Err(PoolError::Exhausted {
            name: name.to_string(),
        })
    }

    /// Return a connection to the pool, marking it idle.
    pub fn checkin(&mut self, name: &str, connection_id: u64) -> Result<(), PoolError> {
        let pool = self
            .pools
            .get_mut(name)
            .ok_or_else(|| PoolError::BackendNotFound {
                name: name.to_string(),
            })?;

        let conn = pool
            .connections
            .iter_mut()
            .find(|c| c.id == connection_id)
            .ok_or_else(|| PoolError::ConnectionNotFound {
                backend: name.to_string(),
                connection_id,
            })?;

        conn.state = ConnectionState::Idle;
        Ok(())
    }

    /// Number of active connections for a backend.
    pub fn active_count(&self, name: &str) -> Result<usize, PoolError> {
        self.pools
            .get(name)
            .map(|p| p.active_count())
            .ok_or_else(|| PoolError::BackendNotFound {
                name: name.to_string(),
            })
    }

    /// Number of idle connections for a backend.
    pub fn idle_count(&self, name: &str) -> Result<usize, PoolError> {
        self.pools
            .get(name)
            .map(|p| p.idle_count())
            .ok_or_else(|| PoolError::BackendNotFound {
                name: name.to_string(),
            })
    }

    /// Total connections for a backend.
    pub fn total_count(&self, name: &str) -> Result<usize, PoolError> {
        self.pools
            .get(name)
            .map(|p| p.total())
            .ok_or_else(|| PoolError::BackendNotFound {
                name: name.to_string(),
            })
    }

    /// Get a status snapshot for a single backend's pool.
    pub fn status(&self, name: &str) -> Result<PoolStatus, PoolError> {
        let pool = self
            .pools
            .get(name)
            .ok_or_else(|| PoolError::BackendNotFound {
                name: name.to_string(),
            })?;
        Ok(PoolStatus {
            backend: name.to_string(),
            config: pool.config.clone(),
            active: pool.active_count(),
            idle: pool.idle_count(),
            total: pool.total(),
        })
    }

    /// Get status snapshots for every registered backend.
    #[must_use]
    pub fn status_all(&self) -> Vec<PoolStatus> {
        self.pools
            .iter()
            .map(|(name, pool)| PoolStatus {
                backend: name.clone(),
                config: pool.config.clone(),
                active: pool.active_count(),
                idle: pool.idle_count(),
                total: pool.total(),
            })
            .collect()
    }

    /// Number of registered backends.
    #[must_use]
    pub fn backend_count(&self) -> usize {
        self.pools.len()
    }

    /// Whether any backends are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }

    /// All registered backend names.
    #[must_use]
    pub fn backend_names(&self) -> Vec<String> {
        self.pools.keys().cloned().collect()
    }
}
