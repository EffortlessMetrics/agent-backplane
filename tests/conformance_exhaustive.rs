// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive conformance tests for the ABP contract, protocol, and serde roundtrips.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::chain::{ChainError, ReceiptChain};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, canonical_json, receipt_hash,
};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use chrono::Utc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend).outcome(outcome).build()
}

fn make_hashed_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .with_hash()
        .expect("hashing should succeed")
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

// ===========================================================================
// 1. Receipt contract tests (15+)
// ===========================================================================

#[test]
fn receipt_hash_deterministic_same_input() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "same receipt must produce identical hash");
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = make_receipt("mock", Outcome::Complete);
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_excludes_receipt_sha256_field() {
    let mut r1 = make_receipt("mock", Outcome::Complete);
    r1.receipt_sha256 = None;

    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("decafbad".into());

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2, "receipt_sha256 must be excluded from hash input");
}

#[test]
fn receipt_with_hash_populates_sha256() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_with_hash_matches_manual_hash() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let expected = receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_deref().unwrap(), expected);
}

#[test]
fn receipt_required_fields_populated() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(!r.meta.run_id.is_nil());
    assert!(!r.meta.contract_version.is_empty());
    assert!(!r.backend.id.is_empty());
}

#[test]
fn receipt_contract_version_matches_constant() {
    let r = make_receipt("mock", Outcome::Complete);
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_duration_ms_non_negative() {
    let r = make_receipt("mock", Outcome::Complete);
    // duration_ms is u64, so always >= 0, but verify it was computed
    assert!(
        r.meta.duration_ms < 10_000,
        "builder default should be near-instant"
    );
}

#[test]
fn receipt_started_before_finished() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.meta.started_at <= r.meta.finished_at);
}

#[test]
fn receipt_different_backends_different_hashes() {
    let r1 = make_receipt("backend-a", Outcome::Complete);
    let r2 = make_receipt("backend-b", Outcome::Complete);
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(
        h1, h2,
        "different backend ids should produce different hashes"
    );
}

#[test]
fn receipt_different_outcomes_different_hashes() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = make_receipt("mock", Outcome::Failed);
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_metadata_completeness_work_order_id() {
    let wo_id = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(wo_id)
        .build();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[test]
fn receipt_builder_backend_version() {
    let r = ReceiptBuilder::new("mock")
        .backend_version("2.0".to_string())
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
}

#[test]
fn receipt_builder_adapter_version() {
    let r = ReceiptBuilder::new("mock")
        .adapter_version("0.3".to_string())
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.3"));
}

#[test]
fn receipt_builder_trace_events() {
    let evt = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(evt)
        .build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_artifacts() {
    let art = ArtifactRef {
        kind: "diff".into(),
        path: "/tmp/patch".into(),
    };
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(art)
        .build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "diff");
}

#[test]
fn receipt_default_mode_is_mapped() {
    let r = make_receipt("mock", Outcome::Complete);
    assert_eq!(r.mode, ExecutionMode::default());
}

#[test]
fn receipt_usage_fields_default_none() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(r.usage.estimated_cost_usd.is_none());
}

// ===========================================================================
// 1b. Receipt chain integrity tests
// ===========================================================================

#[test]
fn chain_empty_verify_fails() {
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify(), Err(ChainError::EmptyChain)));
}

#[test]
fn chain_single_hashed_receipt_verifies() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_multiple_receipts_verify() {
    let mut chain = ReceiptChain::new();
    for _ in 0..5 {
        chain
            .push(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_rejects_duplicate_run_id() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    // Same run_id => duplicate
    r2.receipt_sha256 = r.receipt_sha256.clone();
    let mut chain = ReceiptChain::new();
    chain.push(r).unwrap();
    assert!(matches!(
        chain.push(r2),
        Err(ChainError::DuplicateId { .. })
    ));
}

#[test]
fn chain_tampered_hash_rejected() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let mut chain = ReceiptChain::new();
    assert!(matches!(chain.push(r), Err(ChainError::InvalidHash { .. })));
}

#[test]
fn chain_find_by_id() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id;
    let mut chain = ReceiptChain::new();
    chain.push(r).unwrap();
    assert!(chain.find_by_id(&id).is_some());
    assert!(chain.find_by_id(&Uuid::new_v4()).is_none());
}

#[test]
fn chain_find_by_backend() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("alpha", Outcome::Complete))
        .unwrap();
    chain
        .push(make_hashed_receipt("beta", Outcome::Complete))
        .unwrap();
    chain
        .push(make_hashed_receipt("alpha", Outcome::Failed))
        .unwrap();
    assert_eq!(chain.find_by_backend("alpha").len(), 2);
    assert_eq!(chain.find_by_backend("beta").len(), 1);
    assert_eq!(chain.find_by_backend("gamma").len(), 0);
}

#[test]
fn chain_success_rate_calculation() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("m", Outcome::Complete))
        .unwrap();
    chain
        .push(make_hashed_receipt("m", Outcome::Failed))
        .unwrap();
    let rate = chain.success_rate();
    assert!((rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn chain_total_events() {
    let evt = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(evt)
        .with_hash()
        .unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(r).unwrap();
    assert_eq!(chain.total_events(), 1);
}

// ===========================================================================
// 2. Envelope contract tests (15+)
// ===========================================================================

#[test]
fn envelope_hello_tag_is_t() {
    let hello = make_hello();
    let json = serde_json::to_string(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn envelope_run_tag_is_t() {
    let wo = make_work_order("do something");
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = serde_json::to_string(&run).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn envelope_event_tag_is_t() {
    let evt = Envelope::Event {
        ref_id: "run-1".into(),
        event: make_event(AgentEventKind::RunStarted {
            message: "hi".into(),
        }),
    };
    let json = serde_json::to_string(&evt).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn envelope_final_tag_is_t() {
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: make_receipt("mock", Outcome::Complete),
    };
    let json = serde_json::to_string(&fin).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn envelope_fatal_tag_is_t() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "crash".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&fatal).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn envelope_hello_has_contract_version() {
    let hello = make_hello();
    if let Envelope::Hello {
        contract_version, ..
    } = &hello
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn envelope_hello_has_backend_identity() {
    let hello = make_hello();
    if let Envelope::Hello { backend, .. } = &hello {
        assert!(!backend.id.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn envelope_hello_default_mode_mapped() {
    let hello = make_hello();
    if let Envelope::Hello { mode, .. } = &hello {
        assert_eq!(*mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn envelope_run_has_valid_work_order() {
    let wo = make_work_order("test task");
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    if let Envelope::Run { id, work_order } = &run {
        assert!(!id.is_empty());
        assert_eq!(work_order.task, "test task");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn envelope_event_ref_id_correlation() {
    let run_id = "run-42";
    let evt = Envelope::Event {
        ref_id: run_id.into(),
        event: make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
    };
    if let Envelope::Event { ref_id, .. } = &evt {
        assert_eq!(ref_id, run_id);
    } else {
        panic!("expected Event");
    }
}

#[test]
fn envelope_final_has_valid_receipt() {
    let receipt = make_hashed_receipt("mock", Outcome::Complete);
    let fin = Envelope::Final {
        ref_id: "run-42".into(),
        receipt: receipt.clone(),
    };
    if let Envelope::Final { ref_id, receipt: r } = &fin {
        assert_eq!(ref_id, "run-42");
        assert!(r.receipt_sha256.is_some());
    } else {
        panic!("expected Final");
    }
}

#[test]
fn envelope_fatal_has_error_message() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "something broke".into(),
        error_code: None,
    };
    if let Envelope::Fatal { error, .. } = &fatal {
        assert_eq!(error, "something broke");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn envelope_fatal_ref_id_optional() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&fatal).unwrap();
    assert!(json.contains(r#""ref_id":null"#));
}

#[test]
fn envelope_fatal_with_error_code() {
    let fatal = Envelope::fatal_with_code(
        Some("run-1".into()),
        "auth failed",
        abp_error::ErrorCode::BackendAuthFailed,
    );
    assert_eq!(
        fatal.error_code(),
        Some(abp_error::ErrorCode::BackendAuthFailed)
    );
}

#[test]
fn envelope_hello_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt-backend".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    if let Envelope::Hello { mode, .. } = &hello {
        assert_eq!(*mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn envelope_hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);
    let hello = Envelope::hello(
        BackendIdentity {
            id: "cap-test".into(),
            backend_version: None,
            adapter_version: None,
        },
        caps,
    );
    if let Envelope::Hello { capabilities, .. } = &hello {
        assert_eq!(capabilities.len(), 2);
        assert_eq!(capabilities[&Capability::Streaming], SupportLevel::Native);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn envelope_event_all_agent_event_kinds() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "d".into(),
        },
        AgentEventKind::AssistantDelta { text: "t".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("id1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"cmd": "ls"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("id1".into()),
            output: serde_json::json!("ok"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "a.txt".into(),
            summary: "created".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
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
    for kind in kinds {
        let evt = Envelope::Event {
            ref_id: "run-x".into(),
            event: make_event(kind),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains(r#""t":"event""#));
    }
}

// ===========================================================================
// 3. WorkOrder contract tests (15+)
// ===========================================================================

#[test]
fn work_order_id_is_valid_uuid() {
    let wo = make_work_order("hello");
    assert!(!wo.id.is_nil());
}

#[test]
fn work_order_task_preserved() {
    let wo = make_work_order("fix the bug");
    assert_eq!(wo.task, "fix the bug");
}

#[test]
fn work_order_task_non_empty() {
    let wo = make_work_order("something");
    assert!(!wo.task.is_empty());
}

#[test]
fn work_order_default_lane_is_patch_first() {
    let wo = make_work_order("task");
    assert_eq!(wo.lane, ExecutionLane::PatchFirst);
}

#[test]
fn work_order_builder_lane_override() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert_eq!(wo.lane, ExecutionLane::WorkspaceFirst);
}

#[test]
fn work_order_default_workspace_mode() {
    let wo = make_work_order("task");
    assert_eq!(wo.workspace.mode, WorkspaceMode::Staged);
}

#[test]
fn work_order_builder_workspace_staged() {
    let wo = WorkOrderBuilder::new("task")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    assert_eq!(wo.workspace.mode, WorkspaceMode::Staged);
}

#[test]
fn work_order_builder_root() {
    let wo = WorkOrderBuilder::new("task").root("/my/project").build();
    assert_eq!(wo.workspace.root, "/my/project");
}

#[test]
fn work_order_builder_include_exclude() {
    let wo = WorkOrderBuilder::new("task")
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_builder_context() {
    let ctx = ContextPacket {
        files: vec!["main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "snippet1".into(),
            content: "fn main() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn work_order_builder_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec![],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec![],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec![],
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["bash"]);
    assert_eq!(wo.policy.deny_write, vec!["/etc/**"]);
}

#[test]
fn work_order_builder_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("task").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
    assert_eq!(
        wo.requirements.required[0].capability,
        Capability::Streaming
    );
}

#[test]
fn work_order_builder_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4o").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn work_order_builder_max_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(1.50).build();
    assert_eq!(wo.config.max_budget_usd, Some(1.50));
}

#[test]
fn work_order_builder_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn work_order_config_vendor_btreemap() {
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            model: None,
            vendor: {
                let mut m = BTreeMap::new();
                m.insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
                m
            },
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        })
        .build();
    assert!(wo.config.vendor.contains_key("abp"));
}

#[test]
fn work_order_unique_ids() {
    let wo1 = make_work_order("task");
    let wo2 = make_work_order("task");
    assert_ne!(wo1.id, wo2.id, "each work order should get a unique id");
}

#[test]
fn work_order_default_context_empty() {
    let wo = make_work_order("task");
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn work_order_default_policy_empty() {
    let wo = make_work_order("task");
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.policy.deny_read.is_empty());
    assert!(wo.policy.deny_write.is_empty());
}

// ===========================================================================
// 4. Serde roundtrip contract tests (15+)
// ===========================================================================

#[test]
fn serde_roundtrip_receipt() {
    let r = make_receipt("mock", Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
}

#[test]
fn serde_roundtrip_receipt_with_hash() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn serde_roundtrip_work_order() {
    let wo = make_work_order("roundtrip task");
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.lane, wo2.lane);
}

#[test]
fn serde_roundtrip_agent_event_run_started() {
    let evt = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(evt2.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn serde_roundtrip_agent_event_tool_call() {
    let evt = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"cmd": "ls -la"}),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_name, input, ..
    } = &evt2.kind
    {
        assert_eq!(tool_name, "bash");
        assert_eq!(input["cmd"], "ls -la");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn serde_roundtrip_agent_event_tool_result() {
    let evt = make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: None,
        output: serde_json::json!({"content": "file contents"}),
        is_error: true,
    });
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolResult { is_error, .. } = &evt2.kind {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn serde_roundtrip_agent_event_file_changed() {
    let evt = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added fn main".into(),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::FileChanged { path, summary } = &evt2.kind {
        assert_eq!(path, "src/main.rs");
        assert_eq!(summary, "added fn main");
    } else {
        panic!("expected FileChanged");
    }
}

#[test]
fn serde_roundtrip_agent_event_command_executed() {
    let evt = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &evt2.kind {
        assert_eq!(*exit_code, Some(0));
    } else {
        panic!("expected CommandExecuted");
    }
}

#[test]
fn serde_roundtrip_agent_event_error_with_code() {
    let evt = make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &evt2.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
    } else {
        panic!("expected Error");
    }
}

#[test]
fn serde_roundtrip_envelope_hello() {
    let hello = make_hello();
    let json = serde_json::to_string(&hello).unwrap();
    let hello2: Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(hello2, Envelope::Hello { .. }));
}

#[test]
fn serde_roundtrip_envelope_run() {
    let wo = make_work_order("test");
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let json = serde_json::to_string(&run).unwrap();
    let run2: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Run { id, work_order } = &run2 {
        assert_eq!(id, "r1");
        assert_eq!(work_order.task, "test");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn serde_roundtrip_envelope_event() {
    let evt = Envelope::Event {
        ref_id: "r1".into(),
        event: make_event(AgentEventKind::Warning {
            message: "warn".into(),
        }),
    };
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(evt2, Envelope::Event { .. }));
}

#[test]
fn serde_roundtrip_envelope_final() {
    let fin = Envelope::Final {
        ref_id: "r1".into(),
        receipt: make_receipt("mock", Outcome::Complete),
    };
    let json = serde_json::to_string(&fin).unwrap();
    let fin2: Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(fin2, Envelope::Final { .. }));
}

#[test]
fn serde_roundtrip_envelope_fatal() {
    let fatal = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "oom".into(),
        error_code: Some(abp_error::ErrorCode::Internal),
    };
    let json = serde_json::to_string(&fatal).unwrap();
    let fatal2: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Fatal {
        error, error_code, ..
    } = &fatal2
    {
        assert_eq!(error, "oom");
        assert_eq!(*error_code, Some(abp_error::ErrorCode::Internal));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn serde_roundtrip_btreemap_ordering_preserved() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);
    caps.insert(Capability::Vision, SupportLevel::Unsupported);
    let json1 = serde_json::to_string(&caps).unwrap();
    let caps2: CapabilityManifest = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&caps2).unwrap();
    assert_eq!(
        json1, json2,
        "BTreeMap ordering must be stable across roundtrips"
    );
}

#[test]
fn serde_roundtrip_runtime_config_btreemap() {
    let mut vendor = BTreeMap::new();
    vendor.insert("zeta".to_string(), serde_json::json!("z"));
    vendor.insert("alpha".to_string(), serde_json::json!("a"));
    vendor.insert("mid".to_string(), serde_json::json!("m"));
    let cfg = RuntimeConfig {
        model: Some("gpt-4o".into()),
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: Some(5.0),
        max_turns: Some(20),
    };
    let json1 = serde_json::to_string(&cfg).unwrap();
    let cfg2: RuntimeConfig = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&cfg2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn serde_roundtrip_agent_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("custom_field".to_string(), serde_json::json!(42));
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&evt).unwrap();
    let evt2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(evt2.ext.is_some());
    assert_eq!(evt2.ext.unwrap()["custom_field"], 42);
}

// ===========================================================================
// 4b. JSONL codec roundtrip tests
// ===========================================================================

#[test]
fn jsonl_codec_encode_ends_with_newline() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn jsonl_codec_roundtrip_hello() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn jsonl_codec_roundtrip_fatal() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, "boom");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn jsonl_codec_decode_stream_multiple() {
    let envelopes = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "e1".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e2".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e3".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

#[test]
fn jsonl_codec_decode_stream_skips_blank_lines() {
    let mut data = String::new();
    data.push_str(
        &JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "x".into(),
            error_code: None,
        })
        .unwrap(),
    );
    data.push('\n'); // blank line
    data.push_str(
        &JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "y".into(),
            error_code: None,
        })
        .unwrap(),
    );
    let reader = BufReader::new(data.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

#[test]
fn jsonl_codec_decode_invalid_json_fails() {
    let result = JsonlCodec::decode("not valid json at all");
    assert!(result.is_err());
}

#[test]
fn jsonl_codec_roundtrip_run_envelope() {
    let wo = make_work_order("codec test");
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.task, "codec test");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn jsonl_codec_roundtrip_event_envelope() {
    let evt = Envelope::Event {
        ref_id: "abc".into(),
        event: make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }),
    };
    let line = JsonlCodec::encode(&evt).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn jsonl_codec_roundtrip_final_envelope() {
    let fin = Envelope::Final {
        ref_id: "xyz".into(),
        receipt: make_hashed_receipt("mock", Outcome::Complete),
    };
    let line = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert!(receipt.receipt_sha256.is_some());
    } else {
        panic!("expected Final");
    }
}

// ===========================================================================
// 5. Canonical JSON and version helpers
// ===========================================================================

#[test]
fn canonical_json_deterministic() {
    let r = make_receipt("mock", Outcome::Complete);
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_json_btreemap_sorted_keys() {
    let mut map = BTreeMap::new();
    map.insert("zebra", 1);
    map.insert("alpha", 2);
    map.insert("middle", 3);
    let json = canonical_json(&map).unwrap();
    let alpha_pos = json.find("alpha").unwrap();
    let middle_pos = json.find("middle").unwrap();
    let zebra_pos = json.find("zebra").unwrap();
    assert!(alpha_pos < middle_pos);
    assert!(middle_pos < zebra_pos);
}

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
    assert!(is_compatible_version("abp/v0.1", CONTRACT_VERSION));
}

#[test]
fn version_incompatibility_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

// ===========================================================================
// 6. Outcome / ExecutionMode / SupportLevel serde
// ===========================================================================

#[test]
fn serde_roundtrip_all_outcomes() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let o2: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, o2);
    }
}

#[test]
fn serde_roundtrip_execution_modes() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let json = serde_json::to_string(&mode).unwrap();
        let m2: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, m2);
    }
}

#[test]
fn serde_roundtrip_execution_lanes() {
    for lane in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        let json = serde_json::to_string(&lane).unwrap();
        let l2: ExecutionLane = serde_json::from_str(&json).unwrap();
        assert_eq!(lane, l2);
    }
}

#[test]
fn serde_roundtrip_workspace_modes() {
    for mode in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        let json = serde_json::to_string(&mode).unwrap();
        let m2: WorkspaceMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, m2);
    }
}

#[test]
fn serde_roundtrip_support_levels() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "beta only".into(),
        },
    ];
    for level in levels {
        let json = serde_json::to_string(&level).unwrap();
        let l2: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(level, l2);
    }
}

#[test]
fn serde_roundtrip_usage_normalized() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.003),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let u2: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(u2.input_tokens, Some(100));
    assert_eq!(u2.output_tokens, Some(200));
    assert_eq!(u2.estimated_cost_usd, Some(0.003));
}

#[test]
fn serde_roundtrip_verification_report() {
    let vr = VerificationReport {
        git_diff: Some("diff --git ...".into()),
        git_status: Some("M src/main.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&vr).unwrap();
    let vr2: VerificationReport = serde_json::from_str(&json).unwrap();
    assert!(vr2.harness_ok);
    assert!(vr2.git_diff.is_some());
}

#[test]
fn serde_roundtrip_policy_profile() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into(), "read".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec!["/etc/shadow".into()],
        deny_write: vec!["/boot/**".into()],
        allow_network: vec!["api.github.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["bash".into()],
    };
    let json = serde_json::to_string(&policy).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(p2.allowed_tools, vec!["bash", "read"]);
    assert_eq!(p2.deny_network, vec!["*.evil.com"]);
}

#[test]
fn serde_roundtrip_workspace_spec() {
    let ws = WorkspaceSpec {
        root: "/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string(&ws).unwrap();
    let ws2: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(ws2.root, "/project");
    assert_eq!(ws2.mode, WorkspaceMode::Staged);
}

#[test]
fn serde_roundtrip_backend_identity() {
    let bi = BackendIdentity {
        id: "claude".into(),
        backend_version: Some("3.5".into()),
        adapter_version: Some("0.1".into()),
    };
    let json = serde_json::to_string(&bi).unwrap();
    let bi2: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(bi2.id, "claude");
    assert_eq!(bi2.backend_version.as_deref(), Some("3.5"));
}

#[test]
fn serde_roundtrip_artifact_ref() {
    let art = ArtifactRef {
        kind: "patch".into(),
        path: "/tmp/out.diff".into(),
    };
    let json = serde_json::to_string(&art).unwrap();
    let art2: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(art2.kind, "patch");
    assert_eq!(art2.path, "/tmp/out.diff");
}

#[test]
fn serde_roundtrip_context_packet() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into(), "b.rs".into()],
        snippets: vec![ContextSnippet {
            name: "helper".into(),
            content: "fn help() {}".into(),
        }],
    };
    let json = serde_json::to_string(&ctx).unwrap();
    let ctx2: ContextPacket = serde_json::from_str(&json).unwrap();
    assert_eq!(ctx2.files.len(), 2);
    assert_eq!(ctx2.snippets[0].name, "helper");
}
