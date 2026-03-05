#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive cross-SDK conformance and mapping fidelity tests.
//!
//! Validates that the Agent Backplane faithfully translates between all
//! six dialect surfaces, enforces passthrough/mapped mode invariants,
//! detects early failures for unmappable requests, and preserves
//! capability negotiation semantics across every dialect pair.

use std::collections::BTreeMap;

use abp_capability::{NegotiationResult, check_capability, generate_report, negotiate};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, CapabilityRequirement,
    CapabilityRequirements, ExecutionMode, MinSupport, SupportLevel,
    ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage},
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationStrategy, FidelityLabel, can_emulate,
    compute_fidelity,
};
use abp_mapping::{Fidelity, MappingError, MappingMatrix, features, known_rules, validate_mapping};
use abp_projection::{ProjectionError, ProjectionMatrix};
use abp_stream::{EventFilter, EventRecorder, EventStats, EventTransform, event_kind_name};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// All six ABP dialects.
const ALL_DIALECTS: &[Dialect] = &[
    Dialect::OpenAi,
    Dialect::Claude,
    Dialect::Gemini,
    Dialect::Codex,
    Dialect::Kimi,
    Dialect::Copilot,
];

/// The five well-known feature keys.
const ALL_FEATURES: &[&str] = &[
    features::TOOL_USE,
    features::STREAMING,
    features::THINKING,
    features::IMAGE_INPUT,
    features::CODE_EXEC,
];

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn manifest_with(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require_caps(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
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

// ═══════════════════════════════════════════════════════════════════════════
// 1. Dialect enum completeness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_six_variants() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_all_contains_openai() {
    assert!(Dialect::all().contains(&Dialect::OpenAi));
}

#[test]
fn dialect_all_contains_claude() {
    assert!(Dialect::all().contains(&Dialect::Claude));
}

#[test]
fn dialect_all_contains_gemini() {
    assert!(Dialect::all().contains(&Dialect::Gemini));
}

#[test]
fn dialect_all_contains_codex() {
    assert!(Dialect::all().contains(&Dialect::Codex));
}

#[test]
fn dialect_all_contains_kimi() {
    assert!(Dialect::all().contains(&Dialect::Kimi));
}

#[test]
fn dialect_all_contains_copilot() {
    assert!(Dialect::all().contains(&Dialect::Copilot));
}

#[test]
fn dialect_labels_are_unique() {
    let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
    let mut deduped = labels.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(labels.len(), deduped.len());
}

#[test]
fn dialect_display_matches_label() {
    for &d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn dialect_serde_roundtrip() {
    for &d in Dialect::all() {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn dialect_serde_uses_snake_case() {
    let json = serde_json::to_string(&Dialect::OpenAi).unwrap();
    assert_eq!(json, "\"open_ai\"");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Self-mapping is always lossless
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn self_mapping_lossless_for_all_dialects_all_features() {
    let reg = known_rules();
    for &d in ALL_DIALECTS {
        for &f in ALL_FEATURES {
            let rule = reg.lookup(d, d, f);
            assert!(
                rule.is_some(),
                "missing self-mapping rule for {d:?} feature {f}",
            );
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "self-mapping for {d:?} feature {f} is not lossless",
            );
        }
    }
}

#[test]
fn self_mapping_validation_yields_no_errors() {
    let reg = known_rules();
    for &d in ALL_DIALECTS {
        let feats: Vec<String> = ALL_FEATURES.iter().map(|f| f.to_string()).collect();
        let results = validate_mapping(&reg, d, d, &feats);
        for v in &results {
            assert!(
                v.errors.is_empty(),
                "self-mapping validation for {d:?} feature {} has errors: {:?}",
                v.feature,
                v.errors,
            );
        }
    }
}

#[test]
fn self_mapping_matrix_all_supported() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    for &d in ALL_DIALECTS {
        assert!(
            matrix.is_supported(d, d),
            "matrix says {d:?} -> {d:?} unsupported",
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Cross-dialect tool_use format mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_openai_to_claude_is_lossless() {
    let reg = known_rules();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_openai_to_gemini_is_lossless() {
    let reg = known_rules();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE);
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_openai_to_codex_is_lossy() {
    let reg = known_rules();
    let r = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE);
    assert!(!r.unwrap().fidelity.is_lossless());
    assert!(!r.unwrap().fidelity.is_unsupported());
}

#[test]
fn tool_use_claude_to_gemini_is_lossless() {
    let reg = known_rules();
    let r = reg.lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE);
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_kimi_to_openai_is_lossless() {
    let reg = known_rules();
    let r = reg.lookup(Dialect::Kimi, Dialect::OpenAi, features::TOOL_USE);
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_copilot_to_claude_is_lossless() {
    let reg = known_rules();
    let r = reg.lookup(Dialect::Copilot, Dialect::Claude, features::TOOL_USE);
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_kimi_to_copilot_is_lossless() {
    let reg = known_rules();
    let r = reg.lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE);
    assert!(r.unwrap().fidelity.is_lossless());
}

#[test]
fn tool_use_codex_to_any_is_lossy_labeled() {
    let reg = known_rules();
    for &target in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let r = reg
            .lookup(Dialect::Codex, target, features::TOOL_USE)
            .unwrap();
        assert!(
            matches!(r.fidelity, Fidelity::LossyLabeled { .. }),
            "Codex -> {target:?} tool_use should be LossyLabeled",
        );
    }
}

#[test]
fn tool_use_ir_roundtrip_preserves_structure() {
    let tool_use = IrContentBlock::ToolUse {
        id: "call_123".into(),
        name: "read_file".into(),
        input: json!({"path": "/tmp/test.rs"}),
    };
    let msg = IrMessage::new(IrRole::Assistant, vec![tool_use.clone()]);
    let conv = IrConversation::from_messages(vec![msg]);

    let serialized = serde_json::to_string(&conv).unwrap();
    let deserialized: IrConversation = serde_json::from_str(&serialized).unwrap();
    assert_eq!(conv, deserialized);
}

#[test]
fn tool_use_ir_extracts_tool_calls() {
    let blocks = vec![
        IrContentBlock::Text {
            text: "Let me read that file.".into(),
        },
        IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "read_file".into(),
            input: json!({"path": "src/lib.rs"}),
        },
        IrContentBlock::ToolUse {
            id: "call_2".into(),
            name: "write_file".into(),
            input: json!({"path": "out.txt", "content": "hello"}),
        },
    ];
    let msg = IrMessage::new(IrRole::Assistant, blocks);
    assert_eq!(msg.tool_use_blocks().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Cross-dialect streaming event mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_lossless_between_all_major_pairs() {
    let reg = known_rules();
    let major = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ];
    for &a in &major {
        for &b in &major {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::STREAMING);
            assert!(rule.is_some(), "missing streaming rule for {a:?} -> {b:?}",);
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "streaming {a:?} -> {b:?} should be lossless",
            );
        }
    }
}

#[test]
fn streaming_kimi_to_all_is_lossless() {
    let reg = known_rules();
    for &target in ALL_DIALECTS {
        if target == Dialect::Kimi {
            continue;
        }
        let rule = reg.lookup(Dialect::Kimi, target, features::STREAMING);
        assert!(
            rule.is_some(),
            "missing streaming rule for Kimi -> {target:?}",
        );
        assert!(
            rule.unwrap().fidelity.is_lossless(),
            "streaming Kimi -> {target:?} should be lossless",
        );
    }
}

#[test]
fn streaming_copilot_to_all_is_lossless() {
    let reg = known_rules();
    for &target in ALL_DIALECTS {
        if target == Dialect::Copilot {
            continue;
        }
        let rule = reg.lookup(Dialect::Copilot, target, features::STREAMING);
        assert!(
            rule.is_some(),
            "missing streaming rule for Copilot -> {target:?}",
        );
        assert!(
            rule.unwrap().fidelity.is_lossless(),
            "streaming Copilot -> {target:?} should be lossless",
        );
    }
}

#[test]
fn streaming_event_kind_names_are_stable() {
    let delta = AgentEventKind::AssistantDelta { text: "hi".into() };
    assert_eq!(event_kind_name(&delta), "assistant_delta");

    let msg = AgentEventKind::AssistantMessage { text: "hi".into() };
    assert_eq!(event_kind_name(&msg), "assistant_message");
}

#[test]
fn streaming_event_filter_by_kind() {
    let filter = EventFilter::by_kind("assistant_delta");
    let delta_ev = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    let msg_ev = make_event(AgentEventKind::AssistantMessage {
        text: "full".into(),
    });
    assert!(filter.matches(&delta_ev));
    assert!(!filter.matches(&msg_ev));
}

#[test]
fn streaming_event_recorder_captures_all() {
    let recorder = EventRecorder::new();
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "hel".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "lo".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    for ev in &events {
        recorder.record(ev);
    }
    assert_eq!(recorder.len(), 4);
}

#[test]
fn streaming_event_stats_counts_by_kind() {
    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }));
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }));
    stats.observe(&make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    }));
    assert_eq!(stats.total_events(), 3);
    assert_eq!(stats.error_count(), 1);
    assert_eq!(stats.count_for("assistant_delta"), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Cross-dialect error code mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_event_roundtrip_preserves_code() {
    let ev = make_event(AgentEventKind::Error {
        message: "rate limit exceeded".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let json = serde_json::to_value(&ev).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "rate limit exceeded");
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
        }
        _ => panic!("expected Error event"),
    }
}

#[test]
fn error_event_without_code_is_valid() {
    let ev = make_event(AgentEventKind::Error {
        message: "unknown error".into(),
        error_code: None,
    });
    let json = serde_json::to_value(&ev).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::Error { error_code, .. } => assert!(error_code.is_none()),
        _ => panic!("expected Error event"),
    }
}

#[test]
fn error_filter_captures_only_errors() {
    let filter = EventFilter::errors_only();
    let error_ev = make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    });
    let ok_ev = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    assert!(filter.matches(&error_ev));
    assert!(!filter.matches(&ok_ev));
}

#[test]
fn error_exclude_filter_skips_errors() {
    let filter = EventFilter::exclude_errors();
    let error_ev = make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    });
    let ok_ev = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    assert!(!filter.matches(&error_ev));
    assert!(filter.matches(&ok_ev));
}

#[test]
fn warning_event_roundtrip() {
    let ev = make_event(AgentEventKind::Warning {
        message: "quota low".into(),
    });
    let json = serde_json::to_value(&ev).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::Warning { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Passthrough mode verification
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_mode_is_not_default() {
    let mode = ExecutionMode::default();
    assert!(matches!(mode, ExecutionMode::Mapped));
}

#[test]
fn passthrough_mode_serde_roundtrip() {
    let mode = ExecutionMode::Passthrough;
    let json = serde_json::to_value(&mode).unwrap();
    let back: ExecutionMode = serde_json::from_value(json).unwrap();
    assert!(matches!(back, ExecutionMode::Passthrough));
}

#[test]
fn passthrough_mode_identity_transform() {
    let transform = EventTransform::identity();
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "hello".into(),
    });
    let transformed = transform.apply(ev.clone());
    // AgentEventKind doesn't impl PartialEq, so compare via serde
    let orig_json = serde_json::to_value(&ev.kind).unwrap();
    let trans_json = serde_json::to_value(&transformed.kind).unwrap();
    assert_eq!(orig_json, trans_json);
}

#[test]
fn passthrough_ir_conversation_unchanged() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ]);
    let serialized = serde_json::to_string(&conv).unwrap();
    let deserialized: IrConversation = serde_json::from_str(&serialized).unwrap();
    assert_eq!(conv, deserialized);
}

#[test]
fn passthrough_preserves_ext_metadata() {
    let mut ext = BTreeMap::new();
    ext.insert("vendor_id".into(), json!("abc-123"));
    ext.insert("trace_flags".into(), json!(42));
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: Some(ext.clone()),
    };
    let json = serde_json::to_value(&ev).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back.ext.unwrap(), ext);
}

#[test]
fn passthrough_preserves_tool_call_fields() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_001".into()),
        parent_tool_use_id: Some("tu_000".into()),
        input: json!({"command": "ls -la"}),
    });
    let json = serde_json::to_value(&ev).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match back.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input,
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_use_id.as_deref(), Some("tu_001"));
            assert_eq!(parent_tool_use_id.as_deref(), Some("tu_000"));
            assert_eq!(input, json!({"command": "ls -la"}));
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn passthrough_preserves_tool_result_fields() {
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_002".into()),
        output: json!("file contents here"),
        is_error: false,
    });
    let json = serde_json::to_value(&ev).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::ToolResult {
            tool_name,
            tool_use_id,
            output,
            is_error,
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("tu_002"));
            assert_eq!(output, &json!("file contents here"));
            assert!(!is_error);
        }
        _ => panic!("expected ToolResult"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Mapped mode fidelity checks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapped_mode_is_default() {
    assert!(matches!(ExecutionMode::default(), ExecutionMode::Mapped));
}

#[test]
fn mapped_mode_serde_roundtrip() {
    let mode = ExecutionMode::Mapped;
    let json = serde_json::to_value(&mode).unwrap();
    let back: ExecutionMode = serde_json::from_value(json).unwrap();
    assert!(matches!(back, ExecutionMode::Mapped));
}

#[test]
fn mapped_validation_all_features_openai_to_claude() {
    let reg = known_rules();
    let feats: Vec<String> = ALL_FEATURES.iter().map(|f| f.to_string()).collect();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(results.len(), ALL_FEATURES.len());
    for v in &results {
        assert!(
            !v.fidelity.is_unsupported(),
            "OpenAI -> Claude feature {} should not be unsupported",
            v.feature,
        );
    }
}

#[test]
fn mapped_validation_lossy_feature_has_warning() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &["thinking".into()]);
    assert_eq!(results.len(), 1);
    match &results[0].fidelity {
        Fidelity::LossyLabeled { warning } => {
            assert!(!warning.is_empty(), "lossy mapping must carry a warning");
        }
        other => panic!("expected LossyLabeled, got {other:?}"),
    }
}

#[test]
fn mapped_mode_rank_targets_prefers_lossless() {
    let reg = known_rules();
    let features = &[features::TOOL_USE, features::STREAMING];
    let ranked = reg.rank_targets(Dialect::OpenAi, features);
    assert!(
        !ranked.is_empty(),
        "OpenAI should have at least one mapping target",
    );
    // First ranked target should have higher or equal lossless count than later ones
    for window in ranked.windows(2) {
        assert!(window[0].1 >= window[1].1);
    }
}

#[test]
fn mapped_matrix_from_known_rules_is_populated() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    let mut supported_count = 0;
    for &a in ALL_DIALECTS {
        for &b in ALL_DIALECTS {
            if matrix.is_supported(a, b) {
                supported_count += 1;
            }
        }
    }
    // At minimum self-mappings (6) plus many cross-dialect pairs
    assert!(
        supported_count >= 6 + 20,
        "expected many supported pairs, got {supported_count}"
    );
}

#[test]
fn mapped_mode_emulation_is_labeled() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert!(!report.is_empty());
    assert!(
        !report.applied.is_empty(),
        "ExtendedThinking should have an emulation strategy",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Early failure on unmappable requests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unmappable_image_input_openai_to_codex_fails() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &["image_input".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
}

#[test]
fn unmappable_image_input_claude_to_codex_fails() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::Codex,
        &["image_input".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn unmappable_image_input_kimi_to_any_fails() {
    let reg = known_rules();
    for &target in ALL_DIALECTS {
        if target == Dialect::Kimi {
            continue;
        }
        let results = validate_mapping(&reg, Dialect::Kimi, target, &["image_input".into()]);
        assert!(
            results[0].fidelity.is_unsupported(),
            "Kimi -> {target:?} image_input should be unsupported",
        );
    }
}

#[test]
fn unmappable_code_exec_kimi_to_any_fails() {
    let reg = known_rules();
    for &target in ALL_DIALECTS {
        if target == Dialect::Kimi {
            continue;
        }
        let results = validate_mapping(&reg, Dialect::Kimi, target, &["code_exec".into()]);
        assert!(
            results[0].fidelity.is_unsupported(),
            "Kimi -> {target:?} code_exec should be unsupported",
        );
    }
}

#[test]
fn empty_feature_name_is_invalid_input() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        results[0].errors[0],
        MappingError::InvalidInput { .. },
    ));
}

#[test]
fn unknown_feature_yields_unsupported() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent_feature".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        results[0].errors[0],
        MappingError::FeatureUnsupported { .. },
    ));
}

#[test]
fn projection_empty_matrix_fails() {
    let matrix = ProjectionMatrix::new();
    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let result = matrix.project(&wo);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProjectionError::EmptyMatrix));
}

#[test]
fn unsupported_capability_negotiation_fails() {
    let manifest = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
    let reqs = require_caps(&[(Capability::CodeExecution, MinSupport::Native)]);
    let result = negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported_caps(), vec![Capability::CodeExecution]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Capability intersection for cross-dialect execution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_native_streaming_passes() {
    let manifest = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
    let reqs = require_caps(&[(Capability::Streaming, MinSupport::Native)]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 1);
}

#[test]
fn capability_emulated_satisfies_emulated_requirement() {
    let manifest = manifest_with(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let reqs = require_caps(&[(Capability::ToolRead, MinSupport::Emulated)]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
}

#[test]
fn capability_absent_is_unsupported() {
    let manifest: CapabilityManifest = BTreeMap::new();
    let level = check_capability(&manifest, &Capability::ToolWrite);
    assert!(matches!(
        level,
        abp_capability::SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn capability_restricted_maps_to_emulated() {
    let manifest = manifest_with(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    let level = check_capability(&manifest, &Capability::ToolBash);
    assert!(matches!(
        level,
        abp_capability::SupportLevel::Restricted { .. }
    ));
}

#[test]
fn capability_report_fully_compatible() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn capability_report_incompatible() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![],
        vec![Capability::CodeExecution],
    );
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn capability_report_mixed() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ExtendedThinking],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 1);
}

#[test]
fn capability_total_counts_all_buckets() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::CodeExecution],
    );
    assert_eq!(result.total(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Token counting differences across dialects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_usage_from_io_computes_total() {
    let usage = IrUsage::from_io(100, 50);
    assert_eq!(usage.total_tokens, 150);
}

#[test]
fn ir_usage_with_cache_computes_total() {
    let usage = IrUsage::with_cache(100, 50, 20, 10);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.cache_read_tokens, 20);
    assert_eq!(usage.cache_write_tokens, 10);
}

#[test]
fn ir_usage_merge_sums_all_fields() {
    let a = IrUsage::with_cache(100, 50, 20, 10);
    let b = IrUsage::with_cache(200, 100, 30, 15);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 150);
    assert_eq!(merged.total_tokens, 450);
    assert_eq!(merged.cache_read_tokens, 50);
    assert_eq!(merged.cache_write_tokens, 25);
}

#[test]
fn ir_usage_default_is_zero() {
    let usage = IrUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
    assert_eq!(usage.cache_read_tokens, 0);
    assert_eq!(usage.cache_write_tokens, 0);
}

#[test]
fn ir_usage_serde_roundtrip() {
    let usage = IrUsage::with_cache(1000, 500, 200, 100);
    let json = serde_json::to_value(&usage).unwrap();
    let back: IrUsage = serde_json::from_value(json).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn ir_usage_deterministic_json() {
    let usage = IrUsage::with_cache(100, 50, 20, 10);
    let json1 = serde_json::to_string(&usage).unwrap();
    let json2 = serde_json::to_string(&usage).unwrap();
    assert_eq!(json1, json2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. System prompt handling differences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_system_message_at_start() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"));
    assert!(conv.system_message().is_some());
    assert_eq!(
        conv.system_message().unwrap().text_content(),
        "You are helpful."
    );
}

#[test]
fn ir_no_system_message() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));
    assert!(conv.system_message().is_none());
}

#[test]
fn emulation_injects_system_prompt_when_missing() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));
    assert!(conv.system_message().is_none());

    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(!report.applied.is_empty());
    // System message should now be present
    assert!(conv.system_message().is_some());
}

#[test]
fn emulation_appends_to_existing_system_prompt() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"));

    let original_count = conv.messages.len();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    // Should not add new message, but append to existing system
    assert_eq!(conv.messages.len(), original_count);
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("You are helpful."));
    assert!(sys_text.contains("Think step by step"));
}

#[test]
fn ir_messages_by_role_filters_correctly() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "u1"),
        IrMessage::text(IrRole::Assistant, "a1"),
        IrMessage::text(IrRole::User, "u2"),
        IrMessage::text(IrRole::Assistant, "a2"),
    ]);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Multi-turn conversation format differences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_multi_turn_conversation_structure() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "What is 2+2?"),
        IrMessage::text(IrRole::Assistant, "4"),
        IrMessage::text(IrRole::User, "And 3+3?"),
        IrMessage::text(IrRole::Assistant, "6"),
    ]);
    assert_eq!(conv.len(), 5);
    assert!(!conv.is_empty());
    assert_eq!(conv.last_assistant().unwrap().text_content(), "6");
}

#[test]
fn ir_conversation_with_tool_round() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read /tmp/test.rs"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "/tmp/test.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The file contains a main function."),
    ]);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.tool_calls().len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 1);
}

#[test]
fn ir_empty_conversation() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
    assert!(conv.last_message().is_none());
    assert!(conv.tool_calls().is_empty());
}

#[test]
fn ir_conversation_serde_roundtrip() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi"),
    ]);
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn ir_message_metadata_uses_btreemap() {
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata.insert("z_key".into(), json!("last"));
    msg.metadata.insert("a_key".into(), json!("first"));

    let json = serde_json::to_string(&msg).unwrap();
    // BTreeMap serializes in sorted key order
    let a_pos = json.find("a_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(
        a_pos < z_pos,
        "BTreeMap should serialize a_key before z_key"
    );
}

#[test]
fn ir_message_text_only_detection() {
    let text_msg = IrMessage::text(IrRole::User, "hello");
    assert!(text_msg.is_text_only());

    let mixed_msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "let me call a tool".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "bash".into(),
                input: json!({}),
            },
        ],
    );
    assert!(!mixed_msg.is_text_only());
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Tool choice parameter mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_tool_definition_serde_roundtrip() {
    let tool = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file from disk.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"}
            },
            "required": ["path"]
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn ir_tool_definition_parameters_are_json_schema() {
    let tool = IrToolDefinition {
        name: "bash".into(),
        description: "Run a shell command.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout": {"type": "integer", "minimum": 0}
            },
            "required": ["command"]
        }),
    };
    let params = &tool.parameters;
    assert_eq!(params["type"], "object");
    assert!(params["properties"]["command"].is_object());
}

#[test]
fn ir_multiple_tool_definitions_preserved() {
    let tools = vec![
        IrToolDefinition {
            name: "read_file".into(),
            description: "Read file".into(),
            parameters: json!({"type": "object"}),
        },
        IrToolDefinition {
            name: "write_file".into(),
            description: "Write file".into(),
            parameters: json!({"type": "object"}),
        },
        IrToolDefinition {
            name: "bash".into(),
            description: "Run command".into(),
            parameters: json!({"type": "object"}),
        },
    ];
    let json = serde_json::to_string(&tools).unwrap();
    let back: Vec<IrToolDefinition> = serde_json::from_str(&json).unwrap();
    assert_eq!(tools.len(), back.len());
    for (orig, restored) in tools.iter().zip(back.iter()) {
        assert_eq!(orig.name, restored.name);
    }
}

#[test]
fn ir_tool_use_block_serde_uses_tag() {
    let block = IrContentBlock::ToolUse {
        id: "call_1".into(),
        name: "grep".into(),
        input: json!({"pattern": "TODO"}),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_use");
    assert_eq!(json["name"], "grep");
}

#[test]
fn ir_tool_result_block_serde_uses_tag() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_1".into(),
        content: vec![IrContentBlock::Text {
            text: "found 3 matches".into(),
        }],
        is_error: false,
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["is_error"], false);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Response format mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_content_block_text_roundtrip() {
    let block = IrContentBlock::Text {
        text: "Hello, world!".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_content_block_image_roundtrip() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_content_block_thinking_roundtrip() {
    let block = IrContentBlock::Thinking {
        text: "Let me think about this step by step...".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_content_block_discriminator_is_type() {
    let blocks = vec![
        IrContentBlock::Text {
            text: "hello".into(),
        },
        IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data".into(),
        },
        IrContentBlock::ToolUse {
            id: "c1".into(),
            name: "t".into(),
            input: json!({}),
        },
        IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        },
        IrContentBlock::Thinking { text: "hmm".into() },
    ];
    let expected_types = ["text", "image", "tool_use", "tool_result", "thinking"];
    for (block, expected) in blocks.iter().zip(expected_types.iter()) {
        let json = serde_json::to_value(block).unwrap();
        assert_eq!(
            json["type"].as_str().unwrap(),
            *expected,
            "block discriminator mismatch for {expected}",
        );
    }
}

#[test]
fn agent_event_kind_discriminator_is_type() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let json = serde_json::to_value(&ev.kind).unwrap();
    assert_eq!(json["type"], "run_started");
}

#[test]
fn agent_event_all_kinds_roundtrip() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta { text: "tok".into() },
        AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu1".into()),
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu1".into()),
            output: json!("file.txt"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
            error_code: Some(abp_error::ErrorCode::Internal),
        },
    ];
    for kind in kinds {
        let ev = make_event(kind);
        let json = serde_json::to_value(&ev).unwrap();
        let back: AgentEvent = serde_json::from_value(json).unwrap();
        // Just verify it round-trips without panic
        let _ = event_kind_name(&back.kind);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Model name cross-reference validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_detection_openai_request() {
    let detector = DialectDetector::new();
    let msg = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::OpenAi);
}

#[test]
fn dialect_detection_claude_response() {
    let detector = DialectDetector::new();
    let msg = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Claude);
}

#[test]
fn dialect_detection_gemini_request() {
    let detector = DialectDetector::new();
    let msg = json!({
        "contents": [{"parts": [{"text": "hello"}]}],
        "generationConfig": {"temperature": 0.7}
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Gemini);
}

#[test]
fn dialect_detection_codex_response() {
    let detector = DialectDetector::new();
    let msg = json!({
        "object": "response",
        "status": "completed",
        "items": [{"type": "message", "content": "done"}]
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Codex);
}

#[test]
fn dialect_detection_kimi_request() {
    let detector = DialectDetector::new();
    let msg = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "refs": [{"type": "doc", "id": "123"}],
        "search_plus": true
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Kimi);
}

#[test]
fn dialect_detection_copilot_request() {
    let detector = DialectDetector::new();
    let msg = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "references": [{"type": "file", "path": "src/lib.rs"}],
        "agent_mode": true
    });
    let result = detector.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Copilot);
}

#[test]
fn dialect_detection_returns_none_for_non_object() {
    let detector = DialectDetector::new();
    assert!(detector.detect(&json!("just a string")).is_none());
    assert!(detector.detect(&json!(42)).is_none());
    assert!(detector.detect(&json!(null)).is_none());
    assert!(detector.detect(&json!([1, 2, 3])).is_none());
}

#[test]
fn dialect_detection_returns_none_for_empty_object() {
    let detector = DialectDetector::new();
    assert!(detector.detect(&json!({})).is_none());
}

#[test]
fn dialect_detect_all_returns_sorted_by_confidence() {
    let detector = DialectDetector::new();
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7
    });
    let results = detector.detect_all(&msg);
    assert!(!results.is_empty());
    for window in results.windows(2) {
        assert!(window[0].confidence >= window[1].confidence);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Dialect validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_openai_valid_request() {
    let validator = DialectValidator::new();
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = validator.validate(&msg, Dialect::OpenAi);
    assert!(result.valid);
}

#[test]
fn validation_openai_missing_model() {
    let validator = DialectValidator::new();
    let msg = json!({
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = validator.validate(&msg, Dialect::OpenAi);
    assert!(!result.valid);
}

#[test]
fn validation_claude_valid_request() {
    let validator = DialectValidator::new();
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let result = validator.validate(&msg, Dialect::Claude);
    assert!(result.valid);
}

#[test]
fn validation_gemini_valid_request() {
    let validator = DialectValidator::new();
    let msg = json!({
        "contents": [{"parts": [{"text": "hello"}]}]
    });
    let result = validator.validate(&msg, Dialect::Gemini);
    assert!(result.valid);
}

#[test]
fn validation_non_object_fails_all_dialects() {
    let validator = DialectValidator::new();
    for &d in ALL_DIALECTS {
        let result = validator.validate(&json!("not an object"), d);
        assert!(!result.valid, "non-object should fail validation for {d:?}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Emulation labeling (no silent degradation)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn emulation_extended_thinking_has_strategy() {
    assert!(can_emulate(&Capability::ExtendedThinking));
}

#[test]
fn emulation_code_execution_cannot_emulate() {
    assert!(!can_emulate(&Capability::CodeExecution));
}

#[test]
fn emulation_image_input_has_strategy() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn emulation_stop_sequences_has_strategy() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn emulation_structured_output_has_strategy() {
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn emulation_disabled_capabilities_produce_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.has_unemulatable());
    assert!(!report.warnings.is_empty());
}

#[test]
fn emulation_config_override() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code execution.".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let strategy = engine.resolve_strategy(&Capability::CodeExecution);
    assert!(matches!(
        strategy,
        EmulationStrategy::SystemPromptInjection { .. },
    ));
}

#[test]
fn fidelity_labels_native_for_supported_caps() {
    let native_caps = vec![Capability::Streaming, Capability::ToolUse];
    let report = abp_emulation::EmulationReport::default();
    let labels = compute_fidelity(&native_caps, &report);
    assert_eq!(labels.len(), 2);
    for label in labels.values() {
        assert!(matches!(label, FidelityLabel::Native));
    }
}

#[test]
fn fidelity_labels_emulated_for_applied_caps() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels.get(&Capability::ExtendedThinking).unwrap(),
        FidelityLabel::Emulated { .. },
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Mapping registry completeness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn known_rules_is_non_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
}

#[test]
fn known_rules_covers_all_self_mappings() {
    let reg = known_rules();
    let expected = ALL_DIALECTS.len() * ALL_FEATURES.len();
    let mut count = 0;
    for &d in ALL_DIALECTS {
        for &f in ALL_FEATURES {
            if reg.lookup(d, d, f).is_some() {
                count += 1;
            }
        }
    }
    assert_eq!(count, expected);
}

#[test]
fn known_rules_tool_use_covers_all_dialect_pairs() {
    let reg = known_rules();
    for &a in ALL_DIALECTS {
        for &b in ALL_DIALECTS {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::TOOL_USE);
            assert!(rule.is_some(), "missing tool_use rule for {a:?} -> {b:?}",);
        }
    }
}

#[test]
fn known_rules_streaming_covers_all_dialect_pairs() {
    let reg = known_rules();
    for &a in ALL_DIALECTS {
        for &b in ALL_DIALECTS {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::STREAMING);
            assert!(rule.is_some(), "missing streaming rule for {a:?} -> {b:?}",);
        }
    }
}

#[test]
fn mapping_matrix_from_known_rules_symmetric_for_self() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    for &d in ALL_DIALECTS {
        assert!(matrix.is_supported(d, d));
    }
}

#[test]
fn mapping_error_display_includes_context() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let display = err.to_string();
    assert!(display.contains("logprobs"));
}

#[test]
fn mapping_error_serde_roundtrip() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system".into(),
    };
    let json = serde_json::to_value(&err).unwrap();
    let back: MappingError = serde_json::from_value(json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn fidelity_serde_roundtrip_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_value(&f).unwrap();
    let back: Fidelity = serde_json::from_value(json).unwrap();
    assert_eq!(f, back);
}

#[test]
fn fidelity_serde_roundtrip_lossy() {
    let f = Fidelity::LossyLabeled {
        warning: "test warning".into(),
    };
    let json = serde_json::to_value(&f).unwrap();
    let back: Fidelity = serde_json::from_value(json).unwrap();
    assert_eq!(f, back);
}

#[test]
fn fidelity_serde_roundtrip_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_value(&f).unwrap();
    let back: Fidelity = serde_json::from_value(json).unwrap();
    assert_eq!(f, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Projection integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn projection_selects_best_fit_backend() {
    let mut pm = ProjectionMatrix::new();
    let manifest = manifest_with(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
    ]);
    pm.register_backend("claude-backend", manifest, Dialect::Claude, 80);

    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let result = pm.project(&wo);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().selected_backend, "claude-backend");
}

#[test]
fn projection_with_mapping_registry() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);

    let manifest = manifest_with(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
    ]);
    pm.register_backend("gemini-backend", manifest, Dialect::Gemini, 50);

    let wo = abp_core::WorkOrderBuilder::new("test").build();
    let result = pm.project(&wo);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// BTreeMap determinism
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn btreemap_ext_field_deterministic_serialization() {
    let mut ext1: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    ext1.insert("z_field".into(), json!(1));
    ext1.insert("a_field".into(), json!(2));
    ext1.insert("m_field".into(), json!(3));

    let mut ext2: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    ext2.insert("m_field".into(), json!(3));
    ext2.insert("a_field".into(), json!(2));
    ext2.insert("z_field".into(), json!(1));

    let json1 = serde_json::to_string(&ext1).unwrap();
    let json2 = serde_json::to_string(&ext2).unwrap();
    assert_eq!(
        json1, json2,
        "BTreeMap must produce deterministic JSON regardless of insertion order"
    );
}

#[test]
fn ir_message_metadata_deterministic() {
    let mut msg1 = IrMessage::text(IrRole::User, "test");
    msg1.metadata.insert("zebra".into(), json!(1));
    msg1.metadata.insert("alpha".into(), json!(2));

    let mut msg2 = IrMessage::text(IrRole::User, "test");
    msg2.metadata.insert("alpha".into(), json!(2));
    msg2.metadata.insert("zebra".into(), json!(1));

    let json1 = serde_json::to_string(&msg1).unwrap();
    let json2 = serde_json::to_string(&msg2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn capability_manifest_deterministic() {
    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolUse, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Native);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::Streaming, SupportLevel::Native);
    m2.insert(Capability::ToolUse, SupportLevel::Native);

    let json1 = serde_json::to_string(&m1).unwrap();
    let json2 = serde_json::to_string(&m2).unwrap();
    assert_eq!(json1, json2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: thinking feature mapping across all dialect pairs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_self_mapping_lossless() {
    let reg = known_rules();
    for &d in ALL_DIALECTS {
        let rule = reg.lookup(d, d, features::THINKING).unwrap();
        assert!(rule.fidelity.is_lossless());
    }
}

#[test]
fn thinking_cross_dialect_never_unsupported() {
    let reg = known_rules();
    for &a in ALL_DIALECTS {
        for &b in ALL_DIALECTS {
            if a == b {
                continue;
            }
            if let Some(rule) = reg.lookup(a, b, features::THINKING) {
                // Thinking cross-dialect is lossy but never unsupported
                assert!(
                    !rule.fidelity.is_unsupported(),
                    "thinking {a:?} -> {b:?} should not be unsupported",
                );
            }
        }
    }
}

#[test]
fn thinking_ir_block_roundtrip() {
    let block = IrContentBlock::Thinking {
        text: "Let me reason through this carefully.\n1. First...\n2. Then...".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "thinking");
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(block, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: image_input feature mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn image_input_openai_claude_gemini_triangle_lossless() {
    let reg = known_rules();
    let image_capable = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
    for &a in &image_capable {
        for &b in &image_capable {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "image_input {a:?} -> {b:?} should be lossless",
            );
        }
    }
}

#[test]
fn image_input_to_codex_always_unsupported() {
    let reg = known_rules();
    for &source in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let rule = reg
            .lookup(source, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: code_exec feature mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn code_exec_self_mapping_lossless() {
    let reg = known_rules();
    for &d in ALL_DIALECTS {
        let rule = reg.lookup(d, d, features::CODE_EXEC).unwrap();
        assert!(rule.fidelity.is_lossless());
    }
}

#[test]
fn code_exec_between_capable_dialects_is_lossy() {
    let reg = known_rules();
    let capable = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ];
    for &a in &capable {
        for &b in &capable {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::CODE_EXEC).unwrap();
            assert!(
                matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
                "code_exec {a:?} -> {b:?} should be LossyLabeled",
            );
        }
    }
}

#[test]
fn code_exec_kimi_to_any_unsupported() {
    let reg = known_rules();
    for &target in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ] {
        let rule = reg
            .lookup(Dialect::Kimi, target, features::CODE_EXEC)
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }
}
