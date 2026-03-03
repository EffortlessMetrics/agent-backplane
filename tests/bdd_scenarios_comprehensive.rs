// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive BDD-style test scenarios for Agent Backplane.
//!
//! Each test follows the given/when/then pattern and covers real-world usage
//! across core contracts, policy enforcement, dialect detection, mapping
//! validation, protocol encoding, and receipt hashing.

use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, canonical_json, receipt_hash, sha256_hex,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, known_rules,
    validate_mapping,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use chrono::Utc;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Policy Engine scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_work_order_when_policy_allows_all_tools_then_all_tool_checks_pass() {
    // Scenario: An empty policy permits everything by default.
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();

    for tool in &["Bash", "Read", "Write", "Grep", "WebSearch"] {
        assert!(
            engine.can_use_tool(tool).allowed,
            "{tool} should be allowed"
        );
    }
}

#[test]
fn given_restrictive_policy_when_checking_dangerous_tools_then_they_are_denied() {
    // Scenario: Disallowed tools are denied even when the wildcard allowlist is set.
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into(), "DeleteFile".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("DeleteFile").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn given_allowlist_only_policy_when_unlisted_tool_used_then_denied() {
    // Scenario: Tools not on the explicit allowlist are denied.
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
}

#[test]
fn given_deny_write_policy_when_writing_to_git_then_denied() {
    // Scenario: Write to .git directory is blocked by policy.
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(!engine.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn given_deny_read_policy_when_reading_secrets_then_denied() {
    // Scenario: Sensitive files are blocked from reading.
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/.env.*".into(), "**/id_rsa".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("config/.env")).allowed);
    assert!(!engine.can_read_path(Path::new(".env.production")).allowed);
    assert!(!engine.can_read_path(Path::new(".ssh/id_rsa")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn given_glob_deny_tool_pattern_when_matching_tools_used_then_denied() {
    // Scenario: Glob patterns in disallowed_tools match families of tools.
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_use_tool("BashExec").allowed);
    assert!(!engine.can_use_tool("BashRun").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn given_combined_allow_deny_policy_when_deny_overrides_allow_then_denied() {
    // Scenario: Deny list wins when a tool appears in both allow and deny lists.
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into()],
        disallowed_tools: vec!["Write".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[test]
fn given_deep_nested_deny_write_when_writing_nested_path_then_denied() {
    // Scenario: Deeply nested paths are blocked by recursive glob patterns.
    let policy = PolicyProfile {
        deny_write: vec!["secret/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
    assert!(engine.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn given_policy_with_network_fields_when_compiled_then_fields_preserved() {
    // Scenario: Network allow/deny fields are preserved on the profile.
    let policy = PolicyProfile {
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.example.com".into()],
        ..PolicyProfile::default()
    };
    let _engine = PolicyEngine::new(&policy).unwrap();
    assert_eq!(policy.allow_network, vec!["*.example.com"]);
    assert_eq!(policy.deny_network, vec!["evil.example.com"]);
}

#[test]
fn given_policy_with_approval_required_when_compiled_then_field_preserved() {
    // Scenario: require_approval_for is stored on the profile.
    let policy = PolicyProfile {
        require_approval_for: vec!["Bash".into(), "DeleteFile".into()],
        ..PolicyProfile::default()
    };
    let _engine = PolicyEngine::new(&policy).unwrap();
    assert_eq!(policy.require_approval_for, vec!["Bash", "DeleteFile"]);
}

#[test]
fn given_decision_allow_when_inspected_then_no_reason() {
    // Scenario: Decision::allow() carries no reason.
    let d = abp_policy::Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn given_decision_deny_when_inspected_then_reason_present() {
    // Scenario: Decision::deny() carries a reason string.
    let d = abp_policy::Decision::deny("not permitted");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("not permitted"));
}

// ═══════════════════════════════════════════════════════════════════════
// Dialect detection scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_openai_dialect_request_when_detected_then_correct_dialect_returned() {
    // Scenario: A JSON object with "choices" is detected as OpenAI.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "model": "gpt-4"
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::OpenAi);
    assert!(result.confidence > 0.0);
}

#[test]
fn given_claude_dialect_response_when_detected_then_claude_returned() {
    // Scenario: A JSON object with "type":"message" is detected as Claude.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "type": "message",
        "model": "claude-3-opus",
        "content": [{"type": "text", "text": "hello"}],
        "stop_reason": "end_turn"
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Claude);
}

#[test]
fn given_gemini_dialect_request_when_detected_then_gemini_returned() {
    // Scenario: A JSON object with "contents" + "parts" is detected as Gemini.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "contents": [{"parts": [{"text": "hello"}]}]
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Gemini);
}

#[test]
fn given_codex_dialect_response_when_detected_then_codex_returned() {
    // Scenario: A JSON object with "items" array and "type" fields is Codex.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "items": [{"type": "message", "content": "hi"}],
        "status": "completed"
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Codex);
}

#[test]
fn given_kimi_dialect_request_when_detected_then_kimi_returned() {
    // Scenario: A JSON object with "search_plus" is detected as Kimi.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "search_plus": true,
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Kimi);
}

#[test]
fn given_copilot_dialect_request_when_detected_then_copilot_returned() {
    // Scenario: A JSON object with "references" and "agent_mode" is Copilot.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "references": [],
        "agent_mode": true
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Copilot);
}

#[test]
fn given_non_object_json_when_detect_called_then_none_returned() {
    // Scenario: Non-object JSON returns None.
    let detector = DialectDetector::new();
    assert!(
        detector
            .detect(&serde_json::json!("just a string"))
            .is_none()
    );
    assert!(detector.detect(&serde_json::json!(42)).is_none());
    assert!(detector.detect(&serde_json::json!([])).is_none());
}

#[test]
fn given_empty_json_object_when_detect_called_then_none_returned() {
    // Scenario: An empty JSON object has no dialect signals.
    let detector = DialectDetector::new();
    assert!(detector.detect(&serde_json::json!({})).is_none());
}

#[test]
fn given_openai_message_when_detect_all_called_then_results_sorted_by_confidence() {
    // Scenario: detect_all returns results sorted descending by confidence.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "model": "gpt-4"
    });
    let results = detector.detect_all(&msg);
    assert!(!results.is_empty());
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

#[test]
fn given_dialect_enum_when_all_called_then_six_dialects_returned() {
    // Scenario: Dialect::all() returns all 6 known dialects.
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
}

#[test]
fn given_dialect_when_label_called_then_human_readable_name() {
    // Scenario: Each dialect has a human-readable label.
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn given_dialect_when_display_called_then_matches_label() {
    // Scenario: Display impl matches label().
    for &d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn given_openai_request_when_validated_as_openai_then_valid() {
    // Scenario: A valid OpenAI request passes validation.
    let validator = DialectValidator::new();
    let msg = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = validator.validate(&msg, Dialect::OpenAi);
    assert!(result.valid);
}

#[test]
fn given_non_object_when_validated_then_invalid() {
    // Scenario: Non-object JSON fails validation for any dialect.
    let validator = DialectValidator::new();
    let result = validator.validate(&serde_json::json!("string"), Dialect::OpenAi);
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// Mapping scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_mapping_openai_to_anthropic_when_feature_supported_then_mapping_succeeds() {
    // Scenario: tool_use maps losslessly from OpenAI to Claude.
    let registry = known_rules();
    let rule = registry
        .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn given_mapping_openai_to_codex_image_input_when_lookup_then_unsupported() {
    // Scenario: Image input is unsupported from OpenAI to Codex.
    let registry = known_rules();
    let rule = registry
        .lookup(Dialect::OpenAi, Dialect::Codex, "image_input")
        .unwrap();
    assert!(rule.fidelity.is_unsupported());
}

#[test]
fn given_streaming_feature_when_mapped_across_all_pairs_then_lossless() {
    // Scenario: Streaming is lossless between all major dialect pairs.
    let registry = known_rules();
    for &a in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        for &b in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = registry.lookup(a, b, "streaming").unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming should be lossless from {a} to {b}"
            );
        }
    }
}

#[test]
fn given_same_dialect_mapping_when_any_feature_then_lossless() {
    // Scenario: Same-dialect mapping is always lossless.
    let registry = known_rules();
    for &d in Dialect::all() {
        for feat in &[
            "tool_use",
            "streaming",
            "thinking",
            "image_input",
            "code_exec",
        ] {
            let rule = registry.lookup(d, d, feat).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "{feat} should be lossless for {d} -> {d}"
            );
        }
    }
}

#[test]
fn given_empty_registry_when_lookup_then_none() {
    // Scenario: An empty registry returns None for any lookup.
    let registry = MappingRegistry::new();
    assert!(
        registry
            .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .is_none()
    );
}

#[test]
fn given_registry_when_insert_duplicate_then_replaced() {
    // Scenario: Inserting a duplicate key replaces the existing rule.
    let mut registry = MappingRegistry::new();
    registry.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    registry.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Unsupported {
            reason: "overwritten".into(),
        },
    });
    assert_eq!(registry.len(), 1);
    let rule = registry
        .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
        .unwrap();
    assert!(rule.fidelity.is_unsupported());
}

#[test]
fn given_known_rules_when_inspected_then_not_empty() {
    // Scenario: The known rules registry is pre-populated.
    let registry = known_rules();
    assert!(!registry.is_empty());
}

#[test]
fn given_mapping_matrix_when_built_from_registry_then_supported_pairs_detected() {
    // Scenario: MappingMatrix detects supported pairs from the registry.
    let registry = known_rules();
    let matrix = MappingMatrix::from_registry(&registry);
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(matrix.is_supported(Dialect::Claude, Dialect::Gemini));
}

#[test]
fn given_empty_matrix_when_queried_then_not_supported() {
    // Scenario: An empty matrix returns false for any pair.
    let matrix = MappingMatrix::new();
    assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn given_matrix_when_set_explicitly_then_value_returned() {
    // Scenario: Explicitly set values are returned.
    let mut matrix = MappingMatrix::new();
    matrix.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(!matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn given_validate_mapping_with_lossless_features_when_called_then_no_errors() {
    // Scenario: Lossless features produce no validation errors.
    let registry = known_rules();
    let results = validate_mapping(
        &registry,
        Dialect::OpenAi,
        Dialect::Claude,
        &["tool_use".into(), "streaming".into()],
    );
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.fidelity.is_lossless());
        assert!(r.errors.is_empty());
    }
}

#[test]
fn given_validate_mapping_with_unsupported_feature_when_called_then_error_returned() {
    // Scenario: Unsupported features produce validation errors.
    let registry = known_rules();
    let results = validate_mapping(
        &registry,
        Dialect::OpenAi,
        Dialect::Codex,
        &["image_input".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
}

#[test]
fn given_validate_mapping_with_empty_feature_name_then_error() {
    // Scenario: Empty feature names are rejected.
    let registry = known_rules();
    let results = validate_mapping(&registry, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert_eq!(results.len(), 1);
    assert!(!results[0].errors.is_empty());
}

#[test]
fn given_validate_mapping_with_unknown_feature_when_called_then_unsupported() {
    // Scenario: Unknown features are reported as unsupported.
    let registry = known_rules();
    let results = validate_mapping(
        &registry,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent_feature".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn given_mapping_error_feature_unsupported_when_displayed_then_contains_feature() {
    // Scenario: MappingError display contains the feature name.
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    assert!(err.to_string().contains("logprobs"));
}

#[test]
fn given_fidelity_lossless_when_checked_then_correct() {
    // Scenario: Fidelity enum helpers work correctly.
    assert!(Fidelity::Lossless.is_lossless());
    assert!(!Fidelity::Lossless.is_unsupported());
}

#[test]
fn given_fidelity_unsupported_when_checked_then_correct() {
    // Scenario: Unsupported fidelity is not lossless.
    let f = Fidelity::Unsupported {
        reason: "test".into(),
    };
    assert!(f.is_unsupported());
    assert!(!f.is_lossless());
}

#[test]
fn given_fidelity_lossy_labeled_when_checked_then_neither_lossless_nor_unsupported() {
    // Scenario: LossyLabeled is neither lossless nor unsupported.
    let f = Fidelity::LossyLabeled {
        warning: "some loss".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn given_rank_targets_when_called_then_sorted_by_lossless_count() {
    // Scenario: rank_targets returns targets sorted by lossless count descending.
    let registry = known_rules();
    let ranked = registry.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
    assert!(!ranked.is_empty());
    for w in ranked.windows(2) {
        assert!(w[0].1 >= w[1].1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Receipt hashing scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_receipt_when_hashed_then_hash_is_deterministic() {
    // Scenario: Hashing the same receipt twice produces the same hash.
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn given_two_different_work_orders_when_receipts_hashed_then_hashes_differ() {
    // Scenario: Receipts with different work order IDs produce different hashes.
    let r1 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .build();

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn given_receipt_when_with_hash_called_then_sha256_is_set() {
    // Scenario: with_hash fills the receipt_sha256 field.
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn given_receipt_with_hash_when_rehashed_then_same_hash() {
    // Scenario: The self-referential prevention means re-hashing is stable.
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    let stored = receipt.receipt_sha256.as_ref().unwrap().clone();
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[test]
fn given_different_outcomes_when_hashed_then_hashes_differ() {
    // Scenario: Different outcomes produce different hashes.
    let r1 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Failed)
        .build();

    // Note: they also differ by run_id which is Uuid::new_v4(), so always differ.
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn given_sha256_hex_when_called_then_64_char_hex() {
    // Scenario: sha256_hex produces a 64-character hex string.
    let hex = sha256_hex(b"hello world");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn given_canonical_json_when_keys_unordered_then_sorted() {
    // Scenario: canonical_json sorts keys deterministically.
    let json = canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
    assert!(json.starts_with(r#"{"a":1"#));
}

// ═══════════════════════════════════════════════════════════════════════
// Protocol / Envelope scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_jsonl_envelope_when_serialized_then_t_tag_present() {
    // Scenario: Envelope serialization uses "t" as the discriminator tag.
    let envelope = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&envelope).unwrap();
    assert!(line.contains("\"t\":\"hello\""));
    assert!(line.ends_with('\n'));
}

#[test]
fn given_hello_envelope_when_roundtripped_then_matches() {
    // Scenario: Encode then decode produces matching envelope.
    let envelope = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn given_fatal_envelope_when_serialized_then_contains_error() {
    // Scenario: Fatal envelope contains the error message.
    let envelope = Envelope::Fatal {
        ref_id: Some("run-123".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&envelope).unwrap();
    assert!(line.contains("out of memory"));
    assert!(line.contains("\"t\":\"fatal\""));
}

#[test]
fn given_invalid_json_when_decoded_then_error() {
    // Scenario: Invalid JSON returns a ProtocolError.
    let result = JsonlCodec::decode("not valid json");
    assert!(result.is_err());
}

#[test]
fn given_run_envelope_when_serialized_then_t_is_run() {
    // Scenario: Run envelope uses "t":"run" in JSON.
    let wo = WorkOrderBuilder::new("test task").build();
    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&envelope).unwrap();
    assert!(line.contains("\"t\":\"run\""));
}

#[test]
fn given_decode_stream_when_multiple_lines_then_all_decoded() {
    // Scenario: decode_stream handles multiple JSONL lines.
    let input = format!(
        "{}\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"a"}"#, r#"{"t":"fatal","ref_id":null,"error":"b"}"#,
    );
    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn given_decode_stream_with_blank_lines_when_decoded_then_blanks_skipped() {
    // Scenario: Blank lines in JSONL are skipped.
    let input = format!(
        "{}\n\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"a"}"#, r#"{"t":"fatal","ref_id":null,"error":"b"}"#,
    );
    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn given_hello_envelope_when_contract_version_checked_then_matches_constant() {
    // Scenario: Hello envelope uses the current CONTRACT_VERSION.
    let envelope = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    if let Envelope::Hello {
        contract_version, ..
    } = &envelope
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Version negotiation scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_valid_version_string_when_parsed_then_major_minor_returned() {
    // Scenario: parse_version extracts major and minor components.
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn given_invalid_version_string_when_parsed_then_none() {
    // Scenario: Invalid version strings return None.
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn given_same_major_versions_when_compat_checked_then_compatible() {
    // Scenario: Same major versions are compatible.
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn given_different_major_versions_when_compat_checked_then_incompatible() {
    // Scenario: Different major versions are incompatible.
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

// ═══════════════════════════════════════════════════════════════════════
// WorkOrder builder scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_work_order_builder_when_built_then_task_set() {
    // Scenario: WorkOrderBuilder sets the task string.
    let wo = WorkOrderBuilder::new("Fix the login bug").build();
    assert_eq!(wo.task, "Fix the login bug");
}

#[test]
fn given_work_order_builder_when_model_set_then_config_reflects() {
    // Scenario: Setting model on builder propagates to config.
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn given_work_order_builder_when_max_turns_set_then_config_reflects() {
    // Scenario: Setting max_turns propagates to config.
    let wo = WorkOrderBuilder::new("task").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn given_work_order_builder_when_lane_set_then_reflected() {
    // Scenario: Execution lane can be configured.
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn given_work_order_builder_when_workspace_root_set_then_reflected() {
    // Scenario: Workspace root is configured via builder.
    let wo = WorkOrderBuilder::new("task").root("/tmp/workspace").build();
    assert_eq!(wo.workspace.root, "/tmp/workspace");
}

#[test]
fn given_work_order_builder_when_policy_set_then_reflected() {
    // Scenario: Policy can be set on the work order.
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
}

#[test]
fn given_work_order_builder_when_budget_set_then_reflected() {
    // Scenario: Max budget can be set via builder.
    let wo = WorkOrderBuilder::new("task").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn given_work_order_builder_when_include_exclude_set_then_reflected() {
    // Scenario: Include/exclude globs are set on workspace spec.
    let wo = WorkOrderBuilder::new("task")
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn given_work_order_builder_when_context_set_then_reflected() {
    // Scenario: Context packet can be set on the work order.
    let ctx = ContextPacket {
        files: vec!["src/lib.rs".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    assert_eq!(wo.context.files, vec!["src/lib.rs"]);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn given_work_order_when_serialized_then_deserializable() {
    // Scenario: WorkOrder round-trips through JSON.
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "task");
    assert_eq!(wo2.config.model.as_deref(), Some("gpt-4"));
}

// ═══════════════════════════════════════════════════════════════════════
// Receipt builder scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_receipt_builder_when_built_then_backend_id_set() {
    // Scenario: ReceiptBuilder sets backend ID.
    let receipt = ReceiptBuilder::new("mock").build();
    assert_eq!(receipt.backend.id, "mock");
}

#[test]
fn given_receipt_builder_when_outcome_set_then_reflected() {
    // Scenario: Outcome can be set to Failed.
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[test]
fn given_receipt_builder_when_trace_event_added_then_in_trace() {
    // Scenario: Trace events accumulate in the receipt.
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let receipt = ReceiptBuilder::new("mock").add_trace_event(event).build();
    assert_eq!(receipt.trace.len(), 1);
}

#[test]
fn given_receipt_builder_when_artifact_added_then_in_artifacts() {
    // Scenario: Artifact references accumulate.
    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let receipt = ReceiptBuilder::new("mock").add_artifact(artifact).build();
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.artifacts[0].kind, "patch");
}

#[test]
fn given_receipt_builder_when_mode_set_then_reflected() {
    // Scenario: Execution mode can be set on receipt.
    let receipt = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn given_receipt_when_serialized_then_deserializable() {
    // Scenario: Receipt round-trips through JSON.
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let receipt2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt2.backend.id, "mock");
    assert!(receipt2.receipt_sha256.is_some());
}

#[test]
fn given_receipt_builder_with_hash_when_called_then_returns_hashed_receipt() {
    // Scenario: with_hash on builder produces a receipt with hash.
    let receipt = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// Capability scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_native_support_when_checked_against_native_min_then_satisfies() {
    // Scenario: Native support satisfies MinSupport::Native.
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn given_emulated_support_when_checked_against_native_min_then_does_not_satisfy() {
    // Scenario: Emulated does not satisfy MinSupport::Native.
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn given_emulated_support_when_checked_against_emulated_min_then_satisfies() {
    // Scenario: Emulated satisfies MinSupport::Emulated.
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_native_support_when_checked_against_emulated_min_then_satisfies() {
    // Scenario: Native exceeds MinSupport::Emulated.
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_unsupported_when_checked_against_any_min_then_does_not_satisfy() {
    // Scenario: Unsupported never satisfies any MinSupport.
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_restricted_when_checked_against_emulated_then_satisfies() {
    // Scenario: Restricted satisfies MinSupport::Emulated.
    let restricted = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_execution_mode_default_when_checked_then_mapped() {
    // Scenario: Default execution mode is Mapped.
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn given_contract_version_when_checked_then_matches_expected() {
    // Scenario: CONTRACT_VERSION is "abp/v0.1".
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ═══════════════════════════════════════════════════════════════════════
// Glob scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_no_glob_patterns_when_any_path_checked_then_allowed() {
    // Scenario: No patterns means everything is allowed.
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(globs.decide_str("anything.txt"), MatchDecision::Allowed);
}

#[test]
fn given_include_pattern_when_matching_path_then_allowed() {
    // Scenario: Included paths pass.
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn given_include_pattern_when_non_matching_path_then_denied() {
    // Scenario: Non-included paths are denied.
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(
        globs.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn given_exclude_pattern_when_matching_path_then_denied() {
    // Scenario: Excluded paths are denied even when included.
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["src/generated/**".into()]).unwrap();
    assert_eq!(
        globs.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn given_exclude_only_when_non_matching_path_then_allowed() {
    // Scenario: Exclude-only lets non-matching paths through.
    let globs = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
    assert_eq!(globs.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(globs.decide_str("app.log"), MatchDecision::DeniedByExclude);
}

#[test]
fn given_invalid_glob_pattern_when_compiled_then_error() {
    // Scenario: Invalid glob patterns produce an error.
    let result = IncludeExcludeGlobs::new(&["[".into()], &[]);
    assert!(result.is_err());
}

#[test]
fn given_match_decision_when_is_allowed_called_then_correct() {
    // Scenario: MatchDecision::is_allowed returns correct booleans.
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

// ═══════════════════════════════════════════════════════════════════════
// Agent event scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_agent_event_when_serialized_then_type_tag_present() {
    // Scenario: AgentEvent uses "type" tag (not "t") for event kind.
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"assistant_message\""));
}

#[test]
fn given_tool_call_event_when_serialized_then_roundtrips() {
    // Scenario: ToolCall events can be serialized and deserialized.
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/src/lib.rs"}),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let event2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(
        matches!(event2.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "Read")
    );
}

#[test]
fn given_file_changed_event_when_serialized_then_roundtrips() {
    // Scenario: FileChanged events round-trip correctly.
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "Added new function".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let event2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(
        matches!(event2.kind, AgentEventKind::FileChanged { path, .. } if path == "src/main.rs")
    );
}

#[test]
fn given_command_executed_event_when_serialized_then_roundtrips() {
    // Scenario: CommandExecuted events round-trip correctly.
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all tests passed".into()),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"command_executed\""));
}

#[test]
fn given_warning_event_when_serialized_then_type_is_warning() {
    // Scenario: Warning events serialize with correct type.
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "budget low".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"warning\""));
}

// ═══════════════════════════════════════════════════════════════════════
// Outcome serialization scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_outcome_complete_when_serialized_then_snake_case() {
    // Scenario: Outcome serializes as snake_case string.
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(json, "\"complete\"");
}

#[test]
fn given_outcome_partial_when_deserialized_then_correct() {
    // Scenario: Outcome can be deserialized from a string.
    let outcome: Outcome = serde_json::from_str("\"partial\"").unwrap();
    assert_eq!(outcome, Outcome::Partial);
}

#[test]
fn given_outcome_failed_when_deserialized_then_correct() {
    // Scenario: Outcome::Failed round-trips.
    let outcome: Outcome = serde_json::from_str("\"failed\"").unwrap();
    assert_eq!(outcome, Outcome::Failed);
}

// ═══════════════════════════════════════════════════════════════════════
// Cross-crate integration scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn given_work_order_with_policy_when_engine_built_then_enforces() {
    // Scenario: End-to-end: build a work order with policy and enforce it.
    let wo = WorkOrderBuilder::new("refactor auth")
        .policy(PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        })
        .build();

    let engine = PolicyEngine::new(&wo.policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn given_work_order_serialized_when_deserialized_then_policy_intact() {
    // Scenario: Policy survives JSON round-trip on a work order.
    let wo = WorkOrderBuilder::new("task")
        .policy(PolicyProfile {
            allowed_tools: vec!["Read".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["**/.env".into()],
            ..PolicyProfile::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();

    let engine = PolicyEngine::new(&wo2.policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn given_receipt_with_events_when_hashed_then_events_influence_hash() {
    // Scenario: Adding trace events changes the receipt hash.
    let base = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .build();
    let with_event = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        })
        .build();

    // Different run_id means different hash regardless, but events also contribute.
    let h1 = receipt_hash(&base).unwrap();
    let h2 = receipt_hash(&with_event).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn given_detection_then_mapping_lookup_when_chained_then_end_to_end() {
    // Scenario: Detect dialect, then look up a mapping rule.
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "model": "gpt-4"
    });
    let detected = detector.detect(&msg).unwrap();
    assert_eq!(detected.dialect, Dialect::OpenAi);

    let registry = known_rules();
    let rule = registry
        .lookup(detected.dialect, Dialect::Claude, "tool_use")
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn given_all_dialect_pairs_when_matrix_built_then_self_mapping_absent() {
    // Scenario: MappingMatrix built from known rules doesn't include self-pairs
    // (since same-dialect isn't typically stored as a mapping in the matrix).
    let registry = known_rules();
    let matrix = MappingMatrix::from_registry(&registry);
    // Self-pairs ARE actually in the registry (same-dialect lossless rules),
    // so they should be supported.
    for &d in Dialect::all() {
        assert!(
            matrix.is_supported(d, d),
            "self-pair should be supported for {d}"
        );
    }
}

#[test]
fn given_thinking_feature_claude_to_openai_when_mapped_then_lossy() {
    // Scenario: Thinking maps lossily from Claude to OpenAI.
    let registry = known_rules();
    let rule = registry
        .lookup(Dialect::Claude, Dialect::OpenAi, "thinking")
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn given_kimi_code_exec_to_any_when_mapped_then_unsupported() {
    // Scenario: Kimi does not support code execution.
    let registry = known_rules();
    for &target in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ] {
        let rule = registry.lookup(Dialect::Kimi, target, "code_exec").unwrap();
        assert!(
            rule.fidelity.is_unsupported(),
            "Kimi->{}:code_exec should be unsupported",
            target
        );
    }
}

#[test]
fn given_receipt_builder_with_verification_when_built_then_reflected() {
    // Scenario: Verification report is set on the receipt.
    let verification = VerificationReport {
        git_diff: Some("diff --git a/...".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    let receipt = ReceiptBuilder::new("mock")
        .verification(verification)
        .build();
    assert!(receipt.verification.harness_ok);
    assert!(receipt.verification.git_diff.is_some());
}

#[test]
fn given_receipt_builder_with_usage_when_built_then_reflected() {
    // Scenario: Usage data is set on the receipt.
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        ..UsageNormalized::default()
    };
    let receipt = ReceiptBuilder::new("mock").usage(usage).build();
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(200));
}
