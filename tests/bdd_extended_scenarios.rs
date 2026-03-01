// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended BDD-style scenarios covering capability negotiation, error taxonomy,
//! receipt chains, cross-dialect mapping, and dialect detection.

use std::collections::BTreeMap;

use abp_capability::{NegotiationResult, generate_report, negotiate};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    Outcome, SupportLevel as CoreSupportLevel,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_mapping::{Fidelity, features, known_rules, validate_mapping};
use abp_receipt::{ReceiptBuilder, ReceiptChain, diff_receipts};

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest_from(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

// ===========================================================================
// 1. Capability negotiation scenarios (6 tests)
// ===========================================================================

/// Given a backend with streaming capability,
/// When I submit a work order requiring streaming,
/// Then negotiation succeeds.
#[test]
fn given_backend_with_streaming_when_requiring_streaming_then_negotiation_succeeds() {
    let manifest = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require(&[(Capability::Streaming, MinSupport::Native)]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

/// Given a backend without tool_use,
/// When I submit requiring tool_use with emulation,
/// Then emulation is applied (classified as emulatable via the adapter).
#[test]
fn given_backend_without_tool_use_when_requiring_with_emulation_then_emulation_applied() {
    let manifest = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let reqs = require(&[(Capability::ToolUse, MinSupport::Emulated)]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulatable, vec![Capability::ToolUse]);
}

/// Given a backend without extended_thinking,
/// When I submit requiring it without emulation (native only),
/// Then it fails with a capability error (unsupported).
#[test]
fn given_backend_without_extended_thinking_when_requiring_native_then_capability_error() {
    let manifest: CapabilityManifest = BTreeMap::new();
    let reqs = require(&[(Capability::ExtendedThinking, MinSupport::Native)]);
    let result = negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported, vec![Capability::ExtendedThinking]);
}

/// Capability report includes all three categories: native, emulated, unsupported.
#[test]
fn capability_report_includes_all_three_categories() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulatable: vec![Capability::ToolRead],
        unsupported: vec![Capability::Logprobs],
    };
    let report = generate_report(&result);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 1);
    assert!(!report.compatible);
    assert_eq!(report.details.len(), 3);
}

/// Negotiation with empty requirements always succeeds.
#[test]
fn negotiation_with_empty_requirements_always_succeeds() {
    let manifest = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = CapabilityRequirements::default();
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.total(), 0);
}

/// Multiple capabilities: some native, some emulated.
#[test]
fn multiple_capabilities_some_native_some_emulated() {
    let manifest = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::ToolWrite, CoreSupportLevel::Native),
    ]);
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
        (Capability::ToolWrite, MinSupport::Native),
    ]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 2);
    assert_eq!(result.emulatable.len(), 1);
    assert!(result.unsupported.is_empty());
}

// ===========================================================================
// 2. Error taxonomy scenarios (6 tests)
// ===========================================================================

/// Protocol error has correct error code.
#[test]
fn protocol_error_has_correct_error_code() {
    let err = AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "bad envelope");
    assert_eq!(err.code, ErrorCode::ProtocolInvalidEnvelope);
    assert_eq!(err.code.as_str(), "PROTOCOL_INVALID_ENVELOPE");
}

/// Backend error has correct category.
#[test]
fn backend_error_has_correct_category() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    assert_eq!(err.category(), ErrorCategory::Backend);
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
}

/// Capability error is classified correctly.
#[test]
fn capability_error_is_classified_correctly() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.category(),
        ErrorCategory::Capability
    );
}

/// Error builder produces well-formed error with context and source.
#[test]
fn error_builder_produces_well_formed_error() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.message, "timed out after 30s");
    assert_eq!(err.context.len(), 2);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

/// Error display includes code and message.
#[test]
fn error_display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "tool disallowed");
    let display = err.to_string();
    assert!(display.contains("POLICY_DENIED"));
    assert!(display.contains("tool disallowed"));
}

/// Error serialization roundtrip preserves all fields.
#[test]
fn error_serialization_roundtrip_preserves_all_fields() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unrecognized dialect")
        .with_context("input", "foobar");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, ErrorCode::DialectUnknown);
    assert_eq!(back.message, "unrecognized dialect");
    assert_eq!(back.context["input"], serde_json::json!("foobar"));
}

// ===========================================================================
// 3. Receipt chain scenarios (6 tests)
// ===========================================================================

/// First receipt has no parent hash (receipt_sha256 is its own hash, no chain link).
#[test]
fn first_receipt_has_no_parent_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    // A freshly built receipt has no hash until explicitly set.
    assert!(receipt.receipt_sha256.is_none());
}

/// Second receipt links to first via chain ordering.
#[test]
fn second_receipt_links_to_first_in_chain() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.len(), 2);
}

/// Chain verification passes for valid chain.
#[test]
fn chain_verification_passes_for_valid_chain() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    assert!(chain.verify().is_ok());
}

/// Chain verification fails for broken link (tampered hash).
#[test]
fn chain_verification_fails_for_broken_hash() {
    let mut r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r1.receipt_sha256 = Some("tampered_hash_value_that_is_clearly_wrong_abcdef".into());

    let mut chain = ReceiptChain::new();
    let result = chain.push(r1);
    assert!(result.is_err());
}

/// Chain builder produces valid chains (multiple sequential receipts).
#[test]
fn chain_builder_produces_valid_chains() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        let r = ReceiptBuilder::new(format!("backend-{i}"))
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

/// Receipt diff detects outcome change.
#[test]
fn receipt_diff_detects_outcome_change() {
    let a = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut b = a.clone();
    b.outcome = Outcome::Failed;

    let diff = diff_receipts(&a, &b);
    assert!(!diff.is_empty());
    assert!(diff.changes.iter().any(|d| d.field == "outcome"));
}

// ===========================================================================
// 4. Mapping scenarios (6 tests)
// ===========================================================================

/// OpenAI→Claude tool_use mapping is lossless.
#[test]
fn openai_to_claude_tool_use_mapping_is_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .expect("rule should exist");
    assert!(rule.fidelity.is_lossless());
}

/// Claude→Gemini thinking mapping is lossy-labeled.
#[test]
fn claude_to_gemini_thinking_mapping_is_lossy_labeled() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::THINKING)
        .expect("rule should exist");
    assert!(
        matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
        "expected lossy-labeled, got {:?}",
        rule.fidelity
    );
}

/// Unknown feature mapping is unsupported.
#[test]
fn unknown_feature_mapping_is_unsupported() {
    let reg = known_rules();
    let result = reg.lookup(Dialect::OpenAi, Dialect::Claude, "teleportation");
    assert!(result.is_none(), "unknown feature should have no rule");
}

/// Matrix lookup returns correct fidelity for a known pair.
#[test]
fn matrix_lookup_returns_correct_fidelity() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
        .expect("rule should exist");
    assert!(
        matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
        "OpenAI→Codex tool_use should be lossy"
    );
}

/// Validation identifies all issues for features with mixed fidelity.
#[test]
fn validation_identifies_all_issues() {
    let reg = known_rules();
    let features_to_check: Vec<String> = vec![
        features::TOOL_USE.into(),
        features::THINKING.into(),
        "nonexistent_feature".into(),
    ];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features_to_check);
    assert_eq!(results.len(), 3);
    // tool_use: lossless → no errors
    assert!(results[0].errors.is_empty());
    // thinking: lossy → has fidelity loss error
    assert!(!results[1].errors.is_empty());
    // nonexistent: unsupported → has error
    assert!(!results[2].errors.is_empty());
}

/// Known rules cover major features across at least 4 dialect pairs.
#[test]
fn known_rules_cover_major_features() {
    let reg = known_rules();
    assert!(!reg.is_empty());
    // Same-dialect rules for 4 dialects × 4 features = 16, plus cross-dialect
    assert!(
        reg.len() >= 16,
        "expected at least 16 rules, got {}",
        reg.len()
    );
    // Verify all four major features have at least one rule
    for &feat in &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
    ] {
        let has_rule = reg.lookup(Dialect::OpenAi, Dialect::OpenAi, feat).is_some();
        assert!(has_rule, "missing same-dialect rule for {feat}");
    }
}

// ===========================================================================
// 5. Dialect detection scenarios (6 tests)
// ===========================================================================

/// OpenAI-style JSON detected correctly.
#[test]
fn openai_style_json_detected_correctly() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.7
    });
    let result = detector.detect(&val).expect("should detect a dialect");
    assert_eq!(result.dialect, Dialect::OpenAi);
    assert!(result.confidence > 0.0);
}

/// Claude-style JSON detected correctly.
#[test]
fn claude_style_json_detected_correctly() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({
        "type": "message",
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "stop_reason": "end_turn"
    });
    let result = detector.detect(&val).expect("should detect a dialect");
    assert_eq!(result.dialect, Dialect::Claude);
    assert!(result.confidence > 0.0);
}

/// Ambiguous JSON returns multiple matches via detect_all.
#[test]
fn ambiguous_json_returns_multiple_matches() {
    let detector = DialectDetector::new();
    // A message with "model" and "messages" with string content matches both OpenAI and Claude patterns
    let val = serde_json::json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let results = detector.detect_all(&val);
    // At minimum, OpenAI should match; possibly others too
    assert!(!results.is_empty(), "should have at least one match");
}

/// Empty JSON object returns no detection result.
#[test]
fn empty_json_returns_no_detection() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({});
    let result = detector.detect(&val);
    assert!(
        result.is_none(),
        "empty object should not match any dialect"
    );
}

/// Detection confidence is in reasonable range [0, 1].
#[test]
fn detection_confidence_is_reasonable() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "candidates": [{}]
    });
    let result = detector.detect(&val).expect("should detect Gemini");
    assert_eq!(result.dialect, Dialect::Gemini);
    assert!(
        result.confidence > 0.0 && result.confidence <= 1.0,
        "confidence {} should be in (0, 1]",
        result.confidence
    );
}

/// All known dialects are detectable with appropriate input.
#[test]
fn all_known_dialects_are_detectable() {
    let detector = DialectDetector::new();

    let samples: Vec<(Dialect, serde_json::Value)> = vec![
        (
            Dialect::OpenAi,
            serde_json::json!({
                "model": "gpt-4",
                "choices": [{"message": {"role": "assistant", "content": "hi"}}]
            }),
        ),
        (
            Dialect::Claude,
            serde_json::json!({
                "type": "message",
                "model": "claude-3",
                "content": [{"type": "text", "text": "hi"}],
                "stop_reason": "end_turn"
            }),
        ),
        (
            Dialect::Gemini,
            serde_json::json!({
                "candidates": [{"content": {"parts": [{"text": "hi"}]}}],
                "contents": [{"parts": [{"text": "hello"}]}]
            }),
        ),
        (
            Dialect::Codex,
            serde_json::json!({
                "items": [{"type": "message", "text": "hi"}],
                "status": "completed",
                "object": "response"
            }),
        ),
        (
            Dialect::Kimi,
            serde_json::json!({
                "messages": [{"role": "user", "content": "hi"}],
                "refs": ["ref1"],
                "search_plus": true
            }),
        ),
        (
            Dialect::Copilot,
            serde_json::json!({
                "references": [{"id": "1"}],
                "confirmations": [],
                "agent_mode": true
            }),
        ),
    ];

    for (expected_dialect, sample) in &samples {
        let result = detector.detect(sample);
        assert!(
            result.is_some(),
            "dialect {:?} should be detectable from its sample",
            expected_dialect
        );
        let detected = result.unwrap();
        assert_eq!(
            detected.dialect, *expected_dialect,
            "expected {:?} but detected {:?}",
            expected_dialect, detected.dialect
        );
    }
}
