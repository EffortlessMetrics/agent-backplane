// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive contract type coverage tests for abp-core.
//!
//! Every public type is tested for: construction, serde roundtrip, Clone, Debug,
//! and Default (where applicable).

use abp_core::*;
use chrono::Utc;
use serde_json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

fn roundtrip<T>(val: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_string(val).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

fn assert_debug<T: std::fmt::Debug>(val: &T) {
    let s = format!("{:?}", val);
    assert!(!s.is_empty());
}

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task")
        .root("/tmp/ws")
        .lane(ExecutionLane::PatchFirst)
        .workspace_mode(WorkspaceMode::Staged)
        .build()
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ── CONTRACT_VERSION ─────────────────────────────────────────────────────

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_prefix() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
}

// ── WorkOrder ────────────────────────────────────────────────────────────

#[test]
fn work_order_construction() {
    let wo = make_work_order();
    assert_eq!(wo.task, "test task");
    assert_eq!(wo.workspace.root, "/tmp/ws");
}

#[test]
fn work_order_serde_roundtrip() {
    let wo = make_work_order();
    let wo2 = roundtrip(&wo);
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.id, wo2.id);
}

#[test]
fn work_order_clone() {
    let wo = make_work_order();
    let wo2 = wo.clone();
    assert_eq!(wo.id, wo2.id);
}

#[test]
fn work_order_debug() {
    assert_debug(&make_work_order());
}

// ── WorkOrderBuilder ─────────────────────────────────────────────────────

#[test]
fn work_order_builder_all_setters() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/ws")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target".into()])
        .context(ContextPacket::default())
        .policy(PolicyProfile::default())
        .requirements(CapabilityRequirements::default())
        .config(RuntimeConfig::default())
        .model("gpt-4")
        .max_budget_usd(10.0)
        .max_turns(5)
        .build();

    assert_eq!(wo.task, "task");
    assert_eq!(wo.workspace.root, "/ws");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(5));
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target"]);
}

#[test]
fn work_order_builder_debug() {
    assert_debug(&WorkOrderBuilder::new("task"));
}

// ── RuntimeConfig (WorkOrderConfig) ──────────────────────────────────────

#[test]
fn runtime_config_default() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    assert!(cfg.vendor.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn runtime_config_serde_roundtrip() {
    let mut cfg = RuntimeConfig::default();
    cfg.model = Some("claude-3".into());
    cfg.max_turns = Some(10);
    let cfg2 = roundtrip(&cfg);
    assert_eq!(cfg.model, cfg2.model);
    assert_eq!(cfg.max_turns, cfg2.max_turns);
}

#[test]
fn runtime_config_clone_debug() {
    let cfg = RuntimeConfig::default();
    assert_debug(&cfg);
    let _ = cfg.clone();
}

// ── Receipt ──────────────────────────────────────────────────────────────

#[test]
fn receipt_construction() {
    let r = make_receipt();
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn receipt_serde_roundtrip() {
    let r = make_receipt();
    let r2 = roundtrip(&r);
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
}

#[test]
fn receipt_clone() {
    let r = make_receipt();
    let r2 = r.clone();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
}

#[test]
fn receipt_debug() {
    assert_debug(&make_receipt());
}

#[test]
fn receipt_with_hash() {
    let r = make_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
}

// ── RunMetadata (ReceiptMeta) ────────────────────────────────────────────

#[test]
fn run_metadata_construction() {
    let now = Utc::now();
    let meta = RunMetadata {
        run_id: Uuid::new_v4(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: now,
        finished_at: now,
        duration_ms: 100,
    };
    assert_eq!(meta.contract_version, CONTRACT_VERSION);
    assert_eq!(meta.duration_ms, 100);
}

#[test]
fn run_metadata_serde_roundtrip() {
    let now = Utc::now();
    let meta = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: now,
        finished_at: now,
        duration_ms: 42,
    };
    let meta2 = roundtrip(&meta);
    assert_eq!(meta.run_id, meta2.run_id);
    assert_eq!(meta.duration_ms, meta2.duration_ms);
}

#[test]
fn run_metadata_clone_debug() {
    let now = Utc::now();
    let meta = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: now,
        finished_at: now,
        duration_ms: 0,
    };
    assert_debug(&meta);
    let _ = meta.clone();
}

// ── ReceiptBuilder ───────────────────────────────────────────────────────

#[test]
fn receipt_builder_all_setters() {
    let now = Utc::now();
    let r = ReceiptBuilder::new("test-backend")
        .backend_id("override-backend")
        .outcome(Outcome::Partial)
        .started_at(now)
        .finished_at(now)
        .work_order_id(Uuid::nil())
        .capabilities(BTreeMap::new())
        .mode(ExecutionMode::Passthrough)
        .backend_version("1.0")
        .adapter_version("0.1")
        .usage_raw(serde_json::json!({"tokens": 100}))
        .usage(UsageNormalized::default())
        .verification(VerificationReport::default())
        .add_trace_event(make_agent_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        })
        .build();

    assert_eq!(r.backend.id, "override-backend");
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
}

#[test]
fn receipt_builder_with_hash() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_builder_debug() {
    assert_debug(&ReceiptBuilder::new("mock"));
}

// ── Outcome ──────────────────────────────────────────────────────────────

#[test]
fn outcome_complete() {
    let o = Outcome::Complete;
    let o2 = roundtrip(&o);
    assert_eq!(o, o2);
}

#[test]
fn outcome_partial() {
    let o = Outcome::Partial;
    let o2 = roundtrip(&o);
    assert_eq!(o, o2);
}

#[test]
fn outcome_failed() {
    let o = Outcome::Failed;
    let o2 = roundtrip(&o);
    assert_eq!(o, o2);
}

#[test]
fn outcome_clone_debug() {
    for o in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        assert_debug(&o);
        let _ = o.clone();
    }
}

// ── AgentEvent ───────────────────────────────────────────────────────────

#[test]
fn agent_event_construction() {
    let e = make_agent_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    assert!(e.ext.is_none());
}

#[test]
fn agent_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("raw".into(), serde_json::json!("data"));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "warn".into(),
        },
        ext: Some(ext),
    };
    assert!(e.ext.is_some());
    let e2 = roundtrip(&e);
    assert!(e2.ext.is_some());
}

#[test]
fn agent_event_serde_roundtrip() {
    let e = make_agent_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let e2 = roundtrip(&e);
    assert_debug(&e2);
}

#[test]
fn agent_event_clone_debug() {
    let e = make_agent_event(AgentEventKind::Error {
        message: "err".into(),
    });
    assert_debug(&e);
    let _ = e.clone();
}

// ── AgentEventKind (all variants) ────────────────────────────────────────

#[test]
fn event_kind_run_started() {
    let k = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let e = make_agent_event(k);
    let e2 = roundtrip(&e);
    assert_debug(&e2);
}

#[test]
fn event_kind_run_completed() {
    let k = AgentEventKind::RunCompleted {
        message: "done".into(),
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

#[test]
fn event_kind_assistant_delta() {
    let k = AgentEventKind::AssistantDelta {
        text: "tok".into(),
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

#[test]
fn event_kind_assistant_message() {
    let k = AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

#[test]
fn event_kind_tool_call() {
    let k = AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("id1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/tmp/f"}),
    };
    let e = make_agent_event(k);
    let e2 = roundtrip(&e);
    assert_debug(&e2);
}

#[test]
fn event_kind_tool_result() {
    let k = AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("id1".into()),
        output: serde_json::json!("contents"),
        is_error: false,
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

#[test]
fn event_kind_file_changed() {
    let k = AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added fn".into(),
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

#[test]
fn event_kind_command_executed() {
    let k = AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

#[test]
fn event_kind_warning() {
    let k = AgentEventKind::Warning {
        message: "slow".into(),
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

#[test]
fn event_kind_error() {
    let k = AgentEventKind::Error {
        message: "boom".into(),
    };
    let e = make_agent_event(k);
    let _ = roundtrip(&e);
}

// ── Capability ───────────────────────────────────────────────────────────

#[test]
fn capability_all_variants_serde() {
    let caps = [
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
    ];
    for cap in &caps {
        let rt = roundtrip(cap);
        assert_eq!(cap, &rt);
    }
}

#[test]
fn capability_clone_debug_ord() {
    let a = Capability::ToolRead;
    let b = Capability::ToolWrite;
    assert_debug(&a);
    let _ = a.clone();
    // Ord is derived, just exercise it
    let _ = a.cmp(&b);
}

// ── SupportLevel ─────────────────────────────────────────────────────────

#[test]
fn support_level_native() {
    let s = SupportLevel::Native;
    let s2 = roundtrip(&s);
    assert_debug(&s2);
}

#[test]
fn support_level_emulated() {
    let s = SupportLevel::Emulated;
    let _ = roundtrip(&s);
}

#[test]
fn support_level_unsupported() {
    let s = SupportLevel::Unsupported;
    let _ = roundtrip(&s);
}

#[test]
fn support_level_restricted() {
    let s = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    let s2 = roundtrip(&s);
    assert_debug(&s2);
}

#[test]
fn support_level_satisfies() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Restricted { reason: "x".into() }.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_clone_debug() {
    for s in [
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "r".into(),
        },
    ] {
        assert_debug(&s);
        let _ = s.clone();
    }
}

// ── CapabilityManifest ───────────────────────────────────────────────────

#[test]
fn capability_manifest_construction_and_roundtrip() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    assert!(m.contains_key(&Capability::ToolRead));
    let m2: CapabilityManifest = roundtrip(&m);
    assert_eq!(m.len(), m2.len());
}

// ── PolicyProfile ────────────────────────────────────────────────────────

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
fn policy_profile_serde_roundtrip() {
    let mut p = PolicyProfile::default();
    p.allowed_tools = vec!["read".into(), "write".into()];
    p.deny_read = vec!["secrets/*".into()];
    let p2 = roundtrip(&p);
    assert_eq!(p.allowed_tools, p2.allowed_tools);
    assert_eq!(p.deny_read, p2.deny_read);
}

#[test]
fn policy_profile_clone_debug() {
    let p = PolicyProfile::default();
    assert_debug(&p);
    let _ = p.clone();
}

// ── ExecutionMode ────────────────────────────────────────────────────────

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough() {
    let m = ExecutionMode::Passthrough;
    let m2 = roundtrip(&m);
    assert_eq!(m, m2);
}

#[test]
fn execution_mode_mapped() {
    let m = ExecutionMode::Mapped;
    let m2 = roundtrip(&m);
    assert_eq!(m, m2);
}

#[test]
fn execution_mode_clone_debug() {
    for m in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        assert_debug(&m);
        let _ = m.clone();
    }
}

// ── ExecutionLane ────────────────────────────────────────────────────────

#[test]
fn execution_lane_patch_first() {
    let l = ExecutionLane::PatchFirst;
    let l2 = roundtrip(&l);
    assert_debug(&l2);
}

#[test]
fn execution_lane_workspace_first() {
    let l = ExecutionLane::WorkspaceFirst;
    let _ = roundtrip(&l);
}

#[test]
fn execution_lane_clone_debug() {
    for l in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        assert_debug(&l);
        let _ = l.clone();
    }
}

// ── WorkspaceSpec (WorkspaceConfig) ──────────────────────────────────────

#[test]
fn workspace_spec_construction() {
    let ws = WorkspaceSpec {
        root: "/tmp".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec!["target".into()],
    };
    assert_eq!(ws.root, "/tmp");
}

#[test]
fn workspace_spec_serde_roundtrip() {
    let ws = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws2 = roundtrip(&ws);
    assert_eq!(ws.root, ws2.root);
}

#[test]
fn workspace_spec_clone_debug() {
    let ws = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    assert_debug(&ws);
    let _ = ws.clone();
}

// ── WorkspaceMode ────────────────────────────────────────────────────────

#[test]
fn workspace_mode_pass_through() {
    let m = WorkspaceMode::PassThrough;
    let m2 = roundtrip(&m);
    assert_debug(&m2);
}

#[test]
fn workspace_mode_staged() {
    let m = WorkspaceMode::Staged;
    let _ = roundtrip(&m);
}

#[test]
fn workspace_mode_clone_debug() {
    for m in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        assert_debug(&m);
        let _ = m.clone();
    }
}

// ── ContextPacket ────────────────────────────────────────────────────────

#[test]
fn context_packet_default() {
    let cp = ContextPacket::default();
    assert!(cp.files.is_empty());
    assert!(cp.snippets.is_empty());
}

#[test]
fn context_packet_serde_roundtrip() {
    let cp = ContextPacket {
        files: vec!["src/lib.rs".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    let cp2 = roundtrip(&cp);
    assert_eq!(cp.files, cp2.files);
    assert_eq!(cp.snippets.len(), cp2.snippets.len());
}

#[test]
fn context_packet_clone_debug() {
    let cp = ContextPacket::default();
    assert_debug(&cp);
    let _ = cp.clone();
}

// ── ContextSnippet (ContextItem) ─────────────────────────────────────────

#[test]
fn context_snippet_construction_and_roundtrip() {
    let cs = ContextSnippet {
        name: "snippet".into(),
        content: "data".into(),
    };
    let cs2 = roundtrip(&cs);
    assert_eq!(cs.name, cs2.name);
    assert_eq!(cs.content, cs2.content);
}

#[test]
fn context_snippet_clone_debug() {
    let cs = ContextSnippet {
        name: "s".into(),
        content: "c".into(),
    };
    assert_debug(&cs);
    let _ = cs.clone();
}

// ── CapabilityRequirements ───────────────────────────────────────────────

#[test]
fn capability_requirements_default() {
    let cr = CapabilityRequirements::default();
    assert!(cr.required.is_empty());
}

#[test]
fn capability_requirements_serde_roundtrip() {
    let cr = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let cr2 = roundtrip(&cr);
    assert_eq!(cr.required.len(), cr2.required.len());
}

#[test]
fn capability_requirements_clone_debug() {
    let cr = CapabilityRequirements::default();
    assert_debug(&cr);
    let _ = cr.clone();
}

// ── MinSupport ───────────────────────────────────────────────────────────

#[test]
fn min_support_native_serde() {
    let m = MinSupport::Native;
    let m2 = roundtrip(&m);
    assert_debug(&m2);
}

#[test]
fn min_support_emulated_serde() {
    let m = MinSupport::Emulated;
    let _ = roundtrip(&m);
}

#[test]
fn min_support_clone_debug() {
    for m in [MinSupport::Native, MinSupport::Emulated] {
        assert_debug(&m);
        let _ = m.clone();
    }
}

// ── BackendIdentity ──────────────────────────────────────────────────────

#[test]
fn backend_identity_construction_and_roundtrip() {
    let bi = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0".into()),
        adapter_version: None,
    };
    let bi2 = roundtrip(&bi);
    assert_eq!(bi.id, bi2.id);
    assert_eq!(bi.backend_version, bi2.backend_version);
}

#[test]
fn backend_identity_clone_debug() {
    let bi = BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    };
    assert_debug(&bi);
    let _ = bi.clone();
}

// ── UsageNormalized ──────────────────────────────────────────────────────

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
fn usage_normalized_serde_roundtrip() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(10),
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    };
    let u2 = roundtrip(&u);
    assert_eq!(u.input_tokens, u2.input_tokens);
    assert_eq!(u.output_tokens, u2.output_tokens);
}

#[test]
fn usage_normalized_clone_debug() {
    let u = UsageNormalized::default();
    assert_debug(&u);
    let _ = u.clone();
}

// ── ArtifactRef ──────────────────────────────────────────────────────────

#[test]
fn artifact_ref_construction_and_roundtrip() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
    };
    let a2 = roundtrip(&a);
    assert_eq!(a.kind, a2.kind);
    assert_eq!(a.path, a2.path);
}

#[test]
fn artifact_ref_clone_debug() {
    let a = ArtifactRef {
        kind: "log".into(),
        path: "run.log".into(),
    };
    assert_debug(&a);
    let _ = a.clone();
}

// ── VerificationReport ───────────────────────────────────────────────────

#[test]
fn verification_report_default() {
    let v = VerificationReport::default();
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
    assert!(!v.harness_ok);
}

#[test]
fn verification_report_serde_roundtrip() {
    let v = VerificationReport {
        git_diff: Some("diff".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    let v2 = roundtrip(&v);
    assert_eq!(v.git_diff, v2.git_diff);
    assert!(v2.harness_ok);
}

#[test]
fn verification_report_clone_debug() {
    let v = VerificationReport::default();
    assert_debug(&v);
    let _ = v.clone();
}

// ── ContractError ────────────────────────────────────────────────────────

#[test]
fn contract_error_debug() {
    let bad_json: Result<WorkOrder, _> = serde_json::from_str("not json");
    let err = bad_json.unwrap_err();
    let ce = ContractError::Json(err);
    assert_debug(&ce);
    assert!(!format!("{}", ce).is_empty());
}

// ── canonical_json & sha256_hex ──────────────────────────────────────────

#[test]
fn canonical_json_deterministic() {
    let r = make_receipt();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn sha256_hex_length() {
    let hash = sha256_hex(b"hello");
    assert_eq!(hash.len(), 64);
}

// ── CapabilityRequirement ────────────────────────────────────────────────

#[test]
fn capability_requirement_construction_and_roundtrip() {
    let cr = CapabilityRequirement {
        capability: Capability::McpClient,
        min_support: MinSupport::Emulated,
    };
    let cr2 = roundtrip(&cr);
    assert_debug(&cr2);
}

#[test]
fn capability_requirement_clone_debug() {
    let cr = CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    };
    assert_debug(&cr);
    let _ = cr.clone();
}
