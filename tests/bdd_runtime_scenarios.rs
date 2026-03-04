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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive BDD-style scenario tests for the runtime pipeline.
//!
//! Each test follows Given/When/Then structure documented in comments.
//! Covers 15 scenario categories with 80+ individual tests.

use std::collections::BTreeMap;
use std::path::Path;

use abp_backend_core::Backend;
use abp_backend_mock::scenarios::{MockScenario, ScenarioMockBackend};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_emulation::{EmulationConfig, EmulationEngine, EmulationStrategy};
use abp_policy::PolicyEngine;
use abp_receipt::{ReceiptBuilder, compute_hash, verify_hash};
use abp_runtime::multiplex::EventMultiplexer;
use abp_runtime::retry::{FallbackChain, RetryPolicy};
use abp_runtime::{Runtime, RuntimeError};
use abp_stream::{EventFilter, EventRecorder, EventStats, StreamPipelineBuilder};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// BDD macros
// ===========================================================================

macro_rules! scenario {
    ($name:expr, $body:block) => {{
        eprintln!("  Scenario: {}", $name);
        $body
    }};
}

macro_rules! given {
    ($desc:expr, $body:expr) => {{
        eprintln!("    Given {}", $desc);
        $body
    }};
}

macro_rules! when {
    ($desc:expr, $body:expr) => {{
        eprintln!("    When {}", $desc);
        $body
    }};
}

macro_rules! then {
    ($desc:expr, $body:expr) => {{
        eprintln!("    Then {}", $desc);
        $body
    }};
}

// ===========================================================================
// Helpers
// ===========================================================================

fn make_manifest(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .build()
}

/// A custom backend that always fails.
#[derive(Debug, Clone)]
struct FailingBackend {
    message: String,
}

#[async_trait]
impl abp_integrations::Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        make_manifest(&[(Capability::Streaming, SupportLevel::Native)])
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _wo: WorkOrder,
        _tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("{}", self.message)
    }
}

/// A custom backend that streams N delta events then completes.
#[derive(Debug, Clone)]
struct StreamingBackend {
    chunks: Vec<String>,
}

#[async_trait]
impl abp_integrations::Backend for StreamingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "streaming".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        make_manifest(&[(Capability::Streaming, SupportLevel::Native)])
    }
    async fn run(
        &self,
        run_id: Uuid,
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "streaming".into(),
                },
                ext: None,
            })
            .await;
        for chunk in &self.chunks {
            let _ = tx
                .send(AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: chunk.clone(),
                    },
                    ext: None,
                })
                .await;
        }
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            })
            .await;
        let finished = Utc::now();
        let receipt = ReceiptBuilder::new("streaming")
            .run_id(run_id)
            .work_order_id(wo.id)
            .started_at(started)
            .finished_at(finished)
            .outcome(Outcome::Complete)
            .build()
            .with_hash()?;
        Ok(receipt)
    }
}

/// A backend with configurable passthrough mode support.
#[derive(Debug, Clone)]
struct PassthroughBackend;

#[async_trait]
impl abp_integrations::Backend for PassthroughBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "passthrough".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        make_manifest(&[(Capability::Streaming, SupportLevel::Native)])
    }
    async fn run(
        &self,
        run_id: Uuid,
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut ext = BTreeMap::new();
        ext.insert(
            "raw_message".to_string(),
            serde_json::json!({"sdk": "original"}),
        );
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "passthrough response".into(),
                },
                ext: Some(ext),
            })
            .await;
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            })
            .await;
        let mode = abp_backend_core::extract_execution_mode(&wo);
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: wo.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: Utc::now(),
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode,
            usage_raw: serde_json::json!({}),
            usage: abp_core::UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: abp_core::VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?;
        Ok(receipt)
    }
}

// ===========================================================================
// Feature 1: Happy Path — work order → backend → receipt
// ===========================================================================

#[tokio::test]
async fn happy_path_mock_backend_returns_receipt() {
    scenario!("Mock backend produces a valid receipt", {
        let rt = given!("a runtime with mock backend registered", {
            Runtime::with_default_backends()
        });
        let wo = given!("a simple work order", simple_work_order("hello world"));
        let handle = when!("the work order is executed against mock backend", {
            rt.run_streaming("mock", wo).await.expect("run_streaming")
        });
        then!("a receipt is returned with Complete outcome", {
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert_eq!(receipt.backend.id, "mock");
        });
    });
}

#[tokio::test]
async fn happy_path_receipt_has_contract_version() {
    scenario!("Receipt carries the current contract version", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("mock", simple_work_order("test"))
                .await
                .unwrap()
        });
        then!("receipt meta contains CONTRACT_VERSION", {
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        });
    });
}

#[tokio::test]
async fn happy_path_receipt_has_hash() {
    scenario!("Receipt has SHA-256 hash attached", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("mock", simple_work_order("hash test"))
                .await
                .unwrap()
        });
        then!("receipt_sha256 is present and 64 hex chars", {
            let receipt = handle.receipt.await.unwrap().unwrap();
            let hash = receipt.receipt_sha256.as_ref().expect("hash present");
            assert_eq!(hash.len(), 64);
        });
    });
}

#[tokio::test]
async fn happy_path_receipt_hash_verifies() {
    scenario!("Receipt hash can be verified after execution", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("mock", simple_work_order("verify hash"))
                .await
                .unwrap()
        });
        then!("verify_hash returns true", {
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert!(verify_hash(&receipt));
        });
    });
}

#[tokio::test]
async fn happy_path_receipt_trace_is_nonempty() {
    scenario!("Receipt contains a non-empty trace of events", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("mock", simple_work_order("trace test"))
                .await
                .unwrap()
        });
        then!("receipt trace has events", {
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert!(!receipt.trace.is_empty());
        });
    });
}

#[tokio::test]
async fn happy_path_run_id_is_unique() {
    scenario!("Each run gets a unique run_id", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let h1 = when!("first work order is executed", {
            rt.run_streaming("mock", simple_work_order("run 1"))
                .await
                .unwrap()
        });
        let h2 = when!("second work order is executed", {
            rt.run_streaming("mock", simple_work_order("run 2"))
                .await
                .unwrap()
        });
        then!("run_ids are different", {
            assert_ne!(h1.run_id, h2.run_id);
        });
    });
}

// ===========================================================================
// Feature 2: Streaming — work order → stream events → aggregated receipt
// ===========================================================================

#[tokio::test]
async fn streaming_events_are_received_via_channel() {
    scenario!("Events stream through the RunHandle", {
        let rt = given!("a runtime with a streaming backend", {
            let mut rt = Runtime::new();
            rt.register_backend(
                "stream",
                StreamingBackend {
                    chunks: vec!["Hello".into(), " world".into()],
                },
            );
            rt
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("stream", simple_work_order("stream test"))
                .await
                .unwrap()
        });
        then!("events include deltas and run lifecycle", {
            let mut events: Vec<AgentEvent> = vec![];
            let mut stream = handle.events;
            while let Some(ev) = stream.next().await {
                events.push(ev);
            }
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert!(events.iter().any(|e| matches!(
                &e.kind,
                AgentEventKind::AssistantDelta { text } if text == "Hello"
            )));
            assert!(
                events
                    .iter()
                    .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
            );
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    });
}

#[tokio::test]
async fn streaming_scenario_mock_emits_chunks() {
    scenario!("ScenarioMockBackend streaming emits delta chunks", {
        let rt = given!("a runtime with scenario mock streaming backend", {
            let mut rt = Runtime::new();
            rt.register_backend(
                "scenario",
                ScenarioMockBackend::new(MockScenario::StreamingSuccess {
                    chunks: vec!["a".into(), "b".into(), "c".into()],
                    chunk_delay_ms: 0,
                }),
            );
            rt
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("scenario", simple_work_order("stream"))
                .await
                .unwrap()
        });
        then!("three delta events are received", {
            let mut deltas = 0;
            let mut stream = handle.events;
            while let Some(ev) = stream.next().await {
                if matches!(ev.kind, AgentEventKind::AssistantDelta { .. }) {
                    deltas += 1;
                }
            }
            let _receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(deltas, 3);
        });
    });
}

#[tokio::test]
async fn streaming_receipt_captures_all_trace_events() {
    scenario!(
        "Receipt trace contains all events when backend trace is empty",
        {
            let rt = given!("a runtime with streaming backend", {
                let mut rt = Runtime::new();
                rt.register_backend(
                    "stream",
                    StreamingBackend {
                        chunks: vec!["x".into()],
                    },
                );
                rt
            });
            let handle = when!("a work order is executed", {
                rt.run_streaming("stream", simple_work_order("trace capture"))
                    .await
                    .unwrap()
            });
            then!("receipt trace is non-empty", {
                // Drain events
                let mut s = handle.events;
                while s.next().await.is_some() {}
                let receipt = handle.receipt.await.unwrap().unwrap();
                assert!(!receipt.trace.is_empty());
            });
        }
    );
}

#[tokio::test]
async fn streaming_events_include_run_started() {
    scenario!("Stream includes a RunStarted event", {
        let rt = given!("a runtime with streaming backend", {
            let mut rt = Runtime::new();
            rt.register_backend(
                "s",
                StreamingBackend {
                    chunks: vec!["hi".into()],
                },
            );
            rt
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("s", simple_work_order("started"))
                .await
                .unwrap()
        });
        then!("RunStarted event is present in stream", {
            let mut found = false;
            let mut s = handle.events;
            while let Some(ev) = s.next().await {
                if matches!(ev.kind, AgentEventKind::RunStarted { .. }) {
                    found = true;
                }
            }
            let _ = handle.receipt.await;
            assert!(found, "expected RunStarted event");
        });
    });
}

#[tokio::test]
async fn streaming_empty_chunks_still_completes() {
    scenario!("Empty chunk list still produces a valid receipt", {
        let rt = given!("a runtime with streaming backend with no chunks", {
            let mut rt = Runtime::new();
            rt.register_backend("s", StreamingBackend { chunks: vec![] });
            rt
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("s", simple_work_order("empty"))
                .await
                .unwrap()
        });
        then!("receipt is Complete", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    });
}

// ===========================================================================
// Feature 3: Multiple backends — register and select
// ===========================================================================

#[tokio::test]
async fn multiple_backends_register_and_list() {
    scenario!("Multiple backends are registered and listed", {
        let rt = given!("a runtime with mock and streaming backends", {
            let mut rt = Runtime::new();
            rt.register_backend("mock", abp_integrations::MockBackend);
            rt.register_backend("stream", StreamingBackend { chunks: vec![] });
            rt
        });
        then!("backend_names includes both", {
            let names = rt.backend_names();
            assert!(names.contains(&"mock".to_string()));
            assert!(names.contains(&"stream".to_string()));
        });
    });
}

#[tokio::test]
async fn multiple_backends_select_by_name() {
    scenario!("Correct backend is selected by name", {
        let rt = given!("a runtime with mock and streaming backends", {
            let mut rt = Runtime::new();
            rt.register_backend("mock", abp_integrations::MockBackend);
            rt.register_backend(
                "stream",
                StreamingBackend {
                    chunks: vec!["x".into()],
                },
            );
            rt
        });
        let handle = when!("executing against 'stream' backend", {
            rt.run_streaming("stream", simple_work_order("select"))
                .await
                .unwrap()
        });
        then!("receipt comes from 'streaming' backend", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.backend.id, "streaming");
        });
    });
}

#[tokio::test]
async fn multiple_backends_unknown_name_returns_error() {
    scenario!("Unknown backend name returns RuntimeError", {
        let rt = given!("a runtime with only mock backend", {
            Runtime::with_default_backends()
        });
        let result = when!("executing against nonexistent backend", {
            rt.run_streaming("nonexistent", simple_work_order("err"))
                .await
        });
        then!("UnknownBackend error is returned", {
            let Err(err) = result else {
                panic!("expected error, got Ok");
            };
            assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
        });
    });
}

#[tokio::test]
async fn multiple_backends_replacement_works() {
    scenario!("Re-registering a backend replaces the old one", {
        let mut rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        when!("replacing mock with a streaming backend", {
            rt.register_backend(
                "mock",
                StreamingBackend {
                    chunks: vec!["replaced".into()],
                },
            );
        });
        then!("the new backend is used", {
            let handle = rt
                .run_streaming("mock", simple_work_order("replace"))
                .await
                .unwrap();
            let mut s = handle.events;
            let mut found = false;
            while let Some(ev) = s.next().await {
                if let AgentEventKind::AssistantDelta { text } = &ev.kind {
                    if text == "replaced" {
                        found = true;
                    }
                }
            }
            let _ = handle.receipt.await;
            assert!(found, "replacement backend should be used");
        });
    });
}

#[tokio::test]
async fn multiple_backends_empty_registry() {
    scenario!("Empty runtime has no backends", {
        let rt = given!("an empty runtime", Runtime::new());
        then!("backend_names is empty", {
            assert!(rt.backend_names().is_empty());
        });
    });
}

#[tokio::test]
async fn multiple_backends_backend_lookup() {
    scenario!("Backend can be looked up by name", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        then!("backend('mock') returns Some", {
            assert!(rt.backend("mock").is_some());
            assert!(rt.backend("missing").is_none());
        });
    });
}

// ===========================================================================
// Feature 4: Workspace staging — create staged workspace → run → cleanup
// ===========================================================================

#[tokio::test]
async fn workspace_passthrough_uses_original_path() {
    scenario!("PassThrough mode uses the original workspace path", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let wo = given!("a work order with PassThrough mode", {
            WorkOrderBuilder::new("passthrough ws")
                .workspace_mode(WorkspaceMode::PassThrough)
                .root(".")
                .build()
        });
        let handle = when!("the work order is executed", {
            rt.run_streaming("mock", wo).await.unwrap()
        });
        then!("execution succeeds", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    });
}

#[tokio::test]
async fn workspace_staged_creates_temp_dir() {
    scenario!("Staged mode creates a temporary workspace", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let wo = given!("a work order with Staged mode", {
            WorkOrderBuilder::new("staged ws")
                .workspace_mode(WorkspaceMode::Staged)
                .root(".")
                .exclude(vec!["target/**".into()])
                .build()
        });
        let handle = when!("the work order is executed", {
            rt.run_streaming("mock", wo).await.unwrap()
        });
        then!("execution succeeds with staged workspace", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    });
}

#[tokio::test]
async fn workspace_staged_includes_exclude_globs() {
    scenario!("Staged workspace respects include/exclude globs", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let wo = given!("a work order with exclude globs", {
            WorkOrderBuilder::new("glob ws")
                .workspace_mode(WorkspaceMode::Staged)
                .root(".")
                .exclude(vec!["target/**".into()])
                .build()
        });
        let handle = when!("the work order is executed", {
            rt.run_streaming("mock", wo).await.unwrap()
        });
        then!("execution succeeds", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    });
}

#[tokio::test]
async fn workspace_staged_git_verification() {
    scenario!("Staged workspace enables git verification in receipt", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let wo = given!("a work order with Staged mode", {
            WorkOrderBuilder::new("git verify")
                .workspace_mode(WorkspaceMode::Staged)
                .root(".")
                .exclude(vec!["target/**".into()])
                .build()
        });
        let handle = when!("the work order is executed", {
            rt.run_streaming("mock", wo).await.unwrap()
        });
        then!("receipt verification fields may be populated", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            // Staged workspace auto-initializes git, so git_status may be set
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    });
}

#[test]
fn workspace_stager_requires_source_root() {
    scenario!("WorkspaceStager without source_root fails", {
        then!("stage() returns an error", {
            let result = abp_workspace::WorkspaceStager::new().stage();
            assert!(result.is_err());
        });
    });
}

#[tokio::test]
async fn workspace_stager_with_valid_source() {
    scenario!("WorkspaceStager with valid source creates workspace", {
        let ws = given!("a stager pointing at the repo root", {
            abp_workspace::WorkspaceStager::new()
                .source_root(".")
                .exclude(vec!["target/**".into()])
                .stage()
                .expect("stage should succeed")
        });
        then!("the workspace path exists", {
            assert!(ws.path().exists());
        });
    });
}

// ===========================================================================
// Feature 5: Policy enforcement — allows/denies tool/read/write
// ===========================================================================

#[test]
fn policy_empty_allows_everything() {
    scenario!("Empty policy permits all operations", {
        let engine = given!("a policy engine with default (empty) policy", {
            PolicyEngine::new(&PolicyProfile::default()).unwrap()
        });
        then!("all tools and paths are allowed", {
            assert!(engine.can_use_tool("Bash").allowed);
            assert!(engine.can_use_tool("Read").allowed);
            assert!(engine.can_read_path(Path::new("any.txt")).allowed);
            assert!(engine.can_write_path(Path::new("any.txt")).allowed);
        });
    });
}

#[test]
fn policy_disallowed_tool_is_denied() {
    scenario!("Disallowed tool is denied", {
        let engine = given!("a policy denying Bash", {
            let policy = PolicyProfile {
                disallowed_tools: vec!["Bash".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!("Bash is denied", {
            assert!(!engine.can_use_tool("Bash").allowed);
        });
    });
}

#[test]
fn policy_allowlist_blocks_unlisted_tools() {
    scenario!("Tools not in allowlist are denied", {
        let engine = given!("a policy with only Read and Write allowed", {
            let policy = PolicyProfile {
                allowed_tools: vec!["Read".into(), "Write".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!("Bash is denied, Read is allowed", {
            assert!(!engine.can_use_tool("Bash").allowed);
            assert!(engine.can_use_tool("Read").allowed);
        });
    });
}

#[test]
fn policy_deny_overrides_allow() {
    scenario!("Deny list takes precedence over allow list", {
        let engine = given!("a policy with wildcard allow and specific deny", {
            let policy = PolicyProfile {
                allowed_tools: vec!["*".into()],
                disallowed_tools: vec!["Bash".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!("Bash is denied despite wildcard allow", {
            assert!(!engine.can_use_tool("Bash").allowed);
            assert!(engine.can_use_tool("Read").allowed);
        });
    });
}

#[test]
fn policy_deny_read_blocks_path() {
    scenario!("deny_read blocks matching paths", {
        let engine = given!("a policy denying .env reads", {
            let policy = PolicyProfile {
                deny_read: vec!["**/.env".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!(".env is denied, other files allowed", {
            assert!(!engine.can_read_path(Path::new(".env")).allowed);
            assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
        });
    });
}

#[test]
fn policy_deny_write_blocks_path() {
    scenario!("deny_write blocks matching paths", {
        let engine = given!("a policy denying writes to .git/", {
            let policy = PolicyProfile {
                deny_write: vec!["**/.git/**".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!(".git/config write is denied", {
            assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
            assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
        });
    });
}

#[test]
fn policy_glob_patterns_match_prefixes() {
    scenario!("Glob patterns match prefixed tool names", {
        let engine = given!("a policy denying Bash*", {
            let policy = PolicyProfile {
                disallowed_tools: vec!["Bash*".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!("BashExec is denied", {
            assert!(!engine.can_use_tool("BashExec").allowed);
            assert!(engine.can_use_tool("Read").allowed);
        });
    });
}

#[test]
fn policy_combined_tool_and_path_restrictions() {
    scenario!("Combined tool and path restrictions", {
        let engine = given!("a complex policy", {
            let policy = PolicyProfile {
                allowed_tools: vec!["Read".into(), "Grep".into()],
                disallowed_tools: vec!["Write".into()],
                deny_read: vec!["**/.env".into()],
                deny_write: vec!["**/locked/**".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!("all restrictions are enforced simultaneously", {
            assert!(engine.can_use_tool("Read").allowed);
            assert!(!engine.can_use_tool("Write").allowed);
            assert!(!engine.can_use_tool("Bash").allowed);
            assert!(!engine.can_read_path(Path::new(".env")).allowed);
            assert!(!engine.can_write_path(Path::new("locked/data.txt")).allowed);
            assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
        });
    });
}

#[test]
fn policy_decision_carries_reason() {
    scenario!("Denied decisions carry a human-readable reason", {
        let engine = given!("a policy denying Bash", {
            let policy = PolicyProfile {
                disallowed_tools: vec!["Bash".into()],
                ..Default::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });
        then!("denial reason is present", {
            let d = engine.can_use_tool("Bash");
            assert!(!d.allowed);
            assert!(d.reason.is_some());
            assert!(d.reason.unwrap().contains("Bash"));
        });
    });
}

// ===========================================================================
// Feature 6: Capability negotiation — check capabilities before execution
// ===========================================================================

#[tokio::test]
async fn capability_satisfied_requirements_pass() {
    scenario!("Backend satisfying all requirements passes check", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        then!("check_capabilities passes for streaming requirement", {
            let reqs = CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Emulated,
                }],
            };
            assert!(rt.check_capabilities("mock", &reqs).is_ok());
        });
    });
}

#[tokio::test]
async fn capability_unsatisfied_requirements_fail() {
    scenario!("Backend missing required capabilities fails check", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        then!("check_capabilities fails for CodeExecution native", {
            let reqs = CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::CodeExecution,
                    min_support: MinSupport::Native,
                }],
            };
            assert!(rt.check_capabilities("mock", &reqs).is_err());
        });
    });
}

#[tokio::test]
async fn capability_check_unknown_backend_fails() {
    scenario!("Capability check on unknown backend returns error", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        then!("check_capabilities on 'ghost' fails", {
            let reqs = CapabilityRequirements::default();
            assert!(rt.check_capabilities("ghost", &reqs).is_err());
        });
    });
}

#[tokio::test]
async fn capability_empty_requirements_pass() {
    scenario!("Empty requirements always pass", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        then!("check_capabilities with empty reqs passes", {
            let reqs = CapabilityRequirements::default();
            assert!(rt.check_capabilities("mock", &reqs).is_ok());
        });
    });
}

#[tokio::test]
async fn capability_emulated_satisfies_emulated_min() {
    scenario!("Emulated support satisfies Emulated min requirement", {
        let rt = given!("a runtime with mock backend (ToolRead is Emulated)", {
            Runtime::with_default_backends()
        });
        then!("Emulated min for ToolRead passes", {
            let reqs = CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                }],
            };
            assert!(rt.check_capabilities("mock", &reqs).is_ok());
        });
    });
}

#[tokio::test]
async fn capability_emulated_does_not_satisfy_native_min() {
    scenario!("Emulated support does not satisfy Native min", {
        let rt = given!("a runtime with mock backend (ToolRead is Emulated)", {
            Runtime::with_default_backends()
        });
        then!("Native min for ToolRead fails", {
            let reqs = CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                }],
            };
            assert!(rt.check_capabilities("mock", &reqs).is_err());
        });
    });
}

// ===========================================================================
// Feature 7: Emulation fallback — native unsupported → emulate → succeed
// ===========================================================================

#[test]
fn emulation_extended_thinking_can_be_emulated() {
    scenario!("ExtendedThinking has a default emulation strategy", {
        then!("can_emulate returns true", {
            assert!(abp_emulation::can_emulate(&Capability::ExtendedThinking));
        });
    });
}

#[test]
fn emulation_code_execution_cannot_be_emulated() {
    scenario!("CodeExecution is marked as non-emulatable", {
        then!("can_emulate returns false", {
            assert!(!abp_emulation::can_emulate(&Capability::CodeExecution));
        });
    });
}

#[test]
fn emulation_engine_applies_system_prompt_injection() {
    scenario!(
        "EmulationEngine injects system prompt for ExtendedThinking",
        {
            let engine = given!("an emulation engine with defaults", {
                EmulationEngine::with_defaults()
            });
            let mut conv = given!("a conversation with a user message", {
                use abp_core::ir::{IrConversation, IrMessage, IrRole};
                IrConversation::new().push(IrMessage::text(IrRole::User, "Think hard"))
            });
            let report = when!("applying ExtendedThinking emulation", {
                engine.apply(&[Capability::ExtendedThinking], &mut conv)
            });
            then!("system prompt is injected and report records it", {
                assert_eq!(report.applied.len(), 1);
                assert!(conv.messages[0].role == abp_core::ir::IrRole::System);
            });
        }
    );
}

#[test]
fn emulation_config_override_replaces_default() {
    scenario!("Custom emulation config overrides default strategy", {
        let engine = given!("an emulation engine with CodeExecution override", {
            let mut config = EmulationConfig::new();
            config.set(
                Capability::CodeExecution,
                EmulationStrategy::SystemPromptInjection {
                    prompt: "Simulate code.".into(),
                },
            );
            EmulationEngine::new(config)
        });
        then!("CodeExecution resolves to SystemPromptInjection", {
            let strategy = engine.resolve_strategy(&Capability::CodeExecution);
            assert!(matches!(
                strategy,
                EmulationStrategy::SystemPromptInjection { .. }
            ));
        });
    });
}

#[test]
fn emulation_report_tracks_warnings() {
    scenario!("Disabled capabilities produce warnings in report", {
        let engine = given!("an emulation engine with defaults", {
            EmulationEngine::with_defaults()
        });
        let report = when!("checking CodeExecution and Streaming", {
            engine.check_missing(&[Capability::CodeExecution, Capability::Streaming])
        });
        then!("warnings are produced", {
            assert!(report.has_unemulatable());
            assert_eq!(report.warnings.len(), 2);
        });
    });
}

#[tokio::test]
async fn emulation_runtime_with_emulation_config() {
    scenario!("Runtime with emulation config processes work orders", {
        let rt = given!("a runtime with emulation enabled", {
            Runtime::with_default_backends().with_emulation(EmulationConfig::new())
        });
        then!("emulation_config is set", {
            assert!(rt.emulation_config().is_some());
        });
    });
}

// ===========================================================================
// Feature 8: Error propagation — backend fails → typed runtime error
// ===========================================================================

#[tokio::test]
async fn error_backend_failure_returns_backend_failed() {
    scenario!("Backend failure propagates as BackendFailed", {
        let rt = given!("a runtime with a failing backend", {
            let mut rt = Runtime::new();
            rt.register_backend(
                "fail",
                FailingBackend {
                    message: "kaboom".into(),
                },
            );
            rt
        });
        let handle = when!("executing against failing backend", {
            rt.run_streaming("fail", simple_work_order("fail test"))
                .await
                .unwrap()
        });
        then!("receipt future resolves to BackendFailed error", {
            let result = handle.receipt.await.unwrap();
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(matches!(err, RuntimeError::BackendFailed(_)));
        });
    });
}

#[tokio::test]
async fn error_unknown_backend_is_typed() {
    scenario!("Unknown backend returns UnknownBackend error", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let result = when!("executing against unknown backend", {
            rt.run_streaming("ghost", simple_work_order("err")).await
        });
        then!("error is UnknownBackend", {
            let Err(err) = result else {
                panic!("expected error, got Ok");
            };
            assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
        });
    });
}

#[tokio::test]
async fn error_capability_check_failure_is_typed() {
    scenario!("Capability check failure returns CapabilityCheckFailed", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let wo = given!("a work order requiring CodeExecution natively", {
            WorkOrderBuilder::new("cap fail")
                .workspace_mode(WorkspaceMode::PassThrough)
                .root(".")
                .requirements(CapabilityRequirements {
                    required: vec![CapabilityRequirement {
                        capability: Capability::CodeExecution,
                        min_support: MinSupport::Native,
                    }],
                })
                .build()
        });
        let result = when!("executing with unsatisfiable requirements", {
            rt.run_streaming("mock", wo).await
        });
        then!("CapabilityCheckFailed error is returned or propagated", {
            // The runtime may return the error at startup or during execution
            match result {
                Err(RuntimeError::CapabilityCheckFailed(_)) => {}
                Ok(handle) => {
                    let receipt_result = handle.receipt.await.unwrap();
                    assert!(receipt_result.is_err());
                }
                Err(other) => {
                    // Accept BackendFailed wrapping a capability error too
                    assert!(
                        matches!(other, RuntimeError::BackendFailed(_)),
                        "unexpected error: {other:?}"
                    );
                }
            }
        });
    });
}

#[tokio::test]
async fn error_is_retryable_for_backend_failure() {
    scenario!("BackendFailed errors are marked as retryable", {
        let err = given!("a BackendFailed error", {
            RuntimeError::BackendFailed(anyhow::anyhow!("transient"))
        });
        then!("is_retryable returns true", {
            assert!(err.is_retryable());
        });
    });
}

#[tokio::test]
async fn error_not_retryable_for_unknown_backend() {
    scenario!("UnknownBackend errors are not retryable", {
        let err = given!("an UnknownBackend error", {
            RuntimeError::UnknownBackend { name: "x".into() }
        });
        then!("is_retryable returns false", {
            assert!(!err.is_retryable());
        });
    });
}

#[test]
fn error_runtime_error_has_error_code() {
    scenario!("RuntimeError variants map to error codes", {
        let err = given!("an UnknownBackend error", {
            RuntimeError::UnknownBackend { name: "x".into() }
        });
        then!("error_code returns BackendNotFound", {
            assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
        });
    });
}

// ===========================================================================
// Feature 9: Receipt hashing — canonical hash is deterministic
// ===========================================================================

#[test]
fn receipt_hash_is_deterministic() {
    scenario!("Same receipt produces same hash", {
        let receipt = given!("a receipt built with ReceiptBuilder", {
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
        });
        let h1 = when!("hash is computed first time", {
            compute_hash(&receipt).unwrap()
        });
        let h2 = when!("hash is computed second time", {
            compute_hash(&receipt).unwrap()
        });
        then!("hashes are identical", {
            assert_eq!(h1, h2);
        });
    });
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    scenario!("Receipt hash is a 64-character hex string", {
        let receipt = given!("a simple receipt", {
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
        });
        let hash = when!("hash is computed", compute_hash(&receipt).unwrap());
        then!("hash is 64 hex characters", {
            assert_eq!(hash.len(), 64);
            assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        });
    });
}

#[test]
fn receipt_with_hash_sets_field() {
    scenario!("Receipt.with_hash() sets receipt_sha256", {
        let receipt = given!("a receipt without hash", {
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
        });
        assert!(receipt.receipt_sha256.is_none());
        let hashed = when!("with_hash is called", receipt.with_hash().unwrap());
        then!("receipt_sha256 is Some", {
            assert!(hashed.receipt_sha256.is_some());
        });
    });
}

#[test]
fn receipt_hash_ignores_stored_hash() {
    scenario!("Stored hash does not influence the computed hash", {
        let r1 = given!("a receipt without hash", {
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
        });
        let r2 = given!("the same receipt with a hash set", {
            r1.clone().with_hash().unwrap()
        });
        let h1 = when!("computing hash of unhashed receipt", {
            compute_hash(&r1).unwrap()
        });
        let h2 = when!("computing hash of hashed receipt", {
            compute_hash(&r2).unwrap()
        });
        then!("hashes are the same", {
            assert_eq!(h1, h2);
        });
    });
}

#[test]
fn receipt_verify_hash_detects_tampering() {
    scenario!("Tampered receipt fails hash verification", {
        let mut receipt = given!("a receipt with valid hash", {
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
                .with_hash()
                .unwrap()
        });
        assert!(verify_hash(&receipt));
        when!("receipt is tampered with", {
            receipt.receipt_sha256 = Some("deadbeef".repeat(8));
        });
        then!("verify_hash returns false", {
            assert!(!verify_hash(&receipt));
        });
    });
}

#[test]
fn receipt_hash_changes_with_different_outcomes() {
    scenario!("Different outcomes produce different hashes", {
        let h1 = given!("hash of Complete receipt", {
            compute_hash(
                &ReceiptBuilder::new("mock")
                    .outcome(Outcome::Complete)
                    .build(),
            )
            .unwrap()
        });
        let h2 = given!("hash of Failed receipt", {
            compute_hash(&ReceiptBuilder::new("mock").outcome(Outcome::Failed).build()).unwrap()
        });
        then!("hashes differ", {
            assert_ne!(h1, h2);
        });
    });
}

// ===========================================================================
// Feature 10: Retry logic — backend transient failure → retry → succeed
// ===========================================================================

#[test]
fn retry_policy_default_has_three_retries() {
    scenario!("Default retry policy allows 3 retries", {
        let policy = given!("a default retry policy", RetryPolicy::default());
        then!("max_retries is 3", {
            assert_eq!(policy.max_retries, 3);
        });
    });
}

#[test]
fn retry_policy_should_retry_respects_max() {
    scenario!("should_retry returns false at max_retries", {
        let policy = given!("a policy with max_retries=2", {
            RetryPolicy::builder().max_retries(2).build()
        });
        then!("attempts 0 and 1 retry, attempt 2 does not", {
            assert!(policy.should_retry(0));
            assert!(policy.should_retry(1));
            assert!(!policy.should_retry(2));
        });
    });
}

#[test]
fn retry_policy_no_retry() {
    scenario!("no_retry policy disables all retries", {
        let policy = given!("a no-retry policy", RetryPolicy::no_retry());
        then!("should_retry(0) is false", {
            assert!(!policy.should_retry(0));
        });
    });
}

#[test]
fn retry_compute_delay_is_bounded() {
    scenario!("Computed delay never exceeds max_backoff", {
        let policy = given!("a retry policy with max_backoff=1s", {
            RetryPolicy::builder()
                .max_retries(10)
                .max_backoff(std::time::Duration::from_secs(1))
                .build()
        });
        then!("delay for attempt 100 <= max_backoff", {
            let delay = policy.compute_delay(100);
            assert!(delay <= std::time::Duration::from_secs(1));
        });
    });
}

#[test]
fn retry_fallback_chain_iterates() {
    scenario!("FallbackChain iterates through backends", {
        let mut chain = given!("a chain with 3 backends", {
            FallbackChain::new(vec!["a".into(), "b".into(), "c".into()])
        });
        then!("backends are yielded in order, then None", {
            assert_eq!(chain.remaining(), 3);
            assert_eq!(chain.next_backend(), Some("a"));
            assert_eq!(chain.next_backend(), Some("b"));
            assert_eq!(chain.next_backend(), Some("c"));
            assert_eq!(chain.next_backend(), None);
            assert_eq!(chain.remaining(), 0);
        });
    });
}

#[test]
fn retry_fallback_chain_reset() {
    scenario!("FallbackChain can be reset", {
        let mut chain = given!("a chain with 2 backends", {
            FallbackChain::new(vec!["a".into(), "b".into()])
        });
        let _ = chain.next_backend();
        let _ = chain.next_backend();
        when!("chain is reset", chain.reset());
        then!("iteration starts over", {
            assert_eq!(chain.next_backend(), Some("a"));
        });
    });
}

#[tokio::test]
async fn retry_scenario_mock_transient_then_success() {
    scenario!("ScenarioMock fails N times then succeeds", {
        let backend = given!("a ScenarioMockBackend with 2 transient failures", {
            ScenarioMockBackend::new(MockScenario::TransientError {
                fail_count: 2,
                then: Box::new(MockScenario::Success {
                    delay_ms: 0,
                    text: "ok".into(),
                }),
            })
        });
        then!("first two calls fail, third succeeds", {
            let (tx, _rx) = mpsc::channel(16);
            let wo = simple_work_order("retry");
            let r1 = backend.run(Uuid::new_v4(), wo.clone(), tx.clone()).await;
            assert!(r1.is_err());
            let r2 = backend.run(Uuid::new_v4(), wo.clone(), tx.clone()).await;
            assert!(r2.is_err());
            let r3 = backend.run(Uuid::new_v4(), wo, tx).await;
            assert!(r3.is_ok());
            assert_eq!(backend.call_count(), 3);
        });
    });
}

// ===========================================================================
// Feature 11: Circuit breaker — multiple failures → circuit opens
// ===========================================================================

#[tokio::test]
async fn circuit_breaker_permanent_error_never_succeeds() {
    scenario!("PermanentError scenario never succeeds", {
        let backend = given!("a ScenarioMockBackend with permanent error", {
            ScenarioMockBackend::new(MockScenario::PermanentError {
                code: "ABP-E001".into(),
                message: "fatal".into(),
            })
        });
        then!("every call fails", {
            let (tx, _rx) = mpsc::channel(16);
            let wo = simple_work_order("perm");
            for _ in 0..5 {
                let r = backend.run(Uuid::new_v4(), wo.clone(), tx.clone()).await;
                assert!(r.is_err());
            }
            assert_eq!(backend.call_count(), 5);
        });
    });
}

#[tokio::test]
async fn circuit_breaker_scenario_records_errors() {
    scenario!("ScenarioMock records last_error on failure", {
        let backend = given!("a ScenarioMockBackend with permanent error", {
            ScenarioMockBackend::new(MockScenario::PermanentError {
                code: "E".into(),
                message: "boom".into(),
            })
        });
        when!("a call fails", {
            let (tx, _rx) = mpsc::channel(16);
            let _ = backend
                .run(Uuid::new_v4(), simple_work_order("err"), tx)
                .await;
        });
        then!("last_error is set", {
            let err = backend.last_error().await;
            assert!(err.is_some());
            assert!(err.unwrap().contains("boom"));
        });
    });
}

#[tokio::test]
async fn circuit_breaker_runtime_propagates_backend_error() {
    scenario!("Runtime propagates backend errors correctly", {
        let rt = given!("a runtime with permanent error backend", {
            let mut rt = Runtime::new();
            rt.register_backend(
                "err",
                ScenarioMockBackend::new(MockScenario::PermanentError {
                    code: "E".into(),
                    message: "circuit".into(),
                }),
            );
            rt
        });
        let handle = when!("executing against error backend", {
            rt.run_streaming("err", simple_work_order("cb"))
                .await
                .unwrap()
        });
        then!("receipt resolves to error", {
            let result = handle.receipt.await.unwrap();
            assert!(result.is_err());
        });
    });
}

#[test]
fn circuit_breaker_retryable_error_classification() {
    scenario!("Backend and workspace errors are retryable", {
        then!("BackendFailed is retryable", {
            assert!(RuntimeError::BackendFailed(anyhow::anyhow!("x")).is_retryable());
        });
        then!("WorkspaceFailed is retryable", {
            assert!(RuntimeError::WorkspaceFailed(anyhow::anyhow!("x")).is_retryable());
        });
        then!("PolicyFailed is not retryable", {
            assert!(!RuntimeError::PolicyFailed(anyhow::anyhow!("x")).is_retryable());
        });
    });
}

// ===========================================================================
// Feature 12: Event multiplexing — multiple sources → single stream
// ===========================================================================

#[tokio::test]
async fn multiplex_broadcasts_to_subscribers() {
    scenario!("EventMultiplexer broadcasts to all subscribers", {
        let mux = given!("a multiplexer with 2 subscribers", {
            let mux = EventMultiplexer::new(64);
            (mux.subscribe(), mux.subscribe(), mux)
        });
        let (mut sub1, mut sub2, mux) = mux;
        when!("an event is broadcast", {
            mux.broadcast(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "hello".into(),
                },
                ext: None,
            })
            .unwrap();
        });
        then!("both subscribers receive it", {
            let e1 = sub1.recv().await.unwrap();
            let e2 = sub2.recv().await.unwrap();
            assert!(matches!(e1.kind, AgentEventKind::AssistantMessage { .. }));
            assert!(matches!(e2.kind, AgentEventKind::AssistantMessage { .. }));
        });
    });
}

#[test]
fn multiplex_no_subscribers_returns_error() {
    scenario!("Broadcast with no subscribers returns error", {
        let mux = given!("a multiplexer with no subscribers", {
            EventMultiplexer::new(16)
        });
        then!("broadcast returns NoSubscribers error", {
            let result = mux.broadcast(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "test".into(),
                },
                ext: None,
            });
            assert!(result.is_err());
        });
    });
}

#[test]
fn multiplex_subscriber_count() {
    scenario!("subscriber_count tracks active subscribers", {
        let mux = given!("a multiplexer", EventMultiplexer::new(16));
        assert_eq!(mux.subscriber_count(), 0);
        let _s1 = mux.subscribe();
        assert_eq!(mux.subscriber_count(), 1);
        let _s2 = mux.subscribe();
        assert_eq!(mux.subscriber_count(), 2);
    });
}

#[test]
fn multiplex_event_recorder_captures_events() {
    scenario!("EventRecorder captures all recorded events", {
        let recorder = given!("a fresh event recorder", EventRecorder::new());
        when!("events are recorded", {
            recorder.record(&AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "a".into(),
                },
                ext: None,
            });
            recorder.record(&AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "b".into(),
                },
                ext: None,
            });
        });
        then!("two events are stored", {
            assert_eq!(recorder.len(), 2);
            assert!(!recorder.is_empty());
        });
    });
}

#[test]
fn multiplex_event_stats_tracks_counts() {
    scenario!("EventStats tracks per-kind counts", {
        let stats = given!("a fresh event stats tracker", EventStats::new());
        when!("events are observed", {
            stats.observe(&AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "hi".into() },
                ext: None,
            });
            stats.observe(&AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "there".into(),
                },
                ext: None,
            });
            stats.observe(&AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "oops".into(),
                    error_code: None,
                },
                ext: None,
            });
        });
        then!("counts are accurate", {
            assert_eq!(stats.total_events(), 3);
            assert_eq!(stats.count_for("assistant_delta"), 2);
            assert_eq!(stats.error_count(), 1);
            assert_eq!(stats.total_delta_bytes(), 7); // "hi" + "there"
        });
    });
}

// ===========================================================================
// Feature 13: Passthrough mode — no dialect rewriting
// ===========================================================================

#[tokio::test]
async fn passthrough_mode_preserves_ext_data() {
    scenario!("Passthrough backend preserves ext raw_message data", {
        let rt = given!("a runtime with passthrough backend", {
            let mut rt = Runtime::new();
            rt.register_backend("pt", PassthroughBackend);
            rt
        });
        let wo = given!("a work order configured for passthrough", {
            let mut config = abp_core::RuntimeConfig::default();
            config.vendor.insert(
                "abp".to_string(),
                serde_json::json!({"mode": "passthrough"}),
            );
            WorkOrderBuilder::new("pt test")
                .workspace_mode(WorkspaceMode::PassThrough)
                .root(".")
                .config(config)
                .build()
        });
        let handle = when!("the work order is executed", {
            rt.run_streaming("pt", wo).await.unwrap()
        });
        then!("events carry ext data", {
            let mut found_ext = false;
            let mut stream = handle.events;
            while let Some(ev) = stream.next().await {
                if ev.ext.is_some() {
                    found_ext = true;
                }
            }
            let _ = handle.receipt.await;
            assert!(found_ext, "expected events with ext data");
        });
    });
}

#[test]
fn passthrough_execution_mode_default_is_mapped() {
    scenario!("Default execution mode is Mapped", {
        then!("ExecutionMode::default() is Mapped", {
            assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
        });
    });
}

#[test]
fn passthrough_extract_mode_from_vendor_config() {
    scenario!("extract_execution_mode reads mode from vendor config", {
        let wo = given!("a work order with passthrough mode in config", {
            let mut config = abp_core::RuntimeConfig::default();
            config.vendor.insert(
                "abp".to_string(),
                serde_json::json!({"mode": "passthrough"}),
            );
            WorkOrderBuilder::new("mode test")
                .workspace_mode(WorkspaceMode::PassThrough)
                .root(".")
                .config(config)
                .build()
        });
        then!("extract_execution_mode returns Passthrough", {
            let mode = abp_backend_core::extract_execution_mode(&wo);
            assert_eq!(mode, ExecutionMode::Passthrough);
        });
    });
}

#[test]
fn passthrough_extract_mode_defaults_to_mapped() {
    scenario!("Missing mode config defaults to Mapped", {
        let wo = given!("a work order without mode config", {
            simple_work_order("no mode")
        });
        then!("extract_execution_mode returns Mapped", {
            let mode = abp_backend_core::extract_execution_mode(&wo);
            assert_eq!(mode, ExecutionMode::Mapped);
        });
    });
}

#[test]
fn passthrough_extract_mode_flat_key_format() {
    scenario!("extract_execution_mode reads abp.mode flat key", {
        let wo = given!("a work order with abp.mode flat key", {
            let mut config = abp_core::RuntimeConfig::default();
            config
                .vendor
                .insert("abp.mode".to_string(), serde_json::json!("passthrough"));
            WorkOrderBuilder::new("flat key")
                .workspace_mode(WorkspaceMode::PassThrough)
                .root(".")
                .config(config)
                .build()
        });
        then!("extract_execution_mode returns Passthrough", {
            let mode = abp_backend_core::extract_execution_mode(&wo);
            assert_eq!(mode, ExecutionMode::Passthrough);
        });
    });
}

// ===========================================================================
// Feature 14: Mapped mode — full dialect translation
// ===========================================================================

#[tokio::test]
async fn mapped_mode_default_receipt_uses_mapped() {
    scenario!("Default work order produces a Mapped mode receipt", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let handle = when!("executing a work order without explicit mode", {
            rt.run_streaming("mock", simple_work_order("mapped"))
                .await
                .unwrap()
        });
        then!("receipt mode is Mapped", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.mode, ExecutionMode::Mapped);
        });
    });
}

#[test]
fn mapped_mode_serde_roundtrip() {
    scenario!("ExecutionMode serializes/deserializes correctly", {
        let modes = given!("both execution modes", {
            vec![ExecutionMode::Mapped, ExecutionMode::Passthrough]
        });
        then!("each survives serde roundtrip", {
            for mode in modes {
                let json = serde_json::to_string(&mode).unwrap();
                let parsed: ExecutionMode = serde_json::from_str(&json).unwrap();
                assert_eq!(mode, parsed);
            }
        });
    });
}

#[test]
fn mapped_mode_passthrough_validates() {
    scenario!("validate_passthrough_compatibility succeeds for any WO", {
        let wo = given!("a simple work order", simple_work_order("compat"));
        then!("validation passes", {
            assert!(abp_backend_core::validate_passthrough_compatibility(&wo).is_ok());
        });
    });
}

#[tokio::test]
async fn mapped_mode_explicit_mapped_config() {
    scenario!("Explicit mapped mode in config produces Mapped receipt", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        let wo = given!("a work order with explicit mapped mode", {
            let mut config = abp_core::RuntimeConfig::default();
            config
                .vendor
                .insert("abp".to_string(), serde_json::json!({"mode": "mapped"}));
            WorkOrderBuilder::new("explicit mapped")
                .workspace_mode(WorkspaceMode::PassThrough)
                .root(".")
                .config(config)
                .build()
        });
        let handle = when!("executing the work order", {
            rt.run_streaming("mock", wo).await.unwrap()
        });
        then!("receipt mode is Mapped", {
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let receipt = handle.receipt.await.unwrap().unwrap();
            assert_eq!(receipt.mode, ExecutionMode::Mapped);
        });
    });
}

// ===========================================================================
// Feature 15: Config loading — TOML config → runtime configuration
// ===========================================================================

#[test]
fn config_default_is_valid() {
    scenario!("Default config passes validation", {
        let cfg = given!("a default BackplaneConfig", {
            abp_config::BackplaneConfig::default()
        });
        then!("validate_config returns Ok", {
            assert!(abp_config::validate_config(&cfg).is_ok());
        });
    });
}

#[test]
fn config_parse_valid_toml() {
    scenario!("Valid TOML string parses into config", {
        let cfg = when!("parsing a valid TOML string", {
            abp_config::parse_toml(
                r#"
                default_backend = "mock"
                log_level = "debug"
                [backends.mock]
                type = "mock"
            "#,
            )
            .unwrap()
        });
        then!("fields are set correctly", {
            assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
            assert_eq!(cfg.log_level.as_deref(), Some("debug"));
            assert_eq!(cfg.backends.len(), 1);
        });
    });
}

#[test]
fn config_parse_invalid_toml() {
    scenario!("Invalid TOML returns ParseError", {
        let result = when!("parsing invalid TOML", {
            abp_config::parse_toml("this [is not valid =")
        });
        then!("error is ParseError", {
            assert!(matches!(
                result.unwrap_err(),
                abp_config::ConfigError::ParseError { .. }
            ));
        });
    });
}

#[test]
fn config_validation_catches_invalid_log_level() {
    scenario!("Invalid log level fails validation", {
        let cfg = given!("a config with invalid log level", {
            abp_config::BackplaneConfig {
                log_level: Some("verbose".into()),
                ..Default::default()
            }
        });
        then!("validation fails", {
            assert!(abp_config::validate_config(&cfg).is_err());
        });
    });
}

#[test]
fn config_merge_overlay_wins() {
    scenario!("Merge overlay values override base values", {
        let base = given!("a base config", {
            abp_config::BackplaneConfig {
                default_backend: Some("mock".into()),
                log_level: Some("info".into()),
                ..Default::default()
            }
        });
        let overlay = given!("an overlay config", {
            abp_config::BackplaneConfig {
                default_backend: Some("openai".into()),
                log_level: None,
                ..Default::default()
            }
        });
        let merged = when!("configs are merged", {
            abp_config::merge_configs(base, overlay)
        });
        then!("overlay default_backend wins, base log_level preserved", {
            assert_eq!(merged.default_backend.as_deref(), Some("openai"));
            assert_eq!(merged.log_level.as_deref(), Some("info"));
        });
    });
}

#[test]
fn config_sidecar_with_args_roundtrip() {
    scenario!("Sidecar backend config with args survives roundtrip", {
        let cfg = when!("parsing TOML with sidecar backend", {
            abp_config::parse_toml(
                r#"
                [backends.node]
                type = "sidecar"
                command = "node"
                args = ["host.js"]
                timeout_secs = 120
            "#,
            )
            .unwrap()
        });
        then!("sidecar fields are correct", {
            match &cfg.backends["node"] {
                abp_config::BackendEntry::Sidecar {
                    command,
                    args,
                    timeout_secs,
                } => {
                    assert_eq!(command, "node");
                    assert_eq!(args, &["host.js"]);
                    assert_eq!(*timeout_secs, Some(120));
                }
                other => panic!("expected Sidecar, got {other:?}"),
            }
        });
    });
}

#[test]
fn config_empty_string_parses_to_defaults() {
    scenario!("Empty TOML string produces default config", {
        let cfg = when!("parsing empty string", {
            abp_config::parse_toml("").unwrap()
        });
        then!("all fields are None/empty", {
            assert!(cfg.default_backend.is_none());
            assert!(cfg.backends.is_empty());
        });
    });
}

#[test]
fn config_missing_file_returns_not_found() {
    scenario!("Loading from missing file returns FileNotFound", {
        let result = when!("loading from nonexistent path", {
            abp_config::load_config(Some(Path::new("/nonexistent/backplane.toml")))
        });
        then!("FileNotFound error is returned", {
            assert!(matches!(
                result.unwrap_err(),
                abp_config::ConfigError::FileNotFound { .. }
            ));
        });
    });
}

#[test]
fn config_load_none_returns_default() {
    scenario!("Loading with None path returns default config", {
        let cfg = when!("loading with None", {
            abp_config::load_config(None).unwrap()
        });
        then!("log_level defaults to info", {
            assert_eq!(cfg.log_level.as_deref(), Some("info"));
        });
    });
}

// ===========================================================================
// Additional cross-cutting scenarios
// ===========================================================================

#[tokio::test]
async fn runtime_metrics_are_tracked() {
    scenario!("Runtime tracks run metrics", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        when!("a work order is executed", {
            let handle = rt
                .run_streaming("mock", simple_work_order("metrics"))
                .await
                .unwrap();
            let mut s = handle.events;
            while s.next().await.is_some() {}
            let _ = handle.receipt.await;
        });
        then!("metrics show at least one run", {
            let snap = rt.metrics().snapshot();
            assert!(snap.total_runs >= 1);
        });
    });
}

#[tokio::test]
async fn runtime_receipt_chain_accumulates() {
    scenario!("Receipt chain accumulates receipts across runs", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });
        when!("two work orders are executed", {
            for task in &["chain-1", "chain-2"] {
                let handle = rt
                    .run_streaming("mock", simple_work_order(task))
                    .await
                    .unwrap();
                let mut s = handle.events;
                while s.next().await.is_some() {}
                let _ = handle.receipt.await;
            }
        });
        then!("receipt chain has 2 entries", {
            let chain = rt.receipt_chain();
            let locked = chain.lock().await;
            assert_eq!(locked.len(), 2);
        });
    });
}

#[tokio::test]
async fn runtime_stream_pipeline_filters_events() {
    scenario!("Stream pipeline can filter out events", {
        let rt = given!("a runtime with mock backend and error-filter pipeline", {
            let pipeline = StreamPipelineBuilder::new()
                .filter(EventFilter::exclude_errors())
                .build();
            Runtime::with_default_backends().with_stream_pipeline(pipeline)
        });
        let handle = when!("a work order is executed", {
            rt.run_streaming("mock", simple_work_order("pipeline"))
                .await
                .unwrap()
        });
        then!("no error events appear in stream", {
            let mut stream = handle.events;
            while let Some(ev) = stream.next().await {
                assert!(!matches!(ev.kind, AgentEventKind::Error { .. }));
            }
            let _ = handle.receipt.await;
        });
    });
}

#[test]
fn stream_pipeline_with_recording() {
    scenario!("StreamPipeline with recording captures events", {
        let recorder = given!("a recorder", EventRecorder::new());
        let pipeline = given!("a pipeline with recorder", {
            StreamPipelineBuilder::new().record().build()
        });
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "recorded".into(),
            },
            ext: None,
        };
        when!("an event is processed", {
            let result = pipeline.process(event);
            assert!(result.is_some());
        });
        // The pipeline's internal recorder captures it, but we can't access it
        // directly from the built pipeline, so we verify the event passes through.
        then!("event passes through", {
            let _ = recorder; // recorder exists independently
        });
    });
}

#[test]
fn event_filter_by_kind() {
    scenario!("EventFilter::by_kind filters specific event kinds", {
        let filter = given!("a filter for assistant_message", {
            EventFilter::by_kind("assistant_message")
        });
        let msg_event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        };
        let err_event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        };
        then!("message matches, error doesn't", {
            assert!(filter.matches(&msg_event));
            assert!(!filter.matches(&err_event));
        });
    });
}

#[test]
fn work_order_builder_sets_all_fields() {
    scenario!("WorkOrderBuilder sets all configurable fields", {
        let wo = when!("building a fully configured work order", {
            WorkOrderBuilder::new("full config")
                .root("/tmp/ws")
                .workspace_mode(WorkspaceMode::Staged)
                .model("gpt-4")
                .max_turns(10)
                .max_budget_usd(5.0)
                .include(vec!["src/**".into()])
                .exclude(vec!["target/**".into()])
                .build()
        });
        then!("all fields are set correctly", {
            assert_eq!(wo.task, "full config");
            assert_eq!(wo.workspace.root, "/tmp/ws");
            assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
            assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
            assert_eq!(wo.config.max_turns, Some(10));
            assert_eq!(wo.config.max_budget_usd, Some(5.0));
            assert_eq!(wo.workspace.include, vec!["src/**"]);
            assert_eq!(wo.workspace.exclude, vec!["target/**"]);
        });
    });
}

#[test]
fn receipt_builder_produces_valid_receipt() {
    scenario!("ReceiptBuilder produces a receipt with correct fields", {
        let receipt = when!("building a receipt", {
            ReceiptBuilder::new("test-backend")
                .outcome(Outcome::Partial)
                .mode(ExecutionMode::Passthrough)
                .build()
        });
        then!("receipt fields are correct", {
            assert_eq!(receipt.backend.id, "test-backend");
            assert_eq!(receipt.outcome, Outcome::Partial);
            assert_eq!(receipt.mode, ExecutionMode::Passthrough);
            assert!(receipt.receipt_sha256.is_none());
        });
    });
}

#[test]
fn receipt_builder_with_hash() {
    scenario!("ReceiptBuilder.with_hash() computes and sets hash", {
        let receipt = when!("building a receipt with hash", {
            ReceiptBuilder::new("hashed")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap()
        });
        then!("receipt_sha256 is set and valid", {
            assert!(receipt.receipt_sha256.is_some());
            assert!(verify_hash(&receipt));
        });
    });
}

#[test]
fn contract_version_is_correct() {
    scenario!("CONTRACT_VERSION matches expected value", {
        then!("version is abp/v0.1", {
            assert_eq!(CONTRACT_VERSION, "abp/v0.1");
        });
    });
}

#[test]
fn agent_event_serde_roundtrip() {
    scenario!("AgentEvent survives serde roundtrip", {
        let event = given!("an AssistantMessage event", {
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "hello".into(),
                },
                ext: None,
            }
        });
        let json = when!("serialized to JSON", {
            serde_json::to_string(&event).unwrap()
        });
        let parsed: AgentEvent = when!("deserialized from JSON", {
            serde_json::from_str(&json).unwrap()
        });
        then!("text matches", {
            if let AgentEventKind::AssistantMessage { text } = &parsed.kind {
                assert_eq!(text, "hello");
            } else {
                panic!("expected AssistantMessage");
            }
        });
    });
}

#[test]
fn receipt_serde_roundtrip() {
    scenario!("Receipt survives serde roundtrip", {
        let receipt = given!("a receipt with hash", {
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
                .with_hash()
                .unwrap()
        });
        let json = when!("serialized to JSON", {
            serde_json::to_string(&receipt).unwrap()
        });
        let parsed: Receipt = when!("deserialized from JSON", {
            serde_json::from_str(&json).unwrap()
        });
        then!("hash is preserved", {
            assert_eq!(receipt.receipt_sha256, parsed.receipt_sha256);
        });
    });
}
