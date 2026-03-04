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
//! Cross-crate integration tests.
//!
//! Exercises boundaries between workspace crates to ensure types, contracts,
//! and behaviour remain consistent when composed across crate lines.

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_glob::IncludeExcludeGlobs;
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{self as receipt_crate, ReceiptChain};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use chrono::Utc;
use serde_json::json;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .build()
}

fn make_hashed_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .with_hash()
        .expect("hash should succeed")
}

fn sample_capability_manifest() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m
}

fn sample_backend_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.1".into()),
    }
}

fn default_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    }
}

fn make_policy(
    allowed: &[&str],
    disallowed: &[&str],
    deny_read: &[&str],
    deny_write: &[&str],
) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: allowed.iter().map(|s| s.to_string()).collect(),
        disallowed_tools: disallowed.iter().map(|s| s.to_string()).collect(),
        deny_read: deny_read.iter().map(|s| s.to_string()).collect(),
        deny_write: deny_write.iter().map(|s| s.to_string()).collect(),
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    }
}

// ===========================================================================
// 1. Core types flow through protocol layer
// ===========================================================================

#[test]
fn core_work_order_survives_protocol_roundtrip() {
    let wo = make_work_order("hello world");
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo.clone(),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "hello world");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn core_receipt_survives_protocol_roundtrip() {
    let receipt = make_receipt("mock");
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: receipt.clone(),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt: r, .. } => {
            assert_eq!(r.backend.id, "mock");
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn core_agent_event_survives_protocol_roundtrip() {
    let evt = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: evt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hi"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn core_work_order_with_context_flows_through_protocol() {
    let ctx = ContextPacket {
        files: vec!["main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "example".into(),
            content: "snippet content".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task with context")
        .context(ctx)
        .build();
    let env = Envelope::Run {
        id: "ctx-run".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.context.files.len(), 1);
            assert_eq!(work_order.context.snippets.len(), 1);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn contract_version_is_embedded_in_receipt() {
    let receipt = make_receipt("mock");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn hello_envelope_roundtrip() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: sample_backend_identity(),
        capabilities: sample_capability_manifest(),
        mode: ExecutionMode::Mapped,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            backend,
            capabilities,
            contract_version,
            ..
        } => {
            assert_eq!(backend.id, "test-backend");
            assert!(capabilities.contains_key(&Capability::Streaming));
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn fatal_envelope_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-x".into()),
        error: "something broke".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert_eq!(error, "something broke");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ===========================================================================
// 2. Receipt hashing is canonical across crates
// ===========================================================================

#[test]
fn receipt_hash_is_deterministic() {
    let r1 = make_hashed_receipt("mock");
    let r2 = make_hashed_receipt("mock");
    assert!(r1.receipt_sha256.is_some());
    assert!(r2.receipt_sha256.is_some());
}

#[test]
fn receipt_hash_verifies_via_receipt_crate() {
    let r = make_hashed_receipt("mock");
    assert!(receipt_crate::verify_hash(&r));
}

#[test]
fn receipt_hash_null_before_hashing() {
    let r = make_receipt("mock");
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn mutated_receipt_fails_verification() {
    let mut r = make_hashed_receipt("mock");
    r.backend.id = "tampered".into();
    assert!(!receipt_crate::verify_hash(&r));
}

#[test]
fn receipt_chain_preserves_order() {
    let mut chain = ReceiptChain::new();
    chain.push(make_hashed_receipt("a")).unwrap();
    chain.push(make_hashed_receipt("b")).unwrap();
    assert_eq!(chain.len(), 2);
}

#[test]
fn receipt_chain_validates_hashes() {
    let mut chain = ReceiptChain::new();
    let r = make_hashed_receipt("mock");
    chain.push(r.clone()).unwrap();
    // Duplicate ID should be rejected
    let result = chain.push(r);
    assert!(result.is_err());
}

#[test]
fn receipt_diff_detects_changes() {
    use abp_receipt::diff_receipts;

    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let diff = diff_receipts(&r1, &r2);
    assert!(!diff.changes.is_empty());
}

#[test]
fn receipt_diff_same_receipt_is_empty() {
    use abp_receipt::diff_receipts;

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let diff = diff_receipts(&r, &r);
    assert!(diff.is_empty());
}

// ===========================================================================
// 3. Work order builder produces valid specs
// ===========================================================================

#[test]
fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
}

#[test]
fn work_order_builder_sets_root() {
    let wo = WorkOrderBuilder::new("t").root("/tmp").build();
    assert_eq!(wo.workspace.root, "/tmp");
}

#[test]
fn work_order_builder_sets_workspace_mode() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn work_order_builder_sets_policy() {
    let wo = WorkOrderBuilder::new("t")
        .policy(make_policy(&["read"], &[], &[], &[]))
        .build();
    assert_eq!(wo.policy.allowed_tools.len(), 1);
}

#[test]
fn work_order_builder_sets_execution_lane() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::PatchFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn work_order_builder_sets_include_exclude() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["src/**".into()])
        .exclude(vec!["*.tmp".into()])
        .build();
    assert_eq!(wo.workspace.include.len(), 1);
    assert_eq!(wo.workspace.exclude.len(), 1);
}

#[test]
fn work_order_workspace_spec_has_defaults() {
    let wo = WorkOrderBuilder::new("t").build();
    let spec = &wo.workspace;
    assert!(spec.include.is_empty());
    assert!(spec.exclude.is_empty());
}

// ===========================================================================
// 4. Policy engine used by runtime before execution
// ===========================================================================

#[test]
fn policy_engine_from_core_policy_profile() {
    let profile = make_policy(
        &["read_file", "write_file"],
        &["rm"],
        &["secret/**"],
        &["config/**"],
    );
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(engine.can_use_tool("read_file").allowed);
    assert!(!engine.can_use_tool("rm").allowed);
}

#[test]
fn policy_engine_denies_unlisted_tool_when_allow_present() {
    let profile = make_policy(&["read_file"], &[], &[], &[]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(!engine.can_use_tool("write_file").allowed);
}

#[test]
fn policy_engine_allows_all_when_no_allow_list() {
    let profile = make_policy(&[], &["rm"], &[], &[]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(engine.can_use_tool("read_file").allowed);
    assert!(!engine.can_use_tool("rm").allowed);
}

#[test]
fn policy_engine_checks_read_paths() {
    let profile = make_policy(&[], &[], &["secret/**"], &[]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_read_path(Path::new("secret/key.pem")).allowed);
}

#[test]
fn policy_engine_checks_write_paths() {
    let profile = make_policy(&[], &[], &[], &["config/**"]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_write_path(Path::new("config/app.toml")).allowed);
}

#[test]
fn policy_profile_from_work_order_used_by_engine() {
    let wo = WorkOrderBuilder::new("policy test")
        .policy(make_policy(&["grep"], &[], &[], &[]))
        .build();
    let engine = PolicyEngine::new(&wo.policy).unwrap();
    assert!(engine.can_use_tool("grep").allowed);
    assert!(!engine.can_use_tool("rm").allowed);
}

#[test]
fn policy_auditor_records_cross_crate_decisions() {
    use abp_policy::audit::PolicyAuditor;

    let profile = make_policy(&["read"], &[], &[], &[]);
    let engine = PolicyEngine::new(&profile).unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_tool("read");
    auditor.check_tool("write");
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn policy_engine_deny_read_glob() {
    let profile = make_policy(&[], &[], &["*.pem", "*.key"], &[]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(!engine.can_read_path(Path::new("server.pem")).allowed);
    assert!(!engine.can_read_path(Path::new("private.key")).allowed);
    assert!(engine.can_read_path(Path::new("readme.md")).allowed);
}

// ===========================================================================
// 5. Workspace staging used by runtime
// ===========================================================================

#[test]
fn workspace_manager_prepares_passthrough() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();
    assert!(prepared.path().exists());
}

#[test]
fn workspace_stager_excludes_patterns() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(tmp.path().join("secret.key"), "secret").unwrap();

    let staged = WorkspaceStager::new()
        .source_root(tmp.path())
        .exclude(vec!["*.key".into()])
        .stage()
        .unwrap();

    assert!(staged.path().join("src").join("main.rs").exists());
    assert!(!staged.path().join("secret.key").exists());
}

#[test]
fn workspace_stager_includes_only_matching() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("include_me.rs"), "code").unwrap();
    std::fs::write(tmp.path().join("skip_me.txt"), "text").unwrap();

    let staged = WorkspaceStager::new()
        .source_root(tmp.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();

    assert!(staged.path().join("include_me.rs").exists());
    assert!(!staged.path().join("skip_me.txt").exists());
}

#[test]
fn workspace_spec_from_work_order_used_by_manager() {
    let wo = WorkOrderBuilder::new("ws test")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let prepared = WorkspaceManager::prepare(&wo.workspace).unwrap();
    assert!(prepared.path().exists());
}

#[test]
fn workspace_snapshot_captures_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "world").unwrap();

    let snap = abp_workspace::snapshot::capture(tmp.path()).unwrap();
    assert_eq!(snap.file_count(), 2);
}

#[test]
fn workspace_change_tracker() {
    use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};

    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "src/main.rs".into(),
        kind: ChangeKind::Modified,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    assert_eq!(tracker.changes().len(), 1);
}

// ===========================================================================
// 6. Runtime wires together policy, workspace, and backend
// ===========================================================================

#[tokio::test]
async fn runtime_run_streaming_with_mock() {
    use abp_integrations::MockBackend;
    use abp_runtime::Runtime;

    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    let wo = make_work_order("test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn runtime_run_streaming_unknown_backend_errors() {
    use abp_runtime::Runtime;

    let rt = Runtime::new();
    let wo = make_work_order("fail test");
    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(result.is_err());
}

#[test]
fn runtime_error_has_error_code() {
    use abp_runtime::RuntimeError;

    let err = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    let code = err.error_code();
    assert_eq!(code.category(), ErrorCategory::Backend);
}

#[test]
fn runtime_error_converts_to_abp_error() {
    use abp_runtime::RuntimeError;

    let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_workspace_failed() {
    use abp_runtime::RuntimeError;

    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

// ===========================================================================
// 7. Glob crate used by both policy and workspace
// ===========================================================================

#[test]
fn glob_include_exclude_basic() {
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["*.tmp".into()]).unwrap();
    assert!(globs.decide_str("src/main.rs").is_allowed());
    assert!(!globs.decide_str("build.tmp").is_allowed());
}

#[test]
fn glob_empty_include_allows_all_except_excludes() {
    let globs = IncludeExcludeGlobs::new(&[], &["*.tmp".into()]).unwrap();
    assert!(globs.decide_str("src/main.rs").is_allowed());
    assert!(!globs.decide_str("build.tmp").is_allowed());
}

#[test]
fn glob_patterns_shared_between_policy_and_workspace() {
    let deny_patterns: Vec<String> = vec!["**/secret/**".into()];

    let profile = PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec![],
        deny_read: deny_patterns.clone(),
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(!engine.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);

    let globs = IncludeExcludeGlobs::new(&[], &deny_patterns).unwrap();
    assert!(!globs.decide_str("secret/key.pem").is_allowed());
    assert!(globs.decide_str("src/main.rs").is_allowed());
}

#[test]
fn glob_match_decision_enum_variants() {
    let globs = IncludeExcludeGlobs::new(&["*.rs".into()], &["test_*.rs".into()]).unwrap();
    let allowed = globs.decide_str("main.rs");
    let denied = globs.decide_str("test_foo.rs");
    assert!(allowed.is_allowed());
    assert!(!denied.is_allowed());
}

#[test]
fn glob_workspace_exclude_dot_git() {
    let globs = IncludeExcludeGlobs::new(&[], &[".git/**".into()]).unwrap();
    assert!(!globs.decide_str(".git/config").is_allowed());
    assert!(globs.decide_str("src/lib.rs").is_allowed());
}

// ===========================================================================
// 8. Error types bridge across crates
// ===========================================================================

#[test]
fn error_code_categories() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn error_code_as_str_format() {
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
}

#[test]
fn abp_error_new_and_fields() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "tool not allowed");
    assert_eq!(err.code, ErrorCode::PolicyDenied);
    assert_eq!(err.message, "tool not allowed");
}

#[test]
fn abp_error_converts_to_dto() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "process died");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::BackendCrashed);
    assert_eq!(dto.message, "process died");
}

#[test]
fn error_code_display() {
    let code = ErrorCode::WorkspaceInitFailed;
    let s = format!("{code}");
    assert!(!s.is_empty());
}

#[test]
fn protocol_error_is_distinct() {
    let json = "not valid json";
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

// ===========================================================================
// 9. Capability negotiation
// ===========================================================================

#[test]
fn capability_manifest_construction() {
    let m = sample_capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
    assert!(m.contains_key(&Capability::ToolRead));
    assert!(m.contains_key(&Capability::ToolWrite));
}

#[test]
fn support_level_ordering() {
    assert!(matches!(SupportLevel::Native, SupportLevel::Native));
    assert!(matches!(SupportLevel::Emulated, SupportLevel::Emulated));
    assert!(matches!(
        SupportLevel::Unsupported,
        SupportLevel::Unsupported
    ));
}

#[test]
fn capability_requirements_default_empty() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

#[test]
fn backend_identity_fields() {
    let id = sample_backend_identity();
    assert_eq!(id.id, "test-backend");
    assert_eq!(id.backend_version, Some("1.0".into()));
    assert_eq!(id.adapter_version, Some("0.1".into()));
}

// ===========================================================================
// 10. Event streaming across crate boundaries
// ===========================================================================

#[test]
fn event_run_started_through_protocol() {
    let evt = make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: evt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::RunStarted { message } => assert_eq!(message, "starting"),
            other => panic!("expected RunStarted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_call_with_optional_fields() {
    let evt = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: Some("parent-1".into()),
        input: json!({"path": "main.rs"}),
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: evt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let rt = JsonlCodec::decode(json.trim()).unwrap();
    match rt {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                parent_tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                assert_eq!(parent_tool_use_id.as_deref(), Some("parent-1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_file_changed_through_protocol() {
    let evt = make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: evt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(summary, "added function");
            }
            other => panic!("expected FileChanged, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_error_with_code() {
    let evt = make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: Some(ErrorCode::BackendCrashed),
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: evt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::Error {
                message,
                error_code,
            } => {
                assert_eq!(message, "oops");
                assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
            }
            other => panic!("expected Error, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_with_ext_data() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "hello".into(),
        },
        ext: Some({
            let mut m = BTreeMap::new();
            m.insert("custom".into(), json!(42));
            m
        }),
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: evt,
    };
    let json_str = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json_str.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            let ext = event.ext.unwrap();
            assert_eq!(ext["custom"], json!(42));
        }
        _ => panic!("expected Event"),
    }
}

// ===========================================================================
// 11. Stream pipeline
// ===========================================================================

#[test]
fn stream_pipeline_processes_events() {
    use abp_stream::{EventStats, StreamPipelineBuilder};

    let pipeline = StreamPipelineBuilder::new()
        .record()
        .with_stats(EventStats::new())
        .build();
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    for event in &events {
        pipeline.process(event.clone());
    }
    assert_eq!(pipeline.stats().unwrap().total_events(), 3);
    assert_eq!(pipeline.recorder().unwrap().len(), 3);
}

#[test]
fn stream_pipeline_without_stats() {
    use abp_stream::StreamPipelineBuilder;

    let pipeline = StreamPipelineBuilder::new().record().build();
    pipeline.process(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    assert!(pipeline.stats().is_none());
    assert_eq!(pipeline.recorder().unwrap().len(), 1);
}

#[test]
fn event_recorder_standalone() {
    use abp_stream::EventRecorder;

    let recorder = EventRecorder::new();
    recorder.record(&make_event(AgentEventKind::RunStarted {
        message: "a".into(),
    }));
    recorder.record(&make_event(AgentEventKind::RunCompleted {
        message: "b".into(),
    }));
    assert_eq!(recorder.len(), 2);
}

#[test]
fn event_stats_standalone() {
    use abp_stream::EventStats;

    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "hi".into(),
    }));
    assert_eq!(stats.total_events(), 2);
}

// ===========================================================================
// 12. Emulation layer
// ===========================================================================

#[test]
fn emulation_strategy_construction() {
    use abp_emulation::EmulationStrategy;

    let strat = EmulationStrategy::SystemPromptInjection {
        prompt: "Use tool: {tool_name}".into(),
    };
    match &strat {
        EmulationStrategy::SystemPromptInjection { prompt } => {
            assert!(prompt.contains("{tool_name}"));
        }
        _ => panic!("expected SystemPromptInjection"),
    }
}

#[test]
fn emulation_engine_resolves_strategy() {
    use abp_emulation::{EmulationEngine, EmulationStrategy};

    let engine = EmulationEngine::with_defaults();
    let strat = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(
        strat,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn emulation_can_emulate_check() {
    use abp_emulation::can_emulate;

    assert!(can_emulate(&Capability::ExtendedThinking));
}

// ===========================================================================
// 13. Dialect and projection
// ===========================================================================

#[test]
fn dialect_enum_variants() {
    use abp_dialect::Dialect;

    let claude = Dialect::Claude;
    let openai = Dialect::OpenAi;
    assert_ne!(format!("{claude:?}"), format!("{openai:?}"));
}

#[test]
fn projection_matrix_register_and_project() {
    use abp_dialect::Dialect;
    use abp_projection::ProjectionMatrix;

    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", sample_capability_manifest(), Dialect::Claude, 10);

    let wo = make_work_order("projection test");
    let result = matrix.project(&wo);
    assert!(result.is_ok());
}

#[test]
fn projection_matrix_empty_returns_error() {
    use abp_projection::ProjectionMatrix;

    let matrix = ProjectionMatrix::new();
    let wo = make_work_order("empty matrix");
    let result = matrix.project(&wo);
    assert!(result.is_err());
}

// ===========================================================================
// 14. Serde roundtrip guarantees
// ===========================================================================

#[test]
fn serde_work_order_roundtrip() {
    let wo = make_work_order("serde test");
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, wo.task);
}

#[test]
fn serde_receipt_roundtrip() {
    let r = make_receipt("mock");
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.backend.id, "mock");
}

#[test]
fn serde_agent_event_kind_variants() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "delta".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: Some("tu".into()),
            output: json!("ok"),
            is_error: false,
        },
    ];
    for kind in kinds {
        let evt = make_event(kind);
        let json = serde_json::to_string(&evt).unwrap();
        let _rt: AgentEvent = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn serde_policy_profile_roundtrip() {
    let p = make_policy(&["read"], &["rm"], &["secret/**"], &["config/**"]);
    let json = serde_json::to_string(&p).unwrap();
    let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.allowed_tools, p.allowed_tools);
    assert_eq!(rt.disallowed_tools, p.disallowed_tools);
}

#[test]
fn serde_envelope_run_variant() {
    let env = Envelope::Run {
        id: "test".into(),
        work_order: make_work_order("t"),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"t\":\"run\""));
}

#[test]
fn serde_context_packet_with_files() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![ContextSnippet {
            name: "s1".into(),
            content: "data".into(),
        }],
    };
    let json = serde_json::to_string(&ctx).unwrap();
    let rt: ContextPacket = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.files.len(), 1);
    assert_eq!(rt.snippets.len(), 1);
}

#[test]
fn serde_verification_report_roundtrip() {
    let report = VerificationReport {
        git_diff: Some("diff --git ...".into()),
        git_status: Some("M src/main.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&report).unwrap();
    let rt: VerificationReport = serde_json::from_str(&json).unwrap();
    assert!(rt.harness_ok);
    assert!(rt.git_diff.is_some());
}

#[test]
fn serde_outcome_variants() {
    for outcome in &[Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(outcome).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(&rt, outcome);
    }
}

#[test]
fn serde_backend_identity_option_fields() {
    let id = BackendIdentity {
        id: "test".into(),
        backend_version: None,
        adapter_version: None,
    };
    let json = serde_json::to_string(&id).unwrap();
    let rt: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert!(rt.backend_version.is_none());
}

// ===========================================================================
// 15. Work order builder advanced
// ===========================================================================

#[test]
fn work_order_builder_all_fields() {
    let wo = WorkOrderBuilder::new("full config")
        .root("/workspace")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["*.tmp".into()])
        .lane(ExecutionLane::PatchFirst)
        .model("gpt-4o")
        .max_turns(5)
        .policy(make_policy(&["read"], &[], &[], &[]))
        .build();

    assert_eq!(wo.task, "full config");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert_eq!(wo.config.model, Some("gpt-4o".into()));
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn work_order_serde_roundtrip() {
    let wo = WorkOrderBuilder::new("roundtrip test")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(make_policy(&["read"], &[], &[], &[]))
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, wo.task);
    assert_eq!(rt.policy.allowed_tools, wo.policy.allowed_tools);
}

#[test]
fn work_order_with_full_config() {
    let config = RuntimeConfig {
        model: Some("claude-sonnet-4-20250514".into()),
        vendor: BTreeMap::new(),
        env: {
            let mut m = BTreeMap::new();
            m.insert("API_KEY".into(), "test".into());
            m
        },
        max_turns: Some(10),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("config test").config(config).build();
    assert_eq!(wo.config.model, Some("claude-sonnet-4-20250514".into()));
    assert_eq!(wo.config.max_turns, Some(10));
}

// ===========================================================================
// 16. Receipt builder advanced
// ===========================================================================

#[test]
fn receipt_builder_with_usage() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        })
        .build();
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(50));
}

#[test]
fn receipt_builder_with_verification() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: None,
            harness_ok: true,
        })
        .build();
    assert!(receipt.verification.harness_ok);
}

#[test]
fn receipt_builder_with_artifacts() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "fix.diff".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();
    assert_eq!(receipt.artifacts.len(), 2);
}

#[test]
fn receipt_builder_with_mode() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .mode(ExecutionMode::Passthrough)
        .build();
    assert!(matches!(receipt.mode, ExecutionMode::Passthrough));
}

#[test]
fn receipt_builder_chains_fluently() {
    let receipt = ReceiptBuilder::new("backend")
        .outcome(Outcome::Complete)
        .mode(ExecutionMode::Mapped)
        .backend_version("1.0")
        .adapter_version("0.5")
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            ..Default::default()
        })
        .build();

    assert_eq!(receipt.backend.id, "backend");
    assert_eq!(receipt.backend.backend_version, Some("1.0".into()));
    assert_eq!(receipt.backend.adapter_version, Some("0.5".into()));
}

// ===========================================================================
// 17. Contract version consistency
// ===========================================================================

#[test]
fn contract_version_format() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
}

#[test]
fn contract_version_in_receipt_meta() {
    let r = make_receipt("mock");
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn contract_version_not_empty() {
    assert!(!CONTRACT_VERSION.is_empty());
}

// ===========================================================================
// 18. Policy composition
// ===========================================================================

#[test]
fn policy_compose_multiple_profiles() {
    use abp_policy::compose::{ComposedEngine, PolicyPrecedence};

    let profile1 = make_policy(&["read"], &[], &[], &[]);
    let profile2 = make_policy(&[], &["rm"], &[], &[]);
    let engine =
        ComposedEngine::new(vec![profile1, profile2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(engine.check_tool("read").is_allow());
    assert!(engine.check_tool("rm").is_deny());
}

#[test]
fn policy_rules_engine_evaluates() {
    use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};

    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "deny-rm".into(),
        description: "Deny rm tool".into(),
        condition: RuleCondition::Pattern("rm".into()),
        effect: RuleEffect::Deny,
        priority: 100,
    });
    let evals = engine.evaluate_all("rm");
    assert!(!evals.is_empty());
    assert!(evals[0].matched);
}

#[test]
fn policy_validator_detects_issues() {
    use abp_policy::compose::PolicyValidator;

    let mut profile = default_policy();
    profile.allowed_tools = vec!["read".into()];
    profile.disallowed_tools = vec!["read".into()]; // Overlapping!
    let warnings = PolicyValidator::validate(&profile);
    assert!(!warnings.is_empty(), "should detect overlapping allow/deny");
}

#[test]
fn policy_set_merge_profiles() {
    use abp_policy::compose::PolicySet;

    let mut set = PolicySet::new("combined");
    set.add(make_policy(&["read"], &[], &[], &[]));
    set.add(make_policy(&["write"], &[], &[], &[]));
    let merged = set.merge();
    assert_eq!(merged.allowed_tools.len(), 2);
}

// ===========================================================================
// 19. Full pipeline: work order -> policy -> workspace -> backend -> receipt
// ===========================================================================

#[tokio::test]
async fn full_pipeline_mock_backend() {
    use abp_integrations::MockBackend;
    use abp_runtime::Runtime;

    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);

    let wo = WorkOrderBuilder::new("full pipeline test")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(default_policy())
        .build();

    // Policy check
    let engine = PolicyEngine::new(&wo.policy).unwrap();
    assert!(engine.can_use_tool("read_file").allowed);

    // Run
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

// ===========================================================================
// 20. Protocol codec edge cases
// ===========================================================================

#[test]
fn protocol_decode_invalid_json() {
    let result = JsonlCodec::decode("}{not json");
    assert!(result.is_err());
}

#[test]
fn protocol_decode_empty_string() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn protocol_decode_stream() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "a".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "b".into(),
        }),
    ];
    let mut buf = Vec::new();
    for evt in &events {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: evt.clone(),
        };
        let line = JsonlCodec::encode(&env).unwrap();
        buf.extend_from_slice(line.as_bytes());
    }
    let reader = std::io::BufReader::new(buf.as_slice());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn protocol_envelope_discriminator_is_t() {
    let env = Envelope::Run {
        id: "test".into(),
        work_order: make_work_order("t"),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"t\""));
}

// ===========================================================================
// 21. Cross-boundary type consistency
// ===========================================================================

#[test]
fn work_order_policy_globs_workspace_integration() {
    let deny_patterns: Vec<String> = vec!["**/tests/**".into()];

    let wo = WorkOrderBuilder::new("glob integration")
        .root(".")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**/*.rs".into()])
        .exclude(deny_patterns.clone())
        .policy(PolicyProfile {
            allowed_tools: vec![],
            disallowed_tools: vec![],
            deny_read: deny_patterns.clone(),
            deny_write: deny_patterns.clone(),
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        })
        .build();

    // Policy engine uses deny patterns
    let engine = PolicyEngine::new(&wo.policy).unwrap();
    assert!(!engine.can_read_path(Path::new("tests/test.rs")).allowed);

    // Glob crate uses the same patterns for workspace filtering
    let globs = IncludeExcludeGlobs::new(&wo.workspace.include, &wo.workspace.exclude).unwrap();
    assert!(globs.decide_str("src/main.rs").is_allowed());
    assert!(!globs.decide_str("src/tests/test.rs").is_allowed());
}

#[test]
fn receipt_chain_rejects_duplicate_ids() {
    let mut chain = ReceiptChain::new();
    let r = make_hashed_receipt("mock");
    chain.push(r.clone()).unwrap();
    let result = chain.push(r);
    assert!(result.is_err());
}

#[test]
fn tool_call_event_tool_use_id_roundtrip() {
    let evt = make_event(AgentEventKind::ToolCall {
        tool_name: "write_file".into(),
        tool_use_id: Some("tu-42".into()),
        parent_tool_use_id: Some("parent-1".into()),
        input: json!({"path": "x.rs", "content": "code"}),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    match rt.kind {
        AgentEventKind::ToolCall {
            parent_tool_use_id, ..
        } => {
            assert_eq!(parent_tool_use_id, Some("parent-1".into()));
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn workspace_stager_git_init() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let staged = WorkspaceStager::new()
        .source_root(tmp.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(staged.path().join(".git").exists());
}

#[test]
fn workspace_ops_operation_log() {
    use abp_workspace::ops::{FileOperation, OperationLog};

    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "out.txt".into(),
        size: 42,
    });
    log.record(FileOperation::Read {
        path: "in.txt".into(),
    });
    log.record(FileOperation::Delete {
        path: "tmp.txt".into(),
    });

    assert_eq!(log.reads().len(), 1);
    assert_eq!(log.writes().len(), 1);
    assert_eq!(log.deletes().len(), 1);
}

#[test]
fn workspace_template_registry() {
    use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};

    let mut registry = TemplateRegistry::new();
    let tpl = WorkspaceTemplate::new("rust-cli", "Rust CLI project");
    registry.register(tpl);

    assert!(registry.get("rust-cli").is_some());
    assert!(registry.get("nonexistent").is_none());
}

// ===========================================================================
// 22. Backend integration points
// ===========================================================================

#[test]
fn runtime_with_projection_matrix() {
    use abp_dialect::Dialect;
    use abp_projection::ProjectionMatrix;
    use abp_runtime::Runtime;

    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", sample_capability_manifest(), Dialect::Claude, 10);

    let rt = Runtime::with_default_backends().with_projection(matrix);
    assert!(rt.projection().is_some());
}

#[test]
fn config_error_variants() {
    use abp_config::ConfigError;

    let err = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("missing.toml") || msg.contains("FileNotFound"));

    let err2 = ConfigError::ParseError {
        reason: "bad toml".into(),
    };
    let msg2 = format!("{err2}");
    assert!(!msg2.is_empty());
}

#[test]
fn backend_registry_lookup() {
    use abp_integrations::MockBackend;
    use abp_runtime::Runtime;

    let mut rt = Runtime::new();
    rt.register_backend("test", MockBackend);
    assert!(rt.backend("test").is_some());
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn mock_backend_identity_is_populated() {
    use abp_integrations::{Backend, MockBackend};

    let mock = MockBackend;
    let id = mock.identity();
    assert!(!id.id.is_empty());
    assert!(id.backend_version.is_some());
}

// ===========================================================================
// 23. Additional cross-boundary edge cases
// ===========================================================================

#[test]
fn empty_work_order_through_protocol() {
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "empty".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn large_context_through_protocol() {
    let wo = WorkOrderBuilder::new("big context")
        .context(ContextPacket {
            files: vec!["big.txt".into()],
            snippets: vec![],
        })
        .build();
    let env = Envelope::Run {
        id: "big".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.context.files.len(), 1);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn receipt_with_all_fields_through_protocol() {
    let receipt = ReceiptBuilder::new("full-backend")
        .outcome(Outcome::Complete)
        .mode(ExecutionMode::Mapped)
        .backend_version("2.0")
        .adapter_version("1.0")
        .capabilities(sample_capability_manifest())
        .usage(UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(200),
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            request_units: Some(1),
            estimated_cost_usd: Some(0.01),
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "fix.diff".into(),
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("clean".into()),
            harness_ok: true,
        })
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .with_hash()
        .unwrap();

    let env = Envelope::Final {
        ref_id: "full".into(),
        receipt: receipt.clone(),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt: r, .. } => {
            assert_eq!(r.backend.backend_version, Some("2.0".into()));
            assert_eq!(r.usage.input_tokens, Some(500));
            assert_eq!(r.artifacts.len(), 1);
            assert!(r.verification.harness_ok);
            assert!(receipt_crate::verify_hash(&r));
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn multiple_events_in_sequence_through_protocol() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "chunk1".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "chunk2".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let mut encoded = Vec::new();
    for evt in &events {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: evt.clone(),
        };
        let line = JsonlCodec::encode(&env).unwrap();
        encoded.extend_from_slice(line.as_bytes());
    }

    let reader = std::io::BufReader::new(encoded.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(decoded.len(), events.len());
    assert!(decoded.iter().all(|r| r.is_ok()));
}

#[test]
fn usage_normalized_default() {
    let usage = UsageNormalized::default();
    assert!(usage.input_tokens.is_none());
    assert!(usage.output_tokens.is_none());
    assert!(usage.cache_read_tokens.is_none());
    assert!(usage.cache_write_tokens.is_none());
    assert!(usage.request_units.is_none());
    assert!(usage.estimated_cost_usd.is_none());
}

#[test]
fn execution_mode_variants() {
    let passthrough = ExecutionMode::Passthrough;
    let mapped = ExecutionMode::Mapped;
    assert!(matches!(passthrough, ExecutionMode::Passthrough));
    assert!(matches!(mapped, ExecutionMode::Mapped));
}

#[test]
fn execution_lane_variants() {
    let pf = ExecutionLane::PatchFirst;
    let wf = ExecutionLane::WorkspaceFirst;
    assert!(matches!(pf, ExecutionLane::PatchFirst));
    assert!(matches!(wf, ExecutionLane::WorkspaceFirst));
}

#[test]
fn workspace_mode_serde() {
    let staged = WorkspaceMode::Staged;
    let json = serde_json::to_string(&staged).unwrap();
    let rt: WorkspaceMode = serde_json::from_str(&json).unwrap();
    assert!(matches!(rt, WorkspaceMode::Staged));
}

#[test]
fn default_policy_allows_everything() {
    let engine = PolicyEngine::new(&default_policy()).unwrap();
    assert!(engine.can_use_tool("any_tool").allowed);
    assert!(engine.can_read_path(Path::new("any/path")).allowed);
    assert!(engine.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn policy_with_deny_network() {
    let profile = PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec![],
        allow_network: vec!["example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec![],
    };
    let json = serde_json::to_string(&profile).unwrap();
    let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.allow_network, vec!["example.com"]);
    assert_eq!(rt.deny_network, vec!["evil.com"]);
}

#[test]
fn receipt_meta_has_run_id() {
    let r = make_receipt("mock");
    assert!(!r.meta.run_id.is_nil());
}

#[test]
fn receipt_meta_has_timestamps() {
    let r = make_receipt("mock");
    assert!(r.meta.started_at <= r.meta.finished_at);
}

#[test]
fn receipt_builder_with_capabilities() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .capabilities(sample_capability_manifest())
        .build();
    assert!(!receipt.capabilities.is_empty());
}

#[test]
fn emulation_config_with_override() {
    use abp_emulation::{EmulationConfig, EmulationEngine, EmulationStrategy};

    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "not needed".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let strat = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(strat, EmulationStrategy::Disabled { .. }));
}

#[test]
fn receipt_chain_verify_valid_chain() {
    let mut chain = ReceiptChain::new();
    chain.push(make_hashed_receipt("a")).unwrap();
    chain.push(make_hashed_receipt("b")).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn receipt_chain_iter() {
    let mut chain = ReceiptChain::new();
    chain.push(make_hashed_receipt("a")).unwrap();
    chain.push(make_hashed_receipt("b")).unwrap();
    let ids: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(ids.len(), 2);
}

#[test]
fn work_order_builder_context() {
    let wo = WorkOrderBuilder::new("ctx test")
        .context(ContextPacket {
            files: vec!["a.rs".into(), "b.rs".into()],
            snippets: vec![],
        })
        .build();
    assert_eq!(wo.context.files.len(), 2);
}

#[test]
fn policy_require_approval_for_field() {
    let profile = PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec!["dangerous_tool".into()],
    };
    let json = serde_json::to_string(&profile).unwrap();
    let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.require_approval_for, vec!["dangerous_tool"]);
}

#[test]
fn workspace_stager_no_git_init() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "content").unwrap();

    let staged = WorkspaceStager::new()
        .source_root(tmp.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!staged.path().join(".git").exists());
}

#[test]
fn error_code_backend_crashed() {
    assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
}

#[test]
fn error_code_workspace_init_failed() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
}

#[test]
fn runtime_lists_backends() {
    use abp_integrations::MockBackend;
    use abp_runtime::Runtime;

    let mut rt = Runtime::new();
    rt.register_backend("alpha", MockBackend);
    rt.register_backend("beta", MockBackend);
    let names = rt.backend_names();
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));
}
