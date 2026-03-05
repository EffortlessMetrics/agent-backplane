// SPDX-License-Identifier: MIT OR Apache-2.0
//! Mutation-resistant tests: designed to fail if common mutations are applied.
//!
//! Each test targets a specific mutation class: boundary values, boolean
//! negation, return value swaps, operator swaps, null/empty returns, and
//! early return deletion.

use std::collections::BTreeMap;
use std::path::Path;

use abp_capability::{
    EmulationStrategy, NegotiationResult, SupportLevel, check_capability, negotiate_capabilities,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    CONTRACT_VERSION, Capability, CapabilityManifest, ExecutionMode, Outcome, PolicyProfile,
    SupportLevel as CoreSupportLevel,
};
use abp_error::{ErrorCategory, ErrorCode};
use abp_error_taxonomy::{ClassificationCategory, ErrorClassifier, ErrorSeverity, RecoveryAction};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_ir::normalize;
use abp_policy::compose::{ComposedEngine, PolicyPrecedence};
use abp_policy::{Decision, PolicyEngine};
use abp_receipt::{ReceiptBuilder, compute_hash, verify_hash};

// ═══════════════════════════════════════════════════════════════════════
// § 1  Receipt hashing — must detect any algorithm / canonicalization change
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex must be exactly 64 chars");
    assert!(
        h.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be lowercase hex"
    );
}

#[test]
fn receipt_hash_deterministic_across_calls() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2, "same receipt must always produce the same hash");
}

#[test]
fn receipt_hash_changes_on_outcome_mutation() {
    let complete = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let failed = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_ne!(
        compute_hash(&complete).unwrap(),
        compute_hash(&failed).unwrap(),
        "different outcomes must produce different hashes"
    );
}

#[test]
fn receipt_hash_changes_on_backend_mutation() {
    let a = ReceiptBuilder::new("backend-a")
        .outcome(Outcome::Complete)
        .build();
    let b = ReceiptBuilder::new("backend-b")
        .outcome(Outcome::Complete)
        .build();
    assert_ne!(
        compute_hash(&a).unwrap(),
        compute_hash(&b).unwrap(),
        "different backends must produce different hashes"
    );
}

#[test]
fn verify_hash_true_when_correct() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    assert!(verify_hash(&r), "correct hash must verify as true");
}

#[test]
fn verify_hash_false_when_tampered() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some("0".repeat(64));
    assert!(!verify_hash(&r), "wrong hash must verify as false");
}

#[test]
fn verify_hash_true_when_none() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(
        verify_hash(&r),
        "receipt with no stored hash must verify as true"
    );
}

#[test]
fn receipt_hash_self_referential_prevention() {
    // Hashing must nullify receipt_sha256 before computing — changing
    // receipt_sha256 to any value must not alter the hash.
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h_none = compute_hash(&r).unwrap();
    r.receipt_sha256 = Some("ignored".into());
    let h_some = compute_hash(&r).unwrap();
    assert_eq!(
        h_none, h_some,
        "hash must be independent of receipt_sha256 field"
    );
}

#[test]
fn receipt_with_hash_produces_verifiable_receipt() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some(), "with_hash must set the hash");
    assert!(verify_hash(&r), "with_hash receipt must self-verify");
}

// ═══════════════════════════════════════════════════════════════════════
// § 2  Policy evaluation — must catch allow/deny inversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_empty_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn policy_disallowed_tool_is_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_use_tool("Bash");
    assert!(!d.allowed, "disallowed tool must be denied");
    assert!(d.reason.is_some(), "denial must include a reason");
}

#[test]
fn policy_allowlist_blocks_unlisted_tool() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(
        !engine.can_use_tool("Write").allowed,
        "unlisted tool must be blocked when allowlist is set"
    );
}

#[test]
fn policy_deny_beats_allow_for_tools() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine.can_use_tool("Bash").allowed,
        "deny must override wildcard allow"
    );
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_deny_read_blocks_path() {
    let policy = PolicyProfile {
        deny_read: vec!["secret/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(engine.can_read_path(Path::new("public/index.html")).allowed);
}

#[test]
fn policy_deny_write_blocks_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_decision_allow_fields() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn policy_decision_deny_fields() {
    let d = Decision::deny("forbidden");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("forbidden"));
}

#[test]
fn composed_deny_overrides_default() {
    let deny_bash = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let allow_all = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let engine =
        ComposedEngine::new(vec![allow_all, deny_bash], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(
        engine.check_tool("Bash").is_deny(),
        "DenyOverrides must deny when any policy denies"
    );
}

#[test]
fn composed_allow_overrides() {
    let deny_bash = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let allow_all = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let engine =
        ComposedEngine::new(vec![allow_all, deny_bash], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(
        engine.check_tool("Bash").is_allow(),
        "AllowOverrides must allow when any policy allows"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// § 3  Capability matching — must catch equality check changes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn check_capability_native() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn check_capability_missing_is_unsupported() {
    let m = CapabilityManifest::new();
    assert!(matches!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn check_capability_emulated() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolUse, CoreSupportLevel::Emulated);
    assert!(matches!(
        check_capability(&m, &Capability::ToolUse),
        SupportLevel::Emulated { .. }
    ));
}

#[test]
fn check_capability_restricted() {
    let mut m = CapabilityManifest::new();
    m.insert(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    );
    assert!(matches!(
        check_capability(&m, &Capability::ToolBash),
        SupportLevel::Restricted { .. }
    ));
}

#[test]
fn negotiate_all_native_is_viable() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    m.insert(Capability::ToolUse, CoreSupportLevel::Native);
    let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(result.is_viable());
    assert_eq!(result.native.len(), 2);
    assert!(result.unsupported.is_empty());
}

#[test]
fn negotiate_missing_capability_not_viable() {
    let m = CapabilityManifest::new();
    let result = negotiate_capabilities(&[Capability::Streaming], &m);
    assert!(
        !result.is_viable(),
        "missing cap must make result non-viable"
    );
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn negotiate_total_counts_all_buckets() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Audio, "not available".into())],
    };
    assert_eq!(
        result.total(),
        3,
        "total must be native + emulated + unsupported"
    );
}

#[test]
fn negotiate_emulated_caps_extracts_names() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    assert_eq!(result.emulated_caps(), vec![Capability::ToolRead]);
}

#[test]
fn negotiate_unsupported_caps_extracts_names() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![(Capability::Audio, "reason".into())],
    };
    assert_eq!(result.unsupported_caps(), vec![Capability::Audio]);
}

#[test]
fn emulation_strategy_fidelity_loss() {
    assert!(
        EmulationStrategy::Approximate.has_fidelity_loss(),
        "Approximate must report fidelity loss"
    );
    assert!(
        !EmulationStrategy::ClientSide.has_fidelity_loss(),
        "ClientSide must NOT report fidelity loss"
    );
    assert!(
        !EmulationStrategy::ServerFallback.has_fidelity_loss(),
        "ServerFallback must NOT report fidelity loss"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// § 4  Error classification — must catch severity / category swaps
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_category_mapping_protocol() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn error_category_mapping_backend() {
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
    assert_eq!(
        ErrorCode::BackendRateLimited.category(),
        ErrorCategory::Backend
    );
    assert_eq!(
        ErrorCode::BackendContentFiltered.category(),
        ErrorCategory::Backend
    );
}

#[test]
fn error_category_mapping_policy() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn error_category_mapping_receipt() {
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(
        ErrorCode::ReceiptStoreFailed.category(),
        ErrorCategory::Receipt
    );
}

#[test]
fn error_retryable_positive() {
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(ErrorCode::BackendCrashed.is_retryable());
    assert!(ErrorCode::RateLimitExceeded.is_retryable());
    assert!(ErrorCode::CircuitBreakerOpen.is_retryable());
    assert!(ErrorCode::StreamClosed.is_retryable());
}

#[test]
fn error_retryable_negative() {
    assert!(!ErrorCode::BackendNotFound.is_retryable());
    assert!(!ErrorCode::BackendAuthFailed.is_retryable());
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
    assert!(!ErrorCode::Internal.is_retryable());
    assert!(!ErrorCode::CapabilityUnsupported.is_retryable());
}

#[test]
fn error_as_str_stable() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
}

#[test]
fn error_classifier_severity_retriable() {
    let c = ErrorClassifier::new();
    let class = c.classify(&ErrorCode::BackendTimeout);
    assert_eq!(class.severity, ErrorSeverity::Retriable);
    assert_eq!(class.category, ClassificationCategory::TimeoutError);
}

#[test]
fn error_classifier_severity_fatal() {
    let c = ErrorClassifier::new();
    let class = c.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(class.severity, ErrorSeverity::Fatal);
    assert_eq!(class.category, ClassificationCategory::Authentication);
}

#[test]
fn error_classifier_severity_degraded() {
    let c = ErrorClassifier::new();
    let class = c.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(class.severity, ErrorSeverity::Degraded);
    assert_eq!(class.category, ClassificationCategory::MappingFailure);
}

#[test]
fn error_classifier_recovery_retry() {
    let c = ErrorClassifier::new();
    let class = c.classify(&ErrorCode::BackendRateLimited);
    assert_eq!(class.recovery.action, RecoveryAction::Retry);
    assert!(
        class.recovery.delay_ms.is_some(),
        "retry recovery must suggest a delay"
    );
}

#[test]
fn error_classifier_recovery_no_action() {
    let c = ErrorClassifier::new();
    // CapabilityEmulationFailed is Degraded + CapabilityUnsupported → Fallback
    // but the generic Degraded catch-all gives None. Use Informational to test that.
    // MappingLossyConversion is Degraded + MappingFailure → Fallback (specific match).
    // Test the specific: degraded mapping → fallback.
    let class = c.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(
        class.recovery.action,
        RecoveryAction::Fallback,
        "degraded mapping errors should suggest fallback"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// § 5  Glob path filtering — must catch any include/exclude inversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn glob_no_patterns_allows_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("any/path.txt"), MatchDecision::Allowed);
}

#[test]
fn glob_exclude_denies_match() {
    let g = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn glob_include_denies_non_match() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_exclude_beats_include() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &["src/generated/**".into()]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn glob_match_decision_is_allowed_returns_correct_bool() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn glob_decide_path_matches_decide_str() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &["*.log".into()]).unwrap();
    let path = Path::new("src/main.rs");
    assert_eq!(g.decide_path(path), g.decide_str("src/main.rs"));
}

// ═══════════════════════════════════════════════════════════════════════
// § 6  IR normalization — must catch early return / null return mutations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn normalize_role_canonical_names() {
    assert_eq!(normalize::normalize_role("system"), Some(IrRole::System));
    assert_eq!(normalize::normalize_role("user"), Some(IrRole::User));
    assert_eq!(
        normalize::normalize_role("assistant"),
        Some(IrRole::Assistant)
    );
    assert_eq!(normalize::normalize_role("tool"), Some(IrRole::Tool));
}

#[test]
fn normalize_role_vendor_aliases() {
    assert_eq!(normalize::normalize_role("model"), Some(IrRole::Assistant),);
    assert_eq!(normalize::normalize_role("function"), Some(IrRole::Tool));
    assert_eq!(normalize::normalize_role("developer"), Some(IrRole::System),);
    assert_eq!(normalize::normalize_role("human"), Some(IrRole::User));
    assert_eq!(normalize::normalize_role("bot"), Some(IrRole::Assistant));
}

#[test]
fn normalize_role_unknown_returns_none() {
    assert_eq!(normalize::normalize_role("narrator"), None);
    assert_eq!(normalize::normalize_role(""), None);
    assert_eq!(normalize::normalize_role("SYSTEM"), None);
}

#[test]
fn dedup_system_merges_multiple() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "rule A"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::System, "rule B"));
    let result = normalize::dedup_system(&conv);
    let sys_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .collect();
    assert_eq!(sys_msgs.len(), 1, "must merge into exactly one system msg");
    assert!(
        sys_msgs[0].text_content().contains("rule A"),
        "merged text must contain first system"
    );
    assert!(
        sys_msgs[0].text_content().contains("rule B"),
        "merged text must contain second system"
    );
}

#[test]
fn strip_empty_removes_contentless_messages() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hello"),
        IrMessage {
            role: IrRole::Assistant,
            content: vec![],
            metadata: BTreeMap::new(),
        },
    ]);
    let result = normalize::strip_empty(&conv);
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].role, IrRole::User);
}

#[test]
fn trim_text_strips_whitespace() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello  "));
    let result = normalize::trim_text(&conv);
    assert_eq!(result.messages[0].text_content(), "hello");
}

#[test]
fn normalize_pipeline_is_idempotent() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  sys  "))
        .push(IrMessage::text(IrRole::User, " hi "));
    let first = normalize::normalize(&conv);
    let second = normalize::normalize(&first);
    assert_eq!(
        first.messages.len(),
        second.messages.len(),
        "normalize must be idempotent"
    );
    for (a, b) in first.messages.iter().zip(second.messages.iter()) {
        assert_eq!(a.text_content(), b.text_content());
        assert_eq!(a.role, b.role);
    }
}

#[test]
fn sort_tools_deterministic_order() {
    let mut tools = vec![
        IrToolDefinition {
            name: "zebra".into(),
            description: String::new(),
            parameters: serde_json::json!({}),
        },
        IrToolDefinition {
            name: "alpha".into(),
            description: String::new(),
            parameters: serde_json::json!({}),
        },
    ];
    normalize::sort_tools(&mut tools);
    assert_eq!(tools[0].name, "alpha");
    assert_eq!(tools[1].name, "zebra");
}

#[test]
fn extract_system_separates_system_from_rest() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "be nice"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let (sys, rest) = normalize::extract_system(&conv);
    assert_eq!(sys.as_deref(), Some("be nice"));
    assert_eq!(rest.messages.len(), 1);
    assert_eq!(rest.messages[0].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════
// § 7  Contract constants — boundary value / return value checks
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_exact_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn outcome_variants_distinct() {
    assert_ne!(Outcome::Complete, Outcome::Partial);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Partial, Outcome::Failed);
}

// ═══════════════════════════════════════════════════════════════════════
// § 8  Operator swap guards — arithmetic in total(), len(), etc.
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn negotiation_total_is_sum_not_product() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    // 2 + 1 + 0 = 3   (mutation: * would give 0)
    assert_eq!(r.total(), 3);
}

#[test]
fn negotiation_total_zero_when_all_empty() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert_eq!(r.total(), 0, "empty result total must be zero");
}

#[test]
fn negotiation_viable_boundary_empty_unsupported() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert!(
        r.is_viable(),
        "no unsupported caps means viable (boundary: empty)"
    );
}

#[test]
fn negotiation_viable_boundary_one_unsupported() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![(Capability::Audio, "n/a".into())],
    };
    assert!(
        !r.is_viable(),
        "one unsupported cap makes it non-viable (off-by-one guard)"
    );
}

#[test]
fn error_category_distinct_for_every_domain() {
    // Catches mutation that maps two different codes to the same category
    let pairs: Vec<(ErrorCode, ErrorCategory)> = vec![
        (ErrorCode::ProtocolInvalidEnvelope, ErrorCategory::Protocol),
        (ErrorCode::BackendNotFound, ErrorCategory::Backend),
        (ErrorCode::CapabilityUnsupported, ErrorCategory::Capability),
        (ErrorCode::PolicyDenied, ErrorCategory::Policy),
        (ErrorCode::WorkspaceInitFailed, ErrorCategory::Workspace),
        (ErrorCode::IrLoweringFailed, ErrorCategory::Ir),
        (ErrorCode::ReceiptHashMismatch, ErrorCategory::Receipt),
        (ErrorCode::DialectUnknown, ErrorCategory::Dialect),
        (ErrorCode::ConfigInvalid, ErrorCategory::Config),
        (ErrorCode::MappingDialectMismatch, ErrorCategory::Mapping),
        (ErrorCode::ExecutionToolFailed, ErrorCategory::Execution),
        (ErrorCode::ContractVersionMismatch, ErrorCategory::Contract),
        (ErrorCode::RateLimitExceeded, ErrorCategory::RateLimit),
        (ErrorCode::StreamClosed, ErrorCategory::Stream),
        (ErrorCode::ValidationFailed, ErrorCategory::Validation),
        (ErrorCode::SidecarSpawnFailed, ErrorCategory::Sidecar),
        (ErrorCode::Internal, ErrorCategory::Internal),
    ];
    for (code, expected) in &pairs {
        assert_eq!(
            code.category(),
            *expected,
            "{code:?} must map to {expected:?}"
        );
    }
}

#[test]
fn normalize_tool_schemas_adds_type_object() {
    let tools = vec![IrToolDefinition {
        name: "my_tool".into(),
        description: "desc".into(),
        parameters: serde_json::json!({"properties": {}}),
    }];
    let normalized = normalize::normalize_tool_schemas(&tools);
    let ty = normalized[0].parameters.get("type");
    assert_eq!(
        ty,
        Some(&serde_json::json!("object")),
        "must inject type: object when missing"
    );
}

#[test]
fn merge_adjacent_text_coalesces() {
    let conv = IrConversation::from_messages(vec![IrMessage {
        role: IrRole::User,
        content: vec![
            IrContentBlock::Text {
                text: "hello ".into(),
            },
            IrContentBlock::Text {
                text: "world".into(),
            },
        ],
        metadata: BTreeMap::new(),
    }]);
    let result = normalize::merge_adjacent_text(&conv);
    assert_eq!(result.messages[0].content.len(), 1);
    assert_eq!(result.messages[0].text_content(), "hello world");
}
