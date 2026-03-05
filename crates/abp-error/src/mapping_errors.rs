#![allow(dead_code, unused_imports)]
//! Typed errors for cross-SDK mapping failures.
//!
//! Each variant carries structured context describing what went wrong during
//! dialect translation, capability negotiation, or fidelity approximation.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{AbpError, ErrorCode};

// ---------------------------------------------------------------------------
// MappingError
// ---------------------------------------------------------------------------

/// Typed error for cross-SDK mapping / negotiation / execution failures.
///
/// Every variant has a stable `code()` returning a string like `"ABP-MAP-001"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MappingError {
    /// The target dialect does not support the requested feature at all.
    FeatureUnsupported {
        /// The feature that is unsupported (e.g. `"tool_use"`, `"vision"`).
        feature: String,
        /// Source SDK dialect identifier.
        source_dialect: String,
        /// Target SDK dialect identifier.
        target_dialect: String,
    },
    /// An emulation attempt for an unsupported feature failed.
    EmulationFailed {
        /// The feature being emulated.
        feature: String,
        /// Why emulation failed.
        reason: String,
    },
    /// Translation succeeded but with measurable fidelity loss.
    FidelityLoss {
        /// The field or element that lost fidelity.
        field: String,
        /// Original value (stringified for portability).
        original: String,
        /// The approximation used in the target dialect.
        approximation: String,
    },
    /// A field maps to multiple candidates and the system cannot choose.
    AmbiguousMapping {
        /// The field that has ambiguous mapping.
        field: String,
        /// The set of equally valid target candidates.
        candidates: Vec<String>,
    },
    /// Capability negotiation between source and target failed entirely.
    NegotiationFailed {
        /// Human-readable explanation of why negotiation failed.
        reason: String,
    },
}

impl MappingError {
    /// Stable error code string (e.g. `"ABP-MAP-001"`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::FeatureUnsupported { .. } => "ABP-MAP-001",
            Self::EmulationFailed { .. } => "ABP-MAP-002",
            Self::FidelityLoss { .. } => "ABP-MAP-003",
            Self::AmbiguousMapping { .. } => "ABP-MAP-004",
            Self::NegotiationFailed { .. } => "ABP-MAP-005",
        }
    }

    /// Convert into a unified [`AbpError`] with structured context.
    pub fn into_abp_error(self) -> AbpError {
        let code = match &self {
            Self::FeatureUnsupported { .. } => ErrorCode::MappingUnsupportedCapability,
            Self::EmulationFailed { .. } => ErrorCode::CapabilityEmulationFailed,
            Self::FidelityLoss { .. } => ErrorCode::MappingLossyConversion,
            Self::AmbiguousMapping { .. } => ErrorCode::MappingDialectMismatch,
            Self::NegotiationFailed { .. } => ErrorCode::MappingDialectMismatch,
        };

        AbpError::new(code, self.to_string()).with_context("mapping_code", self.code())
    }
}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FeatureUnsupported {
                feature,
                source_dialect,
                target_dialect,
            } => write!(
                f,
                "[{}] feature '{}' unsupported: {} -> {}",
                self.code(),
                feature,
                source_dialect,
                target_dialect
            ),
            Self::EmulationFailed { feature, reason } => {
                write!(
                    f,
                    "[{}] emulation of '{}' failed: {}",
                    self.code(),
                    feature,
                    reason
                )
            }
            Self::FidelityLoss {
                field,
                original,
                approximation,
            } => write!(
                f,
                "[{}] fidelity loss on '{}': '{}' approximated as '{}'",
                self.code(),
                field,
                original,
                approximation
            ),
            Self::AmbiguousMapping { field, candidates } => write!(
                f,
                "[{}] ambiguous mapping for '{}': candidates [{}]",
                self.code(),
                field,
                candidates.join(", ")
            ),
            Self::NegotiationFailed { reason } => {
                write!(f, "[{}] negotiation failed: {}", self.code(), reason)
            }
        }
    }
}

impl std::error::Error for MappingError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_unsupported_code() {
        let e = MappingError::FeatureUnsupported {
            feature: "vision".into(),
            source_dialect: "openai".into(),
            target_dialect: "gemini".into(),
        };
        assert_eq!(e.code(), "ABP-MAP-001");
    }

    #[test]
    fn emulation_failed_code() {
        let e = MappingError::EmulationFailed {
            feature: "tool_use".into(),
            reason: "no adapter".into(),
        };
        assert_eq!(e.code(), "ABP-MAP-002");
    }

    #[test]
    fn fidelity_loss_code() {
        let e = MappingError::FidelityLoss {
            field: "temperature".into(),
            original: "0.73".into(),
            approximation: "0.7".into(),
        };
        assert_eq!(e.code(), "ABP-MAP-003");
    }

    #[test]
    fn ambiguous_mapping_code() {
        let e = MappingError::AmbiguousMapping {
            field: "role".into(),
            candidates: vec!["user".into(), "human".into()],
        };
        assert_eq!(e.code(), "ABP-MAP-004");
    }

    #[test]
    fn negotiation_failed_code() {
        let e = MappingError::NegotiationFailed {
            reason: "incompatible".into(),
        };
        assert_eq!(e.code(), "ABP-MAP-005");
    }

    #[test]
    fn display_feature_unsupported() {
        let e = MappingError::FeatureUnsupported {
            feature: "streaming".into(),
            source_dialect: "anthropic".into(),
            target_dialect: "codex".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-MAP-001"));
        assert!(s.contains("streaming"));
        assert!(s.contains("anthropic"));
        assert!(s.contains("codex"));
    }

    #[test]
    fn display_emulation_failed() {
        let e = MappingError::EmulationFailed {
            feature: "code_exec".into(),
            reason: "sandbox unavailable".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-MAP-002"));
        assert!(s.contains("code_exec"));
        assert!(s.contains("sandbox unavailable"));
    }

    #[test]
    fn display_fidelity_loss() {
        let e = MappingError::FidelityLoss {
            field: "max_tokens".into(),
            original: "4096".into(),
            approximation: "4000".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-MAP-003"));
        assert!(s.contains("4096"));
        assert!(s.contains("4000"));
    }

    #[test]
    fn display_ambiguous_mapping() {
        let e = MappingError::AmbiguousMapping {
            field: "stop_sequence".into(),
            candidates: vec!["stop".into(), "end_turn".into(), "eos".into()],
        };
        let s = e.to_string();
        assert!(s.contains("ABP-MAP-004"));
        assert!(s.contains("stop, end_turn, eos"));
    }

    #[test]
    fn display_negotiation_failed() {
        let e = MappingError::NegotiationFailed {
            reason: "no common capability set".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-MAP-005"));
        assert!(s.contains("no common capability set"));
    }

    #[test]
    fn serde_roundtrip_feature_unsupported() {
        let e = MappingError::FeatureUnsupported {
            feature: "vision".into(),
            source_dialect: "openai".into(),
            target_dialect: "gemini".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_emulation_failed() {
        let e = MappingError::EmulationFailed {
            feature: "tool_use".into(),
            reason: "not supported".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_fidelity_loss() {
        let e = MappingError::FidelityLoss {
            field: "temperature".into(),
            original: "0.73".into(),
            approximation: "0.7".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_ambiguous_mapping() {
        let e = MappingError::AmbiguousMapping {
            field: "role".into(),
            candidates: vec!["user".into(), "human".into()],
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_negotiation_failed() {
        let e = MappingError::NegotiationFailed {
            reason: "incompatible".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn into_abp_error_feature_unsupported() {
        let e = MappingError::FeatureUnsupported {
            feature: "vision".into(),
            source_dialect: "openai".into(),
            target_dialect: "gemini".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::MappingUnsupportedCapability);
        assert_eq!(
            abp.context["mapping_code"],
            serde_json::json!("ABP-MAP-001")
        );
    }

    #[test]
    fn into_abp_error_emulation_failed() {
        let e = MappingError::EmulationFailed {
            feature: "tool_use".into(),
            reason: "fail".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::CapabilityEmulationFailed);
    }

    #[test]
    fn into_abp_error_fidelity_loss() {
        let e = MappingError::FidelityLoss {
            field: "temp".into(),
            original: "0.7".into(),
            approximation: "0.5".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::MappingLossyConversion);
    }

    #[test]
    fn into_abp_error_ambiguous() {
        let e = MappingError::AmbiguousMapping {
            field: "role".into(),
            candidates: vec!["a".into(), "b".into()],
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::MappingDialectMismatch);
    }

    #[test]
    fn into_abp_error_negotiation() {
        let e = MappingError::NegotiationFailed {
            reason: "nope".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::MappingDialectMismatch);
    }

    #[test]
    fn std_error_impl() {
        let e = MappingError::NegotiationFailed {
            reason: "test".into(),
        };
        let dyn_err: &dyn std::error::Error = &e;
        assert!(dyn_err.source().is_none());
    }

    #[test]
    fn all_codes_unique() {
        let codes = [
            MappingError::FeatureUnsupported {
                feature: String::new(),
                source_dialect: String::new(),
                target_dialect: String::new(),
            }
            .code(),
            MappingError::EmulationFailed {
                feature: String::new(),
                reason: String::new(),
            }
            .code(),
            MappingError::FidelityLoss {
                field: String::new(),
                original: String::new(),
                approximation: String::new(),
            }
            .code(),
            MappingError::AmbiguousMapping {
                field: String::new(),
                candidates: vec![],
            }
            .code(),
            MappingError::NegotiationFailed {
                reason: String::new(),
            }
            .code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len(), "duplicate error codes found");
    }
}
