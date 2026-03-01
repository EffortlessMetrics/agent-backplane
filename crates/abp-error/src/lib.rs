//! Unified error taxonomy with stable error codes for the Agent Backplane.
//!
//! Every ABP error carries an [`ErrorCode`] (a machine-readable, stable string
//! tag), a human-readable message, an optional cause chain, and arbitrary
//! key-value context.  Use the builder returned by [`AbpError::new`] to
//! construct errors fluently.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// ErrorCategory
// ---------------------------------------------------------------------------

/// Broad family that an [`ErrorCode`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// JSONL / wire-format errors.
    Protocol,
    /// Backend lifecycle errors.
    Backend,
    /// Capability negotiation errors.
    Capability,
    /// Policy evaluation errors.
    Policy,
    /// Workspace staging / git errors.
    Workspace,
    /// IR translation errors.
    Ir,
    /// Receipt integrity errors.
    Receipt,
    /// Dialect mapping errors.
    Dialect,
    /// Configuration errors.
    Config,
    /// Catch-all for unexpected internal errors.
    Internal,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Protocol => "protocol",
            Self::Backend => "backend",
            Self::Capability => "capability",
            Self::Policy => "policy",
            Self::Workspace => "workspace",
            Self::Ir => "ir",
            Self::Receipt => "receipt",
            Self::Dialect => "dialect",
            Self::Config => "config",
            Self::Internal => "internal",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// ErrorCode
// ---------------------------------------------------------------------------

/// Machine-readable, stable error code.
///
/// Each variant serialises to a `SCREAMING_SNAKE_CASE` string that is
/// guaranteed not to change across patch releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // -- Protocol --
    /// Envelope failed to parse or has missing/invalid fields.
    ProtocolInvalidEnvelope,
    /// Message arrived in wrong order (e.g. event before hello).
    ProtocolUnexpectedMessage,
    /// Contract version mismatch between host and sidecar.
    ProtocolVersionMismatch,

    // -- Backend --
    /// Requested backend name does not exist.
    BackendNotFound,
    /// Backend did not respond within the configured timeout.
    BackendTimeout,
    /// Backend process exited unexpectedly.
    BackendCrashed,

    // -- Capability --
    /// A required capability is not supported by the backend.
    CapabilityUnsupported,
    /// Capability emulation layer failed.
    CapabilityEmulationFailed,

    // -- Policy --
    /// A policy rule denied the operation.
    PolicyDenied,
    /// Policy definition itself is malformed.
    PolicyInvalid,

    // -- Workspace --
    /// Failed to initialise the staged workspace.
    WorkspaceInitFailed,
    /// Failed to copy / stage files into the workspace.
    WorkspaceStagingFailed,

    // -- IR --
    /// IR lowering (high-level → wire) failed.
    IrLoweringFailed,
    /// IR structure is invalid or inconsistent.
    IrInvalid,

    // -- Receipt --
    /// Computed receipt hash does not match the declared hash.
    ReceiptHashMismatch,
    /// Receipt chain has a gap or out-of-order entry.
    ReceiptChainBroken,

    // -- Dialect --
    /// Dialect identifier is not recognised.
    DialectUnknown,
    /// Mapping between dialects failed.
    DialectMappingFailed,

    // -- Config --
    /// Configuration file or value is invalid.
    ConfigInvalid,

    // -- Internal --
    /// Catch-all for unexpected internal errors.
    Internal,
}

impl ErrorCode {
    /// Returns the broad [`ErrorCategory`] this code belongs to.
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::ProtocolInvalidEnvelope
            | Self::ProtocolUnexpectedMessage
            | Self::ProtocolVersionMismatch => ErrorCategory::Protocol,

            Self::BackendNotFound | Self::BackendTimeout | Self::BackendCrashed => {
                ErrorCategory::Backend
            }

            Self::CapabilityUnsupported | Self::CapabilityEmulationFailed => {
                ErrorCategory::Capability
            }

            Self::PolicyDenied | Self::PolicyInvalid => ErrorCategory::Policy,

            Self::WorkspaceInitFailed | Self::WorkspaceStagingFailed => ErrorCategory::Workspace,

            Self::IrLoweringFailed | Self::IrInvalid => ErrorCategory::Ir,

            Self::ReceiptHashMismatch | Self::ReceiptChainBroken => ErrorCategory::Receipt,

            Self::DialectUnknown | Self::DialectMappingFailed => ErrorCategory::Dialect,

            Self::ConfigInvalid => ErrorCategory::Config,

            Self::Internal => ErrorCategory::Internal,
        }
    }

    /// Stable `&'static str` representation of the code (e.g.
    /// `"PROTOCOL_INVALID_ENVELOPE"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProtocolInvalidEnvelope => "PROTOCOL_INVALID_ENVELOPE",
            Self::ProtocolUnexpectedMessage => "PROTOCOL_UNEXPECTED_MESSAGE",
            Self::ProtocolVersionMismatch => "PROTOCOL_VERSION_MISMATCH",
            Self::BackendNotFound => "BACKEND_NOT_FOUND",
            Self::BackendTimeout => "BACKEND_TIMEOUT",
            Self::BackendCrashed => "BACKEND_CRASHED",
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            Self::CapabilityEmulationFailed => "CAPABILITY_EMULATION_FAILED",
            Self::PolicyDenied => "POLICY_DENIED",
            Self::PolicyInvalid => "POLICY_INVALID",
            Self::WorkspaceInitFailed => "WORKSPACE_INIT_FAILED",
            Self::WorkspaceStagingFailed => "WORKSPACE_STAGING_FAILED",
            Self::IrLoweringFailed => "IR_LOWERING_FAILED",
            Self::IrInvalid => "IR_INVALID",
            Self::ReceiptHashMismatch => "RECEIPT_HASH_MISMATCH",
            Self::ReceiptChainBroken => "RECEIPT_CHAIN_BROKEN",
            Self::DialectUnknown => "DIALECT_UNKNOWN",
            Self::DialectMappingFailed => "DIALECT_MAPPING_FAILED",
            Self::ConfigInvalid => "CONFIG_INVALID",
            Self::Internal => "INTERNAL",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// AbpError
// ---------------------------------------------------------------------------

/// Unified ABP error.
///
/// Carries a stable [`ErrorCode`], a human-readable message, an optional
/// source error for cause-chaining, and arbitrary structured context.
///
/// # Builder usage
///
/// ```
/// use abp_error::{AbpError, ErrorCode};
///
/// let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30 s")
///     .with_context("backend", "openai")
///     .with_context("timeout_ms", 30_000);
/// ```
pub struct AbpError {
    /// Machine-readable error code.
    pub code: ErrorCode,
    /// Human-readable description.
    pub message: String,
    /// Optional underlying cause.
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
    /// Arbitrary structured context for diagnostics.
    pub context: BTreeMap<String, serde_json::Value>,
}

impl AbpError {
    /// Create a new error with the given code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source: None,
            context: BTreeMap::new(),
        }
    }

    /// Attach a key-value pair to the diagnostic context.
    ///
    /// The value is converted via [`serde_json::to_value`]; if serialisation
    /// fails, the entry is silently skipped.
    pub fn with_context(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.context.insert(key.into(), v);
        }
        self
    }

    /// Attach an underlying cause.
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Shorthand for `self.code.category()`.
    pub fn category(&self) -> ErrorCategory {
        self.code.category()
    }
}

impl fmt::Debug for AbpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("AbpError");
        d.field("code", &self.code);
        d.field("message", &self.message);
        if let Some(ref src) = self.source {
            d.field("source", &src.to_string());
        }
        if !self.context.is_empty() {
            d.field("context", &self.context);
        }
        d.finish()
    }
}

impl fmt::Display for AbpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)?;
        if !self.context.is_empty() {
            // Deterministic output thanks to BTreeMap.
            if let Ok(ctx) = serde_json::to_string(&self.context) {
                write!(f, " {ctx}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for AbpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

// ---------------------------------------------------------------------------
// Serialization support
// ---------------------------------------------------------------------------

/// Serialisable snapshot of an [`AbpError`] (without the opaque source).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AbpErrorDto {
    /// Error code.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
    /// Structured context.
    pub context: BTreeMap<String, serde_json::Value>,
    /// String representation of the source error, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_message: Option<String>,
}

impl From<&AbpError> for AbpErrorDto {
    fn from(err: &AbpError) -> Self {
        Self {
            code: err.code,
            message: err.message.clone(),
            context: err.context.clone(),
            source_message: err.source.as_ref().map(|s| s.to_string()),
        }
    }
}

impl From<AbpErrorDto> for AbpError {
    fn from(dto: AbpErrorDto) -> Self {
        Self {
            code: dto.code,
            message: dto.message,
            source: None,
            context: dto.context,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io;

    /// All error codes for exhaustive iteration in tests.
    const ALL_CODES: &[ErrorCode] = &[
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
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

    // -- Construction & Display -----------------------------------------

    #[test]
    fn basic_construction() {
        let err = AbpError::new(ErrorCode::Internal, "boom");
        assert_eq!(err.code, ErrorCode::Internal);
        assert_eq!(err.message, "boom");
        assert!(err.source.is_none());
        assert!(err.context.is_empty());
    }

    #[test]
    fn display_without_context() {
        let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
        assert_eq!(err.to_string(), "[BACKEND_NOT_FOUND] no such backend");
    }

    #[test]
    fn display_with_context() {
        let err =
            AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 5000);
        let s = err.to_string();
        assert!(s.starts_with("[BACKEND_TIMEOUT] timed out"));
        assert!(s.contains("timeout_ms"));
        assert!(s.contains("5000"));
    }

    #[test]
    fn debug_impl() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "nope");
        let dbg = format!("{err:?}");
        assert!(dbg.contains("PolicyDenied"));
        assert!(dbg.contains("nope"));
    }

    #[test]
    fn debug_with_source() {
        let src = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
        let dbg = format!("{err:?}");
        assert!(dbg.contains("source"));
        assert!(dbg.contains("file missing"));
    }

    // -- Error code categorization --------------------------------------

    #[test]
    fn protocol_codes_categorised() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolUnexpectedMessage.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolVersionMismatch.category(),
            ErrorCategory::Protocol
        );
    }

    #[test]
    fn backend_codes_categorised() {
        assert_eq!(
            ErrorCode::BackendNotFound.category(),
            ErrorCategory::Backend
        );
        assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
        assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
    }

    #[test]
    fn capability_codes_categorised() {
        assert_eq!(
            ErrorCode::CapabilityUnsupported.category(),
            ErrorCategory::Capability
        );
        assert_eq!(
            ErrorCode::CapabilityEmulationFailed.category(),
            ErrorCategory::Capability
        );
    }

    #[test]
    fn policy_codes_categorised() {
        assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
        assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
    }

    #[test]
    fn workspace_codes_categorised() {
        assert_eq!(
            ErrorCode::WorkspaceInitFailed.category(),
            ErrorCategory::Workspace
        );
        assert_eq!(
            ErrorCode::WorkspaceStagingFailed.category(),
            ErrorCategory::Workspace
        );
    }

    #[test]
    fn ir_codes_categorised() {
        assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
        assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
    }

    #[test]
    fn receipt_codes_categorised() {
        assert_eq!(
            ErrorCode::ReceiptHashMismatch.category(),
            ErrorCategory::Receipt
        );
        assert_eq!(
            ErrorCode::ReceiptChainBroken.category(),
            ErrorCategory::Receipt
        );
    }

    #[test]
    fn dialect_codes_categorised() {
        assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
        assert_eq!(
            ErrorCode::DialectMappingFailed.category(),
            ErrorCategory::Dialect
        );
    }

    #[test]
    fn config_code_categorised() {
        assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    }

    #[test]
    fn internal_code_categorised() {
        assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
    }

    // -- Builder pattern ------------------------------------------------

    #[test]
    fn builder_with_context_multiple_keys() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
            .with_context("backend", "openai")
            .with_context("timeout_ms", 30_000)
            .with_context("retries", 3);
        assert_eq!(err.context.len(), 3);
        assert_eq!(err.context["backend"], serde_json::json!("openai"));
        assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
        assert_eq!(err.context["retries"], serde_json::json!(3));
    }

    #[test]
    fn builder_with_source() {
        let src = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied").with_source(src);
        assert!(err.source.is_some());
        let displayed = err.source.as_ref().unwrap().to_string();
        assert_eq!(displayed, "access denied");
    }

    #[test]
    fn builder_chaining_all() {
        let src = io::Error::other("underlying");
        let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
            .with_context("file", "backplane.toml")
            .with_source(src);
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
        assert_eq!(err.context["file"], serde_json::json!("backplane.toml"));
        assert!(err.source.is_some());
    }

    #[test]
    fn category_shorthand() {
        let err = AbpError::new(ErrorCode::DialectUnknown, "unknown dialect");
        assert_eq!(err.category(), ErrorCategory::Dialect);
    }

    // -- Serialization / Deserialization --------------------------------

    #[test]
    fn error_code_serde_roundtrip() {
        let code = ErrorCode::ProtocolInvalidEnvelope;
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(json, r#""PROTOCOL_INVALID_ENVELOPE""#);
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }

    #[test]
    fn error_category_serde_roundtrip() {
        let cat = ErrorCategory::Backend;
        let json = serde_json::to_string(&cat).unwrap();
        assert_eq!(json, r#""backend""#);
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }

    #[test]
    fn dto_roundtrip_without_source() {
        let err = AbpError::new(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
        assert!(back.source_message.is_none());
    }

    #[test]
    fn dto_roundtrip_with_source() {
        let src = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
        let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("pipe broke"));
    }

    #[test]
    fn dto_to_abp_error() {
        let dto = AbpErrorDto {
            code: ErrorCode::ConfigInvalid,
            message: "bad".into(),
            context: BTreeMap::new(),
            source_message: Some("inner".into()),
        };
        let err: AbpError = dto.into();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
        // Source is lost in DTO → AbpError conversion (opaque type).
        assert!(err.source.is_none());
    }

    // -- Error chain (source) preservation ------------------------------

    #[test]
    fn std_error_source_chain() {
        let inner = io::Error::new(io::ErrorKind::NotFound, "not found");
        let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);
        let src = std::error::Error::source(&err).unwrap();
        assert_eq!(src.to_string(), "not found");
    }

    #[test]
    fn std_error_source_none_by_default() {
        let err = AbpError::new(ErrorCode::Internal, "oops");
        assert!(std::error::Error::source(&err).is_none());
    }

    // -- Unique string representations ----------------------------------

    #[test]
    fn all_codes_have_unique_as_str() {
        let mut seen = HashSet::new();
        for code in ALL_CODES {
            let s = code.as_str();
            assert!(seen.insert(s), "duplicate as_str value: {s}");
        }
        assert_eq!(seen.len(), ALL_CODES.len());
    }

    #[test]
    fn all_codes_display_matches_as_str() {
        for code in ALL_CODES {
            assert_eq!(code.to_string(), code.as_str());
        }
    }

    #[test]
    fn error_code_count() {
        // Ensure we don't silently drop a variant from ALL_CODES.
        assert_eq!(ALL_CODES.len(), 20);
    }

    #[test]
    fn all_codes_serialize_to_as_str() {
        for code in ALL_CODES {
            let json = serde_json::to_string(code).unwrap();
            let expected = format!(r#""{}""#, code.as_str());
            assert_eq!(json, expected, "mismatch for {code:?}");
        }
    }

    #[test]
    fn error_category_display() {
        assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
        assert_eq!(ErrorCategory::Backend.to_string(), "backend");
        assert_eq!(ErrorCategory::Internal.to_string(), "internal");
    }

    #[test]
    fn context_with_nested_json() {
        let err = AbpError::new(ErrorCode::Internal, "nested")
            .with_context("details", serde_json::json!({"a": 1, "b": [2, 3]}));
        assert_eq!(
            err.context["details"],
            serde_json::json!({"a": 1, "b": [2, 3]})
        );
    }
}
