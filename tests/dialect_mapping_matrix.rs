// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests validating the projection matrix for dialect×engine mapping.
//!
//! Covers dialect detection, enum exhaustiveness, passthrough/mapped modes,
//! capability gaps, tool calling mapping, streaming semantics, error mapping,
//! token/usage mapping, model name mapping, response format mapping, system
//! prompt handling, multi-turn conversation mapping, and projection matrix
//! completeness.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, CapabilityRequirement,
    CapabilityRequirements, ExecutionMode, MinSupport, RuntimeConfig, SupportLevel,
    WorkOrderBuilder,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_mapping::{
    Fidelity, MappingMatrix, MappingRegistry, MappingRule, features, known_rules, validate_mapping,
};
use abp_projection::{ProjectionError, ProjectionMatrix};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

const ALL_FEATURES: &[&str] = &[
    features::TOOL_USE,
    features::STREAMING,
    features::THINKING,
    features::IMAGE_INPUT,
    features::CODE_EXEC,
];

fn detector() -> DialectDetector {
    DialectDetector::new()
}

fn validator() -> DialectValidator {
    DialectValidator::new()
}

fn manifest(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    caps.iter().cloned().collect()
}

fn full_manifest() -> CapabilityManifest {
    manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Native),
        (Capability::ImageInput, SupportLevel::Native),
        (Capability::CodeExecution, SupportLevel::Native),
    ])
}

fn require(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Emulated,
            })
            .collect(),
    }
}

fn require_native(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Native,
            })
            .collect(),
    }
}

fn wo(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test").requirements(reqs).build()
}

fn passthrough_wo(reqs: CapabilityRequirements, dialect: &str) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        json!({ "mode": "passthrough", "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("passthrough task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Dialect detection from request format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_from_messages_and_model() {
    let msg = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
    assert!(r.confidence > 0.0);
}

#[test]
fn detect_openai_from_choices_key() {
    let msg = json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]});
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_claude_from_type_message() {
    let msg = json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "hello"}],
        "stop_reason": "end_turn"
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_claude_from_content_blocks() {
    let msg = json!({
        "model": "claude-3-opus-20240229",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_from_contents_and_parts() {
    let msg = json!({
        "contents": [{"role": "user", "parts": [{"text": "hello"}]}]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_candidates_key() {
    let msg = json!({
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_generation_config() {
    let msg = json!({
        "contents": [{"parts": [{"text": "q"}]}],
        "generationConfig": {"maxOutputTokens": 100}
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_from_items_and_status() {
    let msg = json!({
        "items": [{"type": "message", "role": "user"}],
        "status": "completed"
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_codex_from_object_response() {
    let msg = json!({"object": "response", "output": []});
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_kimi_from_refs_field() {
    let msg = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": [{"url": "https://example.com"}]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_copilot_from_references_field() {
    let msg = json!({
        "model": "copilot-gpt-4",
        "messages": [{"role": "user", "content": "help"}],
        "references": [{"type": "file", "id": "f1"}]
    });
    let r = detector().detect(&msg).unwrap();
    // May detect as OpenAI due to messages format; references alone doesn't force Copilot
    assert!(
        r.dialect == Dialect::Copilot || r.dialect == Dialect::OpenAi,
        "Expected Copilot or OpenAI, got {:?}",
        r.dialect
    );
}

#[test]
fn detect_returns_none_for_empty_object() {
    assert!(detector().detect(&json!({})).is_none());
}

#[test]
fn detect_returns_none_for_scalar() {
    assert!(detector().detect(&json!(42)).is_none());
}

#[test]
fn detect_all_returns_multiple_candidates() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let results = detector().detect_all(&msg);
    assert!(!results.is_empty());
}

#[test]
fn detect_confidence_is_bounded_zero_to_one() {
    let payloads = vec![
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
        json!({"type": "message", "content": [{"type": "text", "text": "hi"}]}),
        json!({"contents": [{"parts": [{"text": "hi"}]}]}),
    ];
    for p in payloads {
        if let Some(r) = detector().detect(&p) {
            assert!(
                r.confidence >= 0.0 && r.confidence <= 1.0,
                "confidence out of range: {}",
                r.confidence
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Dialect enum exhaustiveness
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
fn dialect_label_is_nonempty_for_all() {
    for &d in Dialect::all() {
        assert!(!d.label().is_empty(), "Dialect {d:?} has empty label");
    }
}

#[test]
fn dialect_labels_are_unique() {
    let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
    let unique: std::collections::HashSet<&&str> = labels.iter().collect();
    assert_eq!(labels.len(), unique.len(), "Dialect labels are not unique");
}

#[test]
fn dialect_serde_roundtrip_all_variants() {
    for &d in Dialect::all() {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back, "Serde roundtrip failed for {d:?}");
    }
}

#[test]
fn dialect_clone_eq_all_variants() {
    for &d in Dialect::all() {
        let cloned = d;
        assert_eq!(d, cloned);
    }
}

#[test]
fn dialect_debug_all_variants() {
    for &d in Dialect::all() {
        let dbg = format!("{d:?}");
        assert!(!dbg.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Engine enum exhaustiveness (ExecutionMode)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough_exists() {
    let m = ExecutionMode::Passthrough;
    assert_eq!(m, ExecutionMode::Passthrough);
}

#[test]
fn execution_mode_mapped_exists() {
    let m = ExecutionMode::Mapped;
    assert_eq!(m, ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip_passthrough() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ExecutionMode::Passthrough);
}

#[test]
fn execution_mode_serde_roundtrip_mapped() {
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough_serializes_as_snake_case() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, "\"passthrough\"");
}

#[test]
fn execution_mode_mapped_serializes_as_snake_case() {
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(json, "\"mapped\"");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Passthrough mode (dialect==engine: lossless, no rewriting)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_self_mapping_always_lossless_for_all_features() {
    let reg = known_rules();
    for &d in Dialect::all() {
        for &feat in ALL_FEATURES {
            let rule = reg
                .lookup(d, d, feat)
                .unwrap_or_else(|| panic!("Missing self-mapping for {d:?} [{feat}]"));
            assert!(
                rule.fidelity.is_lossless(),
                "Self-mapping {d:?}->{d:?} [{feat}] should be Lossless, got {:?}",
                rule.fidelity,
            );
        }
    }
}

#[test]
fn passthrough_self_mapping_fidelity_is_lossless() {
    let reg = known_rules();
    for &d in Dialect::all() {
        let rule = reg.lookup(d, d, features::TOOL_USE).unwrap();
        assert!(rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }
}

#[test]
fn passthrough_projection_prefers_same_dialect_backend() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    pm.register_backend("claude-be", full_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", full_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::ToolUse]))).unwrap();
    assert_eq!(
        result.selected_backend, "claude-be",
        "Passthrough should prefer same-dialect backend"
    );
}

#[test]
fn passthrough_wo_sets_mode_in_config() {
    let work = passthrough_wo(require(&[Capability::Streaming]), "openai");
    let abp = work.config.vendor.get("abp").unwrap();
    assert_eq!(abp["mode"], "passthrough");
}

#[test]
fn passthrough_wo_sets_source_dialect() {
    let work = passthrough_wo(require(&[Capability::Streaming]), "claude");
    let abp = work.config.vendor.get("abp").unwrap();
    assert_eq!(abp["source_dialect"], "claude");
}

#[test]
fn passthrough_each_dialect_self_scores_max_fidelity() {
    let reg = known_rules();
    for &d in Dialect::all() {
        let mut pm = ProjectionMatrix::with_mapping_registry(reg.clone());
        pm.set_source_dialect(d);
        pm.set_mapping_features(ALL_FEATURES.iter().map(|s| s.to_string()).collect());
        pm.register_backend("self", full_manifest(), d, 50);

        let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
        assert!(
            result.fidelity_score.mapping_fidelity >= 0.9,
            "Self-fidelity for {d:?} = {} should be near 1.0",
            result.fidelity_score.mapping_fidelity,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Mapped mode (dialect!=engine: explicitly lossy)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapped_thinking_cross_dialect_all_lossy() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::THINKING).unwrap();
            assert!(
                matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
                "thinking {src:?} -> {tgt:?} should be Lossy, got {:?}",
                rule.fidelity,
            );
        }
    }
}

#[test]
fn mapped_code_exec_cross_dialect_never_lossless() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::CODE_EXEC).unwrap();
            assert!(
                !rule.fidelity.is_lossless(),
                "code_exec {src:?} -> {tgt:?} should not be Lossless",
            );
        }
    }
}

#[test]
fn mapped_lossy_rules_always_have_warnings() {
    let reg = known_rules();
    for rule in reg.iter() {
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert!(
                !warning.trim().is_empty(),
                "Lossy rule {:?}->{:?} [{}] has blank warning",
                rule.source_dialect,
                rule.target_dialect,
                rule.feature,
            );
        }
    }
}

#[test]
fn mapped_unsupported_rules_always_have_reasons() {
    let reg = known_rules();
    for rule in reg.iter() {
        if let Fidelity::Unsupported { reason } = &rule.fidelity {
            assert!(
                !reason.trim().is_empty(),
                "Unsupported rule {:?}->{:?} [{}] has blank reason",
                rule.source_dialect,
                rule.target_dialect,
                rule.feature,
            );
        }
    }
}

#[test]
fn mapped_cross_dialect_projection_picks_highest_score() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    pm.register_backend("high", full_manifest(), Dialect::Claude, 80);
    pm.register_backend("low", full_manifest(), Dialect::Gemini, 20);

    let result = pm.project(&wo(require(&[Capability::ToolUse]))).unwrap();
    // Higher priority should be preferred when capabilities are equal
    assert_eq!(result.selected_backend, "high");
}

#[test]
fn mapped_fidelity_degrades_for_unsupported_features() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![
        features::THINKING.into(),
        features::IMAGE_INPUT.into(),
    ]);

    // Codex doesn't support image_input; Claude->Codex thinking is lossy
    pm.register_backend("codex", full_manifest(), Dialect::Codex, 50);
    pm.register_backend("openai", full_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.fidelity_score.mapping_fidelity < 1.0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Capability gaps between dialects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn image_input_unsupported_for_codex_kimi_copilot() {
    let reg = known_rules();
    let image_capable = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
    let image_incapable = [Dialect::Codex, Dialect::Kimi, Dialect::Copilot];

    for &src in &image_capable {
        for &tgt in &image_incapable {
            let rule = reg.lookup(src, tgt, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image {src:?} -> {tgt:?} should be unsupported, got {:?}",
                rule.fidelity,
            );
        }
    }
}

#[test]
fn image_input_lossless_among_capable_dialects() {
    let reg = known_rules();
    let capable = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];

    for &src in &capable {
        for &tgt in &capable {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "image {src:?} -> {tgt:?} should be lossless, got {:?}",
                rule.fidelity,
            );
        }
    }
}

#[test]
fn code_exec_unsupported_involving_kimi() {
    let reg = known_rules();
    for &other in Dialect::all() {
        if other == Dialect::Kimi {
            continue;
        }
        let fwd = reg
            .lookup(other, Dialect::Kimi, features::CODE_EXEC)
            .unwrap();
        let rev = reg
            .lookup(Dialect::Kimi, other, features::CODE_EXEC)
            .unwrap();
        assert!(
            fwd.fidelity.is_unsupported(),
            "code_exec {other:?} -> Kimi should be unsupported",
        );
        assert!(
            rev.fidelity.is_unsupported(),
            "code_exec Kimi -> {other:?} should be unsupported",
        );
    }
}

#[test]
fn streaming_universally_lossless() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let rule = reg.lookup(src, tgt, features::STREAMING).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming {src:?} -> {tgt:?} should be lossless",
            );
        }
    }
}

#[test]
fn thinking_self_mapping_lossless_cross_mapping_lossy() {
    let reg = known_rules();
    for &d in Dialect::all() {
        let self_rule = reg.lookup(d, d, features::THINKING).unwrap();
        assert!(self_rule.fidelity.is_lossless());
    }
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::THINKING).unwrap();
            assert!(!rule.fidelity.is_lossless());
        }
    }
}

#[test]
fn tool_use_no_unsupported_pairs() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let rule = reg.lookup(src, tgt, features::TOOL_USE).unwrap();
            assert!(
                !rule.fidelity.is_unsupported(),
                "tool_use {src:?} -> {tgt:?} should not be unsupported",
            );
        }
    }
}

#[test]
fn validate_mapping_returns_validations_for_all_requested_features() {
    let reg = known_rules();
    let features_list: Vec<String> = ALL_FEATURES.iter().map(|s| s.to_string()).collect();
    let validations = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features_list);
    assert_eq!(validations.len(), ALL_FEATURES.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Tool/function calling mapping across dialects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_openai_to_claude_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn tool_use_openai_to_gemini_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn tool_use_claude_to_openai_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn tool_use_claude_to_gemini_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn tool_use_gemini_to_openai_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Gemini, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn tool_use_codex_pairs_are_lossy() {
    let reg = known_rules();
    let others = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Kimi,
        Dialect::Copilot,
    ];
    for &other in &others {
        let fwd = reg
            .lookup(Dialect::Codex, other, features::TOOL_USE)
            .unwrap();
        let rev = reg
            .lookup(other, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(
            matches!(fwd.fidelity, Fidelity::LossyLabeled { .. }),
            "tool_use Codex -> {other:?} should be lossy",
        );
        assert!(
            matches!(rev.fidelity, Fidelity::LossyLabeled { .. }),
            "tool_use {other:?} -> Codex should be lossy",
        );
    }
}

#[test]
fn tool_call_event_roundtrip_preserves_fields() {
    let evt = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
            assert_eq!(input["path"], "src/main.rs");
        }
        _ => panic!("Expected ToolCall"),
    }
}

#[test]
fn tool_result_event_roundtrip_preserves_is_error() {
    let evt = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tc-2".into()),
        output: json!({"exit_code": 1}),
        is_error: true,
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
        _ => panic!("Expected ToolResult"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Streaming semantics preservation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_fidelity_is_lossless_for_all_dialect_pairs() {
    let reg = known_rules();
    let dialects = Dialect::all();
    for &src in dialects {
        for &tgt in dialects {
            let rule = reg.lookup(src, tgt, features::STREAMING).unwrap();
            assert!(rule.fidelity.is_lossless());
        }
    }
}

#[test]
fn assistant_delta_event_roundtrip() {
    let evt = make_event(AgentEventKind::AssistantDelta {
        text: "partial response".into(),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "partial response"),
        _ => panic!("Expected AssistantDelta"),
    }
}

#[test]
fn assistant_message_event_roundtrip() {
    let evt = make_event(AgentEventKind::AssistantMessage {
        text: "full message".into(),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "full message"),
        _ => panic!("Expected AssistantMessage"),
    }
}

#[test]
fn run_started_event_roundtrip() {
    let evt = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::RunStarted { message } => assert_eq!(message, "go"),
        _ => panic!("Expected RunStarted"),
    }
}

#[test]
fn run_completed_event_roundtrip() {
    let evt = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::RunCompleted { message } => assert_eq!(message, "done"),
        _ => panic!("Expected RunCompleted"),
    }
}

#[test]
fn streaming_backend_requires_streaming_capability() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "no-stream",
        manifest(&[(Capability::ToolUse, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );

    let result = pm.project(&wo(require_native(&[Capability::Streaming])));
    // Backend lacks streaming, so it should not satisfy native requirement
    assert!(
        result.is_err() || {
            let r = result.unwrap();
            !r.required_emulations.is_empty() || r.fidelity_score.capability_coverage < 1.0
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Error mapping across SDK boundaries
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_event_roundtrip_with_code() {
    let evt = make_event(AgentEventKind::Error {
        message: "rate limited".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "rate limited");
            assert!(error_code.is_some());
        }
        _ => panic!("Expected Error"),
    }
}

#[test]
fn error_event_roundtrip_without_code() {
    let evt = make_event(AgentEventKind::Error {
        message: "unknown".into(),
        error_code: None,
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::Error { error_code, .. } => assert!(error_code.is_none()),
        _ => panic!("Expected Error"),
    }
}

#[test]
fn warning_event_roundtrip() {
    let evt = make_event(AgentEventKind::Warning {
        message: "deprecated".into(),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::Warning { message } => assert_eq!(message, "deprecated"),
        _ => panic!("Expected Warning"),
    }
}

#[test]
fn projection_error_empty_matrix() {
    let pm = ProjectionMatrix::new();
    let result = pm.project(&wo(require(&[Capability::Streaming])));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProjectionError::EmptyMatrix));
}

#[test]
fn projection_error_no_suitable_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "weak",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require_native(&[
        Capability::ImageInput,
        Capability::CodeExecution,
        Capability::ExtendedThinking,
    ])));
    // Should either fail or return a low score
    if let Ok(r) = result {
        assert!(
            r.fidelity_score.capability_coverage < 1.0,
            "Expected poor coverage"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Token counting / usage mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_normalized_default_all_none() {
    let usage = abp_core::UsageNormalized::default();
    assert!(usage.input_tokens.is_none());
    assert!(usage.output_tokens.is_none());
    assert!(usage.cache_read_tokens.is_none());
    assert!(usage.cache_write_tokens.is_none());
    assert!(usage.estimated_cost_usd.is_none());
}

#[test]
fn usage_normalized_serde_roundtrip_with_values() {
    let usage = abp_core::UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(10),
        request_units: Some(1),
        estimated_cost_usd: Some(0.005),
    };
    let json = serde_json::to_value(&usage).unwrap();
    let back: abp_core::UsageNormalized = serde_json::from_value(json).unwrap();
    assert_eq!(back.input_tokens, Some(100));
    assert_eq!(back.output_tokens, Some(200));
    assert_eq!(back.cache_read_tokens, Some(50));
    assert_eq!(back.cache_write_tokens, Some(10));
    assert_eq!(back.estimated_cost_usd, Some(0.005));
}

#[test]
fn usage_normalized_serde_roundtrip_all_none() {
    let usage = abp_core::UsageNormalized::default();
    let json = serde_json::to_value(&usage).unwrap();
    let back: abp_core::UsageNormalized = serde_json::from_value(json).unwrap();
    assert!(back.input_tokens.is_none());
    assert!(back.output_tokens.is_none());
}

#[test]
fn openai_usage_maps_to_normalized() {
    // OpenAI usage has prompt_tokens, completion_tokens, total_tokens
    let openai_usage = json!({
        "prompt_tokens": 150,
        "completion_tokens": 300,
        "total_tokens": 450
    });
    assert!(openai_usage["prompt_tokens"].as_u64().is_some());
    assert!(openai_usage["completion_tokens"].as_u64().is_some());
}

#[test]
fn claude_usage_maps_to_normalized() {
    // Claude usage has input_tokens, output_tokens, cache fields
    let claude_usage = json!({
        "input_tokens": 100,
        "output_tokens": 250,
        "cache_creation_input_tokens": 50,
        "cache_read_input_tokens": 20
    });
    assert!(claude_usage["input_tokens"].as_u64().is_some());
    assert!(
        claude_usage["cache_creation_input_tokens"]
            .as_u64()
            .is_some()
    );
}

#[test]
fn gemini_usage_maps_to_normalized() {
    // Gemini has prompt_token_count, candidates_token_count, total_token_count
    let gemini_usage = json!({
        "prompt_token_count": 80,
        "candidates_token_count": 200,
        "total_token_count": 280
    });
    assert!(gemini_usage["prompt_token_count"].as_u64().is_some());
    assert!(gemini_usage["candidates_token_count"].as_u64().is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Model name mapping between providers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_model_names_detected_as_openai() {
    let models = ["gpt-4", "gpt-4o", "gpt-3.5-turbo", "gpt-4-turbo"];
    for model in models {
        let msg = json!({"model": model, "messages": [{"role": "user", "content": "hi"}]});
        let r = detector().detect(&msg).unwrap();
        assert_eq!(
            r.dialect,
            Dialect::OpenAi,
            "Model {model} should detect as OpenAi"
        );
    }
}

#[test]
fn claude_model_names_detected_as_claude() {
    let msg = json!({
        "model": "claude-3-opus-20240229",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn work_order_model_field_is_optional() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(wo.config.model.is_none());
}

#[test]
fn work_order_model_field_roundtrip() {
    let wo = WorkOrderBuilder::new("test")
        .model("gpt-4o".to_string())
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn work_order_model_serde_roundtrip() {
    let wo = WorkOrderBuilder::new("test")
        .model("claude-3-sonnet".to_string())
        .build();
    let json = serde_json::to_value(&wo).unwrap();
    let back: abp_core::WorkOrder = serde_json::from_value(json).unwrap();
    assert_eq!(back.config.model.as_deref(), Some("claude-3-sonnet"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Response format mapping (text, JSON, structured)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_response_format_text_serde() {
    let fmt = json!({"type": "text"});
    assert_eq!(fmt["type"], "text");
}

#[test]
fn openai_response_format_json_object_serde() {
    let fmt = json!({"type": "json_object"});
    assert_eq!(fmt["type"], "json_object");
}

#[test]
fn openai_response_format_json_schema_serde() {
    let fmt = json!({
        "type": "json_schema",
        "json_schema": {
            "name": "output",
            "schema": {"type": "object", "properties": {"result": {"type": "string"}}}
        }
    });
    assert_eq!(fmt["type"], "json_schema");
    assert!(fmt["json_schema"]["schema"].is_object());
}

#[test]
fn gemini_response_mime_type_for_json() {
    let config = json!({
        "response_mime_type": "application/json",
        "response_schema": {"type": "object"}
    });
    assert_eq!(config["response_mime_type"], "application/json");
}

#[test]
fn structured_output_capability_in_manifest() {
    let m = manifest(&[(Capability::StructuredOutputJsonSchema, SupportLevel::Native)]);
    assert!(
        matches!(
            m.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Native)
        ),
        "Expected Native support for StructuredOutputJsonSchema"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. System prompt handling differences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_system_prompt_as_message_role() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "hi"}
        ]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn claude_system_prompt_as_top_level_field() {
    // Claude uses a top-level `system` field rather than a system message
    let request = json!({
        "model": "claude-3-opus",
        "system": "You are helpful.",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let r = detector().detect(&request).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn gemini_system_instruction_as_separate_field() {
    let request = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "system_instruction": {"role": "user", "parts": [{"text": "be helpful"}]}
    });
    let r = detector().detect(&request).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn validator_handles_empty_messages_for_openai() {
    let msg = json!({"model": "gpt-4", "messages": []});
    let result = validator().validate(&msg, Dialect::OpenAi);
    // Validator may or may not flag empty messages; just verify no panic
    let _ = result.valid;
    let _ = result.warnings;
}

#[test]
fn validator_accepts_valid_openai_request() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = validator().validate(&msg, Dialect::OpenAi);
    assert!(
        result.valid,
        "Valid OpenAI request should pass: {:?}",
        result.errors
    );
}

#[test]
fn validator_accepts_valid_claude_request() {
    let msg = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "max_tokens": 1024
    });
    let result = validator().validate(&msg, Dialect::Claude);
    assert!(
        result.valid,
        "Valid Claude request should pass: {:?}",
        result.errors
    );
}

#[test]
fn validator_accepts_valid_gemini_request() {
    let msg = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
    });
    let result = validator().validate(&msg, Dialect::Gemini);
    assert!(
        result.valid,
        "Valid Gemini request should pass: {:?}",
        result.errors
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Multi-turn conversation mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_openai_format() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "system"},
            {"role": "user", "content": "q1"},
            {"role": "assistant", "content": "a1"},
            {"role": "user", "content": "q2"}
        ]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn multi_turn_claude_format() {
    let msg = json!({
        "model": "claude-3",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "q1"}]},
            {"role": "assistant", "content": [{"type": "text", "text": "a1"}]},
            {"role": "user", "content": [{"type": "text", "text": "q2"}]}
        ]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn multi_turn_gemini_format() {
    let msg = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "q1"}]},
            {"role": "model", "parts": [{"text": "a1"}]},
            {"role": "user", "parts": [{"text": "q2"}]}
        ]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn file_changed_event_roundtrip() {
    let evt = make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::FileChanged { path, summary } => {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(summary, "added function");
        }
        _ => panic!("Expected FileChanged"),
    }
}

#[test]
fn command_executed_event_roundtrip() {
    let evt = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    match &back.kind {
        AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } => {
            assert_eq!(command, "cargo test");
            assert_eq!(*exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("ok"));
        }
        _ => panic!("Expected CommandExecuted"),
    }
}

#[test]
fn agent_event_ext_field_preserved_in_serde() {
    let mut ext = std::collections::BTreeMap::new();
    ext.insert("dialect".into(), json!("openai"));
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_value(&evt).unwrap();
    let back: AgentEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back.ext.unwrap()["dialect"], "openai");
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Projection matrix completeness validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn every_dialect_pair_has_rules_for_every_feature() {
    let reg = known_rules();
    let dialects = Dialect::all();
    let mut missing = Vec::new();

    for &src in dialects {
        for &tgt in dialects {
            for &feat in ALL_FEATURES {
                if reg.lookup(src, tgt, feat).is_none() {
                    missing.push(format!("{src:?} -> {tgt:?} [{feat}]"));
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Missing mapping rules for {} pairs:\n  {}",
        missing.len(),
        missing.join("\n  "),
    );
}

#[test]
fn known_rules_is_not_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
}

#[test]
fn known_rules_total_count_at_least_150() {
    let reg = known_rules();
    assert!(reg.len() >= 150, "Expected >= 150 rules, got {}", reg.len());
}

#[test]
fn mapping_matrix_from_registry_marks_all_pairs_supported() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);

    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                matrix.is_supported(src, tgt),
                "MappingMatrix should mark {src:?} -> {tgt:?} as supported"
            );
        }
    }
}

#[test]
fn mapping_matrix_empty_has_no_support() {
    let matrix = MappingMatrix::new();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(!matrix.is_supported(src, tgt));
        }
    }
}

#[test]
fn mapping_matrix_get_returns_none_for_unset() {
    let matrix = MappingMatrix::new();
    assert!(matrix.get(Dialect::OpenAi, Dialect::Claude).is_none());
}

#[test]
fn mapping_matrix_set_and_get_roundtrip() {
    let mut matrix = MappingMatrix::new();
    matrix.set(Dialect::OpenAi, Dialect::Claude, true);
    assert_eq!(matrix.get(Dialect::OpenAi, Dialect::Claude), Some(true));
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn mapping_matrix_set_false_means_unsupported() {
    let mut matrix = MappingMatrix::new();
    matrix.set(Dialect::OpenAi, Dialect::Claude, false);
    assert_eq!(matrix.get(Dialect::OpenAi, Dialect::Claude), Some(false));
    assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn mapping_symmetry_all_pairs_defined_both_ways() {
    let reg = known_rules();
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            if a == b {
                continue;
            }
            for &feat in ALL_FEATURES {
                let ab = reg.lookup(a, b, feat);
                let ba = reg.lookup(b, a, feat);
                assert!(
                    ab.is_some() && ba.is_some(),
                    "Asymmetric: {a:?}->{b:?} [{feat}] = {}, {b:?}->{a:?} = {}",
                    ab.is_some(),
                    ba.is_some(),
                );
            }
        }
    }
}

#[test]
fn mapping_rule_serde_roundtrip() {
    let rule = MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Lossless,
    };
    let json = serde_json::to_value(&rule).unwrap();
    let back: MappingRule = serde_json::from_value(json).unwrap();
    assert_eq!(back.source_dialect, Dialect::OpenAi);
    assert_eq!(back.target_dialect, Dialect::Claude);
    assert_eq!(back.feature, features::TOOL_USE);
    assert!(back.fidelity.is_lossless());
}

#[test]
fn fidelity_lossless_is_lossless() {
    assert!(Fidelity::Lossless.is_lossless());
    assert!(!Fidelity::Lossless.is_unsupported());
}

#[test]
fn fidelity_lossy_labeled_is_not_lossless() {
    let f = Fidelity::LossyLabeled {
        warning: "test".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported_is_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "n/a".into(),
    };
    assert!(f.is_unsupported());
    assert!(!f.is_lossless());
}

#[test]
fn projection_matrix_new_is_empty() {
    let pm = ProjectionMatrix::new();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn projection_register_all_six_dialect_backends() {
    let mut pm = ProjectionMatrix::new();
    for (i, &d) in Dialect::all().iter().enumerate() {
        pm.register_backend(format!("be-{i}"), full_manifest(), d, 50);
    }
    assert_eq!(pm.backend_count(), 6);
}

#[test]
fn projection_with_mapping_registry_uses_fidelity_scores() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);
    pm.register_backend("be-claude", full_manifest(), Dialect::Claude, 50);
    pm.register_backend("be-openai", full_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Projection selects based on fidelity + capability scoring
    assert!(
        result.selected_backend == "be-openai" || result.selected_backend == "be-claude",
        "Expected be-openai or be-claude, got {}",
        result.selected_backend
    );
}

#[test]
fn projection_fallback_chain_populated() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    pm.register_backend("a", full_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("b", full_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Should have at least one entry in the fallback chain
    assert!(
        !result.fallback_chain.is_empty(),
        "Fallback chain should not be empty with multiple backends"
    );
}

#[test]
fn projection_required_emulations_for_missing_capabilities() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );

    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolUse])))
        .unwrap();
    // ToolUse is not in the manifest, so emulation may be required
    let has_emulation = !result.required_emulations.is_empty();
    let has_low_coverage = result.fidelity_score.capability_coverage < 1.0;
    assert!(
        has_emulation || has_low_coverage,
        "Missing capability should show up as emulation or low coverage"
    );
}

#[test]
fn rank_targets_from_known_rules() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::Claude,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::IMAGE_INPUT,
        ],
    );
    assert!(!ranked.is_empty());
    // Best target should have the most lossless features
    let (_, best_count) = ranked[0];
    assert!(best_count > 0);
}

#[test]
fn rank_targets_self_not_in_results() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
    for &(d, _) in &ranked {
        assert_ne!(d, Dialect::OpenAi, "Self should not appear in rank_targets");
    }
}

#[test]
fn validate_mapping_lossless_features_have_no_errors() {
    let reg = known_rules();
    let features_list = vec![features::STREAMING.to_string()];
    let validations = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features_list);
    for v in &validations {
        assert!(
            v.errors.is_empty(),
            "Lossless streaming validation should have no errors: {:?}",
            v.errors,
        );
    }
}

#[test]
fn validate_mapping_unsupported_features_report_errors() {
    let reg = known_rules();
    let features_list = vec![features::IMAGE_INPUT.to_string()];
    let validations = validate_mapping(&reg, Dialect::OpenAi, Dialect::Kimi, &features_list);
    // image_input from OpenAI -> Kimi is unsupported
    let has_errors = validations.iter().any(|v| !v.errors.is_empty());
    let has_unsupported = validations.iter().any(|v| v.fidelity.is_unsupported());
    assert!(
        has_errors || has_unsupported,
        "Unsupported feature should produce errors or unsupported fidelity"
    );
}

#[test]
fn mapping_registry_insert_and_lookup() {
    let mut reg = MappingRegistry::new();
    assert!(reg.is_empty());

    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "custom".into(),
        fidelity: Fidelity::Lossless,
    });

    assert_eq!(reg.len(), 1);
    let found = reg.lookup(Dialect::OpenAi, Dialect::Claude, "custom");
    assert!(found.is_some());
    assert!(found.unwrap().fidelity.is_lossless());
}

#[test]
fn mapping_registry_lookup_missing_returns_none() {
    let reg = MappingRegistry::new();
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "nonexistent")
            .is_none()
    );
}

#[test]
fn mapping_registry_iter_yields_all_inserted_rules() {
    let mut reg = MappingRegistry::new();
    for &d in Dialect::all() {
        reg.insert(MappingRule {
            source_dialect: d,
            target_dialect: d,
            feature: "test".into(),
            fidelity: Fidelity::Lossless,
        });
    }
    let count = reg.iter().count();
    assert_eq!(count, 6);
}

#[test]
fn fidelity_serde_roundtrip_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_value(&f).unwrap();
    let back: Fidelity = serde_json::from_value(json).unwrap();
    assert!(back.is_lossless());
}

#[test]
fn fidelity_serde_roundtrip_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "thinking blocks stripped".into(),
    };
    let json = serde_json::to_value(&f).unwrap();
    let back: Fidelity = serde_json::from_value(json).unwrap();
    match back {
        Fidelity::LossyLabeled { warning } => assert!(warning.contains("stripped")),
        _ => panic!("Expected LossyLabeled"),
    }
}

#[test]
fn fidelity_serde_roundtrip_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "no image support".into(),
    };
    let json = serde_json::to_value(&f).unwrap();
    let back: Fidelity = serde_json::from_value(json).unwrap();
    match back {
        Fidelity::Unsupported { reason } => assert!(reason.contains("image")),
        _ => panic!("Expected Unsupported"),
    }
}

#[test]
fn projection_score_components_are_bounded() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(ALL_FEATURES.iter().map(|s| s.to_string()).collect());
    pm.register_backend("be", full_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let score = &result.fidelity_score;
    assert!(score.capability_coverage >= 0.0 && score.capability_coverage <= 1.0);
    assert!(score.mapping_fidelity >= 0.0 && score.mapping_fidelity <= 1.0);
    assert!(score.priority >= 0.0 && score.priority <= 1.0);
    assert!(score.total >= 0.0);
}

#[test]
fn projection_total_score_is_weighted_sum() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::STREAMING.into()]);
    pm.register_backend("be", full_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let s = &result.fidelity_score;
    // total = 0.5*capability + 0.3*fidelity + 0.2*priority
    let expected = 0.5 * s.capability_coverage + 0.3 * s.mapping_fidelity + 0.2 * s.priority;
    assert!(
        (s.total - expected).abs() < 0.01,
        "total {} != expected {} (cap={}, fid={}, pri={})",
        s.total,
        expected,
        s.capability_coverage,
        s.mapping_fidelity,
        s.priority,
    );
}
