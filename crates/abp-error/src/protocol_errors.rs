#![allow(dead_code, unused_imports)]
//! Enhanced protocol errors for JSONL sidecar communication failures.
//!
//! Covers handshake, versioning, envelope parsing, stream interruption,
//! timeouts, and sidecar process crashes.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{AbpError, ErrorCode};

// ---------------------------------------------------------------------------
// ProtocolError
// ---------------------------------------------------------------------------

/// Typed protocol-layer error.
///
/// Every variant has a stable `code()` like `"ABP-PROTO-001"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolError {
    /// The sidecar did not complete the hello handshake.
    HandshakeFailed {
        /// What went wrong during the handshake.
        reason: String,
    },
    /// The contract version advertised by the sidecar does not match.
    VersionMismatch {
        /// Version the host expected.
        expected: String,
        /// Version the sidecar advertised.
        actual: String,
    },
    /// An envelope could not be parsed or is missing required fields.
    EnvelopeMalformed {
        /// Raw line that failed to parse.
        raw_line: String,
        /// Parser error detail.
        parse_error: String,
    },
    /// The event stream was interrupted before the sidecar sent `final`.
    StreamInterrupted {
        /// How many events were received before interruption.
        events_received: u64,
        /// Human-readable reason for the interruption.
        reason: String,
    },
    /// A protocol-level timeout expired (distinct from backend timeout).
    TimeoutExpired {
        /// What operation timed out.
        operation: String,
        /// Configured timeout in milliseconds.
        timeout_ms: u64,
    },
    /// The sidecar process crashed or exited unexpectedly.
    SidecarCrashed {
        /// Process exit code, if available.
        exit_code: Option<i32>,
        /// Last stderr output captured before crash.
        stderr_tail: String,
    },
}

impl ProtocolError {
    /// Stable error code string (e.g. `"ABP-PROTO-001"`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::HandshakeFailed { .. } => "ABP-PROTO-001",
            Self::VersionMismatch { .. } => "ABP-PROTO-002",
            Self::EnvelopeMalformed { .. } => "ABP-PROTO-003",
            Self::StreamInterrupted { .. } => "ABP-PROTO-004",
            Self::TimeoutExpired { .. } => "ABP-PROTO-005",
            Self::SidecarCrashed { .. } => "ABP-PROTO-006",
        }
    }

    /// Convert into a unified [`AbpError`] with structured context.
    pub fn into_abp_error(self) -> AbpError {
        let code = match &self {
            Self::HandshakeFailed { .. } => ErrorCode::ProtocolHandshakeFailed,
            Self::VersionMismatch { .. } => ErrorCode::ProtocolVersionMismatch,
            Self::EnvelopeMalformed { .. } => ErrorCode::ProtocolInvalidEnvelope,
            Self::StreamInterrupted { .. } => ErrorCode::ProtocolUnexpectedMessage,
            Self::TimeoutExpired { .. } => ErrorCode::BackendTimeout,
            Self::SidecarCrashed { .. } => ErrorCode::BackendCrashed,
        };

        AbpError::new(code, self.to_string()).with_context("protocol_code", self.code())
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HandshakeFailed { reason } => {
                write!(f, "[{}] handshake failed: {}", self.code(), reason)
            }
            Self::VersionMismatch { expected, actual } => write!(
                f,
                "[{}] version mismatch: expected '{}', got '{}'",
                self.code(),
                expected,
                actual
            ),
            Self::EnvelopeMalformed {
                raw_line,
                parse_error,
            } => write!(
                f,
                "[{}] malformed envelope: {} (line: '{}')",
                self.code(),
                parse_error,
                raw_line
            ),
            Self::StreamInterrupted {
                events_received,
                reason,
            } => write!(
                f,
                "[{}] stream interrupted after {} events: {}",
                self.code(),
                events_received,
                reason
            ),
            Self::TimeoutExpired {
                operation,
                timeout_ms,
            } => write!(
                f,
                "[{}] timeout expired for '{}' after {}ms",
                self.code(),
                operation,
                timeout_ms
            ),
            Self::SidecarCrashed {
                exit_code,
                stderr_tail,
            } => {
                let ec = exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into());
                write!(
                    f,
                    "[{}] sidecar crashed (exit {}): {}",
                    self.code(),
                    ec,
                    stderr_tail
                )
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_failed_code() {
        let e = ProtocolError::HandshakeFailed {
            reason: "no hello".into(),
        };
        assert_eq!(e.code(), "ABP-PROTO-001");
    }

    #[test]
    fn version_mismatch_code() {
        let e = ProtocolError::VersionMismatch {
            expected: "abp/v0.1".into(),
            actual: "abp/v0.2".into(),
        };
        assert_eq!(e.code(), "ABP-PROTO-002");
    }

    #[test]
    fn envelope_malformed_code() {
        let e = ProtocolError::EnvelopeMalformed {
            raw_line: "{bad".into(),
            parse_error: "expected value".into(),
        };
        assert_eq!(e.code(), "ABP-PROTO-003");
    }

    #[test]
    fn stream_interrupted_code() {
        let e = ProtocolError::StreamInterrupted {
            events_received: 5,
            reason: "EOF".into(),
        };
        assert_eq!(e.code(), "ABP-PROTO-004");
    }

    #[test]
    fn timeout_expired_code() {
        let e = ProtocolError::TimeoutExpired {
            operation: "hello".into(),
            timeout_ms: 5000,
        };
        assert_eq!(e.code(), "ABP-PROTO-005");
    }

    #[test]
    fn sidecar_crashed_code() {
        let e = ProtocolError::SidecarCrashed {
            exit_code: Some(1),
            stderr_tail: "segfault".into(),
        };
        assert_eq!(e.code(), "ABP-PROTO-006");
    }

    #[test]
    fn display_handshake_failed() {
        let e = ProtocolError::HandshakeFailed {
            reason: "timeout waiting for hello".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-PROTO-001"));
        assert!(s.contains("timeout waiting for hello"));
    }

    #[test]
    fn display_version_mismatch() {
        let e = ProtocolError::VersionMismatch {
            expected: "abp/v0.1".into(),
            actual: "abp/v0.2".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-PROTO-002"));
        assert!(s.contains("abp/v0.1"));
        assert!(s.contains("abp/v0.2"));
    }

    #[test]
    fn display_envelope_malformed() {
        let e = ProtocolError::EnvelopeMalformed {
            raw_line: "{bad json".into(),
            parse_error: "expected value at line 1".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-PROTO-003"));
        assert!(s.contains("{bad json"));
    }

    #[test]
    fn display_stream_interrupted() {
        let e = ProtocolError::StreamInterrupted {
            events_received: 42,
            reason: "connection reset".into(),
        };
        let s = e.to_string();
        assert!(s.contains("ABP-PROTO-004"));
        assert!(s.contains("42"));
    }

    #[test]
    fn display_sidecar_crashed_with_exit_code() {
        let e = ProtocolError::SidecarCrashed {
            exit_code: Some(137),
            stderr_tail: "killed".into(),
        };
        let s = e.to_string();
        assert!(s.contains("137"));
        assert!(s.contains("killed"));
    }

    #[test]
    fn display_sidecar_crashed_unknown_exit() {
        let e = ProtocolError::SidecarCrashed {
            exit_code: None,
            stderr_tail: "signal".into(),
        };
        let s = e.to_string();
        assert!(s.contains("unknown"));
    }

    #[test]
    fn serde_roundtrip_handshake() {
        let e = ProtocolError::HandshakeFailed {
            reason: "test".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ProtocolError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_version_mismatch() {
        let e = ProtocolError::VersionMismatch {
            expected: "v0.1".into(),
            actual: "v0.2".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ProtocolError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_envelope_malformed() {
        let e = ProtocolError::EnvelopeMalformed {
            raw_line: "bad".into(),
            parse_error: "err".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ProtocolError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_stream_interrupted() {
        let e = ProtocolError::StreamInterrupted {
            events_received: 10,
            reason: "eof".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ProtocolError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_timeout_expired() {
        let e = ProtocolError::TimeoutExpired {
            operation: "run".into(),
            timeout_ms: 30000,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ProtocolError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_sidecar_crashed() {
        let e = ProtocolError::SidecarCrashed {
            exit_code: Some(1),
            stderr_tail: "boom".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ProtocolError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn into_abp_error_handshake() {
        let e = ProtocolError::HandshakeFailed {
            reason: "no hello".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::ProtocolHandshakeFailed);
        assert_eq!(
            abp.context["protocol_code"],
            serde_json::json!("ABP-PROTO-001")
        );
    }

    #[test]
    fn into_abp_error_version() {
        let e = ProtocolError::VersionMismatch {
            expected: "v0.1".into(),
            actual: "v0.2".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::ProtocolVersionMismatch);
    }

    #[test]
    fn into_abp_error_envelope() {
        let e = ProtocolError::EnvelopeMalformed {
            raw_line: "x".into(),
            parse_error: "y".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
    }

    #[test]
    fn into_abp_error_stream() {
        let e = ProtocolError::StreamInterrupted {
            events_received: 0,
            reason: "eof".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::ProtocolUnexpectedMessage);
    }

    #[test]
    fn into_abp_error_timeout() {
        let e = ProtocolError::TimeoutExpired {
            operation: "hello".into(),
            timeout_ms: 5000,
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendTimeout);
    }

    #[test]
    fn into_abp_error_crash() {
        let e = ProtocolError::SidecarCrashed {
            exit_code: Some(1),
            stderr_tail: "err".into(),
        };
        let abp = e.into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendCrashed);
    }

    #[test]
    fn std_error_impl() {
        let e = ProtocolError::HandshakeFailed {
            reason: "test".into(),
        };
        let dyn_err: &dyn std::error::Error = &e;
        assert!(dyn_err.source().is_none());
    }

    #[test]
    fn all_codes_unique() {
        let codes = [
            ProtocolError::HandshakeFailed {
                reason: String::new(),
            }
            .code(),
            ProtocolError::VersionMismatch {
                expected: String::new(),
                actual: String::new(),
            }
            .code(),
            ProtocolError::EnvelopeMalformed {
                raw_line: String::new(),
                parse_error: String::new(),
            }
            .code(),
            ProtocolError::StreamInterrupted {
                events_received: 0,
                reason: String::new(),
            }
            .code(),
            ProtocolError::TimeoutExpired {
                operation: String::new(),
                timeout_ms: 0,
            }
            .code(),
            ProtocolError::SidecarCrashed {
                exit_code: None,
                stderr_tail: String::new(),
            }
            .code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len(), "duplicate error codes found");
    }
}
