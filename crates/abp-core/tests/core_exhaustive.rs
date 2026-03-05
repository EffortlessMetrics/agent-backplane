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
//! Exhaustive unit tests for every public type in `abp-core`.

use abp_core::*;
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ─── helpers ────────────────────────────────────────────────────────────

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// CONTRACT_VERSION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_starts_with_abp() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
}

// ═══════════════════════════════════════════════════════════════════════
// WorkOrder & WorkOrderBuilder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert_eq!(wo.workspace.root, ".");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.requirements.required.is_empty());
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn work_order_builder_lane() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
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
fn work_order_builder_include_exclude() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_builder_context() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "use this".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "hint");
}

#[test]
fn work_order_builder_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["secret/**".into()],
        deny_write: vec![],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec![],
        require_approval_for: vec!["bash".into()],
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo.policy.disallowed_tools, vec!["bash"]);
    assert_eq!(wo.policy.deny_read, vec!["secret/**"]);
    assert_eq!(wo.policy.require_approval_for, vec!["bash"]);
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
    let cfg = RuntimeConfig {
        model: Some("gpt-4".into()),
        max_turns: Some(5),
        max_budget_usd: Some(1.0),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn work_order_builder_model() {
    let wo = WorkOrderBuilder::new("t").model("claude-3").build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
}

#[test]
fn work_order_builder_max_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(2.5).build();
    assert_eq!(wo.config.max_budget_usd, Some(2.5));
}

#[test]
fn work_order_builder_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(20).build();
    assert_eq!(wo.config.max_turns, Some(20));
}

#[test]
fn work_order_has_unique_id() {
    let a = make_work_order();
    let b = make_work_order();
    assert_ne!(a.id, b.id);
}

#[test]
fn work_order_serialization_roundtrip() {
    let wo = make_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, wo.task);
    assert_eq!(back.id, wo.id);
}

// ═══════════════════════════════════════════════════════════════════════
// ExecutionLane
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn execution_lane_patch_first_serde() {
    let val = ExecutionLane::PatchFirst;
    let json = serde_json::to_value(&val).unwrap();
    assert_eq!(json, json!("patch_first"));
    let back: ExecutionLane = serde_json::from_value(json).unwrap();
    assert!(matches!(back, ExecutionLane::PatchFirst));
}

#[test]
fn execution_lane_workspace_first_serde() {
    let val = ExecutionLane::WorkspaceFirst;
    let json = serde_json::to_value(&val).unwrap();
    assert_eq!(json, json!("workspace_first"));
    let back: ExecutionLane = serde_json::from_value(json).unwrap();
    assert!(matches!(back, ExecutionLane::WorkspaceFirst));
}

// ═══════════════════════════════════════════════════════════════════════
// WorkspaceMode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workspace_mode_passthrough_serde() {
    let v = WorkspaceMode::PassThrough;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("pass_through"));
    let back: WorkspaceMode = serde_json::from_value(j).unwrap();
    assert!(matches!(back, WorkspaceMode::PassThrough));
}

#[test]
fn workspace_mode_staged_serde() {
    let v = WorkspaceMode::Staged;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("staged"));
    let back: WorkspaceMode = serde_json::from_value(j).unwrap();
    assert!(matches!(back, WorkspaceMode::Staged));
}

// ═══════════════════════════════════════════════════════════════════════
// ExecutionMode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough_serde() {
    let v = ExecutionMode::Passthrough;
    let j = serde_json::to_value(v).unwrap();
    assert_eq!(j, json!("passthrough"));
    let back: ExecutionMode = serde_json::from_value(j).unwrap();
    assert_eq!(back, ExecutionMode::Passthrough);
}

#[test]
fn execution_mode_mapped_serde() {
    let v = ExecutionMode::Mapped;
    let j = serde_json::to_value(v).unwrap();
    assert_eq!(j, json!("mapped"));
    let back: ExecutionMode = serde_json::from_value(j).unwrap();
    assert_eq!(back, ExecutionMode::Mapped);
}

// ═══════════════════════════════════════════════════════════════════════
// Outcome
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn outcome_complete_serde() {
    let v = Outcome::Complete;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("complete"));
    let back: Outcome = serde_json::from_value(j).unwrap();
    assert_eq!(back, Outcome::Complete);
}

#[test]
fn outcome_partial_serde() {
    let v = Outcome::Partial;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("partial"));
    let back: Outcome = serde_json::from_value(j).unwrap();
    assert_eq!(back, Outcome::Partial);
}

#[test]
fn outcome_failed_serde() {
    let v = Outcome::Failed;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("failed"));
    let back: Outcome = serde_json::from_value(j).unwrap();
    assert_eq!(back, Outcome::Failed);
}

#[test]
fn outcome_equality() {
    assert_eq!(Outcome::Complete, Outcome::Complete);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Partial, Outcome::Failed);
}

// ═══════════════════════════════════════════════════════════════════════
// Capability — all 26 variants
// ═══════════════════════════════════════════════════════════════════════

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
    ]
}

#[test]
fn capability_count_is_26() {
    assert_eq!(all_capabilities().len(), 26);
}

#[test]
fn capability_all_variants_roundtrip() {
    for cap in all_capabilities() {
        let j = serde_json::to_value(&cap).unwrap();
        let back: Capability = serde_json::from_value(j.clone()).unwrap();
        assert_eq!(cap, back, "roundtrip failed for {j}");
    }
}

#[test]
fn capability_ord_is_consistent() {
    let caps = all_capabilities();
    for i in 0..caps.len() {
        for j in 0..caps.len() {
            // Ord must be total — (a<=b) || (b<=a)
            assert!(caps[i] <= caps[j] || caps[j] <= caps[i]);
        }
    }
}

#[test]
fn capability_hash_works_in_btreemap() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    assert!(m.contains_key(&Capability::Streaming));
    assert!(!m.contains_key(&Capability::ToolWrite));
}

// ═══════════════════════════════════════════════════════════════════════
// SupportLevel
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn support_level_native_serde() {
    let v = SupportLevel::Native;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("native"));
}

#[test]
fn support_level_emulated_serde() {
    let v = SupportLevel::Emulated;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("emulated"));
}

#[test]
fn support_level_unsupported_serde() {
    let v = SupportLevel::Unsupported;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("unsupported"));
}

#[test]
fn support_level_restricted_serde() {
    let v = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    let j = serde_json::to_value(&v).unwrap();
    let back: SupportLevel = serde_json::from_value(j).unwrap();
    if let SupportLevel::Restricted { reason } = back {
        assert_eq!(reason, "policy");
    } else {
        panic!("expected Restricted");
    }
}

#[test]
fn support_level_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Restricted { reason: "x".into() }.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Restricted { reason: "x".into() }.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════
// MinSupport
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn min_support_native_serde() {
    let v = MinSupport::Native;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("native"));
    let back: MinSupport = serde_json::from_value(j).unwrap();
    assert!(matches!(back, MinSupport::Native));
}

#[test]
fn min_support_emulated_serde() {
    let v = MinSupport::Emulated;
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, json!("emulated"));
    let back: MinSupport = serde_json::from_value(j).unwrap();
    assert!(matches!(back, MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════
// ContextPacket & ContextSnippet
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn context_packet_default_is_empty() {
    let cp = ContextPacket::default();
    assert!(cp.files.is_empty());
    assert!(cp.snippets.is_empty());
}

#[test]
fn context_packet_roundtrip() {
    let cp = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "do stuff".into(),
        }],
    };
    let json = serde_json::to_string(&cp).unwrap();
    let back: ContextPacket = serde_json::from_str(&json).unwrap();
    assert_eq!(back.files, cp.files);
    assert_eq!(back.snippets[0].name, "hint");
    assert_eq!(back.snippets[0].content, "do stuff");
}

#[test]
fn context_snippet_construction() {
    let s = ContextSnippet {
        name: "n".into(),
        content: "c".into(),
    };
    assert_eq!(s.name, "n");
    assert_eq!(s.content, "c");
}

// ═══════════════════════════════════════════════════════════════════════
// RuntimeConfig
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn runtime_config_default() {
    let rc = RuntimeConfig::default();
    assert!(rc.model.is_none());
    assert!(rc.vendor.is_empty());
    assert!(rc.env.is_empty());
    assert!(rc.max_budget_usd.is_none());
    assert!(rc.max_turns.is_none());
}

#[test]
fn runtime_config_roundtrip() {
    let rc = RuntimeConfig {
        model: Some("model".into()),
        max_turns: Some(10),
        vendor: {
            let mut v = BTreeMap::new();
            v.insert("key".into(), serde_json::Value::Bool(true));
            v
        },
        env: {
            let mut e = BTreeMap::new();
            e.insert("FOO".into(), "bar".into());
            e
        },
        ..Default::default()
    };
    let json = serde_json::to_string(&rc).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model.as_deref(), Some("model"));
    assert_eq!(back.max_turns, Some(10));
    assert_eq!(back.vendor["key"], json!(true));
    assert_eq!(back.env["FOO"], "bar");
}

// ═══════════════════════════════════════════════════════════════════════
// PolicyProfile
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_profile_default_permits_all() {
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
        deny_read: vec![".env".into()],
        deny_write: vec!["*.lock".into()],
        allow_network: vec!["github.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["rm".into()],
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(back.allowed_tools, vec!["read"]);
    assert_eq!(back.deny_network, vec!["evil.com"]);
}

// ═══════════════════════════════════════════════════════════════════════
// CapabilityRequirements & CapabilityRequirement
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_requirements_default() {
    let cr = CapabilityRequirements::default();
    assert!(cr.required.is_empty());
}

#[test]
fn capability_requirement_roundtrip() {
    let req = CapabilityRequirement {
        capability: Capability::McpClient,
        min_support: MinSupport::Emulated,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CapabilityRequirement = serde_json::from_str(&json).unwrap();
    assert_eq!(back.capability, Capability::McpClient);
    assert!(matches!(back.min_support, MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════
// WorkspaceSpec
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workspace_spec_roundtrip() {
    let ws = WorkspaceSpec {
        root: "/tmp".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec!["target".into()],
    };
    let json = serde_json::to_string(&ws).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.root, "/tmp");
    assert!(matches!(back.mode, WorkspaceMode::Staged));
}

// ═══════════════════════════════════════════════════════════════════════
// BackendIdentity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backend_identity_roundtrip() {
    let bi = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("2.0".into()),
    };
    let json = serde_json::to_string(&bi).unwrap();
    let back: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "sidecar:node");
    assert_eq!(back.backend_version.as_deref(), Some("1.0"));
    assert_eq!(back.adapter_version.as_deref(), Some("2.0"));
}

#[test]
fn backend_identity_optional_versions() {
    let bi = BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    };
    let json = serde_json::to_string(&bi).unwrap();
    let back: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert!(back.backend_version.is_none());
    assert!(back.adapter_version.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// Receipt & ReceiptBuilder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_defaults() {
    let r = ReceiptBuilder::new("test").build();
    assert_eq!(r.backend.id, "test");
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.mode, ExecutionMode::Mapped);
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert!(r.receipt_sha256.is_none());
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_backend_id() {
    let r = ReceiptBuilder::new("a").backend_id("b").build();
    assert_eq!(r.backend.id, "b");
}

#[test]
fn receipt_builder_outcome() {
    let r = ReceiptBuilder::new("x").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_mode() {
    let r = ReceiptBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_builder_backend_version() {
    let r = ReceiptBuilder::new("x").backend_version("1.0").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.0"));
}

#[test]
fn receipt_builder_adapter_version() {
    let r = ReceiptBuilder::new("x").adapter_version("2.0").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("2.0"));
}

#[test]
fn receipt_builder_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("x").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn receipt_builder_timestamps() {
    let start = Utc::now();
    let end = start + chrono::Duration::seconds(5);
    let r = ReceiptBuilder::new("x")
        .started_at(start)
        .finished_at(end)
        .build();
    assert_eq!(r.meta.started_at, start);
    assert_eq!(r.meta.finished_at, end);
    assert_eq!(r.meta.duration_ms, 5000);
}

#[test]
fn receipt_builder_usage_raw() {
    let r = ReceiptBuilder::new("x")
        .usage_raw(json!({"tokens": 100}))
        .build();
    assert_eq!(r.usage_raw["tokens"], 100);
}

#[test]
fn receipt_builder_usage() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("x").usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
}

#[test]
fn receipt_builder_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("x").capabilities(caps).build();
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn receipt_builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("x").verification(v).build();
    assert_eq!(r.verification.git_diff.as_deref(), Some("diff"));
    assert!(r.verification.harness_ok);
}

#[test]
fn receipt_builder_add_trace_event() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let r = ReceiptBuilder::new("x").add_trace_event(ev).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_add_artifact() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
    };
    let r = ReceiptBuilder::new("x").add_artifact(a).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn receipt_has_unique_run_id() {
    let a = make_receipt();
    let b = make_receipt();
    assert_ne!(a.meta.run_id, b.meta.run_id);
}

#[test]
fn receipt_serialization_roundtrip() {
    let r = make_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backend.id, "mock");
    assert_eq!(back.meta.run_id, r.meta.run_id);
}

// ═══════════════════════════════════════════════════════════════════════
// Receipt hashing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_hash_sets_sha256() {
    let r = make_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_is_deterministic() {
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_nulls_sha256_field() {
    // A receipt with an existing hash should produce the same hash
    // as one without, because receipt_hash nulls the field.
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("bogus".into());
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_hash_verifies_correctly() {
    let r = make_receipt().with_hash().unwrap();
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &recomputed);
}

#[test]
fn receipt_builder_with_hash_shortcut() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn receipt_hash_changes_with_outcome() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .work_order_id(r1.meta.work_order_id)
        .started_at(r1.meta.started_at)
        .finished_at(r1.meta.finished_at)
        .build();
    // run_id differs so hashes will differ
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════
// RunMetadata
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_metadata_roundtrip() {
    let now = Utc::now();
    let rm = RunMetadata {
        run_id: Uuid::new_v4(),
        work_order_id: Uuid::new_v4(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: now,
        finished_at: now,
        duration_ms: 42,
    };
    let json = serde_json::to_string(&rm).unwrap();
    let back: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_id, rm.run_id);
    assert_eq!(back.duration_ms, 42);
    assert_eq!(back.contract_version, CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════════
// UsageNormalized
// ═══════════════════════════════════════════════════════════════════════

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
fn usage_normalized_all_fields() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, Some(100));
    assert_eq!(back.request_units, Some(1));
}

// ═══════════════════════════════════════════════════════════════════════
// VerificationReport
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn verification_report_default() {
    let vr = VerificationReport::default();
    assert!(vr.git_diff.is_none());
    assert!(vr.git_status.is_none());
    assert!(!vr.harness_ok);
}

#[test]
fn verification_report_construction() {
    let vr = VerificationReport {
        git_diff: Some("diff --git".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&vr).unwrap();
    let back: VerificationReport = serde_json::from_str(&json).unwrap();
    assert!(back.harness_ok);
    assert_eq!(back.git_status.as_deref(), Some("M src/lib.rs"));
}

// ═══════════════════════════════════════════════════════════════════════
// ArtifactRef
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn artifact_ref_roundtrip() {
    let a = ArtifactRef {
        kind: "log".into(),
        path: "out.log".into(),
    };
    let json = serde_json::to_string(&a).unwrap();
    let back: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.kind, "log");
    assert_eq!(back.path, "out.log");
}

// ═══════════════════════════════════════════════════════════════════════
// AgentEvent & AgentEventKind — all variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn agent_event_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_run_completed() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn agent_event_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &back.kind {
        assert_eq!(text, "tok");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn agent_event_assistant_message() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "Hello".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &back.kind {
        assert_eq!(text, "Hello");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn agent_event_tool_call() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "a.rs"}),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        ..
    } = &back.kind
    {
        assert_eq!(tool_name, "read");
        assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn agent_event_tool_result() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu_1".into()),
        output: json!("file contents"),
        is_error: false,
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolResult {
        is_error, output, ..
    } = &back.kind
    {
        assert!(!is_error);
        assert_eq!(output, &json!("file contents"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn agent_event_tool_result_error() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("exit 1"),
        is_error: true,
    });
    if let AgentEventKind::ToolResult { is_error, .. } = &e.kind {
        assert!(is_error);
    }
}

#[test]
fn agent_event_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added fn".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn agent_event_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(0),
        output_preview: Some("file.rs".into()),
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &back.kind {
        assert_eq!(*exit_code, Some(0));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn agent_event_warning() {
    let e = make_event(AgentEventKind::Warning {
        message: "slow".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("warning"));
}

#[test]
fn agent_event_error() {
    let e = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &back.kind
    {
        assert_eq!(message, "boom");
        assert!(error_code.is_none());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn agent_event_error_with_code() {
    // Construct via JSON to avoid direct abp_error dependency in integration test
    let json_str = r#"{"ts":"2024-01-01T00:00:00Z","type":"error","message":"hash bad","error_code":"receipt_hash_mismatch"}"#;
    let e: AgentEvent = serde_json::from_str(json_str).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert!(error_code.is_some());
    } else {
        panic!("expected Error variant");
    }
}

#[test]
fn agent_event_ext_field() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("raw_message"));
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(back.ext.is_some());
}

#[test]
fn agent_event_ext_none_omitted() {
    let e = make_event(AgentEventKind::Warning {
        message: "w".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("ext"));
}

#[test]
fn agent_event_timestamp_preserved() {
    let ts = Utc::now();
    let e = AgentEvent {
        ts,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ts, ts);
}

#[test]
fn agent_event_kind_tag_is_type() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("type").is_some(), "discriminator should be 'type'");
    assert_eq!(v["type"], "run_started");
}

// ═══════════════════════════════════════════════════════════════════════
// CapabilityManifest (type alias)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_is_btreemap() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    assert_eq!(m.len(), 2);
}

#[test]
fn capability_manifest_serialization() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert!(back.contains_key(&Capability::Streaming));
}

// ═══════════════════════════════════════════════════════════════════════
// canonical_json
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn canonical_json_sorts_keys() {
    let j = canonical_json(&json!({"b": 2, "a": 1})).unwrap();
    assert!(j.starts_with(r#"{"a":1"#));
}

#[test]
fn canonical_json_deterministic() {
    let val = json!({"z": 3, "m": 2, "a": 1});
    let a = canonical_json(&val).unwrap();
    let b = canonical_json(&val).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_handles_nested() {
    let val = json!({"outer": {"b": 2, "a": 1}});
    let j = canonical_json(&val).unwrap();
    assert!(j.contains(r#""a":1"#));
}

// ═══════════════════════════════════════════════════════════════════════
// sha256_hex
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sha256_hex_length() {
    let h = sha256_hex(b"hello");
    assert_eq!(h.len(), 64);
}

#[test]
fn sha256_hex_known_value() {
    // SHA-256 of "hello" is well-known
    let h = sha256_hex(b"hello");
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_hex_empty() {
    let h = sha256_hex(b"");
    assert_eq!(
        h,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_deterministic() {
    assert_eq!(sha256_hex(b"test"), sha256_hex(b"test"));
}

// ═══════════════════════════════════════════════════════════════════════
// ContractError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn contract_error_json_variant() {
    let bad: Result<serde_json::Value, _> = serde_json::from_str("not json");
    let err = ContractError::from(bad.unwrap_err());
    let msg = format!("{err}");
    assert!(msg.contains("serialize") || msg.contains("JSON") || msg.contains("expected"));
}

#[test]
fn contract_error_display() {
    let e = ContractError::Json(serde_json::from_str::<()>("bad").unwrap_err());
    let s = format!("{e}");
    assert!(!s.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// validate module
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_valid_receipt() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(abp_core::validate::validate_receipt(&r).is_ok());
}

#[test]
fn validate_empty_backend_id() {
    let r = ReceiptBuilder::new("").outcome(Outcome::Complete).build();
    let errs = abp_core::validate::validate_receipt(&r).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, abp_core::validate::ValidationError::EmptyBackendId)));
}

#[test]
fn validate_bad_hash() {
    let mut r = make_receipt();
    r.receipt_sha256 = Some("bad_hash".into());
    let errs = abp_core::validate::validate_receipt(&r).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, abp_core::validate::ValidationError::InvalidHash { .. })));
}

// ═══════════════════════════════════════════════════════════════════════
// Full Receipt with trace + artifacts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_full_construction_and_hash() {
    let ev1 = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let ev2 = make_event(AgentEventKind::AssistantMessage {
        text: "done".into(),
    });
    let ev3 = make_event(AgentEventKind::RunCompleted {
        message: "ok".into(),
    });
    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
    };
    let r = ReceiptBuilder::new("full-test")
        .outcome(Outcome::Complete)
        .backend_version("1.0.0")
        .adapter_version("0.1.0")
        .usage_raw(json!({"total": 300}))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            ..Default::default()
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M f.rs".into()),
            harness_ok: true,
        })
        .add_trace_event(ev1)
        .add_trace_event(ev2)
        .add_trace_event(ev3)
        .add_artifact(artifact)
        .with_hash()
        .unwrap();

    assert_eq!(r.trace.len(), 3);
    assert_eq!(r.artifacts.len(), 1);
    assert!(r.receipt_sha256.is_some());

    // Roundtrip the full receipt
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.trace.len(), 3);
    assert_eq!(back.receipt_sha256, r.receipt_sha256);
}

// ═══════════════════════════════════════════════════════════════════════
// CapabilityManifest satisfies logic via negotiate module
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn negotiation_all_required_satisfied() {
    use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest};

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::ToolWrite, SupportLevel::Emulated);

    let req = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![Capability::ToolWrite],
        minimum_support: SupportLevel::Emulated,
    };
    let result = CapabilityNegotiator::negotiate(&req, &manifest);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 1);
    assert_eq!(result.bonus.len(), 1);
}

#[test]
fn negotiation_unsatisfied_required() {
    use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest};

    let manifest = CapabilityManifest::new();
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &manifest);
    assert!(!result.is_compatible);
    assert_eq!(result.unsatisfied.len(), 1);
}

#[test]
fn negotiation_best_match() {
    use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest};

    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolRead, SupportLevel::Native);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::ToolRead, SupportLevel::Native);
    m2.insert(Capability::Streaming, SupportLevel::Native);

    let req = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![Capability::Streaming],
        minimum_support: SupportLevel::Emulated,
    };

    let manifests = vec![("backend_a", m1), ("backend_b", m2)];
    let (name, _) = CapabilityNegotiator::best_match(&req, &manifests).unwrap();
    assert_eq!(name, "backend_b");
}

// ═══════════════════════════════════════════════════════════════════════
// IR types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_all_variants_roundtrip() {
    use abp_core::ir::IrRole;
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let j = serde_json::to_value(role).unwrap();
        let back: IrRole = serde_json::from_value(j).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn ir_message_text_helper() {
    use abp_core::ir::{IrMessage, IrRole};
    let m = IrMessage::text(IrRole::User, "hello");
    assert!(m.is_text_only());
    assert_eq!(m.text_content(), "hello");
}

#[test]
fn ir_usage_from_io() {
    use abp_core::ir::IrUsage;
    let u = IrUsage::from_io(100, 200);
    assert_eq!(u.total_tokens, 300);
    assert_eq!(u.cache_read_tokens, 0);
}

#[test]
fn ir_usage_with_cache() {
    use abp_core::ir::IrUsage;
    let u = IrUsage::with_cache(100, 200, 50, 25);
    assert_eq!(u.total_tokens, 300);
    assert_eq!(u.cache_read_tokens, 50);
    assert_eq!(u.cache_write_tokens, 25);
}

#[test]
fn ir_usage_merge() {
    use abp_core::ir::IrUsage;
    let a = IrUsage::from_io(100, 200);
    let b = IrUsage::from_io(50, 50);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 150);
    assert_eq!(merged.output_tokens, 250);
    assert_eq!(merged.total_tokens, 400);
}

#[test]
fn ir_conversation_push_and_len() {
    use abp_core::ir::{IrConversation, IrMessage, IrRole};
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful"))
        .push(IrMessage::text(IrRole::User, "Hi"));
    assert_eq!(conv.len(), 2);
    assert!(!conv.is_empty());
}

#[test]
fn ir_conversation_system_message() {
    use abp_core::ir::{IrConversation, IrMessage, IrRole};
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "usr"));
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════
// error module
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_code_has_code_string() {
    use abp_core::error::ErrorCode;
    let c = ErrorCode::InvalidHash;
    let code = c.code();
    assert!(code.starts_with("ABP-"));
}

#[test]
fn error_code_has_category() {
    use abp_core::error::ErrorCode;
    let cat = ErrorCode::InvalidHash.category();
    assert!(!cat.is_empty());
}

#[test]
fn error_code_has_description() {
    use abp_core::error::ErrorCode;
    let desc = ErrorCode::InvalidHash.description();
    assert!(!desc.is_empty());
}

#[test]
fn error_catalog_all_returns_entries() {
    use abp_core::error::ErrorCatalog;
    let all = ErrorCatalog::all();
    assert!(!all.is_empty());
}

#[test]
fn error_catalog_lookup_known_code() {
    use abp_core::error::ErrorCatalog;
    // Each ErrorCode has a code() string; lookup should find it
    let all = abp_core::error::ErrorCatalog::all();
    for ec in &all {
        let code_str = ec.code();
        let found = ErrorCatalog::lookup(code_str);
        assert!(found.is_some(), "lookup failed for {code_str}");
    }
}

#[test]
fn error_info_construction() {
    use abp_core::error::{ErrorCode, ErrorInfo};
    let info = ErrorInfo::new(ErrorCode::IoError, "disk full").with_context("device", "/dev/sda");
    assert_eq!(info.code, ErrorCode::IoError);
}

// ═══════════════════════════════════════════════════════════════════════
// config module
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_defaults_values() {
    use abp_core::config::ConfigDefaults;
    assert!(ConfigDefaults::default_max_turns() > 0);
    assert!(ConfigDefaults::default_max_budget() > 0.0);
    assert!(!ConfigDefaults::default_model().is_empty());
}

#[test]
fn config_apply_defaults() {
    use abp_core::config::ConfigDefaults;
    let mut wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_turns.is_none());
    ConfigDefaults::apply_defaults(&mut wo);
    assert!(wo.config.max_turns.is_some());
}

#[test]
fn config_validator_warns_on_issues() {
    use abp_core::config::ConfigValidator;
    let wo = WorkOrderBuilder::new("").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    // Empty task should produce at least one warning
    assert!(!warnings.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// stream module
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_stream_basic() {
    use abp_core::stream::EventStream;
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let stream = EventStream::new(events);
    assert_eq!(stream.len(), 3);
    assert!(!stream.is_empty());
}

#[test]
fn event_stream_by_kind() {
    use abp_core::stream::EventStream;
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::Warning {
            message: "w1".into(),
        }),
        make_event(AgentEventKind::Warning {
            message: "w2".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let warnings = stream.by_kind("warning");
    assert_eq!(warnings.len(), 2);
}

#[test]
fn event_stream_count_by_kind() {
    use abp_core::stream::EventStream;
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "a".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "b".into() }),
    ];
    let stream = EventStream::new(events);
    let counts = stream.count_by_kind();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("run_started"), Some(&1));
}

// ═══════════════════════════════════════════════════════════════════════
// aggregate module
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_aggregator_basic() {
    use abp_core::aggregate::EventAggregator;
    let mut agg = EventAggregator::new();
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    });
    agg.add(&ev);
    assert_eq!(agg.event_count(), 1);
    assert_eq!(agg.unique_tool_count(), 1);
    assert!(!agg.has_errors());
}

#[test]
fn event_aggregator_detects_errors() {
    use abp_core::aggregate::EventAggregator;
    let mut agg = EventAggregator::new();
    agg.add(&make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    }));
    assert!(agg.has_errors());
    assert_eq!(agg.error_messages().len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// CapabilityDiff (negotiate module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_diff_detects_changes() {
    use abp_core::negotiate::CapabilityDiff;
    let mut old = CapabilityManifest::new();
    old.insert(Capability::Streaming, SupportLevel::Native);
    old.insert(Capability::ToolRead, SupportLevel::Native);

    let mut new = CapabilityManifest::new();
    new.insert(Capability::ToolRead, SupportLevel::Emulated);
    new.insert(Capability::ToolWrite, SupportLevel::Native);

    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.contains(&Capability::ToolWrite));
    assert!(diff.removed.contains(&Capability::Streaming));
}

// ═══════════════════════════════════════════════════════════════════════
// MappingError (error module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_fidelity_loss() {
    use abp_core::error::MappingError;
    let e = MappingError::FidelityLoss {
        field: "extended_thinking".into(),
        source_dialect: "anthropic".into(),
        target_dialect: "openai".into(),
        detail: "not supported".into(),
    };
    assert!(e.is_degraded());
    assert!(!e.is_fatal());
    assert_eq!(e.code(), MappingError::FIDELITY_LOSS_CODE);
}

#[test]
fn mapping_error_unsupported_capability() {
    use abp_core::error::MappingError;
    let e = MappingError::UnsupportedCapability {
        capability: "logprobs".into(),
        dialect: "anthropic".into(),
    };
    assert!(e.is_fatal());
    assert_eq!(e.code(), MappingError::UNSUPPORTED_CAP_CODE);
}

// ═══════════════════════════════════════════════════════════════════════
// Extension traits (ext module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_ext_is_code_task() {
    use abp_core::ext::WorkOrderExt;
    let wo = WorkOrderBuilder::new("fix the auth bug").build();
    assert!(wo.is_code_task());

    let wo2 = WorkOrderBuilder::new("write a poem").build();
    assert!(!wo2.is_code_task());
}

#[test]
fn work_order_ext_task_summary() {
    use abp_core::ext::WorkOrderExt;
    let wo = WorkOrderBuilder::new("a very long task description that goes on and on").build();
    let summary = wo.task_summary(10);
    assert!(summary.len() <= 13); // 10 + "..."
}

#[test]
fn work_order_ext_has_capability() {
    use abp_core::ext::WorkOrderExt;
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert!(wo.has_capability(&Capability::Streaming));
    assert!(!wo.has_capability(&Capability::ToolRead));
}

#[test]
fn receipt_ext_trait() {
    use abp_core::ext::ReceiptExt;
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }))
        .build();
    assert!(r.is_success());
    assert_eq!(r.total_tool_calls(), 1);
}
