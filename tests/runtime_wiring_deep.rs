#![allow(clippy::all)]
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

//! Deep integration tests for runtime wiring — connecting projection matrix,
//! capability negotiation, and mapper factory together.

use abp_capability::{
    CapabilityRegistry, NegotiationResult, SupportLevel, check_capability, generate_report,
    negotiate, negotiate_capabilities,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel, WorkOrderBuilder,
    ir::{IrContentBlock, IrConversation, IrMessage, IrRole},
};
use abp_dialect::Dialect;
use abp_error::{AbpError, ErrorCode};
use abp_mapper::{MapError, default_ir_mapper, supported_ir_pairs};
use abp_projection::{ProjectionMatrix, ProjectionMode};
use abp_runtime::{Runtime, RuntimeError};

// ── Helpers ─────────────────────────────────────────────────────────────

fn manifest(caps: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
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

fn simple_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn tool_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read src/main.rs"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({"path": "src/main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Runtime + ProjectionMatrix (15 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn projection_selects_mapper_for_identity_pair() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi);
    assert!(mapper.is_some(), "identity pair should resolve a mapper");
}

#[test]
fn projection_selects_mapper_for_openai_to_claude() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
    assert!(mapper.is_some());
    let m = mapper.unwrap();
    assert_eq!(m.source_dialect(), Dialect::OpenAi);
    assert_eq!(m.target_dialect(), Dialect::Claude);
}

#[test]
fn projection_selects_mapper_for_claude_to_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::Claude, Dialect::OpenAi);
    assert!(mapper.is_some());
    let m = mapper.unwrap();
    assert_eq!(m.source_dialect(), Dialect::Claude);
    assert_eq!(m.target_dialect(), Dialect::OpenAi);
}

#[test]
fn projection_returns_none_for_unsupported_pair() {
    let pm = ProjectionMatrix::with_defaults();
    // Kimi ↔ Copilot is registered as unsupported in defaults
    let mapper = pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot);
    assert!(mapper.is_none());
}

#[test]
fn projection_lookup_passthrough_mode() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::Claude, Dialect::Claude);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().mode, ProjectionMode::Passthrough);
}

#[test]
fn projection_lookup_mapped_mode() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
}

#[test]
fn projection_lookup_unsupported_mode() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().mode, ProjectionMode::Unsupported);
}

#[test]
fn runtime_with_projection_selects_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mock",
        manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    let rt = Runtime::with_default_backends().with_projection(pm);

    let wo = WorkOrderBuilder::new("test")
        .requirements(require_caps(&[Capability::Streaming]))
        .build();

    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "mock");
}

#[test]
fn runtime_without_projection_returns_error() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("test").build();
    let err = rt.select_backend(&wo).unwrap_err();
    assert!(matches!(err, RuntimeError::NoProjectionMatch { .. }));
}

#[test]
fn runtime_projection_selects_highest_scoring_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "backend-low",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        Dialect::OpenAi,
        10,
    );
    pm.register_backend(
        "backend-high",
        manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]),
        Dialect::OpenAi,
        90,
    );

    let mut rt = Runtime::new();
    rt.register_backend("backend-low", abp_backend_mock::MockBackend);
    rt.register_backend("backend-high", abp_backend_mock::MockBackend);
    let rt = rt.with_projection(pm);

    let wo = WorkOrderBuilder::new("test")
        .requirements(require_caps(&[Capability::Streaming]))
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "backend-high");
}

#[test]
fn projection_codex_to_openai_uses_identity_mapper() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::Codex, Dialect::OpenAi);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().mapper_hint.as_deref(), Some("identity"));
}

#[test]
fn projection_find_route_identity_has_zero_cost() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi);
    assert!(route.is_some());
    let r = route.unwrap();
    assert_eq!(r.cost, 0);
    assert_eq!(r.fidelity, 1.0);
}

#[test]
fn projection_find_route_direct_has_cost_one() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude);
    assert!(route.is_some());
    let r = route.unwrap();
    assert_eq!(r.cost, 1);
    assert!(r.is_direct());
}

#[test]
fn projection_compatibility_score_identity_perfect() {
    let pm = ProjectionMatrix::with_defaults();
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
    assert_eq!(score.fidelity, 1.0);
    assert_eq!(score.lossy_features, 0);
    assert_eq!(score.unsupported_features, 0);
}

#[test]
fn projection_result_has_fallback_chain() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "first",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );
    pm.register_backend(
        "second",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let mut rt = Runtime::new();
    rt.register_backend("first", abp_backend_mock::MockBackend);
    rt.register_backend("second", abp_backend_mock::MockBackend);
    let rt = rt.with_projection(pm);

    let wo = WorkOrderBuilder::new("test")
        .requirements(require_caps(&[Capability::Streaming]))
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert!(!result.fallback_chain.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Runtime + Capability negotiation (15 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_streaming_native_is_viable() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    let result = negotiate_capabilities(&[Capability::Streaming], &m);
    assert!(result.is_viable());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn negotiate_missing_capability_returns_unsupported() {
    let m = CapabilityManifest::new();
    let result = negotiate_capabilities(&[Capability::McpClient], &m);
    assert!(!result.is_viable());
    assert_eq!(result.unsupported.len(), 1);
    assert_eq!(result.unsupported[0].0, Capability::McpClient);
}

#[test]
fn check_capability_not_in_manifest_reason() {
    let m = CapabilityManifest::new();
    let level = check_capability(&m, &Capability::Vision);
    match level {
        SupportLevel::Unsupported { reason } => {
            assert_eq!(reason, "not declared in manifest");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn check_capability_explicitly_unsupported_reason() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Vision, CoreSupportLevel::Unsupported);
    let level = check_capability(&m, &Capability::Vision);
    match level {
        SupportLevel::Unsupported { reason } => {
            assert_eq!(reason, "explicitly marked unsupported");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn check_capability_emulated_returns_method() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, CoreSupportLevel::Emulated);
    let level = check_capability(&m, &Capability::ToolRead);
    match level {
        SupportLevel::Emulated { method } => {
            assert_eq!(method, "adapter");
        }
        other => panic!("expected Emulated, got {other:?}"),
    }
}

#[test]
fn runtime_check_capabilities_passes_for_mock() {
    let rt = Runtime::with_default_backends();
    let reqs = require_caps(&[Capability::Streaming]);
    rt.check_capabilities("mock", &reqs).unwrap();
}

#[test]
fn runtime_check_capabilities_fails_for_missing() {
    let rt = Runtime::with_default_backends();
    let reqs = require_native(&[Capability::McpClient]);
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[test]
fn runtime_check_capabilities_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let reqs = require_caps(&[Capability::Streaming]);
    let err = rt.check_capabilities("nonexistent", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn negotiate_empty_requirements_always_viable() {
    let m = CapabilityManifest::new();
    let result = negotiate_capabilities(&[], &m);
    assert!(result.is_viable());
    assert_eq!(result.total(), 0);
}

#[test]
fn negotiate_multiple_capabilities_mixed_support() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    m.insert(Capability::ToolRead, CoreSupportLevel::Emulated);
    let result = negotiate_capabilities(
        &[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::McpClient,
        ],
        &m,
    );
    assert!(!result.is_viable());
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn negotiate_with_structured_requirements() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let result = negotiate(&m, &reqs);
    assert!(result.is_compatible());
}

#[test]
fn generate_report_compatible() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn generate_report_incompatible() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::McpClient]);
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
}

#[test]
fn capability_registry_with_defaults_has_all_dialects() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.len() >= 6);
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
    assert!(reg.contains("google/gemini-1.5-pro"));
}

#[test]
fn capability_registry_negotiate_by_name() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::Streaming])
        .unwrap();
    assert!(result.is_viable());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Mapper pipeline (15 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapper_identity_preserves_conversation() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.messages.len(), conv.messages.len());
    assert_eq!(result, conv);
}

#[test]
fn ir_mapper_openai_to_claude_maps_messages() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert!(!result.messages.is_empty());
}

#[test]
fn ir_mapper_claude_to_openai_maps_messages() {
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    assert!(!result.messages.is_empty());
}

#[test]
fn ir_mapper_openai_to_gemini_maps_messages() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert!(!result.messages.is_empty());
}

#[test]
fn ir_mapper_claude_to_gemini_maps_messages() {
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    assert!(!result.messages.is_empty());
}

#[test]
fn ir_mapper_unsupported_pair_returns_none() {
    let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Copilot);
    assert!(mapper.is_none());
}

#[test]
fn ir_mapper_supported_pairs_includes_identity() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(
            pairs.contains(&(d, d)),
            "identity pair ({d:?}, {d:?}) missing"
        );
    }
}

#[test]
fn ir_mapper_supported_pairs_includes_cross_dialect() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
}

#[test]
fn ir_mapper_roundtrip_openai_claude_openai() {
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let back = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let conv = simple_conversation();

    let mid = fwd
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let final_conv = back
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();

    // Text content should survive roundtrip
    let original_user = conv.messages[1].text_content();
    let roundtrip_user_msgs: Vec<_> = final_conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::User)
        .collect();
    assert!(!roundtrip_user_msgs.is_empty());
    assert!(
        roundtrip_user_msgs
            .iter()
            .any(|m| m.text_content() == original_user)
    );
}

#[test]
fn ir_mapper_tool_use_roundtrip() {
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = tool_conversation();

    let result = fwd
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();

    // Tool use blocks should survive the mapping
    let tool_calls = result.tool_calls();
    assert!(!tool_calls.is_empty());
}

#[test]
fn ir_mapper_response_mapping_openai_to_claude() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Done!")]);
    let result = mapper
        .map_response(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert!(!result.messages.is_empty());
}

#[test]
fn ir_mapper_empty_conversation() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = IrConversation::new();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn projection_resolve_mapper_openai_to_claude_maps_request() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let req = abp_mapper::DialectRequest {
        dialect: Dialect::OpenAi,
        body: serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    // Should produce a Claude-formatted body
    assert!(result.is_object());
}

#[test]
fn ir_mapper_gemini_to_openai_roundtrip() {
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let back = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).unwrap();
    let conv = simple_conversation();
    let mid = fwd
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    let final_conv = back
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    // User messages should survive
    let user_msgs: Vec<_> = final_conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::User)
        .collect();
    assert!(!user_msgs.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Error propagation (15 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_code_backend_not_found_snake_case() {
    let code = ErrorCode::BackendNotFound;
    assert_eq!(code.as_str(), "backend_not_found");
}

#[test]
fn error_code_backend_timeout_snake_case() {
    let code = ErrorCode::BackendTimeout;
    assert_eq!(code.as_str(), "backend_timeout");
}

#[test]
fn error_code_capability_unsupported_snake_case() {
    let code = ErrorCode::CapabilityUnsupported;
    assert_eq!(code.as_str(), "capability_unsupported");
}

#[test]
fn error_code_display_uses_message() {
    let code = ErrorCode::BackendTimeout;
    let display = format!("{code}");
    assert_eq!(display, code.message());
    assert_eq!(display, "backend timed out");
}

#[test]
fn runtime_error_unknown_backend_has_correct_code() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_capability_check_has_correct_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_workspace_failed_has_correct_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_policy_failed_has_correct_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_backend_failed_has_correct_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_no_projection_match_has_correct_code() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no matrix".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn abp_error_converts_to_runtime_error() {
    let abp_err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), ErrorCode::BackendTimeout);
}

#[test]
fn runtime_error_into_abp_error_preserves_code() {
    let rt_err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let abp_err = rt_err.into_abp_error();
    assert_eq!(abp_err.code, ErrorCode::BackendNotFound);
    assert!(abp_err.message.contains("missing"));
}

#[test]
fn map_error_unsupported_pair_display() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("Kimi"));
    assert!(msg.contains("Copilot"));
}

#[test]
fn projection_empty_matrix_error() {
    let pm = ProjectionMatrix::new();
    let wo = WorkOrderBuilder::new("test").build();
    let err = pm.project(&wo).unwrap_err();
    assert!(err.to_string().contains("empty"));
}

#[test]
fn error_code_serde_roundtrip_snake_case() {
    let code = ErrorCode::BackendTimeout;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ErrorCode::BackendTimeout);
}

// ═══════════════════════════════════════════════════════════════════════
// Additional cross-cutting integration tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn projection_matrix_with_capability_negotiation() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-backend",
        manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
            (Capability::ExtendedThinking, CoreSupportLevel::Native),
        ]),
        Dialect::Claude,
        80,
    );
    pm.register_backend(
        "openai-backend",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );

    let wo = WorkOrderBuilder::new("test")
        .requirements(require_caps(&[
            Capability::Streaming,
            Capability::ExtendedThinking,
        ]))
        .build();

    let result = pm.project(&wo).unwrap();
    // Claude backend should be selected (it has ExtendedThinking)
    assert_eq!(result.selected_backend, "claude-backend");
}

#[test]
fn runtime_projection_with_mapper_and_capabilities() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register_backend(
        "mock",
        manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );

    let rt = Runtime::with_default_backends().with_projection(pm);

    // Verify projection matrix is accessible
    assert!(rt.projection().is_some());

    let wo = WorkOrderBuilder::new("test")
        .requirements(require_caps(&[Capability::Streaming]))
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "mock");

    // The mapper should be resolvable for OpenAI identity
    let pm = rt.projection().unwrap();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi);
    assert!(mapper.is_some());
}

#[test]
fn error_code_categories_are_correct() {
    use abp_error::ErrorCategory;

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
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(
        ErrorCode::MappingDialectMismatch.category(),
        ErrorCategory::Mapping
    );
}

#[test]
fn error_code_retryability() {
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(!ErrorCode::BackendNotFound.is_retryable());
    assert!(!ErrorCode::CapabilityUnsupported.is_retryable());
    assert!(!ErrorCode::PolicyDenied.is_retryable());
}

#[test]
fn ir_mapper_all_identity_pairs_work() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d).unwrap();
        let conv = simple_conversation();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(result, conv, "identity pair for {d:?} should preserve");
    }
}

#[test]
fn negotiate_restricted_capability_treated_as_emulated() {
    let mut m = CapabilityManifest::new();
    m.insert(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed only".into(),
        },
    );
    let result = negotiate_capabilities(&[Capability::ToolBash], &m);
    // Restricted is treated as emulated in negotiation
    assert!(result.is_viable());
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn classified_runtime_error_preserves_context() {
    let abp_err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml");
    let rt_err: RuntimeError = abp_err.into();
    let back = rt_err.into_abp_error();
    assert_eq!(back.code, ErrorCode::ConfigInvalid);
    assert_eq!(
        back.context.get("file"),
        Some(&serde_json::json!("backplane.toml"))
    );
}

#[test]
fn projection_score_components_sum_correctly() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "test-backend",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        Dialect::OpenAi,
        100,
    );
    let wo = WorkOrderBuilder::new("test")
        .requirements(require_caps(&[Capability::Streaming]))
        .build();
    let result = pm.project(&wo).unwrap();
    let score = &result.fidelity_score;
    // Total is a weighted sum of components
    assert!(score.total > 0.0);
    assert!(score.capability_coverage > 0.0);
    assert!(score.priority > 0.0);
}

#[test]
fn negotiation_result_total_counts_all_buckets() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::McpClient],
    );
    assert_eq!(result.total(), 3);
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn negotiation_result_emulated_caps_extraction() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![],
    );
    let emu = result.emulated_caps();
    assert_eq!(emu.len(), 2);
    assert!(emu.contains(&Capability::ToolRead));
    assert!(emu.contains(&Capability::ToolWrite));
}

#[test]
fn negotiation_result_unsupported_caps_extraction() {
    let result = NegotiationResult::from_simple(
        vec![],
        vec![],
        vec![Capability::McpClient, Capability::McpServer],
    );
    let unsup = result.unsupported_caps();
    assert_eq!(unsup.len(), 2);
    assert!(unsup.contains(&Capability::McpClient));
    assert!(unsup.contains(&Capability::McpServer));
}

#[test]
fn map_error_lossy_conversion_display() {
    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "target has no thinking block".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("lossy"));
}

#[test]
fn map_error_unmappable_tool_display() {
    let err = MapError::UnmappableTool {
        name: "computer_use".into(),
        reason: "not supported in target".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("computer_use"));
}

#[test]
fn map_error_incompatible_capability_display() {
    let err = MapError::IncompatibleCapability {
        capability: "logprobs".into(),
        reason: "target dialect does not support logprobs".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
}

#[test]
fn error_code_all_variants_have_messages() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::PolicyDenied,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::Internal,
    ];
    for code in codes {
        assert!(
            !code.message().is_empty(),
            "{:?} should have a message",
            code
        );
        assert!(!code.as_str().is_empty(), "{:?} should have as_str", code);
    }
}

#[test]
fn projection_matrix_defaults_cover_all_identity_pairs() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let entry = pm.lookup(d, d);
        assert!(entry.is_some(), "identity pair ({d:?}, {d:?}) should exist");
        assert_eq!(entry.unwrap().mode, ProjectionMode::Passthrough);
    }
}
