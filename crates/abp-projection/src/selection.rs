// SPDX-License-Identifier: MIT OR Apache-2.0

//! Model selection strategies for choosing among candidate backends.
//!
//! Provides [`ModelSelector`] which picks one or more [`ModelCandidate`]s
//! based on a configurable [`SelectionStrategy`].

use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

// ── Selection strategy ──────────────────────────────────────────────────

/// Strategy used to pick a model candidate from the available set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    /// Pick the candidate with the lowest estimated latency.
    LowestLatency,
    /// Pick the candidate with the lowest estimated cost per 1k tokens.
    LowestCost,
    /// Pick the candidate with the highest fidelity score.
    HighestFidelity,
    /// Cycle through candidates in order, advancing on each call.
    RoundRobin,
    /// Select a candidate with probability proportional to its weight.
    WeightedRandom,
    /// Use the first viable candidate in insertion order (fallback chain).
    FallbackChain,
}

// ── Model candidate ─────────────────────────────────────────────────────

/// A candidate model/backend for selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCandidate {
    /// Unique backend identifier.
    pub backend_name: String,
    /// Model identifier within the backend.
    pub model_id: String,
    /// Estimated latency in milliseconds, if known.
    pub estimated_latency_ms: Option<u64>,
    /// Estimated cost per 1,000 tokens, if known.
    pub estimated_cost_per_1k_tokens: Option<f64>,
    /// Fidelity score in `[0.0, 1.0]`, if known.
    pub fidelity_score: Option<f64>,
    /// Weight for weighted-random selection (higher = more likely).
    pub weight: f64,
}

// ── Model selector ──────────────────────────────────────────────────────

/// Selects model candidates according to a [`SelectionStrategy`].
///
/// For [`SelectionStrategy::RoundRobin`] an internal counter tracks the
/// current position, so successive calls to [`select`](Self::select)
/// cycle through the candidate list.
///
/// For [`SelectionStrategy::WeightedRandom`] selection is proportional to
/// candidate weights using system-time entropy.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelSelector {
    /// The strategy used to pick candidates.
    pub strategy: SelectionStrategy,
    /// Available candidates to choose from.
    pub candidates: Vec<ModelCandidate>,
    #[serde(skip)]
    counter: AtomicUsize,
}

impl Clone for ModelSelector {
    fn clone(&self) -> Self {
        Self {
            strategy: self.strategy,
            candidates: self.candidates.clone(),
            counter: AtomicUsize::new(self.counter.load(Ordering::Relaxed)),
        }
    }
}

impl ModelSelector {
    /// Creates a new selector with the given strategy and candidates.
    #[must_use]
    pub fn new(strategy: SelectionStrategy, candidates: Vec<ModelCandidate>) -> Self {
        Self {
            strategy,
            candidates,
            counter: AtomicUsize::new(0),
        }
    }

    /// Picks the best candidate according to the configured strategy.
    ///
    /// Returns `None` if there are no candidates.
    #[must_use]
    pub fn select(&self) -> Option<&ModelCandidate> {
        if self.candidates.is_empty() {
            return None;
        }
        match self.strategy {
            SelectionStrategy::LowestLatency => self.select_lowest_latency(),
            SelectionStrategy::LowestCost => self.select_lowest_cost(),
            SelectionStrategy::HighestFidelity => self.select_highest_fidelity(),
            SelectionStrategy::RoundRobin => self.select_round_robin(),
            SelectionStrategy::WeightedRandom => self.select_weighted_random(),
            SelectionStrategy::FallbackChain => self.candidates.first(),
        }
    }

    /// Returns up to `n` candidates ranked by the configured strategy.
    ///
    /// The returned list is ordered from most-preferred to least-preferred.
    #[must_use]
    pub fn select_n(&self, n: usize) -> Vec<&ModelCandidate> {
        if self.candidates.is_empty() || n == 0 {
            return Vec::new();
        }
        let mut indices: Vec<usize> = (0..self.candidates.len()).collect();
        match self.strategy {
            SelectionStrategy::LowestLatency => {
                indices
                    .sort_by_key(|&i| self.candidates[i].estimated_latency_ms.unwrap_or(u64::MAX));
            }
            SelectionStrategy::LowestCost => {
                indices.sort_by(|&a, &b| {
                    let ca = self.candidates[a]
                        .estimated_cost_per_1k_tokens
                        .unwrap_or(f64::MAX);
                    let cb = self.candidates[b]
                        .estimated_cost_per_1k_tokens
                        .unwrap_or(f64::MAX);
                    ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SelectionStrategy::HighestFidelity => {
                indices.sort_by(|&a, &b| {
                    let fa = self.candidates[a].fidelity_score.unwrap_or(0.0);
                    let fb = self.candidates[b].fidelity_score.unwrap_or(0.0);
                    fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SelectionStrategy::RoundRobin => {
                let start = self.counter.load(Ordering::Relaxed) % self.candidates.len();
                indices = (0..self.candidates.len())
                    .map(|i| (start + i) % self.candidates.len())
                    .collect();
            }
            SelectionStrategy::WeightedRandom => {
                indices.sort_by(|&a, &b| {
                    let wa = self.candidates[a].weight;
                    let wb = self.candidates[b].weight;
                    wb.partial_cmp(&wa).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SelectionStrategy::FallbackChain => {
                // Already in insertion order.
            }
        }
        indices
            .into_iter()
            .take(n)
            .map(|i| &self.candidates[i])
            .collect()
    }

    fn select_lowest_latency(&self) -> Option<&ModelCandidate> {
        self.candidates
            .iter()
            .min_by_key(|c| c.estimated_latency_ms.unwrap_or(u64::MAX))
    }

    fn select_lowest_cost(&self) -> Option<&ModelCandidate> {
        self.candidates.iter().min_by(|a, b| {
            let ca = a.estimated_cost_per_1k_tokens.unwrap_or(f64::MAX);
            let cb = b.estimated_cost_per_1k_tokens.unwrap_or(f64::MAX);
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    fn select_highest_fidelity(&self) -> Option<&ModelCandidate> {
        self.candidates.iter().max_by(|a, b| {
            let fa = a.fidelity_score.unwrap_or(0.0);
            let fb = b.fidelity_score.unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    fn select_round_robin(&self) -> Option<&ModelCandidate> {
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.candidates.len();
        Some(&self.candidates[idx])
    }

    fn select_weighted_random(&self) -> Option<&ModelCandidate> {
        let total: f64 = self.candidates.iter().map(|c| c.weight.max(0.0)).sum();
        if total <= 0.0 {
            return self.candidates.first();
        }

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let r = (f64::from(nanos) / f64::from(u32::MAX)) * total;

        let mut cumulative = 0.0;
        for candidate in &self.candidates {
            cumulative += candidate.weight.max(0.0);
            if r < cumulative {
                return Some(candidate);
            }
        }
        self.candidates.last()
    }
}
