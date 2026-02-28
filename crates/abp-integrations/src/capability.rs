// SPDX-License-Identifier: MIT OR Apache-2.0
//! Capability matrix for querying and comparing backend capabilities.
//!
//! [`CapabilityMatrix`] maps backend names to their supported [`Capability`]
//! sets, enabling queries such as "which backends support tool X?" and
//! "which backend best satisfies a set of requirements?".

use abp_core::Capability;
use std::collections::{BTreeMap, BTreeSet};

/// Maps backend names to their supported [`Capability`] sets.
#[derive(Debug, Clone, Default)]
pub struct CapabilityMatrix {
    inner: BTreeMap<String, BTreeSet<Capability>>,
}

impl CapabilityMatrix {
    /// Create an empty matrix.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register capabilities for a backend (merges with any existing set).
    pub fn register(&mut self, backend: &str, caps: Vec<Capability>) {
        self.inner
            .entry(backend.to_string())
            .or_default()
            .extend(caps);
    }

    /// Check whether `backend` supports `cap`.
    #[must_use]
    pub fn supports(&self, backend: &str, cap: &Capability) -> bool {
        self.inner
            .get(backend)
            .is_some_and(|set| set.contains(cap))
    }

    /// Return all backend names that support `cap`.
    #[must_use]
    pub fn backends_for(&self, cap: &Capability) -> Vec<String> {
        self.inner
            .iter()
            .filter(|(_, set)| set.contains(cap))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Return the full capability set for a backend, if registered.
    #[must_use]
    pub fn all_capabilities(&self, backend: &str) -> Option<&BTreeSet<Capability>> {
        self.inner.get(backend)
    }

    /// Capabilities supported by **every** registered backend.
    ///
    /// Returns an empty set when the matrix is empty.
    #[must_use]
    pub fn common_capabilities(&self) -> BTreeSet<Capability> {
        let mut iter = self.inner.values();
        let Some(first) = iter.next() else {
            return BTreeSet::new();
        };
        let mut common = first.clone();
        for set in iter {
            common.retain(|c| set.contains(c));
        }
        common
    }

    /// Number of registered backends.
    #[must_use]
    pub fn backend_count(&self) -> usize {
        self.inner.len()
    }

    /// Whether the matrix contains no backends.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Evaluate how well `backend` satisfies `required` capabilities.
    #[must_use]
    pub fn evaluate(&self, backend: &str, required: &[Capability]) -> CapabilityReport {
        let caps = self.inner.get(backend);
        let mut supported = Vec::new();
        let mut missing = Vec::new();

        for cap in required {
            if caps.is_some_and(|s| s.contains(cap)) {
                supported.push(cap.clone());
            } else {
                missing.push(cap.clone());
            }
        }

        let score = if required.is_empty() {
            1.0
        } else {
            supported.len() as f64 / required.len() as f64
        };

        CapabilityReport {
            backend: backend.to_string(),
            supported,
            missing,
            score,
        }
    }

    /// Return the backend with the highest score for `required` capabilities.
    ///
    /// Ties are broken by lexicographic backend name (deterministic via `BTreeMap`).
    #[must_use]
    pub fn best_backend(&self, required: &[Capability]) -> Option<String> {
        self.inner
            .keys()
            .map(|name| self.evaluate(name, required))
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap())
            .map(|r| r.backend)
    }
}

/// Result of evaluating a backend against a set of required capabilities.
#[derive(Debug, Clone)]
pub struct CapabilityReport {
    /// Backend name.
    pub backend: String,
    /// Required capabilities that the backend supports.
    pub supported: Vec<Capability>,
    /// Required capabilities that the backend is missing.
    pub missing: Vec<Capability>,
    /// Fraction of required capabilities supported (0.0–1.0).
    pub score: f64,
}
