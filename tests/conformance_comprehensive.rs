#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive conformance test suite validating ABP contract invariants
//! across all SDK types and dialect mappings.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirements, ContextPacket, ContextSnippet, ContractError,
    ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, canonical_json, receipt_hash, sha256_hex,
};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, ClaudeToOpenAiMapper, CodexClaudeIrMapper,
    DialectRequest, DialectResponse, GeminiKimiIrMapper, GeminiToOpenAiMapper, IdentityMapper,
    IrIdentityMapper, IrMapper, MapError, Mapper, MappingError, OpenAiClaudeIrMapper,
    OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
    OpenAiToClaudeMapper, OpenAiToGeminiMapper, default_ir_mapper, supported_ir_pairs,
};
use abp_mapping::{Fidelity, MappingRegistry, MappingRule};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: CONTRACT_VERSION consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_format() {
    assert!(CONTRACT_VERSION.starts_with("abp/v"));
}

#[test]
fn contract_version_is_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_parseable() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}

#[test]
fn contract_version_self_compatible() {
    assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
}

#[test]
fn contract_version_embedded_in_receipt() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn contract_version_embedded_in_hello_envelope() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, CONTRACT_VERSION),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn openai_dialect_version_format() {
    assert!(abp_openai_sdk::dialect::DIALECT_VERSION.starts_with("openai/"));
}

#[test]
fn claude_dialect_version_format() {
    assert!(abp_claude_sdk::dialect::DIALECT_VERSION.starts_with("claude/"));
}

#[test]
fn gemini_dialect_version_format() {
    assert!(abp_gemini_sdk::dialect::DIALECT_VERSION.starts_with("gemini/"));
}

#[test]
fn codex_dialect_version_format() {
    assert!(abp_codex_sdk::dialect::DIALECT_VERSION.starts_with("codex/"));
}

#[test]
fn kimi_dialect_version_format() {
    assert!(abp_kimi_sdk::dialect::DIALECT_VERSION.starts_with("kimi/"));
}

#[test]
fn copilot_dialect_version_format() {
    assert!(abp_copilot_sdk::dialect::DIALECT_VERSION.starts_with("copilot/"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: Receipt hash determinism
// ═══════════════════════════════════════════════════════════════════════════

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

#[test]
fn receipt_hash_is_deterministic() {
    let r = make_receipt("mock");
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_length_is_64() {
    let r = make_receipt("mock");
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_with_hash_fills_sha256() {
    let r = make_receipt("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_ignores_existing_hash_field() {
    let mut r = make_receipt("mock");
    let h1 = receipt_hash(&r).unwrap();
    r.receipt_sha256 = Some("garbage".into());
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "receipt_hash must ignore the receipt_sha256 field");
}

#[test]
fn receipt_hash_differs_for_different_backends() {
    let r1 = ReceiptBuilder::new("backend-a")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("backend-b")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_differs_for_different_outcomes() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    // Work order IDs differ (uuid v4), so hashes will differ.
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = sha256_hex(b"hello world");
    let h2 = sha256_hex(b"hello world");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn sha256_hex_differs_for_different_input() {
    let h1 = sha256_hex(b"hello");
    let h2 = sha256_hex(b"world");
    assert_ne!(h1, h2);
}

#[test]
fn canonical_json_sorts_keys() {
    let j = canonical_json(&json!({"z": 1, "a": 2})).unwrap();
    assert!(j.starts_with(r#"{"a":2"#));
}

#[test]
fn canonical_json_deterministic() {
    let v = json!({"b": 2, "a": 1, "c": [3, 2, 1]});
    let j1 = canonical_json(&v).unwrap();
    let j2 = canonical_json(&v).unwrap();
    assert_eq!(j1, j2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Serde tag format consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_uses_t_tag() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = serde_json::to_string(&hello).unwrap();
    assert!(
        json.contains(r#""t":"hello""#),
        "Envelope must use \"t\" tag"
    );
}

#[test]
fn envelope_fatal_uses_t_tag() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&fatal).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn agent_event_kind_uses_type_tag() {
    let event = AgentEventKind::AssistantMessage { text: "hi".into() };
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        json.contains(r#""type":"assistant_message""#),
        "AgentEventKind must use \"type\" tag"
    );
}

#[test]
fn agent_event_kind_run_started_tag() {
    let event = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"run_started""#));
}

#[test]
fn agent_event_kind_run_completed_tag() {
    let event = AgentEventKind::RunCompleted {
        message: "done".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"run_completed""#));
}

#[test]
fn agent_event_kind_tool_call_tag() {
    let event = AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"tool_call""#));
}

#[test]
fn agent_event_kind_tool_result_tag() {
    let event = AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: None,
        output: json!("ok"),
        is_error: false,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"tool_result""#));
}

#[test]
fn agent_event_kind_file_changed_tag() {
    let event = AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "modified".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"file_changed""#));
}

#[test]
fn agent_event_kind_command_executed_tag() {
    let event = AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(0),
        output_preview: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"command_executed""#));
}

#[test]
fn agent_event_kind_warning_tag() {
    let event = AgentEventKind::Warning {
        message: "caution".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"warning""#));
}

#[test]
fn agent_event_kind_error_tag() {
    let event = AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"error""#));
}

#[test]
fn agent_event_kind_assistant_delta_tag() {
    let event = AgentEventKind::AssistantDelta { text: "tok".into() };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"assistant_delta""#));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: AgentEventKind roundtrip through serde
// ═══════════════════════════════════════════════════════════════════════════

fn roundtrip_event_kind(kind: &AgentEventKind) {
    let json = serde_json::to_string(kind).unwrap();
    let back: AgentEventKind = serde_json::from_str(&json).unwrap();
    // Compare JSON to avoid non-PartialEq issues
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn serde_roundtrip_run_started() {
    roundtrip_event_kind(&AgentEventKind::RunStarted {
        message: "starting".into(),
    });
}

#[test]
fn serde_roundtrip_run_completed() {
    roundtrip_event_kind(&AgentEventKind::RunCompleted {
        message: "done".into(),
    });
}

#[test]
fn serde_roundtrip_assistant_delta() {
    roundtrip_event_kind(&AgentEventKind::AssistantDelta { text: "tok".into() });
}

#[test]
fn serde_roundtrip_assistant_message() {
    roundtrip_event_kind(&AgentEventKind::AssistantMessage {
        text: "Hello!".into(),
    });
}

#[test]
fn serde_roundtrip_tool_call() {
    roundtrip_event_kind(&AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc_1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    });
}

#[test]
fn serde_roundtrip_tool_result() {
    roundtrip_event_kind(&AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc_1".into()),
        output: json!("file contents"),
        is_error: false,
    });
}

#[test]
fn serde_roundtrip_file_changed() {
    roundtrip_event_kind(&AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    });
}

#[test]
fn serde_roundtrip_command_executed() {
    roundtrip_event_kind(&AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("all passed".into()),
    });
}

#[test]
fn serde_roundtrip_warning() {
    roundtrip_event_kind(&AgentEventKind::Warning {
        message: "deprecated API".into(),
    });
}

#[test]
fn serde_roundtrip_error() {
    roundtrip_event_kind(&AgentEventKind::Error {
        message: "timeout".into(),
        error_code: None,
    });
}

#[test]
fn serde_roundtrip_agent_event_full() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn serde_roundtrip_agent_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), json!("value"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "partial".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(back.ext.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Envelope (protocol) serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_hello_roundtrip() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    let back = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(back, Envelope::Hello { .. }));
}

#[test]
fn envelope_fatal_roundtrip() {
    let fatal = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "crash".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    let back = JsonlCodec::decode(json.trim()).unwrap();
    match back {
        Envelope::Fatal { error, .. } => assert_eq!(error, "crash"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_encode_ends_with_newline() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    assert!(json.ends_with('\n'));
}

#[test]
fn envelope_decode_invalid_json_is_error() {
    let result = JsonlCodec::decode("not json");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: WorkOrder construction and serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_basic() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn work_order_builder_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4o").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn work_order_builder_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(5).build();
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn work_order_builder_max_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(1.5).build();
    assert_eq!(wo.config.max_budget_usd, Some(1.5));
}

#[test]
fn work_order_builder_lane() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn work_order_serde_roundtrip() {
    let wo = WorkOrderBuilder::new("test task")
        .model("gpt-4o")
        .max_turns(10)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "test task");
    assert_eq!(back.config.model.as_deref(), Some("gpt-4o"));
    assert_eq!(back.config.max_turns, Some(10));
}

#[test]
fn work_order_has_uuid_id() {
    let wo = WorkOrderBuilder::new("task").build();
    assert_ne!(wo.id, Uuid::nil());
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: Capability manifest completeness per SDK
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_manifest_has_streaming() {
    let m = abp_openai_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn openai_manifest_has_tool_read() {
    let m = abp_openai_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::ToolRead));
}

#[test]
fn openai_manifest_has_structured_output() {
    let m = abp_openai_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn claude_manifest_has_streaming() {
    let m = abp_claude_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn claude_manifest_has_native_tool_read() {
    let m = abp_claude_sdk::dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn claude_manifest_has_mcp_client() {
    let m = abp_claude_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::McpClient));
}

#[test]
fn gemini_manifest_has_streaming() {
    let m = abp_gemini_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn gemini_manifest_has_structured_output() {
    let m = abp_gemini_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn codex_manifest_has_streaming() {
    let m = abp_codex_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn codex_manifest_has_tool_bash() {
    let m = abp_codex_sdk::dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolBash),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn kimi_manifest_has_streaming() {
    let m = abp_kimi_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn kimi_manifest_has_web_search() {
    let m = abp_kimi_sdk::dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn copilot_manifest_has_streaming() {
    let m = abp_copilot_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn copilot_manifest_has_web_search() {
    let m = abp_copilot_sdk::dialect::capability_manifest();
    assert!(m.contains_key(&Capability::ToolWebSearch));
}

#[test]
fn all_sdk_manifests_are_nonempty() {
    let manifests: Vec<CapabilityManifest> = vec![
        abp_openai_sdk::dialect::capability_manifest(),
        abp_claude_sdk::dialect::capability_manifest(),
        abp_gemini_sdk::dialect::capability_manifest(),
        abp_codex_sdk::dialect::capability_manifest(),
        abp_kimi_sdk::dialect::capability_manifest(),
        abp_copilot_sdk::dialect::capability_manifest(),
    ];
    for m in &manifests {
        assert!(!m.is_empty(), "Capability manifest must not be empty");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: Model name mapping (canonical ↔ vendor)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_canonical_model() {
    assert_eq!(
        abp_openai_sdk::dialect::to_canonical_model("gpt-4o"),
        "openai/gpt-4o"
    );
}

#[test]
fn openai_from_canonical_model() {
    assert_eq!(
        abp_openai_sdk::dialect::from_canonical_model("openai/gpt-4o"),
        "gpt-4o"
    );
}

#[test]
fn openai_from_canonical_passthrough() {
    assert_eq!(
        abp_openai_sdk::dialect::from_canonical_model("gpt-4o"),
        "gpt-4o"
    );
}

#[test]
fn openai_known_models() {
    assert!(abp_openai_sdk::dialect::is_known_model("gpt-4o"));
    assert!(abp_openai_sdk::dialect::is_known_model("gpt-4o-mini"));
    assert!(!abp_openai_sdk::dialect::is_known_model("unknown-model"));
}

#[test]
fn claude_to_canonical_model() {
    assert_eq!(
        abp_claude_sdk::dialect::to_canonical_model("claude-sonnet-4-20250514"),
        "anthropic/claude-sonnet-4-20250514"
    );
}

#[test]
fn claude_from_canonical_model() {
    assert_eq!(
        abp_claude_sdk::dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn claude_known_models() {
    assert!(abp_claude_sdk::dialect::is_known_model(
        "claude-sonnet-4-20250514"
    ));
    assert!(!abp_claude_sdk::dialect::is_known_model("unknown"));
}

#[test]
fn gemini_to_canonical_model() {
    assert_eq!(
        abp_gemini_sdk::dialect::to_canonical_model("gemini-2.5-flash"),
        "google/gemini-2.5-flash"
    );
}

#[test]
fn gemini_from_canonical_model() {
    assert_eq!(
        abp_gemini_sdk::dialect::from_canonical_model("google/gemini-2.5-flash"),
        "gemini-2.5-flash"
    );
}

#[test]
fn gemini_known_models() {
    assert!(abp_gemini_sdk::dialect::is_known_model("gemini-2.5-flash"));
    assert!(abp_gemini_sdk::dialect::is_known_model("gemini-2.5-pro"));
    assert!(!abp_gemini_sdk::dialect::is_known_model("unknown"));
}

#[test]
fn codex_to_canonical_model() {
    assert_eq!(
        abp_codex_sdk::dialect::to_canonical_model("codex-mini-latest"),
        "openai/codex-mini-latest"
    );
}

#[test]
fn codex_from_canonical_model() {
    assert_eq!(
        abp_codex_sdk::dialect::from_canonical_model("openai/codex-mini-latest"),
        "codex-mini-latest"
    );
}

#[test]
fn codex_known_models() {
    assert!(abp_codex_sdk::dialect::is_known_model("codex-mini-latest"));
    assert!(!abp_codex_sdk::dialect::is_known_model("unknown"));
}

#[test]
fn kimi_to_canonical_model() {
    assert_eq!(
        abp_kimi_sdk::dialect::to_canonical_model("moonshot-v1-8k"),
        "moonshot/moonshot-v1-8k"
    );
}

#[test]
fn kimi_from_canonical_model() {
    assert_eq!(
        abp_kimi_sdk::dialect::from_canonical_model("moonshot/moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
}

#[test]
fn kimi_known_models() {
    assert!(abp_kimi_sdk::dialect::is_known_model("moonshot-v1-8k"));
    assert!(abp_kimi_sdk::dialect::is_known_model("k1"));
    assert!(!abp_kimi_sdk::dialect::is_known_model("unknown"));
}

#[test]
fn copilot_to_canonical_model() {
    assert_eq!(
        abp_copilot_sdk::dialect::to_canonical_model("gpt-4o"),
        "copilot/gpt-4o"
    );
}

#[test]
fn copilot_from_canonical_model() {
    assert_eq!(
        abp_copilot_sdk::dialect::from_canonical_model("copilot/gpt-4o"),
        "gpt-4o"
    );
}

#[test]
fn copilot_known_models() {
    assert!(abp_copilot_sdk::dialect::is_known_model("gpt-4o"));
    assert!(abp_copilot_sdk::dialect::is_known_model("claude-sonnet-4"));
    assert!(!abp_copilot_sdk::dialect::is_known_model("unknown"));
}

#[test]
fn model_canonical_roundtrip_openai() {
    let vendor = "gpt-4o";
    let canonical = abp_openai_sdk::dialect::to_canonical_model(vendor);
    let back = abp_openai_sdk::dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_canonical_roundtrip_claude() {
    let vendor = "claude-sonnet-4-20250514";
    let canonical = abp_claude_sdk::dialect::to_canonical_model(vendor);
    let back = abp_claude_sdk::dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_canonical_roundtrip_gemini() {
    let vendor = "gemini-2.5-flash";
    let canonical = abp_gemini_sdk::dialect::to_canonical_model(vendor);
    let back = abp_gemini_sdk::dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_canonical_roundtrip_codex() {
    let vendor = "codex-mini-latest";
    let canonical = abp_codex_sdk::dialect::to_canonical_model(vendor);
    let back = abp_codex_sdk::dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_canonical_roundtrip_kimi() {
    let vendor = "moonshot-v1-8k";
    let canonical = abp_kimi_sdk::dialect::to_canonical_model(vendor);
    let back = abp_kimi_sdk::dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_canonical_roundtrip_copilot() {
    let vendor = "gpt-4o";
    let canonical = abp_copilot_sdk::dialect::to_canonical_model(vendor);
    let back = abp_copilot_sdk::dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: IR types roundtrip through serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_roundtrip() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn ir_content_block_text_roundtrip() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_content_block_tool_use_roundtrip() {
    let block = IrContentBlock::ToolUse {
        id: "tu1".into(),
        name: "read_file".into(),
        input: json!({"path": "src/main.rs"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_content_block_tool_result_roundtrip() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu1".into(),
        content: vec![IrContentBlock::Text {
            text: "contents".into(),
        }],
        is_error: false,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_content_block_image_roundtrip() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_content_block_thinking_roundtrip() {
    let block = IrContentBlock::Thinking {
        text: "let me think...".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn ir_message_text_helper() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(msg.role, IrRole::User);
    assert!(msg.is_text_only());
    assert_eq!(msg.text_content(), "hello");
}

#[test]
fn ir_conversation_roundtrip() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful"))
        .push(IrMessage::text(IrRole::User, "Hello"));
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn ir_conversation_helpers() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "hi"));
    assert_eq!(conv.len(), 3);
    assert!(!conv.is_empty());
    assert!(conv.system_message().is_some());
    assert!(conv.last_assistant().is_some());
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::from_io(200, 100);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 150);
    assert_eq!(merged.total_tokens, 450);
}

#[test]
fn ir_tool_definition_roundtrip() {
    let tool = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: SDK lowering roundtrips (to_ir → from_ir)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_lowering_text_roundtrip() {
    use abp_openai_sdk::dialect::OpenAIMessage;
    use abp_openai_sdk::lowering;

    let messages = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("You are helpful".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = lowering::to_ir(&messages);
    assert_eq!(ir.len(), 2);
    let back = lowering::from_ir(&ir);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[1].content.as_deref(), Some("Hello"));
}

#[test]
fn claude_lowering_text_roundtrip() {
    use abp_claude_sdk::dialect::ClaudeMessage;
    use abp_claude_sdk::lowering;

    let messages = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello Claude".into(),
    }];
    let ir = lowering::to_ir(&messages, Some("You are helpful"));
    assert_eq!(ir.len(), 2); // system + user
    let back = lowering::from_ir(&ir);
    // from_ir skips system messages
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert!(back[0].content.contains("Hello Claude"));
}

#[test]
fn gemini_lowering_text_roundtrip() {
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
    use abp_gemini_sdk::lowering;

    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hello Gemini".into())],
    }];
    let ir = lowering::to_ir(&contents, None);
    assert_eq!(ir.len(), 1);
    let back = lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello Gemini"),
        _ => panic!("expected text"),
    }
}

#[test]
fn codex_lowering_input_roundtrip() {
    use abp_codex_sdk::dialect::CodexInputItem;
    use abp_codex_sdk::lowering;

    let items = vec![CodexInputItem::Message {
        role: "user".into(),
        content: "Hello Codex".into(),
    }];
    let ir = lowering::input_to_ir(&items);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "Hello Codex");
}

#[test]
fn kimi_lowering_text_roundtrip() {
    use abp_kimi_sdk::dialect::KimiMessage;
    use abp_kimi_sdk::lowering;

    let messages = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello Kimi".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = lowering::to_ir(&messages);
    assert_eq!(ir.len(), 1);
    let back = lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("Hello Kimi"));
}

#[test]
fn copilot_lowering_text_roundtrip() {
    use abp_copilot_sdk::dialect::CopilotMessage;
    use abp_copilot_sdk::lowering;

    let messages = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello Copilot".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = lowering::to_ir(&messages);
    assert_eq!(ir.len(), 1);
    let back = lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello Copilot");
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 11: Cross-dialect IR mapping
// ═══════════════════════════════════════════════════════════════════════════

fn make_simple_ir() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there"))
}

#[test]
fn ir_identity_mapper_roundtrip() {
    let mapper = IrIdentityMapper;
    let conv = make_simple_ir();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped, conv);
}

#[test]
fn ir_openai_claude_mapper_request() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &conv);
    assert!(result.is_ok());
    let mapped = result.unwrap();
    assert!(!mapped.is_empty());
}

#[test]
fn ir_claude_openai_mapper_request() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Claude, Dialect::OpenAi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_openai_gemini_mapper_request() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Gemini, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_gemini_openai_mapper_request() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Gemini, Dialect::OpenAi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_claude_gemini_mapper_request() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_gemini_claude_mapper_request() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Gemini, Dialect::Claude, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_openai_codex_mapper_request() {
    let mapper = OpenAiCodexIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Codex, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_codex_openai_mapper_request() {
    let mapper = OpenAiCodexIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Codex, Dialect::OpenAi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_openai_kimi_mapper_request() {
    let mapper = OpenAiKimiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Kimi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_kimi_openai_mapper_request() {
    let mapper = OpenAiKimiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Kimi, Dialect::OpenAi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_claude_kimi_mapper_request() {
    let mapper = ClaudeKimiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Claude, Dialect::Kimi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_kimi_claude_mapper_request() {
    let mapper = ClaudeKimiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Kimi, Dialect::Claude, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_openai_copilot_mapper_request() {
    let mapper = OpenAiCopilotIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Copilot, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_copilot_openai_mapper_request() {
    let mapper = OpenAiCopilotIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Copilot, Dialect::OpenAi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_gemini_kimi_mapper_request() {
    let mapper = GeminiKimiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_kimi_gemini_mapper_request() {
    let mapper = GeminiKimiIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Kimi, Dialect::Gemini, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_codex_claude_mapper_request() {
    let mapper = CodexClaudeIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
    assert!(result.is_ok());
}

#[test]
fn ir_claude_codex_mapper_request() {
    let mapper = CodexClaudeIrMapper;
    let conv = make_simple_ir();
    let result = mapper.map_request(Dialect::Claude, Dialect::Codex, &conv);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 12: Mapper factory coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn factory_returns_identity_for_same_dialect() {
    for d in Dialect::all() {
        let mapper = default_ir_mapper(*d, *d);
        assert!(
            mapper.is_some(),
            "factory must return a mapper for {d} -> {d}"
        );
    }
}

#[test]
fn factory_returns_mapper_for_all_supported_pairs() {
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to);
        assert!(
            mapper.is_some(),
            "factory must return mapper for {from} -> {to}"
        );
    }
}

#[test]
fn factory_supported_pairs_includes_identity() {
    let pairs = supported_ir_pairs();
    for d in Dialect::all() {
        assert!(
            pairs.contains(&(*d, *d)),
            "supported_ir_pairs must include ({d}, {d})"
        );
    }
}

#[test]
fn factory_supported_pairs_includes_cross_dialect() {
    let pairs = supported_ir_pairs();
    // Check major cross-dialect pairs
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
    assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
}

#[test]
fn factory_all_mappers_can_map_simple_request() {
    let conv = make_simple_ir();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv);
        assert!(
            result.is_ok(),
            "mapping {from} -> {to} failed: {:?}",
            result.err()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 13: Dialect enum coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_six() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_all_contains_expected() {
    let all = Dialect::all();
    assert!(all.contains(&Dialect::OpenAi));
    assert!(all.contains(&Dialect::Claude));
    assert!(all.contains(&Dialect::Gemini));
    assert!(all.contains(&Dialect::Codex));
    assert!(all.contains(&Dialect::Kimi));
    assert!(all.contains(&Dialect::Copilot));
}

#[test]
fn dialect_labels_are_nonempty() {
    for d in Dialect::all() {
        assert!(!d.label().is_empty(), "{d:?} has empty label");
    }
}

#[test]
fn dialect_serde_roundtrip() {
    for d in Dialect::all() {
        let json = serde_json::to_string(d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(*d, back);
    }
}

#[test]
fn dialect_display_matches_label() {
    for d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 14: SDK backend names
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_backend_name() {
    assert_eq!(abp_openai_sdk::BACKEND_NAME, "sidecar:openai");
}

#[test]
fn claude_backend_name() {
    assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
}

#[test]
fn gemini_backend_name() {
    assert_eq!(abp_gemini_sdk::BACKEND_NAME, "sidecar:gemini");
}

#[test]
fn codex_backend_name() {
    assert_eq!(abp_codex_sdk::BACKEND_NAME, "sidecar:codex");
}

#[test]
fn kimi_backend_name() {
    assert_eq!(abp_kimi_sdk::BACKEND_NAME, "sidecar:kimi");
}

#[test]
fn copilot_backend_name() {
    assert_eq!(abp_copilot_sdk::BACKEND_NAME, "sidecar:copilot");
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 15: WorkOrder mapping from each SDK
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_map_work_order_produces_request() {
    let wo = WorkOrderBuilder::new("Refactor auth")
        .model("gpt-4o")
        .build();
    let cfg = abp_openai_sdk::dialect::OpenAIConfig::default();
    let req = abp_openai_sdk::dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4o");
    assert!(!req.messages.is_empty());
}

#[test]
fn claude_map_work_order_produces_request() {
    let wo = WorkOrderBuilder::new("Fix bug").build();
    let cfg = abp_claude_sdk::dialect::ClaudeConfig::default();
    let req = abp_claude_sdk::dialect::map_work_order(&wo, &cfg);
    assert!(!req.messages.is_empty());
    assert!(req.messages[0].content.contains("Fix bug"));
}

#[test]
fn gemini_map_work_order_produces_request() {
    let wo = WorkOrderBuilder::new("Add tests").build();
    let cfg = abp_gemini_sdk::dialect::GeminiConfig::default();
    let req = abp_gemini_sdk::dialect::map_work_order(&wo, &cfg);
    assert!(!req.contents.is_empty());
}

#[test]
fn codex_map_work_order_produces_request() {
    let wo = WorkOrderBuilder::new("Generate code").build();
    let cfg = abp_codex_sdk::dialect::CodexConfig::default();
    let req = abp_codex_sdk::dialect::map_work_order(&wo, &cfg);
    assert!(!req.input.is_empty());
}

#[test]
fn kimi_map_work_order_produces_request() {
    let wo = WorkOrderBuilder::new("Search topic").build();
    let cfg = abp_kimi_sdk::dialect::KimiConfig::default();
    let req = abp_kimi_sdk::dialect::map_work_order(&wo, &cfg);
    assert!(!req.messages.is_empty());
}

#[test]
fn copilot_map_work_order_produces_request() {
    let wo = WorkOrderBuilder::new("Help me").build();
    let cfg = abp_copilot_sdk::dialect::CopilotConfig::default();
    let req = abp_copilot_sdk::dialect::map_work_order(&wo, &cfg);
    assert!(!req.messages.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 16: Tool def roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_tool_def_roundtrip() {
    use abp_openai_sdk::dialect::*;
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let openai = tool_def_to_openai(&canonical);
    let back = tool_def_from_openai(&openai);
    assert_eq!(canonical, back);
}

#[test]
fn claude_tool_def_roundtrip() {
    use abp_claude_sdk::dialect::*;
    let canonical = CanonicalToolDef {
        name: "write_file".into(),
        description: "Write a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let claude = tool_def_to_claude(&canonical);
    let back = tool_def_from_claude(&claude);
    assert_eq!(canonical, back);
}

#[test]
fn gemini_tool_def_roundtrip() {
    use abp_gemini_sdk::dialect::*;
    let canonical = CanonicalToolDef {
        name: "search".into(),
        description: "Search".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let gemini = tool_def_to_gemini(&canonical);
    let back = tool_def_from_gemini(&gemini);
    assert_eq!(canonical, back);
}

#[test]
fn codex_tool_def_roundtrip() {
    use abp_codex_sdk::dialect::*;
    let canonical = CanonicalToolDef {
        name: "execute".into(),
        description: "Execute code".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let codex = tool_def_to_codex(&canonical);
    let back = tool_def_from_codex(&codex);
    assert_eq!(canonical, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 17: Mapping validation types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lossless_is_lossless() {
    assert!(Fidelity::Lossless.is_lossless());
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
fn fidelity_lossy_is_neither() {
    let f = Fidelity::LossyLabeled {
        warning: "some loss".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn mapping_registry_insert_and_lookup() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    assert_eq!(reg.len(), 1);
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
    assert!(rule.is_some());
}

#[test]
fn mapping_error_display() {
    let err = abp_mapping::MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    assert!(err.to_string().contains("logprobs"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 18: MapError types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_error_unsupported_pair_roundtrip() {
    let err = MapError::UnsupportedPair {
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_lossy_conversion_roundtrip() {
    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "target has no thinking block".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_unmappable_content_roundtrip() {
    let err = MapError::UnmappableContent {
        field: "image".into(),
        reason: "no image support".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 19: Execution mode, outcome, and support level serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}

#[test]
fn outcome_serde_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, back);
    }
}

#[test]
fn support_level_satisfies_logic() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 20: Protocol version negotiation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn version_compatibility_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
}

#[test]
fn version_incompatibility_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 21: IR mapping preserves semantics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapping_preserves_message_count_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi"));
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped.len(), conv.len());
}

#[test]
fn ir_mapping_preserves_text_content_identity() {
    let mapper = IrIdentityMapper;
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "exact text"));
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped.messages[0].text_content(), "exact text");
}

#[test]
fn ir_mapping_preserves_tool_calls_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "x.rs"}),
        }],
    ));
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert!(
        !mapped.tool_calls().is_empty(),
        "tool calls must be preserved"
    );
}

#[test]
fn ir_identity_preserves_system_message() {
    let mapper = IrIdentityMapper;
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are a helper"))
        .push(IrMessage::text(IrRole::User, "Hi"));
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert!(mapped.system_message().is_some());
    assert_eq!(
        mapped.system_message().unwrap().text_content(),
        "You are a helper"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 22: Receipt builder completeness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_sets_backend_id() {
    let r = ReceiptBuilder::new("my-backend").build();
    assert_eq!(r.backend.id, "my-backend");
}

#[test]
fn receipt_builder_sets_outcome() {
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_sets_mode() {
    let r = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_builder_sets_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("mock").capabilities(caps).build();
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn receipt_builder_with_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_builder_contract_version() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}
