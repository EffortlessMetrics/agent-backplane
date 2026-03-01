// SPDX-License-Identifier: MIT OR Apache-2.0
//! Advanced capability negotiation between work-order requirements and backend manifests.

use crate::{Capability, CapabilityManifest, SupportLevel, WorkOrder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Numeric rank for [`SupportLevel`] used during negotiation comparisons.
fn support_rank(level: &SupportLevel) -> u8 {
    match level {
        SupportLevel::Native => 3,
        SupportLevel::Emulated => 2,
        SupportLevel::Restricted { .. } => 1,
        SupportLevel::Unsupported => 0,
    }
}

/// A request describing what capabilities a caller needs from a backend.
#[derive(Debug, Clone)]
pub struct NegotiationRequest {
    /// Capabilities that **must** be present at or above `minimum_support`.
    pub required: Vec<Capability>,
    /// Capabilities that are nice-to-have but not mandatory.
    pub preferred: Vec<Capability>,
    /// The lowest [`SupportLevel`] that counts as "satisfied".
    pub minimum_support: SupportLevel,
}

/// The outcome of negotiating a [`NegotiationRequest`] against a [`CapabilityManifest`].
#[derive(Debug, Clone)]
pub struct NegotiationResult {
    /// Required capabilities that the manifest satisfies.
    pub satisfied: Vec<Capability>,
    /// Required capabilities the manifest does **not** satisfy.
    pub unsatisfied: Vec<Capability>,
    /// Preferred capabilities the manifest satisfies (extra value).
    pub bonus: Vec<Capability>,
    /// `true` when every required capability is satisfied.
    pub is_compatible: bool,
}

/// Stateless negotiator that matches requests against manifests.
pub struct CapabilityNegotiator;

impl CapabilityNegotiator {
    /// Check a single manifest against a request.
    #[must_use]
    pub fn negotiate(
        request: &NegotiationRequest,
        manifest: &CapabilityManifest,
    ) -> NegotiationResult {
        let min_rank = support_rank(&request.minimum_support);

        let mut satisfied = Vec::new();
        let mut unsatisfied = Vec::new();

        for cap in &request.required {
            let meets = manifest
                .get(cap)
                .is_some_and(|level| support_rank(level) >= min_rank);
            if meets {
                satisfied.push(cap.clone());
            } else {
                unsatisfied.push(cap.clone());
            }
        }

        let bonus: Vec<Capability> = request
            .preferred
            .iter()
            .filter(|cap| {
                manifest
                    .get(cap)
                    .is_some_and(|level| support_rank(level) >= min_rank)
            })
            .cloned()
            .collect();

        let is_compatible = unsatisfied.is_empty();

        NegotiationResult {
            satisfied,
            unsatisfied,
            bonus,
            is_compatible,
        }
    }

    /// Pick the best manifest from a named set.
    ///
    /// Returns `None` when no manifest is compatible. Among compatible
    /// manifests the one with the highest total score
    /// (`satisfied.len() + bonus.len()`) wins; ties are broken by name order.
    #[must_use]
    pub fn best_match(
        request: &NegotiationRequest,
        manifests: &[(&str, CapabilityManifest)],
    ) -> Option<(String, NegotiationResult)> {
        manifests
            .iter()
            .map(|(name, manifest)| {
                let result = Self::negotiate(request, manifest);
                (name.to_string(), result)
            })
            .filter(|(_, result)| result.is_compatible)
            .max_by(|(name_a, a), (name_b, b)| {
                let score_a = a.satisfied.len() + a.bonus.len();
                let score_b = b.satisfied.len() + b.bonus.len();
                score_a.cmp(&score_b).then_with(|| name_b.cmp(name_a)) // deterministic tie-break
            })
    }
}

/// Describes the difference between two [`CapabilityManifest`]s.
#[derive(Debug, Clone)]
pub struct CapabilityDiff {
    /// Capabilities present in `new` but absent from `old`.
    pub added: Vec<Capability>,
    /// Capabilities present in `old` but absent from `new`.
    pub removed: Vec<Capability>,
    /// Capabilities whose support level increased (`old_level`, `new_level`).
    pub upgraded: Vec<(Capability, SupportLevel, SupportLevel)>,
    /// Capabilities whose support level decreased (`old_level`, `new_level`).
    pub downgraded: Vec<(Capability, SupportLevel, SupportLevel)>,
}

impl CapabilityDiff {
    /// Compute the diff from `old` to `new`.
    #[must_use]
    pub fn diff(old: &CapabilityManifest, new: &CapabilityManifest) -> Self {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut upgraded = Vec::new();
        let mut downgraded = Vec::new();

        for (cap, new_level) in new {
            match old.get(cap) {
                None => added.push(cap.clone()),
                Some(old_level) => {
                    let old_r = support_rank(old_level);
                    let new_r = support_rank(new_level);
                    if new_r > old_r {
                        upgraded.push((cap.clone(), old_level.clone(), new_level.clone()));
                    } else if new_r < old_r {
                        downgraded.push((cap.clone(), old_level.clone(), new_level.clone()));
                    }
                }
            }
        }

        for cap in old.keys() {
            if !new.contains_key(cap) {
                removed.push(cap.clone());
            }
        }

        Self {
            added,
            removed,
            upgraded,
            downgraded,
        }
    }
}

// ---------------------------------------------------------------------------
// Dialect-aware capability negotiation
// ---------------------------------------------------------------------------

/// How well a capability is supported when translating between two dialects.
///
/// Unlike [`SupportLevel`] (which describes a backend's raw support),
/// `DialectSupportLevel` captures whether translation between a *source*
/// and *target* dialect can preserve the capability natively, requires
/// emulation, or is impossible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum DialectSupportLevel {
    /// The target dialect supports this capability natively — no translation needed.
    Native,
    /// The capability can be emulated via adapter logic.
    Emulated {
        /// Human-readable explanation of how emulation works.
        detail: String,
    },
    /// The capability cannot be provided for this dialect pair.
    Unsupported {
        /// Human-readable reason the capability is unavailable.
        reason: String,
    },
}

/// A single entry in a [`CapabilityReport`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReportEntry {
    /// The capability being assessed.
    pub capability: Capability,
    /// How the capability is supported for the dialect pair.
    pub support: DialectSupportLevel,
}

/// Result of a pre-execution capability check for a specific dialect pair.
///
/// Maps each requested capability to its [`DialectSupportLevel`], indicating
/// what is native, what needs emulation, and what will fail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    /// The source dialect (e.g. `"claude"`, `"openai"`).
    pub source_dialect: String,
    /// The target dialect (e.g. `"openai"`, `"gemini"`).
    pub target_dialect: String,
    /// Per-capability assessment.
    pub entries: Vec<CapabilityReportEntry>,
}

impl CapabilityReport {
    /// Returns entries with [`DialectSupportLevel::Native`].
    #[must_use]
    pub fn native_capabilities(&self) -> Vec<&CapabilityReportEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.support, DialectSupportLevel::Native))
            .collect()
    }

    /// Returns entries with [`DialectSupportLevel::Emulated`].
    #[must_use]
    pub fn emulated_capabilities(&self) -> Vec<&CapabilityReportEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.support, DialectSupportLevel::Emulated { .. }))
            .collect()
    }

    /// Returns entries with [`DialectSupportLevel::Unsupported`].
    #[must_use]
    pub fn unsupported_capabilities(&self) -> Vec<&CapabilityReportEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.support, DialectSupportLevel::Unsupported { .. }))
            .collect()
    }

    /// `true` when every requested capability is native or emulated.
    #[must_use]
    pub fn all_satisfiable(&self) -> bool {
        self.entries
            .iter()
            .all(|e| !matches!(e.support, DialectSupportLevel::Unsupported { .. }))
    }

    /// Serialize the report as a JSON value suitable for inclusion in receipt
    /// metadata (e.g. inside `usage_raw` or a vendor extension field).
    #[must_use]
    pub fn to_receipt_metadata(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Returns the built-in capability manifest for a well-known dialect.
///
/// Unknown dialects return an empty manifest.
#[must_use]
pub fn dialect_manifest(dialect: &str) -> BTreeMap<Capability, DialectSupportLevel> {
    match dialect {
        "claude" => claude_manifest(),
        "openai" => openai_manifest(),
        "gemini" => gemini_manifest(),
        _ => BTreeMap::new(),
    }
}

fn claude_manifest() -> BTreeMap<Capability, DialectSupportLevel> {
    BTreeMap::from([
        (Capability::Streaming, DialectSupportLevel::Native),
        (Capability::ToolUse, DialectSupportLevel::Native),
        (Capability::ToolRead, DialectSupportLevel::Native),
        (Capability::ToolWrite, DialectSupportLevel::Native),
        (Capability::ToolEdit, DialectSupportLevel::Native),
        (Capability::ToolBash, DialectSupportLevel::Native),
        (Capability::ToolGlob, DialectSupportLevel::Native),
        (Capability::ToolGrep, DialectSupportLevel::Native),
        (
            Capability::StructuredOutputJsonSchema,
            DialectSupportLevel::Emulated {
                detail: "tool_use with JSON schema".into(),
            },
        ),
        (Capability::ExtendedThinking, DialectSupportLevel::Native),
        (Capability::ImageInput, DialectSupportLevel::Native),
        (
            Capability::PdfInput,
            DialectSupportLevel::Emulated {
                detail: "converted to text before sending".into(),
            },
        ),
        (
            Capability::CodeExecution,
            DialectSupportLevel::Emulated {
                detail: "via tool_bash".into(),
            },
        ),
        (
            Capability::StopSequences,
            DialectSupportLevel::Native,
        ),
        (
            Capability::Logprobs,
            DialectSupportLevel::Unsupported {
                reason: "Claude API does not expose logprobs".into(),
            },
        ),
        (
            Capability::SeedDeterminism,
            DialectSupportLevel::Unsupported {
                reason: "Claude API does not support seed parameter".into(),
            },
        ),
    ])
}

fn openai_manifest() -> BTreeMap<Capability, DialectSupportLevel> {
    BTreeMap::from([
        (Capability::Streaming, DialectSupportLevel::Native),
        (Capability::ToolUse, DialectSupportLevel::Native),
        (Capability::ToolRead, DialectSupportLevel::Native),
        (Capability::ToolWrite, DialectSupportLevel::Native),
        (Capability::ToolEdit, DialectSupportLevel::Native),
        (Capability::ToolBash, DialectSupportLevel::Native),
        (
            Capability::StructuredOutputJsonSchema,
            DialectSupportLevel::Native,
        ),
        (Capability::ImageInput, DialectSupportLevel::Native),
        (
            Capability::PdfInput,
            DialectSupportLevel::Unsupported {
                reason: "OpenAI API does not accept PDF directly".into(),
            },
        ),
        (Capability::CodeExecution, DialectSupportLevel::Native),
        (Capability::Logprobs, DialectSupportLevel::Native),
        (Capability::SeedDeterminism, DialectSupportLevel::Native),
        (Capability::StopSequences, DialectSupportLevel::Native),
        (
            Capability::ExtendedThinking,
            DialectSupportLevel::Unsupported {
                reason: "OpenAI API does not expose extended thinking".into(),
            },
        ),
    ])
}

fn gemini_manifest() -> BTreeMap<Capability, DialectSupportLevel> {
    BTreeMap::from([
        (Capability::Streaming, DialectSupportLevel::Native),
        (Capability::ToolUse, DialectSupportLevel::Native),
        (Capability::ToolRead, DialectSupportLevel::Native),
        (Capability::ToolWrite, DialectSupportLevel::Native),
        (
            Capability::StructuredOutputJsonSchema,
            DialectSupportLevel::Native,
        ),
        (Capability::ImageInput, DialectSupportLevel::Native),
        (Capability::PdfInput, DialectSupportLevel::Native),
        (
            Capability::CodeExecution,
            DialectSupportLevel::Emulated {
                detail: "via code_execution tool".into(),
            },
        ),
        (
            Capability::Logprobs,
            DialectSupportLevel::Unsupported {
                reason: "Gemini API does not expose logprobs".into(),
            },
        ),
        (
            Capability::SeedDeterminism,
            DialectSupportLevel::Unsupported {
                reason: "Gemini API does not support seed parameter".into(),
            },
        ),
        (Capability::StopSequences, DialectSupportLevel::Native),
        (
            Capability::ExtendedThinking,
            DialectSupportLevel::Emulated {
                detail: "via thinking mode configuration".into(),
            },
        ),
    ])
}

/// Pre-execution capability check.
///
/// Given a [`WorkOrder`] and a source→target dialect pair, returns a
/// [`CapabilityReport`] indicating which requested capabilities are native,
/// emulated, or unsupported for the target dialect.
#[must_use]
pub fn check_capabilities(
    work_order: &WorkOrder,
    source_dialect: &str,
    target_dialect: &str,
) -> CapabilityReport {
    let target = dialect_manifest(target_dialect);

    let requested: Vec<Capability> = work_order
        .requirements
        .required
        .iter()
        .map(|r| r.capability.clone())
        .collect();

    let entries: Vec<CapabilityReportEntry> = requested
        .into_iter()
        .map(|cap| {
            let support = target
                .get(&cap)
                .cloned()
                .unwrap_or(DialectSupportLevel::Unsupported {
                    reason: format!(
                        "capability not recognized by dialect '{}'",
                        target_dialect
                    ),
                });
            CapabilityReportEntry {
                capability: cap,
                support,
            }
        })
        .collect();

    CapabilityReport {
        source_dialect: source_dialect.to_string(),
        target_dialect: target_dialect.to_string(),
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_rank_ordering() {
        assert!(support_rank(&SupportLevel::Native) > support_rank(&SupportLevel::Emulated));
        assert!(
            support_rank(&SupportLevel::Emulated)
                > support_rank(&SupportLevel::Restricted {
                    reason: String::new()
                })
        );
        assert!(
            support_rank(&SupportLevel::Restricted {
                reason: String::new()
            }) > support_rank(&SupportLevel::Unsupported)
        );
    }
}
