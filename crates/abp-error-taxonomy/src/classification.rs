//! Error classification, severity levels, and recovery suggestions.
//!
//! This module provides a structured way to classify [`ErrorCode`]s by
//! operational severity, assign a high-level category, and suggest a
//! recovery strategy.

use crate::ErrorCode;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ErrorSeverity
// ---------------------------------------------------------------------------

/// How severe an error is from an operational perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSeverity {
    /// Unrecoverable — the operation cannot continue.
    Fatal,
    /// Transient — the same request may succeed if retried.
    Retriable,
    /// The operation completed but with reduced quality or missing data.
    Degraded,
    /// Advisory only — no user action required.
    Informational,
}

// ---------------------------------------------------------------------------
// ClassificationCategory
// ---------------------------------------------------------------------------

/// Operational category used for error classification and recovery routing.
///
/// This is distinct from [`abp_error::ErrorCategory`] which groups codes by
/// domain (Protocol, Backend, …). `ClassificationCategory` groups by the
/// *nature of the failure* for recovery purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationCategory {
    /// Credentials missing, expired, or rejected.
    Authentication,
    /// Upstream rate-limit / quota exhaustion.
    RateLimit,
    /// Requested model identifier is unknown.
    ModelNotFound,
    /// Malformed or semantically invalid request.
    InvalidRequest,
    /// Content was blocked by a safety filter.
    ContentFilter,
    /// Input exceeds the model's context window.
    ContextLength,
    /// Upstream 5xx / internal server error.
    ServerError,
    /// DNS, TCP, TLS, or other network-level failure.
    NetworkError,
    /// Wire-protocol framing or handshake error.
    ProtocolError,
    /// The backend does not support a required capability.
    CapabilityUnsupported,
    /// Cross-dialect translation failed.
    MappingFailure,
    /// Operation exceeded its deadline.
    TimeoutError,
}

// ---------------------------------------------------------------------------
// RecoveryAction
// ---------------------------------------------------------------------------

/// The kind of action a caller should take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    /// Retry the same request (possibly after a delay).
    Retry,
    /// Try an alternative backend or model.
    Fallback,
    /// Reduce the input size / context window usage.
    ReduceContext,
    /// Switch to a different model that supports the required capability.
    ChangeModel,
    /// Human intervention is required.
    ContactAdmin,
    /// No automated recovery is possible.
    None,
}

// ---------------------------------------------------------------------------
// RecoverySuggestion
// ---------------------------------------------------------------------------

/// A concrete recovery suggestion for an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RecoverySuggestion {
    /// What the caller should do.
    pub action: RecoveryAction,
    /// Human-readable explanation of the suggestion.
    pub description: String,
    /// Suggested delay in milliseconds before retrying (if applicable).
    pub delay_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// ErrorClassification
// ---------------------------------------------------------------------------

/// Full classification of an error code — severity, category, and recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ErrorClassification {
    /// The error code that was classified.
    pub code: ErrorCode,
    /// Operational severity.
    pub severity: ErrorSeverity,
    /// Operational category.
    pub category: ClassificationCategory,
    /// Suggested recovery strategy.
    pub recovery: RecoverySuggestion,
}

// ---------------------------------------------------------------------------
// ErrorClassifier
// ---------------------------------------------------------------------------

/// Classifies [`ErrorCode`] values into severity / category / recovery triples.
#[derive(Debug, Clone, Default)]
pub struct ErrorClassifier;

impl ErrorClassifier {
    /// Create a new classifier.
    pub fn new() -> Self {
        Self
    }

    /// Classify a single error code.
    pub fn classify(&self, code: &ErrorCode) -> ErrorClassification {
        let (severity, category) = Self::severity_and_category(code);
        let recovery = self.suggest_recovery_inner(severity, category);
        ErrorClassification {
            code: *code,
            severity,
            category,
            recovery,
        }
    }

    /// Suggest a recovery action for an already-computed classification.
    pub fn suggest_recovery(&self, classification: &ErrorClassification) -> RecoverySuggestion {
        self.suggest_recovery_inner(classification.severity, classification.category)
    }

    // -- private helpers ----------------------------------------------------

    fn severity_and_category(code: &ErrorCode) -> (ErrorSeverity, ClassificationCategory) {
        match code {
            // Protocol
            ErrorCode::ProtocolInvalidEnvelope => {
                (ErrorSeverity::Fatal, ClassificationCategory::ProtocolError)
            }
            ErrorCode::ProtocolHandshakeFailed => {
                (ErrorSeverity::Fatal, ClassificationCategory::ProtocolError)
            }
            ErrorCode::ProtocolMissingRefId => {
                (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest)
            }
            ErrorCode::ProtocolUnexpectedMessage => {
                (ErrorSeverity::Fatal, ClassificationCategory::ProtocolError)
            }
            ErrorCode::ProtocolVersionMismatch => {
                (ErrorSeverity::Fatal, ClassificationCategory::ProtocolError)
            }

            // Mapping
            ErrorCode::MappingUnsupportedCapability => (
                ErrorSeverity::Fatal,
                ClassificationCategory::CapabilityUnsupported,
            ),
            ErrorCode::MappingDialectMismatch => {
                (ErrorSeverity::Fatal, ClassificationCategory::MappingFailure)
            }
            ErrorCode::MappingLossyConversion => (
                ErrorSeverity::Degraded,
                ClassificationCategory::MappingFailure,
            ),
            ErrorCode::MappingUnmappableTool => {
                (ErrorSeverity::Fatal, ClassificationCategory::MappingFailure)
            }

            // Backend
            ErrorCode::BackendNotFound => {
                (ErrorSeverity::Fatal, ClassificationCategory::ServerError)
            }
            ErrorCode::BackendUnavailable => (
                ErrorSeverity::Retriable,
                ClassificationCategory::ServerError,
            ),
            ErrorCode::BackendTimeout => (
                ErrorSeverity::Retriable,
                ClassificationCategory::TimeoutError,
            ),
            ErrorCode::BackendRateLimited => {
                (ErrorSeverity::Retriable, ClassificationCategory::RateLimit)
            }
            ErrorCode::BackendAuthFailed => {
                (ErrorSeverity::Fatal, ClassificationCategory::Authentication)
            }
            ErrorCode::BackendModelNotFound => {
                (ErrorSeverity::Fatal, ClassificationCategory::ModelNotFound)
            }
            ErrorCode::BackendCrashed => (
                ErrorSeverity::Retriable,
                ClassificationCategory::ServerError,
            ),

            // Execution
            ErrorCode::ExecutionToolFailed => {
                (ErrorSeverity::Fatal, ClassificationCategory::ServerError)
            }
            ErrorCode::ExecutionWorkspaceError => {
                (ErrorSeverity::Fatal, ClassificationCategory::ServerError)
            }
            ErrorCode::ExecutionPermissionDenied => {
                (ErrorSeverity::Fatal, ClassificationCategory::Authentication)
            }

            // Contract
            ErrorCode::ContractVersionMismatch => {
                (ErrorSeverity::Fatal, ClassificationCategory::ProtocolError)
            }
            ErrorCode::ContractSchemaViolation => {
                (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest)
            }
            ErrorCode::ContractInvalidReceipt => {
                (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest)
            }

            // Capability
            ErrorCode::CapabilityUnsupported => (
                ErrorSeverity::Fatal,
                ClassificationCategory::CapabilityUnsupported,
            ),
            ErrorCode::CapabilityEmulationFailed => (
                ErrorSeverity::Degraded,
                ClassificationCategory::CapabilityUnsupported,
            ),

            // Policy
            ErrorCode::PolicyDenied => {
                (ErrorSeverity::Fatal, ClassificationCategory::ContentFilter)
            }
            ErrorCode::PolicyInvalid => {
                (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest)
            }

            // Workspace
            ErrorCode::WorkspaceInitFailed => {
                (ErrorSeverity::Fatal, ClassificationCategory::ServerError)
            }
            ErrorCode::WorkspaceStagingFailed => {
                (ErrorSeverity::Fatal, ClassificationCategory::ServerError)
            }

            // IR
            ErrorCode::IrLoweringFailed => {
                (ErrorSeverity::Fatal, ClassificationCategory::MappingFailure)
            }
            ErrorCode::IrInvalid => (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest),

            // Receipt
            ErrorCode::ReceiptHashMismatch => {
                (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest)
            }
            ErrorCode::ReceiptChainBroken => {
                (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest)
            }

            // Dialect
            ErrorCode::DialectUnknown => {
                (ErrorSeverity::Fatal, ClassificationCategory::MappingFailure)
            }
            ErrorCode::DialectMappingFailed => {
                (ErrorSeverity::Fatal, ClassificationCategory::MappingFailure)
            }

            // Config
            ErrorCode::ConfigInvalid => {
                (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest)
            }

            // Internal
            ErrorCode::Internal => (ErrorSeverity::Fatal, ClassificationCategory::ServerError),
        }
    }

    fn suggest_recovery_inner(
        &self,
        severity: ErrorSeverity,
        category: ClassificationCategory,
    ) -> RecoverySuggestion {
        match (severity, category) {
            // Retriable errors
            (ErrorSeverity::Retriable, ClassificationCategory::RateLimit) => RecoverySuggestion {
                action: RecoveryAction::Retry,
                description: "Rate limited — back off and retry after a delay".into(),
                delay_ms: Some(2000),
            },
            (ErrorSeverity::Retriable, ClassificationCategory::TimeoutError) => {
                RecoverySuggestion {
                    action: RecoveryAction::Retry,
                    description: "Request timed out — retry with the same parameters".into(),
                    delay_ms: Some(1000),
                }
            }
            (ErrorSeverity::Retriable, ClassificationCategory::ServerError) => RecoverySuggestion {
                action: RecoveryAction::Retry,
                description: "Server error — retry after a short delay".into(),
                delay_ms: Some(1000),
            },

            // Fatal errors with specific recovery paths
            (_, ClassificationCategory::Authentication) => RecoverySuggestion {
                action: RecoveryAction::ContactAdmin,
                description: "Authentication failed — verify credentials or API keys".into(),
                delay_ms: None,
            },
            (_, ClassificationCategory::ModelNotFound) => RecoverySuggestion {
                action: RecoveryAction::ChangeModel,
                description: "Model not found — switch to an available model".into(),
                delay_ms: None,
            },
            (_, ClassificationCategory::ContextLength) => RecoverySuggestion {
                action: RecoveryAction::ReduceContext,
                description: "Context too long — reduce input size or summarise".into(),
                delay_ms: None,
            },
            (_, ClassificationCategory::CapabilityUnsupported) => RecoverySuggestion {
                action: RecoveryAction::Fallback,
                description: "Capability unsupported — try an alternative backend".into(),
                delay_ms: None,
            },
            (_, ClassificationCategory::MappingFailure) => RecoverySuggestion {
                action: RecoveryAction::Fallback,
                description: "Mapping failed — try a compatible dialect or backend".into(),
                delay_ms: None,
            },
            (_, ClassificationCategory::ContentFilter) => RecoverySuggestion {
                action: RecoveryAction::ContactAdmin,
                description: "Content blocked by policy — review the request content".into(),
                delay_ms: None,
            },

            // Degraded - informational recovery
            (ErrorSeverity::Degraded, _) => RecoverySuggestion {
                action: RecoveryAction::None,
                description: "Operation completed with reduced fidelity — no action required"
                    .into(),
                delay_ms: None,
            },

            // Informational
            (ErrorSeverity::Informational, _) => RecoverySuggestion {
                action: RecoveryAction::None,
                description: "Informational — no action required".into(),
                delay_ms: None,
            },

            // Remaining fatal errors
            (ErrorSeverity::Fatal, ClassificationCategory::ProtocolError) => RecoverySuggestion {
                action: RecoveryAction::ContactAdmin,
                description: "Protocol error — check sidecar compatibility and configuration"
                    .into(),
                delay_ms: None,
            },
            (ErrorSeverity::Fatal, ClassificationCategory::InvalidRequest) => RecoverySuggestion {
                action: RecoveryAction::None,
                description: "Invalid request — fix the request payload before retrying".into(),
                delay_ms: None,
            },
            (ErrorSeverity::Fatal, ClassificationCategory::ServerError) => RecoverySuggestion {
                action: RecoveryAction::ContactAdmin,
                description: "Fatal server error — investigate logs and contact support".into(),
                delay_ms: None,
            },
            (ErrorSeverity::Fatal, ClassificationCategory::NetworkError) => RecoverySuggestion {
                action: RecoveryAction::Retry,
                description: "Network error — check connectivity and retry".into(),
                delay_ms: Some(3000),
            },
            (ErrorSeverity::Fatal, ClassificationCategory::RateLimit) => RecoverySuggestion {
                action: RecoveryAction::Retry,
                description: "Rate limited — wait before retrying".into(),
                delay_ms: Some(5000),
            },
            (ErrorSeverity::Fatal, ClassificationCategory::TimeoutError) => RecoverySuggestion {
                action: RecoveryAction::Retry,
                description: "Timeout — retry with a longer deadline".into(),
                delay_ms: Some(2000),
            },

            // Catch-all retriable
            (ErrorSeverity::Retriable, _) => RecoverySuggestion {
                action: RecoveryAction::Retry,
                description: "Transient error — retry after a short delay".into(),
                delay_ms: Some(1000),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_returns_retriable_for_rate_limited() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendRateLimited);
        assert_eq!(cl.severity, ErrorSeverity::Retriable);
        assert_eq!(cl.category, ClassificationCategory::RateLimit);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
        assert!(cl.recovery.delay_ms.is_some());
    }

    #[test]
    fn classifier_returns_fatal_for_auth_failed() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendAuthFailed);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::Authentication);
        assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    }
}
