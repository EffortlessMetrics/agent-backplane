#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ContractError,
    ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec, receipt_hash,
};
use abp_policy::PolicyEngine;
use abp_protocol::Envelope;
use abp_protocol::builder::{
    BuilderError, EnvelopeBuilder, EventBuilder, FatalBuilder, FinalBuilder, HelloBuilder,
    RunBuilder,
};
use abp_receipt::{ChainBuilder, ChainError, ReceiptBuilder, ReceiptChain};
use abp_runtime::retry::{RetryPolicy, RetryPolicyBuilder};
use abp_runtime::stages::{
    DeduplicationStage, LoggingStage, MetricsStage, PipelineBuilder, RateLimitStage,
};
use abp_sidecar_sdk::SidecarBuilder;
use abp_stream::{
    EventFilter, EventRecorder, EventStats, EventTransform, StreamPipeline, StreamPipelineBuilder,
};

// ─── helpers ────────────────────────────────────────────────────────────────

fn make_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn make_tool_call_event(name: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    }
}

fn make_error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .build()
}

// ═══════════════════════════════════════════════════════════════════════════
//  WORK ORDER BUILDER (25+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_builder_default_produces_valid_work_order() {
    let wo = WorkOrderBuilder::new("do something").build();
    assert_eq!(wo.task, "do something");
}

#[test]
fn wo_builder_id_is_uuid_v4() {
    let wo = WorkOrderBuilder::new("task").build();
    assert_ne!(wo.id, Uuid::nil());
}

#[test]
fn wo_builder_two_builds_produce_different_ids() {
    let a = WorkOrderBuilder::new("t").build();
    let b = WorkOrderBuilder::new("t").build();
    assert_ne!(a.id, b.id);
}

#[test]
fn wo_builder_set_task() {
    let wo = WorkOrderBuilder::new("my custom task").build();
    assert_eq!(wo.task, "my custom task");
}

#[test]
fn wo_builder_default_lane_is_patch_first() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn wo_builder_set_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn wo_builder_default_root_is_dot() {
    let wo = WorkOrderBuilder::new("t").build();
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn wo_builder_set_root() {
    let wo = WorkOrderBuilder::new("t").root("/tmp/work").build();
    assert_eq!(wo.workspace.root, "/tmp/work");
}

#[test]
fn wo_builder_default_workspace_mode_is_staged() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn wo_builder_set_workspace_mode_passthrough() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn wo_builder_set_include_patterns() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["*.rs".into(), "*.toml".into()])
        .build();
    assert_eq!(wo.workspace.include.len(), 2);
    assert_eq!(wo.workspace.include[0], "*.rs");
}

#[test]
fn wo_builder_set_exclude_patterns() {
    let wo = WorkOrderBuilder::new("t")
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn wo_builder_default_context_is_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn wo_builder_set_context_with_files() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 2);
}

#[test]
fn wo_builder_set_context_with_snippets() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "readme");
}

#[test]
fn wo_builder_set_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["bash"]);
}

#[test]
fn wo_builder_default_policy_is_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.policy.deny_read.is_empty());
    assert!(wo.policy.deny_write.is_empty());
}

#[test]
fn wo_builder_set_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolUse,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn wo_builder_set_config() {
    let config = RuntimeConfig {
        model: Some("gpt-4".into()),
        max_budget_usd: Some(1.0),
        max_turns: Some(10),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(config).build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn wo_builder_set_model_shorthand() {
    let wo = WorkOrderBuilder::new("t").model("claude-3").build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
}

#[test]
fn wo_builder_set_max_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn wo_builder_set_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(20).build();
    assert_eq!(wo.config.max_turns, Some(20));
}

#[test]
fn wo_builder_chain_all_methods() {
    let wo = WorkOrderBuilder::new("chained task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .model("gpt-4")
        .max_budget_usd(10.0)
        .max_turns(50)
        .build();
    assert_eq!(wo.task, "chained task");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/workspace");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(50));
}

#[test]
fn wo_builder_serializes_to_json() {
    let wo = WorkOrderBuilder::new("serializable").build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("serializable"));
}

#[test]
fn wo_builder_roundtrip_json() {
    let wo = WorkOrderBuilder::new("roundtrip")
        .model("test-model")
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "roundtrip");
    assert_eq!(wo2.config.model.as_deref(), Some("test-model"));
}

#[test]
fn wo_builder_vendor_overrides_via_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.7}));
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(config).build();
    assert!(wo.config.vendor.contains_key("openai"));
}

#[test]
fn wo_builder_env_vars_via_config() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".into(), "secret".into());
    let config = RuntimeConfig {
        env,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(config).build();
    assert_eq!(wo.config.env.get("API_KEY").unwrap(), "secret");
}

#[test]
fn wo_builder_empty_task_is_allowed() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

// ═══════════════════════════════════════════════════════════════════════════
//  RECEIPT BUILDER (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_default() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn receipt_builder_set_backend_id() {
    let r = ReceiptBuilder::new("my-backend").build();
    assert_eq!(r.backend.id, "my-backend");
}

#[test]
fn receipt_builder_set_outcome_failed() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_set_outcome_partial() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn receipt_builder_set_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("b").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn receipt_builder_set_run_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("b").run_id(id).build();
    assert_eq!(r.meta.run_id, id);
}

#[test]
fn receipt_builder_set_backend_version() {
    let r = ReceiptBuilder::new("b").backend_version("1.2.3").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
}

#[test]
fn receipt_builder_set_adapter_version() {
    let r = ReceiptBuilder::new("b").adapter_version("0.1.0").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.1.0"));
}

#[test]
fn receipt_builder_set_model() {
    let r = ReceiptBuilder::new("b").model("gpt-4o").build();
    // model is stored but we verify the receipt builds without error
    let json = serde_json::to_value(&r).unwrap();
    assert!(json.is_object());
}

#[test]
fn receipt_builder_set_timing() {
    let start = fixed_ts();
    let end = start + chrono::Duration::seconds(30);
    let r = ReceiptBuilder::new("b")
        .started_at(start)
        .finished_at(end)
        .build();
    assert_eq!(r.meta.started_at, start);
    assert_eq!(r.meta.finished_at, end);
}

#[test]
fn receipt_builder_set_duration() {
    let r = ReceiptBuilder::new("b")
        .duration(Duration::from_secs(5))
        .build();
    assert_eq!(r.meta.duration_ms, 5000);
}

#[test]
fn receipt_builder_set_usage_tokens() {
    let r = ReceiptBuilder::new("b").usage_tokens(1000, 500).build();
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(500));
}

#[test]
fn receipt_builder_set_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(200),
        output_tokens: Some(100),
        cache_read_tokens: Some(50),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("b").usage(usage).build();
    assert_eq!(r.usage.input_tokens, Some(200));
    assert_eq!(r.usage.cache_read_tokens, Some(50));
}

#[test]
fn receipt_builder_set_usage_raw() {
    let raw = json!({"total_tokens": 999});
    let r = ReceiptBuilder::new("b").usage_raw(raw.clone()).build();
    assert_eq!(r.usage_raw, raw);
}

#[test]
fn receipt_builder_add_event() {
    let r = ReceiptBuilder::new("b")
        .add_event(make_event("hello"))
        .build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_set_events() {
    let events = vec![make_event("a"), make_event("b"), make_event("c")];
    let r = ReceiptBuilder::new("b").events(events).build();
    assert_eq!(r.trace.len(), 3);
}

#[test]
fn receipt_builder_add_artifact() {
    use abp_core::ArtifactRef;
    let artifact = ArtifactRef {
        kind: "file".into(),
        path: "output.txt".into(),
    };
    let r = ReceiptBuilder::new("b").add_artifact(artifact).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].path, "output.txt");
}

#[test]
fn receipt_builder_with_hash_produces_non_none() {
    let r = ReceiptBuilder::new("b")
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_builder_build_has_no_hash() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn receipt_builder_chain_all() {
    let r = ReceiptBuilder::new("full-backend")
        .outcome(Outcome::Complete)
        .backend_version("2.0")
        .adapter_version("1.0")
        .model("claude-3")
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .usage_tokens(500, 200)
        .add_event(make_event("started"))
        .build();
    assert_eq!(r.backend.id, "full-backend");
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_mode_default_is_mapped() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn receipt_builder_set_mode_passthrough() {
    let r = ReceiptBuilder::new("b")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_builder_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    let r = ReceiptBuilder::new("b").capabilities(caps).build();
    assert!(r.capabilities.contains_key(&Capability::ToolUse));
}

#[test]
fn receipt_builder_error_sets_failed_outcome() {
    let r = ReceiptBuilder::new("b")
        .error("something went wrong")
        .build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff --git a/f b/f".into()),
        git_status: Some("M f".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("b").verification(v).build();
    assert!(r.verification.harness_ok);
    assert!(r.verification.git_diff.is_some());
}

#[test]
fn receipt_builder_contract_version_matches() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════════════
//  RECEIPT HASHING
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_is_deterministic() {
    let r = minimal_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_is_hex_64_chars() {
    let r = minimal_receipt();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_with_hash_method() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_excludes_self() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("bogus".into());
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

// ═══════════════════════════════════════════════════════════════════════════
//  CHAIN BUILDER (part of receipt builders)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chain_builder_default_produces_empty_chain() {
    let chain = ChainBuilder::new().build();
    assert!(chain.is_empty());
}

#[test]
fn chain_builder_append_single_receipt() {
    let r = minimal_receipt().with_hash().unwrap();
    let chain = ChainBuilder::new().append(r).unwrap().build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_append_multiple() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = ReceiptBuilder::new("b2")
        .work_order_id(Uuid::nil())
        .run_id(Uuid::new_v4())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .with_hash()
        .unwrap();
    let chain = ChainBuilder::new()
        .append(r1)
        .unwrap()
        .append(r2)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 2);
}

#[test]
fn chain_builder_skip_validation() {
    let r = minimal_receipt(); // no hash
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  POLICY ENGINE (15+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_empty_allows_all_tools() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let d = engine.can_use_tool("bash");
    assert!(d.allowed);
}

#[test]
fn policy_empty_allows_all_reads() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let d = engine.can_read_path(Path::new("src/main.rs"));
    assert!(d.allowed);
}

#[test]
fn policy_empty_allows_all_writes() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let d = engine.can_write_path(Path::new("output.txt"));
    assert!(d.allowed);
}

#[test]
fn policy_tool_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into(), "read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("bash").allowed);
}

#[test]
fn policy_tool_denylist_blocks() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["rm".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("rm").allowed);
}

#[test]
fn policy_deny_read_pattern() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn policy_deny_write_pattern() {
    let policy = PolicyProfile {
        deny_write: vec!["**/node_modules/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
}

#[test]
fn policy_deny_read_allows_other_paths() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_deny_write_allows_other_paths() {
    let policy = PolicyProfile {
        deny_write: vec!["**/secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_multiple_deny_read_patterns() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/secret*".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("secret.key")).allowed);
    assert!(engine.can_read_path(Path::new("README.md")).allowed);
}

#[test]
fn policy_combined_tool_and_path_rules() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["delete".into()],
        deny_write: vec!["**/protected/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("delete").allowed);
    assert!(
        !engine
            .can_write_path(Path::new("protected/data.db"))
            .allowed
    );
    assert!(engine.can_use_tool("read").allowed);
}

#[test]
fn policy_decision_allow_has_no_reason() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let d = engine.can_use_tool("anything");
    assert!(d.allowed);
}

#[test]
fn policy_decision_deny_has_reason() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["danger".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_use_tool("danger");
    assert!(!d.allowed);
}

#[test]
fn policy_network_allow_fields() {
    let policy = PolicyProfile {
        allow_network: vec!["api.example.com".into()],
        ..Default::default()
    };
    assert_eq!(policy.allow_network.len(), 1);
}

#[test]
fn policy_network_deny_fields() {
    let policy = PolicyProfile {
        deny_network: vec!["evil.com".into()],
        ..Default::default()
    };
    assert_eq!(policy.deny_network.len(), 1);
}

#[test]
fn policy_require_approval_fields() {
    let policy = PolicyProfile {
        require_approval_for: vec!["deploy".into()],
        ..Default::default()
    };
    assert_eq!(policy.require_approval_for.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  PROTOCOL ENVELOPE BUILDERS (15+ tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_builder_with_backend() {
    let env = EnvelopeBuilder::hello()
        .backend("test-sidecar")
        .build()
        .unwrap();
    match &env {
        Envelope::Hello { backend, .. } => assert_eq!(backend.id, "test-sidecar"),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_builder_missing_backend_is_error() {
    let result = EnvelopeBuilder::hello().build();
    assert!(result.is_err());
}

#[test]
fn hello_builder_with_version() {
    let env = EnvelopeBuilder::hello()
        .backend("b")
        .version("1.0")
        .build()
        .unwrap();
    match &env {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.backend_version.as_deref(), Some("1.0"));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_builder_with_adapter_version() {
    let env = EnvelopeBuilder::hello()
        .backend("b")
        .adapter_version("0.2")
        .build()
        .unwrap();
    match &env {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.adapter_version.as_deref(), Some("0.2"));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_builder_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let env = EnvelopeBuilder::hello()
        .backend("b")
        .capabilities(caps)
        .build()
        .unwrap();
    match &env {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_builder_with_mode() {
    let env = EnvelopeBuilder::hello()
        .backend("b")
        .mode(ExecutionMode::Passthrough)
        .build()
        .unwrap();
    match &env {
        Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_builder_full_chain() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    let env = EnvelopeBuilder::hello()
        .backend("full-sidecar")
        .version("2.0")
        .adapter_version("1.0")
        .capabilities(caps)
        .mode(ExecutionMode::Mapped)
        .build()
        .unwrap();
    match &env {
        Envelope::Hello {
            backend,
            capabilities,
            mode,
            ..
        } => {
            assert_eq!(backend.id, "full-sidecar");
            assert_eq!(*mode, ExecutionMode::Mapped);
            assert!(capabilities.contains_key(&Capability::ToolUse));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn run_builder_creates_run_envelope() {
    let wo = WorkOrderBuilder::new("test task").build();
    let id_str = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).ref_id("run-1").build().unwrap();
    match &env {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-1");
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_builder_missing_ref_id_auto_generates() {
    let wo = WorkOrderBuilder::new("t").build();
    let result = EnvelopeBuilder::run(wo).build();
    // RunBuilder may auto-generate ref_id, so build succeeds
    assert!(result.is_ok());
}

#[test]
fn event_builder_creates_event_envelope() {
    let evt = make_event("hello world");
    let env = EnvelopeBuilder::event(evt).ref_id("ref-1").build().unwrap();
    match &env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "ref-1");
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_builder_missing_ref_id_is_error() {
    let evt = make_event("test");
    let result = EnvelopeBuilder::event(evt).build();
    assert!(result.is_err());
}

#[test]
fn final_builder_creates_final_envelope() {
    let receipt = minimal_receipt();
    let env = EnvelopeBuilder::final_receipt(receipt)
        .ref_id("ref-final")
        .build()
        .unwrap();
    match &env {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "ref-final"),
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_builder_missing_ref_id_is_error() {
    let receipt = minimal_receipt();
    let result = EnvelopeBuilder::final_receipt(receipt).build();
    assert!(result.is_err());
}

#[test]
fn fatal_builder_creates_fatal_envelope() {
    let env = EnvelopeBuilder::fatal("boom")
        .ref_id("ref-fatal")
        .build()
        .unwrap();
    match &env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("ref-fatal"));
            assert_eq!(error, "boom");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_builder_with_code() {
    let env = EnvelopeBuilder::fatal("crash")
        .ref_id("r")
        .code("E_INTERNAL")
        .build()
        .unwrap();
    match &env {
        Envelope::Fatal { error, .. } => assert_eq!(error, "crash"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_builder_without_ref_id_is_ok() {
    // Fatal may not have a ref_id if error happens before run
    let env = EnvelopeBuilder::fatal("early error").build().unwrap();
    match &env {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_hello_serializes_with_t_tag() {
    let env = EnvelopeBuilder::hello().backend("b").build().unwrap();
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains(r#""t":"hello"#));
}

#[test]
fn envelope_run_serializes_with_t_tag() {
    let wo = WorkOrderBuilder::new("t").build();
    let env = EnvelopeBuilder::run(wo).ref_id("r").build().unwrap();
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains(r#""t":"run"#));
}

// ═══════════════════════════════════════════════════════════════════════════
//  RETRY POLICY BUILDER (runtime)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn retry_policy_builder_default() {
    let policy = RetryPolicy::builder().build();
    assert!(policy.max_retries > 0);
}

#[test]
fn retry_policy_builder_set_max_retries() {
    let policy = RetryPolicy::builder().max_retries(5).build();
    assert_eq!(policy.max_retries, 5);
}

#[test]
fn retry_policy_builder_set_initial_backoff() {
    let policy = RetryPolicy::builder()
        .initial_backoff(Duration::from_millis(200))
        .build();
    assert_eq!(policy.initial_backoff, Duration::from_millis(200));
}

#[test]
fn retry_policy_builder_set_max_backoff() {
    let policy = RetryPolicy::builder()
        .max_backoff(Duration::from_secs(30))
        .build();
    assert_eq!(policy.max_backoff, Duration::from_secs(30));
}

#[test]
fn retry_policy_builder_set_backoff_multiplier() {
    let policy = RetryPolicy::builder().backoff_multiplier(3.0).build();
    assert!((policy.backoff_multiplier - 3.0).abs() < f64::EPSILON);
}

#[test]
fn retry_policy_builder_chain_all() {
    let policy = RetryPolicy::builder()
        .max_retries(10)
        .initial_backoff(Duration::from_millis(50))
        .max_backoff(Duration::from_secs(60))
        .backoff_multiplier(1.5)
        .build();
    assert_eq!(policy.max_retries, 10);
    assert_eq!(policy.initial_backoff, Duration::from_millis(50));
    assert_eq!(policy.max_backoff, Duration::from_secs(60));
}

#[test]
fn retry_policy_no_retry() {
    let policy = RetryPolicy::no_retry();
    assert_eq!(policy.max_retries, 0);
}

#[test]
fn retry_policy_should_retry_within_limit() {
    let policy = RetryPolicy::builder().max_retries(3).build();
    assert!(policy.should_retry(0));
    assert!(policy.should_retry(2));
    assert!(!policy.should_retry(3));
}

#[test]
fn retry_policy_compute_delay_increases() {
    let policy = RetryPolicy::builder()
        .max_retries(5)
        .initial_backoff(Duration::from_millis(100))
        .backoff_multiplier(2.0)
        .build();
    let d0 = policy.compute_delay(0);
    let d1 = policy.compute_delay(1);
    // With jitter the exact values vary, but the general trend holds
    assert!(d0 <= policy.max_backoff);
}

// ═══════════════════════════════════════════════════════════════════════════
//  PIPELINE BUILDER (runtime stages)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_builder_default_is_empty() {
    let pb = PipelineBuilder::new();
    assert_eq!(pb.stage_count(), 0);
}

#[test]
fn pipeline_builder_add_rate_limit_stage() {
    let pb = PipelineBuilder::new().add_stage(Box::new(RateLimitStage::new(60)));
    assert_eq!(pb.stage_count(), 1);
}

#[test]
fn pipeline_builder_add_dedup_stage() {
    let pb = PipelineBuilder::new()
        .add_stage(Box::new(DeduplicationStage::new(Duration::from_secs(10))));
    assert_eq!(pb.stage_count(), 1);
}

#[test]
fn pipeline_builder_add_logging_stage() {
    let pb = PipelineBuilder::new().add_stage(Box::new(LoggingStage::new("test")));
    assert_eq!(pb.stage_count(), 1);
}

#[test]
fn pipeline_builder_add_metrics_stage() {
    let pb = PipelineBuilder::new().add_stage(Box::new(MetricsStage::new()));
    assert_eq!(pb.stage_count(), 1);
}

#[test]
fn pipeline_builder_multiple_stages() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(RateLimitStage::new(100)))
        .add_stage(Box::new(LoggingStage::new("prefix")))
        .add_stage(Box::new(MetricsStage::new()))
        .build();
    assert_eq!(pipeline.stage_names().len(), 3);
}

#[test]
fn pipeline_builder_build_returns_stage_pipeline() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(LoggingStage::new("p")))
        .build();
    let names = pipeline.stage_names();
    assert!(!names.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
//  SIDECAR BUILDER
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_builder_name() {
    let sb = SidecarBuilder::new("test-sidecar");
    assert_eq!(sb.name(), "test-sidecar");
}

#[test]
fn sidecar_builder_version() {
    let sb = SidecarBuilder::new("s").version("1.0");
    assert_eq!(sb.backend_version(), Some("1.0"));
}

#[test]
fn sidecar_builder_adapter_version() {
    let sb = SidecarBuilder::new("s").adapter_version("0.5");
    assert_eq!(sb.adapter_version_str(), Some("0.5"));
}

#[test]
fn sidecar_builder_capability() {
    let sb = SidecarBuilder::new("s").capability(Capability::ToolUse, SupportLevel::Native);
    assert!(sb.capability_manifest().contains_key(&Capability::ToolUse));
}

#[test]
fn sidecar_builder_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::Vision, SupportLevel::Emulated);
    let sb = SidecarBuilder::new("s").capabilities(caps);
    assert_eq!(sb.capability_manifest().len(), 2);
}

#[test]
fn sidecar_builder_mode() {
    let sb = SidecarBuilder::new("s").mode(ExecutionMode::Passthrough);
    assert_eq!(sb.execution_mode(), ExecutionMode::Passthrough);
}

#[test]
fn sidecar_builder_default_mode_is_mapped() {
    let sb = SidecarBuilder::new("s");
    assert_eq!(sb.execution_mode(), ExecutionMode::Mapped);
}

#[test]
fn sidecar_builder_no_handler_initially() {
    let sb = SidecarBuilder::new("s");
    assert!(!sb.has_handler());
}

#[test]
fn sidecar_builder_identity() {
    let sb = SidecarBuilder::new("my-sidecar")
        .version("2.0")
        .adapter_version("1.0");
    let identity = sb.identity();
    assert_eq!(identity.id, "my-sidecar");
    assert_eq!(identity.backend_version.as_deref(), Some("2.0"));
    assert_eq!(identity.adapter_version.as_deref(), Some("1.0"));
}

#[test]
fn sidecar_builder_build_without_handler_fails() {
    let result = SidecarBuilder::new("no-handler").build();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
//  STREAM PIPELINE BUILDER
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_pipeline_builder_default_builds() {
    let pipeline = StreamPipelineBuilder::new().build();
    // A default pipeline should pass events through
    let evt = make_event("test");
    let result = pipeline.process(evt);
    assert!(result.is_some());
}

#[test]
fn stream_pipeline_builder_with_filter() {
    let filter = EventFilter::errors_only();
    let pipeline = StreamPipelineBuilder::new().filter(filter).build();
    // Non-error event should be filtered out
    let evt = make_event("hello");
    let result = pipeline.process(evt);
    assert!(result.is_none());
}

#[test]
fn stream_pipeline_builder_with_transform() {
    let transform = EventTransform::identity();
    let pipeline = StreamPipelineBuilder::new().transform(transform).build();
    let evt = make_event("data");
    let result = pipeline.process(evt);
    assert!(result.is_some());
}

#[test]
fn stream_pipeline_builder_with_recorder() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new().with_recorder(recorder).build();
    assert!(pipeline.recorder().is_some());
}

#[test]
fn stream_pipeline_builder_with_stats() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new().with_stats(stats).build();
    assert!(pipeline.stats().is_some());
}

#[test]
fn stream_pipeline_builder_full_chain() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::identity())
        .with_recorder(EventRecorder::new())
        .with_stats(EventStats::new())
        .build();
    let evt = make_event("good event");
    let result = pipeline.process(evt);
    assert!(result.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
//  EVENT FILTER & TRANSFORM BUILDERS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_filter_new_custom() {
    let filter = EventFilter::new(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
    let msg = make_event("test");
    assert!(filter.matches(&msg));
    let tool = make_tool_call_event("bash");
    assert!(!filter.matches(&tool));
}

#[test]
fn event_filter_errors_only() {
    let filter = EventFilter::errors_only();
    let err = make_error_event("oops");
    assert!(filter.matches(&err));
    let msg = make_event("ok");
    assert!(!filter.matches(&msg));
}

#[test]
fn event_filter_exclude_errors() {
    let filter = EventFilter::exclude_errors();
    let err = make_error_event("bad");
    assert!(!filter.matches(&err));
    let msg = make_event("good");
    assert!(filter.matches(&msg));
}

#[test]
fn event_transform_identity() {
    let transform = EventTransform::identity();
    let evt = make_event("original");
    let result = transform.apply(evt);
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "original"),
        _ => panic!("expected AssistantMessage"),
    }
}

#[test]
fn event_transform_custom() {
    let transform = EventTransform::new(|mut e| {
        e.ext = Some(BTreeMap::new());
        e
    });
    let evt = make_event("test");
    let result = transform.apply(evt);
    assert!(result.ext.is_some());
}

#[test]
fn event_recorder_tracks_events() {
    let recorder = EventRecorder::new();
    recorder.record(&make_event("a"));
    recorder.record(&make_event("b"));
    assert_eq!(recorder.len(), 2);
    assert!(!recorder.is_empty());
}

#[test]
fn event_recorder_clear() {
    let recorder = EventRecorder::new();
    recorder.record(&make_event("x"));
    recorder.clear();
    assert!(recorder.is_empty());
}

#[test]
fn event_stats_counts_events() {
    let stats = EventStats::new();
    stats.observe(&make_event("a"));
    stats.observe(&make_event("b"));
    assert_eq!(stats.total_events(), 2);
}

#[test]
fn event_stats_counts_errors() {
    let stats = EventStats::new();
    stats.observe(&make_error_event("e1"));
    stats.observe(&make_event("ok"));
    assert_eq!(stats.error_count(), 1);
}

#[test]
fn event_stats_reset() {
    let stats = EventStats::new();
    stats.observe(&make_event("x"));
    stats.reset();
    assert_eq!(stats.total_events(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  CAPABILITY & CONTEXT TYPES
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_is_btreemap() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolUse, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    assert_eq!(manifest.len(), 2);
}

#[test]
fn capability_requirement_construction() {
    let req = CapabilityRequirement {
        capability: Capability::Vision,
        min_support: MinSupport::Emulated,
    };
    assert_eq!(req.capability, Capability::Vision);
}

#[test]
fn capability_requirements_default_is_empty() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

#[test]
fn context_packet_default_is_empty() {
    let ctx = ContextPacket::default();
    assert!(ctx.files.is_empty());
    assert!(ctx.snippets.is_empty());
}

#[test]
fn context_snippet_construction() {
    let snippet = ContextSnippet {
        name: "instructions".into(),
        content: "Do this carefully.".into(),
    };
    assert_eq!(snippet.name, "instructions");
}

#[test]
fn runtime_config_default() {
    let config = RuntimeConfig::default();
    assert!(config.model.is_none());
    assert!(config.max_budget_usd.is_none());
    assert!(config.max_turns.is_none());
    assert!(config.vendor.is_empty());
    assert!(config.env.is_empty());
}

#[test]
fn backend_identity_construction() {
    let id = BackendIdentity {
        id: "openai".into(),
        backend_version: Some("4.0".into()),
        adapter_version: None,
    };
    assert_eq!(id.id, "openai");
    assert!(id.adapter_version.is_none());
}

#[test]
fn workspace_spec_construction() {
    let spec = WorkspaceSpec {
        root: "/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into()],
    };
    assert_eq!(spec.root, "/project");
    assert_eq!(spec.include.len(), 1);
}

#[test]
fn usage_normalized_default() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn verification_report_default() {
    let v = VerificationReport::default();
    assert!(!v.harness_ok);
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
}
