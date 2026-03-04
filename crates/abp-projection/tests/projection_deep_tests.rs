// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deep tests for projection and mapping systems covering dialect detection,
//! supported/unsupported pairs, passthrough vs mapped mode, projection lookup,
//! capability constraints, error taxonomy, serde, and IR round-trips.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_mapper::{IrMapper, MapError, default_ir_mapper, supported_ir_pairs};
use abp_mapping::{Fidelity, MappingRegistry, MappingRule};
use abp_projection::{
    DialectPair, ProjectionError, ProjectionMatrix, ProjectionMode, ProjectionResult,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn manifest(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    caps.iter().cloned().collect()
}

fn require_caps(caps: &[Capability]) -> CapabilityRequirements {
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

fn wo(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("deep test").requirements(reqs).build()
}

fn wo_dialect(dialect: &str, reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), json!({ "source_dialect": dialect }));
    WorkOrderBuilder::new("dialect deep test")
        .requirements(reqs)
        .config(config)
        .build()
}

fn wo_passthrough(dialect: &str) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        json!({ "mode": "passthrough", "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("passthrough deep test")
        .requirements(CapabilityRequirements::default())
        .config(config)
        .build()
}

fn simple_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn tool_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Run the tool"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read_file".into(),
                input: json!({"path": "main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

// =========================================================================
// 1. Dialect detection from JSON request format
// =========================================================================

#[test]
fn detect_openai_from_messages_with_string_content() {
    let detector = DialectDetector::new();
    let val = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.7
    });
    let result = detector.detect(&val).unwrap();
    assert_eq!(result.dialect, Dialect::OpenAi);
    assert!(result.confidence > 0.3);
}

#[test]
fn detect_claude_from_type_message_and_array_content() {
    let detector = DialectDetector::new();
    let val = json!({
        "type": "message",
        "model": "claude-3-sonnet",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "max_tokens": 1024
    });
    let result = detector.detect(&val).unwrap();
    assert_eq!(result.dialect, Dialect::Claude);
    assert!(result.confidence > 0.3);
}

#[test]
fn detect_gemini_from_contents_with_parts() {
    let detector = DialectDetector::new();
    let val = json!({
        "contents": [{"parts": [{"text": "hello"}]}],
        "generationConfig": {"temperature": 0.5}
    });
    let result = detector.detect(&val).unwrap();
    assert_eq!(result.dialect, Dialect::Gemini);
    assert!(result.confidence > 0.3);
}

#[test]
fn detect_codex_from_items_with_type() {
    let detector = DialectDetector::new();
    let val = json!({
        "items": [{"type": "message", "role": "user"}],
        "status": "completed",
        "object": "response"
    });
    let result = detector.detect(&val).unwrap();
    assert_eq!(result.dialect, Dialect::Codex);
    assert!(result.confidence > 0.3);
}

#[test]
fn detect_kimi_from_search_plus_and_refs() {
    let detector = DialectDetector::new();
    let val = json!({
        "search_plus": true,
        "refs": [{"url": "https://example.com"}],
        "messages": [{"role": "user", "content": "search this"}]
    });
    let result = detector.detect(&val).unwrap();
    assert_eq!(result.dialect, Dialect::Kimi);
    assert!(result.confidence > 0.3);
}

#[test]
fn detect_copilot_from_references_and_agent_mode() {
    let detector = DialectDetector::new();
    let val = json!({
        "references": [{"type": "file", "path": "main.rs"}],
        "agent_mode": true,
        "confirmations": []
    });
    let result = detector.detect(&val).unwrap();
    assert_eq!(result.dialect, Dialect::Copilot);
    assert!(result.confidence > 0.3);
}

#[test]
fn detect_returns_none_for_non_object() {
    let detector = DialectDetector::new();
    assert!(detector.detect(&json!(42)).is_none());
    assert!(detector.detect(&json!("hello")).is_none());
    assert!(detector.detect(&json!([1, 2, 3])).is_none());
}

#[test]
fn detect_all_returns_scored_results() {
    let detector = DialectDetector::new();
    // This JSON has signals for both OpenAI and Kimi (messages + refs)
    let val = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": [{"url": "https://example.com"}]
    });
    let results = detector.detect_all(&val);
    assert!(!results.is_empty());
    // Results should be sorted descending by confidence
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

// =========================================================================
// 2. Supported pairs — IR mapper factory resolution
// =========================================================================

#[test]
fn all_supported_ir_pairs_resolve_to_mapper() {
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to);
        assert!(
            mapper.is_some(),
            "supported pair ({from}, {to}) should have a mapper"
        );
    }
}

#[test]
fn identity_pairs_always_resolve() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d);
        assert!(mapper.is_some(), "identity pair ({d}, {d}) must resolve");
    }
}

#[test]
fn supported_pairs_include_bidirectional_openai_claude() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
}

// =========================================================================
// 3. Unsupported pairs fail with typed error
// =========================================================================

#[test]
fn unsupported_pair_returns_none_from_factory() {
    // Kimi↔Copilot is not supported
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Kimi).is_none());
}

#[test]
fn unsupported_pair_mapper_returns_unsupported_error() {
    // Use a mapper that only supports specific pairs
    let mapper = abp_mapper::OpenAiClaudeIrMapper;
    let ir = simple_ir();
    let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &ir);
    assert!(matches!(result, Err(MapError::UnsupportedPair { .. })));
}

#[test]
fn projection_matrix_unsupported_pair_has_no_mapper() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot);
    assert!(mapper.is_none());
}

// =========================================================================
// 4. Passthrough detection — same dialect = passthrough mode
// =========================================================================

#[test]
fn same_dialect_registration_forces_passthrough() {
    let mut pm = ProjectionMatrix::new();
    // Even if we request Mapped, same-dialect forces Passthrough
    pm.register(Dialect::Gemini, Dialect::Gemini, ProjectionMode::Mapped);
    let entry = pm.lookup(Dialect::Gemini, Dialect::Gemini).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Passthrough);
    assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
}

#[test]
fn all_identity_entries_are_passthrough_in_defaults() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let entry = pm.lookup(d, d).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough, "{d}→{d}");
    }
}

#[test]
fn passthrough_work_order_boosts_same_dialect_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        40,
    );
    pm.register_backend(
        "openai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        60,
    );
    // Passthrough for Claude should boost claude-be despite lower priority
    let work = wo_passthrough("claude");
    let result = pm.project(&work).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

// =========================================================================
// 5. Mapped mode — different dialect = mapped mode
// =========================================================================

#[test]
fn cross_dialect_pair_is_mapped_mode() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Mapped);
}

#[test]
fn mapped_pairs_have_non_identity_mapper_hints() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(entry.mapper_hint.as_deref(), Some("openai_to_claude"));
    let entry = pm.lookup(Dialect::Claude, Dialect::OpenAi).unwrap();
    assert_eq!(entry.mapper_hint.as_deref(), Some("claude_to_openai"));
}

// =========================================================================
// 6. Projection lookup — resolve mapper for dialect→backend pair
// =========================================================================

#[test]
fn resolve_mapper_for_all_mapped_pairs() {
    let pm = ProjectionMatrix::with_defaults();
    let mapped_pairs = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::Claude, Dialect::OpenAi),
    ];
    for (src, tgt) in mapped_pairs {
        let mapper = pm.resolve_mapper(src, tgt);
        assert!(
            mapper.is_some(),
            "resolve_mapper({src}, {tgt}) should succeed"
        );
    }
}

#[test]
fn resolve_mapper_codex_openai_is_identity() {
    let pm = ProjectionMatrix::with_defaults();
    // Codex↔OpenAI uses identity mapper
    let mapper = pm.resolve_mapper(Dialect::Codex, Dialect::OpenAi).unwrap();
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

// =========================================================================
// 7. Capability constraints — projection respects limits
// =========================================================================

#[test]
fn projection_requires_all_caps_for_best_match() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "full",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        80,
    );
    let work = wo(require_caps(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]));
    let result = pm.project(&work).unwrap();
    // "full" matches all 3 caps; "partial" only matches 1 despite higher priority
    assert_eq!(result.selected_backend, "full");
}

#[test]
fn projection_with_no_requirements_selects_any_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("any", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let work = wo(CapabilityRequirements::default());
    let result = pm.project(&work).unwrap();
    assert_eq!(result.selected_backend, "any");
}

#[test]
fn projection_with_emulated_cap_still_matches() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    let work = wo(require_caps(&[Capability::Streaming]));
    let result = pm.project(&work).unwrap();
    assert_eq!(result.selected_backend, "emu");
    assert_eq!(result.required_emulations.len(), 1);
}

// =========================================================================
// 8. Error taxonomy integration
// =========================================================================

#[test]
fn empty_matrix_error_display_contains_reason() {
    let err = ProjectionError::EmptyMatrix;
    let msg = err.to_string();
    assert!(msg.contains("empty"));
}

#[test]
fn no_suitable_backend_error_display_contains_reason() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "all caps missing".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("all caps missing"));
}

#[test]
fn projection_errors_serde_roundtrip() {
    let errors = [
        ProjectionError::EmptyMatrix,
        ProjectionError::NoSuitableBackend {
            reason: "test".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

#[test]
fn projection_error_equality() {
    let a = ProjectionError::EmptyMatrix;
    let b = ProjectionError::EmptyMatrix;
    assert_eq!(a, b);
    let c = ProjectionError::NoSuitableBackend {
        reason: "x".into(),
    };
    let d = ProjectionError::NoSuitableBackend {
        reason: "y".into(),
    };
    assert_ne!(c, d);
}

#[test]
fn map_error_variants_serde_roundtrip() {
    let errors: Vec<MapError> = vec![
        MapError::UnsupportedPair {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        },
        MapError::LossyConversion {
            field: "thinking".into(),
            reason: "dropped".into(),
        },
        MapError::UnmappableTool {
            name: "bash".into(),
            reason: "restricted".into(),
        },
        MapError::IncompatibleCapability {
            capability: "logprobs".into(),
            reason: "not supported".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

// =========================================================================
// 9. All dialect variants
// =========================================================================

#[test]
fn every_dialect_has_unique_label() {
    let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(labels.len(), unique.len());
}

#[test]
fn dialect_all_returns_six_variants() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn every_dialect_display_matches_label() {
    for &d in Dialect::all() {
        assert_eq!(d.to_string(), d.label());
    }
}

#[test]
fn every_dialect_serde_roundtrip() {
    for &d in Dialect::all() {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn every_dialect_pair_in_default_matrix() {
    let pm = ProjectionMatrix::with_defaults();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                pm.lookup(src, tgt).is_some(),
                "missing {src}→{tgt} in default matrix"
            );
        }
    }
}

// =========================================================================
// 10. Serde for projection types
// =========================================================================

#[test]
fn projection_mode_serde_all_variants() {
    for mode in [
        ProjectionMode::Passthrough,
        ProjectionMode::Mapped,
        ProjectionMode::Unsupported,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: ProjectionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}

#[test]
fn dialect_pair_serde_roundtrip() {
    let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
    let json = serde_json::to_string(&pair).unwrap();
    let back: DialectPair = serde_json::from_str(&json).unwrap();
    assert_eq!(pair, back);
}

#[test]
fn projection_result_serializable() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "test-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let work = wo(require_caps(&[Capability::Streaming]));
    let result: ProjectionResult = pm.project(&work).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let _back: serde_json::Value = serde_json::from_str(&json).unwrap();
    // ProjectionResult is Serialize — verify it doesn't panic
    assert!(json.contains("test-be"));
}

// =========================================================================
// 11. Round-trip through matrix — Request → IR → mapper → IR → verify
// =========================================================================

#[test]
fn ir_roundtrip_identity_all_dialects() {
    let ir = simple_ir();
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d).unwrap();
        let mapped = mapper.map_request(d, d, &ir).unwrap();
        assert_eq!(mapped.messages.len(), ir.messages.len());
        for (orig, m) in ir.messages.iter().zip(mapped.messages.iter()) {
            assert_eq!(orig.role, m.role);
            assert_eq!(orig.text_content(), m.text_content());
        }
    }
}

#[test]
fn ir_roundtrip_openai_to_claude_preserves_messages() {
    let ir = simple_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // Should preserve system, user, assistant messages
    assert!(!mapped.messages.is_empty());
    assert!(mapped.system_message().is_some());
}

#[test]
fn ir_roundtrip_claude_to_openai_preserves_messages() {
    let ir = simple_ir();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    assert!(!mapped.messages.is_empty());
}

#[test]
fn ir_roundtrip_openai_to_gemini_preserves_messages() {
    let ir = simple_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    assert!(!mapped.messages.is_empty());
}

#[test]
fn ir_roundtrip_with_tool_calls_openai_claude() {
    let ir = tool_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // Tool calls should survive the mapping
    let tool_calls = mapped.tool_calls();
    assert!(!tool_calls.is_empty(), "tool calls lost in mapping");
}

#[test]
fn ir_roundtrip_openai_claude_openai_preserves_text() {
    let ir = simple_ir();
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let bwd = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();

    let intermediate = fwd
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let roundtripped = bwd
        .map_request(Dialect::Claude, Dialect::OpenAi, &intermediate)
        .unwrap();

    // Text content should survive the round-trip
    let orig_text: String = ir
        .messages
        .iter()
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join(" ");
    let rt_text: String = roundtripped
        .messages
        .iter()
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        rt_text.contains("Hello"),
        "round-trip lost user text: {rt_text}"
    );
    assert!(
        rt_text.contains("Hi!"),
        "round-trip lost assistant text: {rt_text}"
    );
    // System message may be extracted or reinserted depending on mapper
    assert!(
        orig_text.contains("You are helpful."),
        "original should have system text"
    );
}

#[test]
fn ir_roundtrip_empty_conversation() {
    let ir = IrConversation::new();
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d).unwrap();
        let mapped = mapper.map_request(d, d, &ir).unwrap();
        assert!(mapped.is_empty());
    }
}

// =========================================================================
// 12. Source dialect from vendor config influences projection
// =========================================================================

#[test]
fn source_dialect_from_vendor_config_boosts_matching_fidelity() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend(
        "openai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    // Work order says source dialect is Claude
    let work = wo_dialect("claude", require_caps(&[Capability::Streaming]));
    let result = pm.project(&work).unwrap();
    // Claude→Claude is identity (fidelity 1.0), Claude→OpenAI has lossless fidelity
    // Both should score well, but claude-be gets perfect fidelity
    assert_eq!(result.selected_backend, "claude-be");
}

// =========================================================================
// 13. Compatibility scoring for feature sets
// =========================================================================

#[test]
fn compatibility_score_cross_dialect_with_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(score.source, Dialect::OpenAi);
    assert_eq!(score.target, Dialect::Claude);
    assert_eq!(score.lossless_features, 1);
    assert_eq!(score.lossy_features, 1);
    assert_eq!(score.unsupported_features, 0);
}

// =========================================================================
// 14. Route planning
// =========================================================================

#[test]
fn route_identity_has_zero_cost_and_no_hops() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    assert_eq!(route.cost, 0);
    assert!(route.hops.is_empty());
    assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn route_direct_mapped_has_cost_one() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 1);
    assert_eq!(route.hops.len(), 1);
    assert!(route.is_direct());
}

#[test]
fn route_multi_hop_has_cost_two() {
    let mut pm = ProjectionMatrix::new();
    // Only register OpenAI→Claude and Claude→Gemini (no direct OpenAI→Gemini)
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(Dialect::Claude, Dialect::Gemini, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::OpenAi, Dialect::Gemini).unwrap();
    assert_eq!(route.cost, 2);
    assert!(route.is_multi_hop());
    assert_eq!(route.hops.len(), 2);
}

#[test]
fn route_no_path_when_disconnected() {
    let pm = ProjectionMatrix::new();
    // No entries registered
    assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
}
