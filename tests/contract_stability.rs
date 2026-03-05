// SPDX-License-Identifier: MIT OR Apache-2.0
//! Contract stability tests — verify that ABP contract types remain stable
//! and backward compatible across versions.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_protocol::Envelope;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fixed UUID for deterministic tests.
fn fixed_uuid() -> Uuid {
    Uuid::parse_str("01234567-89ab-cdef-0123-456789abcdef").unwrap()
}

/// Fixed timestamp for deterministic tests.
fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

/// Build a minimal, deterministic WorkOrder.
fn minimal_work_order() -> WorkOrder {
    WorkOrder {
        id: fixed_uuid(),
        task: "test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

/// Build a minimal, deterministic Receipt.
fn minimal_receipt() -> Receipt {
    let mut receipt = ReceiptBuilder::new("mock-backend")
        .work_order_id(fixed_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Complete)
        .build();
    // Pin the run_id for deterministic hashing tests
    receipt.meta.run_id = fixed_uuid();
    receipt
}

// ===========================================================================
// 1. Contract version tests
// ===========================================================================

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_in_receipt_metadata() {
    let receipt = minimal_receipt();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn contract_version_in_hello_envelope() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::default(),
    };
    let json = serde_json::to_value(&hello).unwrap();
    assert_eq!(json["contract_version"], CONTRACT_VERSION);
}

// ===========================================================================
// 2. Required fields — WorkOrder
// ===========================================================================

#[test]
fn work_order_requires_id() {
    let json = serde_json::to_value(minimal_work_order()).unwrap();
    assert!(json.get("id").is_some(), "WorkOrder must have 'id'");
}

#[test]
fn work_order_requires_task() {
    let json = serde_json::to_value(minimal_work_order()).unwrap();
    assert!(json.get("task").is_some(), "WorkOrder must have 'task'");
}

#[test]
fn work_order_requires_lane() {
    let json = serde_json::to_value(minimal_work_order()).unwrap();
    assert!(json.get("lane").is_some(), "WorkOrder must have 'lane'");
}

#[test]
fn work_order_all_required_fields_present() {
    let json = serde_json::to_value(minimal_work_order()).unwrap();
    let required = [
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ];
    for field in &required {
        assert!(
            json.get(field).is_some(),
            "WorkOrder missing required field: {field}"
        );
    }
}

// ===========================================================================
// 3. Required fields — Receipt
// ===========================================================================

#[test]
fn receipt_all_required_fields_present() {
    let json = serde_json::to_value(minimal_receipt()).unwrap();
    let required = [
        "meta",
        "backend",
        "capabilities",
        "mode",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
    ];
    for field in &required {
        assert!(
            json.get(field).is_some(),
            "Receipt missing required field: {field}"
        );
    }
}

// ===========================================================================
// 4. Optional fields don't break old serializations
// ===========================================================================

#[test]
fn receipt_optional_sha256_can_be_null() {
    let receipt = minimal_receipt();
    let json = serde_json::to_value(&receipt).unwrap();
    // receipt_sha256 is optional/null before hashing
    assert!(json.get("receipt_sha256").is_none() || json["receipt_sha256"].is_null());
}

#[test]
fn runtime_config_optional_fields_default_to_none() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn usage_normalized_all_optional_fields_default_to_none() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn old_receipt_without_optional_fields_still_deserializes() {
    // Simulate an old receipt JSON that doesn't have receipt_sha256
    let receipt = minimal_receipt();
    let mut json = serde_json::to_value(&receipt).unwrap();
    // Remove optional fields that might be added later
    json.as_object_mut().unwrap().remove("receipt_sha256");
    let deser: Receipt = serde_json::from_value(json).unwrap();
    assert!(deser.receipt_sha256.is_none());
}

// ===========================================================================
// 5. Enum stability — serialized strings
// ===========================================================================

#[test]
fn execution_lane_serializes_to_expected_strings() {
    assert_eq!(
        serde_json::to_value(ExecutionLane::PatchFirst).unwrap(),
        "patch_first"
    );
    assert_eq!(
        serde_json::to_value(ExecutionLane::WorkspaceFirst).unwrap(),
        "workspace_first"
    );
}

#[test]
fn workspace_mode_serializes_to_expected_strings() {
    assert_eq!(
        serde_json::to_value(WorkspaceMode::PassThrough).unwrap(),
        "pass_through"
    );
    assert_eq!(
        serde_json::to_value(WorkspaceMode::Staged).unwrap(),
        "staged"
    );
}

#[test]
fn execution_mode_serializes_to_expected_strings() {
    assert_eq!(
        serde_json::to_value(ExecutionMode::Passthrough).unwrap(),
        "passthrough"
    );
    assert_eq!(
        serde_json::to_value(ExecutionMode::Mapped).unwrap(),
        "mapped"
    );
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn outcome_serializes_to_expected_strings() {
    assert_eq!(serde_json::to_value(Outcome::Complete).unwrap(), "complete");
    assert_eq!(serde_json::to_value(Outcome::Partial).unwrap(), "partial");
    assert_eq!(serde_json::to_value(Outcome::Failed).unwrap(), "failed");
}

#[test]
fn support_level_native_and_emulated_serialize_correctly() {
    assert_eq!(
        serde_json::to_value(SupportLevel::Native).unwrap(),
        "native"
    );
    assert_eq!(
        serde_json::to_value(SupportLevel::Emulated).unwrap(),
        "emulated"
    );
    assert_eq!(
        serde_json::to_value(SupportLevel::Unsupported).unwrap(),
        "unsupported"
    );
}

#[test]
fn support_level_restricted_serializes_with_reason() {
    let json = serde_json::to_value(SupportLevel::Restricted {
        reason: "beta".into(),
    })
    .unwrap();
    assert_eq!(json["restricted"]["reason"], "beta");
}

#[test]
fn min_support_serializes_to_expected_strings() {
    assert_eq!(serde_json::to_value(MinSupport::Native).unwrap(), "native");
    assert_eq!(
        serde_json::to_value(MinSupport::Emulated).unwrap(),
        "emulated"
    );
    assert_eq!(serde_json::to_value(MinSupport::Any).unwrap(), "any");
}

#[test]
fn capability_subset_serializes_to_expected_strings() {
    let checks = [
        (Capability::Streaming, "streaming"),
        (Capability::ToolRead, "tool_read"),
        (Capability::ToolWrite, "tool_write"),
        (Capability::ToolEdit, "tool_edit"),
        (Capability::ToolBash, "tool_bash"),
        (Capability::ToolGlob, "tool_glob"),
        (Capability::ToolGrep, "tool_grep"),
        (Capability::ToolWebSearch, "tool_web_search"),
        (Capability::ToolWebFetch, "tool_web_fetch"),
        (Capability::ToolAskUser, "tool_ask_user"),
        (Capability::McpClient, "mcp_client"),
        (Capability::McpServer, "mcp_server"),
        (Capability::ToolUse, "tool_use"),
        (Capability::ExtendedThinking, "extended_thinking"),
        (Capability::ImageInput, "image_input"),
        (Capability::PdfInput, "pdf_input"),
        (Capability::CodeExecution, "code_execution"),
        (Capability::Logprobs, "logprobs"),
        (Capability::Vision, "vision"),
        (Capability::Audio, "audio"),
        (Capability::JsonMode, "json_mode"),
        (Capability::Temperature, "temperature"),
        (Capability::TopP, "top_p"),
        (Capability::TopK, "top_k"),
        (Capability::MaxTokens, "max_tokens"),
        (Capability::Embeddings, "embeddings"),
        (Capability::ImageGeneration, "image_generation"),
    ];
    for (cap, expected) in &checks {
        assert_eq!(
            serde_json::to_value(cap).unwrap(),
            *expected,
            "Capability::{cap:?} should serialize to \"{expected}\""
        );
    }
}

// ===========================================================================
// 6. Serde roundtrip / forward compatibility
// ===========================================================================

#[test]
fn work_order_serde_roundtrip() {
    let wo = minimal_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let deser: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.id, wo.id);
    assert_eq!(deser.task, wo.task);
}

#[test]
fn receipt_serde_roundtrip() {
    let receipt = minimal_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.meta.run_id, receipt.meta.run_id);
    assert_eq!(deser.outcome, receipt.outcome);
}

#[test]
fn work_order_with_extra_fields_still_deserializes() {
    // Simulate forward compat: JSON has unknown fields
    let wo = minimal_work_order();
    let mut json = serde_json::to_value(&wo).unwrap();
    json.as_object_mut()
        .unwrap()
        .insert("future_field".into(), serde_json::json!("v2_data"));
    // Should not fail — serde default behavior ignores unknown fields
    let _deser: WorkOrder = serde_json::from_value(json).unwrap();
}

#[test]
fn receipt_with_extra_fields_still_deserializes() {
    let receipt = minimal_receipt();
    let mut json = serde_json::to_value(&receipt).unwrap();
    json.as_object_mut()
        .unwrap()
        .insert("future_field".into(), serde_json::json!(42));
    let _deser: Receipt = serde_json::from_value(json).unwrap();
}

// ===========================================================================
// 7. Hash stability
// ===========================================================================

#[test]
fn receipt_hash_is_deterministic() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = minimal_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    assert!(r1.receipt_sha256.is_some());
}

#[test]
fn receipt_hash_nullifies_sha256_before_hashing() {
    let receipt = minimal_receipt().with_hash().unwrap();
    // Re-hash should produce the same value — proves hash field is excluded
    let rehashed = receipt.clone().with_hash().unwrap();
    assert_eq!(rehashed.receipt_sha256, receipt.receipt_sha256);
}

#[test]
fn receipt_hash_changes_when_outcome_differs() {
    let mut r1 = ReceiptBuilder::new("mock")
        .work_order_id(fixed_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Complete)
        .build();
    r1.meta.run_id = fixed_uuid();
    let r1 = r1.with_hash().unwrap();

    let mut r2 = ReceiptBuilder::new("mock")
        .work_order_id(fixed_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Failed)
        .build();
    r2.meta.run_id = fixed_uuid();
    let r2 = r2.with_hash().unwrap();

    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// 8. Protocol envelope stability
// ===========================================================================

#[test]
fn envelope_hello_uses_t_discriminator() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let json = serde_json::to_value(&hello).unwrap();
    assert_eq!(json["t"], "hello", "Envelope discriminator must be 't'");
}

#[test]
fn envelope_run_serializes_correctly() {
    let wo = minimal_work_order();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = serde_json::to_value(&run).unwrap();
    assert_eq!(json["t"], "run");
    assert_eq!(json["id"], "run-1");
    assert!(json.get("work_order").is_some());
}

#[test]
fn envelope_event_serializes_correctly() {
    let evt = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "hello".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: evt,
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "event");
    assert_eq!(json["ref_id"], "run-1");
}

#[test]
fn envelope_final_serializes_correctly() {
    let receipt = minimal_receipt();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "final");
    assert_eq!(json["ref_id"], "run-1");
    assert!(json.get("receipt").is_some());
}

#[test]
fn envelope_fatal_serializes_correctly() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "fatal");
    assert_eq!(json["error"], "boom");
}

#[test]
fn all_envelope_variants_roundtrip() {
    let variants: Vec<Envelope> = vec![
        Envelope::Hello {
            contract_version: CONTRACT_VERSION.into(),
            backend: BackendIdentity {
                id: "b".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: BTreeMap::new(),
            mode: ExecutionMode::Mapped,
        },
        Envelope::Run {
            id: "r".into(),
            work_order: minimal_work_order(),
        },
        Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "r".into(),
            receipt: minimal_receipt(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "err".into(),
            error_code: None,
        },
    ];
    for env in &variants {
        let json = serde_json::to_string(env).unwrap();
        let _deser: Envelope = serde_json::from_str(&json).unwrap();
    }
}

// ===========================================================================
// 9. AgentEvent stability
// ===========================================================================

#[test]
fn agent_event_kind_run_started_serializes_correctly() {
    let evt = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };
    let json = serde_json::to_value(&evt).unwrap();
    assert_eq!(json["type"], "run_started");
    assert_eq!(json["message"], "starting");
}

#[test]
fn agent_event_kind_tool_call_serializes_correctly() {
    let evt = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "foo.rs"}),
        },
        ext: None,
    };
    let json = serde_json::to_value(&evt).unwrap();
    assert_eq!(json["type"], "tool_call");
    assert_eq!(json["tool_name"], "read_file");
    assert_eq!(json["tool_use_id"], "tu_1");
}

#[test]
fn agent_event_kind_tool_result_serializes_correctly() {
    let evt = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            output: serde_json::json!("file contents"),
            is_error: false,
        },
        ext: None,
    };
    let json = serde_json::to_value(&evt).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert!(!json["is_error"].as_bool().unwrap());
}

#[test]
fn agent_event_all_kinds_roundtrip() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta { text: "tok".into() },
        AgentEventKind::AssistantMessage {
            text: "full".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!("ok"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "a.rs".into(),
            summary: "edited".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        },
    ];
    for kind in kinds {
        let evt = AgentEvent {
            ts: fixed_ts(),
            kind,
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let deser: AgentEvent = serde_json::from_str(&json).unwrap();
        // Verify the type tag survived roundtrip
        let v1 = serde_json::to_value(&evt).unwrap();
        let v2 = serde_json::to_value(&deser).unwrap();
        assert_eq!(v1["type"], v2["type"]);
    }
}

#[test]
fn agent_event_kind_type_tags_are_snake_case() {
    let expected_tags = [
        (
            AgentEventKind::RunStarted {
                message: String::new(),
            },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted {
                message: String::new(),
            },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta {
                text: String::new(),
            },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage {
                text: String::new(),
            },
            "assistant_message",
        ),
        (
            AgentEventKind::FileChanged {
                path: String::new(),
                summary: String::new(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: String::new(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (
            AgentEventKind::Warning {
                message: String::new(),
            },
            "warning",
        ),
        (
            AgentEventKind::Error {
                message: String::new(),
                error_code: None,
            },
            "error",
        ),
    ];
    for (kind, expected_tag) in &expected_tags {
        let evt = AgentEvent {
            ts: fixed_ts(),
            kind: kind.clone(),
            ext: None,
        };
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(
            json["type"], *expected_tag,
            "Expected type tag '{expected_tag}'"
        );
    }
}

// ===========================================================================
// 10. Schema shape — snapshot-style checks
// ===========================================================================

#[test]
fn work_order_schema_has_expected_top_level_keys() {
    let json = serde_json::to_value(minimal_work_order()).unwrap();
    let obj = json.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    assert!(keys.contains(&&"id".to_string()));
    assert!(keys.contains(&&"task".to_string()));
    assert!(keys.contains(&&"lane".to_string()));
    assert!(keys.contains(&&"workspace".to_string()));
    assert!(keys.contains(&&"context".to_string()));
    assert!(keys.contains(&&"policy".to_string()));
    assert!(keys.contains(&&"requirements".to_string()));
    assert!(keys.contains(&&"config".to_string()));
    // No unexpected extra top-level keys beyond what we define
    assert_eq!(obj.len(), 8, "WorkOrder should have exactly 8 fields");
}

#[test]
fn receipt_schema_has_expected_top_level_keys() {
    let json = serde_json::to_value(minimal_receipt()).unwrap();
    let obj = json.as_object().unwrap();
    let expected = [
        "meta",
        "backend",
        "capabilities",
        "mode",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
        "receipt_sha256",
    ];
    for key in &expected {
        assert!(
            obj.contains_key(*key),
            "Receipt missing expected key: {key}"
        );
    }
}

#[test]
fn run_metadata_schema_shape() {
    let receipt = minimal_receipt();
    let meta_json = serde_json::to_value(&receipt.meta).unwrap();
    let obj = meta_json.as_object().unwrap();
    let expected = [
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ];
    for key in &expected {
        assert!(
            obj.contains_key(*key),
            "RunMetadata missing expected key: {key}"
        );
    }
}

// ===========================================================================
// 11. Backward compatible additions
// ===========================================================================

#[test]
fn adding_capability_to_manifest_is_backward_compatible() {
    let mut manifest: CapabilityManifest = BTreeMap::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    let json = serde_json::to_string(&manifest).unwrap();

    // Add a new capability
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    let json2 = serde_json::to_string(&manifest).unwrap();

    // Old manifest can still be deserialized
    let old: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(old.len(), 1);

    // New manifest also deserializes
    let new: CapabilityManifest = serde_json::from_str(&json2).unwrap();
    assert_eq!(new.len(), 2);
}

#[test]
fn adding_context_snippets_is_backward_compatible() {
    // Old format: empty snippets
    let old_json = r#"{"files":[],"snippets":[]}"#;
    let ctx: ContextPacket = serde_json::from_str(old_json).unwrap();
    assert!(ctx.snippets.is_empty());

    // New format: with snippets
    let new_json = r#"{"files":["a.rs"],"snippets":[{"name":"hint","content":"do X"}]}"#;
    let ctx2: ContextPacket = serde_json::from_str(new_json).unwrap();
    assert_eq!(ctx2.snippets.len(), 1);
    assert_eq!(ctx2.snippets[0].name, "hint");
}

#[test]
fn policy_profile_defaults_are_empty() {
    let p = PolicyProfile::default();
    let json = serde_json::to_value(&p).unwrap();
    let obj = json.as_object().unwrap();
    // All fields should be empty arrays
    for (key, val) in obj {
        assert!(
            val.as_array().is_some_and(|a| a.is_empty()),
            "PolicyProfile default field '{key}' should be empty array"
        );
    }
}

#[test]
fn builder_defaults_produce_valid_work_order() {
    let wo = WorkOrderBuilder::new("hello world").build();
    assert_eq!(wo.task, "hello world");
    assert_eq!(wo.lane, ExecutionLane::PatchFirst);
    assert_eq!(wo.workspace.mode, WorkspaceMode::Staged);
    assert_eq!(wo.workspace.root, ".");
    // Verify it serializes successfully
    let _ = serde_json::to_string(&wo).unwrap();
}

#[test]
fn builder_defaults_produce_valid_receipt() {
    let receipt = ReceiptBuilder::new("test-backend").build();
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    // Verify it serializes successfully
    let _ = serde_json::to_string(&receipt).unwrap();
}

// ===========================================================================
// 12. Deterministic serialization (BTreeMap ordering)
// ===========================================================================

#[test]
fn capability_manifest_serialization_is_deterministic() {
    let mut m1: CapabilityManifest = BTreeMap::new();
    m1.insert(Capability::ToolBash, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Emulated);
    m1.insert(Capability::ToolRead, SupportLevel::Native);

    let mut m2: CapabilityManifest = BTreeMap::new();
    // Insert in different order
    m2.insert(Capability::ToolRead, SupportLevel::Native);
    m2.insert(Capability::ToolBash, SupportLevel::Native);
    m2.insert(Capability::Streaming, SupportLevel::Emulated);

    let j1 = serde_json::to_string(&m1).unwrap();
    let j2 = serde_json::to_string(&m2).unwrap();
    assert_eq!(
        j1, j2,
        "BTreeMap must produce deterministic JSON regardless of insert order"
    );
}

#[test]
fn runtime_config_vendor_map_is_deterministic() {
    let mut c1 = RuntimeConfig::default();
    c1.vendor.insert("b".into(), serde_json::json!("val_b"));
    c1.vendor.insert("a".into(), serde_json::json!("val_a"));

    let mut c2 = RuntimeConfig::default();
    c2.vendor.insert("a".into(), serde_json::json!("val_a"));
    c2.vendor.insert("b".into(), serde_json::json!("val_b"));

    assert_eq!(
        serde_json::to_string(&c1).unwrap(),
        serde_json::to_string(&c2).unwrap()
    );
}
