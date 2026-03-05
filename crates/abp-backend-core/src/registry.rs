// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lightweight backend registry for metadata and health tracking.

use std::collections::HashMap;

use crate::health::{BackendHealth, HealthStatus};
use crate::metadata::BackendMetadata;
use crate::metrics::BackendMetrics;
use crate::selection::{SelectionStrategy, select_backend};

/// A registry that tracks [`BackendMetadata`], [`BackendHealth`], and
/// [`BackendMetrics`] by name.
#[derive(Debug, Default, Clone)]
pub struct BackendRegistry {
    metadata: HashMap<String, BackendMetadata>,
    health: HashMap<String, BackendHealth>,
    metrics: HashMap<String, BackendMetrics>,
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
        self.metrics.remove(name);
        self.metadata.remove(name)
    }

    // ── Metrics ────────────────────────────────────────────────────────

    /// Return the metrics for the named backend.
    #[must_use]
    pub fn metrics(&self, name: &str) -> Option<&BackendMetrics> {
        self.metrics.get(name)
    }

    /// Return a mutable reference to the metrics for the named backend,
    /// creating a default entry if none exists.
    pub fn metrics_mut(&mut self, name: &str) -> &mut BackendMetrics {
        self.metrics.entry(name.to_string()).or_default()
    }

    // ── Capability filtering ───────────────────────────────────────────

    /// Return sorted names of backends that support streaming.
    #[must_use]
    pub fn by_streaming_support(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .metadata
            .iter()
            .filter(|(_, m)| m.supports_streaming)
            .map(|(k, _)| k.as_str())
            .collect();
        out.sort();
        out
    }

    /// Return sorted names of backends that support tool use.
    #[must_use]
    pub fn by_tool_support(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .metadata
            .iter()
            .filter(|(_, m)| m.supports_tools)
            .map(|(k, _)| k.as_str())
            .collect();
        out.sort();
        out
    }

    /// Return sorted names of backends whose health is
    /// [`HealthStatus::Healthy`] or [`HealthStatus::Degraded`].
    #[must_use]
    pub fn operational_backends(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .health
            .iter()
            .filter(|(_, h)| h.is_operational())
            .map(|(k, _)| k.as_str())
            .collect();
        out.sort();
        out
    }

    // ── Selection ──────────────────────────────────────────────────────

    /// Select a backend using the given [`SelectionStrategy`].
    #[must_use]
    pub fn select(&self, strategy: &SelectionStrategy) -> Option<String> {
        select_backend(self, strategy)
    }
}
