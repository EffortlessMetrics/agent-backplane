#![doc = include_str!("../README.md")]
//!
//! # Error taxonomy
//!
//! `abp-error` provides a unified, machine-readable error system for the
//! entire Agent Backplane workspace.
//!
//! ## Key types
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`ErrorCode`] | Stable `snake_case` tag (e.g. `backend_timeout`) |
//! | [`ErrorCategory`] | Broad family a code belongs to (protocol, backend, …) |
//! | [`AbpError`] | Full error: code + message + cause chain + context |
//! | [`ErrorInfo`] | Lightweight serialisable descriptor (no cause chain) |
//! | [`AbpErrorDto`] | Wire-safe snapshot of an `AbpError` |
//! | [`ErrorStats`] | Tracks occurrence counts by code and category |
//!
//! ## Error codes
//!
//! Every ABP error carries an [`ErrorCode`] variant.  Codes are grouped by
//! [`ErrorCategory`] (Protocol, Backend, Capability, Policy, Workspace, IR,
//! Receipt, Dialect, Config, Mapping, Execution, Contract, Internal).
//! Each code's string representation is guaranteed stable across patch
//! releases.
//!
//! ## Building errors
//!
//! ```
//! use abp_error::{AbpError, ErrorCode};
//!
//! let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30 s")
//!     .with_context("backend", "openai")
//!     .with_context("timeout_ms", 30_000);
//! assert!(err.is_retryable());
//! ```
//!
//! ## Companion modules
//!
//! * [`recovery`] — retry / fallback / degrade / abort strategies per error
//! * [`diagnostic`] — probable-cause analysis and suggested fixes
//! * [`aggregate`] — time-windowed error trending
//! * [`vendor_map`] — maps vendor HTTP errors to `ErrorCode`

#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Error aggregation with time-windowed trending.
pub mod aggregate;
/// Recovery-oriented error categorization with retryability and suggested delays.
pub mod category;
/// Diagnostic generation with probable cause and suggested fixes.
pub mod diagnostic;
/// Typed errors for cross-SDK mapping failures.
pub mod mapping_errors;
/// Enhanced protocol errors for JSONL sidecar communication.
pub mod protocol_errors;
/// Recovery strategies for each error type (Retry, Fallback, Degrade, Abort).
pub mod recovery;
/// Vendor-specific API error wrappers with structured metadata.
pub mod vendor_errors;
/// Vendor-specific error mapping (OpenAI, Anthropic, Gemini).
pub mod vendor_map;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// ErrorCategory
// ---------------------------------------------------------------------------

/// Broad family that an [`ErrorCode`] belongs to.
///
/// # Examples
///
/// ```
/// use abp_error::{ErrorCode, ErrorCategory};
///
/// assert_eq!(ErrorCode::BackendNotFound.category(), ErrorCategory::Backend);
/// assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    /// Cross-dialect mapping / translation errors.
    Mapping,
    /// Task execution errors.
    Execution,
    /// Contract validation errors.
    Contract,
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
            Self::Mapping => "mapping",
            Self::Execution => "execution",
            Self::Contract => "contract",
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
/// Each variant serialises to a `snake_case` string that is guaranteed not to
/// change across patch releases.
///
/// # Examples
///
/// ```
/// use abp_error::ErrorCode;
///
/// let code = ErrorCode::BackendTimeout;
/// assert_eq!(code.as_str(), "backend_timeout");
/// assert_eq!(code.to_string(), "backend timed out");
/// assert_eq!(code.category().to_string(), "backend");
/// ```
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // -- Protocol --
    /// Envelope failed to parse or has missing/invalid fields.
    ProtocolInvalidEnvelope,
    /// The sidecar handshake (hello exchange) failed.
    ProtocolHandshakeFailed,
    /// The `ref_id` field is missing from the envelope.
    ProtocolMissingRefId,
    /// Message arrived in wrong order (e.g. event before hello).
    ProtocolUnexpectedMessage,
    /// Contract version mismatch between host and sidecar.
    ProtocolVersionMismatch,

    // -- Mapping --
    /// A required capability is not supported by the target dialect.
    MappingUnsupportedCapability,
    /// The source and target dialects are incompatible.
    MappingDialectMismatch,
    /// Translation succeeded but information was lost.
    MappingLossyConversion,
    /// A tool call cannot be represented in the target dialect.
    MappingUnmappableTool,

    // -- Backend --
    /// Requested backend name does not exist.
    BackendNotFound,
    /// The backend is temporarily unavailable.
    BackendUnavailable,
    /// Backend did not respond within the configured timeout.
    BackendTimeout,
    /// The backend rejected the request due to rate limiting.
    BackendRateLimited,
    /// Authentication with the backend failed.
    BackendAuthFailed,
    /// The requested model was not found on the backend.
    BackendModelNotFound,
    /// Backend process exited unexpectedly.
    BackendCrashed,

    // -- Execution --
    /// A tool invocation failed during execution.
    ExecutionToolFailed,
    /// An error occurred in the staged workspace.
    ExecutionWorkspaceError,
    /// The operation was denied due to insufficient permissions.
    ExecutionPermissionDenied,

    // -- Contract --
    /// The contract version does not match the expected version.
    ContractVersionMismatch,
    /// The payload violates the contract schema.
    ContractSchemaViolation,
    /// The receipt is structurally invalid or cannot be verified.
    ContractInvalidReceipt,

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
            | Self::ProtocolHandshakeFailed
            | Self::ProtocolMissingRefId
            | Self::ProtocolUnexpectedMessage
            | Self::ProtocolVersionMismatch => ErrorCategory::Protocol,

            Self::MappingUnsupportedCapability
            | Self::MappingDialectMismatch
            | Self::MappingLossyConversion
            | Self::MappingUnmappableTool => ErrorCategory::Mapping,

            Self::BackendNotFound
            | Self::BackendUnavailable
            | Self::BackendTimeout
            | Self::BackendRateLimited
            | Self::BackendAuthFailed
            | Self::BackendModelNotFound
            | Self::BackendCrashed => ErrorCategory::Backend,

            Self::ExecutionToolFailed
            | Self::ExecutionWorkspaceError
            | Self::ExecutionPermissionDenied => ErrorCategory::Execution,

            Self::ContractVersionMismatch
            | Self::ContractSchemaViolation
            | Self::ContractInvalidReceipt => ErrorCategory::Contract,

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

    /// Stable `&'static str` representation of the code in snake_case
    /// (e.g. `"protocol_invalid_envelope"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProtocolInvalidEnvelope => "protocol_invalid_envelope",
            Self::ProtocolHandshakeFailed => "protocol_handshake_failed",
            Self::ProtocolMissingRefId => "protocol_missing_ref_id",
            Self::ProtocolUnexpectedMessage => "protocol_unexpected_message",
            Self::ProtocolVersionMismatch => "protocol_version_mismatch",
            Self::MappingUnsupportedCapability => "mapping_unsupported_capability",
            Self::MappingDialectMismatch => "mapping_dialect_mismatch",
            Self::MappingLossyConversion => "mapping_lossy_conversion",
            Self::MappingUnmappableTool => "mapping_unmappable_tool",
            Self::BackendNotFound => "backend_not_found",
            Self::BackendUnavailable => "backend_unavailable",
            Self::BackendTimeout => "backend_timeout",
            Self::BackendRateLimited => "backend_rate_limited",
            Self::BackendAuthFailed => "backend_auth_failed",
            Self::BackendModelNotFound => "backend_model_not_found",
            Self::BackendCrashed => "backend_crashed",
            Self::ExecutionToolFailed => "execution_tool_failed",
            Self::ExecutionWorkspaceError => "execution_workspace_error",
            Self::ExecutionPermissionDenied => "execution_permission_denied",
            Self::ContractVersionMismatch => "contract_version_mismatch",
            Self::ContractSchemaViolation => "contract_schema_violation",
            Self::ContractInvalidReceipt => "contract_invalid_receipt",
            Self::CapabilityUnsupported => "capability_unsupported",
            Self::CapabilityEmulationFailed => "capability_emulation_failed",
            Self::PolicyDenied => "policy_denied",
            Self::PolicyInvalid => "policy_invalid",
            Self::WorkspaceInitFailed => "workspace_init_failed",
            Self::WorkspaceStagingFailed => "workspace_staging_failed",
            Self::IrLoweringFailed => "ir_lowering_failed",
            Self::IrInvalid => "ir_invalid",
            Self::ReceiptHashMismatch => "receipt_hash_mismatch",
            Self::ReceiptChainBroken => "receipt_chain_broken",
            Self::DialectUnknown => "dialect_unknown",
            Self::DialectMappingFailed => "dialect_mapping_failed",
            Self::ConfigInvalid => "config_invalid",
            Self::Internal => "internal",
        }
    }

    /// Human-readable description of what this error code means.
    pub fn message(&self) -> &'static str {
        match self {
            Self::ProtocolInvalidEnvelope => "envelope failed to parse or has invalid fields",
            Self::ProtocolHandshakeFailed => "sidecar handshake failed",
            Self::ProtocolMissingRefId => "ref_id field is missing from the envelope",
            Self::ProtocolUnexpectedMessage => "message arrived in unexpected order",
            Self::ProtocolVersionMismatch => "protocol version mismatch between host and sidecar",
            Self::MappingUnsupportedCapability => {
                "required capability is not supported by the target dialect"
            }
            Self::MappingDialectMismatch => "source and target dialects are incompatible",
            Self::MappingLossyConversion => "translation succeeded but information was lost",
            Self::MappingUnmappableTool => "tool call cannot be represented in the target dialect",
            Self::BackendNotFound => "requested backend does not exist",
            Self::BackendUnavailable => "backend is temporarily unavailable",
            Self::BackendTimeout => "backend timed out",
            Self::BackendRateLimited => "backend rejected the request due to rate limiting",
            Self::BackendAuthFailed => "authentication with the backend failed",
            Self::BackendModelNotFound => "requested model was not found on the backend",
            Self::BackendCrashed => "backend process exited unexpectedly",
            Self::ExecutionToolFailed => "tool invocation failed during execution",
            Self::ExecutionWorkspaceError => "an error occurred in the staged workspace",
            Self::ExecutionPermissionDenied => "operation denied due to insufficient permissions",
            Self::ContractVersionMismatch => "contract version does not match the expected version",
            Self::ContractSchemaViolation => "payload violates the contract schema",
            Self::ContractInvalidReceipt => "receipt is structurally invalid or cannot be verified",
            Self::CapabilityUnsupported => "required capability is not supported by the backend",
            Self::CapabilityEmulationFailed => "capability emulation layer failed",
            Self::PolicyDenied => "policy rule denied the operation",
            Self::PolicyInvalid => "policy definition is malformed",
            Self::WorkspaceInitFailed => "failed to initialise the staged workspace",
            Self::WorkspaceStagingFailed => "failed to stage files into the workspace",
            Self::IrLoweringFailed => "IR lowering failed",
            Self::IrInvalid => "IR structure is invalid or inconsistent",
            Self::ReceiptHashMismatch => "computed receipt hash does not match the declared hash",
            Self::ReceiptChainBroken => "receipt chain has a gap or out-of-order entry",
            Self::DialectUnknown => "dialect identifier is not recognised",
            Self::DialectMappingFailed => "mapping between dialects failed",
            Self::ConfigInvalid => "configuration file or value is invalid",
            Self::Internal => "unexpected internal error",
        }
    }

    /// Whether this error is potentially transient and the operation may
    /// succeed if retried.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::BackendUnavailable
                | Self::BackendTimeout
                | Self::BackendRateLimited
                | Self::BackendCrashed
        )
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

// ---------------------------------------------------------------------------
// ErrorInfo
// ---------------------------------------------------------------------------

/// Lightweight, serialisable error descriptor.
///
/// Pairs an [`ErrorCode`] with a message, arbitrary details, and a retryability
/// flag.  Use this when you need a structured error payload without the
/// cause-chain baggage of [`AbpError`].
///
/// # Examples
///
/// ```
/// use abp_error::{ErrorCode, ErrorInfo};
///
/// let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out after 30 s")
///     .with_detail("backend", "openai");
/// assert!(info.is_retryable);
/// assert_eq!(info.details["backend"], serde_json::json!("openai"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorInfo {
    /// Machine-readable error code.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
    /// Arbitrary key-value details for diagnostics.
    pub details: BTreeMap<String, serde_json::Value>,
    /// Whether the caller should retry the operation.
    pub is_retryable: bool,
}

impl ErrorInfo {
    /// Create a new [`ErrorInfo`].  `is_retryable` is inferred from the code.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            is_retryable: code.is_retryable(),
            code,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    /// Attach a key-value detail.
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.details.insert(key.into(), v);
        }
        self
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

// ---------------------------------------------------------------------------
// ErrorLocation
// ---------------------------------------------------------------------------

/// Source location where an error was created or enriched.
///
/// Typically populated automatically via the [`abp_err!`] macro.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorLocation {
    /// Source file path.
    pub file: String,
    /// Line number.
    pub line: u32,
    /// Column number.
    pub column: u32,
}

impl ErrorLocation {
    /// Create a new source location.
    pub fn new(file: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            file: file.into(),
            line,
            column,
        }
    }
}

impl fmt::Display for ErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
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
    /// Optional source location where this error was created.
    pub location: Option<ErrorLocation>,
}

impl AbpError {
    /// Create a new error with the given code and message.
    ///
    /// # Examples
    ///
    /// ```
    /// use abp_error::{AbpError, ErrorCode};
    ///
    /// let err = AbpError::new(ErrorCode::Internal, "something broke");
    /// assert_eq!(err.code, ErrorCode::Internal);
    /// assert_eq!(err.message, "something broke");
    /// ```
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source: None,
            context: BTreeMap::new(),
            location: None,
        }
    }

    /// Attach a key-value pair to the diagnostic context.
    ///
    /// The value is converted via [`serde_json::to_value`]; if serialisation
    /// fails, the entry is silently skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use abp_error::{AbpError, ErrorCode};
    ///
    /// let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
    ///     .with_context("backend", "openai")
    ///     .with_context("timeout_ms", 30_000);
    /// assert_eq!(err.context.len(), 2);
    /// ```
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

    /// Shorthand for `self.code.is_retryable()`.
    pub fn is_retryable(&self) -> bool {
        self.code.is_retryable()
    }

    /// Convert this error into an [`ErrorInfo`], discarding the source chain.
    pub fn to_info(&self) -> ErrorInfo {
        ErrorInfo {
            code: self.code,
            message: self.message.clone(),
            details: self.context.clone(),
            is_retryable: self.code.is_retryable(),
        }
    }

    /// Attach source location information.
    pub fn with_location(mut self, location: ErrorLocation) -> Self {
        self.location = Some(location);
        self
    }

    /// Iterate over the error cause chain, starting with this error's source.
    pub fn error_chain(&self) -> ErrorChain<'_> {
        ErrorChain {
            current: self
                .source
                .as_deref()
                .map(|e| e as &(dyn std::error::Error + 'static)),
        }
    }

    /// Count the depth of the error cause chain (0 if no source).
    pub fn chain_depth(&self) -> usize {
        self.error_chain().count()
    }

    /// Check if this error's code matches the given code.
    pub fn matches_code(&self, code: ErrorCode) -> bool {
        self.code == code
    }

    /// Check if this error's category matches the given category.
    pub fn matches_category(&self, category: ErrorCategory) -> bool {
        self.code.category() == category
    }

    /// Check if the context contains the given key.
    pub fn has_context_key(&self, key: &str) -> bool {
        self.context.contains_key(key)
    }

    /// Serialise this error to a JSON string via [`AbpErrorDto`].
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let dto: AbpErrorDto = self.into();
        serde_json::to_string(&dto)
    }

    /// Serialise this error to a pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        let dto: AbpErrorDto = self.into();
        serde_json::to_string_pretty(&dto)
    }

    /// Format the full error chain as a multi-line string.
    pub fn display_chain(&self) -> String {
        let mut parts = vec![self.to_string()];
        for (i, cause) in self.error_chain().enumerate() {
            parts.push(format!("  caused by {}: {}", i, cause));
        }
        parts.join("\n")
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
        if let Some(ref loc) = self.location {
            d.field("location", loc);
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
// From conversions
// ---------------------------------------------------------------------------

impl From<std::io::Error> for AbpError {
    fn from(err: std::io::Error) -> Self {
        AbpError::new(ErrorCode::Internal, err.to_string()).with_source(err)
    }
}

impl From<serde_json::Error> for AbpError {
    fn from(err: serde_json::Error) -> Self {
        AbpError::new(ErrorCode::ProtocolInvalidEnvelope, err.to_string()).with_source(err)
    }
}

impl From<String> for AbpError {
    fn from(msg: String) -> Self {
        AbpError::new(ErrorCode::Internal, msg)
    }
}

impl From<&str> for AbpError {
    fn from(msg: &str) -> Self {
        AbpError::new(ErrorCode::Internal, msg)
    }
}

// ---------------------------------------------------------------------------
// ErrorChain iterator
// ---------------------------------------------------------------------------

/// Iterator over the cause chain of an [`AbpError`].
///
/// Yields each successive [`std::error::Error::source`] until the chain ends.
pub struct ErrorChain<'a> {
    current: Option<&'a (dyn std::error::Error + 'a)>,
}

impl<'a> Iterator for ErrorChain<'a> {
    type Item = &'a (dyn std::error::Error + 'a);

    fn next(&mut self) -> Option<Self::Item> {
        let err = self.current?;
        self.current = err.source();
        Some(err)
    }
}

// ---------------------------------------------------------------------------
// Serialization support
// ---------------------------------------------------------------------------

/// Serialisable snapshot of an [`AbpError`] (without the opaque source).
///
/// # Examples
///
/// ```
/// use abp_error::{AbpError, AbpErrorDto, ErrorCode};
///
/// let err = AbpError::new(ErrorCode::Internal, "oops");
/// let dto: AbpErrorDto = (&err).into();
/// assert_eq!(dto.code, ErrorCode::Internal);
/// assert_eq!(dto.message, "oops");
/// ```
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
    /// Source location where this error was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<ErrorLocation>,
    /// Stringified cause chain (source → source → …).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cause_chain: Vec<String>,
}

impl From<&AbpError> for AbpErrorDto {
    fn from(err: &AbpError) -> Self {
        Self {
            code: err.code,
            message: err.message.clone(),
            context: err.context.clone(),
            source_message: err.source.as_ref().map(|s| s.to_string()),
            location: err.location.clone(),
            cause_chain: err.error_chain().map(|e| e.to_string()).collect(),
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
            location: dto.location,
        }
    }
}

// ---------------------------------------------------------------------------
// ErrorStats
// ---------------------------------------------------------------------------

/// Tracks error occurrence counts by code and category.
///
/// # Examples
///
/// ```
/// use abp_error::{AbpError, ErrorCode, ErrorStats};
///
/// let mut stats = ErrorStats::new();
/// stats.record(&AbpError::new(ErrorCode::BackendTimeout, "t1"));
/// stats.record(&AbpError::new(ErrorCode::BackendTimeout, "t2"));
/// stats.record(&AbpError::new(ErrorCode::PolicyDenied, "d1"));
/// assert_eq!(stats.total(), 3);
/// assert_eq!(stats.count_by_code(ErrorCode::BackendTimeout), 2);
/// ```
#[derive(Debug, Default, Clone)]
pub struct ErrorStats {
    by_code: BTreeMap<ErrorCode, u64>,
    by_category: BTreeMap<ErrorCategory, u64>,
    total: u64,
}

impl ErrorStats {
    /// Create a new empty stats tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an error occurrence.
    pub fn record(&mut self, err: &AbpError) {
        self.record_code(err.code);
    }

    /// Record an error code occurrence.
    pub fn record_code(&mut self, code: ErrorCode) {
        *self.by_code.entry(code).or_default() += 1;
        *self.by_category.entry(code.category()).or_default() += 1;
        self.total += 1;
    }

    /// Get the count for a specific error code.
    pub fn count_by_code(&self, code: ErrorCode) -> u64 {
        self.by_code.get(&code).copied().unwrap_or(0)
    }

    /// Get the count for a specific category.
    pub fn count_by_category(&self, cat: ErrorCategory) -> u64 {
        self.by_category.get(&cat).copied().unwrap_or(0)
    }

    /// Total number of errors recorded.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Reset all counters to zero.
    pub fn reset(&mut self) {
        self.by_code.clear();
        self.by_category.clear();
        self.total = 0;
    }

    /// Get all codes that have been recorded, with their counts.
    pub fn codes(&self) -> &BTreeMap<ErrorCode, u64> {
        &self.by_code
    }

    /// Get all categories that have been recorded, with their counts.
    pub fn categories(&self) -> &BTreeMap<ErrorCategory, u64> {
        &self.by_category
    }
}

// ---------------------------------------------------------------------------
// abp_err! macro
// ---------------------------------------------------------------------------

/// Create an [`AbpError`] with automatic source location capture.
///
/// # Examples
///
/// ```
/// use abp_error::{abp_err, ErrorCode};
///
/// let err = abp_err!(ErrorCode::Internal, "something broke");
/// assert!(err.location.is_some());
/// ```
#[macro_export]
macro_rules! abp_err {
    ($code:expr, $msg:expr) => {
        $crate::AbpError::new($code, $msg)
            .with_location($crate::ErrorLocation::new(file!(), line!(), column!()))
    };
    ($code:expr, $msg:expr, $($key:expr => $val:expr),+ $(,)?) => {
        $crate::AbpError::new($code, $msg)
            .with_location($crate::ErrorLocation::new(file!(), line!(), column!()))
            $(.with_context($key, $val))+
    };
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
        // Protocol
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        // Mapping
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        // Backend
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        // Execution
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        // Contract
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        // Capability
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        // Policy
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        // Workspace
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        // IR
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        // Receipt
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        // Dialect
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        // Config
        ErrorCode::ConfigInvalid,
        // Internal
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
        assert_eq!(err.to_string(), "[backend_not_found] no such backend");
    }

    #[test]
    fn display_with_context() {
        let err =
            AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 5000);
        let s = err.to_string();
        assert!(s.starts_with("[backend_timeout] timed out"));
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
            ErrorCode::ProtocolHandshakeFailed.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolMissingRefId.category(),
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
    fn mapping_codes_categorised() {
        assert_eq!(
            ErrorCode::MappingUnsupportedCapability.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingDialectMismatch.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingLossyConversion.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingUnmappableTool.category(),
            ErrorCategory::Mapping
        );
    }

    #[test]
    fn backend_codes_categorised() {
        assert_eq!(
            ErrorCode::BackendNotFound.category(),
            ErrorCategory::Backend
        );
        assert_eq!(
            ErrorCode::BackendUnavailable.category(),
            ErrorCategory::Backend
        );
        assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
        assert_eq!(
            ErrorCode::BackendRateLimited.category(),
            ErrorCategory::Backend
        );
        assert_eq!(
            ErrorCode::BackendAuthFailed.category(),
            ErrorCategory::Backend
        );
        assert_eq!(
            ErrorCode::BackendModelNotFound.category(),
            ErrorCategory::Backend
        );
        assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
    }

    #[test]
    fn execution_codes_categorised() {
        assert_eq!(
            ErrorCode::ExecutionToolFailed.category(),
            ErrorCategory::Execution
        );
        assert_eq!(
            ErrorCode::ExecutionWorkspaceError.category(),
            ErrorCategory::Execution
        );
        assert_eq!(
            ErrorCode::ExecutionPermissionDenied.category(),
            ErrorCategory::Execution
        );
    }

    #[test]
    fn contract_codes_categorised() {
        assert_eq!(
            ErrorCode::ContractVersionMismatch.category(),
            ErrorCategory::Contract
        );
        assert_eq!(
            ErrorCode::ContractSchemaViolation.category(),
            ErrorCategory::Contract
        );
        assert_eq!(
            ErrorCode::ContractInvalidReceipt.category(),
            ErrorCategory::Contract
        );
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

    // -- Retryability ---------------------------------------------------

    #[test]
    fn retryable_backend_codes() {
        assert!(ErrorCode::BackendUnavailable.is_retryable());
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(ErrorCode::BackendCrashed.is_retryable());
    }

    #[test]
    fn non_retryable_codes() {
        assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
        assert!(!ErrorCode::ProtocolHandshakeFailed.is_retryable());
        assert!(!ErrorCode::MappingDialectMismatch.is_retryable());
        assert!(!ErrorCode::BackendNotFound.is_retryable());
        assert!(!ErrorCode::BackendAuthFailed.is_retryable());
        assert!(!ErrorCode::ExecutionToolFailed.is_retryable());
        assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::Internal.is_retryable());
    }

    #[test]
    fn abp_error_is_retryable_delegates() {
        let retryable = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        assert!(retryable.is_retryable());

        let non_retryable = AbpError::new(ErrorCode::PolicyDenied, "denied");
        assert!(!non_retryable.is_retryable());
    }

    // -- Serialization / Deserialization --------------------------------

    #[test]
    fn error_code_serde_roundtrip_all_variants() {
        for &code in ALL_CODES {
            let json = serde_json::to_string(&code).unwrap();
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, code, "roundtrip failed for {code:?}");
        }
    }

    #[test]
    fn error_code_serializes_as_snake_case() {
        let json = serde_json::to_string(&ErrorCode::ProtocolInvalidEnvelope).unwrap();
        assert_eq!(json, r#""protocol_invalid_envelope""#);

        let json = serde_json::to_string(&ErrorCode::BackendRateLimited).unwrap();
        assert_eq!(json, r#""backend_rate_limited""#);

        let json = serde_json::to_string(&ErrorCode::MappingLossyConversion).unwrap();
        assert_eq!(json, r#""mapping_lossy_conversion""#);
    }

    #[test]
    fn error_category_serde_roundtrip() {
        let categories = [
            ErrorCategory::Protocol,
            ErrorCategory::Backend,
            ErrorCategory::Capability,
            ErrorCategory::Policy,
            ErrorCategory::Workspace,
            ErrorCategory::Ir,
            ErrorCategory::Receipt,
            ErrorCategory::Dialect,
            ErrorCategory::Config,
            ErrorCategory::Mapping,
            ErrorCategory::Execution,
            ErrorCategory::Contract,
            ErrorCategory::Internal,
        ];
        for cat in categories {
            let json = serde_json::to_string(&cat).unwrap();
            let back: ErrorCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(back, cat, "roundtrip failed for {cat:?}");
        }
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
            location: None,
            cause_chain: Vec::new(),
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
    fn all_codes_have_unique_messages() {
        let mut seen = HashSet::new();
        for code in ALL_CODES {
            let m = code.message();
            assert!(seen.insert(m), "duplicate message: {m}");
        }
    }

    #[test]
    fn error_code_count() {
        // Ensure we don't silently drop a variant from ALL_CODES.
        assert_eq!(ALL_CODES.len(), 36);
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
        assert_eq!(ErrorCategory::Mapping.to_string(), "mapping");
        assert_eq!(ErrorCategory::Execution.to_string(), "execution");
        assert_eq!(ErrorCategory::Contract.to_string(), "contract");
        assert_eq!(ErrorCategory::Internal.to_string(), "internal");
    }

    #[test]
    fn error_code_display_is_human_readable() {
        // Display should produce a human-readable message, not the code string.
        let msg = ErrorCode::BackendTimeout.to_string();
        assert_eq!(msg, "backend timed out");
        assert_ne!(msg, "backend_timeout");
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

    // -- ErrorInfo ------------------------------------------------------

    #[test]
    fn error_info_construction() {
        let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
        assert_eq!(info.code, ErrorCode::BackendTimeout);
        assert_eq!(info.message, "timed out");
        assert!(info.is_retryable);
        assert!(info.details.is_empty());
    }

    #[test]
    fn error_info_non_retryable() {
        let info = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
        assert!(!info.is_retryable);
    }

    #[test]
    fn error_info_with_details() {
        let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited")
            .with_detail("retry_after_ms", 5000)
            .with_detail("backend", "openai");
        assert_eq!(info.details.len(), 2);
        assert_eq!(info.details["retry_after_ms"], serde_json::json!(5000));
        assert_eq!(info.details["backend"], serde_json::json!("openai"));
    }

    #[test]
    fn error_info_serde_roundtrip() {
        let info = ErrorInfo::new(ErrorCode::ContractSchemaViolation, "bad schema")
            .with_detail("field", "work_order.task");
        let json = serde_json::to_string(&info).unwrap();
        let back: ErrorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn error_info_display() {
        let info = ErrorInfo::new(ErrorCode::ExecutionToolFailed, "tool crashed");
        assert_eq!(info.to_string(), "[execution_tool_failed] tool crashed");
    }

    #[test]
    fn abp_error_to_info() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("ms", 3000);
        let info = err.to_info();
        assert_eq!(info.code, ErrorCode::BackendTimeout);
        assert_eq!(info.message, "timeout");
        assert!(info.is_retryable);
        assert_eq!(info.details["ms"], serde_json::json!(3000));
    }

    #[test]
    fn error_info_deterministic_serialization() {
        let info = ErrorInfo::new(ErrorCode::Internal, "err")
            .with_detail("z_key", "last")
            .with_detail("a_key", "first");
        let json = serde_json::to_string(&info).unwrap();
        // BTreeMap ensures a_key comes before z_key.
        let a_pos = json.find("a_key").unwrap();
        let z_pos = json.find("z_key").unwrap();
        assert!(a_pos < z_pos);
    }

    // -- From conversions -----------------------------------------------

    #[test]
    fn from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let abp_err: AbpError = io_err.into();
        assert_eq!(abp_err.code, ErrorCode::Internal);
        assert!(abp_err.message.contains("file not found"));
        assert!(abp_err.source.is_some());
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let abp_err: AbpError = json_err.into();
        assert_eq!(abp_err.code, ErrorCode::ProtocolInvalidEnvelope);
        assert!(abp_err.source.is_some());
    }

    // -- Display stability for new variants -----------------------------

    #[test]
    fn display_stability_new_protocol_codes() {
        assert_eq!(
            ErrorCode::ProtocolHandshakeFailed.message(),
            "sidecar handshake failed"
        );
        assert_eq!(
            ErrorCode::ProtocolMissingRefId.message(),
            "ref_id field is missing from the envelope"
        );
    }

    #[test]
    fn display_stability_mapping_codes() {
        assert_eq!(
            ErrorCode::MappingUnsupportedCapability.message(),
            "required capability is not supported by the target dialect"
        );
        assert_eq!(
            ErrorCode::MappingUnmappableTool.message(),
            "tool call cannot be represented in the target dialect"
        );
    }

    #[test]
    fn display_stability_backend_codes() {
        assert_eq!(
            ErrorCode::BackendUnavailable.message(),
            "backend is temporarily unavailable"
        );
        assert_eq!(
            ErrorCode::BackendRateLimited.message(),
            "backend rejected the request due to rate limiting"
        );
        assert_eq!(
            ErrorCode::BackendAuthFailed.message(),
            "authentication with the backend failed"
        );
        assert_eq!(
            ErrorCode::BackendModelNotFound.message(),
            "requested model was not found on the backend"
        );
    }

    #[test]
    fn display_stability_execution_codes() {
        assert_eq!(
            ErrorCode::ExecutionToolFailed.message(),
            "tool invocation failed during execution"
        );
        assert_eq!(
            ErrorCode::ExecutionWorkspaceError.message(),
            "an error occurred in the staged workspace"
        );
        assert_eq!(
            ErrorCode::ExecutionPermissionDenied.message(),
            "operation denied due to insufficient permissions"
        );
    }

    #[test]
    fn display_stability_contract_codes() {
        assert_eq!(
            ErrorCode::ContractVersionMismatch.message(),
            "contract version does not match the expected version"
        );
        assert_eq!(
            ErrorCode::ContractSchemaViolation.message(),
            "payload violates the contract schema"
        );
        assert_eq!(
            ErrorCode::ContractInvalidReceipt.message(),
            "receipt is structurally invalid or cannot be verified"
        );
    }

    // -- ErrorLocation --------------------------------------------------

    #[test]
    fn error_location_display() {
        let loc = ErrorLocation::new("src/main.rs", 42, 5);
        assert_eq!(loc.to_string(), "src/main.rs:42:5");
    }

    #[test]
    fn error_location_serde_roundtrip() {
        let loc = ErrorLocation::new("crates/abp-error/src/lib.rs", 10, 1);
        let json = serde_json::to_string(&loc).unwrap();
        let back: ErrorLocation = serde_json::from_str(&json).unwrap();
        assert_eq!(loc, back);
    }

    #[test]
    fn with_location_builder() {
        let err = AbpError::new(ErrorCode::Internal, "boom")
            .with_location(ErrorLocation::new("test.rs", 1, 1));
        assert!(err.location.is_some());
        let loc = err.location.as_ref().unwrap();
        assert_eq!(loc.file, "test.rs");
        assert_eq!(loc.line, 1);
    }

    #[test]
    fn debug_includes_location() {
        let err = AbpError::new(ErrorCode::Internal, "oops")
            .with_location(ErrorLocation::new("foo.rs", 99, 3));
        let dbg = format!("{err:?}");
        assert!(dbg.contains("foo.rs"));
        assert!(dbg.contains("99"));
    }

    // -- Error chain iteration ------------------------------------------

    #[test]
    fn error_chain_no_source() {
        let err = AbpError::new(ErrorCode::Internal, "no source");
        assert_eq!(err.chain_depth(), 0);
        assert_eq!(err.error_chain().count(), 0);
    }

    #[test]
    fn error_chain_single_source() {
        let src = io::Error::new(io::ErrorKind::NotFound, "inner");
        let err = AbpError::new(ErrorCode::Internal, "outer").with_source(src);
        assert_eq!(err.chain_depth(), 1);
        let chain: Vec<String> = err.error_chain().map(|e| e.to_string()).collect();
        assert_eq!(chain, vec!["inner"]);
    }

    #[test]
    fn display_chain_no_source() {
        let err = AbpError::new(ErrorCode::Internal, "lone error");
        assert_eq!(err.display_chain(), "[internal] lone error");
    }

    #[test]
    fn display_chain_with_source() {
        let src = io::Error::new(io::ErrorKind::TimedOut, "connection timeout");
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out").with_source(src);
        let chain = err.display_chain();
        assert!(chain.contains("[backend_timeout] timed out"));
        assert!(chain.contains("caused by 0: connection timeout"));
    }

    // -- Matching utilities ---------------------------------------------

    #[test]
    fn matches_code_positive() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        assert!(err.matches_code(ErrorCode::PolicyDenied));
    }

    #[test]
    fn matches_code_negative() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        assert!(!err.matches_code(ErrorCode::Internal));
    }

    #[test]
    fn matches_category_positive() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        assert!(err.matches_category(ErrorCategory::Backend));
    }

    #[test]
    fn matches_category_negative() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        assert!(!err.matches_category(ErrorCategory::Policy));
    }

    #[test]
    fn has_context_key_positive() {
        let err = AbpError::new(ErrorCode::Internal, "err").with_context("backend", "openai");
        assert!(err.has_context_key("backend"));
    }

    #[test]
    fn has_context_key_negative() {
        let err = AbpError::new(ErrorCode::Internal, "err");
        assert!(!err.has_context_key("nonexistent"));
    }

    // -- JSON serialization on AbpError ---------------------------------

    #[test]
    fn to_json_basic() {
        let err = AbpError::new(ErrorCode::Internal, "oops");
        let json = err.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["code"], "internal");
        assert_eq!(parsed["message"], "oops");
    }

    #[test]
    fn to_json_with_context_and_source() {
        let src = io::Error::new(io::ErrorKind::NotFound, "file gone");
        let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging failed")
            .with_context("path", "/tmp/ws")
            .with_source(src);
        let json = err.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["context"]["path"], "/tmp/ws");
        assert_eq!(parsed["source_message"], "file gone");
        let chain = parsed["cause_chain"].as_array().unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0], "file gone");
    }

    #[test]
    fn to_json_pretty_contains_newlines() {
        let err = AbpError::new(ErrorCode::Internal, "test");
        let pretty = err.to_json_pretty().unwrap();
        assert!(pretty.contains('\n'));
    }

    #[test]
    fn to_json_with_location() {
        let err = AbpError::new(ErrorCode::Internal, "located").with_location(ErrorLocation::new(
            "src/lib.rs",
            10,
            5,
        ));
        let json = err.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["location"]["file"], "src/lib.rs");
        assert_eq!(parsed["location"]["line"], 10);
        assert_eq!(parsed["location"]["column"], 5);
    }

    // -- DTO with new fields --------------------------------------------

    #[test]
    fn dto_preserves_location() {
        let err = AbpError::new(ErrorCode::Internal, "test")
            .with_location(ErrorLocation::new("x.rs", 5, 1));
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.location, Some(ErrorLocation::new("x.rs", 5, 1)));
    }

    #[test]
    fn dto_preserves_cause_chain() {
        let src = io::Error::new(io::ErrorKind::BrokenPipe, "broken");
        let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.cause_chain, vec!["broken"]);
    }

    #[test]
    fn dto_roundtrip_with_location() {
        let err = AbpError::new(ErrorCode::Internal, "loc")
            .with_location(ErrorLocation::new("a.rs", 1, 2));
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
    }

    #[test]
    fn dto_empty_cause_chain_not_serialized() {
        let err = AbpError::new(ErrorCode::Internal, "no cause");
        let json = err.to_json().unwrap();
        assert!(!json.contains("cause_chain"));
    }

    #[test]
    fn dto_none_location_not_serialized() {
        let err = AbpError::new(ErrorCode::Internal, "no loc");
        let json = err.to_json().unwrap();
        assert!(!json.contains("location"));
    }

    // -- From conversions (new) -----------------------------------------

    #[test]
    fn from_string() {
        let err: AbpError = String::from("string error").into();
        assert_eq!(err.code, ErrorCode::Internal);
        assert_eq!(err.message, "string error");
    }

    #[test]
    fn from_str() {
        let err: AbpError = "str error".into();
        assert_eq!(err.code, ErrorCode::Internal);
        assert_eq!(err.message, "str error");
    }

    // -- ErrorStats -----------------------------------------------------

    #[test]
    fn stats_empty() {
        let stats = ErrorStats::new();
        assert_eq!(stats.total(), 0);
        assert_eq!(stats.count_by_code(ErrorCode::Internal), 0);
        assert_eq!(stats.count_by_category(ErrorCategory::Internal), 0);
    }

    #[test]
    fn stats_record_and_count() {
        let mut stats = ErrorStats::new();
        stats.record(&AbpError::new(ErrorCode::BackendTimeout, "t1"));
        stats.record(&AbpError::new(ErrorCode::BackendTimeout, "t2"));
        stats.record(&AbpError::new(ErrorCode::PolicyDenied, "d1"));
        assert_eq!(stats.total(), 3);
        assert_eq!(stats.count_by_code(ErrorCode::BackendTimeout), 2);
        assert_eq!(stats.count_by_code(ErrorCode::PolicyDenied), 1);
        assert_eq!(stats.count_by_category(ErrorCategory::Backend), 2);
        assert_eq!(stats.count_by_category(ErrorCategory::Policy), 1);
    }

    #[test]
    fn stats_record_code() {
        let mut stats = ErrorStats::new();
        stats.record_code(ErrorCode::Internal);
        stats.record_code(ErrorCode::Internal);
        assert_eq!(stats.count_by_code(ErrorCode::Internal), 2);
        assert_eq!(stats.count_by_category(ErrorCategory::Internal), 2);
        assert_eq!(stats.total(), 2);
    }

    #[test]
    fn stats_reset() {
        let mut stats = ErrorStats::new();
        stats.record_code(ErrorCode::BackendTimeout);
        stats.record_code(ErrorCode::PolicyDenied);
        assert_eq!(stats.total(), 2);
        stats.reset();
        assert_eq!(stats.total(), 0);
        assert_eq!(stats.count_by_code(ErrorCode::BackendTimeout), 0);
        assert!(stats.codes().is_empty());
        assert!(stats.categories().is_empty());
    }

    #[test]
    fn stats_codes_and_categories() {
        let mut stats = ErrorStats::new();
        stats.record_code(ErrorCode::BackendTimeout);
        stats.record_code(ErrorCode::BackendCrashed);
        assert_eq!(stats.codes().len(), 2);
        assert_eq!(stats.categories().len(), 1);
        assert_eq!(stats.count_by_category(ErrorCategory::Backend), 2);
    }

    // -- abp_err! macro -------------------------------------------------

    #[test]
    fn abp_err_macro_captures_location() {
        let err = abp_err!(ErrorCode::Internal, "macro error");
        assert!(err.location.is_some());
        let loc = err.location.as_ref().unwrap();
        assert!(loc.file.contains("lib.rs"));
        assert!(loc.line > 0);
    }

    #[test]
    fn abp_err_macro_with_context() {
        let err = abp_err!(
            ErrorCode::BackendTimeout,
            "timed out",
            "backend" => "openai",
            "timeout_ms" => 5000
        );
        assert!(err.location.is_some());
        assert_eq!(err.context["backend"], serde_json::json!("openai"));
        assert_eq!(err.context["timeout_ms"], serde_json::json!(5000));
    }

    // -- Ord derives ----------------------------------------------------

    #[test]
    fn error_category_ord() {
        let mut cats = [
            ErrorCategory::Internal,
            ErrorCategory::Backend,
            ErrorCategory::Protocol,
        ];
        cats.sort();
        assert_eq!(cats.len(), 3);
    }

    #[test]
    fn error_code_ord() {
        let mut codes = [
            ErrorCode::Internal,
            ErrorCode::BackendTimeout,
            ErrorCode::ProtocolInvalidEnvelope,
        ];
        codes.sort();
        assert_eq!(codes.len(), 3);
    }
}
