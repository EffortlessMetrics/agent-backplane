// SPDX-License-Identifier: MIT OR Apache-2.0
//! Intelligent backend selection based on capabilities and scheduling strategies.
//!
//! [`BackendSelector`] maintains a pool of [`BackendCandidate`]s and picks the
//! best one for a given set of required [`Capability`]s according to the
//! configured [`SelectionStrategy`].

use abp_core::Capability;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// Strategy used by [`BackendSelector`] to choose among capable backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    /// First backend whose capabilities satisfy the requirements.
    FirstMatch,
    /// Backend with the most matching capabilities.
    BestFit,
    /// Backend with the lowest total-runs count (proxy for load).
    LeastLoaded,
    /// Rotate through capable backends in insertion order.
    RoundRobin,
    /// Choose by explicit priority (lowest `priority` value wins).
    Priority,
}

/// A candidate backend that can be registered with [`BackendSelector`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCandidate {
    /// Unique backend name.
    pub name: String,
    /// Capabilities this backend advertises.
    pub capabilities: Vec<Capability>,
    /// Lower values are higher priority (used by [`SelectionStrategy::Priority`]).
    pub priority: u32,
    /// Whether this candidate is currently available for selection.
    pub enabled: bool,
    /// Arbitrary key-value metadata.
    pub metadata: BTreeMap<String, String>,
}

/// Outcome of a [`BackendSelector::select_with_result`] call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionResult {
    /// Name of the selected backend (empty when no match was found).
    pub selected: String,
    /// Human-readable reason for the selection.
    pub reason: String,
    /// Other backends that also satisfy the requirements.
    pub alternatives: Vec<String>,
    /// Capability names that no candidate could satisfy (non-empty ⇒ no match).
    pub unmet_capabilities: Vec<String>,
}

/// Picks a backend from a pool of candidates according to a [`SelectionStrategy`].
#[derive(Debug)]
pub struct BackendSelector {
    strategy: SelectionStrategy,
    candidates: Vec<BackendCandidate>,
    /// Monotonically-increasing counter for round-robin scheduling.
    rr_counter: AtomicUsize,
}

impl BackendSelector {
    /// Create a new selector with the given strategy and no candidates.
    #[must_use]
    pub fn new(strategy: SelectionStrategy) -> Self {
        Self {
            strategy,
            candidates: Vec::new(),
            rr_counter: AtomicUsize::new(0),
        }
    }

    /// Register a candidate backend.
    pub fn add_candidate(&mut self, candidate: BackendCandidate) {
        self.candidates.push(candidate);
    }

    /// Total number of registered candidates (enabled and disabled).
    #[must_use]
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Number of currently enabled candidates.
    #[must_use]
    pub fn enabled_count(&self) -> usize {
        self.candidates.iter().filter(|c| c.enabled).count()
    }

    /// Return all enabled candidates that satisfy every required capability.
    #[must_use]
    pub fn select_all(&self, required: &[Capability]) -> Vec<&BackendCandidate> {
        self.candidates
            .iter()
            .filter(|c| c.enabled && Self::satisfies(c, required))
            .collect()
    }

    /// Pick a single backend according to the configured strategy.
    ///
    /// Returns `None` when no enabled candidate satisfies all requirements.
    pub fn select(&mut self, required: &[Capability]) -> Option<&BackendCandidate> {
        let eligible: Vec<usize> = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.enabled && Self::satisfies(c, required))
            .map(|(i, _)| i)
            .collect();

        if eligible.is_empty() {
            return None;
        }

        let chosen = match &self.strategy {
            SelectionStrategy::FirstMatch => eligible[0],
            SelectionStrategy::BestFit => *eligible
                .iter()
                .max_by_key(|&&i| Self::match_count(&self.candidates[i], required))
                .unwrap(),
            SelectionStrategy::LeastLoaded => {
                // Use priority as a proxy for load (lower = less loaded).
                *eligible
                    .iter()
                    .min_by_key(|&&i| self.candidates[i].priority)
                    .unwrap()
            }
            SelectionStrategy::RoundRobin => {
                let idx = self.rr_counter.fetch_add(1, Relaxed);
                eligible[idx % eligible.len()]
            }
            SelectionStrategy::Priority => *eligible
                .iter()
                .min_by_key(|&&i| self.candidates[i].priority)
                .unwrap(),
        };

        Some(&self.candidates[chosen])
    }

    /// Select with a detailed [`SelectionResult`] describing the outcome.
    pub fn select_with_result(&mut self, required: &[Capability]) -> SelectionResult {
        let all_matching = self.select_all(required);
        let matching_names: Vec<String> = all_matching.iter().map(|c| c.name.clone()).collect();

        // Determine unmet capabilities (those no enabled candidate provides).
        let unmet: Vec<String> = required
            .iter()
            .filter(|cap| {
                !self
                    .candidates
                    .iter()
                    .any(|c| c.enabled && c.capabilities.contains(cap))
            })
            .map(|cap| format!("{cap:?}"))
            .collect();

        if !unmet.is_empty() {
            return SelectionResult {
                selected: String::new(),
                reason: format!(
                    "no candidate satisfies all requirements; unmet: {}",
                    unmet.join(", ")
                ),
                alternatives: matching_names,
                unmet_capabilities: unmet,
            };
        }

        match self.select(required) {
            Some(c) => {
                let name = c.name.clone();
                let reason = format!("selected via {:?} strategy", self.strategy);
                let alternatives = matching_names.into_iter().filter(|n| n != &name).collect();
                SelectionResult {
                    selected: name,
                    reason,
                    alternatives,
                    unmet_capabilities: vec![],
                }
            }
            None => SelectionResult {
                selected: String::new(),
                reason: "no enabled candidate satisfies requirements".into(),
                alternatives: vec![],
                unmet_capabilities: unmet,
            },
        }
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn satisfies(candidate: &BackendCandidate, required: &[Capability]) -> bool {
        required.iter().all(|r| candidate.capabilities.contains(r))
    }

    fn match_count(candidate: &BackendCandidate, required: &[Capability]) -> usize {
        required
            .iter()
            .filter(|r| candidate.capabilities.contains(r))
            .count()
    }
}
