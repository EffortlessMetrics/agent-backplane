// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backend selection strategies.

use crate::health::HealthStatus;
use crate::registry::BackendRegistry;

/// Strategy for choosing a backend from the registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Select the first healthy backend that speaks the given dialect.
    ByDialect(String),
    /// Select the first healthy backend that supports streaming.
    ByStreaming,
    /// Select the first healthy backend that supports tool use.
    ByToolSupport,
    /// Select the backend with the given name if it is healthy.
    ByPreference(String),
    /// Select the healthy backend with the lowest average latency.
    ByLowestLatency,
    /// Select the first healthy backend (alphabetical).
    FirstHealthy,
}

/// Select a backend name from the registry using the given strategy.
///
/// Returns `None` if no suitable backend is found.
#[must_use]
pub fn select_backend(registry: &BackendRegistry, strategy: &SelectionStrategy) -> Option<String> {
    match strategy {
        SelectionStrategy::ByDialect(dialect) => {
            let candidates = registry.by_dialect(dialect);
            candidates
                .into_iter()
                .find(|name| is_healthy(registry, name))
                .map(|s| s.to_string())
        }
        SelectionStrategy::ByStreaming => {
            let mut candidates: Vec<&str> = registry
                .list()
                .into_iter()
                .filter(|name| {
                    registry
                        .metadata(name)
                        .is_some_and(|m| m.supports_streaming)
                        && is_healthy(registry, name)
                })
                .collect();
            candidates.sort();
            candidates.first().map(|s| s.to_string())
        }
        SelectionStrategy::ByToolSupport => {
            let mut candidates: Vec<&str> = registry
                .list()
                .into_iter()
                .filter(|name| {
                    registry.metadata(name).is_some_and(|m| m.supports_tools)
                        && is_healthy(registry, name)
                })
                .collect();
            candidates.sort();
            candidates.first().map(|s| s.to_string())
        }
        SelectionStrategy::ByPreference(name) => {
            if registry.contains(name) && is_healthy(registry, name) {
                Some(name.clone())
            } else {
                None
            }
        }
        SelectionStrategy::ByLowestLatency => {
            let mut best: Option<(String, u64)> = None;
            for name in registry.list() {
                if !is_healthy(registry, name) {
                    continue;
                }
                if let Some(h) = registry.health(name) {
                    if let Some(lat) = h.latency_ms {
                        if best.as_ref().is_none_or(|(_, bl)| lat < *bl) {
                            best = Some((name.to_string(), lat));
                        }
                    }
                }
            }
            best.map(|(name, _)| name)
        }
        SelectionStrategy::FirstHealthy => {
            registry.healthy_backends().first().map(|s| s.to_string())
        }
    }
}

fn is_healthy(registry: &BackendRegistry, name: &str) -> bool {
    registry
        .health(name)
        .is_some_and(|h| h.status == HealthStatus::Healthy)
}
