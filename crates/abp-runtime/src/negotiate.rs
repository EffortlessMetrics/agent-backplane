// SPDX-License-Identifier: MIT OR Apache-2.0
//! Combined capability negotiation result for the runtime pipeline.
//!
//! [`NegotiationResult`] merges backend-level capability negotiation
//! (from `abp-capability`) with runtime-level emulation (from `abp-emulation`)
//! into a single summary recorded in the receipt.

use abp_core::Capability;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Entry for a capability fulfilled via emulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmulatedCapability {
    /// The capability being emulated.
    pub capability: Capability,
    /// Where the emulation is applied: `"backend"` or `"runtime"`.
    pub source: String,
    /// Human-readable description of the emulation approach.
    pub description: String,
}

/// Entry for a capability that cannot be satisfied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingCapability {
    /// The capability that is missing.
    pub capability: Capability,
    /// Why it cannot be provided.
    pub reason: String,
}

/// Combined result of pre-execution capability negotiation.
///
/// Produced during the runtime's preflight phase, this merges the backend's
/// capability manifest check with any runtime-level emulation into a single
/// picture of how each required capability will be fulfilled.
///
/// Recorded in the receipt's `usage_raw` under the `"negotiation_result"` key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegotiationResult {
    /// Capabilities natively supported by the backend.
    pub native: Vec<Capability>,
    /// Capabilities provided via emulation (backend or runtime).
    pub emulated: Vec<EmulatedCapability>,
    /// Capabilities that cannot be satisfied.
    pub missing: Vec<MissingCapability>,
}

impl NegotiationResult {
    /// Create a result where all capabilities are natively supported.
    #[must_use]
    pub fn all_native(native: Vec<Capability>) -> Self {
        Self {
            native,
            emulated: vec![],
            missing: vec![],
        }
    }

    /// Returns `true` when all required capabilities can be fulfilled.
    #[must_use]
    pub fn is_viable(&self) -> bool {
        self.missing.is_empty()
    }

    /// Total number of capabilities evaluated.
    #[must_use]
    pub fn total(&self) -> usize {
        self.native.len() + self.emulated.len() + self.missing.len()
    }

    /// Build from a capability negotiation result and optional emulation report.
    #[must_use]
    pub fn from_negotiation(
        cap_result: &abp_capability::NegotiationResult,
        emu_report: Option<&abp_emulation::EmulationReport>,
    ) -> Self {
        let native = cap_result.native.clone();

        let mut emulated: Vec<EmulatedCapability> = cap_result
            .emulated
            .iter()
            .map(|(cap, strategy)| EmulatedCapability {
                capability: cap.clone(),
                source: "backend".into(),
                description: format!("{strategy}"),
            })
            .collect();

        // Collect capabilities emulated by the runtime engine.
        let runtime_emulated_caps: std::collections::BTreeSet<Capability> = emu_report
            .map(|r| r.applied.iter().map(|e| e.capability.clone()).collect())
            .unwrap_or_default();

        if let Some(report) = emu_report {
            for entry in &report.applied {
                if !emulated.iter().any(|e| e.capability == entry.capability) {
                    emulated.push(EmulatedCapability {
                        capability: entry.capability.clone(),
                        source: "runtime".into(),
                        description: match &entry.strategy {
                            abp_emulation::EmulationStrategy::SystemPromptInjection { .. } => {
                                "system prompt injection".into()
                            }
                            abp_emulation::EmulationStrategy::PostProcessing { detail } => {
                                format!("post-processing: {detail}")
                            }
                            abp_emulation::EmulationStrategy::Disabled { reason } => {
                                format!("disabled: {reason}")
                            }
                        },
                    });
                }
            }
        }

        // Capabilities unsupported by the backend and not emulated by runtime.
        let missing: Vec<MissingCapability> = cap_result
            .unsupported
            .iter()
            .filter(|(cap, _)| !runtime_emulated_caps.contains(cap))
            .map(|(cap, reason)| MissingCapability {
                capability: cap.clone(),
                reason: reason.clone(),
            })
            .collect();

        // Unsupported caps rescued by runtime emulation become emulated entries.
        for (cap, _reason) in &cap_result.unsupported {
            if runtime_emulated_caps.contains(cap)
                && !emulated.iter().any(|e| &e.capability == cap)
            {
                emulated.push(EmulatedCapability {
                    capability: cap.clone(),
                    source: "runtime".into(),
                    description: "emulated by runtime".into(),
                });
            }
        }

        Self {
            native,
            emulated,
            missing,
        }
    }
}

impl fmt::Display for NegotiationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} native, {} emulated, {} missing",
            self.native.len(),
            self.emulated.len(),
            self.missing.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_capability::EmulationStrategy as CapStrategy;
    use abp_emulation::{EmulationEntry, EmulationReport, EmulationStrategy as EmuStrategy};

    #[test]
    fn all_native_is_viable() {
        let r = NegotiationResult::all_native(vec![Capability::Streaming]);
        assert!(r.is_viable());
        assert_eq!(r.total(), 1);
        assert!(r.missing.is_empty());
    }

    #[test]
    fn from_negotiation_all_native() {
        let cap = abp_capability::NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        let r = NegotiationResult::from_negotiation(&cap, None);
        assert!(r.is_viable());
        assert_eq!(r.native.len(), 1);
        assert!(r.emulated.is_empty());
    }

    #[test]
    fn from_negotiation_with_backend_emulation() {
        let cap = abp_capability::NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![(Capability::ToolRead, CapStrategy::ClientSide)],
            unsupported: vec![],
        };
        let r = NegotiationResult::from_negotiation(&cap, None);
        assert!(r.is_viable());
        assert_eq!(r.emulated.len(), 1);
        assert_eq!(r.emulated[0].source, "backend");
    }

    #[test]
    fn from_negotiation_with_runtime_emulation() {
        let cap = abp_capability::NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![(Capability::ExtendedThinking, "not declared".into())],
        };
        let emu = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmuStrategy::SystemPromptInjection {
                    prompt: "think step by step".into(),
                },
            }],
            warnings: vec![],
        };
        let r = NegotiationResult::from_negotiation(&cap, Some(&emu));
        assert!(r.is_viable());
        assert!(r.missing.is_empty());
        assert_eq!(r.emulated.len(), 1);
        assert_eq!(r.emulated[0].source, "runtime");
    }

    #[test]
    fn from_negotiation_with_missing() {
        let cap = abp_capability::NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![(Capability::Vision, "not available".into())],
        };
        let r = NegotiationResult::from_negotiation(&cap, None);
        assert!(!r.is_viable());
        assert_eq!(r.missing.len(), 1);
        assert_eq!(r.missing[0].capability, Capability::Vision);
    }

    #[test]
    fn display_format() {
        let r = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![EmulatedCapability {
                capability: Capability::ToolRead,
                source: "backend".into(),
                description: "test".into(),
            }],
            missing: vec![],
        };
        assert_eq!(format!("{r}"), "1 native, 1 emulated, 0 missing");
    }

    #[test]
    fn serde_roundtrip() {
        let r = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![EmulatedCapability {
                capability: Capability::ToolRead,
                source: "backend".into(),
                description: "polyfill".into(),
            }],
            missing: vec![MissingCapability {
                capability: Capability::Vision,
                reason: "not available".into(),
            }],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: NegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
