// SPDX-License-Identifier: MIT OR Apache-2.0
//! Intelligent backend selection based on capabilities and scheduling strategies.
//!
//! [`BackendSelector`] maintains a pool of [`BackendCandidate`]s and picks the
//! best one for a given set of required [`Capability`]s according to the
//! configured [`SelectionStrategy`].

use abp_core::{Capability, WorkOrder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use crate::projection::Dialect;

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

/// Health status of a registered backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BackendHealth {
    /// Backend is fully operational.
    Up,
    /// Backend is operational but experiencing issues.
    Degraded {
        /// Human-readable explanation.
        reason: String,
    },
    /// Backend is not operational.
    Down {
        /// Human-readable explanation.
        reason: String,
    },
}

/// Timestamp-annotated health record for a backend.
#[derive(Debug, Clone)]
pub struct HealthRecord {
    /// Current health status.
    pub health: BackendHealth,
    /// When the health was last checked.
    pub last_check: DateTime<Utc>,
}

/// How well a backend matches a target dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialectMatch {
    /// Capability/dialect not supported.
    Unsupported = 0,
    /// Supported via translation/emulation.
    Emulated = 1,
    /// Natively supported.
    Native = 2,
}

/// Error during backend selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    /// No backend satisfies the requirements.
    NoMatchingBackend {
        /// Explanation of why no match was found.
        reason: String,
    },
    /// All registered backends are unhealthy (Down).
    AllBackendsUnhealthy {
        /// Names of the unhealthy backends.
        backends: Vec<String>,
    },
    /// A requested backend lacks required capabilities.
    CapabilityMismatch {
        /// The backend that was checked.
        backend: String,
        /// Capabilities the backend is missing.
        missing: Vec<Capability>,
    },
    /// No backends are registered in the selector.
    EmptyRegistry,
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMatchingBackend { reason } => write!(f, "no matching backend: {reason}"),
            Self::AllBackendsUnhealthy { backends } => {
                write!(f, "all backends unhealthy: {}", backends.join(", "))
            }
            Self::CapabilityMismatch { backend, missing } => {
                let caps: Vec<String> = missing.iter().map(|c| format!("{c:?}")).collect();
                write!(
                    f,
                    "capability mismatch for {backend}: missing {}",
                    caps.join(", ")
                )
            }
            Self::EmptyRegistry => write!(f, "no backends registered"),
        }
    }
}

impl std::error::Error for SelectionError {}

/// Fallback behavior when the primary selection yields no result.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackStrategy {
    /// Fail immediately if no perfect match.
    #[default]
    None,
    /// Accept partial capability matches, ranked by coverage.
    BestEffort,
    /// Accept any healthy backend regardless of capabilities.
    AnyHealthy,
}

/// Configurable parameters for backend selection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelectionCriteria {
    /// If set, prefer this backend when it qualifies.
    pub preferred_backend: Option<String>,
    /// Capabilities the backend must support.
    pub required_capabilities: Vec<Capability>,
    /// What to do when no perfect match is found.
    pub fallback_strategy: FallbackStrategy,
    /// Target dialect for compatibility scoring.
    pub target_dialect: Option<Dialect>,
}

impl SelectionCriteria {
    /// Build criteria from a [`WorkOrder`].
    #[must_use]
    pub fn from_work_order(wo: &WorkOrder) -> Self {
        Self {
            preferred_backend: wo.config.model.clone(),
            required_capabilities: wo
                .requirements
                .required
                .iter()
                .map(|r| r.capability.clone())
                .collect(),
            fallback_strategy: FallbackStrategy::None,
            target_dialect: None,
        }
    }
}

/// Per-candidate evaluation details included in a [`SelectionReport`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEvaluation {
    /// Backend name.
    pub backend: String,
    /// Health status label.
    pub health_status: String,
    /// Fraction of required capabilities satisfied (0.0–1.0).
    pub capability_score: f64,
    /// Dialect compatibility.
    pub dialect_match: DialectMatch,
    /// Candidate priority value.
    pub priority: u32,
    /// Combined ranking score.
    pub composite_score: f64,
    /// `Some(reason)` if this candidate was rejected.
    pub rejected_reason: Option<String>,
}

/// Report explaining why a backend was selected and what alternatives exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionReport {
    /// Name of the selected backend.
    pub selected_backend: String,
    /// Human-readable explanation of the selection decision.
    pub reason: String,
    /// Other viable backends that were not selected.
    pub alternatives: Vec<String>,
    /// Evaluation details for every candidate considered.
    pub considered: Vec<CandidateEvaluation>,
}

/// Picks a backend from a pool of candidates according to a [`SelectionStrategy`].
#[derive(Debug)]
pub struct BackendSelector {
    strategy: SelectionStrategy,
    candidates: Vec<BackendCandidate>,
    /// Monotonically-increasing counter for round-robin scheduling.
    rr_counter: AtomicUsize,
    /// Health status per backend name.
    health_map: BTreeMap<String, HealthRecord>,
    /// Native dialect per backend name.
    dialect_map: BTreeMap<String, Dialect>,
}

impl BackendSelector {
    /// Create a new selector with the given strategy and no candidates.
    #[must_use]
    pub fn new(strategy: SelectionStrategy) -> Self {
        Self {
            strategy,
            candidates: Vec::new(),
            rr_counter: AtomicUsize::new(0),
            health_map: BTreeMap::new(),
            dialect_map: BTreeMap::new(),
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
    // Health & dialect management
    // ------------------------------------------------------------------

    /// Set the health status for a backend.
    pub fn set_health(&mut self, backend: &str, health: BackendHealth) {
        self.health_map.insert(
            backend.to_string(),
            HealthRecord {
                health,
                last_check: Utc::now(),
            },
        );
    }

    /// Get the health record for a backend.
    #[must_use]
    pub fn get_health(&self, backend: &str) -> Option<&HealthRecord> {
        self.health_map.get(backend)
    }

    /// Set the native dialect for a backend.
    pub fn set_dialect(&mut self, backend: &str, dialect: Dialect) {
        self.dialect_map.insert(backend.to_string(), dialect);
    }

    // ------------------------------------------------------------------
    // Work-order and criteria-based selection
    // ------------------------------------------------------------------

    /// Select the best backend for a [`WorkOrder`].
    ///
    /// Extracts required capabilities from the work order and delegates to
    /// [`select_with_criteria`](Self::select_with_criteria).
    pub fn select_backend(&self, work_order: &WorkOrder) -> Result<&str, SelectionError> {
        let criteria = SelectionCriteria::from_work_order(work_order);
        let report = self.select_with_criteria(&criteria)?;
        self.candidates
            .iter()
            .find(|c| c.name == report.selected_backend)
            .map(|c| c.name.as_str())
            .ok_or_else(|| SelectionError::NoMatchingBackend {
                reason: "internal: selected backend not in candidates".into(),
            })
    }

    /// Select a backend using explicit [`SelectionCriteria`], returning a
    /// detailed [`SelectionReport`].
    pub fn select_with_criteria(
        &self,
        criteria: &SelectionCriteria,
    ) -> Result<SelectionReport, SelectionError> {
        let enabled: Vec<&BackendCandidate> =
            self.candidates.iter().filter(|c| c.enabled).collect();

        if enabled.is_empty() {
            return Err(SelectionError::EmptyRegistry);
        }

        // Check if every enabled backend is down.
        let all_down = enabled
            .iter()
            .all(|c| matches!(self.effective_health(&c.name), BackendHealth::Down { .. }));

        if all_down {
            return Err(SelectionError::AllBackendsUnhealthy {
                backends: enabled.iter().map(|c| c.name.clone()).collect(),
            });
        }

        // Evaluate every enabled candidate.
        let evaluations: Vec<CandidateEvaluation> = enabled
            .iter()
            .map(|c| self.evaluate_candidate(c, criteria))
            .collect();

        // If a preferred backend is specified and viable, use it.
        if let Some(ref preferred) = criteria.preferred_backend {
            if let Some(eval) = evaluations
                .iter()
                .find(|e| e.backend == *preferred && e.rejected_reason.is_none())
            {
                let alternatives = evaluations
                    .iter()
                    .filter(|e| e.backend != *preferred && e.rejected_reason.is_none())
                    .map(|e| e.backend.clone())
                    .collect();
                return Ok(SelectionReport {
                    selected_backend: eval.backend.clone(),
                    reason: "preferred backend matched".into(),
                    alternatives,
                    considered: evaluations,
                });
            }
            // Preferred not viable — return CapabilityMismatch if strict.
            if criteria.fallback_strategy == FallbackStrategy::None {
                if let Some(candidate) = self
                    .candidates
                    .iter()
                    .find(|c| c.name == *preferred && c.enabled)
                {
                    let missing: Vec<Capability> = criteria
                        .required_capabilities
                        .iter()
                        .filter(|cap| !candidate.capabilities.contains(cap))
                        .cloned()
                        .collect();
                    if !missing.is_empty() {
                        return Err(SelectionError::CapabilityMismatch {
                            backend: preferred.clone(),
                            missing,
                        });
                    }
                }
            }
        }

        // Sort viable candidates by composite score (descending).
        let mut viable: Vec<&CandidateEvaluation> = evaluations
            .iter()
            .filter(|e| e.rejected_reason.is_none())
            .collect();
        viable.sort_by(|a, b| {
            b.composite_score
                .partial_cmp(&a.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        match viable.first() {
            Some(best) => {
                let alternatives = viable[1..].iter().map(|e| e.backend.clone()).collect();
                Ok(SelectionReport {
                    selected_backend: best.backend.clone(),
                    reason: format!(
                        "highest composite score ({:.2}) via {:?}",
                        best.composite_score, self.strategy
                    ),
                    alternatives,
                    considered: evaluations,
                })
            }
            None => Err(SelectionError::NoMatchingBackend {
                reason: "no viable backend after evaluation".into(),
            }),
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

    fn evaluate_candidate(
        &self,
        candidate: &BackendCandidate,
        criteria: &SelectionCriteria,
    ) -> CandidateEvaluation {
        let health = self.effective_health(&candidate.name);
        let is_down = matches!(health, BackendHealth::Down { .. });
        let cap_score = Self::capability_score_for(candidate, &criteria.required_capabilities);
        let dialect = self.dialect_match_for(&candidate.name, criteria.target_dialect.as_ref());
        let has_all_caps = cap_score >= 1.0 || criteria.required_capabilities.is_empty();

        let rejected_reason = if is_down {
            Some("backend is down".into())
        } else if !has_all_caps && criteria.fallback_strategy == FallbackStrategy::None {
            let missing: Vec<String> = criteria
                .required_capabilities
                .iter()
                .filter(|c| !candidate.capabilities.contains(c))
                .map(|c| format!("{c:?}"))
                .collect();
            Some(format!("missing capabilities: {}", missing.join(", ")))
        } else {
            None
        };

        let composite = if rejected_reason.is_some() {
            0.0
        } else {
            Self::compute_composite(&health, cap_score, dialect, candidate.priority)
        };

        CandidateEvaluation {
            backend: candidate.name.clone(),
            health_status: health_label(&health),
            capability_score: cap_score,
            dialect_match: dialect,
            priority: candidate.priority,
            composite_score: composite,
            rejected_reason,
        }
    }

    fn effective_health(&self, backend: &str) -> BackendHealth {
        self.health_map
            .get(backend)
            .map(|r| r.health.clone())
            .unwrap_or(BackendHealth::Up)
    }

    fn capability_score_for(candidate: &BackendCandidate, required: &[Capability]) -> f64 {
        if required.is_empty() {
            return 1.0;
        }
        let matched = required
            .iter()
            .filter(|c| candidate.capabilities.contains(c))
            .count();
        matched as f64 / required.len() as f64
    }

    fn dialect_match_for(&self, backend: &str, target: Option<&Dialect>) -> DialectMatch {
        let Some(target) = target else {
            return DialectMatch::Native;
        };
        match self.dialect_map.get(backend) {
            Some(d) if d == target => DialectMatch::Native,
            Some(_) => DialectMatch::Emulated,
            None => DialectMatch::Unsupported,
        }
    }

    fn compute_composite(
        health: &BackendHealth,
        cap_score: f64,
        dialect: DialectMatch,
        priority: u32,
    ) -> f64 {
        let h = match health {
            BackendHealth::Up => 3.0,
            BackendHealth::Degraded { .. } => 1.0,
            BackendHealth::Down { .. } => 0.0,
        };
        let d = match dialect {
            DialectMatch::Native => 3.0,
            DialectMatch::Emulated => 2.0,
            DialectMatch::Unsupported => 1.0,
        };
        let p = 1.0 / (1.0 + priority as f64);
        cap_score * 100.0 + h * 10.0 + d * 5.0 + p
    }
}

fn health_label(health: &BackendHealth) -> String {
    match health {
        BackendHealth::Up => "up".into(),
        BackendHealth::Degraded { reason } => format!("degraded: {reason}"),
        BackendHealth::Down { reason } => format!("down: {reason}"),
    }
}
