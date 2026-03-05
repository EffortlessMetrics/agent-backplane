// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Conversion traits and fidelity reporting for IR translation.
//!
//! `IntoIr` and `FromIr` provide the bidirectional contract that every
//! dialect adapter implements.  `FidelityReport` captures what was lost
//! (or approximated) during translation so callers can make informed
//! decisions about whether the mapping is acceptable.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::Dialect;

// ── Conversion traits ───────────────────────────────────────────────────

/// Convert a dialect-specific type **into** the normalized IR.
///
/// Implementors produce the IR value and a [`FidelityReport`] that
/// describes any information loss.
pub trait IntoIr<T> {
    /// Convert `self` into the IR type `T`.
    ///
    /// # Errors
    ///
    /// Returns a human-readable error message if the conversion fails
    /// (e.g. missing required fields).
    fn into_ir(self) -> Result<(T, FidelityReport), String>;
}

/// Convert the normalized IR **back** into a dialect-specific type.
///
/// Implementors reconstruct a vendor-specific value from the IR,
/// recording any fidelity losses in the report.
pub trait FromIr<T> {
    /// Convert the IR type `T` into `self`'s dialect type.
    ///
    /// # Errors
    ///
    /// Returns a human-readable error message if the conversion fails.
    fn from_ir(ir: T) -> Result<(Self, FidelityReport), String>
    where
        Self: Sized;
}

// ── Fidelity level ──────────────────────────────────────────────────────

/// How faithfully a single field or feature was preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FidelityLevel {
    /// Perfectly preserved — no information lost.
    Lossless,
    /// Approximated — semantics are close but not identical.
    Approximated,
    /// Dropped — the field or feature was entirely lost.
    Dropped,
}

impl std::fmt::Display for FidelityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lossless => f.write_str("lossless"),
            Self::Approximated => f.write_str("approximated"),
            Self::Dropped => f.write_str("dropped"),
        }
    }
}

// ── Fidelity item ───────────────────────────────────────────────────────

/// A single field or feature whose fidelity was affected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FidelityItem {
    /// Name of the affected field or feature.
    pub field: String,
    /// How it was affected.
    pub level: FidelityLevel,
    /// Human-readable explanation.
    pub reason: String,
}

// ── Fidelity report ─────────────────────────────────────────────────────

/// Tracks what was lost (or approximated) during a dialect → IR → dialect
/// conversion so callers can audit translation quality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FidelityReport {
    /// Source dialect (if known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Dialect>,
    /// Target dialect (if known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Dialect>,
    /// Individual field/feature reports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<FidelityItem>,
    /// Whether the overall conversion was completely lossless.
    pub is_lossless: bool,
}

impl FidelityReport {
    /// Create an empty lossless report.
    #[must_use]
    pub fn lossless() -> Self {
        Self {
            source: None,
            target: None,
            items: Vec::new(),
            is_lossless: true,
        }
    }

    /// Create an empty report for a known dialect pair.
    #[must_use]
    pub fn for_pair(source: Dialect, target: Dialect) -> Self {
        Self {
            source: Some(source),
            target: Some(target),
            items: Vec::new(),
            is_lossless: true,
        }
    }

    /// Record that a field was dropped entirely.
    pub fn drop_field(&mut self, field: impl Into<String>, reason: impl Into<String>) {
        self.is_lossless = false;
        self.items.push(FidelityItem {
            field: field.into(),
            level: FidelityLevel::Dropped,
            reason: reason.into(),
        });
    }

    /// Record that a field was approximated.
    pub fn approximate_field(&mut self, field: impl Into<String>, reason: impl Into<String>) {
        self.is_lossless = false;
        self.items.push(FidelityItem {
            field: field.into(),
            level: FidelityLevel::Approximated,
            reason: reason.into(),
        });
    }

    /// Record that a field was preserved losslessly.
    pub fn lossless_field(&mut self, field: impl Into<String>) {
        self.items.push(FidelityItem {
            field: field.into(),
            level: FidelityLevel::Lossless,
            reason: String::new(),
        });
    }

    /// Merge another report into this one.
    pub fn merge(&mut self, other: FidelityReport) {
        if !other.is_lossless {
            self.is_lossless = false;
        }
        self.items.extend(other.items);
    }

    /// Returns the number of fields that were dropped.
    #[must_use]
    pub fn dropped_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.level == FidelityLevel::Dropped)
            .count()
    }

    /// Returns the number of fields that were approximated.
    #[must_use]
    pub fn approximated_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.level == FidelityLevel::Approximated)
            .count()
    }

    /// Returns the names of all dropped fields.
    #[must_use]
    pub fn dropped_fields(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter(|i| i.level == FidelityLevel::Dropped)
            .map(|i| i.field.as_str())
            .collect()
    }
}

impl Default for FidelityReport {
    fn default() -> Self {
        Self::lossless()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fidelity_level_serde_roundtrip() {
        for level in [
            FidelityLevel::Lossless,
            FidelityLevel::Approximated,
            FidelityLevel::Dropped,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: FidelityLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, back);
        }
    }

    #[test]
    fn fidelity_level_display() {
        assert_eq!(FidelityLevel::Lossless.to_string(), "lossless");
        assert_eq!(FidelityLevel::Approximated.to_string(), "approximated");
        assert_eq!(FidelityLevel::Dropped.to_string(), "dropped");
    }

    #[test]
    fn fidelity_item_serde_roundtrip() {
        let item = FidelityItem {
            field: "thinking".into(),
            level: FidelityLevel::Dropped,
            reason: "target dialect does not support thinking".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        let back: FidelityItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item, back);
    }

    #[test]
    fn report_lossless_defaults() {
        let r = FidelityReport::lossless();
        assert!(r.is_lossless);
        assert!(r.items.is_empty());
        assert_eq!(r.source, None);
        assert_eq!(r.target, None);
        assert_eq!(r.dropped_count(), 0);
        assert_eq!(r.approximated_count(), 0);
        assert!(r.dropped_fields().is_empty());
    }

    #[test]
    fn report_default_is_lossless() {
        let r = FidelityReport::default();
        assert!(r.is_lossless);
    }

    #[test]
    fn report_for_pair() {
        let r = FidelityReport::for_pair(Dialect::OpenAi, Dialect::Claude);
        assert!(r.is_lossless);
        assert_eq!(r.source, Some(Dialect::OpenAi));
        assert_eq!(r.target, Some(Dialect::Claude));
    }

    #[test]
    fn report_drop_field() {
        let mut r = FidelityReport::lossless();
        r.drop_field("thinking", "not supported");
        assert!(!r.is_lossless);
        assert_eq!(r.dropped_count(), 1);
        assert_eq!(r.dropped_fields(), vec!["thinking"]);
    }

    #[test]
    fn report_approximate_field() {
        let mut r = FidelityReport::lossless();
        r.approximate_field("temperature", "clamped to 0..1 range");
        assert!(!r.is_lossless);
        assert_eq!(r.approximated_count(), 1);
        assert_eq!(r.dropped_count(), 0);
    }

    #[test]
    fn report_lossless_field() {
        let mut r = FidelityReport::lossless();
        r.lossless_field("messages");
        assert!(r.is_lossless);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].level, FidelityLevel::Lossless);
    }

    #[test]
    fn report_merge() {
        let mut r1 = FidelityReport::for_pair(Dialect::OpenAi, Dialect::Gemini);
        r1.lossless_field("messages");

        let mut r2 = FidelityReport::lossless();
        r2.drop_field("thinking", "not supported");

        r1.merge(r2);
        assert!(!r1.is_lossless);
        assert_eq!(r1.items.len(), 2);
        assert_eq!(r1.dropped_count(), 1);
    }

    #[test]
    fn report_serde_roundtrip() {
        let mut r = FidelityReport::for_pair(Dialect::Claude, Dialect::OpenAi);
        r.drop_field("thinking", "OpenAI has no thinking blocks");
        r.approximate_field("system", "inlined into messages");
        let json = serde_json::to_string(&r).unwrap();
        let back: FidelityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn report_serde_omits_empty_items() {
        let r = FidelityReport::lossless();
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("items"));
        assert!(!json.contains("source"));
        assert!(!json.contains("target"));
    }

    #[test]
    fn report_multiple_drops_and_approximations() {
        let mut r = FidelityReport::lossless();
        r.drop_field("thinking", "not supported");
        r.drop_field("images", "not supported");
        r.approximate_field("system", "inlined");
        assert_eq!(r.dropped_count(), 2);
        assert_eq!(r.approximated_count(), 1);
        assert_eq!(r.dropped_fields(), vec!["thinking", "images"]);
        assert!(!r.is_lossless);
    }
}
