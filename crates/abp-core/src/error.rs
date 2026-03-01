// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]

//! Comprehensive error catalog for the Agent Backplane.
//!
//! Every error code follows the pattern `ABP-X###` where `X` is the category
//! letter and `###` is a three-digit number:
//!
//! - **C** — Contract errors
//! - **P** — Protocol errors
//! - **L** — Policy errors
//! - **R** — Runtime errors
//! - **S** — System errors

use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// ErrorCode enum
// ---------------------------------------------------------------------------

/// Enumeration of all ABP error codes, organized by category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // ── Contract (C) ──────────────────────────────────────────────────
    /// The contract version string is invalid or unsupported.
    InvalidContractVersion,
    /// The work order could not be parsed or is structurally invalid.
    MalformedWorkOrder,
    /// The receipt could not be parsed or is structurally invalid.
    MalformedReceipt,
    /// The receipt hash does not match the recomputed canonical hash.
    InvalidHash,
    /// A required field is missing or empty.
    MissingRequiredField,
    /// The work order ID is invalid (e.g. nil UUID).
    InvalidWorkOrderId,
    /// The run ID is invalid (e.g. nil UUID).
    InvalidRunId,
    /// A work order with the same ID already exists.
    DuplicateWorkOrderId,
    /// The contract version does not match the expected version.
    ContractVersionMismatch,
    /// The outcome value is invalid for the current state.
    InvalidOutcome,
    /// The execution lane value is not recognized.
    InvalidExecutionLane,
    /// The execution mode value is not recognized.
    InvalidExecutionMode,

    // ── Protocol (P) ──────────────────────────────────────────────────
    /// The JSONL envelope could not be parsed.
    InvalidEnvelope,
    /// The sidecar handshake (hello exchange) failed.
    HandshakeFailed,
    /// Received a message not expected in the current protocol state.
    UnexpectedMessage,
    /// The protocol version does not match between host and sidecar.
    VersionMismatch,
    /// A JSONL line could not be parsed as valid JSON.
    MalformedJsonl,
    /// The `ref_id` does not match the active run.
    InvalidRefId,
    /// The envelope exceeds the maximum permitted size.
    EnvelopeTooLarge,
    /// A required field is missing from the envelope.
    MissingEnvelopeField,
    /// The envelope discriminator tag is unknown.
    InvalidEnvelopeTag,
    /// The protocol exchange timed out waiting for a response.
    ProtocolTimeout,
    /// A second hello envelope was received on the same connection.
    DuplicateHello,
    /// A final envelope was received without an active run.
    UnexpectedFinal,

    // ── Policy (L) ────────────────────────────────────────────────────
    /// The requested tool is on the deny list.
    ToolDenied,
    /// Read access to the path is denied by policy.
    ReadDenied,
    /// Write access to the path is denied by policy.
    WriteDenied,
    /// The policy profile could not be compiled into a policy engine.
    PolicyCompilationFailed,
    /// The backend does not support a required capability.
    CapabilityNotSupported,
    /// Network access to the domain is denied by policy.
    NetworkDenied,
    /// The tool requires explicit approval before execution.
    ApprovalRequired,
    /// A general policy violation was detected.
    PolicyViolation,
    /// A glob pattern in the policy is syntactically invalid.
    InvalidGlobPattern,
    /// The tool name is not recognized by the backend.
    ToolNotRegistered,
    /// The path attempts to escape the workspace root.
    PathTraversal,

    // ── Runtime (R) ───────────────────────────────────────────────────
    /// The selected backend is not available or not registered.
    BackendUnavailable,
    /// The backend did not respond within the timeout period.
    BackendTimeout,
    /// Failed to create the staged workspace copy.
    WorkspaceStagingFailed,
    /// The agent event stream was closed unexpectedly.
    EventStreamClosed,
    /// The run was cancelled by the user or control plane.
    RunCancelled,
    /// The sidecar process exited unexpectedly.
    SidecarCrashed,
    /// Failed to spawn the sidecar process.
    SidecarSpawnFailed,
    /// Failed to clean up the staged workspace after the run.
    WorkspaceCleanupFailed,
    /// The run exceeded the maximum number of allowed turns.
    MaxTurnsExceeded,
    /// The run exceeded the configured cost budget.
    BudgetExceeded,
    /// The backend identity does not match the expected backend.
    BackendMismatch,
    /// The run has already been marked as completed.
    RunAlreadyCompleted,
    /// No backend has been registered for the requested identifier.
    NoBackendRegistered,

    // ── System (S) ────────────────────────────────────────────────────
    /// An I/O operation failed.
    IoError,
    /// JSON serialization or deserialization failed.
    SerializationError,
    /// An unexpected internal error occurred.
    InternalError,
    /// The configuration is invalid or incomplete.
    ConfigurationError,
    /// A system resource (memory, file handles, etc.) is exhausted.
    ResourceExhausted,
    /// A byte sequence could not be decoded as valid UTF-8.
    Utf8Error,
    /// An async task failed to join.
    TaskJoinError,
    /// An internal channel was closed unexpectedly.
    ChannelClosed,
    /// An argument value is invalid.
    InvalidArgument,
    /// The operation was denied due to insufficient permissions.
    PermissionDenied,
    /// The requested feature is not yet implemented.
    NotImplemented,
}

impl ErrorCode {
    /// Machine-readable code string (e.g. `"ABP-C001"`).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            // Contract
            Self::InvalidContractVersion => "ABP-C001",
            Self::MalformedWorkOrder => "ABP-C002",
            Self::MalformedReceipt => "ABP-C003",
            Self::InvalidHash => "ABP-C004",
            Self::MissingRequiredField => "ABP-C005",
            Self::InvalidWorkOrderId => "ABP-C006",
            Self::InvalidRunId => "ABP-C007",
            Self::DuplicateWorkOrderId => "ABP-C008",
            Self::ContractVersionMismatch => "ABP-C009",
            Self::InvalidOutcome => "ABP-C010",
            Self::InvalidExecutionLane => "ABP-C011",
            Self::InvalidExecutionMode => "ABP-C012",

            // Protocol
            Self::InvalidEnvelope => "ABP-P001",
            Self::HandshakeFailed => "ABP-P002",
            Self::UnexpectedMessage => "ABP-P003",
            Self::VersionMismatch => "ABP-P004",
            Self::MalformedJsonl => "ABP-P005",
            Self::InvalidRefId => "ABP-P006",
            Self::EnvelopeTooLarge => "ABP-P007",
            Self::MissingEnvelopeField => "ABP-P008",
            Self::InvalidEnvelopeTag => "ABP-P009",
            Self::ProtocolTimeout => "ABP-P010",
            Self::DuplicateHello => "ABP-P011",
            Self::UnexpectedFinal => "ABP-P012",

            // Policy
            Self::ToolDenied => "ABP-L001",
            Self::ReadDenied => "ABP-L002",
            Self::WriteDenied => "ABP-L003",
            Self::PolicyCompilationFailed => "ABP-L004",
            Self::CapabilityNotSupported => "ABP-L005",
            Self::NetworkDenied => "ABP-L006",
            Self::ApprovalRequired => "ABP-L007",
            Self::PolicyViolation => "ABP-L008",
            Self::InvalidGlobPattern => "ABP-L009",
            Self::ToolNotRegistered => "ABP-L010",
            Self::PathTraversal => "ABP-L011",

            // Runtime
            Self::BackendUnavailable => "ABP-R001",
            Self::BackendTimeout => "ABP-R002",
            Self::WorkspaceStagingFailed => "ABP-R003",
            Self::EventStreamClosed => "ABP-R004",
            Self::RunCancelled => "ABP-R005",
            Self::SidecarCrashed => "ABP-R006",
            Self::SidecarSpawnFailed => "ABP-R007",
            Self::WorkspaceCleanupFailed => "ABP-R008",
            Self::MaxTurnsExceeded => "ABP-R009",
            Self::BudgetExceeded => "ABP-R010",
            Self::BackendMismatch => "ABP-R011",
            Self::RunAlreadyCompleted => "ABP-R012",
            Self::NoBackendRegistered => "ABP-R013",

            // System
            Self::IoError => "ABP-S001",
            Self::SerializationError => "ABP-S002",
            Self::InternalError => "ABP-S003",
            Self::ConfigurationError => "ABP-S004",
            Self::ResourceExhausted => "ABP-S005",
            Self::Utf8Error => "ABP-S006",
            Self::TaskJoinError => "ABP-S007",
            Self::ChannelClosed => "ABP-S008",
            Self::InvalidArgument => "ABP-S009",
            Self::PermissionDenied => "ABP-S010",
            Self::NotImplemented => "ABP-S011",
        }
    }

    /// Human-readable category label.
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self {
            Self::InvalidContractVersion
            | Self::MalformedWorkOrder
            | Self::MalformedReceipt
            | Self::InvalidHash
            | Self::MissingRequiredField
            | Self::InvalidWorkOrderId
            | Self::InvalidRunId
            | Self::DuplicateWorkOrderId
            | Self::ContractVersionMismatch
            | Self::InvalidOutcome
            | Self::InvalidExecutionLane
            | Self::InvalidExecutionMode => "contract",

            Self::InvalidEnvelope
            | Self::HandshakeFailed
            | Self::UnexpectedMessage
            | Self::VersionMismatch
            | Self::MalformedJsonl
            | Self::InvalidRefId
            | Self::EnvelopeTooLarge
            | Self::MissingEnvelopeField
            | Self::InvalidEnvelopeTag
            | Self::ProtocolTimeout
            | Self::DuplicateHello
            | Self::UnexpectedFinal => "protocol",

            Self::ToolDenied
            | Self::ReadDenied
            | Self::WriteDenied
            | Self::PolicyCompilationFailed
            | Self::CapabilityNotSupported
            | Self::NetworkDenied
            | Self::ApprovalRequired
            | Self::PolicyViolation
            | Self::InvalidGlobPattern
            | Self::ToolNotRegistered
            | Self::PathTraversal => "policy",

            Self::BackendUnavailable
            | Self::BackendTimeout
            | Self::WorkspaceStagingFailed
            | Self::EventStreamClosed
            | Self::RunCancelled
            | Self::SidecarCrashed
            | Self::SidecarSpawnFailed
            | Self::WorkspaceCleanupFailed
            | Self::MaxTurnsExceeded
            | Self::BudgetExceeded
            | Self::BackendMismatch
            | Self::RunAlreadyCompleted
            | Self::NoBackendRegistered => "runtime",

            Self::IoError
            | Self::SerializationError
            | Self::InternalError
            | Self::ConfigurationError
            | Self::ResourceExhausted
            | Self::Utf8Error
            | Self::TaskJoinError
            | Self::ChannelClosed
            | Self::InvalidArgument
            | Self::PermissionDenied
            | Self::NotImplemented => "system",
        }
    }

    /// Short human-readable description of this error code.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            // Contract
            Self::InvalidContractVersion => "The contract version string is invalid or unsupported",
            Self::MalformedWorkOrder => {
                "The work order could not be parsed or is structurally invalid"
            }
            Self::MalformedReceipt => "The receipt could not be parsed or is structurally invalid",
            Self::InvalidHash => "The receipt hash does not match the recomputed canonical hash",
            Self::MissingRequiredField => "A required field is missing or empty",
            Self::InvalidWorkOrderId => "The work order ID is invalid (e.g. nil UUID)",
            Self::InvalidRunId => "The run ID is invalid (e.g. nil UUID)",
            Self::DuplicateWorkOrderId => "A work order with the same ID already exists",
            Self::ContractVersionMismatch => {
                "The contract version does not match the expected version"
            }
            Self::InvalidOutcome => "The outcome value is invalid for the current state",
            Self::InvalidExecutionLane => "The execution lane value is not recognized",
            Self::InvalidExecutionMode => "The execution mode value is not recognized",

            // Protocol
            Self::InvalidEnvelope => "The JSONL envelope could not be parsed",
            Self::HandshakeFailed => "The sidecar handshake (hello exchange) failed",
            Self::UnexpectedMessage => {
                "Received a message not expected in the current protocol state"
            }
            Self::VersionMismatch => "The protocol version does not match between host and sidecar",
            Self::MalformedJsonl => "A JSONL line could not be parsed as valid JSON",
            Self::InvalidRefId => "The ref_id does not match the active run",
            Self::EnvelopeTooLarge => "The envelope exceeds the maximum permitted size",
            Self::MissingEnvelopeField => "A required field is missing from the envelope",
            Self::InvalidEnvelopeTag => "The envelope discriminator tag is unknown",
            Self::ProtocolTimeout => "The protocol exchange timed out waiting for a response",
            Self::DuplicateHello => "A second hello envelope was received on the same connection",
            Self::UnexpectedFinal => "A final envelope was received without an active run",

            // Policy
            Self::ToolDenied => "The requested tool is on the deny list",
            Self::ReadDenied => "Read access to the path is denied by policy",
            Self::WriteDenied => "Write access to the path is denied by policy",
            Self::PolicyCompilationFailed => {
                "The policy profile could not be compiled into a policy engine"
            }
            Self::CapabilityNotSupported => "The backend does not support a required capability",
            Self::NetworkDenied => "Network access to the domain is denied by policy",
            Self::ApprovalRequired => "The tool requires explicit approval before execution",
            Self::PolicyViolation => "A general policy violation was detected",
            Self::InvalidGlobPattern => "A glob pattern in the policy is syntactically invalid",
            Self::ToolNotRegistered => "The tool name is not recognized by the backend",
            Self::PathTraversal => "The path attempts to escape the workspace root",

            // Runtime
            Self::BackendUnavailable => "The selected backend is not available or not registered",
            Self::BackendTimeout => "The backend did not respond within the timeout period",
            Self::WorkspaceStagingFailed => "Failed to create the staged workspace copy",
            Self::EventStreamClosed => "The agent event stream was closed unexpectedly",
            Self::RunCancelled => "The run was cancelled by the user or control plane",
            Self::SidecarCrashed => "The sidecar process exited unexpectedly",
            Self::SidecarSpawnFailed => "Failed to spawn the sidecar process",
            Self::WorkspaceCleanupFailed => "Failed to clean up the staged workspace after the run",
            Self::MaxTurnsExceeded => "The run exceeded the maximum number of allowed turns",
            Self::BudgetExceeded => "The run exceeded the configured cost budget",
            Self::BackendMismatch => "The backend identity does not match the expected backend",
            Self::RunAlreadyCompleted => "The run has already been marked as completed",
            Self::NoBackendRegistered => {
                "No backend has been registered for the requested identifier"
            }

            // System
            Self::IoError => "An I/O operation failed",
            Self::SerializationError => "JSON serialization or deserialization failed",
            Self::InternalError => "An unexpected internal error occurred",
            Self::ConfigurationError => "The configuration is invalid or incomplete",
            Self::ResourceExhausted => {
                "A system resource (memory, file handles, etc.) is exhausted"
            }
            Self::Utf8Error => "A byte sequence could not be decoded as valid UTF-8",
            Self::TaskJoinError => "An async task failed to join",
            Self::ChannelClosed => "An internal channel was closed unexpectedly",
            Self::InvalidArgument => "An argument value is invalid",
            Self::PermissionDenied => "The operation was denied due to insufficient permissions",
            Self::NotImplemented => "The requested feature is not yet implemented",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

impl std::error::Error for ErrorCode {}

// ---------------------------------------------------------------------------
// Complete list for catalog operations
// ---------------------------------------------------------------------------

/// All error codes, in definition order.
const ALL_CODES: &[ErrorCode] = &[
    // Contract
    ErrorCode::InvalidContractVersion,
    ErrorCode::MalformedWorkOrder,
    ErrorCode::MalformedReceipt,
    ErrorCode::InvalidHash,
    ErrorCode::MissingRequiredField,
    ErrorCode::InvalidWorkOrderId,
    ErrorCode::InvalidRunId,
    ErrorCode::DuplicateWorkOrderId,
    ErrorCode::ContractVersionMismatch,
    ErrorCode::InvalidOutcome,
    ErrorCode::InvalidExecutionLane,
    ErrorCode::InvalidExecutionMode,
    // Protocol
    ErrorCode::InvalidEnvelope,
    ErrorCode::HandshakeFailed,
    ErrorCode::UnexpectedMessage,
    ErrorCode::VersionMismatch,
    ErrorCode::MalformedJsonl,
    ErrorCode::InvalidRefId,
    ErrorCode::EnvelopeTooLarge,
    ErrorCode::MissingEnvelopeField,
    ErrorCode::InvalidEnvelopeTag,
    ErrorCode::ProtocolTimeout,
    ErrorCode::DuplicateHello,
    ErrorCode::UnexpectedFinal,
    // Policy
    ErrorCode::ToolDenied,
    ErrorCode::ReadDenied,
    ErrorCode::WriteDenied,
    ErrorCode::PolicyCompilationFailed,
    ErrorCode::CapabilityNotSupported,
    ErrorCode::NetworkDenied,
    ErrorCode::ApprovalRequired,
    ErrorCode::PolicyViolation,
    ErrorCode::InvalidGlobPattern,
    ErrorCode::ToolNotRegistered,
    ErrorCode::PathTraversal,
    // Runtime
    ErrorCode::BackendUnavailable,
    ErrorCode::BackendTimeout,
    ErrorCode::WorkspaceStagingFailed,
    ErrorCode::EventStreamClosed,
    ErrorCode::RunCancelled,
    ErrorCode::SidecarCrashed,
    ErrorCode::SidecarSpawnFailed,
    ErrorCode::WorkspaceCleanupFailed,
    ErrorCode::MaxTurnsExceeded,
    ErrorCode::BudgetExceeded,
    ErrorCode::BackendMismatch,
    ErrorCode::RunAlreadyCompleted,
    ErrorCode::NoBackendRegistered,
    // System
    ErrorCode::IoError,
    ErrorCode::SerializationError,
    ErrorCode::InternalError,
    ErrorCode::ConfigurationError,
    ErrorCode::ResourceExhausted,
    ErrorCode::Utf8Error,
    ErrorCode::TaskJoinError,
    ErrorCode::ChannelClosed,
    ErrorCode::InvalidArgument,
    ErrorCode::PermissionDenied,
    ErrorCode::NotImplemented,
];

// ---------------------------------------------------------------------------
// ErrorInfo
// ---------------------------------------------------------------------------

/// Rich error value carrying an [`ErrorCode`], a human message, optional
/// structured context, and an optional source error.
pub struct ErrorInfo {
    /// The catalog error code.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
    /// Arbitrary key-value context (deterministic ordering).
    pub context: BTreeMap<String, String>,
    /// Optional underlying cause.
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl ErrorInfo {
    /// Start building an [`ErrorInfo`] for the given code.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: BTreeMap::new(),
            source: None,
        }
    }

    /// Add a key-value context entry.
    #[must_use]
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Attach an underlying source error.
    #[must_use]
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.code(), self.message)?;
        if !self.context.is_empty() {
            write!(f, " (")?;
            let mut first = true;
            for (k, v) in &self.context {
                if !first {
                    write!(f, ", ")?;
                }
                write!(f, "{k}={v}")?;
                first = false;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ErrorInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorInfo")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("context", &self.context)
            .field("source", &self.source.as_ref().map(|e| e.to_string()))
            .finish()
    }
}

impl std::error::Error for ErrorInfo {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

// ---------------------------------------------------------------------------
// ErrorCatalog
// ---------------------------------------------------------------------------

/// Static catalog providing lookup and enumeration of [`ErrorCode`]s.
pub struct ErrorCatalog;

impl ErrorCatalog {
    /// Look up an [`ErrorCode`] by its code string (e.g. `"ABP-C001"`).
    #[must_use]
    pub fn lookup(code: &str) -> Option<ErrorCode> {
        ALL_CODES.iter().find(|c| c.code() == code).copied()
    }

    /// Return every defined [`ErrorCode`].
    #[must_use]
    pub fn all() -> Vec<ErrorCode> {
        ALL_CODES.to_vec()
    }

    /// Return all codes belonging to `category` (e.g. `"contract"`).
    #[must_use]
    pub fn by_category(category: &str) -> Vec<ErrorCode> {
        ALL_CODES
            .iter()
            .filter(|c| c.category() == category)
            .copied()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// MappingError — typed error taxonomy for dialect mapping failures
// ---------------------------------------------------------------------------

/// Categorization of mapping errors by severity.
///
/// - **Fatal**: request cannot proceed at all.
/// - **Degraded**: request proceeds but with known information loss (must be labeled in receipt).
/// - **Emulated**: behavior is synthetically replicated (must be labeled in receipt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MappingErrorKind {
    /// The mapping failure is fatal — the request cannot proceed.
    Fatal,
    /// The request proceeds but with known information loss.
    Degraded,
    /// The feature is synthetically replicated via emulation.
    Emulated,
}

impl fmt::Display for MappingErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fatal => write!(f, "fatal"),
            Self::Degraded => write!(f, "degraded"),
            Self::Emulated => write!(f, "emulated"),
        }
    }
}

/// Typed error for dialect mapping failures.
///
/// Each variant carries structured context about what went wrong during
/// translation between agent dialects. Every variant has a stable error
/// code (e.g. `ABP_E_FIDELITY_LOSS`) and a [`MappingErrorKind`] that
/// indicates whether the failure is fatal, degraded, or emulated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MappingError {
    /// A field cannot be faithfully translated between dialects.
    #[error("[{code}] fidelity loss mapping '{field}' from {source_dialect} to {target_dialect}: {detail}", code = MappingError::FIDELITY_LOSS_CODE)]
    FidelityLoss {
        /// The field that could not be faithfully translated.
        field: String,
        /// The source dialect.
        source_dialect: String,
        /// The target dialect.
        target_dialect: String,
        /// Human-readable explanation.
        detail: String,
    },

    /// The target dialect does not support a requested capability.
    #[error("[{code}] unsupported capability '{capability}' in dialect {dialect}", code = MappingError::UNSUPPORTED_CAP_CODE)]
    UnsupportedCapability {
        /// The capability that is not supported.
        capability: String,
        /// The dialect that lacks support.
        dialect: String,
    },

    /// A feature requires emulation (labeled, never silent).
    #[error("[{code}] emulation required for '{feature}': {detail}", code = MappingError::EMULATION_REQUIRED_CODE)]
    EmulationRequired {
        /// The feature being emulated.
        feature: String,
        /// Human-readable explanation.
        detail: String,
    },

    /// The requested model is not available in the target dialect.
    #[error("[{code}] model '{requested}' not available in dialect {dialect}{}", suggestion.as_ref().map(|s| format!("; try {s}")).unwrap_or_default(), code = MappingError::INCOMPATIBLE_MODEL_CODE)]
    IncompatibleModel {
        /// The model that was requested.
        requested: String,
        /// The dialect that does not support the model.
        dialect: String,
        /// Optional alternative model suggestion.
        suggestion: Option<String>,
    },

    /// A specific parameter cannot be translated to the target dialect.
    #[error("[{code}] parameter '{parameter}' (value: {value}) not mappable to dialect {dialect}", code = MappingError::PARAM_NOT_MAPPABLE_CODE)]
    ParameterNotMappable {
        /// The parameter name.
        parameter: String,
        /// The parameter value that cannot be mapped.
        value: String,
        /// The target dialect.
        dialect: String,
    },

    /// The target dialect does not support streaming.
    #[error("[{code}] streaming not supported by dialect {dialect}", code = MappingError::STREAMING_UNSUPPORTED_CODE)]
    StreamingUnsupported {
        /// The dialect that does not support streaming.
        dialect: String,
    },
}

impl MappingError {
    /// Stable error code for [`MappingError::FidelityLoss`].
    pub const FIDELITY_LOSS_CODE: &'static str = "ABP_E_FIDELITY_LOSS";
    /// Stable error code for [`MappingError::UnsupportedCapability`].
    pub const UNSUPPORTED_CAP_CODE: &'static str = "ABP_E_UNSUPPORTED_CAP";
    /// Stable error code for [`MappingError::EmulationRequired`].
    pub const EMULATION_REQUIRED_CODE: &'static str = "ABP_E_EMULATION_REQUIRED";
    /// Stable error code for [`MappingError::IncompatibleModel`].
    pub const INCOMPATIBLE_MODEL_CODE: &'static str = "ABP_E_INCOMPATIBLE_MODEL";
    /// Stable error code for [`MappingError::ParameterNotMappable`].
    pub const PARAM_NOT_MAPPABLE_CODE: &'static str = "ABP_E_PARAM_NOT_MAPPABLE";
    /// Stable error code for [`MappingError::StreamingUnsupported`].
    pub const STREAMING_UNSUPPORTED_CODE: &'static str = "ABP_E_STREAMING_UNSUPPORTED";

    /// Returns the stable error code string for this variant.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::FidelityLoss { .. } => Self::FIDELITY_LOSS_CODE,
            Self::UnsupportedCapability { .. } => Self::UNSUPPORTED_CAP_CODE,
            Self::EmulationRequired { .. } => Self::EMULATION_REQUIRED_CODE,
            Self::IncompatibleModel { .. } => Self::INCOMPATIBLE_MODEL_CODE,
            Self::ParameterNotMappable { .. } => Self::PARAM_NOT_MAPPABLE_CODE,
            Self::StreamingUnsupported { .. } => Self::STREAMING_UNSUPPORTED_CODE,
        }
    }

    /// Returns the [`MappingErrorKind`] categorization for this error.
    #[must_use]
    pub fn kind(&self) -> MappingErrorKind {
        match self {
            Self::FidelityLoss { .. } => MappingErrorKind::Degraded,
            Self::UnsupportedCapability { .. } => MappingErrorKind::Fatal,
            Self::EmulationRequired { .. } => MappingErrorKind::Emulated,
            Self::IncompatibleModel { .. } => MappingErrorKind::Fatal,
            Self::ParameterNotMappable { .. } => MappingErrorKind::Degraded,
            Self::StreamingUnsupported { .. } => MappingErrorKind::Fatal,
        }
    }

    /// Returns `true` if this error is fatal (the request cannot proceed).
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        self.kind() == MappingErrorKind::Fatal
    }

    /// Returns `true` if this error represents degraded operation.
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        self.kind() == MappingErrorKind::Degraded
    }

    /// Returns `true` if this error represents emulated behavior.
    #[must_use]
    pub fn is_emulated(&self) -> bool {
        self.kind() == MappingErrorKind::Emulated
    }
}

/// Convenience result type for mapping operations.
pub type MappingResult<T> = Result<T, MappingError>;
