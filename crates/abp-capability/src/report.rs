// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Human-readable compatibility reporting for dialect mapping.
//!
//! [`DialectCompatibilityReport`] aggregates negotiation results into a
//! compatibility matrix that can be rendered as a plain-text table, Markdown,
//! or JSON.

use crate::emulation::{self, EmulationPlan, build_emulation_plan};
use crate::{
    CapabilityRegistry, DialectNegotiationResult, TransitionKind, negotiate_dialects,
};
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Classification of a single capability in the compatibility matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatus {
    /// Both dialects support natively — pass through.
    Native,
    /// Target can emulate the feature.
    Emulated,
    /// Target has partial/degraded support.
    Degraded,
    /// Target does not support this feature at all.
    Unsupported,
}

impl fmt::Display for FeatureStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native => write!(f, "native"),
            Self::Emulated => write!(f, "emulated"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unsupported => write!(f, "unsupported"),
        }
    }
}

/// A single row in the compatibility matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixEntry {
    /// The capability.
    pub capability: Capability,
    /// Support level in the source dialect.
    pub source_level: String,
    /// Support level in the target dialect.
    pub target_level: String,
    /// Overall status for this capability.
    pub status: FeatureStatus,
}

/// Full compatibility report between two dialects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialectCompatibilityReport {
    /// Source dialect name.
    pub source: String,
    /// Target dialect name.
    pub target: String,
    /// Per-capability matrix entries.
    pub entries: Vec<MatrixEntry>,
    /// Summary counts.
    pub native_count: usize,
    /// Number emulated.
    pub emulated_count: usize,
    /// Number degraded.
    pub degraded_count: usize,
    /// Number unsupported.
    pub unsupported_count: usize,
    /// Emulation plan for features that need it.
    pub emulation_plan: EmulationPlan,
}

impl DialectCompatibilityReport {
    /// Returns `true` if all features are native or emulated (no unsupported).
    #[must_use]
    pub fn is_viable(&self) -> bool {
        self.unsupported_count == 0
    }

    /// Overall compatibility score from 0.0 to 1.0.
    #[must_use]
    pub fn score(&self) -> f64 {
        let total = self.entries.len();
        if total == 0 {
            return 1.0;
        }
        let weighted: f64 = self
            .entries
            .iter()
            .map(|e| match e.status {
                FeatureStatus::Native => 1.0,
                FeatureStatus::Emulated => 0.7,
                FeatureStatus::Degraded => 0.4,
                FeatureStatus::Unsupported => 0.0,
            })
            .sum();
        weighted / total as f64
    }

    /// Render as a plain-text table.
    #[must_use]
    pub fn format_table(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Compatibility: {} → {}\n",
            self.source, self.target
        ));
        out.push_str(&format!(
            "Score: {:.0}% | {} native, {} emulated, {} degraded, {} unsupported\n",
            self.score() * 100.0,
            self.native_count,
            self.emulated_count,
            self.degraded_count,
            self.unsupported_count,
        ));
        out.push_str(&"-".repeat(72));
        out.push('\n');
        out.push_str(&format!(
            "{:<28} {:<12} {:<12} {:<12}\n",
            "Capability", "Source", "Target", "Status"
        ));
        out.push_str(&"-".repeat(72));
        out.push('\n');
        for entry in &self.entries {
            out.push_str(&format!(
                "{:<28} {:<12} {:<12} {:<12}\n",
                format!("{:?}", entry.capability),
                entry.source_level,
                entry.target_level,
                entry.status,
            ));
        }
        out.push_str(&"-".repeat(72));
        out.push('\n');
        out
    }

    /// Render as a Markdown table.
    #[must_use]
    pub fn format_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "## Compatibility: {} → {}\n\n",
            self.source, self.target
        ));
        out.push_str(&format!(
            "**Score: {:.0}%** | {} native, {} emulated, {} degraded, {} unsupported\n\n",
            self.score() * 100.0,
            self.native_count,
            self.emulated_count,
            self.degraded_count,
            self.unsupported_count,
        ));
        out.push_str("| Capability | Source | Target | Status |\n");
        out.push_str("|---|---|---|---|\n");
        for entry in &self.entries {
            out.push_str(&format!(
                "| {:?} | {} | {} | {} |\n",
                entry.capability, entry.source_level, entry.target_level, entry.status,
            ));
        }
        out
    }

    /// Render as a JSON string.
    #[must_use]
    pub fn format_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

impl fmt::Display for DialectCompatibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} → {}: score={:.0}%, {} native, {} emulated, {} degraded, {} unsupported",
            self.source,
            self.target,
            self.score() * 100.0,
            self.native_count,
            self.emulated_count,
            self.degraded_count,
            self.unsupported_count,
        )
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Map a [`TransitionKind`] to a [`FeatureStatus`].
fn transition_to_status(kind: TransitionKind) -> FeatureStatus {
    match kind {
        TransitionKind::Unchanged | TransitionKind::Upgrade => FeatureStatus::Native,
        TransitionKind::Downgrade => FeatureStatus::Degraded,
        TransitionKind::Lost => FeatureStatus::Unsupported,
    }
}

/// Build a compatibility report from a [`DialectNegotiationResult`].
#[must_use]
pub fn build_report(negotiation: &DialectNegotiationResult) -> DialectCompatibilityReport {
    let mut entries = Vec::new();
    let mut native_count = 0usize;
    let mut emulated_count = 0usize;
    let mut degraded_count = 0usize;
    let mut unsupported_count = 0usize;

    for t in &negotiation.transitions {
        let status = transition_to_status(t.kind);
        match status {
            FeatureStatus::Native => native_count += 1,
            FeatureStatus::Emulated => emulated_count += 1,
            FeatureStatus::Degraded => degraded_count += 1,
            FeatureStatus::Unsupported => unsupported_count += 1,
        }
        entries.push(MatrixEntry {
            capability: t.capability.clone(),
            source_level: t.from.clone(),
            target_level: t.to.clone(),
            status,
        });
    }

    let emulation_plan = build_emulation_plan(&negotiation.emulation_plan);

    DialectCompatibilityReport {
        source: negotiation.source.clone(),
        target: negotiation.target.clone(),
        entries,
        native_count,
        emulated_count,
        degraded_count,
        unsupported_count,
        emulation_plan,
    }
}

/// Build a compatibility report directly from two named manifests.
#[must_use]
pub fn build_report_from_manifests(
    source_name: &str,
    source_manifest: &CapabilityManifest,
    target_name: &str,
    target_manifest: &CapabilityManifest,
) -> DialectCompatibilityReport {
    let negotiation = negotiate_dialects(source_name, source_manifest, target_name, target_manifest);
    build_report(&negotiation)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::SupportLevel as CoreSupportLevel;
    use std::collections::BTreeMap;

    fn manifest_from(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
        entries.iter().cloned().collect()
    }

    #[test]
    fn report_same_manifests() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let report = build_report_from_manifests("a", &m, "b", &m);
        assert!(report.is_viable());
        assert_eq!(report.native_count, 2);
        assert_eq!(report.unsupported_count, 0);
        assert!((report.score() - 1.0).abs() < 0.01);
    }

    #[test]
    fn report_with_loss() {
        let src = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let tgt = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
        ]);
        let report = build_report_from_manifests("src", &src, "tgt", &tgt);
        assert!(!report.is_viable());
        assert_eq!(report.native_count, 1);
        assert_eq!(report.unsupported_count, 1);
    }

    #[test]
    fn report_with_downgrade() {
        let src = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let tgt = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let report = build_report_from_manifests("src", &src, "tgt", &tgt);
        assert_eq!(report.degraded_count, 1);
        assert!(report.is_viable());
    }

    #[test]
    fn report_score_empty() {
        let m: CapabilityManifest = BTreeMap::new();
        let report = build_report_from_manifests("a", &m, "b", &m);
        assert!((report.score() - 1.0).abs() < 0.01);
    }

    #[test]
    fn format_table_contains_headers() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let report = build_report_from_manifests("openai", &m, "claude", &m);
        let table = report.format_table();
        assert!(table.contains("Capability"));
        assert!(table.contains("Source"));
        assert!(table.contains("Target"));
        assert!(table.contains("Status"));
        assert!(table.contains("openai"));
        assert!(table.contains("claude"));
    }

    #[test]
    fn format_markdown_has_table() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let report = build_report_from_manifests("openai", &m, "claude", &m);
        let md = report.format_markdown();
        assert!(md.contains("##"));
        assert!(md.contains("|"));
        assert!(md.contains("Score"));
    }

    #[test]
    fn format_json_valid() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let report = build_report_from_manifests("openai", &m, "claude", &m);
        let json = report.format_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["source"], "openai");
        assert_eq!(parsed["target"], "claude");
    }

    #[test]
    fn report_display() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let report = build_report_from_manifests("a", &m, "b", &m);
        let s = format!("{report}");
        assert!(s.contains("a → b"));
        assert!(s.contains("score="));
    }

    #[test]
    fn report_serde_roundtrip() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Emulated),
        ]);
        let report = build_report_from_manifests("src", &m, "tgt", &m);
        let json = serde_json::to_string(&report).unwrap();
        let back: DialectCompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source, report.source);
        assert_eq!(back.entries.len(), report.entries.len());
    }

    #[test]
    fn feature_status_display() {
        assert_eq!(FeatureStatus::Native.to_string(), "native");
        assert_eq!(FeatureStatus::Emulated.to_string(), "emulated");
        assert_eq!(FeatureStatus::Degraded.to_string(), "degraded");
        assert_eq!(FeatureStatus::Unsupported.to_string(), "unsupported");
    }

    #[test]
    fn feature_status_serde_roundtrip() {
        for s in [
            FeatureStatus::Native,
            FeatureStatus::Emulated,
            FeatureStatus::Degraded,
            FeatureStatus::Unsupported,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: FeatureStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn report_from_real_manifests() {
        use crate::{claude_35_sonnet_manifest, openai_gpt4o_manifest};
        let report = build_report_from_manifests(
            "claude",
            &claude_35_sonnet_manifest(),
            "openai",
            &openai_gpt4o_manifest(),
        );
        // Claude→OpenAI loses ExtendedThinking
        assert!(
            report.entries.iter().any(|e| e.capability == Capability::ExtendedThinking
                && e.status == FeatureStatus::Unsupported)
        );
        assert!(!report.is_viable());
    }

    #[test]
    fn report_score_all_degraded() {
        let src = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let tgt = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let report = build_report_from_manifests("src", &src, "tgt", &tgt);
        assert_eq!(report.degraded_count, 2);
        assert!(report.score() < 1.0);
        assert!(report.score() > 0.0);
    }

    #[test]
    fn matrix_entry_contains_levels() {
        let src = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let tgt = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let report = build_report_from_manifests("src", &src, "tgt", &tgt);
        let entry = &report.entries[0];
        assert_eq!(entry.source_level, "native");
        assert_eq!(entry.target_level, "emulated");
    }
}
