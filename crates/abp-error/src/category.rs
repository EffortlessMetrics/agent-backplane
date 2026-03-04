#![allow(dead_code, unused_imports)]
//! Extended error categorization with retryability and suggested delays.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::ErrorCode;

/// Recovery-oriented error category that groups [`ErrorCode`]s by their
/// operational characteristics (retryability, typical delay, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryCategory {
    /// Backend authentication or API-key errors.
    Authentication,
    /// Rate-limit / quota exhaustion from a vendor.
    RateLimit,
    /// The requested model lacks a required capability.
    ModelCapability,
    /// Caller-supplied input failed validation.
    InputValidation,
    /// Transient network-level failure (timeout, DNS, TCP reset).
    NetworkTransient,
    /// Remote server returned a 5xx or equivalent.
    ServerInternal,
    /// Wire-protocol violation (bad envelope, handshake, ordering).
    ProtocolViolation,
    /// Cross-dialect or IR mapping failure.
    MappingFailure,
    /// A policy rule blocked the operation.
    PolicyViolation,
    /// A local resource (disk, memory, file handles) is exhausted.
    ResourceExhausted,
}

impl std::fmt::Display for RecoveryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Authentication => "authentication",
            Self::RateLimit => "rate_limit",
            Self::ModelCapability => "model_capability",
            Self::InputValidation => "input_validation",
            Self::NetworkTransient => "network_transient",
            Self::ServerInternal => "server_internal",
            Self::ProtocolViolation => "protocol_violation",
            Self::MappingFailure => "mapping_failure",
            Self::PolicyViolation => "policy_violation",
            Self::ResourceExhausted => "resource_exhausted",
        };
        f.write_str(s)
    }
}

/// Map an [`ErrorCode`] to its [`RecoveryCategory`].
pub fn categorize(code: ErrorCode) -> RecoveryCategory {
    match code {
        ErrorCode::BackendAuthFailed => RecoveryCategory::Authentication,

        ErrorCode::BackendRateLimited => RecoveryCategory::RateLimit,

        ErrorCode::BackendModelNotFound
        | ErrorCode::CapabilityUnsupported
        | ErrorCode::CapabilityEmulationFailed => RecoveryCategory::ModelCapability,

        ErrorCode::ContractSchemaViolation
        | ErrorCode::ContractVersionMismatch
        | ErrorCode::ConfigInvalid
        | ErrorCode::IrInvalid => RecoveryCategory::InputValidation,

        ErrorCode::BackendTimeout
        | ErrorCode::BackendUnavailable
        | ErrorCode::BackendCrashed => RecoveryCategory::NetworkTransient,

        ErrorCode::BackendNotFound | ErrorCode::Internal => RecoveryCategory::ServerInternal,

        ErrorCode::ProtocolInvalidEnvelope
        | ErrorCode::ProtocolHandshakeFailed
        | ErrorCode::ProtocolMissingRefId
        | ErrorCode::ProtocolUnexpectedMessage
        | ErrorCode::ProtocolVersionMismatch => RecoveryCategory::ProtocolViolation,

        ErrorCode::MappingUnsupportedCapability
        | ErrorCode::MappingDialectMismatch
        | ErrorCode::MappingLossyConversion
        | ErrorCode::MappingUnmappableTool
        | ErrorCode::DialectUnknown
        | ErrorCode::DialectMappingFailed
        | ErrorCode::IrLoweringFailed
        | ErrorCode::ContractInvalidReceipt
        | ErrorCode::ReceiptHashMismatch
        | ErrorCode::ReceiptChainBroken => RecoveryCategory::MappingFailure,

        ErrorCode::PolicyDenied
        | ErrorCode::PolicyInvalid
        | ErrorCode::ExecutionPermissionDenied => RecoveryCategory::PolicyViolation,

        ErrorCode::WorkspaceInitFailed
        | ErrorCode::WorkspaceStagingFailed
        | ErrorCode::ExecutionToolFailed
        | ErrorCode::ExecutionWorkspaceError => RecoveryCategory::ResourceExhausted,
    }
}

/// Whether operations that produced errors in this category should be retried.
pub fn is_retryable(category: RecoveryCategory) -> bool {
    matches!(
        category,
        RecoveryCategory::RateLimit
            | RecoveryCategory::NetworkTransient
            | RecoveryCategory::ServerInternal
            | RecoveryCategory::ResourceExhausted
    )
}

/// Suggested back-off delay before retrying an operation that failed with this
/// category of error.  Returns [`Duration::ZERO`] for non-retryable categories.
pub fn suggested_delay(category: RecoveryCategory) -> Duration {
    match category {
        RecoveryCategory::RateLimit => Duration::from_secs(30),
        RecoveryCategory::NetworkTransient => Duration::from_secs(2),
        RecoveryCategory::ServerInternal => Duration::from_secs(5),
        RecoveryCategory::ResourceExhausted => Duration::from_secs(10),
        _ => Duration::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorize_auth() {
        assert_eq!(
            categorize(ErrorCode::BackendAuthFailed),
            RecoveryCategory::Authentication
        );
    }

    #[test]
    fn categorize_rate_limit() {
        assert_eq!(
            categorize(ErrorCode::BackendRateLimited),
            RecoveryCategory::RateLimit
        );
    }

    #[test]
    fn categorize_model_capability() {
        assert_eq!(
            categorize(ErrorCode::BackendModelNotFound),
            RecoveryCategory::ModelCapability
        );
        assert_eq!(
            categorize(ErrorCode::CapabilityUnsupported),
            RecoveryCategory::ModelCapability
        );
        assert_eq!(
            categorize(ErrorCode::CapabilityEmulationFailed),
            RecoveryCategory::ModelCapability
        );
    }

    #[test]
    fn categorize_input_validation() {
        assert_eq!(
            categorize(ErrorCode::ContractSchemaViolation),
            RecoveryCategory::InputValidation
        );
        assert_eq!(
            categorize(ErrorCode::ConfigInvalid),
            RecoveryCategory::InputValidation
        );
        assert_eq!(
            categorize(ErrorCode::IrInvalid),
            RecoveryCategory::InputValidation
        );
    }

    #[test]
    fn categorize_network_transient() {
        assert_eq!(
            categorize(ErrorCode::BackendTimeout),
            RecoveryCategory::NetworkTransient
        );
        assert_eq!(
            categorize(ErrorCode::BackendUnavailable),
            RecoveryCategory::NetworkTransient
        );
        assert_eq!(
            categorize(ErrorCode::BackendCrashed),
            RecoveryCategory::NetworkTransient
        );
    }

    #[test]
    fn categorize_server_internal() {
        assert_eq!(
            categorize(ErrorCode::Internal),
            RecoveryCategory::ServerInternal
        );
        assert_eq!(
            categorize(ErrorCode::BackendNotFound),
            RecoveryCategory::ServerInternal
        );
    }

    #[test]
    fn categorize_protocol_violation() {
        assert_eq!(
            categorize(ErrorCode::ProtocolInvalidEnvelope),
            RecoveryCategory::ProtocolViolation
        );
        assert_eq!(
            categorize(ErrorCode::ProtocolHandshakeFailed),
            RecoveryCategory::ProtocolViolation
        );
        assert_eq!(
            categorize(ErrorCode::ProtocolMissingRefId),
            RecoveryCategory::ProtocolViolation
        );
        assert_eq!(
            categorize(ErrorCode::ProtocolUnexpectedMessage),
            RecoveryCategory::ProtocolViolation
        );
        assert_eq!(
            categorize(ErrorCode::ProtocolVersionMismatch),
            RecoveryCategory::ProtocolViolation
        );
    }

    #[test]
    fn categorize_mapping_failure() {
        assert_eq!(
            categorize(ErrorCode::MappingDialectMismatch),
            RecoveryCategory::MappingFailure
        );
        assert_eq!(
            categorize(ErrorCode::DialectUnknown),
            RecoveryCategory::MappingFailure
        );
        assert_eq!(
            categorize(ErrorCode::ReceiptHashMismatch),
            RecoveryCategory::MappingFailure
        );
    }

    #[test]
    fn categorize_policy_violation() {
        assert_eq!(
            categorize(ErrorCode::PolicyDenied),
            RecoveryCategory::PolicyViolation
        );
        assert_eq!(
            categorize(ErrorCode::PolicyInvalid),
            RecoveryCategory::PolicyViolation
        );
        assert_eq!(
            categorize(ErrorCode::ExecutionPermissionDenied),
            RecoveryCategory::PolicyViolation
        );
    }

    #[test]
    fn categorize_resource_exhausted() {
        assert_eq!(
            categorize(ErrorCode::WorkspaceInitFailed),
            RecoveryCategory::ResourceExhausted
        );
        assert_eq!(
            categorize(ErrorCode::ExecutionToolFailed),
            RecoveryCategory::ResourceExhausted
        );
    }

    #[test]
    fn retryable_categories() {
        assert!(is_retryable(RecoveryCategory::RateLimit));
        assert!(is_retryable(RecoveryCategory::NetworkTransient));
        assert!(is_retryable(RecoveryCategory::ServerInternal));
        assert!(is_retryable(RecoveryCategory::ResourceExhausted));
    }

    #[test]
    fn non_retryable_categories() {
        assert!(!is_retryable(RecoveryCategory::Authentication));
        assert!(!is_retryable(RecoveryCategory::ModelCapability));
        assert!(!is_retryable(RecoveryCategory::InputValidation));
        assert!(!is_retryable(RecoveryCategory::ProtocolViolation));
        assert!(!is_retryable(RecoveryCategory::MappingFailure));
        assert!(!is_retryable(RecoveryCategory::PolicyViolation));
    }

    #[test]
    fn suggested_delay_rate_limit() {
        assert_eq!(
            suggested_delay(RecoveryCategory::RateLimit),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn suggested_delay_network_transient() {
        assert_eq!(
            suggested_delay(RecoveryCategory::NetworkTransient),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn suggested_delay_server_internal() {
        assert_eq!(
            suggested_delay(RecoveryCategory::ServerInternal),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn suggested_delay_resource_exhausted() {
        assert_eq!(
            suggested_delay(RecoveryCategory::ResourceExhausted),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn suggested_delay_non_retryable_is_zero() {
        assert_eq!(
            suggested_delay(RecoveryCategory::Authentication),
            Duration::ZERO
        );
        assert_eq!(
            suggested_delay(RecoveryCategory::PolicyViolation),
            Duration::ZERO
        );
        assert_eq!(
            suggested_delay(RecoveryCategory::MappingFailure),
            Duration::ZERO
        );
    }

    #[test]
    fn recovery_category_serde_roundtrip() {
        let cats = [
            RecoveryCategory::Authentication,
            RecoveryCategory::RateLimit,
            RecoveryCategory::ModelCapability,
            RecoveryCategory::InputValidation,
            RecoveryCategory::NetworkTransient,
            RecoveryCategory::ServerInternal,
            RecoveryCategory::ProtocolViolation,
            RecoveryCategory::MappingFailure,
            RecoveryCategory::PolicyViolation,
            RecoveryCategory::ResourceExhausted,
        ];
        for cat in cats {
            let json = serde_json::to_string(&cat).unwrap();
            let back: RecoveryCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(back, cat, "roundtrip failed for {cat:?}");
        }
    }

    #[test]
    fn recovery_category_display() {
        assert_eq!(RecoveryCategory::Authentication.to_string(), "authentication");
        assert_eq!(RecoveryCategory::RateLimit.to_string(), "rate_limit");
        assert_eq!(
            RecoveryCategory::NetworkTransient.to_string(),
            "network_transient"
        );
    }
}
