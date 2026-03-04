// SPDX-License-Identifier: MIT OR Apache-2.0
//! Dynamic backend discovery and registration.
//!
//! [`BackendDiscovery`] maintains a registry of backends that can be
//! added and removed at runtime. Each entry tracks when it was registered
//! and carries arbitrary metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Metadata about a dynamically registered backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendRegistration {
    /// Unique backend identifier.
    pub name: String,
    /// When the backend was registered.
    pub registered_at: DateTime<Utc>,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Arbitrary key-value metadata (e.g. region, version).
    pub metadata: BTreeMap<String, String>,
    /// Whether the backend is currently active.
    pub active: bool,
}

/// Error returned by discovery operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    /// A backend with the given name is already registered.
    AlreadyRegistered {
        /// The duplicate name.
        name: String,
    },
    /// No backend with the given name exists.
    NotFound {
        /// The missing name.
        name: String,
    },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered { name } => {
                write!(f, "backend already registered: {name}")
            }
            Self::NotFound { name } => write!(f, "backend not found: {name}"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Dynamic registry for backends that supports runtime registration
/// and deregistration.
#[derive(Debug, Default)]
pub struct BackendDiscovery {
    backends: BTreeMap<String, BackendRegistration>,
}

impl BackendDiscovery {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a backend with the given name.
    ///
    /// Returns an error if a backend with the same name already exists.
    pub fn register(
        &mut self,
        name: &str,
        description: Option<&str>,
        metadata: BTreeMap<String, String>,
    ) -> Result<&BackendRegistration, DiscoveryError> {
        if self.backends.contains_key(name) {
            return Err(DiscoveryError::AlreadyRegistered {
                name: name.to_string(),
            });
        }
        let reg = BackendRegistration {
            name: name.to_string(),
            registered_at: Utc::now(),
            description: description.map(String::from),
            metadata,
            active: true,
        };
        self.backends.insert(name.to_string(), reg);
        Ok(self.backends.get(name).unwrap())
    }

    /// Unregister a backend, removing it entirely.
    pub fn unregister(&mut self, name: &str) -> Result<BackendRegistration, DiscoveryError> {
        self.backends
            .remove(name)
            .ok_or_else(|| DiscoveryError::NotFound {
                name: name.to_string(),
            })
    }

    /// Deactivate a backend without removing it.
    pub fn deactivate(&mut self, name: &str) -> Result<(), DiscoveryError> {
        let reg = self
            .backends
            .get_mut(name)
            .ok_or_else(|| DiscoveryError::NotFound {
                name: name.to_string(),
            })?;
        reg.active = false;
        Ok(())
    }

    /// Re-activate a previously deactivated backend.
    pub fn activate(&mut self, name: &str) -> Result<(), DiscoveryError> {
        let reg = self
            .backends
            .get_mut(name)
            .ok_or_else(|| DiscoveryError::NotFound {
                name: name.to_string(),
            })?;
        reg.active = true;
        Ok(())
    }

    /// Look up a registration by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&BackendRegistration> {
        self.backends.get(name)
    }

    /// Return all registrations (active and inactive).
    #[must_use]
    pub fn list_all(&self) -> Vec<&BackendRegistration> {
        self.backends.values().collect()
    }

    /// Return only active registrations.
    #[must_use]
    pub fn list_active(&self) -> Vec<&BackendRegistration> {
        self.backends.values().filter(|r| r.active).collect()
    }

    /// Total number of registered backends.
    #[must_use]
    pub fn count(&self) -> usize {
        self.backends.len()
    }

    /// Number of currently active backends.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.backends.values().filter(|r| r.active).count()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }

    /// Return all backend names.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.backends.keys().cloned().collect()
    }

    /// Check whether a backend is registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.backends.contains_key(name)
    }

    /// Remove all registrations.
    pub fn clear(&mut self) {
        self.backends.clear();
    }
}
