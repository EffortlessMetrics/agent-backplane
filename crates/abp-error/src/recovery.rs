#![allow(dead_code, unused_imports)]
//! Recovery strategies for each error type.
//!
//! Given any typed error ([`MappingError`], [`ProtocolError`], [`VendorApiError`]),
//! this module recommends a [`RecoveryStrategy`]: Retry, Fallback, Degrade, or Abort.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

use crate::mapping_errors::MappingError;
use crate::protocol_errors::ProtocolError;
use crate::vendor_errors::VendorApiError;

// ---------------------------------------------------------------------------
// RecoveryStrategy
// ---------------------------------------------------------------------------

/// The recommended recovery action after an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RecoveryStrategy {
    /// Retry the same operation after a delay.
    Retry {
        /// Suggested delay before retrying.
        delay_ms: u64,
        /// Maximum number of retries recommended.
        max_retries: u32,
    },
    /// Fall back to an alternative backend or dialect.
    Fallback {
        /// Human-readable description of the fallback option.
        suggestion: String,
    },
    /// Continue with degraded functionality (e.g. lossy mapping).
    Degrade {
        /// What capability or fidelity is being sacrificed.
        degradation: String,
    },
    /// Abort — the error is not recoverable without human intervention.
    Abort {
        /// Why recovery is impossible.
        reason: String,
    },
}

impl RecoveryStrategy {
    /// Stable code for the strategy type.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Retry { .. } => "ABP-REC-RETRY",
            Self::Fallback { .. } => "ABP-REC-FALLBACK",
            Self::Degrade { .. } => "ABP-REC-DEGRADE",
            Self::Abort { .. } => "ABP-REC-ABORT",
        }
    }

    /// Whether this strategy suggests the operation may eventually succeed.
    pub fn is_recoverable(&self) -> bool {
        !matches!(self, Self::Abort { .. })
    }
}

impl fmt::Display for RecoveryStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retry {
                delay_ms,
                max_retries,
            } => write!(
                f,
                "[{}] retry after {}ms (max {} attempts)",
                self.code(),
                delay_ms,
                max_retries
            ),
            Self::Fallback { suggestion } => {
                write!(f, "[{}] fallback: {}", self.code(), suggestion)
            }
            Self::Degrade { degradation } => {
                write!(f, "[{}] degrade: {}", self.code(), degradation)
            }
            Self::Abort { reason } => {
                write!(f, "[{}] abort: {}", self.code(), reason)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Recommend recovery for each error type
// ---------------------------------------------------------------------------

/// Recommend a recovery strategy for a [`MappingError`].
pub fn recover_mapping(err: &MappingError) -> RecoveryStrategy {
    match err {
        MappingError::FeatureUnsupported { .. } => RecoveryStrategy::Fallback {
            suggestion: "use a backend that supports the required feature".into(),
        },
        MappingError::EmulationFailed { .. } => RecoveryStrategy::Fallback {
            suggestion: "use a backend with native support for the feature".into(),
        },
        MappingError::FidelityLoss { .. } => RecoveryStrategy::Degrade {
            degradation: "proceed with approximated value".into(),
        },
        MappingError::AmbiguousMapping { .. } => RecoveryStrategy::Abort {
            reason: "ambiguous mapping requires explicit configuration".into(),
        },
        MappingError::NegotiationFailed { .. } => RecoveryStrategy::Abort {
            reason: "no compatible capability set found".into(),
        },
    }
}

/// Recommend a recovery strategy for a [`ProtocolError`].
pub fn recover_protocol(err: &ProtocolError) -> RecoveryStrategy {
    match err {
        ProtocolError::HandshakeFailed { .. } => RecoveryStrategy::Retry {
            delay_ms: 1000,
            max_retries: 3,
        },
        ProtocolError::VersionMismatch { .. } => RecoveryStrategy::Abort {
            reason: "contract version mismatch requires sidecar update".into(),
        },
        ProtocolError::EnvelopeMalformed { .. } => RecoveryStrategy::Abort {
            reason: "malformed envelope indicates a sidecar bug".into(),
        },
        ProtocolError::StreamInterrupted { .. } => RecoveryStrategy::Retry {
            delay_ms: 2000,
            max_retries: 2,
        },
        ProtocolError::TimeoutExpired { .. } => RecoveryStrategy::Retry {
            delay_ms: 5000,
            max_retries: 2,
        },
        ProtocolError::SidecarCrashed { .. } => RecoveryStrategy::Retry {
            delay_ms: 3000,
            max_retries: 1,
        },
    }
}

/// Recommend a recovery strategy for a [`VendorApiError`].
pub fn recover_vendor(err: &VendorApiError) -> RecoveryStrategy {
    let d = err.detail();
    match d.status_code {
        429 => {
            let delay = d.retry_after_secs.unwrap_or(30) * 1000;
            RecoveryStrategy::Retry {
                delay_ms: delay,
                max_retries: 3,
            }
        }
        500 | 502 | 503 => RecoveryStrategy::Retry {
            delay_ms: d.retry_after_secs.map(|s| s * 1000).unwrap_or(5000),
            max_retries: 3,
        },
        504 | 408 => RecoveryStrategy::Retry {
            delay_ms: 10000,
            max_retries: 2,
        },
        401 => RecoveryStrategy::Abort {
            reason: "authentication failed — check API key".into(),
        },
        403 => RecoveryStrategy::Abort {
            reason: "permission denied by vendor".into(),
        },
        404 => RecoveryStrategy::Fallback {
            suggestion: "model not found — try an alternative model".into(),
        },
        _ => RecoveryStrategy::Abort {
            reason: format!("unexpected HTTP status {}", d.status_code),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendor_errors::VendorErrorDetail;

    // -- RecoveryStrategy basics --------------------------------------

    #[test]
    fn retry_code() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 1000,
            max_retries: 3,
        };
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn fallback_code() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "x".into(),
        };
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn degrade_code() {
        let s = RecoveryStrategy::Degrade {
            degradation: "x".into(),
        };
        assert_eq!(s.code(), "ABP-REC-DEGRADE");
    }

    #[test]
    fn abort_code() {
        let s = RecoveryStrategy::Abort {
            reason: "x".into(),
        };
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn retry_is_recoverable() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 100,
            max_retries: 1,
        };
        assert!(s.is_recoverable());
    }

    #[test]
    fn fallback_is_recoverable() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "x".into(),
        };
        assert!(s.is_recoverable());
    }

    #[test]
    fn degrade_is_recoverable() {
        let s = RecoveryStrategy::Degrade {
            degradation: "x".into(),
        };
        assert!(s.is_recoverable());
    }

    #[test]
    fn abort_is_not_recoverable() {
        let s = RecoveryStrategy::Abort {
            reason: "x".into(),
        };
        assert!(!s.is_recoverable());
    }

    #[test]
    fn display_retry() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 5000,
            max_retries: 3,
        };
        let d = s.to_string();
        assert!(d.contains("ABP-REC-RETRY"));
        assert!(d.contains("5000ms"));
        assert!(d.contains("3 attempts"));
    }

    #[test]
    fn display_fallback() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "use openai".into(),
        };
        assert!(s.to_string().contains("use openai"));
    }

    #[test]
    fn display_degrade() {
        let s = RecoveryStrategy::Degrade {
            degradation: "lose precision".into(),
        };
        assert!(s.to_string().contains("lose precision"));
    }

    #[test]
    fn display_abort() {
        let s = RecoveryStrategy::Abort {
            reason: "fatal".into(),
        };
        assert!(s.to_string().contains("fatal"));
    }

    #[test]
    fn serde_roundtrip_retry() {
        let s = RecoveryStrategy::Retry {
            delay_ms: 1000,
            max_retries: 2,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn serde_roundtrip_fallback() {
        let s = RecoveryStrategy::Fallback {
            suggestion: "alt".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn serde_roundtrip_degrade() {
        let s = RecoveryStrategy::Degrade {
            degradation: "lossy".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn serde_roundtrip_abort() {
        let s = RecoveryStrategy::Abort {
            reason: "done".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecoveryStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    // -- recover_mapping() --------------------------------------------

    #[test]
    fn mapping_feature_unsupported_fallback() {
        let e = MappingError::FeatureUnsupported {
            feature: "vision".into(),
            source_dialect: "openai".into(),
            target_dialect: "gemini".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn mapping_emulation_failed_fallback() {
        let e = MappingError::EmulationFailed {
            feature: "tool_use".into(),
            reason: "no adapter".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn mapping_fidelity_loss_degrade() {
        let e = MappingError::FidelityLoss {
            field: "temp".into(),
            original: "0.7".into(),
            approximation: "0.5".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-DEGRADE");
    }

    #[test]
    fn mapping_ambiguous_abort() {
        let e = MappingError::AmbiguousMapping {
            field: "role".into(),
            candidates: vec!["a".into(), "b".into()],
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
        assert!(!s.is_recoverable());
    }

    #[test]
    fn mapping_negotiation_abort() {
        let e = MappingError::NegotiationFailed {
            reason: "incompatible".into(),
        };
        let s = recover_mapping(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    // -- recover_protocol() -------------------------------------------

    #[test]
    fn protocol_handshake_retry() {
        let e = ProtocolError::HandshakeFailed {
            reason: "timeout".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn protocol_version_abort() {
        let e = ProtocolError::VersionMismatch {
            expected: "v0.1".into(),
            actual: "v0.2".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn protocol_envelope_abort() {
        let e = ProtocolError::EnvelopeMalformed {
            raw_line: "bad".into(),
            parse_error: "err".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn protocol_stream_retry() {
        let e = ProtocolError::StreamInterrupted {
            events_received: 5,
            reason: "eof".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn protocol_timeout_retry() {
        let e = ProtocolError::TimeoutExpired {
            operation: "hello".into(),
            timeout_ms: 5000,
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn protocol_crash_retry() {
        let e = ProtocolError::SidecarCrashed {
            exit_code: Some(1),
            stderr_tail: "err".into(),
        };
        let s = recover_protocol(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    // -- recover_vendor() ---------------------------------------------

    #[test]
    fn vendor_429_retry() {
        let e = VendorApiError::OpenAi(VendorErrorDetail::new(429, "rate limited"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
        if let RecoveryStrategy::Retry { delay_ms, .. } = &s {
            assert_eq!(*delay_ms, 30000); // default 30s
        } else {
            panic!("expected Retry");
        }
    }

    #[test]
    fn vendor_429_with_retry_after() {
        let e = VendorApiError::OpenAi(
            VendorErrorDetail::new(429, "limited").with_retry_after(10),
        );
        let s = recover_vendor(&e);
        if let RecoveryStrategy::Retry { delay_ms, .. } = &s {
            assert_eq!(*delay_ms, 10000);
        } else {
            panic!("expected Retry");
        }
    }

    #[test]
    fn vendor_503_retry() {
        let e = VendorApiError::Gemini(VendorErrorDetail::new(503, "overloaded"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn vendor_504_retry() {
        let e = VendorApiError::Claude(VendorErrorDetail::new(504, "gateway timeout"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-RETRY");
    }

    #[test]
    fn vendor_401_abort() {
        let e = VendorApiError::Claude(VendorErrorDetail::new(401, "bad key"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
        assert!(!s.is_recoverable());
    }

    #[test]
    fn vendor_403_abort() {
        let e = VendorApiError::Copilot(VendorErrorDetail::new(403, "forbidden"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }

    #[test]
    fn vendor_404_fallback() {
        let e = VendorApiError::Codex(VendorErrorDetail::new(404, "not found"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-FALLBACK");
    }

    #[test]
    fn vendor_unknown_status_abort() {
        let e = VendorApiError::Kimi(VendorErrorDetail::new(418, "teapot"));
        let s = recover_vendor(&e);
        assert_eq!(s.code(), "ABP-REC-ABORT");
    }
}
