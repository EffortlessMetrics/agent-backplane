// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Detailed emulation planning for cross-dialect capability mapping.
//!
//! An [`EmulationPlan`] describes *how* to emulate each non-native capability
//! when translating between SDK dialects, including strategy, cost estimates,
//! confidence levels, and expected failure modes.

use crate::EmulationStrategy;
use abp_core::Capability;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Cost & confidence
// ---------------------------------------------------------------------------

/// Estimated cost of emulating a capability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmulationCost {
    /// Expected additional latency in milliseconds (0 = negligible).
    pub latency_ms: u32,
    /// Quality impact from 0.0 (no impact) to 1.0 (total quality loss).
    pub quality_impact: f64,
    /// Fidelity from 0.0 (no fidelity) to 1.0 (perfect fidelity).
    pub fidelity: f64,
}

impl EmulationCost {
    /// Create a zero-cost entry (negligible overhead, perfect fidelity).
    #[must_use]
    pub fn zero() -> Self {
        Self {
            latency_ms: 0,
            quality_impact: 0.0,
            fidelity: 1.0,
        }
    }

    /// Create a low-cost entry.
    #[must_use]
    pub fn low() -> Self {
        Self {
            latency_ms: 50,
            quality_impact: 0.1,
            fidelity: 0.9,
        }
    }

    /// Create a medium-cost entry.
    #[must_use]
    pub fn medium() -> Self {
        Self {
            latency_ms: 200,
            quality_impact: 0.3,
            fidelity: 0.7,
        }
    }

    /// Create a high-cost entry.
    #[must_use]
    pub fn high() -> Self {
        Self {
            latency_ms: 500,
            quality_impact: 0.6,
            fidelity: 0.4,
        }
    }
}

impl Default for EmulationCost {
    fn default() -> Self {
        Self::medium()
    }
}

impl fmt::Display for EmulationCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "+{}ms, quality={:.0}%, fidelity={:.0}%",
            self.latency_ms,
            (1.0 - self.quality_impact) * 100.0,
            self.fidelity * 100.0,
        )
    }
}

/// Confidence that the emulation will work correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    /// Emulation is well-tested and reliable.
    High,
    /// Emulation works for most cases but may have edge-case issues.
    Medium,
    /// Emulation is best-effort; significant limitations expected.
    Low,
}

impl fmt::Display for ConfidenceLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

/// What happens when an emulated capability fails at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    /// Failure is silently absorbed; output may be degraded.
    Silent,
    /// Returns a structured error the caller can handle gracefully.
    Graceful,
    /// Hard failure — the work order cannot continue.
    Hard,
}

impl fmt::Display for FailureMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Silent => write!(f, "silent"),
            Self::Graceful => write!(f, "graceful"),
            Self::Hard => write!(f, "hard"),
        }
    }
}

// ---------------------------------------------------------------------------
// EmulationPlanEntry
// ---------------------------------------------------------------------------

/// Full description of how a single capability will be emulated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmulationPlanEntry {
    /// The capability being emulated.
    pub capability: Capability,
    /// Which emulation strategy to use.
    pub strategy: EmulationStrategy,
    /// Estimated cost of emulation.
    pub cost: EmulationCost,
    /// How confident we are that emulation will succeed.
    pub confidence: ConfidenceLevel,
    /// What happens if emulation fails.
    pub failure_mode: FailureMode,
    /// Human-readable description of the emulation approach.
    pub description: String,
}

impl fmt::Display for EmulationPlanEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {} [confidence={}, failure={}, {}]",
            self.capability, self.strategy, self.confidence, self.failure_mode, self.cost,
        )
    }
}

// ---------------------------------------------------------------------------
// EmulationPlan
// ---------------------------------------------------------------------------

/// A complete plan for emulating multiple capabilities during dialect mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmulationPlan {
    /// Individual emulation entries, one per capability.
    pub entries: Vec<EmulationPlanEntry>,
}

impl EmulationPlan {
    /// Create an empty emulation plan.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Number of capabilities in the plan.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no emulation is needed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns entries with low confidence.
    #[must_use]
    pub fn low_confidence(&self) -> Vec<&EmulationPlanEntry> {
        self.entries
            .iter()
            .filter(|e| e.confidence == ConfidenceLevel::Low)
            .collect()
    }

    /// Returns entries with hard failure mode.
    #[must_use]
    pub fn hard_failures(&self) -> Vec<&EmulationPlanEntry> {
        self.entries
            .iter()
            .filter(|e| e.failure_mode == FailureMode::Hard)
            .collect()
    }

    /// Total estimated additional latency across all emulations.
    #[must_use]
    pub fn total_latency_ms(&self) -> u32 {
        self.entries.iter().map(|e| e.cost.latency_ms).sum()
    }

    /// Average fidelity across all emulated capabilities (0.0–1.0).
    ///
    /// Returns 1.0 if the plan is empty.
    #[must_use]
    pub fn average_fidelity(&self) -> f64 {
        if self.entries.is_empty() {
            return 1.0;
        }
        let sum: f64 = self.entries.iter().map(|e| e.cost.fidelity).sum();
        sum / self.entries.len() as f64
    }

    /// Minimum confidence level across all entries.
    ///
    /// Returns `High` if the plan is empty.
    #[must_use]
    pub fn min_confidence(&self) -> ConfidenceLevel {
        self.entries
            .iter()
            .map(|e| e.confidence)
            .min_by_key(|c| match c {
                ConfidenceLevel::High => 2,
                ConfidenceLevel::Medium => 1,
                ConfidenceLevel::Low => 0,
            })
            .unwrap_or(ConfidenceLevel::High)
    }

    /// Look up the emulation entry for a specific capability.
    #[must_use]
    pub fn get(&self, cap: &Capability) -> Option<&EmulationPlanEntry> {
        self.entries.iter().find(|e| &e.capability == cap)
    }
}

impl fmt::Display for EmulationPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.entries.is_empty() {
            return write!(f, "no emulation needed");
        }
        write!(
            f,
            "{} emulated capabilities (avg fidelity={:.0}%, +{}ms)",
            self.entries.len(),
            self.average_fidelity() * 100.0,
            self.total_latency_ms(),
        )
    }
}

// ---------------------------------------------------------------------------
// Builder: derive EmulationPlan from strategy + capability
// ---------------------------------------------------------------------------

/// Derive default cost, confidence, and failure mode for a given strategy.
#[must_use]
pub fn default_emulation_plan_entry(
    cap: &Capability,
    strategy: &EmulationStrategy,
) -> EmulationPlanEntry {
    let (cost, confidence, failure_mode, description) = match strategy {
        EmulationStrategy::ClientSide => (
            EmulationCost::low(),
            ConfidenceLevel::High,
            FailureMode::Graceful,
            format!("{cap:?}: client-side polyfill in ABP translation layer"),
        ),
        EmulationStrategy::ServerFallback => (
            EmulationCost::medium(),
            ConfidenceLevel::Medium,
            FailureMode::Graceful,
            format!("{cap:?}: degraded server-side implementation"),
        ),
        EmulationStrategy::Approximate => (
            EmulationCost::high(),
            ConfidenceLevel::Low,
            FailureMode::Silent,
            format!("{cap:?}: best-effort approximation with possible fidelity loss"),
        ),
    };

    EmulationPlanEntry {
        capability: cap.clone(),
        strategy: strategy.clone(),
        cost,
        confidence,
        failure_mode,
        description,
    }
}

/// Build an [`EmulationPlan`] from a list of `(Capability, EmulationStrategy)` pairs.
#[must_use]
pub fn build_emulation_plan(items: &[(Capability, EmulationStrategy)]) -> EmulationPlan {
    let entries = items
        .iter()
        .map(|(cap, strat)| default_emulation_plan_entry(cap, strat))
        .collect();
    EmulationPlan { entries }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_zero() {
        let c = EmulationCost::zero();
        assert_eq!(c.latency_ms, 0);
        assert!((c.fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_display() {
        let c = EmulationCost::low();
        let s = format!("{c}");
        assert!(s.contains("50ms"));
        assert!(s.contains("fidelity=90%"));
    }

    #[test]
    fn confidence_display() {
        assert_eq!(ConfidenceLevel::High.to_string(), "high");
        assert_eq!(ConfidenceLevel::Medium.to_string(), "medium");
        assert_eq!(ConfidenceLevel::Low.to_string(), "low");
    }

    #[test]
    fn failure_mode_display() {
        assert_eq!(FailureMode::Silent.to_string(), "silent");
        assert_eq!(FailureMode::Graceful.to_string(), "graceful");
        assert_eq!(FailureMode::Hard.to_string(), "hard");
    }

    #[test]
    fn plan_empty() {
        let plan = EmulationPlan::empty();
        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
        assert_eq!(plan.total_latency_ms(), 0);
        assert!((plan.average_fidelity() - 1.0).abs() < f64::EPSILON);
        assert_eq!(plan.min_confidence(), ConfidenceLevel::High);
    }

    #[test]
    fn plan_from_entries() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ]);
        assert_eq!(plan.len(), 2);
        assert!(!plan.is_empty());
    }

    #[test]
    fn plan_total_latency() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::ToolUse, EmulationStrategy::ServerFallback),
        ]);
        assert_eq!(plan.total_latency_ms(), 50 + 200);
    }

    #[test]
    fn plan_average_fidelity() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),   // 0.9
            (Capability::Vision, EmulationStrategy::Approximate),     // 0.4
        ]);
        let avg = plan.average_fidelity();
        assert!((avg - 0.65).abs() < 0.01);
    }

    #[test]
    fn plan_min_confidence() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),   // High
            (Capability::Vision, EmulationStrategy::Approximate),     // Low
        ]);
        assert_eq!(plan.min_confidence(), ConfidenceLevel::Low);
    }

    #[test]
    fn plan_low_confidence_entries() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
            (Capability::Audio, EmulationStrategy::Approximate),
        ]);
        let lc = plan.low_confidence();
        assert_eq!(lc.len(), 2);
    }

    #[test]
    fn plan_hard_failures() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ]);
        // Default approximate failures are Silent, not Hard
        assert!(plan.hard_failures().is_empty());
    }

    #[test]
    fn plan_get_by_capability() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ]);
        assert!(plan.get(&Capability::ToolRead).is_some());
        assert!(plan.get(&Capability::Vision).is_some());
        assert!(plan.get(&Capability::Audio).is_none());
    }

    #[test]
    fn plan_display_empty() {
        let plan = EmulationPlan::empty();
        assert_eq!(format!("{plan}"), "no emulation needed");
    }

    #[test]
    fn plan_display_non_empty() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),
        ]);
        let s = format!("{plan}");
        assert!(s.contains("1 emulated"));
    }

    #[test]
    fn entry_display() {
        let entry = default_emulation_plan_entry(
            &Capability::Vision,
            &EmulationStrategy::Approximate,
        );
        let s = format!("{entry}");
        assert!(s.contains("Vision"));
        assert!(s.contains("approximate"));
        assert!(s.contains("confidence=low"));
    }

    #[test]
    fn cost_serde_roundtrip() {
        let cost = EmulationCost::medium();
        let json = serde_json::to_string(&cost).unwrap();
        let back: EmulationCost = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cost);
    }

    #[test]
    fn confidence_serde_roundtrip() {
        for c in [ConfidenceLevel::High, ConfidenceLevel::Medium, ConfidenceLevel::Low] {
            let json = serde_json::to_string(&c).unwrap();
            let back: ConfidenceLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(back, c);
        }
    }

    #[test]
    fn failure_mode_serde_roundtrip() {
        for fm in [FailureMode::Silent, FailureMode::Graceful, FailureMode::Hard] {
            let json = serde_json::to_string(&fm).unwrap();
            let back: FailureMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, fm);
        }
    }

    #[test]
    fn plan_serde_roundtrip() {
        let plan = build_emulation_plan(&[
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ]);
        let json = serde_json::to_string(&plan).unwrap();
        let back: EmulationPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back, plan);
    }

    #[test]
    fn entry_serde_roundtrip() {
        let entry = default_emulation_plan_entry(
            &Capability::ToolUse,
            &EmulationStrategy::ServerFallback,
        );
        let json = serde_json::to_string(&entry).unwrap();
        let back: EmulationPlanEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
    }
}
