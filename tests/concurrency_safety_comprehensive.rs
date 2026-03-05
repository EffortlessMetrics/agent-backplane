#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive concurrency and thread safety tests for ABP.
//!
//! 100+ tests covering Send/Sync bounds, concurrent operations,
//! Arc/Mutex patterns, channel patterns, and race condition guards.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirements, ContextPacket, ContractError, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec, canonical_json, receipt_hash, sha256_hex,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::{Decision, PolicyEngine};
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::ReceiptBuilder as ReceiptReceiptBuilder;
use chrono::Utc;
use uuid::Uuid;

// =====================================================================
// Helpers
// =====================================================================

fn requires_send<T: Send>() {}
fn requires_sync<T: Sync>() {}
fn requires_send_sync<T: Send + Sync>() {}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

fn make_hashed_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap()
}

fn make_policy_engine() -> PolicyEngine {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".to_string()],
        disallowed_tools: vec!["Bash".to_string()],
        deny_read: vec!["**/.env".to_string()],
        deny_write: vec!["**/.git/**".to_string()],
        ..PolicyProfile::default()
    };
    PolicyEngine::new(&policy).unwrap()
}

fn make_envelope_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_envelope_fatal() -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "test error".into(),
        error_code: Some(ErrorCode::Internal),
    }
}

fn make_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        ..Default::default()
    }
}

// =====================================================================
// Section 1: Send + Sync bounds (25+ tests)
// =====================================================================

#[test]
fn send_sync_work_order() {
    requires_send_sync::<WorkOrder>();
}

#[test]
fn send_sync_receipt() {
    requires_send_sync::<Receipt>();
}

#[test]
fn send_sync_agent_event() {
    requires_send_sync::<AgentEvent>();
}

#[test]
fn send_sync_envelope() {
    requires_send_sync::<Envelope>();
}

#[test]
fn send_sync_error_code() {
    requires_send_sync::<ErrorCode>();
}

#[test]
fn send_sync_abp_error() {
    requires_send_sync::<AbpError>();
}

#[test]
fn send_sync_policy_engine() {
    requires_send_sync::<PolicyEngine>();
}

#[test]
fn send_sync_backplane_config() {
    requires_send_sync::<BackplaneConfig>();
}

#[test]
fn send_sync_capability_manifest() {
    requires_send_sync::<CapabilityManifest>();
}

#[test]
fn send_sync_agent_event_kind() {
    requires_send_sync::<AgentEventKind>();
}

#[test]
fn send_sync_execution_mode() {
    requires_send_sync::<ExecutionMode>();
}

#[test]
fn send_sync_execution_lane() {
    requires_send_sync::<ExecutionLane>();
}

#[test]
fn send_sync_outcome() {
    requires_send_sync::<Outcome>();
}

#[test]
fn send_sync_backend_identity() {
    requires_send_sync::<BackendIdentity>();
}

#[test]
fn send_sync_run_metadata() {
    requires_send_sync::<RunMetadata>();
}

#[test]
fn send_sync_usage_normalized() {
    requires_send_sync::<UsageNormalized>();
}

#[test]
fn send_sync_verification_report() {
    requires_send_sync::<VerificationReport>();
}

#[test]
fn send_sync_workspace_spec() {
    requires_send_sync::<WorkspaceSpec>();
}

#[test]
fn send_sync_workspace_mode() {
    requires_send_sync::<WorkspaceMode>();
}

#[test]
fn send_sync_policy_profile() {
    requires_send_sync::<PolicyProfile>();
}

#[test]
fn send_sync_capability() {
    requires_send_sync::<Capability>();
}

#[test]
fn send_sync_support_level() {
    requires_send_sync::<SupportLevel>();
}

#[test]
fn send_sync_min_support() {
    requires_send_sync::<MinSupport>();
}

#[test]
fn send_sync_capability_requirements() {
    requires_send_sync::<CapabilityRequirements>();
}

#[test]
fn send_sync_context_packet() {
    requires_send_sync::<ContextPacket>();
}

#[test]
fn send_sync_runtime_config() {
    requires_send_sync::<RuntimeConfig>();
}

#[test]
fn send_sync_artifact_ref() {
    requires_send_sync::<ArtifactRef>();
}

#[test]
fn send_sync_error_category() {
    requires_send_sync::<ErrorCategory>();
}

#[test]
fn send_sync_error_info() {
    requires_send_sync::<ErrorInfo>();
}

#[test]
fn send_sync_decision() {
    requires_send_sync::<Decision>();
}

#[test]
fn send_sync_match_decision() {
    requires_send_sync::<MatchDecision>();
}

#[test]
fn send_sync_include_exclude_globs() {
    requires_send_sync::<IncludeExcludeGlobs>();
}

#[test]
fn send_sync_backend_entry() {
    requires_send_sync::<BackendEntry>();
}

// =====================================================================
// Section 2: Concurrent operations (25+ tests)
// =====================================================================

#[test]
fn concurrent_receipt_hashing_same_result() {
    let receipt = make_receipt("mock");
    let expected = receipt_hash(&receipt).unwrap();
    let receipt_arc = Arc::new(receipt);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let r = Arc::clone(&receipt_arc);
            thread::spawn(move || receipt_hash(&r).unwrap())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn concurrent_config_reading_is_safe() {
    let config = Arc::new(make_config());

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let c = Arc::clone(&config);
            thread::spawn(move || {
                assert_eq!(c.default_backend.as_deref(), Some("mock"));
                assert!(c.backends.contains_key("mock"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_policy_evaluation_is_safe() {
    let engine = Arc::new(make_policy_engine());

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let e = Arc::clone(&engine);
            thread::spawn(move || {
                assert!(!e.can_use_tool("Bash").allowed);
                assert!(e.can_use_tool("Read").allowed);
                assert!(!e.can_read_path(Path::new(".env")).allowed);
                assert!(!e.can_write_path(Path::new(".git/config")).allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_serde_roundtrip_work_order() {
    let wo = make_work_order("concurrent task");
    let json = serde_json::to_string(&wo).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let j = json.clone();
            thread::spawn(move || {
                let deserialized: WorkOrder = serde_json::from_str(&j).unwrap();
                assert_eq!(deserialized.task, "concurrent task");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_serde_roundtrip_receipt() {
    let receipt = make_receipt("mock");
    let json = serde_json::to_string(&receipt).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let j = json.clone();
            thread::spawn(move || {
                let deserialized: Receipt = serde_json::from_str(&j).unwrap();
                assert_eq!(deserialized.backend.id, "mock");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_envelope_encoding() {
    let env = make_envelope_hello();
    let arc_env = Arc::new(env);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let e = Arc::clone(&arc_env);
            thread::spawn(move || {
                let line = JsonlCodec::encode(&e).unwrap();
                assert!(line.contains("\"t\":\"hello\""));
                assert!(line.ends_with('\n'));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_envelope_decoding() {
    let json_line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let line = json_line.to_string();
            thread::spawn(move || {
                let env = JsonlCodec::decode(&line).unwrap();
                assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_canonical_json_generation() {
    let value = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let expected = canonical_json(&value).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let v = value.clone();
            let exp = expected.clone();
            thread::spawn(move || {
                let result = canonical_json(&v).unwrap();
                assert_eq!(result, exp);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_capability_manifest_access() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    manifest.insert(Capability::ToolBash, SupportLevel::Unsupported);
    let manifest = Arc::new(manifest);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let m = Arc::clone(&manifest);
            thread::spawn(move || {
                assert!(m.contains_key(&Capability::Streaming));
                assert!(m.contains_key(&Capability::ToolRead));
                assert!(m.contains_key(&Capability::ToolBash));
                assert_eq!(m.len(), 3);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_glob_matching() {
    let globs = Arc::new(
        IncludeExcludeGlobs::new(
            &["src/**".into(), "tests/**".into()],
            &["src/generated/**".into()],
        )
        .unwrap(),
    );

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let g = Arc::clone(&globs);
            thread::spawn(move || {
                assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
                assert_eq!(
                    g.decide_str("src/generated/out.rs"),
                    MatchDecision::DeniedByExclude
                );
                assert_eq!(
                    g.decide_str("README.md"),
                    MatchDecision::DeniedByMissingInclude
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_sha256_hex_computation() {
    let data = b"agent backplane concurrency test";
    let expected = sha256_hex(data);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let exp = expected.clone();
            thread::spawn(move || {
                let result = sha256_hex(data);
                assert_eq!(result, exp);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_error_code_operations() {
    let code = ErrorCode::BackendTimeout;

    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(move || {
                assert_eq!(code.as_str(), "backend_timeout");
                assert_eq!(code.category(), ErrorCategory::Backend);
                assert!(code.is_retryable());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_error_info_serialization() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "access denied");
    let json = serde_json::to_string(&info).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let j = json.clone();
            thread::spawn(move || {
                let deserialized: ErrorInfo = serde_json::from_str(&j).unwrap();
                assert_eq!(deserialized.code, ErrorCode::PolicyDenied);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_work_order_construction() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let wo = WorkOrderBuilder::new(format!("task-{i}")).build();
                assert_eq!(wo.task, format!("task-{i}"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_receipt_construction() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let receipt = ReceiptBuilder::new(format!("backend-{i}"))
                    .outcome(Outcome::Complete)
                    .build();
                assert_eq!(receipt.backend.id, format!("backend-{i}"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_agent_event_creation() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let event = make_event(AgentEventKind::AssistantMessage {
                    text: format!("msg-{i}"),
                });
                if let AgentEventKind::AssistantMessage { text } = &event.kind {
                    assert_eq!(text, &format!("msg-{i}"));
                } else {
                    panic!("unexpected event kind");
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_envelope_roundtrip() {
    let env = make_envelope_fatal();
    let encoded = JsonlCodec::encode(&env).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let e = encoded.clone();
            thread::spawn(move || {
                let decoded = JsonlCodec::decode(e.trim()).unwrap();
                let re_encoded = JsonlCodec::encode(&decoded).unwrap();
                let re_decoded = JsonlCodec::decode(re_encoded.trim()).unwrap();
                assert!(matches!(re_decoded, Envelope::Fatal { .. }));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_config_serde_roundtrip() {
    let config = make_config();
    let toml_str = toml::to_string(&config).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let s = toml_str.clone();
            thread::spawn(move || {
                let parsed: BackplaneConfig = toml::from_str(&s).unwrap();
                assert_eq!(parsed.default_backend.as_deref(), Some("mock"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_policy_engine_construction() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let policy = PolicyProfile {
                    disallowed_tools: vec![format!("Tool{i}")],
                    ..PolicyProfile::default()
                };
                let engine = PolicyEngine::new(&policy).unwrap();
                assert!(!engine.can_use_tool(&format!("Tool{i}")).allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_glob_construction() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let globs = IncludeExcludeGlobs::new(
                    &[format!("src/{i}/**")],
                    &[format!("src/{i}/generated/**")],
                )
                .unwrap();
                assert_eq!(
                    globs.decide_str(&format!("src/{i}/lib.rs")),
                    MatchDecision::Allowed
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_outcome_serde() {
    let outcomes = vec![Outcome::Complete, Outcome::Partial, Outcome::Failed];

    let handles: Vec<_> = outcomes
        .into_iter()
        .map(|o| {
            thread::spawn(move || {
                let json = serde_json::to_string(&o).unwrap();
                let deserialized: Outcome = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized, o);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_execution_mode_default() {
    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                let mode = ExecutionMode::default();
                assert_eq!(mode, ExecutionMode::Mapped);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_contract_version_access() {
    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                assert_eq!(CONTRACT_VERSION, "abp/v0.1");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_event_kind_serialization() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
    ];

    let handles: Vec<_> = kinds
        .into_iter()
        .map(|k| {
            thread::spawn(move || {
                let json = serde_json::to_string(&k).unwrap();
                let _: AgentEventKind = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// =====================================================================
// Section 3: Arc sharing patterns (15+ tests)
// =====================================================================

#[test]
fn arc_work_order_shareable() {
    let wo = Arc::new(make_work_order("shared task"));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let w = Arc::clone(&wo);
            thread::spawn(move || {
                assert_eq!(w.task, "shared task");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_receipt_shareable() {
    let receipt = Arc::new(make_receipt("mock"));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || {
                assert_eq!(r.backend.id, "mock");
                assert_eq!(r.outcome, Outcome::Complete);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_policy_engine_shareable() {
    let engine = Arc::new(make_policy_engine());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let e = Arc::clone(&engine);
            thread::spawn(move || {
                let d = e.can_use_tool("Read");
                assert!(d.allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_config_shareable() {
    let config = Arc::new(make_config());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let c = Arc::clone(&config);
            thread::spawn(move || {
                assert!(c.backends.contains_key("mock"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_envelope_shareable() {
    let env = Arc::new(make_envelope_hello());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let e = Arc::clone(&env);
            thread::spawn(move || {
                let encoded = JsonlCodec::encode(&e).unwrap();
                assert!(encoded.contains("hello"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_glob_shareable() {
    let globs =
        Arc::new(IncludeExcludeGlobs::new(&["**/*.rs".into()], &["target/**".into()]).unwrap());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let g = Arc::clone(&globs);
            thread::spawn(move || {
                assert!(g.decide_str("src/lib.rs").is_allowed());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_agent_event_shareable() {
    let event = Arc::new(make_event(AgentEventKind::AssistantMessage {
        text: "shared".into(),
    }));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let e = Arc::clone(&event);
            thread::spawn(move || {
                let json = serde_json::to_string(&*e).unwrap();
                assert!(json.contains("shared"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_concurrent_reads_work_order() {
    let wo = Arc::new(make_work_order("read test"));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let w = Arc::clone(&wo);
            thread::spawn(move || {
                let _ = w.task.len();
                let _ = w.config.model.as_ref();
                let _ = w.workspace.root.as_str();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_concurrent_reads_receipt() {
    let receipt = Arc::new(make_hashed_receipt("mock"));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || {
                let _ = r.meta.run_id;
                let _ = r.receipt_sha256.as_ref();
                let _ = r.backend.id.len();
                let _ = r.trace.len();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_n_threads_process_work_orders_independently() {
    let n = 16;
    let handles: Vec<_> = (0..n)
        .map(|i| {
            thread::spawn(move || {
                let wo = WorkOrderBuilder::new(format!("task-{i}")).build();
                let receipt = ReceiptBuilder::new(format!("backend-{i}"))
                    .outcome(Outcome::Complete)
                    .build()
                    .with_hash()
                    .unwrap();
                assert_eq!(wo.task, format!("task-{i}"));
                assert!(receipt.receipt_sha256.is_some());
                (wo.task.clone(), receipt.receipt_sha256.clone().unwrap())
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.len(), n);
    // All tasks are unique
    let tasks: std::collections::HashSet<_> = results.iter().map(|(t, _)| t.clone()).collect();
    assert_eq!(tasks.len(), n);
}

#[test]
fn arc_rwlock_work_order_concurrent_access() {
    let wo = Arc::new(RwLock::new(make_work_order("rw test")));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let w = Arc::clone(&wo);
            thread::spawn(move || {
                let guard = w.read().unwrap();
                assert_eq!(guard.task, "rw test");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_mutex_receipt_sequential_writes() {
    let receipt = Arc::new(Mutex::new(make_receipt("mock")));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || {
                let mut guard = r.lock().unwrap();
                guard
                    .trace
                    .push(make_event(AgentEventKind::AssistantMessage {
                        text: format!("thread-{i}"),
                    }));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let final_receipt = receipt.lock().unwrap();
    assert_eq!(final_receipt.trace.len(), 8);
}

#[test]
fn arc_error_info_shareable() {
    let info = Arc::new(ErrorInfo::new(ErrorCode::Internal, "shared error"));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let i = Arc::clone(&info);
            thread::spawn(move || {
                assert_eq!(i.code, ErrorCode::Internal);
                assert_eq!(i.message, "shared error");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_capability_manifest_shareable() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    let manifest = Arc::new(manifest);

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let m = Arc::clone(&manifest);
            thread::spawn(move || {
                assert!(m.contains_key(&Capability::Streaming));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn arc_backend_identity_shareable() {
    let id = Arc::new(BackendIdentity {
        id: "shared-backend".into(),
        backend_version: Some("2.0".into()),
        adapter_version: None,
    });

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let bi = Arc::clone(&id);
            thread::spawn(move || {
                assert_eq!(bi.id, "shared-backend");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// =====================================================================
// Section 4: Channel patterns (15+ tests)
// =====================================================================

#[tokio::test]
async fn channel_send_agent_event_mpsc() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    tx.send(event).await.unwrap();
    let received = rx.recv().await.unwrap();
    assert!(matches!(
        received.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn channel_send_receipt_mpsc() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let receipt = make_receipt("mock");
    tx.send(receipt).await.unwrap();
    let received = rx.recv().await.unwrap();
    assert_eq!(received.backend.id, "mock");
}

#[tokio::test]
async fn channel_send_envelope_mpsc() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let env = make_envelope_hello();
    tx.send(env).await.unwrap();
    let received = rx.recv().await.unwrap();
    assert!(matches!(received, Envelope::Hello { .. }));
}

#[tokio::test]
async fn channel_multi_producer_events() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);

    let mut tasks = Vec::new();
    for i in 0..4 {
        let tx_clone = tx.clone();
        tasks.push(tokio::spawn(async move {
            for j in 0..4 {
                let event = AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: format!("producer-{i}-msg-{j}"),
                    },
                    ext: None,
                };
                tx_clone.send(event).await.unwrap();
            }
        }));
    }
    drop(tx);

    let mut count = 0;
    while let Some(_event) = rx.recv().await {
        count += 1;
    }

    for t in tasks {
        t.await.unwrap();
    }
    assert_eq!(count, 16);
}

#[tokio::test]
async fn channel_bounded_backpressure() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(2);
    let tx_clone = tx.clone();

    let sender = tokio::spawn(async move {
        for i in 0..4 {
            let event = make_event(AgentEventKind::AssistantDelta {
                text: format!("msg-{i}"),
            });
            tx_clone.send(event).await.unwrap();
        }
    });

    let mut received = Vec::new();
    for _ in 0..4 {
        if let Some(e) = rx.recv().await {
            received.push(e);
        }
    }
    sender.await.unwrap();
    assert_eq!(received.len(), 4);
}

#[tokio::test]
async fn channel_unbounded_events() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

    for i in 0..100 {
        let event = make_event(AgentEventKind::AssistantDelta {
            text: format!("delta-{i}"),
        });
        tx.send(event).unwrap();
    }
    drop(tx);

    let mut count = 0;
    while let Some(_) = rx.recv().await {
        count += 1;
    }
    assert_eq!(count, 100);
}

#[test]
fn channel_std_mpsc_agent_event() {
    let (tx, rx) = std::sync::mpsc::channel();
    let event = make_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    tx.send(event).unwrap();
    let received = rx.recv().unwrap();
    assert!(matches!(received.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn channel_std_mpsc_receipt() {
    let (tx, rx) = std::sync::mpsc::channel();
    let receipt = make_hashed_receipt("mock");
    tx.send(receipt).unwrap();
    let received = rx.recv().unwrap();
    assert!(received.receipt_sha256.is_some());
}

#[test]
fn channel_std_mpsc_envelope() {
    let (tx, rx) = std::sync::mpsc::channel();
    let env = make_envelope_fatal();
    tx.send(env).unwrap();
    let received = rx.recv().unwrap();
    assert!(matches!(received, Envelope::Fatal { .. }));
}

#[test]
fn channel_std_mpsc_work_order() {
    let (tx, rx) = std::sync::mpsc::channel();
    let wo = make_work_order("channel test");
    tx.send(wo).unwrap();
    let received = rx.recv().unwrap();
    assert_eq!(received.task, "channel test");
}

#[test]
fn channel_std_multi_producer_events() {
    let (tx, rx) = std::sync::mpsc::channel();

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let sender = tx.clone();
            thread::spawn(move || {
                for j in 0..4 {
                    let event = make_event(AgentEventKind::AssistantDelta {
                        text: format!("p{i}-{j}"),
                    });
                    sender.send(event).unwrap();
                }
            })
        })
        .collect();

    drop(tx);
    let events: Vec<_> = rx.iter().collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(events.len(), 16);
}

#[test]
fn channel_std_mpsc_error_code() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(ErrorCode::BackendTimeout).unwrap();
    let received = rx.recv().unwrap();
    assert_eq!(received, ErrorCode::BackendTimeout);
}

#[test]
fn channel_std_mpsc_config() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(make_config()).unwrap();
    let received = rx.recv().unwrap();
    assert_eq!(received.default_backend.as_deref(), Some("mock"));
}

#[tokio::test]
async fn channel_tokio_oneshot_receipt() {
    let (tx, rx) = tokio::sync::oneshot::channel::<Receipt>();
    let receipt = make_hashed_receipt("oneshot-backend");
    tx.send(receipt).unwrap();
    let received = rx.await.unwrap();
    assert_eq!(received.backend.id, "oneshot-backend");
    assert!(received.receipt_sha256.is_some());
}

#[tokio::test]
async fn channel_tokio_broadcast_events() {
    let (tx, _) = tokio::sync::broadcast::channel::<AgentEvent>(16);
    let mut rx1 = tx.subscribe();
    let mut rx2 = tx.subscribe();

    let event = make_event(AgentEventKind::Warning {
        message: "broadcast-warn".into(),
    });
    tx.send(event).unwrap();

    let r1 = rx1.recv().await.unwrap();
    let r2 = rx2.recv().await.unwrap();
    assert!(matches!(r1.kind, AgentEventKind::Warning { .. }));
    assert!(matches!(r2.kind, AgentEventKind::Warning { .. }));
}

// =====================================================================
// Section 5: Race condition guards (20+ tests)
// =====================================================================

#[test]
fn deterministic_receipt_hash_under_concurrency() {
    // Build a receipt with fixed IDs for deterministic hashing
    let now = Utc::now();
    let run_id = Uuid::nil();
    let wo_id = Uuid::nil();

    let receipt = Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: wo_id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let expected_hash = receipt_hash(&receipt).unwrap();
    let receipt = Arc::new(receipt);

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || receipt_hash(&r).unwrap())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected_hash);
    }
}

#[test]
fn no_data_races_in_btreemap_serde() {
    let mut map = BTreeMap::new();
    map.insert("z_key".to_string(), serde_json::json!(1));
    map.insert("a_key".to_string(), serde_json::json!(2));
    map.insert("m_key".to_string(), serde_json::json!(3));
    let map = Arc::new(map);

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let m = Arc::clone(&map);
            thread::spawn(move || serde_json::to_string(&*m).unwrap())
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // BTreeMap ordering is deterministic — all results must be identical
    for r in &results {
        assert_eq!(r, &results[0]);
    }
    // Verify key order is alphabetical
    assert!(results[0].find("a_key").unwrap() < results[0].find("m_key").unwrap());
    assert!(results[0].find("m_key").unwrap() < results[0].find("z_key").unwrap());
}

#[test]
fn hash_deterministic_regardless_of_thread_scheduling() {
    let receipt = make_receipt("determinism-test");
    let json = serde_json::to_string(&receipt).unwrap();

    let handles: Vec<_> = (0..32)
        .map(|_| {
            let j = json.clone();
            thread::spawn(move || {
                let r: Receipt = serde_json::from_str(&j).unwrap();
                receipt_hash(&r).unwrap()
            })
        })
        .collect();

    let hashes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for h in &hashes {
        assert_eq!(h, &hashes[0]);
    }
}

#[test]
fn policy_evaluation_order_independent() {
    let engine = Arc::new(make_policy_engine());
    let tools = vec!["Read", "Write", "Bash", "Grep", "Glob"];

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let e = Arc::clone(&engine);
            let t = tools.clone();
            thread::spawn(move || {
                let results: Vec<_> = t.iter().map(|tool| e.can_use_tool(tool).allowed).collect();
                results
            })
        })
        .collect();

    let all_results: Vec<Vec<bool>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &all_results {
        assert_eq!(r, &all_results[0]);
    }
}

#[test]
fn canonical_json_deterministic_concurrent() {
    let value = serde_json::json!({
        "z": [3, 2, 1],
        "a": {"nested_z": true, "nested_a": false},
        "m": null
    });
    let expected = canonical_json(&value).unwrap();

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let v = value.clone();
            thread::spawn(move || canonical_json(&v).unwrap())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn concurrent_receipt_chain_building() {
    let n = 8;
    let handles: Vec<_> = (0..n)
        .map(|i| {
            thread::spawn(move || {
                let receipt = ReceiptBuilder::new(format!("chain-{i}"))
                    .outcome(Outcome::Complete)
                    .build()
                    .with_hash()
                    .unwrap();
                receipt
            })
        })
        .collect();

    let receipts: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All receipts should have valid hashes
    for r in &receipts {
        assert!(r.receipt_sha256.is_some());
        assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
    }
}

#[test]
fn concurrent_glob_evaluation_deterministic() {
    let globs = Arc::new(
        IncludeExcludeGlobs::new(
            &["src/**".into(), "tests/**".into()],
            &["**/generated/**".into(), "**/*.bak".into()],
        )
        .unwrap(),
    );

    let paths = vec![
        "src/lib.rs",
        "src/generated/types.rs",
        "tests/unit.rs",
        "README.md",
        "src/main.bak",
    ];

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let g = Arc::clone(&globs);
            let p = paths.clone();
            thread::spawn(move || p.iter().map(|path| g.decide_str(path)).collect::<Vec<_>>())
        })
        .collect();

    let all_results: Vec<Vec<MatchDecision>> =
        handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &all_results {
        assert_eq!(r, &all_results[0]);
    }
}

#[test]
fn concurrent_policy_read_path_evaluation() {
    let engine = Arc::new(make_policy_engine());
    let paths: Vec<&str> = vec![
        ".env",
        "src/lib.rs",
        "config/.env",
        "tests/test.rs",
        ".env.production",
    ];

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let e = Arc::clone(&engine);
            let p = paths.clone();
            thread::spawn(move || {
                p.iter()
                    .map(|path| e.can_read_path(Path::new(path)).allowed)
                    .collect::<Vec<_>>()
            })
        })
        .collect();

    let all_results: Vec<Vec<bool>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &all_results {
        assert_eq!(r, &all_results[0]);
    }
}

#[test]
fn concurrent_policy_write_path_evaluation() {
    let engine = Arc::new(make_policy_engine());
    let paths: Vec<&str> = vec![
        ".git/config",
        "src/lib.rs",
        ".git/HEAD",
        "Cargo.toml",
        ".git/refs/heads/main",
    ];

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let e = Arc::clone(&engine);
            let p = paths.clone();
            thread::spawn(move || {
                p.iter()
                    .map(|path| e.can_write_path(Path::new(path)).allowed)
                    .collect::<Vec<_>>()
            })
        })
        .collect();

    let all_results: Vec<Vec<bool>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &all_results {
        assert_eq!(r, &all_results[0]);
    }
}

#[test]
fn concurrent_envelope_encode_decode_deterministic() {
    let envs = vec![
        make_envelope_hello(),
        make_envelope_fatal(),
        Envelope::Run {
            id: "run-1".into(),
            work_order: make_work_order("test"),
        },
    ];

    let expected: Vec<_> = envs
        .iter()
        .map(|e| JsonlCodec::encode(e).unwrap())
        .collect();
    let envs = Arc::new(envs);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let e = Arc::clone(&envs);
            thread::spawn(move || {
                e.iter()
                    .map(|env| JsonlCodec::encode(env).unwrap())
                    .collect::<Vec<_>>()
            })
        })
        .collect();

    for h in handles {
        let results = h.join().unwrap();
        assert_eq!(results, expected);
    }
}

#[test]
fn sha256_hex_deterministic_concurrent() {
    let inputs: Vec<&[u8]> = vec![
        b"input 1",
        b"input 2",
        b"a longer input with more bytes for hashing",
        b"",
        b"\x00\x01\x02",
    ];

    let expected: Vec<_> = inputs.iter().map(|i| sha256_hex(i)).collect();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let inp = inputs.clone();
            thread::spawn(move || inp.iter().map(|i| sha256_hex(i)).collect::<Vec<_>>())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn concurrent_work_order_serde_deterministic() {
    let wo = make_work_order("deterministic serde");
    let expected = serde_json::to_string(&wo).unwrap();
    let wo_arc = Arc::new(wo);

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let w = Arc::clone(&wo_arc);
            thread::spawn(move || serde_json::to_string(&*w).unwrap())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn concurrent_receipt_serde_deterministic() {
    let receipt = make_hashed_receipt("deterministic");
    let expected = serde_json::to_string(&receipt).unwrap();
    let receipt_arc = Arc::new(receipt);

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let r = Arc::clone(&receipt_arc);
            thread::spawn(move || serde_json::to_string(&*r).unwrap())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn concurrent_mixed_operations_no_panic() {
    let wo = Arc::new(make_work_order("mixed ops"));
    let receipt = Arc::new(make_hashed_receipt("mixed"));
    let engine = Arc::new(make_policy_engine());
    let config = Arc::new(make_config());

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let w = Arc::clone(&wo);
            let r = Arc::clone(&receipt);
            let e = Arc::clone(&engine);
            let c = Arc::clone(&config);
            thread::spawn(move || {
                // Mix of operations
                let _ = serde_json::to_string(&*w).unwrap();
                let _ = receipt_hash(&r).unwrap();
                let _ = e.can_use_tool("Read").allowed;
                let _ = c.default_backend.as_ref();
                let _ = canonical_json(&serde_json::json!({"key": i})).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_error_code_equality_checks() {
    let codes = vec![
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::CapabilityUnsupported,
    ];
    let codes = Arc::new(codes);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let c = Arc::clone(&codes);
            thread::spawn(move || {
                let retryable: Vec<_> = c.iter().map(|code| code.is_retryable()).collect();
                let categories: Vec<_> = c.iter().map(|code| code.category()).collect();
                (retryable, categories)
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &results {
        assert_eq!(r, &results[0]);
    }
}

#[test]
fn concurrent_config_validation() {
    let config = make_config();
    let config_arc = Arc::new(config);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let c = Arc::clone(&config_arc);
            thread::spawn(move || {
                let warnings = abp_config::validate_config(&c).unwrap();
                warnings.len()
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &results {
        assert_eq!(r, &results[0]);
    }
}

#[test]
fn concurrent_envelope_fatal_with_error_code() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let env = Envelope::fatal_with_code(
                    Some(format!("run-{i}")),
                    format!("error-{i}"),
                    ErrorCode::BackendTimeout,
                );
                let code = env.error_code();
                assert_eq!(code, Some(ErrorCode::BackendTimeout));
                JsonlCodec::encode(&env).unwrap()
            })
        })
        .collect();

    for h in handles {
        let result = h.join().unwrap();
        assert!(result.contains("backend_timeout"));
    }
}

#[test]
fn concurrent_support_level_satisfies() {
    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
                assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
                assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
                assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
                assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_receipt_with_hash_idempotent() {
    let receipt = make_receipt("idempotent");
    let json = serde_json::to_string(&receipt).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let j = json.clone();
            thread::spawn(move || {
                let r: Receipt = serde_json::from_str(&j).unwrap();
                let hashed = r.with_hash().unwrap();
                let hash = hashed.receipt_sha256.clone().unwrap();
                // Hash again should produce same result
                let rehashed_value = receipt_hash(&hashed).unwrap();
                assert_eq!(hash, rehashed_value);
                hash
            })
        })
        .collect();

    let hashes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for h in &hashes {
        assert_eq!(h, &hashes[0]);
    }
}

#[test]
fn concurrent_config_merge_deterministic() {
    let base = BackplaneConfig {
        default_backend: Some("base-mock".into()),
        log_level: Some("info".into()),
        backends: BTreeMap::from([("base".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("overlay-mock".into()),
        backends: BTreeMap::from([("overlay".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let base_json = serde_json::to_string(&base).unwrap();
    let overlay_json = serde_json::to_string(&overlay).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let bj = base_json.clone();
            let oj = overlay_json.clone();
            thread::spawn(move || {
                let b: BackplaneConfig = serde_json::from_str(&bj).unwrap();
                let o: BackplaneConfig = serde_json::from_str(&oj).unwrap();
                let merged = abp_config::merge_configs(b, o);
                serde_json::to_string(&merged).unwrap()
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &results {
        assert_eq!(r, &results[0]);
    }
}
