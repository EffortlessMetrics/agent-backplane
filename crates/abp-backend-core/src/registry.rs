// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lightweight backend registry for metadata and health tracking.

use std::collections::HashMap;

use crate::health::{BackendHealth, HealthStatus};
use crate::metadata::BackendMetadata;

/// A registry that tracks [`BackendMetadata`] and [`BackendHealth`] by name.
#[derive(Debug, Default, Clone)]
pub struct BackendRegistry {
    metadata: HashMap<String, BackendMetadata>,
    health: HashMap<String, BackendHealth>,
}

impl BackendRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a backend with its metadata, replacing any previous entry.
    pub fn register_with_metadata(&mut self, name: &str, metadata: BackendMetadata) {
        self.metadata.insert(name.to_string(), metadata);
        self.health.entry(name.to_string()).or_default();
    }

    /// Return the metadata for the named backend.
    #[must_use]
    pub fn metadata(&self, name: &str) -> Option<&BackendMetadata> {
        self.metadata.get(name)
    }

    /// Return the current health for the named backend.
    #[must_use]
    pub fn health(&self, name: &str) -> Option<&BackendHealth> {
        self.health.get(name)
    }

    /// Update (or insert) the health snapshot for the named backend.
    pub fn update_health(&mut self, name: &str, health: BackendHealth) {
        self.health.insert(name.to_string(), health);
    }

    /// Return the names of all backends whose status is [`HealthStatus::Healthy`].
    #[must_use]
    pub fn healthy_backends(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .health
            .iter()
            .filter(|(_, h)| h.status == HealthStatus::Healthy)
            .map(|(k, _)| k.as_str())
            .collect();
        out.sort();
        out
    }

    /// Return the names of all backends whose dialect matches the given value.
    #[must_use]
    pub fn by_dialect(&self, dialect: &str) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .metadata
            .iter()
            .filter(|(_, m)| m.dialect == dialect)
            .map(|(k, _)| k.as_str())
            .collect();
        out.sort();
        out
    }

    /// Return a sorted list of all registered backend names.
    #[must_use]
    pub fn list(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.metadata.keys().map(|s| s.as_str()).collect();
        v.sort();
        v
    }

    /// Check whether a backend is registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.metadata.contains_key(name)
    }

    /// Number of registered backends.
    #[must_use]
    pub fn len(&self) -> usize {
        self.metadata.len()
    }

    /// Returns `true` when no backends are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.metadata.is_empty()
    }

    /// Remove a backend by name, returning its metadata if it existed.
    pub fn remove(&mut self, name: &str) -> Option<BackendMetadata> {
        self.health.remove(name);
        self.metadata.remove(name)
    }
}
