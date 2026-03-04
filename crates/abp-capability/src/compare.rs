// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Capability comparison and compatibility scoring.
//!
//! Provides functions to compare two capability sets, identify gaps,
//! compute a compatibility score, and generate human-readable reports.

use crate::{check_capability, EmulationStrategy, SupportLevel};
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How a single capability compares between required and available sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CapabilityComparisonStatus {
    /// Both sets have the capability at the same level.
    Match {
        /// The support level in the available set.
        level: SupportLevel,
    },
    /// Available set supports it but at a different (potentially lower) level.
    Downgraded {
        /// The support level in the required set.
        required_level: SupportLevel,
        /// The support level in the available set.
        available_level: SupportLevel,
    },
    /// Available set supports it at a higher level than required.
    Upgraded {
        /// The support level in the required set.
        required_level: SupportLevel,
        /// The support level in the available set.
        available_level: SupportLevel,
    },
    /// Capability is in the required set but missing or unsupported in available.
    Gap {
        /// Why the capability is missing.
        reason: String,
    },
    /// Capability is in the available set but not required.
    Extra {
        /// The support level in the available set.
        level: SupportLevel,
    },
}

impl fmt::Display for CapabilityComparisonStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Match { level } => write!(f, "match ({level})"),
            Self::Downgraded {
                required_level,
                available_level,
            } => write!(f, "downgraded ({required_level} → {available_level})"),
            Self::Upgraded {
                required_level,
                available_level,
            } => write!(f, "upgraded ({required_level} → {available_level})"),
            Self::Gap { reason } => write!(f, "gap: {reason}"),
            Self::Extra { level } => write!(f, "extra ({level})"),
        }
    }
}

/// Per-capability comparison entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonEntry {
    /// The capability being compared.
    pub capability: Capability,
    /// The comparison status.
    pub status: CapabilityComparisonStatus,
}

impl fmt::Display for ComparisonEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.capability, self.status)
    }
}

/// Result of comparing two capability sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonResult {
    /// Per-capability comparison details.
    pub entries: Vec<ComparisonEntry>,
    /// Number of capabilities that match exactly.
    pub match_count: usize,
    /// Number of capabilities upgraded in available vs required.
    pub upgrade_count: usize,
    /// Number of capabilities downgraded in available vs required.
    pub downgrade_count: usize,
    /// Number of gaps (required but not available).
    pub gap_count: usize,
    /// Number of extras (available but not required).
    pub extra_count: usize,
    /// Compatibility score from 0.0 (no overlap) to 1.0 (perfect match or better).
    pub score: f64,
}

impl ComparisonResult {
    /// Returns `true` if there are no gaps.
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.gap_count == 0
    }

    /// Returns only the gap entries.
    #[must_use]
    pub fn gaps(&self) -> Vec<&ComparisonEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, CapabilityComparisonStatus::Gap { .. }))
            .collect()
    }

    /// Returns only the downgraded entries.
    #[must_use]
    pub fn downgrades(&self) -> Vec<&ComparisonEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, CapabilityComparisonStatus::Downgraded { .. }))
            .collect()
    }
}

impl fmt::Display for ComparisonResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "score: {:.1}% — {} match, {} upgraded, {} downgraded, {} gaps, {} extra",
            self.score * 100.0,
            self.match_count,
            self.upgrade_count,
            self.downgrade_count,
            self.gap_count,
            self.extra_count,
        )
    }
}

/// A human-readable compatibility report between two capability sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    /// The label for the required/source set.
    pub required_label: String,
    /// The label for the available/target set.
    pub available_label: String,
    /// The comparison result.
    pub result: ComparisonResult,
    /// Human-readable summary lines.
    pub summary: Vec<String>,
}

impl fmt::Display for CompatibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Compatibility: {} → {}",
            self.required_label, self.available_label
        )?;
        writeln!(f, "{}", self.result)?;
        for line in &self.summary {
            writeln!(f, "  {line}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Numeric rank for a [`CoreSupportLevel`].
fn support_rank(level: &CoreSupportLevel) -> u8 {
    match level {
        CoreSupportLevel::Native => 3,
        CoreSupportLevel::Emulated => 2,
        CoreSupportLevel::Restricted { .. } => 1,
        CoreSupportLevel::Unsupported => 0,
    }
}

/// Convert a [`CoreSupportLevel`] to a crate-local [`SupportLevel`].
fn to_local_level(level: &CoreSupportLevel) -> SupportLevel {
    match level {
        CoreSupportLevel::Native => SupportLevel::Native,
        CoreSupportLevel::Emulated => SupportLevel::Emulated {
            method: "adapter".into(),
        },
        CoreSupportLevel::Restricted { reason } => SupportLevel::Restricted {
            reason: reason.clone(),
        },
        CoreSupportLevel::Unsupported => SupportLevel::Unsupported {
            reason: "unsupported".into(),
        },
    }
}

/// Compare two capability manifests for compatibility.
///
/// Examines every capability present in either manifest and classifies
/// it as a match, upgrade, downgrade, gap, or extra.
///
/// # Arguments
///
/// * `required` - The capabilities needed (source/baseline)
/// * `available` - The capabilities offered (target)
///
/// # Examples
///
/// ```
/// use abp_capability::compare::compare;
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let mut required = BTreeMap::new();
/// required.insert(Capability::Streaming, CoreSupportLevel::Native);
/// required.insert(Capability::Vision, CoreSupportLevel::Native);
///
/// let mut available = BTreeMap::new();
/// available.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let result = compare(&required, &available);
/// assert_eq!(result.match_count, 1);
/// assert_eq!(result.gap_count, 1);
/// assert!(!result.is_compatible());
/// ```
#[must_use]
pub fn compare(required: &CapabilityManifest, available: &CapabilityManifest) -> ComparisonResult {
    let mut entries = Vec::new();
    let mut match_count = 0usize;
    let mut upgrade_count = 0usize;
    let mut downgrade_count = 0usize;
    let mut gap_count = 0usize;
    let mut extra_count = 0usize;

    // Check all required capabilities against available
    for (cap, req_level) in required {
        if matches!(req_level, CoreSupportLevel::Unsupported) {
            continue; // Unsupported in required is not a real requirement
        }

        let entry = match available.get(cap) {
            Some(avail_level) if matches!(avail_level, CoreSupportLevel::Unsupported) => {
                gap_count += 1;
                ComparisonEntry {
                    capability: cap.clone(),
                    status: CapabilityComparisonStatus::Gap {
                        reason: "explicitly unsupported in target".into(),
                    },
                }
            }
            Some(avail_level) => {
                let req_rank = support_rank(req_level);
                let avail_rank = support_rank(avail_level);

                if req_rank == avail_rank {
                    match_count += 1;
                    ComparisonEntry {
                        capability: cap.clone(),
                        status: CapabilityComparisonStatus::Match {
                            level: to_local_level(avail_level),
                        },
                    }
                } else if avail_rank > req_rank {
                    upgrade_count += 1;
                    ComparisonEntry {
                        capability: cap.clone(),
                        status: CapabilityComparisonStatus::Upgraded {
                            required_level: to_local_level(req_level),
                            available_level: to_local_level(avail_level),
                        },
                    }
                } else {
                    downgrade_count += 1;
                    ComparisonEntry {
                        capability: cap.clone(),
                        status: CapabilityComparisonStatus::Downgraded {
                            required_level: to_local_level(req_level),
                            available_level: to_local_level(avail_level),
                        },
                    }
                }
            }
            None => {
                gap_count += 1;
                ComparisonEntry {
                    capability: cap.clone(),
                    status: CapabilityComparisonStatus::Gap {
                        reason: "not declared in target manifest".into(),
                    },
                }
            }
        };
        entries.push(entry);
    }

    // Check available capabilities not in required (extras)
    for (cap, avail_level) in available {
        if matches!(avail_level, CoreSupportLevel::Unsupported) {
            continue;
        }
        if !required.contains_key(cap) {
            extra_count += 1;
            entries.push(ComparisonEntry {
                capability: cap.clone(),
                status: CapabilityComparisonStatus::Extra {
                    level: to_local_level(avail_level),
                },
            });
        }
    }

    let score = compute_score(match_count, upgrade_count, downgrade_count, gap_count);

    ComparisonResult {
        entries,
        match_count,
        upgrade_count,
        downgrade_count,
        gap_count,
        extra_count,
        score,
    }
}

/// Compute a compatibility score from 0.0 to 1.0.
///
/// - Matches and upgrades count as full satisfaction (1.0 each)
/// - Downgrades count as partial satisfaction (0.5 each)
/// - Gaps count as zero satisfaction
fn compute_score(matches: usize, upgrades: usize, downgrades: usize, gaps: usize) -> f64 {
    let total = matches + upgrades + downgrades + gaps;
    if total == 0 {
        return 1.0; // Vacuously compatible
    }
    let numerator = (matches as f64) + (upgrades as f64) + (downgrades as f64 * 0.5);
    let score = numerator / (total as f64);
    // Clamp to [0.0, 1.0]
    score.clamp(0.0, 1.0)
}

/// Identify capabilities that are gaps between required and available.
///
/// Returns the list of capabilities present in `required` but missing or
/// unsupported in `available`.
///
/// # Examples
///
/// ```
/// use abp_capability::compare::identify_gaps;
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let mut required = BTreeMap::new();
/// required.insert(Capability::Streaming, CoreSupportLevel::Native);
/// required.insert(Capability::Vision, CoreSupportLevel::Native);
///
/// let available = BTreeMap::new();
///
/// let gaps = identify_gaps(&required, &available);
/// assert_eq!(gaps.len(), 2);
/// ```
#[must_use]
pub fn identify_gaps(
    required: &CapabilityManifest,
    available: &CapabilityManifest,
) -> Vec<Capability> {
    required
        .iter()
        .filter(|(_, level)| !matches!(level, CoreSupportLevel::Unsupported))
        .filter(|(cap, _)| {
            !matches!(
                available.get(cap),
                Some(CoreSupportLevel::Native)
                    | Some(CoreSupportLevel::Emulated)
                    | Some(CoreSupportLevel::Restricted { .. })
            )
        })
        .map(|(cap, _)| cap.clone())
        .collect()
}

/// Compute a compatibility score between two manifests (0.0–1.0).
///
/// A convenience wrapper around [`compare`] that returns only the score.
///
/// # Examples
///
/// ```
/// use abp_capability::compare::score;
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let mut required = BTreeMap::new();
/// required.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let mut available = BTreeMap::new();
/// available.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let s = score(&required, &available);
/// assert!((s - 1.0).abs() < f64::EPSILON);
/// ```
#[must_use]
pub fn score(required: &CapabilityManifest, available: &CapabilityManifest) -> f64 {
    compare(required, available).score
}

/// Generate a human-readable compatibility report.
///
/// # Examples
///
/// ```
/// use abp_capability::compare::generate_compatibility_report;
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let mut required = BTreeMap::new();
/// required.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let mut available = BTreeMap::new();
/// available.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let report = generate_compatibility_report("source", &required, "target", &available);
/// assert!(report.result.is_compatible());
/// ```
#[must_use]
pub fn generate_compatibility_report(
    required_label: &str,
    required: &CapabilityManifest,
    available_label: &str,
    available: &CapabilityManifest,
) -> CompatibilityReport {
    let result = compare(required, available);
    let mut summary = Vec::new();

    if result.is_compatible() {
        summary.push(format!(
            "✓ {available_label} is compatible with {required_label}"
        ));
    } else {
        summary.push(format!(
            "✗ {available_label} has {} gap(s) compared to {required_label}",
            result.gap_count,
        ));
    }

    summary.push(format!(
        "Score: {:.1}% ({} match, {} upgraded, {} downgraded, {} gaps)",
        result.score * 100.0,
        result.match_count,
        result.upgrade_count,
        result.downgrade_count,
        result.gap_count,
    ));

    if result.gap_count > 0 {
        summary.push("Missing capabilities:".to_owned());
        for entry in &result.entries {
            if let CapabilityComparisonStatus::Gap { reason } = &entry.status {
                summary.push(format!("  - {:?}: {reason}", entry.capability));
            }
        }
    }

    if result.downgrade_count > 0 {
        summary.push("Downgraded capabilities:".to_owned());
        for entry in &result.entries {
            if let CapabilityComparisonStatus::Downgraded {
                required_level,
                available_level,
            } = &entry.status
            {
                summary.push(format!(
                    "  - {:?}: {required_level} → {available_level}",
                    entry.capability,
                ));
            }
        }
    }

    CompatibilityReport {
        required_label: required_label.to_owned(),
        available_label: available_label.to_owned(),
        result,
        summary,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{Capability, SupportLevel as CoreSupportLevel};
    use std::collections::BTreeMap;

    fn manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
        entries.iter().cloned().collect()
    }

    // ---- compare: basic cases -------------------------------------------

    #[test]
    fn compare_identical_manifests() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let result = compare(&m, &m);
        assert_eq!(result.match_count, 2);
        assert_eq!(result.gap_count, 0);
        assert_eq!(result.upgrade_count, 0);
        assert_eq!(result.downgrade_count, 0);
        assert_eq!(result.extra_count, 0);
        assert!(result.is_compatible());
        assert!((result.score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compare_empty_manifests() {
        let m: CapabilityManifest = BTreeMap::new();
        let result = compare(&m, &m);
        assert_eq!(result.match_count, 0);
        assert_eq!(result.gap_count, 0);
        assert!(result.is_compatible());
        assert!((result.score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compare_all_gaps() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let available: CapabilityManifest = BTreeMap::new();
        let result = compare(&required, &available);
        assert_eq!(result.gap_count, 2);
        assert_eq!(result.match_count, 0);
        assert!(!result.is_compatible());
        assert!((result.score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compare_with_upgrades() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = compare(&required, &available);
        assert_eq!(result.upgrade_count, 1);
        assert_eq!(result.match_count, 0);
        assert!(result.is_compatible());
        assert!((result.score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compare_with_downgrades() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let result = compare(&required, &available);
        assert_eq!(result.downgrade_count, 1);
        assert!(result.is_compatible()); // downgrade != gap
        assert!((result.score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn compare_with_extras() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let result = compare(&required, &available);
        assert_eq!(result.match_count, 1);
        assert_eq!(result.extra_count, 1);
        assert!(result.is_compatible());
    }

    #[test]
    fn compare_mixed_scenario() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),     // match
            (Capability::ToolUse, CoreSupportLevel::Native),       // downgrade
            (Capability::Vision, CoreSupportLevel::Native),        // gap
            (Capability::Audio, CoreSupportLevel::Emulated),       // upgrade
        ]);
        let available = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
            (Capability::Audio, CoreSupportLevel::Native),
            (Capability::Logprobs, CoreSupportLevel::Native), // extra
        ]);
        let result = compare(&required, &available);
        assert_eq!(result.match_count, 1);     // Streaming
        assert_eq!(result.downgrade_count, 1); // ToolUse
        assert_eq!(result.gap_count, 1);       // Vision
        assert_eq!(result.upgrade_count, 1);   // Audio
        assert_eq!(result.extra_count, 1);     // Logprobs
        assert!(!result.is_compatible());
        // Score: (1 + 1 + 0.5 + 0) / 4 = 2.5/4 = 0.625
        assert!((result.score - 0.625).abs() < f64::EPSILON);
    }

    #[test]
    fn compare_unsupported_in_required_is_skipped() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Unsupported),
        ]);
        let available: CapabilityManifest = BTreeMap::new();
        let result = compare(&required, &available);
        assert_eq!(result.gap_count, 0);
        assert!(result.is_compatible());
    }

    #[test]
    fn compare_explicitly_unsupported_in_available_is_gap() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Unsupported)]);
        let result = compare(&required, &available);
        assert_eq!(result.gap_count, 1);
        assert!(!result.is_compatible());
    }

    // ---- identify_gaps ---------------------------------------------------

    #[test]
    fn identify_gaps_none() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let gaps = identify_gaps(&required, &available);
        assert!(gaps.is_empty());
    }

    #[test]
    fn identify_gaps_some_missing() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let gaps = identify_gaps(&required, &available);
        assert_eq!(gaps, vec![Capability::Vision]);
    }

    #[test]
    fn identify_gaps_emulated_is_not_gap() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let gaps = identify_gaps(&required, &available);
        assert!(gaps.is_empty());
    }

    // ---- score -----------------------------------------------------------

    #[test]
    fn score_perfect_match() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let s = score(&m, &m);
        assert!((s - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn score_no_overlap() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available: CapabilityManifest = BTreeMap::new();
        let s = score(&required, &available);
        assert!((s - 0.0).abs() < f64::EPSILON);
    }

    // ---- generate_compatibility_report -----------------------------------

    #[test]
    fn report_compatible() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let report =
            generate_compatibility_report("source", &required, "target", &available);
        assert!(report.result.is_compatible());
        assert!(report.summary[0].contains("compatible"));
        assert_eq!(report.required_label, "source");
        assert_eq!(report.available_label, "target");
    }

    #[test]
    fn report_incompatible_lists_gaps() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let report =
            generate_compatibility_report("source", &required, "target", &available);
        assert!(!report.result.is_compatible());
        let text = format!("{report}");
        assert!(text.contains("gap"));
        assert!(text.contains("Vision"));
    }

    #[test]
    fn report_with_downgrades_lists_them() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let report =
            generate_compatibility_report("source", &required, "target", &available);
        assert!(report.result.is_compatible()); // downgrade is not a gap
        let text = format!("{report}");
        assert!(text.contains("Downgraded"));
    }

    // ---- Display impls ---------------------------------------------------

    #[test]
    fn comparison_result_display() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = compare(&required, &available);
        let s = format!("{result}");
        assert!(s.contains("100.0%"));
        assert!(s.contains("1 match"));
    }

    #[test]
    fn comparison_entry_display() {
        let entry = ComparisonEntry {
            capability: Capability::Streaming,
            status: CapabilityComparisonStatus::Match {
                level: SupportLevel::Native,
            },
        };
        let s = format!("{entry}");
        assert!(s.contains("Streaming"));
        assert!(s.contains("match"));
    }

    #[test]
    fn comparison_status_display_all_variants() {
        let match_s = CapabilityComparisonStatus::Match {
            level: SupportLevel::Native,
        };
        assert!(format!("{match_s}").contains("match"));

        let down = CapabilityComparisonStatus::Downgraded {
            required_level: SupportLevel::Native,
            available_level: SupportLevel::Emulated {
                method: "adapter".into(),
            },
        };
        assert!(format!("{down}").contains("downgraded"));

        let up = CapabilityComparisonStatus::Upgraded {
            required_level: SupportLevel::Emulated {
                method: "adapter".into(),
            },
            available_level: SupportLevel::Native,
        };
        assert!(format!("{up}").contains("upgraded"));

        let gap = CapabilityComparisonStatus::Gap {
            reason: "missing".into(),
        };
        assert!(format!("{gap}").contains("gap"));

        let extra = CapabilityComparisonStatus::Extra {
            level: SupportLevel::Native,
        };
        assert!(format!("{extra}").contains("extra"));
    }

    // ---- ComparisonResult helpers ----------------------------------------

    #[test]
    fn comparison_result_gaps_helper() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = compare(&required, &available);
        let gaps = result.gaps();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].capability, Capability::Vision);
    }

    #[test]
    fn comparison_result_downgrades_helper() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let result = compare(&required, &available);
        let downgrades = result.downgrades();
        assert_eq!(downgrades.len(), 1);
        assert_eq!(downgrades[0].capability, Capability::Streaming);
    }

    // ---- serde roundtrips ------------------------------------------------

    #[test]
    fn comparison_result_serde_roundtrip() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = compare(&required, &available);
        let json = serde_json::to_string(&result).unwrap();
        let back: ComparisonResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.match_count, result.match_count);
        assert_eq!(back.gap_count, result.gap_count);
        assert!((back.score - result.score).abs() < f64::EPSILON);
    }

    #[test]
    fn compatibility_report_serde_roundtrip() {
        let required = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let available = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let report =
            generate_compatibility_report("source", &required, "target", &available);
        let json = serde_json::to_string(&report).unwrap();
        let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.required_label, "source");
        assert_eq!(back.available_label, "target");
    }

    // ---- edge cases ------------------------------------------------------

    #[test]
    fn compare_restricted_in_available_matches_restricted_in_required() {
        let required = manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let available = manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let result = compare(&required, &available);
        assert_eq!(result.match_count, 1);
        assert!(result.is_compatible());
    }

    #[test]
    fn score_with_only_downgrades() {
        let required = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let available = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let s = score(&required, &available);
        assert!((s - 0.5).abs() < f64::EPSILON);
    }
}
