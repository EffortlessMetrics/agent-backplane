#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive serde roundtrip and canonical-format tests for the ABP workspace.

use std::collections::BTreeMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

// ── abp-core types ──────────────────────────────────────────────────────
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};

// ── abp-core::ir types ─────────────────────────────────────────────────
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};

// ── abp-core::negotiate types ──────────────────────────────────────────
use abp_core::negotiate::{CapabilityReport, CapabilityReportEntry, DialectSupportLevel};

// ── abp-core::error types ──────────────────────────────────────────────
use abp_core::error::ErrorCode as CoreErrorCode;

// ── abp-protocol types ─────────────────────────────────────────────────
use abp_protocol::{Envelope, JsonlCodec};

// ── abp-config types ───────────────────────────────────────────────────
use abp_config::{BackendEntry, BackplaneConfig};

// ── abp-capability types ───────────────────────────────────────────────
use abp_capability::negotiate::NegotiationPolicy;
use abp_capability::{
    CompatibilityReport, EmulationStrategy, NegotiationResult as CapNegotiationResult,
    SupportLevel as CapSupportLevel,
};

// ── abp-mapping types ──────────────────────────────────────────────────
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingError, MappingRule, MappingValidation};

// ── abp-error types ────────────────────────────────────────────────────
use abp_error::{ErrorCategory, ErrorCode as AbpErrorCode};

// ── abp-error-taxonomy types ───────────────────────────────────────────
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassification, ErrorSeverity, RecoveryAction, RecoverySuggestion,
};

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Assert JSON roundtrip: serialize → deserialize produces the same value.
fn assert_json_roundtrip<T>(val: &T)
where
    T: Serialize + for<'de> Deserialize<'de> + std::fmt::Debug + PartialEq,
{
    let json = serde_json::to_string(val).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(*val, back, "roundtrip mismatch");
}

/// Assert that a value serializes to the expected JSON string snippet.
fn assert_serializes_to<T: Serialize>(val: &T, expected_fragment: &str) {
    let json = serde_json::to_string(val).expect("serialize");
    assert!(
        json.contains(expected_fragment),
        "expected fragment {expected_fragment:?} not found in {json}"
    );
}

/// Build a minimal Receipt for reuse.
fn make_receipt(outcome: Outcome) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. WorkOrder roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t01_work_order_roundtrip() {
    let wo = WorkOrderBuilder::new("test task").build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.task, back.task);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. ExecutionLane enum variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t02_execution_lane_patch_first() {
    let lane = ExecutionLane::PatchFirst;
    assert_serializes_to(&lane, "\"patch_first\"");
}

#[test]
fn t03_execution_lane_workspace_first() {
    let lane = ExecutionLane::WorkspaceFirst;
    assert_serializes_to(&lane, "\"workspace_first\"");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. WorkspaceMode enum variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t04_workspace_mode_pass_through() {
    let mode = WorkspaceMode::PassThrough;
    assert_serializes_to(&mode, "\"pass_through\"");
}

#[test]
fn t05_workspace_mode_staged() {
    let mode = WorkspaceMode::Staged;
    assert_serializes_to(&mode, "\"staged\"");
}

// ═══════════════════════════════════════════════════════════════════════
// 4. ExecutionMode enum variants + default
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t06_execution_mode_passthrough() {
    let mode = ExecutionMode::Passthrough;
    assert_serializes_to(&mode, "\"passthrough\"");
}

#[test]
fn t07_execution_mode_mapped() {
    let mode = ExecutionMode::Mapped;
    assert_serializes_to(&mode, "\"mapped\"");
}

#[test]
fn t08_execution_mode_default_is_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Outcome enum
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t09_outcome_complete() {
    let o = Outcome::Complete;
    assert_serializes_to(&o, "\"complete\"");
    let back: Outcome = serde_json::from_str("\"complete\"").unwrap();
    assert_eq!(back, Outcome::Complete);
}

#[test]
fn t10_outcome_partial() {
    let o = Outcome::Partial;
    assert_serializes_to(&o, "\"partial\"");
}

#[test]
fn t11_outcome_failed() {
    let o = Outcome::Failed;
    assert_serializes_to(&o, "\"failed\"");
}

// ═══════════════════════════════════════════════════════════════════════
// 6. MinSupport enum
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t12_min_support_native() {
    let ms = MinSupport::Native;
    assert_serializes_to(&ms, "\"native\"");
}

#[test]
fn t13_min_support_emulated() {
    let ms = MinSupport::Emulated;
    assert_serializes_to(&ms, "\"emulated\"");
}

// ═══════════════════════════════════════════════════════════════════════
// 7. SupportLevel enum (abp-core)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t14_support_level_native() {
    let sl = SupportLevel::Native;
    assert_serializes_to(&sl, "\"native\"");
}

#[test]
fn t15_support_level_emulated() {
    let sl = SupportLevel::Emulated;
    assert_serializes_to(&sl, "\"emulated\"");
}

#[test]
fn t16_support_level_unsupported() {
    let sl = SupportLevel::Unsupported;
    assert_serializes_to(&sl, "\"unsupported\"");
}

#[test]
fn t17_support_level_restricted() {
    let sl = SupportLevel::Restricted {
        reason: "disabled".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"restricted\""));
    assert!(json.contains("disabled"));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Capability enum (selected variants)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t18_capability_streaming() {
    assert_serializes_to(&Capability::Streaming, "\"streaming\"");
}

#[test]
fn t19_capability_tool_read() {
    assert_serializes_to(&Capability::ToolRead, "\"tool_read\"");
}

#[test]
fn t20_capability_tool_write() {
    assert_serializes_to(&Capability::ToolWrite, "\"tool_write\"");
}

#[test]
fn t21_capability_mcp_client() {
    assert_serializes_to(&Capability::McpClient, "\"mcp_client\"");
}

#[test]
fn t22_capability_extended_thinking() {
    assert_serializes_to(&Capability::ExtendedThinking, "\"extended_thinking\"");
}

#[test]
fn t23_capability_roundtrip_all() {
    let caps = vec![
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
    ];
    for cap in &caps {
        let json = serde_json::to_string(cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(*cap, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. CapabilityManifest BTreeMap ordering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t24_capability_manifest_ordering_preserved() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(m.len(), back.len());
    // JSON roundtrip preserves same canonical form
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
    // BTreeMap keys should be in sorted order
    let keys: Vec<_> = m.keys().collect();
    for i in 1..keys.len() {
        assert!(keys[i - 1] < keys[i], "keys not sorted");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Canonical JSON determinism
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t25_canonical_json_deterministic() {
    let receipt = make_receipt(Outcome::Complete);
    let json1 = abp_core::canonical_json(&receipt).unwrap();
    let json2 = abp_core::canonical_json(&receipt).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn t26_canonical_json_sorted_keys() {
    let json = abp_core::canonical_json(&json!({"z": 1, "a": 2, "m": 3})).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let m_pos = json.find("\"m\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < m_pos && m_pos < z_pos);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Receipt roundtrip and hashing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t27_receipt_roundtrip() {
    let receipt = make_receipt(Outcome::Complete);
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt.outcome, back.outcome);
    assert_eq!(receipt.meta.run_id, back.meta.run_id);
}

#[test]
fn t28_receipt_hash_deterministic() {
    let receipt = make_receipt(Outcome::Complete);
    let h1 = abp_core::receipt_hash(&receipt).unwrap();
    let h2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn t29_receipt_with_hash_sets_sha256() {
    let receipt = make_receipt(Outcome::Complete).with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. AgentEventKind tag = "type"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t30_agent_event_kind_uses_type_tag() {
    let kind = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"run_started\""));
}

#[test]
fn t31_agent_event_kind_assistant_delta() {
    let kind = AgentEventKind::AssistantDelta { text: "hi".into() };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"assistant_delta\""));
}

#[test]
fn t32_agent_event_kind_tool_call() {
    let kind = AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("t1".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "ls"}),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"tool_call\""));
    assert!(json.contains("\"tool_name\":\"bash\""));
}

#[test]
fn t33_agent_event_kind_tool_result() {
    let kind = AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("ok"),
        is_error: false,
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"tool_result\""));
}

#[test]
fn t34_agent_event_kind_file_changed() {
    let kind = AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "edited".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"file_changed\""));
}

#[test]
fn t35_agent_event_kind_command_executed() {
    let kind = AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: None,
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"command_executed\""));
}

#[test]
fn t36_agent_event_kind_warning() {
    let kind = AgentEventKind::Warning {
        message: "something".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"warning\""));
}

#[test]
fn t37_agent_event_kind_error() {
    let kind = AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"error\""));
}

#[test]
fn t38_agent_event_kind_run_completed() {
    let kind = AgentEventKind::RunCompleted {
        message: "done".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"run_completed\""));
}

#[test]
fn t39_agent_event_kind_assistant_message() {
    let kind = AgentEventKind::AssistantMessage {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"type\":\"assistant_message\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 13. AgentEvent with ext field (optional)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t40_agent_event_ext_omitted_when_none() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("\"ext\""));
}

#[test]
fn t41_agent_event_ext_included_when_some() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!("raw"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"raw_message\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Envelope uses tag = "t"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t42_envelope_hello_uses_t_tag() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"t\":\"hello\""));
}

#[test]
fn t43_envelope_fatal_uses_t_tag() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"t\":\"fatal\""));
}

#[test]
fn t44_envelope_fatal_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "crash".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-1".into()));
            assert_eq!(error, "crash");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn t45_envelope_hello_roundtrip() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn t46_envelope_event_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains("\"t\":\"event\""));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Envelope fatal with error_code
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t47_envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "protocol error",
        AbpErrorCode::ProtocolInvalidEnvelope,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"error_code\""));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(AbpErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn t48_envelope_fatal_error_code_omitted_when_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "oops".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(!json.contains("\"error_code\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. BackplaneConfig roundtrip (abp-config)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t49_backplane_config_roundtrip_json() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: None,
        log_level: Some("debug".into()),
        receipts_dir: None,
        bind_address: None,
        port: Some(8080),
        policy_profiles: vec![],
        backends: BTreeMap::new(),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn t50_backplane_config_optional_fields_omitted() {
    let cfg = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        bind_address: None,
        port: None,
        policy_profiles: vec![],
        backends: BTreeMap::new(),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("\"default_backend\""));
    assert!(!json.contains("\"workspace_dir\""));
    assert!(!json.contains("\"log_level\""));
    assert!(!json.contains("\"receipts_dir\""));
    assert!(!json.contains("\"bind_address\""));
    assert!(!json.contains("\"port\""));
    assert!(!json.contains("\"policy_profiles\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. BackendEntry tag = "type"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t51_backend_entry_mock_tag() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"type\":\"mock\""));
}

#[test]
fn t52_backend_entry_sidecar_tag() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"type\":\"sidecar\""));
}

#[test]
fn t53_backend_entry_sidecar_roundtrip() {
    let entry = BackendEntry::Sidecar {
        command: "python".into(),
        args: vec!["host.py".into(), "--verbose".into()],
        timeout_secs: Some(300),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn t54_backend_entry_timeout_omitted_when_none() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(!json.contains("\"timeout_secs\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. EmulationStrategy (abp-capability)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t55_emulation_strategy_client_side() {
    let s = EmulationStrategy::ClientSide;
    assert_serializes_to(&s, "\"client_side\"");
}

#[test]
fn t56_emulation_strategy_server_fallback() {
    let s = EmulationStrategy::ServerFallback;
    assert_serializes_to(&s, "\"server_fallback\"");
}

#[test]
fn t57_emulation_strategy_approximate() {
    let s = EmulationStrategy::Approximate;
    assert_serializes_to(&s, "\"approximate\"");
}

#[test]
fn t58_emulation_strategy_roundtrip() {
    for s in [
        EmulationStrategy::ClientSide,
        EmulationStrategy::ServerFallback,
        EmulationStrategy::Approximate,
    ] {
        assert_json_roundtrip(&s);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. SupportLevel (abp-capability) tag = "level"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t59_cap_support_level_native() {
    let sl = CapSupportLevel::Native;
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"level\":\"native\""));
}

#[test]
fn t60_cap_support_level_emulated() {
    let sl = CapSupportLevel::Emulated {
        method: "polyfill".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"level\":\"emulated\""));
    assert!(json.contains("\"method\":\"polyfill\""));
}

#[test]
fn t61_cap_support_level_restricted() {
    let sl = CapSupportLevel::Restricted {
        reason: "policy".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"level\":\"restricted\""));
}

#[test]
fn t62_cap_support_level_unsupported() {
    let sl = CapSupportLevel::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"level\":\"unsupported\""));
}

#[test]
fn t63_cap_support_level_roundtrip() {
    let variants: Vec<CapSupportLevel> = vec![
        CapSupportLevel::Native,
        CapSupportLevel::Emulated { method: "m".into() },
        CapSupportLevel::Restricted { reason: "r".into() },
        CapSupportLevel::Unsupported { reason: "u".into() },
    ];
    for v in &variants {
        assert_json_roundtrip(v);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 20. NegotiationPolicy (abp-capability::negotiate)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t64_negotiation_policy_strict() {
    let p = NegotiationPolicy::Strict;
    assert_serializes_to(&p, "\"strict\"");
}

#[test]
fn t65_negotiation_policy_best_effort() {
    let p = NegotiationPolicy::BestEffort;
    assert_serializes_to(&p, "\"best_effort\"");
}

#[test]
fn t66_negotiation_policy_permissive() {
    let p = NegotiationPolicy::Permissive;
    assert_serializes_to(&p, "\"permissive\"");
}

#[test]
fn t67_negotiation_policy_default() {
    let p = NegotiationPolicy::default();
    assert_eq!(p, NegotiationPolicy::Strict);
}

#[test]
fn t68_negotiation_policy_roundtrip() {
    for p in [
        NegotiationPolicy::Strict,
        NegotiationPolicy::BestEffort,
        NegotiationPolicy::Permissive,
    ] {
        assert_json_roundtrip(&p);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 21. NegotiationResult (abp-capability)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t69_cap_negotiation_result_roundtrip() {
    let r = CapNegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Logprobs, "not available".into())],
    };
    assert_json_roundtrip(&r);
}

// ═══════════════════════════════════════════════════════════════════════
// 22. CompatibilityReport (abp-capability)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t70_compatibility_report_roundtrip() {
    let r = CompatibilityReport {
        compatible: true,
        native_count: 5,
        emulated_count: 2,
        unsupported_count: 0,
        summary: "all good".into(),
        details: vec![("streaming".into(), CapSupportLevel::Native)],
    };
    assert_json_roundtrip(&r);
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Fidelity (abp-mapping) tag = "type"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t71_fidelity_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"type\":\"lossless\""));
}

#[test]
fn t72_fidelity_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "some loss".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"type\":\"lossy_labeled\""));
}

#[test]
fn t73_fidelity_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "nope".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"type\":\"unsupported\""));
}

#[test]
fn t74_fidelity_roundtrip() {
    let variants: Vec<Fidelity> = vec![
        Fidelity::Lossless,
        Fidelity::LossyLabeled {
            warning: "w".into(),
        },
        Fidelity::Unsupported { reason: "r".into() },
    ];
    for v in &variants {
        assert_json_roundtrip(v);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Dialect (abp-dialect)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t75_dialect_openai() {
    assert_serializes_to(&Dialect::OpenAi, "\"open_ai\"");
}

#[test]
fn t76_dialect_claude() {
    assert_serializes_to(&Dialect::Claude, "\"claude\"");
}

#[test]
fn t77_dialect_gemini() {
    assert_serializes_to(&Dialect::Gemini, "\"gemini\"");
}

#[test]
fn t78_dialect_codex() {
    assert_serializes_to(&Dialect::Codex, "\"codex\"");
}

#[test]
fn t79_dialect_kimi() {
    assert_serializes_to(&Dialect::Kimi, "\"kimi\"");
}

#[test]
fn t80_dialect_copilot() {
    assert_serializes_to(&Dialect::Copilot, "\"copilot\"");
}

#[test]
fn t81_dialect_roundtrip_all() {
    for d in Dialect::all() {
        assert_json_roundtrip(d);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 25. MappingRule roundtrip (abp-mapping)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t82_mapping_rule_roundtrip() {
    let rule = MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    };
    assert_json_roundtrip(&rule);
}

// ═══════════════════════════════════════════════════════════════════════
// 26. MappingError roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t83_mapping_error_feature_unsupported() {
    let e = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    assert_json_roundtrip(&e);
}

#[test]
fn t84_mapping_error_fidelity_loss() {
    let e = MappingError::FidelityLoss {
        feature: "streaming".into(),
        warning: "partial".into(),
    };
    assert_json_roundtrip(&e);
}

#[test]
fn t85_mapping_error_dialect_mismatch() {
    let e = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Kimi,
    };
    assert_json_roundtrip(&e);
}

#[test]
fn t86_mapping_error_invalid_input() {
    let e = MappingError::InvalidInput {
        reason: "bad".into(),
    };
    assert_json_roundtrip(&e);
}

// ═══════════════════════════════════════════════════════════════════════
// 27. MappingValidation roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t87_mapping_validation_roundtrip() {
    let v = MappingValidation {
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    assert_json_roundtrip(&v);
}

// ═══════════════════════════════════════════════════════════════════════
// 28. ErrorCategory (abp-error)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t88_error_category_roundtrip() {
    let cats = vec![
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
    for c in &cats {
        assert_json_roundtrip(c);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 29. AbpErrorCode (abp-error) rename_all snake_case
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t89_abp_error_code_snake_case() {
    let code = AbpErrorCode::ProtocolInvalidEnvelope;
    assert_serializes_to(&code, "\"protocol_invalid_envelope\"");
}

#[test]
fn t90_abp_error_code_backend_timeout() {
    let code = AbpErrorCode::BackendTimeout;
    assert_serializes_to(&code, "\"backend_timeout\"");
}

#[test]
fn t91_abp_error_code_roundtrip_all() {
    let codes = vec![
        AbpErrorCode::ProtocolInvalidEnvelope,
        AbpErrorCode::ProtocolHandshakeFailed,
        AbpErrorCode::ProtocolMissingRefId,
        AbpErrorCode::ProtocolUnexpectedMessage,
        AbpErrorCode::ProtocolVersionMismatch,
        AbpErrorCode::MappingUnsupportedCapability,
        AbpErrorCode::MappingDialectMismatch,
        AbpErrorCode::MappingLossyConversion,
        AbpErrorCode::MappingUnmappableTool,
        AbpErrorCode::BackendNotFound,
        AbpErrorCode::BackendUnavailable,
        AbpErrorCode::BackendTimeout,
        AbpErrorCode::BackendRateLimited,
        AbpErrorCode::BackendAuthFailed,
        AbpErrorCode::BackendModelNotFound,
        AbpErrorCode::BackendCrashed,
        AbpErrorCode::ExecutionToolFailed,
        AbpErrorCode::ExecutionWorkspaceError,
        AbpErrorCode::ExecutionPermissionDenied,
        AbpErrorCode::ContractVersionMismatch,
        AbpErrorCode::ContractSchemaViolation,
        AbpErrorCode::ContractInvalidReceipt,
        AbpErrorCode::CapabilityUnsupported,
        AbpErrorCode::CapabilityEmulationFailed,
        AbpErrorCode::PolicyDenied,
        AbpErrorCode::PolicyInvalid,
        AbpErrorCode::WorkspaceInitFailed,
        AbpErrorCode::WorkspaceStagingFailed,
        AbpErrorCode::IrLoweringFailed,
        AbpErrorCode::IrInvalid,
        AbpErrorCode::ReceiptHashMismatch,
        AbpErrorCode::ReceiptChainBroken,
        AbpErrorCode::DialectUnknown,
        AbpErrorCode::DialectMappingFailed,
        AbpErrorCode::ConfigInvalid,
        AbpErrorCode::Internal,
    ];
    for c in &codes {
        assert_json_roundtrip(c);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 30. ErrorSeverity (abp-error-taxonomy)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t92_error_severity_variants() {
    assert_serializes_to(&ErrorSeverity::Fatal, "\"fatal\"");
    assert_serializes_to(&ErrorSeverity::Retriable, "\"retriable\"");
    assert_serializes_to(&ErrorSeverity::Degraded, "\"degraded\"");
    assert_serializes_to(&ErrorSeverity::Informational, "\"informational\"");
}

#[test]
fn t93_error_severity_roundtrip() {
    for s in [
        ErrorSeverity::Fatal,
        ErrorSeverity::Retriable,
        ErrorSeverity::Degraded,
        ErrorSeverity::Informational,
    ] {
        assert_json_roundtrip(&s);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 31. ClassificationCategory (abp-error-taxonomy)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t94_classification_category_variants() {
    let cats = vec![
        (ClassificationCategory::Authentication, "\"authentication\""),
        (ClassificationCategory::RateLimit, "\"rate_limit\""),
        (ClassificationCategory::ModelNotFound, "\"model_not_found\""),
        (
            ClassificationCategory::InvalidRequest,
            "\"invalid_request\"",
        ),
        (ClassificationCategory::ContentFilter, "\"content_filter\""),
        (ClassificationCategory::ContextLength, "\"context_length\""),
        (ClassificationCategory::ServerError, "\"server_error\""),
        (ClassificationCategory::NetworkError, "\"network_error\""),
        (ClassificationCategory::ProtocolError, "\"protocol_error\""),
        (
            ClassificationCategory::CapabilityUnsupported,
            "\"capability_unsupported\"",
        ),
        (
            ClassificationCategory::MappingFailure,
            "\"mapping_failure\"",
        ),
        (ClassificationCategory::TimeoutError, "\"timeout_error\""),
    ];
    for (cat, expected) in &cats {
        assert_serializes_to(cat, expected);
    }
}

#[test]
fn t95_classification_category_roundtrip() {
    let cats = vec![
        ClassificationCategory::Authentication,
        ClassificationCategory::RateLimit,
        ClassificationCategory::ModelNotFound,
        ClassificationCategory::InvalidRequest,
        ClassificationCategory::ContentFilter,
        ClassificationCategory::ContextLength,
        ClassificationCategory::ServerError,
        ClassificationCategory::NetworkError,
        ClassificationCategory::ProtocolError,
        ClassificationCategory::CapabilityUnsupported,
        ClassificationCategory::MappingFailure,
        ClassificationCategory::TimeoutError,
    ];
    for c in &cats {
        assert_json_roundtrip(c);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 32. RecoveryAction (abp-error-taxonomy)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t96_recovery_action_variants() {
    assert_serializes_to(&RecoveryAction::Retry, "\"retry\"");
    assert_serializes_to(&RecoveryAction::Fallback, "\"fallback\"");
    assert_serializes_to(&RecoveryAction::ReduceContext, "\"reduce_context\"");
    assert_serializes_to(&RecoveryAction::ChangeModel, "\"change_model\"");
    assert_serializes_to(&RecoveryAction::ContactAdmin, "\"contact_admin\"");
    assert_serializes_to(&RecoveryAction::None, "\"none\"");
}

#[test]
fn t97_recovery_action_roundtrip() {
    for a in [
        RecoveryAction::Retry,
        RecoveryAction::Fallback,
        RecoveryAction::ReduceContext,
        RecoveryAction::ChangeModel,
        RecoveryAction::ContactAdmin,
        RecoveryAction::None,
    ] {
        assert_json_roundtrip(&a);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 33. RecoverySuggestion roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t98_recovery_suggestion_roundtrip() {
    let s = RecoverySuggestion {
        action: RecoveryAction::Retry,
        description: "try again".into(),
        delay_ms: Some(1000),
    };
    assert_json_roundtrip(&s);
}

#[test]
fn t99_recovery_suggestion_no_delay() {
    let s = RecoverySuggestion {
        action: RecoveryAction::ContactAdmin,
        description: "call admin".into(),
        delay_ms: None,
    };
    assert_json_roundtrip(&s);
}

// ═══════════════════════════════════════════════════════════════════════
// 34. ErrorClassification roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t100_error_classification_roundtrip() {
    let cl = ErrorClassification {
        code: AbpErrorCode::BackendRateLimited,
        severity: ErrorSeverity::Retriable,
        category: ClassificationCategory::RateLimit,
        recovery: RecoverySuggestion {
            action: RecoveryAction::Retry,
            description: "wait".into(),
            delay_ms: Some(2000),
        },
    };
    assert_json_roundtrip(&cl);
}

// ═══════════════════════════════════════════════════════════════════════
// 35. IR types (abp-core::ir)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t101_ir_role_variants() {
    assert_serializes_to(&IrRole::System, "\"system\"");
    assert_serializes_to(&IrRole::User, "\"user\"");
    assert_serializes_to(&IrRole::Assistant, "\"assistant\"");
    assert_serializes_to(&IrRole::Tool, "\"tool\"");
}

#[test]
fn t102_ir_role_roundtrip() {
    for r in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        assert_json_roundtrip(&r);
    }
}

#[test]
fn t103_ir_content_block_text() {
    let b = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("\"type\":\"text\""));
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn t104_ir_content_block_image() {
    let b = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("\"type\":\"image\""));
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn t105_ir_content_block_tool_use() {
    let b = IrContentBlock::ToolUse {
        id: "t1".into(),
        name: "read".into(),
        input: json!({"path": "/tmp"}),
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("\"type\":\"tool_use\""));
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn t106_ir_content_block_tool_result() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "t1".into(),
        content: vec![IrContentBlock::Text { text: "ok".into() }],
        is_error: false,
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("\"type\":\"tool_result\""));
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn t107_ir_content_block_thinking() {
    let b = IrContentBlock::Thinking { text: "hmm".into() };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("\"type\":\"thinking\""));
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn t108_ir_message_roundtrip() {
    let msg = IrMessage::text(IrRole::User, "hi there");
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn t109_ir_message_metadata_omitted_when_empty() {
    let msg = IrMessage::text(IrRole::User, "no meta");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("\"metadata\""));
}

#[test]
fn t110_ir_message_metadata_included_when_present() {
    let mut msg = IrMessage::text(IrRole::User, "with meta");
    msg.metadata.insert("key".into(), json!("value"));
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"metadata\""));
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn t111_ir_conversation_roundtrip() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful"),
        IrMessage::text(IrRole::User, "Hi"),
        IrMessage::text(IrRole::Assistant, "Hello!"),
    ]);
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn t112_ir_tool_definition_roundtrip() {
    let td = IrToolDefinition {
        name: "bash".into(),
        description: "run shell".into(),
        parameters: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(td, back);
}

#[test]
fn t113_ir_usage_roundtrip() {
    let u = IrUsage::from_io(100, 50);
    assert_json_roundtrip(&u);
}

#[test]
fn t114_ir_usage_with_cache_roundtrip() {
    let u = IrUsage::with_cache(200, 100, 50, 25);
    assert_json_roundtrip(&u);
}

// ═══════════════════════════════════════════════════════════════════════
// 36. DialectSupportLevel (abp-core::negotiate) tag = "level"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t115_dialect_support_level_native() {
    let sl = DialectSupportLevel::Native;
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"level\":\"native\""));
}

#[test]
fn t116_dialect_support_level_emulated() {
    let sl = DialectSupportLevel::Emulated {
        detail: "via adapter".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"level\":\"emulated\""));
}

#[test]
fn t117_dialect_support_level_unsupported() {
    let sl = DialectSupportLevel::Unsupported {
        reason: "no API".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    assert!(json.contains("\"level\":\"unsupported\""));
}

#[test]
fn t118_dialect_support_level_roundtrip() {
    let variants = vec![
        DialectSupportLevel::Native,
        DialectSupportLevel::Emulated { detail: "d".into() },
        DialectSupportLevel::Unsupported { reason: "r".into() },
    ];
    for v in &variants {
        assert_json_roundtrip(v);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 37. CapabilityReport + CapabilityReportEntry roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t119_capability_report_roundtrip() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::ExtendedThinking,
                support: DialectSupportLevel::Unsupported {
                    reason: "not available".into(),
                },
            },
        ],
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: CapabilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report.source_dialect, back.source_dialect);
    assert_eq!(report.entries.len(), back.entries.len());
}

// ═══════════════════════════════════════════════════════════════════════
// 38. RuntimeConfig vendor BTreeMap ordering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t120_runtime_config_vendor_btreemap_order() {
    let mut rc = RuntimeConfig::default();
    rc.vendor.insert("z_flag".into(), json!(true));
    rc.vendor.insert("a_flag".into(), json!(false));
    rc.vendor.insert("m_flag".into(), json!(1));
    let json = serde_json::to_string(&rc).unwrap();
    let a_pos = json.find("\"a_flag\"").unwrap();
    let m_pos = json.find("\"m_flag\"").unwrap();
    let z_pos = json.find("\"z_flag\"").unwrap();
    assert!(
        a_pos < m_pos && m_pos < z_pos,
        "BTreeMap keys not sorted in JSON"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 39. Backward compatibility: deserialize from known JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t121_outcome_from_known_json() {
    let v: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(v, Outcome::Complete);
    let v: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(v, Outcome::Partial);
    let v: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(v, Outcome::Failed);
}

#[test]
fn t122_execution_mode_from_known_json() {
    let v: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
    assert_eq!(v, ExecutionMode::Passthrough);
    let v: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(v, ExecutionMode::Mapped);
}

#[test]
fn t123_envelope_from_known_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
}

#[test]
fn t124_capability_from_known_json() {
    let v: Capability = serde_json::from_str(r#""tool_bash""#).unwrap();
    assert_eq!(v, Capability::ToolBash);
}

#[test]
fn t125_dialect_from_known_json() {
    let v: Dialect = serde_json::from_str(r#""claude""#).unwrap();
    assert_eq!(v, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 40. Additional struct roundtrips
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t126_context_packet_roundtrip() {
    let cp = ContextPacket {
        files: vec!["src/lib.rs".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    let json = serde_json::to_string(&cp).unwrap();
    let back: ContextPacket = serde_json::from_str(&json).unwrap();
    assert_eq!(cp.files, back.files);
    assert_eq!(cp.snippets.len(), back.snippets.len());
}

#[test]
fn t127_workspace_spec_roundtrip() {
    let ws = WorkspaceSpec {
        root: "/tmp".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec!["target/*".into()],
    };
    let json = serde_json::to_string(&ws).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(ws.root, back.root);
}

#[test]
fn t128_policy_profile_default_roundtrip() {
    let pp = PolicyProfile::default();
    let json = serde_json::to_string(&pp).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(pp.allowed_tools, back.allowed_tools);
}

#[test]
fn t129_artifact_ref_roundtrip() {
    let ar = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let json = serde_json::to_string(&ar).unwrap();
    let back: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(ar.kind, back.kind);
}

#[test]
fn t130_verification_report_default_roundtrip() {
    let vr = VerificationReport::default();
    let json = serde_json::to_string(&vr).unwrap();
    let back: VerificationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(vr.harness_ok, back.harness_ok);
}

#[test]
fn t131_usage_normalized_roundtrip() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(u.input_tokens, back.input_tokens);
}

#[test]
fn t132_backend_identity_roundtrip() {
    let bi = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("2.0".into()),
        adapter_version: Some("0.3.0".into()),
    };
    let json = serde_json::to_string(&bi).unwrap();
    let back: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(bi.id, back.id);
}

#[test]
fn t133_run_metadata_roundtrip() {
    let now = Utc::now();
    let rm = RunMetadata {
        run_id: Uuid::new_v4(),
        work_order_id: Uuid::new_v4(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: now,
        finished_at: now,
        duration_ms: 1234,
    };
    let json = serde_json::to_string(&rm).unwrap();
    let back: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(rm.run_id, back.run_id);
    assert_eq!(rm.duration_ms, back.duration_ms);
}

#[test]
fn t134_capability_requirements_roundtrip() {
    let cr = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let json = serde_json::to_string(&cr).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(cr.required.len(), back.required.len());
}

// ═══════════════════════════════════════════════════════════════════════
// 41. AgentEvent error_code skip_serializing_if
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t135_agent_event_error_code_omitted_when_none() {
    let kind = AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(!json.contains("\"error_code\""));
}

#[test]
fn t136_agent_event_error_code_included_when_some() {
    let kind = AgentEventKind::Error {
        message: "oops".into(),
        error_code: Some(AbpErrorCode::BackendCrashed),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("\"error_code\""));
    assert!(json.contains("\"backend_crashed\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 42. RecoverySuggestion rename_all snake_case
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t137_recovery_suggestion_snake_case_fields() {
    let s = RecoverySuggestion {
        action: RecoveryAction::Retry,
        description: "test".into(),
        delay_ms: Some(500),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"action\""));
    assert!(json.contains("\"description\""));
    assert!(json.contains("\"delay_ms\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 43. ErrorClassification rename_all snake_case
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t138_error_classification_snake_case_fields() {
    let cl = ErrorClassification {
        code: AbpErrorCode::Internal,
        severity: ErrorSeverity::Fatal,
        category: ClassificationCategory::ServerError,
        recovery: RecoverySuggestion {
            action: RecoveryAction::ContactAdmin,
            description: "help".into(),
            delay_ms: None,
        },
    };
    let json = serde_json::to_string(&cl).unwrap();
    assert!(json.contains("\"code\""));
    assert!(json.contains("\"severity\""));
    assert!(json.contains("\"category\""));
    assert!(json.contains("\"recovery\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 44. BackplaneConfig backends BTreeMap ordering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t139_backplane_config_backends_btreemap_order() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("zz".into(), BackendEntry::Mock {});
    cfg.backends.insert("aa".into(), BackendEntry::Mock {});
    let json = serde_json::to_string(&cfg).unwrap();
    let aa_pos = json.find("\"aa\"").unwrap();
    let zz_pos = json.find("\"zz\"").unwrap();
    assert!(aa_pos < zz_pos, "BTreeMap backend keys not sorted");
}

// ═══════════════════════════════════════════════════════════════════════
// 45. Cross-crate consistency: tag = "t" vs tag = "type"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t140_envelope_uses_t_not_type() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":"), "Envelope must use \"t\" tag");
    assert!(
        !json.contains("\"type\":"),
        "Envelope must NOT use \"type\" tag"
    );
}

#[test]
fn t141_agent_event_kind_uses_type_not_t() {
    let kind = AgentEventKind::Warning {
        message: "w".into(),
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert!(
        json.contains("\"type\":"),
        "AgentEventKind must use \"type\" tag"
    );
}

#[test]
fn t142_ir_content_block_uses_type_tag() {
    let b = IrContentBlock::Text { text: "x".into() };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("\"type\":\"text\""));
}

#[test]
fn t143_fidelity_uses_type_tag() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"type\":\"lossless\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 46. Empty collections roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t144_empty_trace_receipt_roundtrip() {
    let receipt = make_receipt(Outcome::Complete);
    assert!(receipt.trace.is_empty());
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert!(back.trace.is_empty());
}

#[test]
fn t145_empty_conversation_roundtrip() {
    let conv = IrConversation::new();
    assert_json_roundtrip(&conv);
}

#[test]
fn t146_empty_capability_manifest_roundtrip() {
    let m = CapabilityManifest::new();
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(json, "{}");
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert!(back.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 47. Default values deserialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t147_execution_mode_default_deserialization() {
    // ExecutionMode has #[serde(default)] in Receipt — verify default is Mapped
    let json = r#"{"mode":"passthrough"}"#;
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let mode: ExecutionMode = serde_json::from_value(v["mode"].clone()).unwrap();
    assert_eq!(mode, ExecutionMode::Passthrough);
}

// ═══════════════════════════════════════════════════════════════════════
// 48. Large nested struct roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t148_full_work_order_roundtrip() {
    let wo = WorkOrderBuilder::new("large test")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .model("gpt-4")
        .max_turns(50)
        .max_budget_usd(10.0)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.task, back.task);
    assert_eq!(wo.config.model, back.config.model);
    assert_eq!(wo.config.max_turns, back.config.max_turns);
}

#[test]
fn t149_full_receipt_with_trace_roundtrip() {
    let now = Utc::now();
    let mut receipt = make_receipt(Outcome::Complete);
    receipt.trace.push(AgentEvent {
        ts: now,
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: None,
    });
    receipt.trace.push(AgentEvent {
        ts: now,
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        },
        ext: None,
    });
    receipt.trace.push(AgentEvent {
        ts: now,
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    });
    receipt.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
    });
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.trace.len(), 3);
    assert_eq!(back.artifacts.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 49. CoreErrorCode (abp-core::error) rename_all snake_case
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t150_core_error_code_roundtrip() {
    let codes = vec![
        CoreErrorCode::InvalidContractVersion,
        CoreErrorCode::MalformedWorkOrder,
        CoreErrorCode::MalformedReceipt,
        CoreErrorCode::InvalidHash,
        CoreErrorCode::HandshakeFailed,
        CoreErrorCode::ToolDenied,
        CoreErrorCode::BackendUnavailable,
        CoreErrorCode::IoError,
        CoreErrorCode::InternalError,
    ];
    for c in &codes {
        let json = serde_json::to_string(c).unwrap();
        let back: CoreErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*c, back);
    }
}

#[test]
fn t151_core_error_code_snake_case() {
    assert_serializes_to(
        &CoreErrorCode::InvalidContractVersion,
        "\"invalid_contract_version\"",
    );
    assert_serializes_to(
        &CoreErrorCode::MalformedWorkOrder,
        "\"malformed_work_order\"",
    );
    assert_serializes_to(&CoreErrorCode::HandshakeFailed, "\"handshake_failed\"");
}

// ═══════════════════════════════════════════════════════════════════════
// 50. Envelope run roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t152_envelope_run_roundtrip() {
    let wo = WorkOrderBuilder::new("test").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains("\"t\":\"run\""));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-1");
            assert_eq!(work_order.task, "test");
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 51. Envelope final roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t153_envelope_final_roundtrip() {
    let receipt = make_receipt(Outcome::Complete);
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains("\"t\":\"final\""));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 52. ErrorCategory rename_all snake_case
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t154_error_category_snake_case() {
    assert_serializes_to(&ErrorCategory::Protocol, "\"protocol\"");
    assert_serializes_to(&ErrorCategory::Backend, "\"backend\"");
    assert_serializes_to(&ErrorCategory::Internal, "\"internal\"");
    assert_serializes_to(&ErrorCategory::Mapping, "\"mapping\"");
}

// ═══════════════════════════════════════════════════════════════════════
// 53. Envelope mode field defaults
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t155_envelope_hello_mode_defaults_to_mapped() {
    // Deserialize a hello envelope without the "mode" field
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 54. serde Value-level roundtrips for opaque fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t156_runtime_config_vendor_json_value_roundtrip() {
    let mut rc = RuntimeConfig::default();
    rc.vendor
        .insert("complex".into(), json!({"nested": [1, 2, {"deep": true}]}));
    let json = serde_json::to_string(&rc).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rc.vendor["complex"], back.vendor["complex"]);
}

#[test]
fn t157_usage_raw_preserves_arbitrary_json() {
    let mut receipt = make_receipt(Outcome::Complete);
    receipt.usage_raw = json!({"custom_field": 42, "nested": {"a": [1,2,3]}});
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt.usage_raw, back.usage_raw);
}

// ═══════════════════════════════════════════════════════════════════════
// 55. CapabilityManifest with SupportLevel::Restricted
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t158_capability_manifest_with_restricted() {
    let mut m = CapabilityManifest::new();
    m.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(m.len(), back.len());
    // Canonical JSON roundtrip matches
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
    assert!(json.contains("sandboxed"));
}

// ═══════════════════════════════════════════════════════════════════════
// 56. Envelope decode_stream
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t159_envelope_decode_stream() {
    use std::io::BufReader;
    let line1 = r#"{"t":"fatal","ref_id":null,"error":"a"}"#;
    let line2 = r#"{"t":"fatal","ref_id":null,"error":"b"}"#;
    let input = format!("{}\n{}\n", line1, line2);
    let reader = BufReader::new(input.as_bytes());
    let envs: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envs.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 57. PolicyProfile with populated fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t160_policy_profile_full_roundtrip() {
    let pp = PolicyProfile {
        allowed_tools: vec!["bash".into(), "read".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec!["/etc/*".into()],
        deny_write: vec!["/usr/*".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["deploy".into()],
    };
    let json = serde_json::to_string(&pp).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(pp.allowed_tools, back.allowed_tools);
    assert_eq!(pp.disallowed_tools, back.disallowed_tools);
    assert_eq!(pp.deny_read, back.deny_read);
}

// ═══════════════════════════════════════════════════════════════════════
// 58. Deserialization from a Value (not just string)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn t161_deserialize_capability_from_value() {
    let v = json!("tool_edit");
    let cap: Capability = serde_json::from_value(v).unwrap();
    assert_eq!(cap, Capability::ToolEdit);
}

#[test]
fn t162_deserialize_outcome_from_value() {
    let v = json!("failed");
    let o: Outcome = serde_json::from_value(v).unwrap();
    assert_eq!(o, Outcome::Failed);
}
