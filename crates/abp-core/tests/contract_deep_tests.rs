// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for abp-core contract types.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::*;
use chrono::{TimeZone, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Helpers ─────────────────────────────────────────────────────────────

fn sample_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "Refactor auth module".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Use JWT".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["write".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: Some(1.0),
            max_turns: Some(10),
        },
    }
}

fn sample_receipt() -> Receipt {
    let t = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: t,
            finished_at: t,
            duration_ms: 0,
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
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ── 1. CONTRACT_VERSION ─────────────────────────────────────────────────

#[test]
fn contract_version_is_abp_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ── 2. WorkOrder construction ───────────────────────────────────────────

#[test]
fn work_order_with_all_fields() {
    let wo = sample_work_order();
    assert_eq!(wo.task, "Refactor auth module");
    assert_eq!(wo.id, Uuid::nil());
    assert_eq!(wo.workspace.root, "/tmp/ws");
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo.requirements.required.len(), 1);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(1.0));
}

#[test]
fn work_order_default_config() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    assert!(cfg.vendor.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn work_order_default_policy() {
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
fn work_order_default_context() {
    let ctx = ContextPacket::default();
    assert!(ctx.files.is_empty());
    assert!(ctx.snippets.is_empty());
}

#[test]
fn work_order_default_requirements() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

// ── 3. WorkOrderBuilder ────────────────────────────────────────────────

#[test]
fn builder_minimal() {
    let wo = WorkOrderBuilder::new("do stuff").build();
    assert_eq!(wo.task, "do stuff");
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn builder_all_fields() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/my/root")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .model("claude-3")
        .max_budget_usd(5.0)
        .max_turns(20)
        .build();

    assert_eq!(wo.task, "task");
    assert_eq!(wo.workspace.root, "/my/root");
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(20));
}

#[test]
fn builder_with_context() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files, vec!["a.rs"]);
}

#[test]
fn builder_with_policy() {
    let mut policy = PolicyProfile::default();
    policy.deny_write.push("*.lock".into());
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    assert_eq!(wo.policy.deny_write, vec!["*.lock"]);
}

#[test]
fn builder_with_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolUse,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn builder_with_runtime_config() {
    let mut cfg = RuntimeConfig::default();
    cfg.env.insert("KEY".into(), "VAL".into());
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.env.get("KEY").unwrap(), "VAL");
}

#[test]
fn builder_generates_unique_ids() {
    let a = WorkOrderBuilder::new("a").build();
    let b = WorkOrderBuilder::new("b").build();
    assert_ne!(a.id, b.id);
}

// ── 4. Receipt construction ────────────────────────────────────────────

#[test]
fn receipt_default_fields() {
    let r = sample_receipt();
    assert_eq!(r.backend.id, "mock");
    assert!(r.receipt_sha256.is_none());
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
}

#[test]
fn receipt_builder_minimal() {
    let r = ReceiptBuilder::new("test-backend").build();
    assert_eq!(r.backend.id, "test-backend");
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn receipt_builder_all_methods() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let wo_id = Uuid::new_v4();

    let evt = AgentEvent {
        ts: t1,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };

    let r = ReceiptBuilder::new("be")
        .backend_version("1.0")
        .adapter_version("0.1")
        .outcome(Outcome::Partial)
        .started_at(t1)
        .finished_at(t2)
        .work_order_id(wo_id)
        .mode(ExecutionMode::Passthrough)
        .add_trace_event(evt)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .usage_raw(json!({"tokens": 100}))
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            ..Default::default()
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: None,
            harness_ok: true,
        })
        .build();

    assert_eq!(r.backend.id, "be");
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.1"));
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.meta.work_order_id, wo_id);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.meta.duration_ms, 5000);
    assert_eq!(r.usage.input_tokens, Some(50));
    assert!(r.verification.harness_ok);
}

#[test]
fn receipt_builder_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("b").capabilities(caps).build();
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

// ── 5. Receipt hashing ─────────────────────────────────────────────────

#[test]
fn receipt_hash_deterministic() {
    let r = sample_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn receipt_hash_self_referential_prevention() {
    let r = sample_receipt();
    let h1 = receipt_hash(&r).unwrap();

    let mut r2 = sample_receipt();
    r2.receipt_sha256 = Some("garbage".into());
    let h2 = receipt_hash(&r2).unwrap();

    // Hash ignores the stored receipt_sha256 value
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_hash_sets_hash() {
    let r = sample_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_with_hash_is_verifiable() {
    let r = sample_receipt().with_hash().unwrap();
    let expected = receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &expected);
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
fn receipt_hash_changes_with_outcome() {
    let r1 = {
        let mut r = sample_receipt();
        r.outcome = Outcome::Complete;
        receipt_hash(&r).unwrap()
    };
    let r2 = {
        let mut r = sample_receipt();
        r.outcome = Outcome::Failed;
        receipt_hash(&r).unwrap()
    };
    assert_ne!(r1, r2);
}

// ── 6. AgentEvent variants ─────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[test]
fn agent_event_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_run_completed() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn agent_event_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    assert!(matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn agent_event_assistant_message() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn agent_event_tool_call() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("id-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "a.rs"}),
    });
    assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn agent_event_tool_result() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("id-1".into()),
        output: json!("contents"),
        is_error: false,
    });
    assert!(matches!(e.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn agent_event_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added fn".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn agent_event_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    assert!(matches!(e.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn agent_event_warning() {
    let e = make_event(AgentEventKind::Warning {
        message: "deprecated".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn agent_event_error() {
    let e = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: Some(abp_error::ErrorCode::Internal),
    });
    assert!(matches!(e.kind, AgentEventKind::Error { .. }));
}

#[test]
fn agent_event_error_without_code() {
    let e = make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    });
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert!(error_code.is_none());
    } else {
        panic!("expected Error");
    }
}

#[test]
fn agent_event_ext_field() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"foo": "bar"}));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    assert!(e.ext.is_some());
    assert!(e.ext.as_ref().unwrap().contains_key("raw_message"));
}

// ── 7. ErrorCode ───────────────────────────────────────────────────────

#[test]
fn error_code_as_str_is_snake_case() {
    let codes = [
        (
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
            "protocol_invalid_envelope",
        ),
        (abp_error::ErrorCode::BackendNotFound, "backend_not_found"),
        (abp_error::ErrorCode::BackendTimeout, "backend_timeout"),
        (abp_error::ErrorCode::PolicyDenied, "policy_denied"),
        (abp_error::ErrorCode::Internal, "internal"),
        (
            abp_error::ErrorCode::ReceiptHashMismatch,
            "receipt_hash_mismatch",
        ),
        (abp_error::ErrorCode::ConfigInvalid, "config_invalid"),
    ];
    for (code, expected) in &codes {
        assert_eq!(code.as_str(), *expected);
    }
}

#[test]
fn error_code_display_returns_message() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let msg = format!("{code}");
    assert!(!msg.is_empty());
    assert_eq!(msg, "backend timed out");
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
}

#[test]
fn error_code_retryable() {
    assert!(abp_error::ErrorCode::BackendTimeout.is_retryable());
    assert!(abp_error::ErrorCode::BackendRateLimited.is_retryable());
    assert!(!abp_error::ErrorCode::PolicyDenied.is_retryable());
    assert!(!abp_error::ErrorCode::Internal.is_retryable());
}

#[test]
fn error_code_serde_roundtrip() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
    let back: abp_error::ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

// ── 8. Outcome ─────────────────────────────────────────────────────────

#[test]
fn outcome_variants_serde() {
    for (variant, expected) in [
        (Outcome::Complete, r#""complete""#),
        (Outcome::Partial, r#""partial""#),
        (Outcome::Failed, r#""failed""#),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected);
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, variant);
    }
}

// ── 9. ExecutionMode ───────────────────────────────────────────────────

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, r#""passthrough""#);
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ExecutionMode::Passthrough);
}

// ── 10. BTreeMap deterministic serialization ────────────────────────────

#[test]
fn vendor_config_deterministic_serialization() {
    let mut cfg1 = RuntimeConfig::default();
    cfg1.vendor.insert("z_key".into(), json!("z_val"));
    cfg1.vendor.insert("a_key".into(), json!("a_val"));
    cfg1.vendor.insert("m_key".into(), json!("m_val"));

    let mut cfg2 = RuntimeConfig::default();
    cfg2.vendor.insert("m_key".into(), json!("m_val"));
    cfg2.vendor.insert("a_key".into(), json!("a_val"));
    cfg2.vendor.insert("z_key".into(), json!("z_val"));

    let json1 = serde_json::to_string(&cfg1).unwrap();
    let json2 = serde_json::to_string(&cfg2).unwrap();
    assert_eq!(json1, json2);
    // Keys are alphabetically sorted
    let idx_a = json1.find("a_key").unwrap();
    let idx_m = json1.find("m_key").unwrap();
    let idx_z = json1.find("z_key").unwrap();
    assert!(idx_a < idx_m);
    assert!(idx_m < idx_z);
}

#[test]
fn env_map_deterministic() {
    let mut cfg = RuntimeConfig::default();
    cfg.env.insert("Z".into(), "1".into());
    cfg.env.insert("A".into(), "2".into());
    let json = serde_json::to_string(&cfg).unwrap();
    let idx_a = json.find(r#""A""#).unwrap();
    let idx_z = json.find(r#""Z""#).unwrap();
    assert!(idx_a < idx_z);
}

#[test]
fn capability_manifest_deterministic() {
    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolWrite, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Emulated);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::Streaming, SupportLevel::Emulated);
    m2.insert(Capability::ToolWrite, SupportLevel::Native);

    let j1 = serde_json::to_string(&m1).unwrap();
    let j2 = serde_json::to_string(&m2).unwrap();
    assert_eq!(j1, j2);
}

// ── 11. IrToolDefinition ───────────────────────────────────────────────

#[test]
fn ir_tool_definition_construction() {
    let td = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        }),
    };
    assert_eq!(td.name, "read_file");
}

#[test]
fn ir_tool_definition_json_schema_embedding() {
    let schema = json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "limit": {"type": "integer", "minimum": 1}
        }
    });
    let td = IrToolDefinition {
        name: "search".into(),
        description: "Search".into(),
        parameters: schema.clone(),
    };
    assert_eq!(td.parameters, schema);
}

#[test]
fn ir_tool_definition_serde_roundtrip() {
    let td = IrToolDefinition {
        name: "tool".into(),
        description: "desc".into(),
        parameters: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back, td);
}

// ── 12. IrMessage / IrConversation ─────────────────────────────────────

#[test]
fn ir_role_variants() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }
}

#[test]
fn ir_message_with_text() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.content.len(), 1);
    assert!(msg.is_text_only());
    assert_eq!(msg.text_content(), "hello");
}

#[test]
fn ir_message_with_metadata() {
    let mut msg = IrMessage::text(IrRole::Tool, "result");
    msg.metadata.insert("tool_call_id".into(), json!("tc-1"));
    assert_eq!(msg.metadata["tool_call_id"], "tc-1");
}

#[test]
fn ir_conversation_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
}

#[test]
fn ir_conversation_push_and_accessors() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hey"));

    assert_eq!(conv.len(), 3);
    assert!(!conv.is_empty());
    assert!(conv.system_message().is_some());
    assert!(conv.last_assistant().is_some());
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
    assert!(conv.last_message().is_some());
}

// ── 13. Config types ───────────────────────────────────────────────────

#[test]
fn runtime_config_vendor_map() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor
        .insert("openai".into(), json!({"temperature": 0.7}));
    cfg.vendor
        .insert("anthropic".into(), json!({"max_tokens": 4096}));
    assert_eq!(cfg.vendor.len(), 2);
    assert_eq!(cfg.vendor["openai"]["temperature"], 0.7);
}

#[test]
fn runtime_config_nested_vendor() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor
        .insert("abp".into(), json!({"mode": "passthrough", "debug": true}));
    let json = serde_json::to_string(&cfg).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vendor["abp"]["mode"], "passthrough");
}

// ── 14. Serde roundtrip: ALL public types ──────────────────────────────

#[test]
fn serde_roundtrip_work_order() {
    let wo = sample_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, wo.task);
    assert_eq!(back.id, wo.id);
}

#[test]
fn serde_roundtrip_receipt() {
    let r = sample_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backend.id, "mock");
    assert_eq!(back.outcome, Outcome::Complete);
}

#[test]
fn serde_roundtrip_execution_lane() {
    for lane in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        let json = serde_json::to_string(&lane).unwrap();
        let back: ExecutionLane = serde_json::from_str(&json).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, j2);
    }
}

#[test]
fn serde_roundtrip_workspace_mode() {
    for mode in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: WorkspaceMode = serde_json::from_str(&json).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, j2);
    }
}

#[test]
fn serde_roundtrip_agent_event_all_variants() {
    let variants: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
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

    for kind in variants {
        let event = make_event(kind);
        let json = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, j2);
    }
}

#[test]
fn serde_roundtrip_support_level() {
    let levels = [
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, j2);
    }
}

#[test]
fn serde_roundtrip_backend_identity() {
    let id = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.2".into()),
    };
    let json = serde_json::to_string(&id).unwrap();
    let back: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "sidecar:node");
    assert_eq!(back.backend_version.as_deref(), Some("1.0"));
}

#[test]
fn serde_roundtrip_usage_normalized() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(10),
        cache_write_tokens: Some(20),
        request_units: Some(5),
        estimated_cost_usd: Some(0.01),
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, Some(100));
    assert_eq!(back.estimated_cost_usd, Some(0.01));
}

#[test]
fn serde_roundtrip_verification_report() {
    let v = VerificationReport {
        git_diff: Some("diff --git a/b".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: VerificationReport = serde_json::from_str(&json).unwrap();
    assert!(back.harness_ok);
    assert!(back.git_diff.is_some());
}

#[test]
fn serde_roundtrip_artifact_ref() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let json = serde_json::to_string(&a).unwrap();
    let back: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.kind, "patch");
}

#[test]
fn serde_roundtrip_run_metadata() {
    let t = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let m = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: t,
        finished_at: t,
        duration_ms: 42,
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.duration_ms, 42);
    assert_eq!(back.contract_version, CONTRACT_VERSION);
}

#[test]
fn serde_roundtrip_context_snippet() {
    let s = ContextSnippet {
        name: "hint".into(),
        content: "Use async".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "hint");
}

#[test]
fn serde_roundtrip_ir_conversation() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 1);
}

#[test]
fn serde_roundtrip_ir_usage() {
    let u = IrUsage::from_io(100, 200);
    let json = serde_json::to_string(&u).unwrap();
    let back: IrUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_tokens, 300);
}

// ── 15. Schema generation ──────────────────────────────────────────────

#[test]
fn schema_generation_work_order() {
    let schema = schemars::schema_for!(WorkOrder);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.get("$schema").is_some() || json.get("title").is_some());
}

#[test]
fn schema_generation_receipt() {
    let schema = schemars::schema_for!(Receipt);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_object());
}

#[test]
fn schema_generation_agent_event() {
    let schema = schemars::schema_for!(AgentEvent);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_object());
}

#[test]
fn schema_generation_ir_tool_definition() {
    let schema = schemars::schema_for!(IrToolDefinition);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_object());
}

// ── 16. SupportLevel::satisfies ────────────────────────────────────────

#[test]
fn support_level_satisfies_matrix() {
    // Native satisfies both Native and Emulated
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));

    // Emulated satisfies Emulated but not Native
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));

    // Unsupported satisfies nothing
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));

    // Restricted satisfies Emulated but not Native
    let restricted = SupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

// ── 17. canonical_json ─────────────────────────────────────────────────

#[test]
fn canonical_json_key_ordering() {
    let val = json!({"z": 1, "a": 2, "m": 3});
    let cj = canonical_json(&val).unwrap();
    assert!(cj.starts_with(r#"{"a":2"#));
}

#[test]
fn canonical_json_deterministic() {
    let wo = sample_work_order();
    let j1 = canonical_json(&wo).unwrap();
    let j2 = canonical_json(&wo).unwrap();
    assert_eq!(j1, j2);
}

// ── 18. sha256_hex ─────────────────────────────────────────────────────

#[test]
fn sha256_hex_produces_64_char_hex() {
    let hex = sha256_hex(b"hello world");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn sha256_hex_deterministic() {
    let a = sha256_hex(b"test");
    let b = sha256_hex(b"test");
    assert_eq!(a, b);
}

#[test]
fn sha256_hex_different_input_different_hash() {
    let a = sha256_hex(b"hello");
    let b = sha256_hex(b"world");
    assert_ne!(a, b);
}

// ── 19. IrContentBlock variants ────────────────────────────────────────

#[test]
fn ir_content_block_text() {
    let b = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains(r#""type":"text""#));
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_image() {
    let b = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    let json = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_tool_use() {
    let b = IrContentBlock::ToolUse {
        id: "tu-1".into(),
        name: "search".into(),
        input: json!({"q": "rust"}),
    };
    let json = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_tool_result() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "tu-1".into(),
        content: vec![IrContentBlock::Text {
            text: "result text".into(),
        }],
        is_error: false,
    };
    let json = serde_json::to_string(&b).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, b);
}

// ── 20. IrUsage helpers ────────────────────────────────────────────────

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 200);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 200);
    assert_eq!(u.total_tokens, 300);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn ir_usage_with_cache() {
    let u = IrUsage::with_cache(100, 200, 10, 20);
    assert_eq!(u.total_tokens, 300);
    assert_eq!(u.cache_read_tokens, 10);
    assert_eq!(u.cache_write_tokens, 20);
}

#[test]
fn ir_usage_default() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

// ── 21. AgentEvent serde tag ───────────────────────────────────────────

#[test]
fn agent_event_kind_tagged_with_type() {
    let e = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let json = serde_json::to_string(&e).unwrap();
    // AgentEventKind uses #[serde(tag = "type")]
    assert!(json.contains(r#""type":"assistant_message""#));
}

#[test]
fn agent_event_ext_skipped_when_none() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("ext"));
}

#[test]
fn agent_event_ext_present_when_some() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), json!("val"));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("key"));
}
