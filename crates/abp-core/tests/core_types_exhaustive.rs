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
//! Exhaustive tests for all public core types in `abp-core`.

use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ContractError, ExecutionLane, ExecutionMode, MinSupport,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// 1. CONTRACT_VERSION
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_starts_with_abp() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. ExecutionMode
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serialize_passthrough() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, r#""passthrough""#);
}

#[test]
fn execution_mode_serialize_mapped() {
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(json, r#""mapped""#);
}

#[test]
fn execution_mode_deserialize_passthrough() {
    let mode: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[test]
fn execution_mode_deserialize_mapped() {
    let mode: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn execution_mode_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Outcome
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn outcome_serialize_complete() {
    assert_eq!(
        serde_json::to_string(&Outcome::Complete).unwrap(),
        r#""complete""#
    );
}

#[test]
fn outcome_serialize_partial() {
    assert_eq!(
        serde_json::to_string(&Outcome::Partial).unwrap(),
        r#""partial""#
    );
}

#[test]
fn outcome_serialize_failed() {
    assert_eq!(
        serde_json::to_string(&Outcome::Failed).unwrap(),
        r#""failed""#
    );
}

#[test]
fn outcome_deserialize_all() {
    let complete: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(complete, Outcome::Complete);
    let partial: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(partial, Outcome::Partial);
    let failed: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(failed, Outcome::Failed);
}

#[test]
fn outcome_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. ExecutionLane
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn execution_lane_serialize_patch_first() {
    let json = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(json, r#""patch_first""#);
}

#[test]
fn execution_lane_serialize_workspace_first() {
    let json = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(json, r#""workspace_first""#);
}

#[test]
fn execution_lane_roundtrip() {
    for lane in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        let json = serde_json::to_string(&lane).unwrap();
        let back: ExecutionLane = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. WorkspaceMode
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_mode_serialize_pass_through() {
    let json = serde_json::to_string(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(json, r#""pass_through""#);
}

#[test]
fn workspace_mode_serialize_staged() {
    let json = serde_json::to_string(&WorkspaceMode::Staged).unwrap();
    assert_eq!(json, r#""staged""#);
}

#[test]
fn workspace_mode_roundtrip() {
    for mode in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: WorkspaceMode = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Capability — all variants
// ═══════════════════════════════════════════════════════════════════════════

fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ]
}

#[test]
fn capability_serialize_all_variants_snake_case() {
    let expected = vec![
        ("streaming", Capability::Streaming),
        ("tool_read", Capability::ToolRead),
        ("tool_write", Capability::ToolWrite),
        ("tool_edit", Capability::ToolEdit),
        ("tool_bash", Capability::ToolBash),
        ("tool_glob", Capability::ToolGlob),
        ("tool_grep", Capability::ToolGrep),
        ("tool_web_search", Capability::ToolWebSearch),
        ("tool_web_fetch", Capability::ToolWebFetch),
        ("tool_ask_user", Capability::ToolAskUser),
        ("hooks_pre_tool_use", Capability::HooksPreToolUse),
        ("hooks_post_tool_use", Capability::HooksPostToolUse),
        ("session_resume", Capability::SessionResume),
        ("session_fork", Capability::SessionFork),
        ("checkpointing", Capability::Checkpointing),
        (
            "structured_output_json_schema",
            Capability::StructuredOutputJsonSchema,
        ),
        ("mcp_client", Capability::McpClient),
        ("mcp_server", Capability::McpServer),
        ("tool_use", Capability::ToolUse),
        ("extended_thinking", Capability::ExtendedThinking),
        ("image_input", Capability::ImageInput),
        ("pdf_input", Capability::PdfInput),
        ("code_execution", Capability::CodeExecution),
        ("logprobs", Capability::Logprobs),
        ("seed_determinism", Capability::SeedDeterminism),
        ("stop_sequences", Capability::StopSequences),
        ("function_calling", Capability::FunctionCalling),
        ("vision", Capability::Vision),
        ("audio", Capability::Audio),
        ("json_mode", Capability::JsonMode),
        ("system_message", Capability::SystemMessage),
        ("temperature", Capability::Temperature),
        ("top_p", Capability::TopP),
        ("top_k", Capability::TopK),
        ("max_tokens", Capability::MaxTokens),
        ("frequency_penalty", Capability::FrequencyPenalty),
        ("presence_penalty", Capability::PresencePenalty),
        ("cache_control", Capability::CacheControl),
        ("batch_mode", Capability::BatchMode),
        ("embeddings", Capability::Embeddings),
        ("image_generation", Capability::ImageGeneration),
    ];
    for (name, cap) in &expected {
        let json = serde_json::to_string(cap).unwrap();
        assert_eq!(json, format!("\"{name}\""), "Capability {cap:?}");
    }
}

#[test]
fn capability_roundtrip_all() {
    for cap in all_capabilities() {
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }
}

#[test]
fn capability_count() {
    assert_eq!(all_capabilities().len(), 41);
}

#[test]
fn capability_ord_is_deterministic() {
    let mut caps = all_capabilities();
    let original = caps.clone();
    caps.sort();
    // Re-sorting should be stable
    let mut again = caps.clone();
    again.sort();
    assert_eq!(caps, again);
    // Sorted order may differ from definition order but must be consistent
    assert_eq!(original.len(), caps.len());
}

#[test]
fn capability_btreemap_insertion_order() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);

    // BTreeMap keys are sorted by Ord
    let keys: Vec<&Capability> = manifest.keys().collect();
    for i in 1..keys.len() {
        assert!(keys[i - 1] < keys[i], "BTreeMap must maintain sorted order");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. SupportLevel
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn support_level_serialize_native() {
    let json = serde_json::to_string(&SupportLevel::Native).unwrap();
    assert_eq!(json, r#""native""#);
}

#[test]
fn support_level_serialize_emulated() {
    let json = serde_json::to_string(&SupportLevel::Emulated).unwrap();
    assert_eq!(json, r#""emulated""#);
}

#[test]
fn support_level_serialize_unsupported() {
    let json = serde_json::to_string(&SupportLevel::Unsupported).unwrap();
    assert_eq!(json, r#""unsupported""#);
}

#[test]
fn support_level_serialize_restricted() {
    let json = serde_json::to_string(&SupportLevel::Restricted {
        reason: "disabled by policy".into(),
    })
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["restricted"]["reason"], "disabled by policy");
}

#[test]
fn support_level_roundtrip_all() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "test reason".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. MinSupport + satisfies()
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn min_support_serialize() {
    assert_eq!(
        serde_json::to_string(&MinSupport::Native).unwrap(),
        r#""native""#
    );
    assert_eq!(
        serde_json::to_string(&MinSupport::Emulated).unwrap(),
        r#""emulated""#
    );
}

#[test]
fn satisfies_native_requires_native_only() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Restricted { reason: "r".into() }.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_emulated_accepts_native_emulated_restricted() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Restricted { reason: "r".into() }.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. CapabilityManifest
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_is_btreemap() {
    let mut m = CapabilityManifest::new();
    assert!(m.is_empty());
    m.insert(Capability::Streaming, SupportLevel::Native);
    assert_eq!(m.len(), 1);
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn capability_manifest_merge() {
    let mut base = CapabilityManifest::new();
    base.insert(Capability::ToolRead, SupportLevel::Native);
    base.insert(Capability::ToolWrite, SupportLevel::Emulated);

    let mut overlay = CapabilityManifest::new();
    overlay.insert(Capability::ToolWrite, SupportLevel::Native);
    overlay.insert(Capability::Streaming, SupportLevel::Native);

    base.extend(overlay);
    assert_eq!(base.len(), 3);
    // overlay overwrites ToolWrite
    assert!(matches!(
        base.get(&Capability::ToolWrite),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_serialization_deterministic() {
    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolRead, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Emulated);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::Streaming, SupportLevel::Emulated);
    m2.insert(Capability::ToolRead, SupportLevel::Native);

    assert_eq!(
        serde_json::to_string(&m1).unwrap(),
        serde_json::to_string(&m2).unwrap()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. CapabilityRequirement + CapabilityRequirements
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_requirement_construction() {
    let req = CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Native,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CapabilityRequirement = serde_json::from_str(&json).unwrap();
    assert_eq!(back.capability, Capability::ToolRead);
}

#[test]
fn capability_requirements_default_empty() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

#[test]
fn capability_requirements_serialize_roundtrip() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let json = serde_json::to_string(&reqs).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(back.required.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. AgentEventKind — all variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_kind_run_started_serde() {
    let kind = AgentEventKind::RunStarted {
        message: "starting".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"run_started""#));
    assert!(json.contains(r#""message":"starting""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, AgentEventKind::RunStarted { .. }));
}

#[test]
fn event_kind_run_completed_serde() {
    let kind = AgentEventKind::RunCompleted {
        message: "done".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"run_completed""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn event_kind_assistant_delta_serde() {
    let kind = AgentEventKind::AssistantDelta {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"assistant_delta""#));
    assert!(json.contains(r#""text":"hello""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn event_kind_assistant_message_serde() {
    let kind = AgentEventKind::AssistantMessage {
        text: "hi there".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"assistant_message""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = back {
        assert_eq!(text, "hi there");
    } else {
        panic!("expected AssistantMessage");
    }
}

#[test]
fn event_kind_tool_call_serde() {
    let kind = AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"tool_call""#));
    assert!(json.contains(r#""tool_name":"read_file""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        parent_tool_use_id,
        ..
    } = back
    {
        assert_eq!(tool_name, "read_file");
        assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
        assert!(parent_tool_use_id.is_none());
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn event_kind_tool_call_with_parent() {
    let kind = AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_2".into()),
        parent_tool_use_id: Some("tu_1".into()),
        input: json!({"command": "ls"}),
    };
    let json = serde_json::to_string(&kind).unwrap();
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall {
        parent_tool_use_id, ..
    } = back
    {
        assert_eq!(parent_tool_use_id.as_deref(), Some("tu_1"));
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn event_kind_tool_result_serde() {
    let kind = AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_1".into()),
        output: json!("file contents"),
        is_error: false,
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"tool_result""#));
    assert!(json.contains(r#""is_error":false"#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolResult {
        tool_name,
        is_error,
        ..
    } = back
    {
        assert_eq!(tool_name, "read_file");
        assert!(!is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn event_kind_tool_result_error() {
    let kind = AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("command not found"),
        is_error: true,
    };
    let json = serde_json::to_string(&kind).unwrap();
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolResult { is_error, .. } = back {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn event_kind_file_changed_serde() {
    let kind = AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"file_changed""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::FileChanged { path, summary } = back {
        assert_eq!(path, "src/lib.rs");
        assert_eq!(summary, "added function");
    } else {
        panic!("expected FileChanged");
    }
}

#[test]
fn event_kind_command_executed_serde() {
    let kind = AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"command_executed""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::CommandExecuted {
        command,
        exit_code,
        output_preview,
    } = back
    {
        assert_eq!(command, "cargo test");
        assert_eq!(exit_code, Some(0));
        assert_eq!(output_preview.as_deref(), Some("test result: ok"));
    } else {
        panic!("expected CommandExecuted");
    }
}

#[test]
fn event_kind_command_executed_optional_fields() {
    let kind = AgentEventKind::CommandExecuted {
        command: "sleep 1".into(),
        exit_code: None,
        output_preview: None,
    };
    let json = serde_json::to_string(&kind).unwrap();
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::CommandExecuted {
        exit_code,
        output_preview,
        ..
    } = back
    {
        assert!(exit_code.is_none());
        assert!(output_preview.is_none());
    } else {
        panic!("expected CommandExecuted");
    }
}

#[test]
fn event_kind_warning_serde() {
    let kind = AgentEventKind::Warning {
        message: "something odd".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"warning""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Warning { message } = back {
        assert_eq!(message, "something odd");
    } else {
        panic!("expected Warning");
    }
}

#[test]
fn event_kind_error_without_code() {
    let kind = AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""type":"error""#));
    // error_code should be skipped when None
    assert!(!json.contains("error_code"));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error {
        message,
        error_code,
    } = back
    {
        assert_eq!(message, "boom");
        assert!(error_code.is_none());
    } else {
        panic!("expected Error");
    }
}

#[test]
fn event_kind_error_with_code() {
    let kind = AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains(r#""error_code":"backend_timeout""#));
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = back {
        let code = error_code.unwrap();
        assert_eq!(code.as_str(), "backend_timeout");
    } else {
        panic!("expected Error");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. AgentEvent (full struct with ts, kind, ext)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_event_serialize_with_kind_flattened() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    // kind is flattened, so "type" appears at top level
    assert!(json.contains(r#""type":"assistant_message""#));
    assert!(json.contains(r#""ts""#));
    // ext is None, check it doesn't add unexpected keys
    assert!(!json.contains("raw_message"));
}

#[test]
fn agent_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"role": "assistant"}));

    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "token".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("raw_message"));
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(back.ext.is_some());
    let ext = back.ext.unwrap();
    assert!(ext.contains_key("raw_message"));
}

#[test]
fn agent_event_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "a.rs".into(),
            summary: "edit".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::FileChanged { path, .. } = &back.kind {
        assert_eq!(path, "a.rs");
    } else {
        panic!("wrong variant");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. WorkOrderBuilder / WorkOrder
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
    assert_eq!(wo.workspace.root, ".");
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.requirements.required.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
}

#[test]
fn work_order_builder_lane() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn work_order_builder_default_lane_is_patch_first() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn work_order_builder_root() {
    let wo = WorkOrderBuilder::new("t").root("/tmp/ws").build();
    assert_eq!(wo.workspace.root, "/tmp/ws");
}

#[test]
fn work_order_builder_workspace_mode() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn work_order_builder_default_workspace_mode_is_staged() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn work_order_builder_include_exclude() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_builder_model() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_builder_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(5).build();
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn work_order_builder_max_budget_usd() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(1.50).build();
    assert_eq!(wo.config.max_budget_usd, Some(1.50));
}

#[test]
fn work_order_builder_context() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "use foo".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "hint");
}

#[test]
fn work_order_builder_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.key".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["write".into()],
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo.policy.disallowed_tools, vec!["bash"]);
    assert_eq!(wo.policy.deny_read, vec!["*.key"]);
}

#[test]
fn work_order_builder_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn work_order_builder_config() {
    let config = RuntimeConfig {
        model: Some("claude-3".into()),
        vendor: BTreeMap::new(),
        env: BTreeMap::new(),
        max_budget_usd: Some(10.0),
        max_turns: Some(20),
    };
    let wo = WorkOrderBuilder::new("t").config(config).build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo.config.max_turns, Some(20));
}

#[test]
fn work_order_has_uuid() {
    let wo = WorkOrderBuilder::new("t").build();
    assert_ne!(wo.id, Uuid::nil());
}

#[test]
fn work_order_serializes_roundtrip() {
    let wo = WorkOrderBuilder::new("Fix auth")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/workspace")
        .model("gpt-4")
        .max_turns(10)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "Fix auth");
    assert_eq!(back.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(back.config.max_turns, Some(10));
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. ReceiptBuilder / Receipt
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_defaults() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.mode, ExecutionMode::Mapped);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_outcome() {
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_backend_id() {
    let r = ReceiptBuilder::new("a").backend_id("b").build();
    assert_eq!(r.backend.id, "b");
}

#[test]
fn receipt_builder_backend_version() {
    let r = ReceiptBuilder::new("mock").backend_version("1.2.3").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
}

#[test]
fn receipt_builder_adapter_version() {
    let r = ReceiptBuilder::new("mock").adapter_version("0.5.0").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
}

#[test]
fn receipt_builder_mode() {
    let r = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_builder_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn receipt_builder_timestamps() {
    let start = Utc::now();
    let finish = start + chrono::Duration::seconds(5);
    let r = ReceiptBuilder::new("mock")
        .started_at(start)
        .finished_at(finish)
        .build();
    assert_eq!(r.meta.started_at, start);
    assert_eq!(r.meta.finished_at, finish);
    assert_eq!(r.meta.duration_ms, 5000);
}

#[test]
fn receipt_builder_add_trace_event() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let r = ReceiptBuilder::new("mock").add_trace_event(event).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_add_artifact() {
    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let r = ReceiptBuilder::new("mock").add_artifact(artifact).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn receipt_builder_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("mock").capabilities(caps).build();
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn receipt_builder_usage_raw() {
    let raw = json!({"prompt_tokens": 100});
    let r = ReceiptBuilder::new("mock").usage_raw(raw.clone()).build();
    assert_eq!(r.usage_raw, raw);
}

#[test]
fn receipt_builder_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.01),
    };
    let r = ReceiptBuilder::new("mock").usage(usage).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(50));
}

#[test]
fn receipt_builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff content".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("mock").verification(v).build();
    assert!(r.verification.harness_ok);
    assert_eq!(r.verification.git_diff.as_deref(), Some("diff content"));
}

#[test]
fn receipt_builder_with_hash() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Receipt hashing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_hash_produces_64_hex_chars() {
    let r = ReceiptBuilder::new("mock").build().with_hash().unwrap();
    let hash = r.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_is_deterministic() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_ignores_existing_hash_field() {
    let mut r = ReceiptBuilder::new("mock").build();
    let h1 = receipt_hash(&r).unwrap();
    r.receipt_sha256 = Some("old_hash_value".into());
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_differs_for_different_outcomes() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    // These will have different run_ids too (Uuid::new_v4), so hashes will differ
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_serializes_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build()
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.outcome, Outcome::Partial);
    assert_eq!(back.receipt_sha256, r.receipt_sha256);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. sha256_hex + canonical_json
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sha256_hex_returns_64_hex_chars() {
    let hex = sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn sha256_hex_known_value() {
    // SHA-256 of empty string
    let hex = sha256_hex(b"");
    assert_eq!(
        hex,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_different_inputs_differ() {
    assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
}

#[test]
fn canonical_json_sorts_keys() {
    let json = canonical_json(&json!({"z": 1, "a": 2})).unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    assert!(a_pos < z_pos, "keys must be sorted");
}

#[test]
fn canonical_json_deterministic() {
    let val = json!({"b": 2, "a": 1});
    let j1 = canonical_json(&val).unwrap();
    let j2 = canonical_json(&val).unwrap();
    assert_eq!(j1, j2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. ErrorCode (from abp_error)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_as_str_snake_case() {
    let code = abp_error::ErrorCode::BackendTimeout;
    assert_eq!(code.as_str(), "backend_timeout");
}

#[test]
fn error_code_display_differs_from_as_str() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let display = format!("{code}");
    let as_str = code.as_str();
    assert_ne!(display, as_str);
}

#[test]
fn error_code_category() {
    assert_eq!(
        abp_error::ErrorCode::BackendNotFound.category(),
        abp_error::ErrorCategory::Backend
    );
    assert_eq!(
        abp_error::ErrorCode::PolicyDenied.category(),
        abp_error::ErrorCategory::Policy
    );
    assert_eq!(
        abp_error::ErrorCode::ProtocolInvalidEnvelope.category(),
        abp_error::ErrorCategory::Protocol
    );
    assert_eq!(
        abp_error::ErrorCode::Internal.category(),
        abp_error::ErrorCategory::Internal
    );
}

#[test]
fn error_code_serde_roundtrip() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
    let back: abp_error::ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn error_code_all_variants_serialize_to_snake_case() {
    let codes = [
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        abp_error::ErrorCode::ProtocolHandshakeFailed,
        abp_error::ErrorCode::ProtocolMissingRefId,
        abp_error::ErrorCode::ProtocolUnexpectedMessage,
        abp_error::ErrorCode::ProtocolVersionMismatch,
        abp_error::ErrorCode::MappingUnsupportedCapability,
        abp_error::ErrorCode::MappingDialectMismatch,
        abp_error::ErrorCode::MappingLossyConversion,
        abp_error::ErrorCode::MappingUnmappableTool,
        abp_error::ErrorCode::BackendNotFound,
        abp_error::ErrorCode::BackendUnavailable,
        abp_error::ErrorCode::BackendTimeout,
        abp_error::ErrorCode::BackendRateLimited,
        abp_error::ErrorCode::BackendAuthFailed,
        abp_error::ErrorCode::BackendModelNotFound,
        abp_error::ErrorCode::BackendCrashed,
        abp_error::ErrorCode::ExecutionToolFailed,
        abp_error::ErrorCode::ExecutionWorkspaceError,
        abp_error::ErrorCode::ExecutionPermissionDenied,
        abp_error::ErrorCode::ContractVersionMismatch,
        abp_error::ErrorCode::ContractSchemaViolation,
        abp_error::ErrorCode::ContractInvalidReceipt,
        abp_error::ErrorCode::CapabilityUnsupported,
        abp_error::ErrorCode::CapabilityEmulationFailed,
        abp_error::ErrorCode::PolicyDenied,
        abp_error::ErrorCode::PolicyInvalid,
        abp_error::ErrorCode::WorkspaceInitFailed,
        abp_error::ErrorCode::WorkspaceStagingFailed,
        abp_error::ErrorCode::IrLoweringFailed,
        abp_error::ErrorCode::IrInvalid,
        abp_error::ErrorCode::ReceiptHashMismatch,
        abp_error::ErrorCode::ReceiptChainBroken,
        abp_error::ErrorCode::DialectUnknown,
        abp_error::ErrorCode::DialectMappingFailed,
        abp_error::ErrorCode::ConfigInvalid,
        abp_error::ErrorCode::Internal,
    ];
    for code in &codes {
        let json = serde_json::to_string(code).unwrap();
        let as_str = code.as_str();
        assert_eq!(json, format!("\"{as_str}\""));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Supporting types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_identity_roundtrip() {
    let id = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    };
    let json = serde_json::to_string(&id).unwrap();
    let back: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "sidecar:node");
    assert_eq!(back.backend_version.as_deref(), Some("1.0.0"));
    assert!(back.adapter_version.is_none());
}

#[test]
fn run_metadata_roundtrip() {
    let now = Utc::now();
    let meta = RunMetadata {
        run_id: Uuid::new_v4(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: now,
        finished_at: now,
        duration_ms: 42,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.contract_version, CONTRACT_VERSION);
    assert_eq!(back.duration_ms, 42);
}

#[test]
fn usage_normalized_default() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn usage_normalized_roundtrip() {
    let u = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(3),
        estimated_cost_usd: Some(0.05),
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, Some(1000));
    assert_eq!(back.output_tokens, Some(500));
    assert_eq!(back.estimated_cost_usd, Some(0.05));
}

#[test]
fn artifact_ref_roundtrip() {
    let a = ArtifactRef {
        kind: "log".into(),
        path: "run.log".into(),
    };
    let json = serde_json::to_string(&a).unwrap();
    let back: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.kind, "log");
    assert_eq!(back.path, "run.log");
}

#[test]
fn verification_report_default() {
    let v = VerificationReport::default();
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
    assert!(!v.harness_ok);
}

#[test]
fn verification_report_roundtrip() {
    let v = VerificationReport {
        git_diff: Some("---\n+++".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: VerificationReport = serde_json::from_str(&json).unwrap();
    assert!(back.harness_ok);
    assert!(back.git_diff.is_some());
}

#[test]
fn context_packet_default() {
    let c = ContextPacket::default();
    assert!(c.files.is_empty());
    assert!(c.snippets.is_empty());
}

#[test]
fn context_snippet_roundtrip() {
    let s = ContextSnippet {
        name: "hint".into(),
        content: "use crate::foo;".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "hint");
    assert_eq!(back.content, "use crate::foo;");
}

#[test]
fn workspace_spec_roundtrip() {
    let ws = WorkspaceSpec {
        root: "/tmp".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string(&ws).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.root, "/tmp");
    assert_eq!(back.include, vec!["src/**"]);
}

#[test]
fn runtime_config_default() {
    let c = RuntimeConfig::default();
    assert!(c.model.is_none());
    assert!(c.vendor.is_empty());
    assert!(c.env.is_empty());
    assert!(c.max_budget_usd.is_none());
    assert!(c.max_turns.is_none());
}

#[test]
fn runtime_config_roundtrip() {
    let mut vendor = BTreeMap::new();
    vendor.insert("key".to_string(), json!("value"));
    let mut env = BTreeMap::new();
    env.insert("PATH".to_string(), "/usr/bin".to_string());

    let c = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        env,
        max_budget_usd: Some(5.0),
        max_turns: Some(10),
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model.as_deref(), Some("gpt-4"));
    assert_eq!(back.max_turns, Some(10));
    assert!(back.vendor.contains_key("key"));
    assert!(back.env.contains_key("PATH"));
}

#[test]
fn policy_profile_default() {
    let p = PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

#[test]
fn policy_profile_roundtrip() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["write_file".into()],
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(back.allowed_tools, vec!["read"]);
    assert_eq!(back.disallowed_tools, vec!["bash"]);
    assert_eq!(back.require_approval_for, vec!["write_file"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. ContractError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_error_display() {
    let err =
        ContractError::Json(serde_json::from_str::<serde_json::Value>("not json").unwrap_err());
    let msg = format!("{err}");
    assert!(msg.contains("serialize") || msg.contains("JSON") || msg.contains("json"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Full WorkOrder JSON structure
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_json_contains_all_top_level_fields() {
    let wo = WorkOrderBuilder::new("test").build();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("id"));
    assert!(obj.contains_key("task"));
    assert!(obj.contains_key("lane"));
    assert!(obj.contains_key("workspace"));
    assert!(obj.contains_key("context"));
    assert!(obj.contains_key("policy"));
    assert!(obj.contains_key("requirements"));
    assert!(obj.contains_key("config"));
}

#[test]
fn receipt_json_contains_all_top_level_fields() {
    let r = ReceiptBuilder::new("mock").build();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("meta"));
    assert!(obj.contains_key("backend"));
    assert!(obj.contains_key("capabilities"));
    assert!(obj.contains_key("mode"));
    assert!(obj.contains_key("usage_raw"));
    assert!(obj.contains_key("usage"));
    assert!(obj.contains_key("trace"));
    assert!(obj.contains_key("artifacts"));
    assert!(obj.contains_key("verification"));
    assert!(obj.contains_key("outcome"));
    assert!(obj.contains_key("receipt_sha256"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_task_work_order() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn unicode_task_work_order() {
    let wo = WorkOrderBuilder::new("修复登录模块 🔧").build();
    assert_eq!(wo.task, "修复登录模块 🔧");
}

#[test]
fn receipt_builder_chain() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .backend_version("1.0")
        .adapter_version("0.1")
        .mode(ExecutionMode::Passthrough)
        .build()
        .with_hash()
        .unwrap();
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn min_support_roundtrip() {
    for ms in [MinSupport::Native, MinSupport::Emulated] {
        let json = serde_json::to_string(&ms).unwrap();
        let back: MinSupport = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());
    }
}

#[test]
fn agent_event_all_kinds_roundtrip() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta { text: "tok".into() },
        AgentEventKind::AssistantMessage { text: "msg".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: None,
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];
    for kind in &kinds {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: kind.clone(),
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        // Verify type tag is present
        assert!(json.contains(r#""type":"#));
        // Verify roundtrip produces valid json
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
        let _ = serde_json::to_string(&back).unwrap();
    }
}
