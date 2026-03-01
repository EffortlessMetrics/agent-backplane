// SPDX-License-Identifier: MIT OR Apache-2.0
//! Mutation-resilience tests: designed to catch common mutants.
//!
//! Each test targets a specific class of mutation:
//! - Boundary conditions (off-by-one, <= vs <)
//! - Boolean logic (true/false swaps, && vs ||)
//! - Return value mutations (None vs Some, Ok vs Err)
//! - Arithmetic mutations (+ vs -, * vs /)
//! - String/collection operations (exact match, is_empty, len, contains)

use std::collections::BTreeMap;
use std::path::Path;

// ═══════════════════════════════════════════════════════════════════════════
// 1. receipt_hash — SHA-256 computation
// ═══════════════════════════════════════════════════════════════════════════

/// Hash length must be exactly 64 hex chars (SHA-256).
#[test]
fn receipt_hash_produces_64_hex_chars() {
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let hash = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest must be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be all hex digits"
    );
}

/// Same receipt must produce identical hashes (determinism).
#[test]
fn receipt_hash_is_deterministic() {
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "same receipt must hash identically");
}

/// Changing the outcome must change the hash.
#[test]
fn receipt_hash_changes_with_outcome() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let r2 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Failed)
        .build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2, "different outcomes must produce different hashes");
}

/// Pre-existing receipt_sha256 field must be nulled before hashing.
#[test]
fn receipt_hash_ignores_stored_hash() {
    let mut r = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    r.receipt_sha256 = Some("decafbad".into());
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "stored hash must not influence computed hash");
}

/// with_hash must populate receipt_sha256.
#[test]
fn with_hash_populates_receipt_sha256() {
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(
        r.receipt_sha256.is_some(),
        "with_hash must set receipt_sha256"
    );
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. abp-receipt — canonicalize and verify
// ═══════════════════════════════════════════════════════════════════════════

/// compute_hash must return exactly 64 hex chars.
#[test]
fn receipt_crate_compute_hash_len() {
    let r = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(abp_receipt::Outcome::Complete)
        .build();
    let h = abp_receipt::compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

/// verify_hash returns true for a correctly hashed receipt.
#[test]
fn receipt_verify_hash_true_on_valid() {
    let mut r = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(abp_receipt::Outcome::Complete)
        .build();
    r.receipt_sha256 = Some(abp_receipt::compute_hash(&r).unwrap());
    assert!(abp_receipt::verify_hash(&r), "valid hash must verify");
}

/// verify_hash returns false for a tampered hash.
#[test]
fn receipt_verify_hash_false_on_tampered() {
    let mut r = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(abp_receipt::Outcome::Complete)
        .build();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(
        !abp_receipt::verify_hash(&r),
        "tampered hash must not verify"
    );
}

/// verify_hash returns true when no hash is stored (None).
#[test]
fn receipt_verify_hash_true_when_none() {
    let r = abp_receipt::ReceiptBuilder::new("mock")
        .outcome(abp_receipt::Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    assert!(abp_receipt::verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. negotiate — capability classification
// ═══════════════════════════════════════════════════════════════════════════

/// All-native requirements → is_compatible must be true.
#[test]
fn negotiate_all_native_is_compatible() {
    let manifest: abp_core::CapabilityManifest = BTreeMap::from([
        (
            abp_core::Capability::Streaming,
            abp_core::SupportLevel::Native,
        ),
        (
            abp_core::Capability::ToolRead,
            abp_core::SupportLevel::Native,
        ),
    ]);
    let reqs = abp_core::CapabilityRequirements {
        required: vec![
            abp_core::CapabilityRequirement {
                capability: abp_core::Capability::Streaming,
                min_support: abp_core::MinSupport::Native,
            },
            abp_core::CapabilityRequirement {
                capability: abp_core::Capability::ToolRead,
                min_support: abp_core::MinSupport::Native,
            },
        ],
    };
    let result = abp_capability::negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 2);
    assert!(result.emulatable.is_empty());
    assert!(result.unsupported.is_empty());
}

/// Missing capabilities → is_compatible must be false, unsupported non-empty.
#[test]
fn negotiate_missing_caps_not_compatible() {
    let manifest: abp_core::CapabilityManifest = BTreeMap::new();
    let reqs = abp_core::CapabilityRequirements {
        required: vec![abp_core::CapabilityRequirement {
            capability: abp_core::Capability::Streaming,
            min_support: abp_core::MinSupport::Native,
        }],
    };
    let result = abp_capability::negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported.len(), 1);
    assert!(result.native.is_empty());
}

/// Empty requirements → compatible (boundary: zero items).
#[test]
fn negotiate_empty_requirements_is_compatible() {
    let manifest: abp_core::CapabilityManifest = BTreeMap::new();
    let reqs = abp_core::CapabilityRequirements::default();
    let result = abp_capability::negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.total(), 0);
}

/// NegotiationResult::total must equal sum of all buckets.
#[test]
fn negotiate_total_equals_sum_of_buckets() {
    let result = abp_capability::NegotiationResult {
        native: vec![abp_core::Capability::Streaming],
        emulatable: vec![abp_core::Capability::ToolRead],
        unsupported: vec![abp_core::Capability::Logprobs],
    };
    assert_eq!(result.total(), 3);
    assert_eq!(
        result.total(),
        result.native.len() + result.emulatable.len() + result.unsupported.len()
    );
}

/// Emulated support level classifies into emulatable bucket.
#[test]
fn negotiate_emulated_goes_to_emulatable() {
    let manifest: abp_core::CapabilityManifest = BTreeMap::from([(
        abp_core::Capability::Streaming,
        abp_core::SupportLevel::Emulated,
    )]);
    let reqs = abp_core::CapabilityRequirements {
        required: vec![abp_core::CapabilityRequirement {
            capability: abp_core::Capability::Streaming,
            min_support: abp_core::MinSupport::Native,
        }],
    };
    let result = abp_capability::negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulatable.len(), 1);
    assert!(result.native.is_empty());
}

/// Restricted maps to emulatable, not unsupported.
#[test]
fn negotiate_restricted_counts_as_emulatable() {
    let manifest: abp_core::CapabilityManifest = BTreeMap::from([(
        abp_core::Capability::ToolBash,
        abp_core::SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let reqs = abp_core::CapabilityRequirements {
        required: vec![abp_core::CapabilityRequirement {
            capability: abp_core::Capability::ToolBash,
            min_support: abp_core::MinSupport::Native,
        }],
    };
    let result = abp_capability::negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulatable.len(), 1);
    assert!(result.unsupported.is_empty());
}

/// check_capability returns Unsupported for missing key, not Native.
#[test]
fn check_capability_missing_is_unsupported() {
    let manifest: abp_core::CapabilityManifest = BTreeMap::new();
    let level = abp_capability::check_capability(&manifest, &abp_core::Capability::Streaming);
    assert_eq!(level, abp_capability::SupportLevel::Unsupported);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. validate_mapping — fidelity checking
// ═══════════════════════════════════════════════════════════════════════════

/// Empty feature name is rejected with an error.
#[test]
fn validate_mapping_rejects_empty_feature() {
    let registry = abp_mapping::MappingRegistry::new();
    let results = abp_mapping::validate_mapping(
        &registry,
        abp_dialect::Dialect::OpenAi,
        abp_dialect::Dialect::Claude,
        &[String::new()],
    );
    assert_eq!(results.len(), 1);
    assert!(
        !results[0].errors.is_empty(),
        "empty feature must produce error"
    );
    assert!(results[0].fidelity.is_unsupported());
}

/// Unknown feature (no rule) should be Unsupported.
#[test]
fn validate_mapping_unknown_feature_unsupported() {
    let registry = abp_mapping::MappingRegistry::new();
    let results = abp_mapping::validate_mapping(
        &registry,
        abp_dialect::Dialect::OpenAi,
        abp_dialect::Dialect::Claude,
        &["nonexistent_feature".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
}

/// Lossless rule must have zero errors and lossless fidelity.
#[test]
fn validate_mapping_lossless_no_errors() {
    let mut registry = abp_mapping::MappingRegistry::new();
    registry.insert(abp_mapping::MappingRule {
        source_dialect: abp_dialect::Dialect::OpenAi,
        target_dialect: abp_dialect::Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: abp_mapping::Fidelity::Lossless,
    });
    let results = abp_mapping::validate_mapping(
        &registry,
        abp_dialect::Dialect::OpenAi,
        abp_dialect::Dialect::Claude,
        &["tool_use".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

/// Lossy rule must produce a FidelityLoss error.
#[test]
fn validate_mapping_lossy_produces_error() {
    let mut registry = abp_mapping::MappingRegistry::new();
    registry.insert(abp_mapping::MappingRule {
        source_dialect: abp_dialect::Dialect::OpenAi,
        target_dialect: abp_dialect::Dialect::Claude,
        feature: "thinking".into(),
        fidelity: abp_mapping::Fidelity::LossyLabeled {
            warning: "partial support".into(),
        },
    });
    let results = abp_mapping::validate_mapping(
        &registry,
        abp_dialect::Dialect::OpenAi,
        abp_dialect::Dialect::Claude,
        &["thinking".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
    assert!(!results[0].fidelity.is_unsupported());
    assert_eq!(results[0].errors.len(), 1);
}

/// MappingRegistry::len must match number of inserted rules.
#[test]
fn mapping_registry_len_tracks_inserts() {
    let mut reg = abp_mapping::MappingRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
    reg.insert(abp_mapping::MappingRule {
        source_dialect: abp_dialect::Dialect::OpenAi,
        target_dialect: abp_dialect::Dialect::Claude,
        feature: "f1".into(),
        fidelity: abp_mapping::Fidelity::Lossless,
    });
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Policy evaluation — allow/deny decisions
// ═══════════════════════════════════════════════════════════════════════════

/// Empty policy allows everything (no false denials).
#[test]
fn policy_empty_allows_all() {
    let engine = abp_policy::PolicyEngine::new(&abp_core::PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

/// Deny list must deny the tool, allowed must be false.
#[test]
fn policy_deny_tool_is_false() {
    let policy = abp_core::PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed);
    assert!(decision.reason.is_some());
}

/// Allowlist blocks tools not in the list.
#[test]
fn policy_allowlist_blocks_unlisted() {
    let policy = abp_core::PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

/// Deny takes precedence over allow (deny overrides wildcard allow).
#[test]
fn policy_deny_overrides_allow() {
    let policy = abp_core::PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

/// deny_read blocks reading the matched path.
#[test]
fn policy_deny_read_blocks_path() {
    let policy = abp_core::PolicyProfile {
        deny_read: vec!["secret*".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secret.txt")).allowed);
    assert!(engine.can_read_path(Path::new("public.txt")).allowed);
}

/// deny_write blocks writing the matched path.
#[test]
fn policy_deny_write_blocks_path() {
    let policy = abp_core::PolicyProfile {
        deny_write: vec!["*.lock".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(engine.can_write_path(Path::new("Cargo.toml")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Glob matching — include/exclude logic
// ═══════════════════════════════════════════════════════════════════════════

/// No patterns → everything is Allowed (not denied).
#[test]
fn glob_no_patterns_allows_all() {
    let globs = abp_glob::IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(
        globs.decide_str("anything.txt"),
        abp_glob::MatchDecision::Allowed
    );
}

/// Include pattern gates: non-matching is DeniedByMissingInclude.
#[test]
fn glob_include_denies_non_matching() {
    let globs = abp_glob::IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(
        globs.decide_str("src/lib.rs"),
        abp_glob::MatchDecision::Allowed
    );
    assert_eq!(
        globs.decide_str("README.md"),
        abp_glob::MatchDecision::DeniedByMissingInclude
    );
}

/// Exclude takes precedence over include.
#[test]
fn glob_exclude_overrides_include() {
    let globs =
        abp_glob::IncludeExcludeGlobs::new(&["src/**".into()], &["src/secret/**".into()]).unwrap();
    assert_eq!(
        globs.decide_str("src/secret/key.pem"),
        abp_glob::MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("src/lib.rs"),
        abp_glob::MatchDecision::Allowed
    );
}

/// MatchDecision::is_allowed returns true only for Allowed.
#[test]
fn glob_is_allowed_only_for_allowed() {
    assert!(abp_glob::MatchDecision::Allowed.is_allowed());
    assert!(!abp_glob::MatchDecision::DeniedByExclude.is_allowed());
    assert!(!abp_glob::MatchDecision::DeniedByMissingInclude.is_allowed());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Error code taxonomy — category classification
// ═══════════════════════════════════════════════════════════════════════════

/// Every error code must map to the correct category.
#[test]
fn error_code_categories_are_correct() {
    use abp_error::{ErrorCategory, ErrorCode};
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

/// as_str must return non-empty SCREAMING_SNAKE_CASE strings.
#[test]
fn error_code_as_str_is_nonempty() {
    use abp_error::ErrorCode;
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
    ];
    for code in &codes {
        let s = code.as_str();
        assert!(!s.is_empty(), "as_str must not be empty for {code:?}");
        assert!(
            s.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
            "as_str must be SCREAMING_SNAKE_CASE: {s}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Config validation — boundary/boolean checks
// ═══════════════════════════════════════════════════════════════════════════

/// Zero timeout must be rejected (boundary: 0 is invalid, 1 is valid).
#[test]
fn config_zero_timeout_rejected() {
    let mut cfg = abp_config::BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        abp_config::BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    assert!(abp_config::validate_config(&cfg).is_err());
}

/// Timeout of 1 is the minimum valid value (boundary).
#[test]
fn config_timeout_one_is_valid() {
    let mut cfg = abp_config::BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        abp_config::BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    assert!(abp_config::validate_config(&cfg).is_ok());
}

/// Timeout exceeding 86400 must be rejected (boundary: max+1 is invalid).
#[test]
fn config_timeout_exceeding_max_rejected() {
    let mut cfg = abp_config::BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        abp_config::BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    assert!(abp_config::validate_config(&cfg).is_err());
}

/// Timeout at exactly 86400 must be accepted (boundary: max is valid).
#[test]
fn config_timeout_at_max_is_valid() {
    let mut cfg = abp_config::BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        abp_config::BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    assert!(abp_config::validate_config(&cfg).is_ok());
}

/// merge_configs: overlay Some wins over base Some.
#[test]
fn config_merge_overlay_wins() {
    let base = abp_config::BackplaneConfig {
        default_backend: Some("old".into()),
        ..Default::default()
    };
    let overlay = abp_config::BackplaneConfig {
        default_backend: Some("new".into()),
        ..Default::default()
    };
    let merged = abp_config::merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("new"));
}

/// merge_configs: overlay None falls back to base.
#[test]
fn config_merge_overlay_none_keeps_base() {
    let base = abp_config::BackplaneConfig {
        workspace_dir: Some("/work".into()),
        ..Default::default()
    };
    let overlay = abp_config::BackplaneConfig {
        workspace_dir: None,
        ..Default::default()
    };
    let merged = abp_config::merge_configs(base, overlay);
    assert_eq!(merged.workspace_dir.as_deref(), Some("/work"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Emulation — boolean/return-value mutations
// ═══════════════════════════════════════════════════════════════════════════

/// can_emulate must return true for ExtendedThinking.
#[test]
fn emulation_can_emulate_extended_thinking() {
    assert!(abp_emulation::can_emulate(
        &abp_core::Capability::ExtendedThinking
    ));
}

/// can_emulate must return false for CodeExecution.
#[test]
fn emulation_cannot_emulate_code_execution() {
    assert!(!abp_emulation::can_emulate(
        &abp_core::Capability::CodeExecution
    ));
}

/// EmulationReport::is_empty is true only when both applied and warnings are empty.
#[test]
fn emulation_report_is_empty_logic() {
    let empty = abp_emulation::EmulationReport::default();
    assert!(empty.is_empty());
    assert!(!empty.has_unemulatable());

    let with_warning = abp_emulation::EmulationReport {
        applied: vec![],
        warnings: vec!["warn".into()],
    };
    assert!(!with_warning.is_empty());
    assert!(with_warning.has_unemulatable());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Telemetry — arithmetic mutations
// ═══════════════════════════════════════════════════════════════════════════

/// Mean of [100, 200, 300] must be 200, not 0 or some other mutation.
#[test]
fn telemetry_mean_duration_exact() {
    let c = abp_telemetry::MetricsCollector::new();
    c.record(abp_telemetry::RunMetrics {
        duration_ms: 100,
        ..Default::default()
    });
    c.record(abp_telemetry::RunMetrics {
        duration_ms: 200,
        ..Default::default()
    });
    c.record(abp_telemetry::RunMetrics {
        duration_ms: 300,
        ..Default::default()
    });
    let s = c.summary();
    assert_eq!(s.count, 3);
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

/// Empty collector summary must have zero count and zero rates.
#[test]
fn telemetry_empty_collector_zeroes() {
    let c = abp_telemetry::MetricsCollector::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    let s = c.summary();
    assert_eq!(s.count, 0);
    assert_eq!(s.total_tokens_in, 0);
    assert_eq!(s.total_tokens_out, 0);
    assert_eq!(s.error_rate, 0.0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Dialect — string comparisons and detection
// ═══════════════════════════════════════════════════════════════════════════

/// Dialect labels must be non-empty and distinct.
#[test]
fn dialect_labels_non_empty_and_distinct() {
    let all = abp_dialect::Dialect::all();
    assert!(!all.is_empty());
    let labels: Vec<&str> = all.iter().map(|d| d.label()).collect();
    for label in &labels {
        assert!(!label.is_empty());
    }
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(unique.len(), labels.len(), "dialect labels must be unique");
}

/// Dialect::all must include at least OpenAi and Claude.
#[test]
fn dialect_all_contains_known_variants() {
    let all = abp_dialect::Dialect::all();
    assert!(all.contains(&abp_dialect::Dialect::OpenAi));
    assert!(all.contains(&abp_dialect::Dialect::Claude));
    assert!(all.contains(&abp_dialect::Dialect::Gemini));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Fidelity — boolean logic mutations
// ═══════════════════════════════════════════════════════════════════════════

/// is_lossless returns true only for Lossless variant.
#[test]
fn fidelity_is_lossless_only_for_lossless() {
    assert!(abp_mapping::Fidelity::Lossless.is_lossless());
    assert!(
        !abp_mapping::Fidelity::LossyLabeled {
            warning: "w".into()
        }
        .is_lossless()
    );
    assert!(!abp_mapping::Fidelity::Unsupported { reason: "r".into() }.is_lossless());
}

/// is_unsupported returns true only for Unsupported variant.
#[test]
fn fidelity_is_unsupported_only_for_unsupported() {
    assert!(abp_mapping::Fidelity::Unsupported { reason: "r".into() }.is_unsupported());
    assert!(!abp_mapping::Fidelity::Lossless.is_unsupported());
    assert!(
        !abp_mapping::Fidelity::LossyLabeled {
            warning: "w".into()
        }
        .is_unsupported()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. CONTRACT_VERSION — exact string match
// ═══════════════════════════════════════════════════════════════════════════

/// CONTRACT_VERSION must be exactly "abp/v0.1".
#[test]
fn contract_version_exact_value() {
    assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
    assert!(!abp_core::CONTRACT_VERSION.is_empty());
    assert!(abp_core::CONTRACT_VERSION.starts_with("abp/"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. MappingMatrix — boolean / collection mutations
// ═══════════════════════════════════════════════════════════════════════════

/// is_supported returns false for unset pairs (default = false, not true).
#[test]
fn mapping_matrix_default_is_unsupported() {
    let matrix = abp_mapping::MappingMatrix::new();
    assert!(!matrix.is_supported(abp_dialect::Dialect::OpenAi, abp_dialect::Dialect::Claude));
    assert!(
        matrix
            .get(abp_dialect::Dialect::OpenAi, abp_dialect::Dialect::Claude)
            .is_none()
    );
}

/// After set(true), is_supported returns true.
#[test]
fn mapping_matrix_set_true_is_supported() {
    let mut matrix = abp_mapping::MappingMatrix::new();
    matrix.set(
        abp_dialect::Dialect::OpenAi,
        abp_dialect::Dialect::Claude,
        true,
    );
    assert!(matrix.is_supported(abp_dialect::Dialect::OpenAi, abp_dialect::Dialect::Claude));
    assert_eq!(
        matrix.get(abp_dialect::Dialect::OpenAi, abp_dialect::Dialect::Claude),
        Some(true)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. SupportLevel::satisfies — boolean boundary mutations
// ═══════════════════════════════════════════════════════════════════════════

/// Native satisfies Native requirement.
#[test]
fn support_level_native_satisfies_native() {
    assert!(abp_core::SupportLevel::Native.satisfies(&abp_core::MinSupport::Native));
}

/// Emulated does NOT satisfy Native requirement.
#[test]
fn support_level_emulated_does_not_satisfy_native() {
    assert!(!abp_core::SupportLevel::Emulated.satisfies(&abp_core::MinSupport::Native));
}

/// Both Native and Emulated satisfy Emulated requirement.
#[test]
fn support_level_emulated_requirement_accepts_both() {
    assert!(abp_core::SupportLevel::Native.satisfies(&abp_core::MinSupport::Emulated));
    assert!(abp_core::SupportLevel::Emulated.satisfies(&abp_core::MinSupport::Emulated));
}

/// Unsupported does NOT satisfy any requirement.
#[test]
fn support_level_unsupported_satisfies_nothing() {
    assert!(!abp_core::SupportLevel::Unsupported.satisfies(&abp_core::MinSupport::Native));
    assert!(!abp_core::SupportLevel::Unsupported.satisfies(&abp_core::MinSupport::Emulated));
}
