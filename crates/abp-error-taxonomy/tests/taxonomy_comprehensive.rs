//! Comprehensive tests for the ABP error taxonomy.
//!
//! Covers: error classification, category mapping, severity levels,
//! recovery strategies, error aggregation, and serde stability.

use abp_error_taxonomy::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use std::collections::{HashMap, HashSet};

// =========================================================================
// Helpers
// =========================================================================

/// Exhaustive list of all error codes.
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

/// Classify severity: "critical" | "error" | "warning".
fn severity_of(code: &ErrorCode) -> &'static str {
    match code {
        ErrorCode::BackendCrashed | ErrorCode::Internal => "critical",
        ErrorCode::ReceiptHashMismatch | ErrorCode::ReceiptChainBroken => "critical",
        ErrorCode::ProtocolVersionMismatch | ErrorCode::ContractVersionMismatch => "critical",
        ErrorCode::MappingLossyConversion | ErrorCode::PolicyDenied => "warning",
        _ => "error",
    }
}

/// Recommended recovery strategy for a category.
fn recovery_strategy(cat: ErrorCategory) -> &'static str {
    match cat {
        ErrorCategory::Protocol => "validate_and_reconnect",
        ErrorCategory::Backend => "retry_with_backoff",
        ErrorCategory::Capability => "check_capabilities_before_submit",
        ErrorCategory::Policy => "adjust_policy_or_request",
        ErrorCategory::Workspace => "verify_filesystem",
        ErrorCategory::Ir => "validate_ir_structure",
        ErrorCategory::Receipt => "recompute_or_rebuild_chain",
        ErrorCategory::Dialect => "use_supported_dialect",
        ErrorCategory::Config => "fix_and_reload_config",
        ErrorCategory::Mapping => "check_dialect_compatibility",
        ErrorCategory::Execution => "check_tool_and_permissions",
        ErrorCategory::Contract => "validate_contract_schema",
        ErrorCategory::Internal => "report_bug",
    }
}

/// Whether a category should recommend retry with backoff.
fn suggests_backoff(cat: ErrorCategory) -> bool {
    matches!(cat, ErrorCategory::Backend)
}

/// Whether a category should recommend circuit breaker.
fn suggests_circuit_breaker(cat: ErrorCategory) -> bool {
    matches!(cat, ErrorCategory::Backend | ErrorCategory::Execution)
}

/// Whether a category should recommend immediate fail-fast.
fn suggests_fail_fast(cat: ErrorCategory) -> bool {
    matches!(
        cat,
        ErrorCategory::Contract | ErrorCategory::Policy | ErrorCategory::Config
    )
}

// =========================================================================
// 1. ErrorClassification (15 tests)
// =========================================================================

#[test]
fn classify_error_by_code_protocol() {
    let err = AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "bad envelope");
    assert_eq!(err.code, ErrorCode::ProtocolInvalidEnvelope);
    assert_eq!(err.category(), ErrorCategory::Protocol);
}

#[test]
fn classify_error_by_code_backend() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.category(), ErrorCategory::Backend);
}

#[test]
fn classify_error_by_code_policy() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "blocked by policy");
    assert_eq!(err.code, ErrorCode::PolicyDenied);
    assert_eq!(err.category(), ErrorCategory::Policy);
}

#[test]
fn classify_every_code_has_known_severity() {
    let valid = ["critical", "error", "warning"];
    for code in ALL_CODES {
        let sev = severity_of(code);
        assert!(
            valid.contains(&sev),
            "code {:?} has unknown severity {sev}",
            code
        );
    }
}

#[test]
fn classify_retryable_codes_are_backend_subset() {
    for code in ALL_CODES {
        if code.is_retryable() {
            assert_eq!(
                code.category(),
                ErrorCategory::Backend,
                "retryable code {:?} should be in Backend category",
                code
            );
        }
    }
}

#[test]
fn classify_non_retryable_includes_all_non_backend_transient() {
    let non_retryable: Vec<_> = ALL_CODES.iter().filter(|c| !c.is_retryable()).collect();
    // BackendNotFound, BackendAuthFailed, BackendModelNotFound are Backend but not retryable
    assert!(non_retryable.contains(&&ErrorCode::BackendNotFound));
    assert!(non_retryable.contains(&&ErrorCode::BackendAuthFailed));
    assert!(non_retryable.contains(&&ErrorCode::BackendModelNotFound));
}

#[test]
fn classify_exactly_four_retryable_codes() {
    let retryable: Vec<_> = ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
    assert_eq!(retryable.len(), 4);
    assert!(retryable.contains(&&ErrorCode::BackendUnavailable));
    assert!(retryable.contains(&&ErrorCode::BackendTimeout));
    assert!(retryable.contains(&&ErrorCode::BackendRateLimited));
    assert!(retryable.contains(&&ErrorCode::BackendCrashed));
}

#[test]
fn classify_error_info_inherits_retryability() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited");
    assert!(info.is_retryable);
    let info2 = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    assert!(!info2.is_retryable);
}

#[test]
fn classify_abp_error_delegates_retryable() {
    let err = AbpError::new(ErrorCode::BackendUnavailable, "unavailable");
    assert!(err.is_retryable());
    let err2 = AbpError::new(ErrorCode::ConfigInvalid, "bad config");
    assert!(!err2.is_retryable());
}

#[test]
fn classify_error_code_as_str_is_snake_case() {
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{s} is not snake_case"
        );
    }
}

#[test]
fn classify_error_code_message_is_nonempty() {
    for code in ALL_CODES {
        assert!(!code.message().is_empty(), "{:?} has empty message", code);
    }
}

#[test]
fn classify_error_code_display_uses_message() {
    for code in ALL_CODES {
        assert_eq!(code.to_string(), code.message());
    }
}

#[test]
fn classify_serde_roundtrip_error_code() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *code, "roundtrip failed for {:?}", code);
    }
}

#[test]
fn classify_serde_roundtrip_error_info() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "slow")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30000);
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn classify_error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::ExecutionToolFailed, "tool crashed");
    assert_eq!(info.to_string(), "[execution_tool_failed] tool crashed");
}

// =========================================================================
// 2. ErrorCategory mapping (15 tests)
// =========================================================================

#[test]
fn category_protocol_codes() {
    let expected = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &expected {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{:?}", code);
    }
}

#[test]
fn category_backend_codes() {
    let expected = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ];
    for code in &expected {
        assert_eq!(code.category(), ErrorCategory::Backend, "{:?}", code);
    }
}

#[test]
fn category_mapping_codes() {
    let expected = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ];
    for code in &expected {
        assert_eq!(code.category(), ErrorCategory::Mapping, "{:?}", code);
    }
}

#[test]
fn category_execution_codes() {
    let expected = [
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ];
    for code in &expected {
        assert_eq!(code.category(), ErrorCategory::Execution, "{:?}", code);
    }
}

#[test]
fn category_contract_codes() {
    let expected = [
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ];
    for code in &expected {
        assert_eq!(code.category(), ErrorCategory::Contract, "{:?}", code);
    }
}

#[test]
fn category_capability_codes() {
    let expected = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in &expected {
        assert_eq!(code.category(), ErrorCategory::Capability, "{:?}", code);
    }
}

#[test]
fn category_policy_codes() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn category_workspace_codes() {
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
fn category_ir_codes() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
}

#[test]
fn category_receipt_codes() {
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
fn category_dialect_codes() {
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(
        ErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
}

#[test]
fn category_config_code() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn category_internal_code() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn category_every_category_has_at_least_one_code() {
    for cat in ALL_CATEGORIES {
        let count = ALL_CODES.iter().filter(|c| c.category() == *cat).count();
        assert!(count >= 1, "category {:?} has no codes", cat);
    }
}

#[test]
fn category_mapping_is_deterministic() {
    for code in ALL_CODES {
        let c1 = code.category();
        let c2 = code.category();
        assert_eq!(c1, c2, "category is not deterministic for {:?}", code);
    }
}

// =========================================================================
// 3. Severity levels (10 tests)
// =========================================================================

#[test]
fn severity_backend_crashed_is_critical() {
    assert_eq!(severity_of(&ErrorCode::BackendCrashed), "critical");
}

#[test]
fn severity_internal_is_critical() {
    assert_eq!(severity_of(&ErrorCode::Internal), "critical");
}

#[test]
fn severity_receipt_hash_mismatch_is_critical() {
    assert_eq!(severity_of(&ErrorCode::ReceiptHashMismatch), "critical");
}

#[test]
fn severity_receipt_chain_broken_is_critical() {
    assert_eq!(severity_of(&ErrorCode::ReceiptChainBroken), "critical");
}

#[test]
fn severity_protocol_version_mismatch_is_critical() {
    assert_eq!(severity_of(&ErrorCode::ProtocolVersionMismatch), "critical");
}

#[test]
fn severity_contract_version_mismatch_is_critical() {
    assert_eq!(
        severity_of(&ErrorCode::ContractVersionMismatch),
        "critical"
    );
}

#[test]
fn severity_policy_denied_is_warning() {
    assert_eq!(severity_of(&ErrorCode::PolicyDenied), "warning");
}

#[test]
fn severity_lossy_conversion_is_warning() {
    assert_eq!(severity_of(&ErrorCode::MappingLossyConversion), "warning");
}

#[test]
fn severity_regular_errors_are_error_level() {
    let error_level_codes = [
        ErrorCode::BackendTimeout,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendAuthFailed,
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::ConfigInvalid,
    ];
    for code in &error_level_codes {
        assert_eq!(
            severity_of(code),
            "error",
            "{:?} should be 'error' severity",
            code
        );
    }
}

#[test]
fn severity_ordering_critical_gt_error_gt_warning() {
    // Verify that our severity system has a clear ordering
    fn severity_rank(sev: &str) -> u8 {
        match sev {
            "critical" => 3,
            "error" => 2,
            "warning" => 1,
            _ => 0,
        }
    }
    assert!(severity_rank("critical") > severity_rank("error"));
    assert!(severity_rank("error") > severity_rank("warning"));
    // BackendCrashed (critical) > BackendTimeout (error) > PolicyDenied (warning)
    assert!(
        severity_rank(severity_of(&ErrorCode::BackendCrashed))
            > severity_rank(severity_of(&ErrorCode::BackendTimeout))
    );
    assert!(
        severity_rank(severity_of(&ErrorCode::BackendTimeout))
            > severity_rank(severity_of(&ErrorCode::PolicyDenied))
    );
}

// =========================================================================
// 4. Recovery strategies (15 tests)
// =========================================================================

#[test]
fn recovery_backend_suggests_retry_with_backoff() {
    let strategy = recovery_strategy(ErrorCategory::Backend);
    assert_eq!(strategy, "retry_with_backoff");
}

#[test]
fn recovery_protocol_suggests_validate_and_reconnect() {
    let strategy = recovery_strategy(ErrorCategory::Protocol);
    assert_eq!(strategy, "validate_and_reconnect");
}

#[test]
fn recovery_policy_suggests_adjust() {
    let strategy = recovery_strategy(ErrorCategory::Policy);
    assert_eq!(strategy, "adjust_policy_or_request");
}

#[test]
fn recovery_config_suggests_fix_and_reload() {
    let strategy = recovery_strategy(ErrorCategory::Config);
    assert_eq!(strategy, "fix_and_reload_config");
}

#[test]
fn recovery_internal_suggests_report_bug() {
    let strategy = recovery_strategy(ErrorCategory::Internal);
    assert_eq!(strategy, "report_bug");
}

#[test]
fn recovery_every_category_has_strategy() {
    for cat in ALL_CATEGORIES {
        let strategy = recovery_strategy(*cat);
        assert!(
            !strategy.is_empty(),
            "category {:?} has no recovery strategy",
            cat
        );
    }
}

#[test]
fn recovery_backoff_only_for_backend() {
    for cat in ALL_CATEGORIES {
        if suggests_backoff(*cat) {
            assert_eq!(*cat, ErrorCategory::Backend);
        }
    }
}

#[test]
fn recovery_circuit_breaker_categories() {
    assert!(suggests_circuit_breaker(ErrorCategory::Backend));
    assert!(suggests_circuit_breaker(ErrorCategory::Execution));
    assert!(!suggests_circuit_breaker(ErrorCategory::Config));
    assert!(!suggests_circuit_breaker(ErrorCategory::Policy));
}

#[test]
fn recovery_fail_fast_categories() {
    assert!(suggests_fail_fast(ErrorCategory::Contract));
    assert!(suggests_fail_fast(ErrorCategory::Policy));
    assert!(suggests_fail_fast(ErrorCategory::Config));
    assert!(!suggests_fail_fast(ErrorCategory::Backend));
    assert!(!suggests_fail_fast(ErrorCategory::Protocol));
}

#[test]
fn recovery_retryable_codes_align_with_backoff_strategy() {
    for code in ALL_CODES {
        if code.is_retryable() {
            assert!(
                suggests_backoff(code.category()),
                "retryable code {:?} should be in a backoff category",
                code
            );
        }
    }
}

#[test]
fn recovery_fail_fast_codes_are_not_retryable() {
    for code in ALL_CODES {
        if suggests_fail_fast(code.category()) {
            assert!(
                !code.is_retryable(),
                "fail-fast code {:?} should not be retryable",
                code
            );
        }
    }
}

#[test]
fn recovery_strategy_for_workspace_is_verify() {
    let strategy = recovery_strategy(ErrorCategory::Workspace);
    assert_eq!(strategy, "verify_filesystem");
}

#[test]
fn recovery_strategy_for_dialect_is_use_supported() {
    let strategy = recovery_strategy(ErrorCategory::Dialect);
    assert_eq!(strategy, "use_supported_dialect");
}

#[test]
fn recovery_strategy_for_receipt_is_recompute() {
    let strategy = recovery_strategy(ErrorCategory::Receipt);
    assert_eq!(strategy, "recompute_or_rebuild_chain");
}

#[test]
fn recovery_all_strategies_are_unique() {
    let mut seen = HashSet::new();
    for cat in ALL_CATEGORIES {
        let strategy = recovery_strategy(*cat);
        assert!(
            seen.insert(strategy),
            "duplicate strategy {strategy} for {:?}",
            cat
        );
    }
}

// =========================================================================
// 5. Error aggregation (10 tests)
// =========================================================================

/// Aggregate errors and find dominant category by count.
fn dominant_category(errors: &[AbpError]) -> Option<ErrorCategory> {
    let mut counts: HashMap<ErrorCategory, usize> = HashMap::new();
    for err in errors {
        *counts.entry(err.category()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(cat, _)| cat)
}

/// Check if any error in the collection is retryable.
fn any_retryable(errors: &[AbpError]) -> bool {
    errors.iter().any(|e| e.is_retryable())
}

/// Collect unique categories from a set of errors.
fn unique_categories(errors: &[AbpError]) -> HashSet<ErrorCategory> {
    errors.iter().map(|e| e.category()).collect()
}

/// Find the highest severity among errors.
fn max_severity(errors: &[AbpError]) -> &'static str {
    fn rank(sev: &str) -> u8 {
        match sev {
            "critical" => 3,
            "error" => 2,
            "warning" => 1,
            _ => 0,
        }
    }
    let mut max = "warning";
    for e in errors {
        let sev = severity_of(&e.code);
        if rank(sev) > rank(max) {
            max = sev;
        }
    }
    max
}

#[test]
fn aggregation_dominant_category_single() {
    let errors = vec![AbpError::new(ErrorCode::BackendTimeout, "timeout")];
    assert_eq!(dominant_category(&errors), Some(ErrorCategory::Backend));
}

#[test]
fn aggregation_dominant_category_majority() {
    let errors = vec![
        AbpError::new(ErrorCode::BackendTimeout, "t1"),
        AbpError::new(ErrorCode::BackendUnavailable, "t2"),
        AbpError::new(ErrorCode::BackendRateLimited, "t3"),
        AbpError::new(ErrorCode::PolicyDenied, "p1"),
    ];
    assert_eq!(dominant_category(&errors), Some(ErrorCategory::Backend));
}

#[test]
fn aggregation_empty_has_no_dominant() {
    let errors: Vec<AbpError> = vec![];
    assert_eq!(dominant_category(&errors), None);
}

#[test]
fn aggregation_any_retryable_with_mixed_errors() {
    let errors = vec![
        AbpError::new(ErrorCode::PolicyDenied, "denied"),
        AbpError::new(ErrorCode::BackendTimeout, "timeout"),
        AbpError::new(ErrorCode::ConfigInvalid, "bad config"),
    ];
    assert!(any_retryable(&errors));
}

#[test]
fn aggregation_none_retryable() {
    let errors = vec![
        AbpError::new(ErrorCode::PolicyDenied, "denied"),
        AbpError::new(ErrorCode::ConfigInvalid, "bad config"),
        AbpError::new(ErrorCode::Internal, "bug"),
    ];
    assert!(!any_retryable(&errors));
}

#[test]
fn aggregation_unique_categories_from_mixed_errors() {
    let errors = vec![
        AbpError::new(ErrorCode::BackendTimeout, "t1"),
        AbpError::new(ErrorCode::BackendCrashed, "t2"),
        AbpError::new(ErrorCode::PolicyDenied, "p1"),
        AbpError::new(ErrorCode::ConfigInvalid, "c1"),
    ];
    let cats = unique_categories(&errors);
    assert_eq!(cats.len(), 3);
    assert!(cats.contains(&ErrorCategory::Backend));
    assert!(cats.contains(&ErrorCategory::Policy));
    assert!(cats.contains(&ErrorCategory::Config));
}

#[test]
fn aggregation_max_severity_picks_critical() {
    let errors = vec![
        AbpError::new(ErrorCode::BackendTimeout, "error-level"),
        AbpError::new(ErrorCode::PolicyDenied, "warning-level"),
        AbpError::new(ErrorCode::Internal, "critical-level"),
    ];
    assert_eq!(max_severity(&errors), "critical");
}

#[test]
fn aggregation_max_severity_all_warnings() {
    let errors = vec![
        AbpError::new(ErrorCode::PolicyDenied, "w1"),
        AbpError::new(ErrorCode::MappingLossyConversion, "w2"),
    ];
    assert_eq!(max_severity(&errors), "warning");
}

#[test]
fn aggregation_collect_error_infos() {
    let errors = [
        AbpError::new(ErrorCode::BackendTimeout, "t1").with_context("backend", "openai"),
        AbpError::new(ErrorCode::BackendRateLimited, "t2").with_context("backend", "anthropic"),
    ];
    let infos: Vec<ErrorInfo> = errors.iter().map(|e| e.to_info()).collect();
    assert_eq!(infos.len(), 2);
    assert!(infos.iter().all(|i| i.is_retryable));
    assert_eq!(infos[0].details["backend"], serde_json::json!("openai"));
    assert_eq!(
        infos[1].details["backend"],
        serde_json::json!("anthropic")
    );
}

#[test]
fn aggregation_group_errors_by_category() {
    let errors = vec![
        AbpError::new(ErrorCode::BackendTimeout, "t1"),
        AbpError::new(ErrorCode::PolicyDenied, "p1"),
        AbpError::new(ErrorCode::BackendCrashed, "t2"),
        AbpError::new(ErrorCode::ConfigInvalid, "c1"),
        AbpError::new(ErrorCode::PolicyInvalid, "p2"),
    ];
    let mut grouped: HashMap<ErrorCategory, Vec<&AbpError>> = HashMap::new();
    for err in &errors {
        grouped.entry(err.category()).or_default().push(err);
    }
    assert_eq!(grouped[&ErrorCategory::Backend].len(), 2);
    assert_eq!(grouped[&ErrorCategory::Policy].len(), 2);
    assert_eq!(grouped[&ErrorCategory::Config].len(), 1);
}

// =========================================================================
// 6. Serde stability (10 tests)
// =========================================================================

#[test]
fn serde_error_code_serializes_to_as_str() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "mismatch for {:?}", code);
    }
}

#[test]
fn serde_error_category_roundtrip_all() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *cat, "category roundtrip failed for {:?}", cat);
    }
}

#[test]
fn serde_error_category_is_snake_case_string() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let inner = json.trim_matches('"');
        assert!(
            inner.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "category serialized to non-snake_case: {inner}"
        );
    }
}

#[test]
fn serde_dto_roundtrip_without_source() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
    assert!(back.source_message.is_none());
}

#[test]
fn serde_dto_roundtrip_with_source_message() {
    let inner = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(inner);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn serde_dto_omits_null_source_message() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(
        !json.contains("source_message"),
        "null source_message should be omitted"
    );
}

#[test]
fn serde_error_info_roundtrip_with_details() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "slow down")
        .with_detail("retry_after_ms", 5000)
        .with_detail("backend", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn serde_dto_context_preserves_nested_json() {
    let err = AbpError::new(ErrorCode::Internal, "nested")
        .with_context("details", serde_json::json!({"a": 1, "b": [2, 3]}));
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.context["details"],
        serde_json::json!({"a": 1, "b": [2, 3]})
    );
}

#[test]
fn serde_dto_context_keys_are_sorted() {
    let err = AbpError::new(ErrorCode::Internal, "ordered")
        .with_context("zebra", 1)
        .with_context("alpha", 2);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let a_pos = json.find("alpha").unwrap();
    let z_pos = json.find("zebra").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should sort keys alphabetically");
}

#[test]
fn serde_invalid_json_rejected() {
    let result = serde_json::from_str::<ErrorCode>("\"not_a_real_code\"");
    assert!(result.is_err(), "unknown code should fail to deserialize");
}
