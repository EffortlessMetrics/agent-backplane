#![allow(dead_code, unused_imports)]
//! Diagnostic generation — produces human-readable diagnostic reports for ABP
//! errors with probable cause, suggested fix, and related error codes.

use serde::{Deserialize, Serialize};

use crate::category::{self, RecoveryCategory};
use crate::{AbpError, ErrorCode};

/// A diagnostic report for a single error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The error code that triggered this diagnostic.
    pub error_code: ErrorCode,
    /// Human-readable message from the original error.
    pub error_message: String,
    /// Probable root cause of the error.
    pub probable_cause: String,
    /// Suggested fix or remediation steps.
    pub suggested_fix: String,
    /// Error codes that commonly occur together with this one.
    pub related_errors: Vec<ErrorCode>,
    /// Whether the error is retryable.
    pub retryable: bool,
    /// Recovery category classification.
    pub recovery_category: RecoveryCategory,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "═══ ABP Diagnostic ═══")?;
        writeln!(
            f,
            "Error:    [{}] {}",
            self.error_code.as_str(),
            self.error_message
        )?;
        writeln!(f, "Category: {}", self.recovery_category)?;
        writeln!(f, "Cause:    {}", self.probable_cause)?;
        writeln!(f, "Fix:      {}", self.suggested_fix)?;
        if self.retryable {
            writeln!(
                f,
                "Retry:    yes (suggested delay: {:?})",
                category::suggested_delay(self.recovery_category)
            )?;
        } else {
            writeln!(f, "Retry:    no")?;
        }
        if !self.related_errors.is_empty() {
            let related: Vec<&str> = self.related_errors.iter().map(|c| c.as_str()).collect();
            writeln!(f, "Related:  {}", related.join(", "))?;
        }
        write!(f, "══════════════════════")
    }
}

/// Generate a [`Diagnostic`] for the given error.
pub fn generate_diagnostic(error: &AbpError) -> Diagnostic {
    let rc = category::categorize(error.code);
    let (cause, fix, related) = diagnostic_template(error.code);

    Diagnostic {
        error_code: error.code,
        error_message: error.message.clone(),
        probable_cause: cause.to_string(),
        suggested_fix: fix.to_string(),
        related_errors: related.to_vec(),
        retryable: category::is_retryable(rc),
        recovery_category: rc,
    }
}

/// Per-error-code diagnostic templates returning (cause, fix, related codes).
fn diagnostic_template(code: ErrorCode) -> (&'static str, &'static str, &'static [ErrorCode]) {
    match code {
        // -- Protocol --
        ErrorCode::ProtocolInvalidEnvelope => (
            "The JSONL envelope could not be parsed or is missing required fields",
            "Ensure the sidecar sends valid JSON with the correct envelope schema",
            &[
                ErrorCode::ProtocolHandshakeFailed,
                ErrorCode::ProtocolMissingRefId,
            ],
        ),
        ErrorCode::ProtocolHandshakeFailed => (
            "The sidecar did not send a valid hello envelope as its first message",
            "Verify the sidecar implementation sends a hello envelope immediately on startup",
            &[
                ErrorCode::ProtocolInvalidEnvelope,
                ErrorCode::ProtocolVersionMismatch,
            ],
        ),
        ErrorCode::ProtocolMissingRefId => (
            "An envelope is missing the ref_id field needed for correlation",
            "Ensure every envelope after hello includes the ref_id from the run command",
            &[ErrorCode::ProtocolInvalidEnvelope],
        ),
        ErrorCode::ProtocolUnexpectedMessage => (
            "A message arrived out of sequence (e.g. event before hello)",
            "Review sidecar protocol state machine: hello → run → event* → final",
            &[ErrorCode::ProtocolHandshakeFailed],
        ),
        ErrorCode::ProtocolVersionMismatch => (
            "The contract version advertised by the sidecar does not match the host",
            "Update the sidecar or host to use a compatible contract version",
            &[
                ErrorCode::ProtocolHandshakeFailed,
                ErrorCode::ContractVersionMismatch,
            ],
        ),

        // -- Mapping --
        ErrorCode::MappingUnsupportedCapability => (
            "The target dialect does not support a capability required by the work order",
            "Choose a backend that supports the required capability or remove it from the request",
            &[
                ErrorCode::CapabilityUnsupported,
                ErrorCode::MappingDialectMismatch,
            ],
        ),
        ErrorCode::MappingDialectMismatch => (
            "The source and target dialects are fundamentally incompatible",
            "Use a backend with a compatible dialect or configure an explicit mapping",
            &[
                ErrorCode::MappingUnsupportedCapability,
                ErrorCode::DialectMappingFailed,
            ],
        ),
        ErrorCode::MappingLossyConversion => (
            "Translation between dialects succeeded but some information was lost",
            "Review the mapping output and consider using a more compatible dialect pair",
            &[ErrorCode::MappingDialectMismatch],
        ),
        ErrorCode::MappingUnmappableTool => (
            "A tool call in the source dialect has no representation in the target",
            "Register a tool adapter or remove the unsupported tool from the work order",
            &[ErrorCode::MappingUnsupportedCapability],
        ),

        // -- Backend --
        ErrorCode::BackendNotFound => (
            "The requested backend name is not registered",
            "Check the backend name in the configuration and verify it is registered",
            &[ErrorCode::ConfigInvalid],
        ),
        ErrorCode::BackendUnavailable => (
            "The backend is temporarily unreachable or overloaded",
            "Retry the request after a short delay; check backend health status",
            &[ErrorCode::BackendTimeout, ErrorCode::BackendCrashed],
        ),
        ErrorCode::BackendTimeout => (
            "The backend did not respond within the configured timeout",
            "Increase the timeout or check backend latency; retry may succeed",
            &[ErrorCode::BackendUnavailable],
        ),
        ErrorCode::BackendRateLimited => (
            "The backend rejected the request due to rate limiting or quota exhaustion",
            "Wait for the rate-limit window to reset; consider reducing request frequency",
            &[ErrorCode::BackendUnavailable],
        ),
        ErrorCode::BackendAuthFailed => (
            "The API key or credentials for this backend are invalid or expired",
            "Verify the API key in your configuration and ensure it has not been revoked",
            &[],
        ),
        ErrorCode::BackendModelNotFound => (
            "The specified model does not exist on this backend",
            "Check the model name for typos and verify it is available in your plan/region",
            &[ErrorCode::BackendNotFound],
        ),
        ErrorCode::BackendCrashed => (
            "The backend sidecar process terminated unexpectedly",
            "Check sidecar logs for crash details; the process may need to be restarted",
            &[ErrorCode::BackendUnavailable],
        ),

        // -- Execution --
        ErrorCode::ExecutionToolFailed => (
            "A tool invocation returned an error during execution",
            "Check the tool's error output and ensure its preconditions are met",
            &[ErrorCode::ExecutionWorkspaceError],
        ),
        ErrorCode::ExecutionWorkspaceError => (
            "An error occurred within the staged workspace (file I/O, git, etc.)",
            "Check disk space, permissions, and that the workspace path is accessible",
            &[
                ErrorCode::WorkspaceInitFailed,
                ErrorCode::WorkspaceStagingFailed,
            ],
        ),
        ErrorCode::ExecutionPermissionDenied => (
            "The operation was blocked due to insufficient permissions",
            "Review the policy profile and grant the necessary permissions",
            &[ErrorCode::PolicyDenied],
        ),

        // -- Contract --
        ErrorCode::ContractVersionMismatch => (
            "The contract version in the payload does not match the expected version",
            "Ensure all components use the same contract version (currently abp/v0.1)",
            &[ErrorCode::ProtocolVersionMismatch],
        ),
        ErrorCode::ContractSchemaViolation => (
            "The payload does not conform to the expected JSON schema",
            "Validate the payload against the contract schema in contracts/schemas/",
            &[ErrorCode::ContractInvalidReceipt],
        ),
        ErrorCode::ContractInvalidReceipt => (
            "The receipt is structurally invalid or its hash cannot be verified",
            "Re-generate the receipt using receipt.with_hash() and verify the chain",
            &[
                ErrorCode::ReceiptHashMismatch,
                ErrorCode::ReceiptChainBroken,
            ],
        ),

        // -- Capability --
        ErrorCode::CapabilityUnsupported => (
            "A required capability is not supported by the selected backend",
            "Choose a backend that supports the capability or use capability emulation",
            &[
                ErrorCode::CapabilityEmulationFailed,
                ErrorCode::MappingUnsupportedCapability,
            ],
        ),
        ErrorCode::CapabilityEmulationFailed => (
            "The emulation layer for an unsupported capability failed",
            "Check the emulation implementation or switch to a backend with native support",
            &[ErrorCode::CapabilityUnsupported],
        ),

        // -- Policy --
        ErrorCode::PolicyDenied => (
            "A policy rule explicitly denied this operation",
            "Review the policy profile and adjust allow/deny rules as needed",
            &[
                ErrorCode::PolicyInvalid,
                ErrorCode::ExecutionPermissionDenied,
            ],
        ),
        ErrorCode::PolicyInvalid => (
            "The policy definition itself is malformed and cannot be compiled",
            "Check the policy TOML/JSON for syntax errors and invalid glob patterns",
            &[ErrorCode::PolicyDenied, ErrorCode::ConfigInvalid],
        ),

        // -- Workspace --
        ErrorCode::WorkspaceInitFailed => (
            "Could not create or initialise the staged workspace directory",
            "Ensure the temp directory is writable and has sufficient space",
            &[ErrorCode::WorkspaceStagingFailed],
        ),
        ErrorCode::WorkspaceStagingFailed => (
            "File copy or staging into the workspace directory failed",
            "Check source paths, disk space, and file permissions",
            &[ErrorCode::WorkspaceInitFailed],
        ),

        // -- IR --
        ErrorCode::IrLoweringFailed => (
            "Lowering from the intermediate representation to wire format failed",
            "Check the IR node types and ensure all required fields are populated",
            &[ErrorCode::IrInvalid],
        ),
        ErrorCode::IrInvalid => (
            "The IR structure is internally inconsistent or contains invalid nodes",
            "Validate IR construction and ensure node types match the schema",
            &[ErrorCode::IrLoweringFailed],
        ),

        // -- Receipt --
        ErrorCode::ReceiptHashMismatch => (
            "The computed SHA-256 hash of the receipt does not match the declared hash",
            "Re-hash the receipt using receipt.with_hash(); do not modify after hashing",
            &[
                ErrorCode::ReceiptChainBroken,
                ErrorCode::ContractInvalidReceipt,
            ],
        ),
        ErrorCode::ReceiptChainBroken => (
            "The receipt chain has a gap or entries are out of order",
            "Verify chain parent_id links and ensure no receipts are missing",
            &[ErrorCode::ReceiptHashMismatch],
        ),

        // -- Dialect --
        ErrorCode::DialectUnknown => (
            "The dialect identifier is not recognised by the system",
            "Check the dialect name for typos and ensure it is registered",
            &[ErrorCode::DialectMappingFailed],
        ),
        ErrorCode::DialectMappingFailed => (
            "Translation between the source and target dialects failed",
            "Check the mapping implementation and ensure both dialects are supported",
            &[ErrorCode::DialectUnknown, ErrorCode::MappingDialectMismatch],
        ),

        // -- Config --
        ErrorCode::ConfigInvalid => (
            "The configuration file or a configuration value is invalid",
            "Check backplane.toml for syntax errors and validate against the schema",
            &[ErrorCode::PolicyInvalid],
        ),

        // -- Internal --
        ErrorCode::Internal => (
            "An unexpected internal error occurred",
            "Check logs for stack traces; this may indicate a bug in ABP",
            &[],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_diagnostic_basic() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
        let diag = generate_diagnostic(&err);
        assert_eq!(diag.error_code, ErrorCode::BackendTimeout);
        assert_eq!(diag.error_message, "timed out after 30s");
        assert!(!diag.probable_cause.is_empty());
        assert!(!diag.suggested_fix.is_empty());
        assert!(diag.retryable);
        assert_eq!(diag.recovery_category, RecoveryCategory::NetworkTransient);
    }

    #[test]
    fn generate_diagnostic_non_retryable() {
        let err = AbpError::new(ErrorCode::BackendAuthFailed, "invalid key");
        let diag = generate_diagnostic(&err);
        assert!(!diag.retryable);
        assert_eq!(diag.recovery_category, RecoveryCategory::Authentication);
    }

    #[test]
    fn generate_diagnostic_has_related() {
        let err = AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "bad json");
        let diag = generate_diagnostic(&err);
        assert!(!diag.related_errors.is_empty());
        assert!(diag
            .related_errors
            .contains(&ErrorCode::ProtocolHandshakeFailed));
    }

    #[test]
    fn generate_diagnostic_internal_no_related() {
        let err = AbpError::new(ErrorCode::Internal, "oops");
        let diag = generate_diagnostic(&err);
        assert!(diag.related_errors.is_empty());
    }

    #[test]
    fn diagnostic_display_formatting() {
        let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited");
        let diag = generate_diagnostic(&err);
        let display = diag.to_string();
        assert!(display.contains("ABP Diagnostic"));
        assert!(display.contains("backend_rate_limited"));
        assert!(display.contains("rate limited"));
        assert!(display.contains("Cause:"));
        assert!(display.contains("Fix:"));
    }

    #[test]
    fn diagnostic_display_retryable_shows_delay() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        let diag = generate_diagnostic(&err);
        let display = diag.to_string();
        assert!(display.contains("Retry:    yes"));
    }

    #[test]
    fn diagnostic_display_non_retryable() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        let diag = generate_diagnostic(&err);
        let display = diag.to_string();
        assert!(display.contains("Retry:    no"));
    }

    #[test]
    fn diagnostic_display_with_related() {
        let err = AbpError::new(ErrorCode::ContractInvalidReceipt, "bad receipt");
        let diag = generate_diagnostic(&err);
        let display = diag.to_string();
        assert!(display.contains("Related:"));
        assert!(display.contains("receipt_hash_mismatch"));
    }

    #[test]
    fn diagnostic_serde_roundtrip() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        let diag = generate_diagnostic(&err);
        let json = serde_json::to_string(&diag).unwrap();
        let back: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error_code, diag.error_code);
        assert_eq!(back.probable_cause, diag.probable_cause);
        assert_eq!(back.suggested_fix, diag.suggested_fix);
        assert_eq!(back.related_errors, diag.related_errors);
        assert_eq!(back.retryable, diag.retryable);
    }

    #[test]
    fn all_error_codes_have_diagnostics() {
        // Ensures every ErrorCode variant produces a valid diagnostic.
        let all_codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
            ErrorCode::MappingUnsupportedCapability,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::MappingLossyConversion,
            ErrorCode::MappingUnmappableTool,
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::BackendCrashed,
            ErrorCode::ExecutionToolFailed,
            ErrorCode::ExecutionWorkspaceError,
            ErrorCode::ExecutionPermissionDenied,
            ErrorCode::ContractVersionMismatch,
            ErrorCode::ContractSchemaViolation,
            ErrorCode::ContractInvalidReceipt,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::CapabilityEmulationFailed,
            ErrorCode::PolicyDenied,
            ErrorCode::PolicyInvalid,
            ErrorCode::WorkspaceInitFailed,
            ErrorCode::WorkspaceStagingFailed,
            ErrorCode::IrLoweringFailed,
            ErrorCode::IrInvalid,
            ErrorCode::ReceiptHashMismatch,
            ErrorCode::ReceiptChainBroken,
            ErrorCode::DialectUnknown,
            ErrorCode::DialectMappingFailed,
            ErrorCode::ConfigInvalid,
            ErrorCode::Internal,
        ];
        for code in all_codes {
            let err = AbpError::new(code, "test");
            let diag = generate_diagnostic(&err);
            assert_eq!(diag.error_code, code);
            assert!(
                !diag.probable_cause.is_empty(),
                "empty probable_cause for {code:?}"
            );
            assert!(
                !diag.suggested_fix.is_empty(),
                "empty suggested_fix for {code:?}"
            );
        }
    }

    #[test]
    fn diagnostic_preserves_error_message() {
        let err = AbpError::new(ErrorCode::ConfigInvalid, "missing field 'backend'");
        let diag = generate_diagnostic(&err);
        assert_eq!(diag.error_message, "missing field 'backend'");
    }

    #[test]
    fn diagnostic_policy_violation_category() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        let diag = generate_diagnostic(&err);
        assert_eq!(diag.recovery_category, RecoveryCategory::PolicyViolation);
        assert!(!diag.retryable);
    }

    #[test]
    fn diagnostic_mapping_failure_category() {
        let err = AbpError::new(ErrorCode::MappingDialectMismatch, "mismatch");
        let diag = generate_diagnostic(&err);
        assert_eq!(diag.recovery_category, RecoveryCategory::MappingFailure);
        assert!(!diag.retryable);
    }
}
