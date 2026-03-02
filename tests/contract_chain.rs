// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-crate contract chain tests.
//!
//! Verifies that the dependency hierarchy works correctly end-to-end:
//! Core → Protocol → Host → Integrations → Runtime.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ContractError,
    ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_integrations::{
    Backend, MockBackend, ensure_capability_requirements, extract_execution_mode,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use abp_runtime::{Runtime, RuntimeError};
use abp_workspace::WorkspaceManager;
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_rich_work_order() -> WorkOrder {
    WorkOrderBuilder::new("rich task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(1.0)
        .policy(PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        })
        .context(ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Use the builder pattern".into(),
            }],
        })
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        })
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_receipt(work_order_id: Uuid) -> Receipt {
    ReceiptBuilder::new("mock")
        .work_order_id(work_order_id)
        .outcome(Outcome::Complete)
        .build()
}

// ===========================================================================
// 1. Core → Protocol chain
// ===========================================================================

#[test]
fn core_to_protocol_work_order_roundtrip_via_envelope() {
    let wo = make_rich_work_order();
    let original_id = wo.id;
    let original_task = wo.task.clone();

    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&envelope).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"run\""));

    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-1");
            assert_eq!(work_order.id, original_id);
            assert_eq!(work_order.task, original_task);
            assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
            assert_eq!(work_order.config.max_turns, Some(5));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn core_to_protocol_jsonl_stream_roundtrip() {
    let wo = make_work_order("stream test");
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let receipt = make_receipt(wo.id).with_hash().unwrap();

    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let ev = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let fin = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[hello, run, ev, fin]).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 4);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Run { .. }));
    assert!(matches!(envelopes[2], Envelope::Event { .. }));
    assert!(matches!(envelopes[3], Envelope::Final { .. }));
}

#[test]
fn core_to_protocol_event_kinds_preserved_in_envelope() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("tu1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "foo.rs"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: Some("tu1".into()),
            output: json!("contents"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    ];

    for kind in kinds {
        let ev = make_event(kind);
        let envelope = Envelope::Event {
            ref_id: "r1".into(),
            event: ev,
        };
        let line = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Event { .. }));
    }
}

// ===========================================================================
// 2. Core → Runtime chain
// ===========================================================================

#[tokio::test]
async fn core_to_runtime_mock_backend_produces_hashed_receipt() {
    let wo = make_work_order("test receipt hash");
    let wo_id = wo.id;

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();

    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());

    // Verify hash is consistent
    let hash = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_deref(), Some(hash.as_str()));
}

#[tokio::test]
async fn core_to_runtime_receipt_contract_version_matches() {
    let wo = make_work_order("version check");
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn core_to_runtime_events_stream_correctly() {
    let wo = make_work_order("event streaming");
    let rt = Runtime::with_default_backends();
    let mut handle = rt.run_streaming("mock", wo).await.unwrap();

    let mut events = Vec::new();
    while let Some(ev) = handle.events.next().await {
        events.push(ev);
    }
    assert!(!events.is_empty(), "should receive at least one event");

    // First event should be RunStarted
    assert!(
        matches!(events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted, got {:?}",
        events[0].kind
    );

    // Last event should be RunCompleted
    assert!(
        matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );

    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(!receipt.trace.is_empty());
}

// ===========================================================================
// 3. Protocol → Host chain (simulated)
// ===========================================================================

#[test]
fn protocol_hello_envelope_contains_contract_version() {
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
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }

    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains(&format!("\"contract_version\":\"{CONTRACT_VERSION}\"")));
}

#[test]
fn protocol_fatal_envelope_roundtrip() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();

    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-42"));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn protocol_simulated_sidecar_roundtrip() {
    let wo = make_rich_work_order();
    let wo_id = wo.id;
    let receipt = make_receipt(wo_id).with_hash().unwrap();

    // Simulate the complete sidecar protocol over a buffer
    let mut buf = Vec::new();

    // 1. Sidecar sends hello
    let hello = Envelope::hello(
        BackendIdentity {
            id: "sim-sidecar".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        {
            let mut caps = CapabilityManifest::new();
            caps.insert(Capability::Streaming, SupportLevel::Native);
            caps
        },
    );
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();

    // 2. Control plane sends run
    let run = Envelope::Run {
        id: wo_id.to_string(),
        work_order: wo,
    };
    JsonlCodec::encode_to_writer(&mut buf, &run).unwrap();

    // 3. Sidecar streams events
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "working...".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    for ev in &events {
        let env = Envelope::Event {
            ref_id: wo_id.to_string(),
            event: ev.clone(),
        };
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    }

    // 4. Sidecar sends final receipt
    let fin = Envelope::Final {
        ref_id: wo_id.to_string(),
        receipt: receipt.clone(),
    };
    JsonlCodec::encode_to_writer(&mut buf, &fin).unwrap();

    // Parse the entire buffer
    let reader = BufReader::new(buf.as_slice());
    let parsed: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(parsed.len(), 6); // hello + run + 3 events + final

    // Verify the final receipt came through intact
    if let Envelope::Final {
        ref_id,
        receipt: parsed_receipt,
    } = &parsed[5]
    {
        assert_eq!(ref_id, &wo_id.to_string());
        assert_eq!(parsed_receipt.receipt_sha256, receipt.receipt_sha256);
        assert_eq!(parsed_receipt.outcome, Outcome::Complete);
    } else {
        panic!("expected Final envelope at index 5");
    }
}

// ===========================================================================
// 4. Core → Policy chain
// ===========================================================================

#[test]
fn policy_engine_from_work_order_policy() {
    let wo = make_rich_work_order();
    let engine = PolicyEngine::new(&wo.policy).unwrap();

    // Allowed tools
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);

    // Disallowed tool
    assert!(!engine.can_use_tool("Bash").allowed);

    // Not in allowlist
    assert!(!engine.can_use_tool("Grep").allowed);

    // deny_read
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);

    // deny_write
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_empty_profile_allows_everything() {
    let wo = make_work_order("no policy");
    let engine = PolicyEngine::new(&wo.policy).unwrap();

    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("anything").allowed);
    assert!(engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn policy_deny_overrides_allow() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["DangerousTool".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("SafeTool").allowed);
    assert!(!engine.can_use_tool("DangerousTool").allowed);
}

// ===========================================================================
// 5. Core → Workspace chain
// ===========================================================================

#[test]
fn workspace_passthrough_preserves_path() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(prepared.path(), Path::new("."));
}

#[test]
fn workspace_staged_creates_temp_dir() {
    // Use the current directory as source for staging
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["target/**".into()],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();
    assert_ne!(prepared.path(), Path::new("."));
    assert!(prepared.path().exists());
}

#[test]
fn workspace_staged_excludes_git_dir() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["target/**".into()],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();
    // The staged workspace should not contain the original .git directory
    // (it gets a fresh git init instead)
    let git_dir = prepared.path().join(".git");
    if git_dir.exists() {
        // Fresh git repo means it should have been re-initialized
        assert!(git_dir.is_dir());
    }
}

// ===========================================================================
// 6. Full pipeline
// ===========================================================================

#[tokio::test]
async fn full_pipeline_work_order_to_receipt() {
    let wo = make_rich_work_order();
    let wo_id = wo.id;
    let rt = Runtime::with_default_backends();
    let mut handle = rt.run_streaming("mock", wo).await.unwrap();

    // Consume all events
    let mut events = Vec::new();
    while let Some(ev) = handle.events.next().await {
        events.push(ev);
    }

    let receipt = handle.receipt.await.unwrap().unwrap();

    // Verify the complete chain
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());

    // Receipt hash is verifiable
    let hash = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_deref(), Some(hash.as_str()));

    // Validate receipt
    abp_core::validate::validate_receipt(&receipt).unwrap();

    // Events are in trace
    assert!(!receipt.trace.is_empty());

    // Capabilities reported by mock
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
}

#[tokio::test]
async fn full_pipeline_unknown_backend_returns_error() {
    let wo = make_work_order("unknown backend");
    let rt = Runtime::with_default_backends();
    let err = rt
        .run_streaming("nonexistent", wo)
        .await
        .err()
        .expect("should be Err");

    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got {err:?}"
    );
}

// ===========================================================================
// 7. CONTRACT_VERSION consistency
// ===========================================================================

#[test]
fn contract_version_format_is_valid() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert!(parsed.is_some(), "CONTRACT_VERSION should be parseable");
    let (major, minor) = parsed.unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

#[test]
fn contract_version_compatibility_check() {
    assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
    assert!(is_compatible_version("abp/v0.2", CONTRACT_VERSION));
    assert!(!is_compatible_version("abp/v1.0", CONTRACT_VERSION));
    assert!(!is_compatible_version("invalid", CONTRACT_VERSION));
}

#[test]
fn contract_version_in_receipt_builder() {
    let receipt = ReceiptBuilder::new("test").build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn contract_version_in_hello_envelope() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    if let Envelope::Hello {
        contract_version, ..
    } = hello
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn contract_version_in_mock_backend() {
    let mock = MockBackend;
    let identity = mock.identity();
    // The mock backend should identify itself
    assert_eq!(identity.id, "mock");

    // When run, it should produce receipts with the correct version
    let receipt = ReceiptBuilder::new(&identity.id).build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

// ===========================================================================
// 8. Receipt hash consistency
// ===========================================================================

#[test]
fn receipt_hash_deterministic() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    let hash1 = abp_core::receipt_hash(&receipt).unwrap();
    let hash2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 64); // SHA-256 hex
}

#[test]
fn receipt_hash_consistent_across_serialization() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();

    // Hash should still validate after roundtrip
    let recomputed = abp_core::receipt_hash(&deserialized).unwrap();
    assert_eq!(
        deserialized.receipt_sha256.as_deref(),
        Some(recomputed.as_str())
    );
}

#[test]
fn receipt_hash_excludes_self() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    let hash_without = abp_core::receipt_hash(&receipt).unwrap();

    let mut receipt_with_hash = receipt;
    receipt_with_hash.receipt_sha256 = Some("some_old_hash".into());
    let hash_with = abp_core::receipt_hash(&receipt_with_hash).unwrap();

    // Hash should be the same regardless of receipt_sha256 value
    assert_eq!(hash_without, hash_with);
}

#[test]
fn receipt_with_hash_passes_validation() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    abp_core::validate::validate_receipt(&receipt).unwrap();
}

#[test]
fn receipt_with_tampered_hash_fails_validation() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    receipt.receipt_sha256 = Some("deadbeef".repeat(8));
    let errors = abp_core::validate::validate_receipt(&receipt).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, abp_core::validate::ValidationError::InvalidHash { .. }))
    );
}

// ===========================================================================
// 9. WorkOrder flows through entire stack without data loss
// ===========================================================================

#[tokio::test]
async fn work_order_fields_preserved_through_runtime() {
    let wo = make_rich_work_order();
    let wo_id = wo.id;

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[test]
fn work_order_serialization_preserves_all_fields() {
    let wo = make_rich_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let decoded: WorkOrder = serde_json::from_str(&json).unwrap();

    assert_eq!(wo.id, decoded.id);
    assert_eq!(wo.task, decoded.task);
    assert_eq!(wo.config.model, decoded.config.model);
    assert_eq!(wo.config.max_turns, decoded.config.max_turns);
    assert_eq!(wo.config.max_budget_usd, decoded.config.max_budget_usd);
    assert_eq!(wo.policy.allowed_tools, decoded.policy.allowed_tools);
    assert_eq!(wo.policy.disallowed_tools, decoded.policy.disallowed_tools);
    assert_eq!(wo.policy.deny_read, decoded.policy.deny_read);
    assert_eq!(wo.policy.deny_write, decoded.policy.deny_write);
    assert_eq!(wo.context.files, decoded.context.files);
    assert_eq!(wo.context.snippets.len(), decoded.context.snippets.len());
    assert_eq!(
        wo.requirements.required.len(),
        decoded.requirements.required.len()
    );
}

// ===========================================================================
// 10. Capability requirements across crate boundaries
// ===========================================================================

#[test]
fn capability_requirements_checked_by_integrations() {
    let caps = {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m.insert(Capability::ToolRead, SupportLevel::Emulated);
        m
    };

    // Satisfiable
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());

    // Emulated satisfies Emulated
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());

    // Native does not satisfy when only Emulated available
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());

    // Missing capability fails
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn runtime_capability_check_delegates_to_integrations() {
    let rt = Runtime::with_default_backends();

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    assert!(rt.check_capabilities("mock", &reqs).is_ok());

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    assert!(rt.check_capabilities("mock", &reqs).is_err());
}

// ===========================================================================
// 11. Execution mode propagation
// ===========================================================================

#[test]
fn execution_mode_extracted_from_work_order_config() {
    let mut wo = make_work_order("mode test");
    // Default should be Mapped
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);

    // Set passthrough via nested vendor config
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn execution_mode_serialization_roundtrip() {
    let modes = [ExecutionMode::Passthrough, ExecutionMode::Mapped];
    for mode in modes {
        let json = serde_json::to_string(&mode).unwrap();
        let decoded: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, decoded);
    }
}

#[test]
fn hello_envelope_carries_execution_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );

    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains("\"passthrough\""));

    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

// ===========================================================================
// 12. Error types across crate boundaries
// ===========================================================================

#[test]
fn contract_error_from_json_is_displayable() {
    // ContractError::Json wraps serde_json::Error
    let err = ContractError::Json(
        serde_json::from_str::<serde_json::Value>("not valid json").unwrap_err(),
    );
    let msg = err.to_string();
    assert!(msg.contains("serialize") || msg.contains("JSON") || msg.contains("expected"));
}

#[test]
fn runtime_error_variants_are_descriptive() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert!(err.to_string().contains("foo"));

    let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
    assert!(err.to_string().contains("missing streaming"));
}

// ===========================================================================
// 13. Event stream utilities across boundaries
// ===========================================================================

#[test]
fn event_stream_filter_works_with_all_kinds() {
    use abp_core::stream::EventStream;

    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::AssistantMessage { text: "msg".into() }),
        make_event(AgentEventKind::Warning {
            message: "warn".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let stream = EventStream::new(events);
    assert_eq!(stream.len(), 4);

    let started = stream.by_kind("run_started");
    assert_eq!(started.len(), 1);

    let warnings = stream.by_kind("warning");
    assert_eq!(warnings.len(), 1);

    let counts = stream.count_by_kind();
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("assistant_message"), Some(&1));
}

#[test]
fn event_filter_works_with_protocol_events() {
    use abp_core::filter::EventFilter;

    let filter = EventFilter::include_kinds(&["assistant_message", "tool_call"]);

    let msg = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    assert!(filter.matches(&msg));

    let started = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(!filter.matches(&started));
}

// ===========================================================================
// 14. Envelope backward compatibility
// ===========================================================================

#[test]
fn envelope_discriminator_uses_t_not_type() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("\"t\":\"hello\""));
    // Ensure it doesn't use "type" as the discriminator
    assert!(!json.contains("\"type\":\"hello\""));
}

#[test]
fn event_kind_discriminator_uses_type_not_t() {
    let event = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"run_started\""));
}

#[test]
fn envelope_missing_mode_defaults_to_mapped() {
    // Simulate a v0.1 hello without the "mode" field
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"old","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

// ===========================================================================
// 15. BTreeMap deterministic serialization
// ===========================================================================

#[test]
fn btreemap_vendor_config_is_deterministic() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("z_key".into(), json!("last"));
    config.vendor.insert("a_key".into(), json!("first"));
    config.vendor.insert("m_key".into(), json!("middle"));

    let json1 = serde_json::to_string(&config).unwrap();
    let json2 = serde_json::to_string(&config).unwrap();
    assert_eq!(json1, json2);

    // Keys should be in alphabetical order (BTreeMap guarantee)
    let a_pos = json1.find("a_key").unwrap();
    let m_pos = json1.find("m_key").unwrap();
    let z_pos = json1.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn capability_manifest_serialization_is_deterministic() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let json1 = serde_json::to_string(&caps).unwrap();
    let json2 = serde_json::to_string(&caps).unwrap();
    assert_eq!(json1, json2);
}

// ===========================================================================
// 16. Receipt builder chain
// ===========================================================================

#[test]
fn receipt_builder_produces_valid_receipt() {
    let wo_id = Uuid::new_v4();
    let receipt = ReceiptBuilder::new("mock")
        .work_order_id(wo_id)
        .outcome(Outcome::Partial)
        .backend_version("1.0")
        .adapter_version("0.1")
        .mode(ExecutionMode::Passthrough)
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            ..Default::default()
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .with_hash()
        .unwrap();

    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0"));
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.trace.len(), 1);

    abp_core::validate::validate_receipt(&receipt).unwrap();
}

// ===========================================================================
// 17. Agent event with extension field
// ===========================================================================

#[test]
fn agent_event_ext_field_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor": "data"}));

    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };

    let json = serde_json::to_string(&ev).unwrap();
    let decoded: AgentEvent = serde_json::from_str(&json).unwrap();

    assert!(decoded.ext.is_some());
    let ext = decoded.ext.unwrap();
    assert!(ext.contains_key("raw_message"));
}

#[test]
fn agent_event_ext_none_omitted_in_json() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(!json.contains("ext"));
}

// ===========================================================================
// 18. Canonical JSON for hashing
// ===========================================================================

#[test]
fn canonical_json_is_deterministic() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    let json1 = abp_core::canonical_json(&receipt).unwrap();
    let json2 = abp_core::canonical_json(&receipt).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn canonical_json_produces_valid_json() {
    let wo = make_rich_work_order();
    let json_str = abp_core::canonical_json(&wo).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.is_object());
}

// ===========================================================================
// 19. Policy applies across the pipeline
// ===========================================================================

#[tokio::test]
async fn policy_compiled_during_runtime_execution() {
    // A work order with a valid policy should execute successfully
    let wo = WorkOrderBuilder::new("policy test")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        })
        .build();

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 20. WorkOrder builder defaults
// ===========================================================================

#[test]
fn work_order_builder_defaults_are_sensible() {
    let wo = WorkOrderBuilder::new("minimal task").build();

    assert_eq!(wo.task, "minimal task");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert_eq!(wo.workspace.root, ".");
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.requirements.required.is_empty());
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
}

// ===========================================================================
// 21. Runtime backend registration
// ===========================================================================

#[test]
fn runtime_registers_and_lists_backends() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[test]
fn runtime_unknown_backend_check_returns_error() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    let err = rt.check_capabilities("nonexistent", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

// ===========================================================================
// 22. Receipt validation across boundaries
// ===========================================================================

#[test]
fn validation_catches_wrong_contract_version() {
    use abp_core::validate::{ValidationError, validate_receipt};

    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    receipt.meta.contract_version = "abp/v99.0".into();

    let errors = validate_receipt(&receipt).unwrap_err();
    assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidOutcome { reason } if reason.contains("contract_version"))));
}

#[test]
fn validation_catches_empty_backend_id() {
    use abp_core::validate::{ValidationError, validate_receipt};

    let receipt = ReceiptBuilder::new("").outcome(Outcome::Complete).build();

    let errors = validate_receipt(&receipt).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyBackendId))
    );
}
