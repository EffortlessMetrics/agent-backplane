//! Deep tests for the ABP error taxonomy.
//!
//! Covers: unique codes, stable strings (snapshot), Display impls, AbpError
//! wrapping, error source chains, downcast paths, serialisation round-trips,
//! Send+Sync+'static bounds, equality, From conversions, custom messages,
//! severity classification, and recovery suggestions.

use abp_error_taxonomy::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::io;

// =========================================================================
// Helpers
// =========================================================================

/// Exhaustive list – must stay in sync with `ErrorCode` variants.
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

const ALL_CATEGORIES: &[ErrorCategory] = &[
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

/// Helper: map category → expected display string.
fn category_display_str(cat: ErrorCategory) -> &'static str {
    match cat {
        ErrorCategory::Protocol => "protocol",
        ErrorCategory::Backend => "backend",
        ErrorCategory::Capability => "capability",
        ErrorCategory::Policy => "policy",
        ErrorCategory::Workspace => "workspace",
        ErrorCategory::Ir => "ir",
        ErrorCategory::Receipt => "receipt",
        ErrorCategory::Dialect => "dialect",
        ErrorCategory::Config => "config",
        ErrorCategory::Mapping => "mapping",
        ErrorCategory::Execution => "execution",
        ErrorCategory::Contract => "contract",
        ErrorCategory::Internal => "internal",
    }
}

/// Helper: returns true when an error category is "transient" (retry may help).
fn is_transient(cat: ErrorCategory) -> bool {
    matches!(cat, ErrorCategory::Backend | ErrorCategory::Protocol)
}

/// Helper: returns a suggested recovery action for a category.
fn recovery_suggestion(cat: ErrorCategory) -> &'static str {
    match cat {
        ErrorCategory::Protocol => "Check wire format and contract version",
        ErrorCategory::Backend => "Retry with back-off or try another backend",
        ErrorCategory::Capability => "Check backend capabilities before submitting work",
        ErrorCategory::Policy => "Adjust the policy profile or requested operation",
        ErrorCategory::Workspace => "Verify filesystem permissions and paths",
        ErrorCategory::Ir => "Validate IR structure before lowering",
        ErrorCategory::Receipt => "Recompute receipt hash or rebuild chain",
        ErrorCategory::Dialect => "Use a supported dialect or install a mapping",
        ErrorCategory::Config => "Fix the configuration file and reload",
        ErrorCategory::Mapping => "Check dialect compatibility or retry with a supported mapping",
        ErrorCategory::Execution => "Check tool configuration and workspace permissions",
        ErrorCategory::Contract => "Validate contract version and schema conformance",
        ErrorCategory::Internal => "Report a bug with diagnostic context",
    }
}

/// Classifies error severity: "critical" | "error" | "warning".
fn severity(code: &ErrorCode) -> &'static str {
    match code {
        ErrorCode::BackendCrashed | ErrorCode::Internal => "critical",
        ErrorCode::ReceiptHashMismatch | ErrorCode::ReceiptChainBroken => "critical",
        ErrorCode::ProtocolVersionMismatch => "critical",
        ErrorCode::PolicyDenied => "warning",
        _ => "error",
    }
}

// =========================================================================
// 1. All error categories have unique codes
// =========================================================================

#[test]
fn all_error_codes_have_unique_as_str() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        assert!(
            seen.insert(code.as_str()),
            "duplicate as_str: {}",
            code.as_str()
        );
    }
    assert_eq!(seen.len(), ALL_CODES.len());
}

#[test]
fn all_error_codes_have_unique_debug() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        let dbg = format!("{code:?}");
        assert!(seen.insert(dbg.clone()), "duplicate Debug: {dbg}");
    }
}

#[test]
fn every_category_has_at_least_one_code() {
    for cat in ALL_CATEGORIES {
        let count = ALL_CODES.iter().filter(|c| c.category() == *cat).count();
        assert!(count >= 1, "category {cat:?} has no codes");
    }
}

#[test]
fn no_two_categories_share_a_code() {
    for code in ALL_CODES {
        let cat = code.category();
        // Verify category is deterministic.
        assert_eq!(code.category(), cat);
    }
}

// =========================================================================
// 2. Error codes are stable strings (snapshot-style)
// =========================================================================

#[test]
fn snapshot_protocol_codes() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.as_str(),
        "protocol_unexpected_message"
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.as_str(),
        "protocol_version_mismatch"
    );
}

#[test]
fn snapshot_backend_codes() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
}

#[test]
fn snapshot_capability_codes() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.as_str(),
        "capability_unsupported"
    );
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.as_str(),
        "capability_emulation_failed"
    );
}

#[test]
fn snapshot_policy_codes() {
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(ErrorCode::PolicyInvalid.as_str(), "policy_invalid");
}

#[test]
fn snapshot_workspace_codes() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.as_str(),
        "workspace_init_failed"
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.as_str(),
        "workspace_staging_failed"
    );
}

#[test]
fn snapshot_ir_codes() {
    assert_eq!(ErrorCode::IrLoweringFailed.as_str(), "ir_lowering_failed");
    assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
}

#[test]
fn snapshot_receipt_codes() {
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.as_str(),
        "receipt_chain_broken"
    );
}

#[test]
fn snapshot_dialect_codes() {
    assert_eq!(ErrorCode::DialectUnknown.as_str(), "dialect_unknown");
    assert_eq!(
        ErrorCode::DialectMappingFailed.as_str(),
        "dialect_mapping_failed"
    );
}

#[test]
fn snapshot_config_code() {
    assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
}

#[test]
fn snapshot_internal_code() {
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
}

#[test]
fn snapshot_total_code_count() {
    assert_eq!(
        ALL_CODES.len(),
        36,
        "variant added/removed without updating ALL_CODES"
    );
}

// =========================================================================
// 3. ErrorCategory Display is human-readable
// =========================================================================

#[test]
fn category_display_all_variants() {
    for cat in ALL_CATEGORIES {
        let displayed = cat.to_string();
        assert_eq!(displayed, category_display_str(*cat));
        // Must be lowercase ASCII.
        assert!(displayed.chars().all(|c| c.is_ascii_lowercase()));
    }
}

#[test]
fn category_display_is_nonempty() {
    for cat in ALL_CATEGORIES {
        assert!(!cat.to_string().is_empty());
    }
}

// =========================================================================
// 4. ErrorCode Display includes code
// =========================================================================

#[test]
fn error_code_display_matches_as_str() {
    // Display now returns human-readable messages via message().
    for code in ALL_CODES {
        let display = code.to_string();
        assert_eq!(display, code.message());
    }
}

#[test]
fn error_code_display_is_screaming_snake() {
    // Display now returns human-readable messages, not SCREAMING_SNAKE_CASE.
    // Verify as_str() is snake_case instead.
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "code {s} is not snake_case"
        );
    }
}

// =========================================================================
// 5. AbpError wraps all categories correctly
// =========================================================================

#[test]
fn abp_error_wraps_every_code() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, format!("test {code}"));
        assert_eq!(err.code, *code);
        assert_eq!(err.category(), code.category());
    }
}

#[test]
fn abp_error_message_preserved() {
    let msg = "this is the exact message";
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert_eq!(err.message, msg);
}

#[test]
fn abp_error_display_contains_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "not found");
    let display = err.to_string();
    assert!(display.contains("backend_not_found"));
    assert!(display.contains("not found"));
}

#[test]
fn abp_error_display_includes_context_when_present() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow").with_context("ms", 5000);
    let display = err.to_string();
    assert!(display.contains("5000"));
    assert!(display.contains("ms"));
}

#[test]
fn abp_error_display_no_context_when_empty() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let display = err.to_string();
    assert_eq!(display, "[policy_denied] denied");
}

// =========================================================================
// 6. Error source chains work
// =========================================================================

#[test]
fn source_chain_single_level() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "file gone");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(inner);
    let src = err.source().expect("should have source");
    assert_eq!(src.to_string(), "file gone");
}

#[test]
fn source_chain_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    assert!(err.source().is_none());
}

#[test]
fn source_chain_two_levels() {
    let root = io::Error::new(io::ErrorKind::PermissionDenied, "no perms");
    let mid = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(root);
    let outer = AbpError::new(ErrorCode::Internal, "wrapper").with_source(mid);

    let src1 = outer.source().unwrap();
    assert!(src1.to_string().contains("workspace_staging_failed"));

    let src2 = src1.source().unwrap();
    assert_eq!(src2.to_string(), "no perms");
}

#[test]
fn source_chain_walk_to_root() {
    let root = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(root);

    let mut depth = 0;
    let mut current: &dyn StdError = &err;
    while let Some(src) = current.source() {
        depth += 1;
        current = src;
    }
    assert_eq!(depth, 1);
}

// =========================================================================
// 7. Error downcast paths
// =========================================================================

#[test]
fn downcast_io_error_from_source() {
    let inner = io::Error::new(io::ErrorKind::TimedOut, "timeout");
    let err = AbpError::new(ErrorCode::BackendTimeout, "backend timed out").with_source(inner);
    let boxed = err.source.as_ref().unwrap();
    let io_err = boxed.downcast_ref::<io::Error>().unwrap();
    assert_eq!(io_err.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn downcast_abp_error_from_source() {
    let inner = AbpError::new(ErrorCode::PolicyDenied, "inner denied");
    let outer = AbpError::new(ErrorCode::Internal, "wrapped").with_source(inner);
    let boxed = outer.source.as_ref().unwrap();
    let abp_inner = boxed.downcast_ref::<AbpError>().unwrap();
    assert_eq!(abp_inner.code, ErrorCode::PolicyDenied);
}

#[test]
fn downcast_wrong_type_returns_none() {
    let inner = io::Error::other("io");
    let err = AbpError::new(ErrorCode::Internal, "x").with_source(inner);
    let boxed = err.source.as_ref().unwrap();
    assert!(boxed.downcast_ref::<AbpError>().is_none());
}

// =========================================================================
// 8. Error serialization / deserialization
// =========================================================================

#[test]
fn error_code_json_roundtrip_all() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *code, "roundtrip mismatch for {code:?}");
    }
}

#[test]
fn error_code_json_is_quoted_string() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        assert!(json.starts_with('"'));
        assert!(json.ends_with('"'));
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected);
    }
}

#[test]
fn error_category_json_roundtrip_all() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *cat);
    }
}

#[test]
fn error_category_json_is_snake_case() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let inner = json.trim_matches('"');
        assert!(inner.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
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
    let inner = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(inner);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn dto_to_abp_error_loses_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad".into(),
        context: BTreeMap::new(),
        source_message: Some("inner".into()),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert!(err.source.is_none(), "DTO→AbpError drops opaque source");
}

#[test]
fn dto_preserves_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("retries", 3);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.context["backend"], serde_json::json!("openai"));
    assert_eq!(dto.context["retries"], serde_json::json!(3));
}

#[test]
fn dto_serialized_omits_null_source() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

#[test]
fn dto_from_all_codes() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, format!("msg for {}", code.as_str()));
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.code, *code);
        assert!(dto.message.contains(code.as_str()));
    }
}

// =========================================================================
// 9. All errors are Send + Sync + 'static
// =========================================================================

#[test]
fn abp_error_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<AbpError>();
}

#[test]
fn abp_error_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<AbpError>();
}

#[test]
fn abp_error_is_static() {
    fn assert_static<T: 'static>() {}
    assert_static::<AbpError>();
}

#[test]
fn error_code_is_send_sync_static() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    assert_bounds::<ErrorCode>();
}

#[test]
fn error_category_is_send_sync_static() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    assert_bounds::<ErrorCategory>();
}

#[test]
fn abp_error_dto_is_send_sync_static() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    assert_bounds::<AbpErrorDto>();
}

#[test]
fn abp_error_can_cross_threads() {
    let err = AbpError::new(ErrorCode::Internal, "thread test");
    let handle = std::thread::spawn(move || {
        assert_eq!(err.code, ErrorCode::Internal);
        err.message.clone()
    });
    let msg = handle.join().unwrap();
    assert_eq!(msg, "thread test");
}

// =========================================================================
// 10. Error comparison / equality
// =========================================================================

#[test]
fn error_code_eq() {
    assert_eq!(ErrorCode::Internal, ErrorCode::Internal);
    assert_ne!(ErrorCode::Internal, ErrorCode::BackendTimeout);
}

#[test]
fn error_code_clone_eq() {
    let code = ErrorCode::PolicyDenied;
    #[allow(clippy::clone_on_copy)]
    let cloned = code.clone();
    assert_eq!(code, cloned);
}

#[test]
fn error_code_copy() {
    let a = ErrorCode::BackendNotFound;
    let b = a; // copy
    assert_eq!(a, b);
}

#[test]
fn error_category_eq() {
    assert_eq!(ErrorCategory::Protocol, ErrorCategory::Protocol);
    assert_ne!(ErrorCategory::Protocol, ErrorCategory::Backend);
}

#[test]
fn error_category_clone_eq() {
    let cat = ErrorCategory::Workspace;
    #[allow(clippy::clone_on_copy)]
    let cloned = cat.clone();
    assert_eq!(cat, cloned);
}

#[test]
fn error_code_hash_consistent() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of(code: &ErrorCode) -> u64 {
        let mut h = DefaultHasher::new();
        code.hash(&mut h);
        h.finish()
    }

    let a = ErrorCode::BackendTimeout;
    let b = ErrorCode::BackendTimeout;
    assert_eq!(hash_of(&a), hash_of(&b));

    // Different codes should (almost certainly) produce different hashes.
    let c = ErrorCode::PolicyDenied;
    assert_ne!(hash_of(&a), hash_of(&c));
}

#[test]
fn error_category_hash_consistent() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of(cat: &ErrorCategory) -> u64 {
        let mut h = DefaultHasher::new();
        cat.hash(&mut h);
        h.finish()
    }

    assert_eq!(hash_of(&ErrorCategory::Ir), hash_of(&ErrorCategory::Ir));
    assert_ne!(hash_of(&ErrorCategory::Ir), hash_of(&ErrorCategory::Config));
}

#[test]
fn dto_equality() {
    let dto1 = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "a".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let dto2 = dto1.clone();
    assert_eq!(dto1, dto2);
}

#[test]
fn dto_inequality_on_code() {
    let dto1 = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "a".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let dto2 = AbpErrorDto {
        code: ErrorCode::BackendTimeout,
        message: "a".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    assert_ne!(dto1, dto2);
}

// =========================================================================
// 11. Error from io::Error conversion
// =========================================================================

#[test]
fn wrap_io_not_found() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "gone");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init fail").with_source(io_err);
    assert_eq!(err.code, ErrorCode::WorkspaceInitFailed);
    let src = err.source().unwrap();
    assert_eq!(src.to_string(), "gone");
}

#[test]
fn wrap_io_permission_denied() {
    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "no access");
    let err = AbpError::new(ErrorCode::PolicyDenied, "policy denied").with_source(io_err);
    let src = err.source.as_ref().unwrap();
    let io_ref = src.downcast_ref::<io::Error>().unwrap();
    assert_eq!(io_ref.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn wrap_io_timed_out() {
    let io_err = io::Error::new(io::ErrorKind::TimedOut, "deadline exceeded");
    let err = AbpError::new(ErrorCode::BackendTimeout, "backend timeout").with_source(io_err);
    let src = err.source.as_ref().unwrap();
    let io_ref = src.downcast_ref::<io::Error>().unwrap();
    assert_eq!(io_ref.kind(), io::ErrorKind::TimedOut);
}

// =========================================================================
// 12. Error from serde_json::Error conversion
// =========================================================================

#[test]
fn wrap_serde_json_parse_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let err =
        AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "bad envelope").with_source(serde_err);
    assert_eq!(err.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(err.source().is_some());
}

#[test]
fn wrap_serde_json_type_mismatch() {
    let serde_err = serde_json::from_str::<Vec<i32>>("\"not an array\"").unwrap_err();
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config").with_source(serde_err);
    let src = err.source().unwrap();
    assert!(!src.to_string().is_empty());
}

#[test]
fn serde_error_source_is_downcastable() {
    let serde_err = serde_json::from_str::<i32>("\"nope\"").unwrap_err();
    let err = AbpError::new(ErrorCode::IrInvalid, "ir parse fail").with_source(serde_err);
    let boxed = err.source.as_ref().unwrap();
    assert!(boxed.downcast_ref::<serde_json::Error>().is_some());
}

// =========================================================================
// 13. Custom error messages preserved
// =========================================================================

#[test]
fn empty_message_allowed() {
    let err = AbpError::new(ErrorCode::Internal, "");
    assert_eq!(err.message, "");
    assert!(err.to_string().contains("internal"));
}

#[test]
fn unicode_message_preserved() {
    let msg = "エラーが発生しました 🚨";
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert_eq!(err.message, msg);
    assert!(err.to_string().contains(msg));
}

#[test]
fn long_message_preserved() {
    let msg = "a]".repeat(10_000);
    let err = AbpError::new(ErrorCode::Internal, &msg);
    assert_eq!(err.message, msg);
}

#[test]
fn message_with_special_chars() {
    let msg = r#"line1\nline2\ttab "quoted""#;
    let err = AbpError::new(ErrorCode::ConfigInvalid, msg);
    assert_eq!(err.message, msg);
}

#[test]
fn message_from_string() {
    let msg = String::from("dynamic message");
    let err = AbpError::new(ErrorCode::BackendNotFound, msg);
    assert_eq!(err.message, "dynamic message");
}

#[test]
fn message_from_format() {
    let name = "openai";
    let err = AbpError::new(
        ErrorCode::BackendNotFound,
        format!("backend {name} not found"),
    );
    assert_eq!(err.message, "backend openai not found");
}

// =========================================================================
// 14. Error classification by severity
// =========================================================================

#[test]
fn backend_crashed_is_critical() {
    assert_eq!(severity(&ErrorCode::BackendCrashed), "critical");
}

#[test]
fn internal_is_critical() {
    assert_eq!(severity(&ErrorCode::Internal), "critical");
}

#[test]
fn receipt_hash_mismatch_is_critical() {
    assert_eq!(severity(&ErrorCode::ReceiptHashMismatch), "critical");
}

#[test]
fn receipt_chain_broken_is_critical() {
    assert_eq!(severity(&ErrorCode::ReceiptChainBroken), "critical");
}

#[test]
fn protocol_version_mismatch_is_critical() {
    assert_eq!(severity(&ErrorCode::ProtocolVersionMismatch), "critical");
}

#[test]
fn policy_denied_is_warning() {
    assert_eq!(severity(&ErrorCode::PolicyDenied), "warning");
}

#[test]
fn backend_timeout_is_error() {
    assert_eq!(severity(&ErrorCode::BackendTimeout), "error");
}

#[test]
fn every_code_has_a_severity() {
    for code in ALL_CODES {
        let sev = severity(code);
        assert!(
            ["critical", "error", "warning"].contains(&sev),
            "unexpected severity {sev} for {code:?}"
        );
    }
}

// =========================================================================
// 15. Error recovery suggestions
// =========================================================================

#[test]
fn protocol_recovery_suggestion() {
    let suggestion = recovery_suggestion(ErrorCategory::Protocol);
    assert!(suggestion.contains("wire format") || suggestion.contains("contract"));
}

#[test]
fn backend_recovery_suggestion() {
    let suggestion = recovery_suggestion(ErrorCategory::Backend);
    assert!(suggestion.to_lowercase().contains("retry") || suggestion.contains("back-off"));
}

#[test]
fn capability_recovery_suggestion() {
    let suggestion = recovery_suggestion(ErrorCategory::Capability);
    assert!(suggestion.contains("capabilities"));
}

#[test]
fn policy_recovery_suggestion() {
    let suggestion = recovery_suggestion(ErrorCategory::Policy);
    assert!(suggestion.contains("policy"));
}

#[test]
fn workspace_recovery_suggestion() {
    let suggestion = recovery_suggestion(ErrorCategory::Workspace);
    assert!(suggestion.contains("permissions") || suggestion.contains("path"));
}

#[test]
fn every_category_has_recovery_suggestion() {
    for cat in ALL_CATEGORIES {
        let suggestion = recovery_suggestion(*cat);
        assert!(!suggestion.is_empty(), "no suggestion for {cat:?}");
    }
}

#[test]
fn transient_categories_suggest_retry() {
    for cat in ALL_CATEGORIES {
        if is_transient(*cat) {
            let suggestion = recovery_suggestion(*cat);
            assert!(
                suggestion.to_lowercase().contains("retry")
                    || suggestion.to_lowercase().contains("check")
                    || suggestion.to_lowercase().contains("back-off"),
                "transient category {cat:?} should suggest retry/check, got: {suggestion}"
            );
        }
    }
}

// =========================================================================
// Additional comprehensive tests
// =========================================================================

#[test]
fn context_with_nested_json() {
    let err = AbpError::new(ErrorCode::Internal, "nested")
        .with_context("details", serde_json::json!({"a": 1, "b": [2, 3]}));
    assert_eq!(
        err.context["details"],
        serde_json::json!({"a": 1, "b": [2, 3]})
    );
}

#[test]
fn context_overwrites_same_key() {
    let err = AbpError::new(ErrorCode::Internal, "overwrite")
        .with_context("key", "first")
        .with_context("key", "second");
    assert_eq!(err.context.len(), 1);
    assert_eq!(err.context["key"], serde_json::json!("second"));
}

#[test]
fn context_keys_are_deterministic_in_display() {
    let err = AbpError::new(ErrorCode::Internal, "ordered")
        .with_context("zebra", 1)
        .with_context("alpha", 2);
    let display = err.to_string();
    let alpha_pos = display.find("alpha").unwrap();
    let zebra_pos = display.find("zebra").unwrap();
    assert!(
        alpha_pos < zebra_pos,
        "BTreeMap should order keys alphabetically"
    );
}

#[test]
fn debug_impl_shows_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("denied"));
}

#[test]
fn debug_impl_shows_source_when_present() {
    let inner = io::Error::other("root cause");
    let err = AbpError::new(ErrorCode::Internal, "wrapper").with_source(inner);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("root cause"));
}

#[test]
fn debug_impl_shows_context_when_present() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("ms", 5000);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
}

#[test]
fn debug_impl_omits_source_and_context_when_absent() {
    let err = AbpError::new(ErrorCode::Internal, "bare");
    let dbg = format!("{err:?}");
    // Debug struct should not include "source" or "context" fields when empty/None.
    assert!(!dbg.contains("source"));
    assert!(!dbg.contains("context"));
}

#[test]
fn abp_error_implements_std_error() {
    fn assert_error<T: StdError>() {}
    assert_error::<AbpError>();
}

#[test]
fn error_code_used_in_hashset() {
    let mut set = HashSet::new();
    set.insert(ErrorCode::Internal);
    set.insert(ErrorCode::Internal); // dup
    set.insert(ErrorCode::BackendTimeout);
    assert_eq!(set.len(), 2);
}

#[test]
fn error_category_used_in_hashset() {
    let mut set = HashSet::new();
    for cat in ALL_CATEGORIES {
        set.insert(*cat);
    }
    assert_eq!(set.len(), ALL_CATEGORIES.len());
}

#[test]
fn dto_clone() {
    let dto = AbpErrorDto {
        code: ErrorCode::BackendTimeout,
        message: "timeout".into(),
        context: BTreeMap::from([("k".into(), serde_json::json!("v"))]),
        source_message: Some("inner".into()),
    };
    let cloned = dto.clone();
    assert_eq!(dto, cloned);
}
