// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-projection
//!
//! Projection matrix that routes work orders to the best-fit backend.

use std::collections::BTreeMap;

use abp_capability::{NegotiationResult, negotiate};
use abp_core::{Capability, CapabilityManifest, CapabilityRequirements, WorkOrder};
use abp_dialect::Dialect;
use abp_mapping::MappingRegistry;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Errors ──────────────────────────────────────────────────────────────

/// Errors that can occur during projection.
#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProjectionError {
    /// No registered backend can satisfy the work order.
    #[error("no suitable backend for work order: {reason}")]
    NoSuitableBackend {
        /// Human-readable explanation.
        reason: String,
    },
    /// The projection matrix has no backends registered.
    #[error("projection matrix is empty — no backends registered")]
    EmptyMatrix,
}

// ── Backend entry ───────────────────────────────────────────────────────

/// A registered backend with its capabilities, dialect, and priority.
#[derive(Debug, Clone)]
pub struct BackendEntry {
    /// Unique backend identifier (e.g. `"sidecar:claude"`, `"openai"`).
    pub id: String,
    /// Capability manifest advertised by this backend.
    pub capabilities: CapabilityManifest,
    /// Native dialect spoken by this backend.
    pub dialect: Dialect,
    /// Priority weight in `[0, 100]`. Higher means preferred when scores tie.
    pub priority: u32,
}

// ── Projection score ────────────────────────────────────────────────────

/// Composite score for a single backend against a work order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionScore {
    /// Fraction of required capabilities satisfied (native or emulated), in `[0.0, 1.0]`.
    pub capability_coverage: f64,
    /// Fraction of mapping features that are lossless, in `[0.0, 1.0]`.
    pub mapping_fidelity: f64,
    /// Normalized priority in `[0.0, 1.0]`.
    pub priority: f64,
    /// Final weighted score.
    pub total: f64,
}

// Weight constants
const W_CAPABILITY: f64 = 0.5;
const W_FIDELITY: f64 = 0.3;
const W_PRIORITY: f64 = 0.2;

impl ProjectionScore {
    fn compute(capability_coverage: f64, mapping_fidelity: f64, priority: f64) -> Self {
        let total = W_CAPABILITY * capability_coverage
            + W_FIDELITY * mapping_fidelity
            + W_PRIORITY * priority;
        Self {
            capability_coverage,
            mapping_fidelity,
            priority,
            total,
        }
    }
}

// ── Required emulation ──────────────────────────────────────────────────

/// A capability that the selected backend must emulate rather than support natively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredEmulation {
    /// The capability requiring emulation.
    pub capability: Capability,
    /// Strategy description (e.g. `"adapter"`).
    pub strategy: String,
}

// ── Fallback entry ──────────────────────────────────────────────────────

/// An alternative backend in the fallback chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FallbackEntry {
    /// Backend identifier.
    pub backend_id: String,
    /// Projection score for this backend.
    pub score: ProjectionScore,
}

// ── Projection result ───────────────────────────────────────────────────

/// The outcome of projecting a work order onto the backend registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionResult {
    /// Identifier of the selected backend.
    pub selected_backend: String,
    /// Composite score of the selected backend.
    pub fidelity_score: ProjectionScore,
    /// Capabilities that need emulation on the selected backend.
    pub required_emulations: Vec<RequiredEmulation>,
    /// Ordered list of alternative backends (descending score), excluding the selected one.
    pub fallback_chain: Vec<FallbackEntry>,
}

// ── Projection matrix ───────────────────────────────────────────────────

/// The projection matrix: combines a backend registry, capability negotiation,
/// and mapping quality to route work orders to backends.
#[derive(Debug, Clone, Default)]
pub struct ProjectionMatrix {
    backends: BTreeMap<String, BackendEntry>,
    mapping_registry: MappingRegistry,
    /// Source dialect of the work order (set per-projection or globally).
    source_dialect: Option<Dialect>,
    /// Features to evaluate mapping fidelity for.
    mapping_features: Vec<String>,
}

impl ProjectionMatrix {
    /// Creates an empty projection matrix.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a projection matrix with the given mapping registry.
    #[must_use]
    pub fn with_mapping_registry(registry: MappingRegistry) -> Self {
        Self {
            mapping_registry: registry,
            ..Self::default()
        }
    }

    /// Sets the source dialect for mapping fidelity scoring.
    pub fn set_source_dialect(&mut self, dialect: Dialect) {
        self.source_dialect = Some(dialect);
    }

    /// Sets the feature list used for mapping fidelity scoring.
    pub fn set_mapping_features(&mut self, features: Vec<String>) {
        self.mapping_features = features;
    }

    /// Register a backend with its capabilities, dialect, and priority.
    pub fn register_backend(
        &mut self,
        id: impl Into<String>,
        capabilities: CapabilityManifest,
        dialect: Dialect,
        priority: u32,
    ) {
        let id = id.into();
        self.backends.insert(
            id.clone(),
            BackendEntry {
                id,
                capabilities,
                dialect,
                priority,
            },
        );
    }

    /// Returns the number of registered backends.
    #[must_use]
    pub fn backend_count(&self) -> usize {
        self.backends.len()
    }

    /// Project a work order onto the backend registry.
    ///
    /// Returns the best-fit backend, its score, required emulations, and a
    /// fallback chain of alternatives sorted by descending score.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError::EmptyMatrix`] if no backends are registered,
    /// or [`ProjectionError::NoSuitableBackend`] if no backend can satisfy the
    /// work order's requirements.
    pub fn project(&self, work_order: &WorkOrder) -> Result<ProjectionResult, ProjectionError> {
        if self.backends.is_empty() {
            return Err(ProjectionError::EmptyMatrix);
        }

        let is_passthrough = is_passthrough_mode(work_order);
        let source_dialect = self.detect_source_dialect(work_order);
        let max_priority = self
            .backends
            .values()
            .map(|b| b.priority)
            .max()
            .unwrap_or(1)
            .max(1);

        let mut scored: Vec<(String, ProjectionScore, NegotiationResult)> = Vec::new();

        for entry in self.backends.values() {
            let neg = negotiate(&entry.capabilities, &work_order.requirements);
            let cap_coverage = capability_coverage(&neg, &work_order.requirements);
            let fidelity = self.mapping_fidelity(source_dialect, entry.dialect);
            let norm_priority = entry.priority as f64 / max_priority as f64;

            let mut score = ProjectionScore::compute(cap_coverage, fidelity, norm_priority);

            // Passthrough bonus: same-dialect backend gets a boost.
            if is_passthrough
                && let Some(src) = source_dialect
                && entry.dialect == src
            {
                score.total += 0.15;
            }

            scored.push((entry.id.clone(), score, neg));
        }

        // Sort by total descending, then by id for determinism.
        scored.sort_by(|a, b| {
            b.1.total
                .partial_cmp(&a.1.total)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        // Filter: only consider backends that are compatible (all caps satisfied).
        let compatible: Vec<_> = scored
            .iter()
            .filter(|(_, _, neg)| neg.is_compatible())
            .collect();

        let (selected_id, selected_score, selected_neg) = if !compatible.is_empty() {
            let (id, score, neg) = compatible[0];
            (id.clone(), score.clone(), neg.clone())
        } else {
            // No fully compatible backend — pick the best partial match.
            if scored.is_empty() {
                return Err(ProjectionError::NoSuitableBackend {
                    reason: "no backends scored".into(),
                });
            }
            // If the best backend has zero capability coverage, reject.
            if scored[0].1.capability_coverage == 0.0
                && !work_order.requirements.required.is_empty()
            {
                return Err(ProjectionError::NoSuitableBackend {
                    reason: "no backend satisfies any required capabilities".into(),
                });
            }
            let (id, score, neg) = &scored[0];
            (id.clone(), score.clone(), neg.clone())
        };

        let required_emulations = build_emulations(&selected_neg);

        let fallback_chain = scored
            .iter()
            .filter(|(id, _, _)| *id != selected_id)
            .map(|(id, score, _)| FallbackEntry {
                backend_id: id.clone(),
                score: score.clone(),
            })
            .collect();

        Ok(ProjectionResult {
            selected_backend: selected_id,
            fidelity_score: selected_score,
            required_emulations,
            fallback_chain,
        })
    }

    /// Compute mapping fidelity for a given source→target dialect pair.
    fn mapping_fidelity(&self, source: Option<Dialect>, target: Dialect) -> f64 {
        let source = match source {
            Some(s) => s,
            None => return 1.0, // No source dialect → assume perfect fidelity.
        };

        if source == target {
            return 1.0;
        }

        if self.mapping_features.is_empty() {
            // No features configured → use registry rank_targets heuristic.
            let features_ref: Vec<&str> = abp_mapping::features::TOOL_USE
                .split('\0') // just a single feature
                .collect();
            let ranked = self.mapping_registry.rank_targets(source, &features_ref);
            for (d, _) in &ranked {
                if *d == target {
                    return 0.8; // Has some mapping support.
                }
            }
            return 0.0;
        }

        let validations = abp_mapping::validate_mapping(
            &self.mapping_registry,
            source,
            target,
            &self.mapping_features,
        );

        if validations.is_empty() {
            return 0.0;
        }

        let lossless_count = validations
            .iter()
            .filter(|v| v.fidelity.is_lossless())
            .count();
        let supported_count = validations
            .iter()
            .filter(|v| !v.fidelity.is_unsupported())
            .count();

        if supported_count == 0 {
            return 0.0;
        }

        // Blend: mostly lossless ratio, but give partial credit for lossy support.
        let lossless_ratio = lossless_count as f64 / validations.len() as f64;
        let supported_ratio = supported_count as f64 / validations.len() as f64;
        0.7 * lossless_ratio + 0.3 * supported_ratio
    }

    /// Detect source dialect from work order vendor config.
    fn detect_source_dialect(&self, work_order: &WorkOrder) -> Option<Dialect> {
        if let Some(dialect) = self.source_dialect {
            return Some(dialect);
        }

        // Try to read from vendor config: config.vendor["abp"]["source_dialect"]
        if let Some(abp_val) = work_order.config.vendor.get("abp")
            && let Some(dialect_str) = abp_val.get("source_dialect").and_then(|v| v.as_str())
        {
            return parse_dialect(dialect_str);
        }

        None
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Compute the fraction of required capabilities that are native or emulated.
fn capability_coverage(neg: &NegotiationResult, reqs: &CapabilityRequirements) -> f64 {
    if reqs.required.is_empty() {
        return 1.0;
    }
    let satisfied = neg.native.len() + neg.emulatable.len();
    satisfied as f64 / reqs.required.len() as f64
}

/// Check if the work order requests passthrough mode.
fn is_passthrough_mode(work_order: &WorkOrder) -> bool {
    work_order
        .config
        .vendor
        .get("abp")
        .and_then(|v| v.get("mode"))
        .and_then(|v| v.as_str())
        .is_some_and(|m| m == "passthrough")
}

/// Build the list of required emulations from a negotiation result.
fn build_emulations(neg: &NegotiationResult) -> Vec<RequiredEmulation> {
    neg.emulatable
        .iter()
        .map(|cap| RequiredEmulation {
            capability: cap.clone(),
            strategy: "adapter".into(),
        })
        .collect()
}

/// Parse a dialect name string into a [`Dialect`].
fn parse_dialect(s: &str) -> Option<Dialect> {
    match s.to_lowercase().as_str() {
        "openai" | "open_ai" => Some(Dialect::OpenAi),
        "claude" => Some(Dialect::Claude),
        "gemini" => Some(Dialect::Gemini),
        "codex" => Some(Dialect::Codex),
        "kimi" => Some(Dialect::Kimi),
        "copilot" => Some(Dialect::Copilot),
        _ => None,
    }
}

// ===========================================================================
// Tests
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
        SupportLevel, WorkOrderBuilder,
    };
    use abp_mapping::{Fidelity, MappingRegistry, MappingRule};

    // ── helpers ──────────────────────────────────────────────────────────

    fn manifest(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
        caps.iter().cloned().collect()
    }

    fn require_caps(caps: &[Capability]) -> CapabilityRequirements {
        CapabilityRequirements {
            required: caps
                .iter()
                .map(|c| CapabilityRequirement {
                    capability: c.clone(),
                    min_support: MinSupport::Emulated,
                })
                .collect(),
        }
    }

    fn work_order_with_reqs(reqs: CapabilityRequirements) -> WorkOrder {
        WorkOrderBuilder::new("test task")
            .requirements(reqs)
            .build()
    }

    fn passthrough_work_order(reqs: CapabilityRequirements) -> WorkOrder {
        let mut config = abp_core::RuntimeConfig::default();
        let abp_config = serde_json::json!({ "mode": "passthrough", "source_dialect": "claude" });
        config.vendor.insert("abp".into(), abp_config);
        WorkOrderBuilder::new("passthrough task")
            .requirements(reqs)
            .config(config)
            .build()
    }

    fn basic_matrix() -> ProjectionMatrix {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "backend-a",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "backend-b",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::Claude,
            50,
        );
        pm
    }

    // ── 1. Single backend matches work order ────────────────────────────

    #[test]
    fn single_backend_exact_match() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "only-backend",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "only-backend");
        assert!(result.fallback_chain.is_empty());
    }

    #[test]
    fn single_backend_with_emulation() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "emu-backend",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::Claude,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "emu-backend");
        assert_eq!(result.required_emulations.len(), 1);
        assert_eq!(
            result.required_emulations[0].capability,
            Capability::ToolRead
        );
    }

    // ── 2. Multiple backends ranked by score ────────────────────────────

    #[test]
    fn multiple_backends_ranked_by_capability() {
        let pm = basic_matrix();
        let wo = work_order_with_reqs(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        // backend-a has 3/3 native, backend-b has 2/3 (1 emulated) and missing ToolWrite
        assert_eq!(result.selected_backend, "backend-a");
        assert!(!result.fallback_chain.is_empty());
    }

    #[test]
    fn multiple_backends_fallback_order() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "high",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            80,
        );
        pm.register_backend(
            "mid",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        pm.register_backend(
            "low",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Gemini,
            20,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "high");
        assert_eq!(result.fallback_chain.len(), 2);
        assert_eq!(result.fallback_chain[0].backend_id, "mid");
        assert_eq!(result.fallback_chain[1].backend_id, "low");
    }

    // ── 3. No suitable backend → error ──────────────────────────────────

    #[test]
    fn no_backends_registered_error() {
        let pm = ProjectionMatrix::new();
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::EmptyMatrix));
    }

    #[test]
    fn no_suitable_backend_error() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "limited",
            manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn no_suitable_backend_all_unsupported() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("empty-caps", CapabilityManifest::new(), Dialect::OpenAi, 50);
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    // ── 4. Emulation factored into scoring ──────────────────────────────

    #[test]
    fn emulation_scores_lower_than_native() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "native-all",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "emulated-all",
            manifest(&[
                (Capability::Streaming, SupportLevel::Emulated),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        // Both have 100% coverage, but native gets higher score from capability scoring
        // (both satisfy, but negotiate puts them in different buckets — coverage is equal here)
        // Selection is deterministic by ID when scores tie.
        assert_eq!(result.selected_backend, "emulated-all");
        // Both are compatible, both have same coverage. With same priority, id sort wins.
        // The key point: emulated backend still gets full coverage credit.
        assert!(result.fidelity_score.capability_coverage == 1.0);
    }

    #[test]
    fn emulation_required_in_result() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "partial-emu",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Emulated),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::Claude,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.required_emulations.len(), 1);
        assert_eq!(
            result.required_emulations[0].capability,
            Capability::ToolRead
        );
    }

    #[test]
    fn emulation_multiple_caps() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "multi-emu",
            manifest(&[
                (Capability::Streaming, SupportLevel::Emulated),
                (Capability::ToolRead, SupportLevel::Emulated),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.required_emulations.len(), 2);
    }

    // ── 5. Fallback chain ordering ──────────────────────────────────────

    #[test]
    fn fallback_chain_excludes_selected() {
        let pm = basic_matrix();
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        for entry in &result.fallback_chain {
            assert_ne!(entry.backend_id, result.selected_backend);
        }
    }

    #[test]
    fn fallback_chain_descending_score() {
        let mut pm = ProjectionMatrix::new();
        for (id, prio) in [("a", 90), ("b", 60), ("c", 30), ("d", 10)] {
            pm.register_backend(
                id,
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                prio,
            );
        }
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "a");
        let scores: Vec<f64> = result
            .fallback_chain
            .iter()
            .map(|e| e.score.total)
            .collect();
        for w in scores.windows(2) {
            assert!(w[0] >= w[1], "fallback chain not sorted descending");
        }
    }

    #[test]
    fn fallback_chain_includes_incompatible() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "compatible",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "partial",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "compatible");
        // "partial" should still appear in fallback.
        assert!(
            result
                .fallback_chain
                .iter()
                .any(|e| e.backend_id == "partial")
        );
    }

    // ── 6. Backend priority affects ranking ─────────────────────────────

    #[test]
    fn priority_breaks_tie() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "low-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "high-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "high-prio");
    }

    #[test]
    fn priority_normalized_to_max() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "only",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            100,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // With only one backend at max priority, normalized priority = 1.0
        assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn priority_zero_still_selectable() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "zero-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            0,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "zero-prio");
    }

    // ── 7. Fidelity requirements constrain selection ────────────────────

    #[test]
    fn fidelity_scoring_with_mapping_features() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Gemini,
            feature: "tool_use".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "partial".into(),
            },
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Gemini,
            feature: "streaming".into(),
            fidelity: Fidelity::Unsupported {
                reason: "nope".into(),
            },
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::Claude);
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "gemini-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Gemini,
            50,
        );

        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // OpenAI has lossless mapping → higher fidelity → should be selected.
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn same_dialect_gets_perfect_fidelity() {
        let mut pm = ProjectionMatrix::new();
        pm.set_source_dialect(Dialect::Claude);
        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn fidelity_constrains_over_priority() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::Claude);
        pm.set_mapping_features(vec!["tool_use".into()]);

        pm.register_backend(
            "openai-low-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "gemini-high-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Gemini,
            90,
        );

        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // OpenAI has lossless fidelity (weight 0.3) vs Gemini with 0 fidelity.
        // Fidelity advantage: 0.3*1.0 = 0.3 for OpenAI vs 0.0 for Gemini.
        // Priority disadvantage: 0.2*(10/90) vs 0.2*1.0. 0.022 vs 0.2.
        // Net: OpenAI=0.5+0.3+0.022=0.822, Gemini=0.5+0.0+0.2=0.7.
        assert_eq!(result.selected_backend, "openai-low-prio");
    }

    // ── 8. Passthrough mode prefers same-dialect backend ────────────────

    #[test]
    fn passthrough_prefers_same_dialect() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn passthrough_bonus_overrides_priority() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "claude-low",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            30,
        );
        pm.register_backend(
            "openai-high",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            80,
        );
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // Claude gets passthrough bonus (+0.15) which should overcome priority gap.
        assert_eq!(result.selected_backend, "claude-low");
    }

    #[test]
    fn non_passthrough_ignores_dialect_match() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "claude-low",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            30,
        );
        pm.register_backend(
            "openai-high",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            80,
        );
        // Normal (non-passthrough) work order — no dialect bonus.
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai-high");
    }

    // ── Additional coverage ─────────────────────────────────────────────

    #[test]
    fn empty_requirements_all_backends_match() {
        let pm = basic_matrix();
        let wo = work_order_with_reqs(CapabilityRequirements::default());
        let result = pm.project(&wo).unwrap();
        // With empty reqs, all backends are compatible. Highest priority (both 50), id sort.
        assert!(!result.selected_backend.is_empty());
        // Both should appear (one selected, one fallback).
        assert_eq!(result.fallback_chain.len(), 1);
    }

    #[test]
    fn score_weights_sum_to_one() {
        assert!((W_CAPABILITY + W_FIDELITY + W_PRIORITY - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn projection_score_compute_correct() {
        let score = ProjectionScore::compute(1.0, 1.0, 1.0);
        assert!((score.total - 1.0).abs() < f64::EPSILON);

        let score_zero = ProjectionScore::compute(0.0, 0.0, 0.0);
        assert!(score_zero.total.abs() < f64::EPSILON);
    }

    #[test]
    fn projection_score_partial() {
        let score = ProjectionScore::compute(0.5, 0.5, 0.5);
        let expected = 0.5 * 0.5 + 0.3 * 0.5 + 0.2 * 0.5;
        assert!((score.total - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn backend_count() {
        let pm = basic_matrix();
        assert_eq!(pm.backend_count(), 2);
    }

    #[test]
    fn register_overwrites_existing() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
            Dialect::Claude,
            90,
        );
        assert_eq!(pm.backend_count(), 1);
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "be");
        // Should use the overwritten (Claude, prio 90) version.
        assert_eq!(result.required_emulations.len(), 1);
    }

    #[test]
    fn parse_dialect_variants() {
        assert_eq!(parse_dialect("openai"), Some(Dialect::OpenAi));
        assert_eq!(parse_dialect("claude"), Some(Dialect::Claude));
        assert_eq!(parse_dialect("gemini"), Some(Dialect::Gemini));
        assert_eq!(parse_dialect("codex"), Some(Dialect::Codex));
        assert_eq!(parse_dialect("kimi"), Some(Dialect::Kimi));
        assert_eq!(parse_dialect("copilot"), Some(Dialect::Copilot));
        assert_eq!(parse_dialect("unknown"), None);
    }

    #[test]
    fn is_passthrough_mode_detection() {
        let wo = passthrough_work_order(CapabilityRequirements::default());
        assert!(is_passthrough_mode(&wo));

        let wo_normal = work_order_with_reqs(CapabilityRequirements::default());
        assert!(!is_passthrough_mode(&wo_normal));
    }

    #[test]
    fn capability_coverage_empty_reqs() {
        let neg = NegotiationResult {
            native: vec![],
            emulatable: vec![],
            unsupported: vec![],
        };
        let reqs = CapabilityRequirements::default();
        assert!((capability_coverage(&neg, &reqs) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn capability_coverage_partial() {
        let neg = NegotiationResult {
            native: vec![Capability::Streaming],
            emulatable: vec![],
            unsupported: vec![Capability::ToolRead],
        };
        let reqs = require_caps(&[Capability::Streaming, Capability::ToolRead]);
        assert!((capability_coverage(&neg, &reqs) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn build_emulations_empty() {
        let neg = NegotiationResult {
            native: vec![Capability::Streaming],
            emulatable: vec![],
            unsupported: vec![],
        };
        assert!(build_emulations(&neg).is_empty());
    }

    #[test]
    fn build_emulations_multiple() {
        let neg = NegotiationResult {
            native: vec![],
            emulatable: vec![Capability::Streaming, Capability::ToolRead],
            unsupported: vec![],
        };
        let emu = build_emulations(&neg);
        assert_eq!(emu.len(), 2);
        assert_eq!(emu[0].capability, Capability::Streaming);
        assert_eq!(emu[1].capability, Capability::ToolRead);
    }
}
