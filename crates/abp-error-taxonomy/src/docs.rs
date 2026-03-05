//! Error code documentation — structured docs and examples for every
//! [`ErrorCode`].
//!
//! This module provides [`ErrorCodeDoc`] which bundles a description, usage
//! examples, and cross-references for a given error code. It is intended for
//! tooling (error catalogues, dashboards, SDK docs).
//!
//! # Examples
//!
//! ```
//! use abp_error_taxonomy::docs::error_code_doc;
//! use abp_error_taxonomy::ErrorCode;
//!
//! let doc = error_code_doc(&ErrorCode::BackendTimeout);
//! assert!(!doc.description.is_empty());
//! assert!(!doc.example.is_empty());
//! ```

use crate::ErrorCode;

// ---------------------------------------------------------------------------
// ErrorCodeDoc
// ---------------------------------------------------------------------------

/// Structured documentation for a single [`ErrorCode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCodeDoc {
    /// The error code being documented.
    pub code: ErrorCode,
    /// Extended description (longer than `ErrorCode::message()`).
    pub description: String,
    /// A short code example showing how this error is constructed.
    pub example: String,
    /// Related error codes the reader may also want to consult.
    pub see_also: Vec<ErrorCode>,
}

/// Return documentation for the given error code.
///
/// Every [`ErrorCode`] variant is covered; no panics.
pub fn error_code_doc(code: &ErrorCode) -> ErrorCodeDoc {
    match code {
        // -- Protocol -------------------------------------------------------
        ErrorCode::ProtocolInvalidEnvelope => ErrorCodeDoc {
            code: *code,
            description: "The incoming JSONL line could not be parsed as a valid \
                          envelope, or a required field was missing or had an \
                          unexpected type. Check that the sidecar is emitting \
                          well-formed JSON with the correct `t` discriminator."
                .into(),
            example: r#"AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "missing field `t`")"#
                .into(),
            see_also: vec![
                ErrorCode::ProtocolHandshakeFailed,
                ErrorCode::ProtocolMissingRefId,
            ],
        },
        ErrorCode::ProtocolHandshakeFailed => ErrorCodeDoc {
            code: *code,
            description: "The sidecar did not send a valid `hello` envelope as \
                          its first message, or the hello payload was rejected. \
                          Ensure the sidecar sends `hello` before any other \
                          message."
                .into(),
            example: r#"AbpError::new(ErrorCode::ProtocolHandshakeFailed, "no hello received within 5 s")"#
                .into(),
            see_also: vec![ErrorCode::ProtocolVersionMismatch],
        },
        ErrorCode::ProtocolMissingRefId => ErrorCodeDoc {
            code: *code,
            description: "An envelope arrived without a `ref_id` field, making \
                          it impossible to correlate with a run."
                .into(),
            example: r#"AbpError::new(ErrorCode::ProtocolMissingRefId, "envelope has no ref_id")"#
                .into(),
            see_also: vec![ErrorCode::ProtocolInvalidEnvelope],
        },
        ErrorCode::ProtocolUnexpectedMessage => ErrorCodeDoc {
            code: *code,
            description: "A message arrived in an invalid order — for example, \
                          an `event` envelope before the `hello` handshake."
                .into(),
            example: r#"AbpError::new(ErrorCode::ProtocolUnexpectedMessage, "event before hello")"#
                .into(),
            see_also: vec![ErrorCode::ProtocolHandshakeFailed],
        },
        ErrorCode::ProtocolVersionMismatch => ErrorCodeDoc {
            code: *code,
            description: "The contract version advertised by the sidecar does \
                          not match the version expected by the host."
                .into(),
            example: r#"AbpError::new(ErrorCode::ProtocolVersionMismatch, "expected abp/v0.1, got abp/v0.2")"#
                .into(),
            see_also: vec![ErrorCode::ContractVersionMismatch],
        },

        // -- Mapping --------------------------------------------------------
        ErrorCode::MappingUnsupportedCapability => ErrorCodeDoc {
            code: *code,
            description: "A capability required by the work order is not \
                          supported by the target dialect and cannot be emulated."
                .into(),
            example: r#"AbpError::new(ErrorCode::MappingUnsupportedCapability, "tool_use not supported by target")"#
                .into(),
            see_also: vec![ErrorCode::CapabilityUnsupported],
        },
        ErrorCode::MappingDialectMismatch => ErrorCodeDoc {
            code: *code,
            description: "The source and target dialects are fundamentally \
                          incompatible — no mapping path exists."
                .into(),
            example: r#"AbpError::new(ErrorCode::MappingDialectMismatch, "cannot map openai → custom_v1")"#
                .into(),
            see_also: vec![ErrorCode::DialectMappingFailed],
        },
        ErrorCode::MappingLossyConversion => ErrorCodeDoc {
            code: *code,
            description: "Translation completed but some information was lost \
                          (e.g., metadata fields that have no equivalent)."
                .into(),
            example: r#"AbpError::new(ErrorCode::MappingLossyConversion, "system prompt metadata dropped")"#
                .into(),
            see_also: vec![ErrorCode::MappingDialectMismatch],
        },
        ErrorCode::MappingUnmappableTool => ErrorCodeDoc {
            code: *code,
            description: "A tool call in the source dialect has no equivalent in \
                          the target dialect."
                .into(),
            example: r#"AbpError::new(ErrorCode::MappingUnmappableTool, "tool `bash` not in target schema")"#
                .into(),
            see_also: vec![ErrorCode::MappingUnsupportedCapability],
        },

        // -- Backend --------------------------------------------------------
        ErrorCode::BackendNotFound => ErrorCodeDoc {
            code: *code,
            description: "The backend name specified in the work order does not \
                          match any registered backend."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendNotFound, "no backend named 'foo'")"#
                .into(),
            see_also: vec![ErrorCode::BackendUnavailable],
        },
        ErrorCode::BackendUnavailable => ErrorCodeDoc {
            code: *code,
            description: "The backend is known but temporarily cannot accept \
                          requests (e.g., 503 from upstream)."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendUnavailable, "upstream returned 503")"#
                .into(),
            see_also: vec![ErrorCode::BackendTimeout, ErrorCode::BackendCrashed],
        },
        ErrorCode::BackendTimeout => ErrorCodeDoc {
            code: *code,
            description: "The backend did not respond within the configured \
                          deadline."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendTimeout, "no response after 30 s")"#
                .into(),
            see_also: vec![ErrorCode::BackendUnavailable],
        },
        ErrorCode::BackendRateLimited => ErrorCodeDoc {
            code: *code,
            description: "The upstream API returned a rate-limit response \
                          (typically HTTP 429). Retry after the suggested delay."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendRateLimited, "429 Too Many Requests")"#
                .into(),
            see_also: vec![ErrorCode::BackendUnavailable],
        },
        ErrorCode::BackendAuthFailed => ErrorCodeDoc {
            code: *code,
            description: "Authentication with the backend failed. This usually \
                          means an invalid or expired API key."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendAuthFailed, "invalid API key")"#.into(),
            see_also: vec![ErrorCode::ExecutionPermissionDenied],
        },
        ErrorCode::BackendModelNotFound => ErrorCodeDoc {
            code: *code,
            description: "The model identifier specified in the work order is \
                          not recognised by the backend."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendModelNotFound, "model 'gpt-5-turbo' not found")"#
                .into(),
            see_also: vec![ErrorCode::BackendNotFound],
        },
        ErrorCode::BackendCrashed => ErrorCodeDoc {
            code: *code,
            description: "The sidecar process exited unexpectedly (non-zero exit \
                          code or signal). Check sidecar stderr logs."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendCrashed, "process exited with code 1")"#
                .into(),
            see_also: vec![ErrorCode::BackendUnavailable],
        },

        // -- Execution ------------------------------------------------------
        ErrorCode::ExecutionToolFailed => ErrorCodeDoc {
            code: *code,
            description: "A tool invocation inside the agent loop failed at \
                          runtime."
                .into(),
            example: r#"AbpError::new(ErrorCode::ExecutionToolFailed, "tool `read_file` returned error")"#
                .into(),
            see_also: vec![ErrorCode::ExecutionPermissionDenied],
        },
        ErrorCode::ExecutionWorkspaceError => ErrorCodeDoc {
            code: *code,
            description: "An operation in the staged workspace (file copy, git \
                          init, diff) failed."
                .into(),
            example: r#"AbpError::new(ErrorCode::ExecutionWorkspaceError, "git diff failed")"#
                .into(),
            see_also: vec![
                ErrorCode::WorkspaceInitFailed,
                ErrorCode::WorkspaceStagingFailed,
            ],
        },
        ErrorCode::ExecutionPermissionDenied => ErrorCodeDoc {
            code: *code,
            description: "The operation was denied because the caller lacks the \
                          required permissions (file ACLs, policy, etc.)."
                .into(),
            example: r#"AbpError::new(ErrorCode::ExecutionPermissionDenied, "write to /etc denied")"#
                .into(),
            see_also: vec![ErrorCode::PolicyDenied],
        },

        // -- Contract -------------------------------------------------------
        ErrorCode::ContractVersionMismatch => ErrorCodeDoc {
            code: *code,
            description: "The contract version in the payload does not match the \
                          runtime's expected version."
                .into(),
            example: r#"AbpError::new(ErrorCode::ContractVersionMismatch, "expected abp/v0.1")"#
                .into(),
            see_also: vec![ErrorCode::ProtocolVersionMismatch],
        },
        ErrorCode::ContractSchemaViolation => ErrorCodeDoc {
            code: *code,
            description: "The payload failed schema validation — a required \
                          field is missing or has the wrong type."
                .into(),
            example: r#"AbpError::new(ErrorCode::ContractSchemaViolation, "missing field `task`")"#
                .into(),
            see_also: vec![ErrorCode::ContractInvalidReceipt],
        },
        ErrorCode::ContractInvalidReceipt => ErrorCodeDoc {
            code: *code,
            description: "The receipt is structurally invalid or its hash cannot \
                          be verified."
                .into(),
            example: r#"AbpError::new(ErrorCode::ContractInvalidReceipt, "receipt missing receipt_sha256")"#
                .into(),
            see_also: vec![
                ErrorCode::ReceiptHashMismatch,
                ErrorCode::ReceiptChainBroken,
            ],
        },

        // -- Capability -----------------------------------------------------
        ErrorCode::CapabilityUnsupported => ErrorCodeDoc {
            code: *code,
            description: "The backend does not support a capability required by \
                          the work order (e.g., tool_use, streaming)."
                .into(),
            example: r#"AbpError::new(ErrorCode::CapabilityUnsupported, "streaming not supported")"#
                .into(),
            see_also: vec![ErrorCode::MappingUnsupportedCapability],
        },
        ErrorCode::CapabilityEmulationFailed => ErrorCodeDoc {
            code: *code,
            description: "ABP attempted to emulate a missing capability but the \
                          emulation layer failed."
                .into(),
            example: r#"AbpError::new(ErrorCode::CapabilityEmulationFailed, "tool_use emulation error")"#
                .into(),
            see_also: vec![ErrorCode::CapabilityUnsupported],
        },

        // -- Policy ---------------------------------------------------------
        ErrorCode::PolicyDenied => ErrorCodeDoc {
            code: *code,
            description: "A policy rule explicitly denied the operation (tool \
                          call, file access, etc.)."
                .into(),
            example: r#"AbpError::new(ErrorCode::PolicyDenied, "write to /etc blocked by policy")"#
                .into(),
            see_also: vec![ErrorCode::PolicyInvalid],
        },
        ErrorCode::PolicyInvalid => ErrorCodeDoc {
            code: *code,
            description: "The policy definition itself is malformed and could \
                          not be compiled."
                .into(),
            example: r#"AbpError::new(ErrorCode::PolicyInvalid, "invalid glob pattern in deny list")"#
                .into(),
            see_also: vec![ErrorCode::PolicyDenied],
        },

        // -- Workspace ------------------------------------------------------
        ErrorCode::WorkspaceInitFailed => ErrorCodeDoc {
            code: *code,
            description: "Failed to create or initialise the staged workspace \
                          directory (temp dir creation, git init)."
                .into(),
            example: r#"AbpError::new(ErrorCode::WorkspaceInitFailed, "failed to create temp dir")"#
                .into(),
            see_also: vec![ErrorCode::WorkspaceStagingFailed],
        },
        ErrorCode::WorkspaceStagingFailed => ErrorCodeDoc {
            code: *code,
            description: "Failed to copy or stage files into the workspace."
                .into(),
            example: r#"AbpError::new(ErrorCode::WorkspaceStagingFailed, "copy failed: permission denied")"#
                .into(),
            see_also: vec![ErrorCode::WorkspaceInitFailed],
        },

        // -- IR -------------------------------------------------------------
        ErrorCode::IrLoweringFailed => ErrorCodeDoc {
            code: *code,
            description: "Lowering from the high-level IR to the wire format \
                          failed."
                .into(),
            example: r#"AbpError::new(ErrorCode::IrLoweringFailed, "cannot lower tool_result to wire")"#
                .into(),
            see_also: vec![ErrorCode::IrInvalid],
        },
        ErrorCode::IrInvalid => ErrorCodeDoc {
            code: *code,
            description: "The intermediate representation is structurally \
                          invalid or internally inconsistent."
                .into(),
            example: r#"AbpError::new(ErrorCode::IrInvalid, "IR node references missing parent")"#
                .into(),
            see_also: vec![ErrorCode::IrLoweringFailed],
        },

        // -- Receipt --------------------------------------------------------
        ErrorCode::ReceiptHashMismatch => ErrorCodeDoc {
            code: *code,
            description: "The computed SHA-256 hash of the receipt does not \
                          match the declared `receipt_sha256` field."
                .into(),
            example: r#"AbpError::new(ErrorCode::ReceiptHashMismatch, "hash mismatch: expected abc, got def")"#
                .into(),
            see_also: vec![ErrorCode::ReceiptChainBroken],
        },
        ErrorCode::ReceiptChainBroken => ErrorCodeDoc {
            code: *code,
            description: "The receipt chain has a gap or out-of-order entry, \
                          breaking the audit trail."
                .into(),
            example: r#"AbpError::new(ErrorCode::ReceiptChainBroken, "missing receipt #3 in chain")"#
                .into(),
            see_also: vec![ErrorCode::ReceiptHashMismatch],
        },

        // -- Dialect --------------------------------------------------------
        ErrorCode::DialectUnknown => ErrorCodeDoc {
            code: *code,
            description: "The dialect identifier is not recognised by any \
                          registered mapper."
                .into(),
            example: r#"AbpError::new(ErrorCode::DialectUnknown, "unknown dialect 'custom_v99'")"#
                .into(),
            see_also: vec![ErrorCode::DialectMappingFailed],
        },
        ErrorCode::DialectMappingFailed => ErrorCodeDoc {
            code: *code,
            description: "Mapping between two recognised dialects failed at \
                          runtime."
                .into(),
            example: r#"AbpError::new(ErrorCode::DialectMappingFailed, "openai → anthropic mapping error")"#
                .into(),
            see_also: vec![ErrorCode::DialectUnknown, ErrorCode::MappingDialectMismatch],
        },

        // -- Config ---------------------------------------------------------
        ErrorCode::ConfigInvalid => ErrorCodeDoc {
            code: *code,
            description: "The configuration file or a configuration value is \
                          malformed."
                .into(),
            example: r#"AbpError::new(ErrorCode::ConfigInvalid, "invalid TOML: unexpected key `foo`")"#
                .into(),
            see_also: vec![],
        },

        // -- RateLimit ------------------------------------------------------
        ErrorCode::RateLimitExceeded => ErrorCodeDoc {
            code: *code,
            description: "ABP's internal rate limiter blocked the request \
                          before it reached the backend. This is distinct from \
                          vendor-side rate limiting (BackendRateLimited)."
                .into(),
            example: r#"AbpError::new(ErrorCode::RateLimitExceeded, "10 req/s limit exceeded")"#
                .into(),
            see_also: vec![ErrorCode::BackendRateLimited, ErrorCode::CircuitBreakerOpen],
        },
        ErrorCode::CircuitBreakerOpen => ErrorCodeDoc {
            code: *code,
            description: "The circuit breaker for the target backend is open \
                          due to repeated recent failures. Requests are \
                          rejected immediately until the breaker transitions \
                          to half-open."
                .into(),
            example: r#"AbpError::new(ErrorCode::CircuitBreakerOpen, "openai circuit breaker open")"#
                .into(),
            see_also: vec![ErrorCode::BackendUnavailable, ErrorCode::RateLimitExceeded],
        },

        // -- Stream ---------------------------------------------------------
        ErrorCode::StreamClosed => ErrorCodeDoc {
            code: *code,
            description: "The event stream was closed prematurely because all \
                          receivers were dropped. The backend may still be \
                          running but has nowhere to send events."
                .into(),
            example: r#"AbpError::new(ErrorCode::StreamClosed, "all event receivers dropped")"#
                .into(),
            see_also: vec![ErrorCode::BackendCrashed],
        },

        // -- ReceiptStore ---------------------------------------------------
        ErrorCode::ReceiptStoreFailed => ErrorCodeDoc {
            code: *code,
            description: "Persisting a receipt to the receipt store failed due \
                          to I/O error, serialization failure, or duplicate ID."
                .into(),
            example: r#"AbpError::new(ErrorCode::ReceiptStoreFailed, "disk full")"#
                .into(),
            see_also: vec![ErrorCode::ContractInvalidReceipt],
        },

        // -- Validation -----------------------------------------------------
        ErrorCode::ValidationFailed => ErrorCodeDoc {
            code: *code,
            description: "Structured validation of the request payload failed. \
                          This covers field-level type checks and constraint \
                          violations beyond schema conformance."
                .into(),
            example: r#"AbpError::new(ErrorCode::ValidationFailed, "temperature must be 0..2")"#
                .into(),
            see_also: vec![ErrorCode::ContractSchemaViolation],
        },

        // -- Sidecar --------------------------------------------------------
        ErrorCode::SidecarSpawnFailed => ErrorCodeDoc {
            code: *code,
            description: "The sidecar process could not be spawned. The binary \
                          may not exist, may not be executable, or the OS may \
                          have rejected the spawn."
                .into(),
            example: r#"AbpError::new(ErrorCode::SidecarSpawnFailed, "node not found in PATH")"#
                .into(),
            see_also: vec![ErrorCode::BackendCrashed, ErrorCode::BackendNotFound],
        },

        // -- Backend (extended) ---------------------------------------------
        ErrorCode::BackendContentFiltered => ErrorCodeDoc {
            code: *code,
            description: "The vendor's safety or content filter blocked the \
                          request or response. The content was deemed unsafe \
                          or in violation of the vendor's usage policy."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendContentFiltered, "response flagged by safety filter")"#
                .into(),
            see_also: vec![ErrorCode::PolicyDenied],
        },
        ErrorCode::BackendContextLength => ErrorCodeDoc {
            code: *code,
            description: "The input exceeds the model's maximum context window \
                          length. Reduce input size or switch to a model with \
                          a larger context window."
                .into(),
            example: r#"AbpError::new(ErrorCode::BackendContextLength, "128k token limit exceeded")"#
                .into(),
            see_also: vec![ErrorCode::ContractSchemaViolation],
        },

        // -- Internal -------------------------------------------------------
        ErrorCode::Internal => ErrorCodeDoc {
            code: *code,
            description: "An unexpected internal error that does not fit any \
                          other category. Check logs for details."
                .into(),
            example: r#"AbpError::new(ErrorCode::Internal, "unexpected panic in worker thread")"#
                .into(),
            see_also: vec![],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 44 error codes for exhaustive iteration.
    const ALL_CODES: &[ErrorCode] = &[
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
        ErrorCode::RateLimitExceeded,
        ErrorCode::CircuitBreakerOpen,
        ErrorCode::StreamClosed,
        ErrorCode::ReceiptStoreFailed,
        ErrorCode::ValidationFailed,
        ErrorCode::SidecarSpawnFailed,
        ErrorCode::BackendContentFiltered,
        ErrorCode::BackendContextLength,
        ErrorCode::Internal,
    ];

    #[test]
    fn every_code_has_documentation() {
        for code in ALL_CODES {
            let doc = error_code_doc(code);
            assert_eq!(doc.code, *code);
            assert!(
                !doc.description.is_empty(),
                "{:?} has empty description",
                code
            );
            assert!(!doc.example.is_empty(), "{:?} has empty example", code);
        }
    }

    #[test]
    fn doc_descriptions_differ_from_short_message() {
        for code in ALL_CODES {
            let doc = error_code_doc(code);
            assert_ne!(
                doc.description,
                code.message(),
                "{:?} doc description should be longer than the short message",
                code
            );
        }
    }

    #[test]
    fn examples_mention_error_code_variant() {
        for code in ALL_CODES {
            let doc = error_code_doc(code);
            let variant_name = format!("{:?}", code);
            assert!(
                doc.example.contains(&variant_name),
                "{:?} example should mention the error code variant",
                code
            );
        }
    }

    #[test]
    fn see_also_references_are_valid_codes() {
        for code in ALL_CODES {
            let doc = error_code_doc(code);
            for related in &doc.see_also {
                assert!(
                    ALL_CODES.contains(related),
                    "{:?} see_also references unknown code {:?}",
                    code,
                    related
                );
            }
        }
    }
}
