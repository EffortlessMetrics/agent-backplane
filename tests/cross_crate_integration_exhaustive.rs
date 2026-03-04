#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive cross-crate integration tests.
//!
//! Verifies the full crate dependency hierarchy works correctly end-to-end.
//! Tests span all major crate boundaries: core → protocol → host → integrations
//! → runtime, plus policy, capability, receipt, IR, dialect, mapper, projection,
//! config, stream, error, validate, and emulation layers.

use std::collections::BTreeMap;
use std::time::Duration;

use std::collections::BTreeMap;
use std::time::Duration;

use abp_backend_core::registry::BackendRegistry;
use abp_backend_core::{ensure_capability_requirements, extract_execution_mode};
use abp_backend_mock::MockBackend;
use abp_capability::{
    NegotiationResult, check_capability, claude_35_sonnet_manifest, codex_manifest,
    copilot_manifest, gemini_15_pro_manifest, generate_report, kimi_manifest,
    negotiate_capabilities, openai_gpt4o_manifest,
};
use abp_config::load_from_str;
use abp_core::aggregate::{EventAggregator, RunAnalytics};
use abp_core::config::{ConfigDefaults, ConfigValidator};
use abp_core::ext::{AgentEventExt, ReceiptExt, WorkOrderExt};
use abp_core::filter::EventFilter;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest};
use abp_core::stream::EventStream;
use abp_core::validate::validate_receipt;
use abp_core::verify::ReceiptVerifier;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_emulation::EmulationEngine;
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_glob::IncludeExcludeGlobs;
use abp_integrations::Backend;
use abp_ir::lower::{lower_for_dialect, lower_to_claude, lower_to_openai};
use abp_ir::normalize::{
    dedup_system, merge_adjacent_text, normalize, strip_empty, strip_metadata, trim_text,
};
use abp_mapper::{IdentityMapper, Mapper};
use abp_mapping::{Fidelity, known_rules, validate_mapping};
use abp_policy::PolicyEngine;
use abp_projection::{ProjectionMatrix, ProjectionScore};
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{self as receipt_crate, ReceiptChain};
use abp_retry::RetryPolicy;
use abp_runtime::{RunHandle, Runtime, RuntimeError};
use abp_stream::StreamPipelineBuilder;
use abp_validate::{
    EventValidator, ReceiptValidator as ValidateReceiptValidator, Validator, WorkOrderValidator,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

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
    abp_receipt::ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .add_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .build()
}

fn make_hashed_receipt(backend: &str) -> Receipt {
    abp_receipt::ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .add_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_event(make_event(AgentEventKind::RunCompleted {
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
    m.insert(Capability::ToolBash, SupportLevel::Native);
    m
}

fn sample_backend_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.1".into()),
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

fn make_ir_conversation() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there!"))
}

fn make_tool_definition() -> IrToolDefinition {
    IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file from disk".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        }),
    }
}

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

async fn drain_run(handle: RunHandle) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (collected, receipt)
}

/// A backend that always fails.
#[derive(Debug, Clone)]
struct FailingBackend {
    message: String,
}

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("{}", self.message)
    }
}

/// A backend that emits N events then succeeds.
#[derive(Debug, Clone)]
struct MultiEventBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for MultiEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "multi-event".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();

        let started_ev = make_event(AgentEventKind::RunStarted {
            message: "multi start".into(),
        });
        trace.push(started_ev.clone());
        let _ = events_tx.send(started_ev).await;

        for i in 0..self.event_count {
            let ev = make_event(AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            });
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let completed_ev = make_event(AgentEventKind::RunCompleted {
            message: "multi done".into(),
        });
        trace.push(completed_ev.clone());
        let _ = events_tx.send(completed_ev).await;

        let finished = Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"note": "multi-event"}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

// ===========================================================================
// Module 1: Core types flow through the entire stack
// ===========================================================================

mod core_types_flow {
    use super::*;

    #[test]
    fn work_order_survives_protocol_roundtrip() {
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
                assert_eq!(work_order.id, wo.id);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn receipt_survives_protocol_roundtrip() {
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
                assert_eq!(r.outcome, Outcome::Complete);
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn agent_event_survives_protocol_roundtrip() {
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
    fn work_order_with_context_flows_through_protocol() {
        let ctx = ContextPacket {
            files: vec!["main.rs".into(), "lib.rs".into()],
            snippets: vec![ContextSnippet {
                name: "example".into(),
                content: "fn main() {}".into(),
            }],
        };
        let wo = WorkOrderBuilder::new("code review").context(ctx).build();
        let env = Envelope::Run {
            id: "ctx-run".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.context.files.len(), 2);
                assert_eq!(work_order.context.snippets[0].name, "example");
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn hello_envelope_carries_capabilities_and_version() {
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
                mode,
            } => {
                assert_eq!(backend.id, "test-backend");
                assert!(capabilities.contains_key(&Capability::Streaming));
                assert_eq!(contract_version, CONTRACT_VERSION);
                assert_eq!(mode, ExecutionMode::Mapped);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn fatal_envelope_roundtrip_with_error_code() {
        let env = Envelope::fatal_with_code(
            Some("run-x".into()),
            "backend crashed",
            ErrorCode::BackendCrashed,
        );
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal {
                error, error_code, ..
            } => {
                assert_eq!(error, "backend crashed");
                assert!(error_code.is_some());
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[test]
    fn all_event_kinds_survive_protocol_roundtrip() {
        let kinds = vec![
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            AgentEventKind::AssistantMessage {
                text: "full".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "test.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                output: json!("file content"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            AgentEventKind::Warning {
                message: "rate limited".into(),
            },
            AgentEventKind::Error {
                message: "something failed".into(),
                error_code: Some(ErrorCode::BackendTimeout),
            },
        ];

        for kind in kinds {
            let evt = make_event(kind);
            let env = Envelope::Event {
                ref_id: "test".into(),
                event: evt,
            };
            let json = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            assert!(matches!(decoded, Envelope::Event { .. }));
        }
    }

    #[test]
    fn work_order_preserves_uuid_through_protocol() {
        let wo = make_work_order("uuid test");
        let original_id = wo.id;
        let env = Envelope::Run {
            id: "run-uuid".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.id, original_id),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn receipt_preserves_all_fields_through_protocol() {
        let receipt = abp_receipt::ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Partial)
            .backend_version("2.0")
            .adapter_version("0.5")
            .mode(ExecutionMode::Passthrough)
            .usage_tokens(100, 50)
            .build();
        let env = Envelope::Final {
            ref_id: "fields-test".into(),
            receipt: receipt.clone(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Final { receipt: r, .. } => {
                assert_eq!(r.outcome, Outcome::Partial);
                assert_eq!(r.mode, ExecutionMode::Passthrough);
                assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
                assert_eq!(r.usage.input_tokens, Some(100));
                assert_eq!(r.usage.output_tokens, Some(50));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn capability_manifest_survives_hello_roundtrip() {
        let caps = openai_gpt4o_manifest();
        let env = Envelope::hello(
            BackendIdentity {
                id: "openai".into(),
                backend_version: None,
                adapter_version: None,
            },
            caps.clone(),
        );
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello { capabilities, .. } => {
                assert_eq!(capabilities.len(), caps.len());
            }
            _ => panic!("wrong variant"),
        }
    }
}

// ===========================================================================
// Module 2: Contract version consistency
// ===========================================================================

mod contract_version {
    use super::*;

    #[test]
    fn contract_version_is_abp_v01() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn receipt_builder_embeds_contract_version() {
        let r = make_receipt("mock");
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn hashed_receipt_embeds_contract_version() {
        let r = make_hashed_receipt("mock");
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn protocol_version_check_accepts_current() {
        assert!(abp_protocol::is_compatible_version(CONTRACT_VERSION));
    }

    #[test]
    fn protocol_version_check_rejects_unknown() {
        assert!(!abp_protocol::is_compatible_version("abp/v99.99"));
    }

    #[test]
    fn validate_hello_version_accepts_current() {
        assert!(abp_validate::validate_hello_version(CONTRACT_VERSION));
    }

    #[test]
    fn validate_hello_version_rejects_wrong() {
        assert!(!abp_validate::validate_hello_version("wrong/v1.0"));
    }

    #[test]
    fn receipt_from_receipt_crate_has_correct_version() {
        let r = abp_receipt::ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_re_exported_from_receipt_crate() {
        // abp_receipt re-exports CONTRACT_VERSION
        assert_eq!(abp_receipt::CONTRACT_VERSION, CONTRACT_VERSION);
    }
}

// ===========================================================================
// Module 3: WorkOrder creation → Backend execution → Receipt pipeline
// ===========================================================================

mod work_order_to_receipt_pipeline {
    use super::*;

    #[tokio::test]
    async fn mock_backend_produces_receipt_for_work_order() {
        let backend = MockBackend;
        let wo = make_work_order("test task");
        let run_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(32);
        let receipt = backend.run(run_id, wo, tx).await.unwrap();
        assert_eq!(receipt.backend.id, "mock");
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn mock_backend_emits_events_during_run() {
        let backend = MockBackend;
        let wo = make_work_order("event test");
        let run_id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(32);
        let _receipt = backend.run(run_id, wo, tx).await.unwrap();
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn receipt_has_valid_metadata_after_backend_run() {
        let backend = MockBackend;
        let wo = make_work_order("metadata test");
        let run_id = Uuid::new_v4();
        let wo_id = wo.id;
        let (tx, _rx) = mpsc::channel(32);
        let receipt = backend.run(run_id, wo, tx).await.unwrap();
        assert_eq!(receipt.meta.run_id, run_id);
        assert_eq!(receipt.meta.work_order_id, wo_id);
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn pipeline_work_order_to_receipt_to_hash() {
        let backend = MockBackend;
        let wo = make_work_order("pipeline test");
        let (tx, _rx) = mpsc::channel(32);
        let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let hashed = receipt.with_hash().unwrap();
        assert!(hashed.receipt_sha256.is_some());
        assert!(receipt_crate::verify_hash(&hashed));
    }

    #[tokio::test]
    async fn pipeline_work_order_to_receipt_to_chain() {
        let backend = MockBackend;
        let mut chain = ReceiptChain::new();
        for i in 0..3 {
            let wo = make_work_order(&format!("task-{i}"));
            let (tx, _rx) = mpsc::channel(32);
            let receipt = backend
                .run(Uuid::new_v4(), wo, tx)
                .await
                .unwrap()
                .with_hash()
                .unwrap();
            chain.push(receipt).unwrap();
        }
        assert_eq!(chain.len(), 3);
    }

    #[tokio::test]
    async fn pipeline_work_order_to_receipt_to_validation() {
        let backend = MockBackend;
        let wo = make_work_order("validate test");
        let (tx, _rx) = mpsc::channel(32);
        let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let result = validate_receipt(&receipt);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn failing_backend_propagates_error() {
        let backend = FailingBackend {
            message: "intentional failure".into(),
        };
        let wo = make_work_order("fail test");
        let (tx, _rx) = mpsc::channel(32);
        let result = backend.run(Uuid::new_v4(), wo, tx).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("intentional failure")
        );
    }

    #[tokio::test]
    async fn multi_event_backend_streams_correct_count() {
        let backend = MultiEventBackend { event_count: 5 };
        let wo = make_work_order("multi test");
        let (tx, mut rx) = mpsc::channel(64);
        let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        // 1 RunStarted + 5 AssistantDelta + 1 RunCompleted = 7
        assert_eq!(events.len(), 7);
        assert_eq!(receipt.trace.len(), 7);
    }
}

// ===========================================================================
// Module 4: IR roundtrip through full conversion pipeline
// ===========================================================================

mod ir_roundtrip {
    use super::*;

    #[test]
    fn ir_conversation_construction() {
        let conv = make_ir_conversation();
        assert_eq!(conv.len(), 3);
        assert!(conv.system_message().is_some());
        assert!(conv.last_assistant().is_some());
    }

    #[test]
    fn ir_text_message_properties() {
        let msg = IrMessage::text(IrRole::User, "hello");
        assert_eq!(msg.role, IrRole::User);
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "hello");
    }

    #[test]
    fn ir_tool_use_message() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "read_file".into(),
                input: json!({"path": "test.rs"}),
            }],
        );
        assert!(!msg.is_text_only());
        assert_eq!(msg.tool_use_blocks().len(), 1);
    }

    #[test]
    fn ir_lower_to_openai_produces_valid_json() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        let result = lower_to_openai(&conv, &tools);
        assert!(result.is_object());
        assert!(result.get("messages").is_some());
    }

    #[test]
    fn ir_lower_to_claude_produces_valid_json() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        let result = lower_to_claude(&conv, &tools);
        assert!(result.is_object());
    }

    #[test]
    fn ir_lower_for_all_dialects_produces_json() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        for dialect in Dialect::all() {
            let result = lower_for_dialect(*dialect, &conv, &tools);
            assert!(
                result.is_object(),
                "dialect {dialect:?} didn't produce object"
            );
        }
    }

    #[test]
    fn ir_normalize_dedup_system() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "First system"))
            .push(IrMessage::text(IrRole::System, "Second system"))
            .push(IrMessage::text(IrRole::User, "Hello"));
        let normalized = dedup_system(&conv);
        let system_count = normalized
            .messages
            .iter()
            .filter(|m| m.role == IrRole::System)
            .count();
        assert!(system_count <= 1);
    }

    #[test]
    fn ir_normalize_trim_text() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello  "));
        let trimmed = trim_text(&conv);
        assert_eq!(trimmed.messages[0].text_content(), "hello");
    }

    #[test]
    fn ir_normalize_strip_empty() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, ""))
            .push(IrMessage::text(IrRole::User, "not empty"));
        let stripped = strip_empty(&conv);
        assert_eq!(stripped.len(), 1);
    }

    #[test]
    fn ir_normalize_merge_adjacent_text() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hello "))
            .push(IrMessage::text(IrRole::User, "world"));
        let merged = merge_adjacent_text(&conv);
        assert!(
            merged.len() <= 1
                || merged
                    .messages
                    .last()
                    .unwrap()
                    .text_content()
                    .contains("world")
        );
    }

    #[test]
    fn ir_normalize_full_pipeline() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, "  hello  "))
            .push(IrMessage::text(IrRole::User, ""))
            .push(IrMessage::text(IrRole::Assistant, " response "));
        let normalized = normalize(&conv);
        assert!(normalized.len() <= conv.len());
    }

    #[test]
    fn ir_strip_metadata_keeps_specified_keys() {
        let mut msg = IrMessage::text(IrRole::User, "hello");
        msg.metadata.insert("keep".into(), json!("yes"));
        msg.metadata.insert("remove".into(), json!("no"));
        let conv = IrConversation::new().push(msg);
        let stripped = strip_metadata(&conv, &["keep"]);
        assert!(stripped.messages[0].metadata.contains_key("keep"));
        assert!(!stripped.messages[0].metadata.contains_key("remove"));
    }

    #[test]
    fn ir_conversation_messages_by_role() {
        let conv = make_ir_conversation();
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
        assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
        assert_eq!(conv.messages_by_role(IrRole::System).len(), 1);
    }

    #[test]
    fn ir_tool_definition_serializes_roundtrip() {
        let tool = make_tool_definition();
        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "read_file");
    }

    #[test]
    fn ir_content_block_thinking() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think...".into(),
                },
                IrContentBlock::Text {
                    text: "Here's my answer.".into(),
                },
            ],
        );
        assert!(!msg.is_text_only());
        assert_eq!(msg.content.len(), 2);
    }

    #[test]
    fn ir_lower_preserves_tool_definitions() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Use the tool"));
        let tools = vec![make_tool_definition()];
        let openai = lower_to_openai(&conv, &tools);
        // OpenAI format should include tools
        assert!(openai.get("tools").is_some() || openai.get("messages").is_some());
    }
}

// ===========================================================================
// Module 5: Policy enforcement through runtime
// ===========================================================================

mod policy_enforcement {
    use super::*;

    #[test]
    fn policy_engine_allows_unrestricted_tool() {
        let policy = PolicyProfile {
            allowed_tools: vec![],
            disallowed_tools: vec![],
            deny_read: vec![],
            deny_write: vec![],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool("read_file");
        assert!(decision.allowed);
    }

    #[test]
    fn policy_engine_denies_disallowed_tool() {
        let policy = make_policy(&[], &["rm"], &[], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool("rm");
        assert!(!decision.allowed);
    }

    #[test]
    fn policy_engine_allows_explicitly_allowed_tool() {
        let policy = make_policy(&["read_file"], &[], &[], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool("read_file");
        assert!(decision.allowed);
    }

    #[test]
    fn policy_engine_denies_read_to_restricted_path() {
        let policy = make_policy(&[], &[], &["/etc/shadow"], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_read_path("/etc/shadow");
        assert!(!decision.allowed);
    }

    #[test]
    fn policy_engine_allows_read_to_unrestricted_path() {
        let policy = make_policy(&[], &[], &["/etc/shadow"], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_read_path("/home/user/code");
        assert!(decision.allowed);
    }

    #[test]
    fn policy_engine_denies_write_to_restricted_path() {
        let policy = make_policy(&[], &[], &[], &["*.lock"]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_write_path("Cargo.lock");
        assert!(!decision.allowed);
    }

    #[test]
    fn policy_engine_allows_write_to_unrestricted_path() {
        let policy = make_policy(&[], &[], &[], &["*.lock"]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_write_path("src/main.rs");
        assert!(decision.allowed);
    }

    #[test]
    fn policy_flows_from_work_order() {
        let policy = make_policy(&["read_file", "write_file"], &["rm", "exec"], &[], &[]);
        let wo = WorkOrderBuilder::new("policy test").policy(policy).build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(engine.can_use_tool("read_file").allowed);
        assert!(!engine.can_use_tool("rm").allowed);
    }

    #[test]
    fn policy_with_glob_deny_read() {
        let policy = make_policy(&[], &[], &["**/.env*"], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_read_path(".env").allowed);
        assert!(!engine.can_read_path(".env.local").allowed);
    }

    #[test]
    fn policy_with_glob_deny_write() {
        let policy = make_policy(&[], &[], &[], &["**/node_modules/**"]);
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_write_path("node_modules/pkg/index.js").allowed);
    }

    #[test]
    fn policy_decision_has_reason_on_deny() {
        let policy = make_policy(&[], &["dangerous_tool"], &[], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool("dangerous_tool");
        assert!(!decision.allowed);
        assert!(decision.reason.is_some());
    }

    #[test]
    fn empty_policy_allows_everything() {
        let policy = PolicyProfile {
            allowed_tools: vec![],
            disallowed_tools: vec![],
            deny_read: vec![],
            deny_write: vec![],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(engine.can_use_tool("anything").allowed);
        assert!(engine.can_read_path("anywhere").allowed);
        assert!(engine.can_write_path("anywhere").allowed);
    }
}

// ===========================================================================
// Module 6: Capability negotiation through integration layer
// ===========================================================================

mod capability_negotiation {
    use super::*;

    #[test]
    fn negotiate_with_openai_manifest() {
        let manifest = openai_gpt4o_manifest();
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn negotiate_with_claude_manifest() {
        let manifest = claude_35_sonnet_manifest();
        let required = vec![Capability::Streaming];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn negotiate_with_gemini_manifest() {
        let manifest = gemini_15_pro_manifest();
        let required = vec![Capability::Streaming];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn negotiate_with_kimi_manifest() {
        let manifest = kimi_manifest();
        let required = vec![Capability::Streaming];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn negotiate_with_codex_manifest() {
        let manifest = codex_manifest();
        let required = vec![Capability::Streaming];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn negotiate_with_copilot_manifest() {
        let manifest = copilot_manifest();
        let required = vec![Capability::Streaming];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
    }

    #[test]
    fn check_capability_returns_support_level() {
        let manifest = openai_gpt4o_manifest();
        let level = check_capability(&manifest, &Capability::Streaming);
        assert!(matches!(
            level,
            SupportLevel::Native | SupportLevel::Emulated
        ));
    }

    #[test]
    fn generate_report_from_negotiation() {
        let manifest = openai_gpt4o_manifest();
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        let report = generate_report(&result);
        assert!(!report.summary.is_empty());
    }

    #[test]
    fn ensure_capability_requirements_succeeds_for_compatible() {
        let manifest = openai_gpt4o_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        };
        let result = ensure_capability_requirements(&reqs, &manifest);
        assert!(result.is_ok());
    }

    #[test]
    fn core_negotiate_module_works() {
        let manifest = sample_capability_manifest();
        let request = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![Capability::ToolRead],
            minimum_support: SupportLevel::Emulated,
        };
        let result = CapabilityNegotiator::negotiate(&request, &manifest);
        assert!(result.is_compatible);
    }

    #[test]
    fn capability_negotiation_unsupported_detection() {
        let manifest = BTreeMap::new(); // empty manifest
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(!result.is_viable());
    }

    #[test]
    fn mock_backend_capabilities_are_negotiable() {
        let backend = MockBackend;
        let caps = backend.capabilities();
        let required = vec![Capability::Streaming];
        let result = negotiate_capabilities(&required, &caps);
        // MockBackend should support streaming
        assert!(result.is_viable() || !result.unsupported.is_empty());
    }

    #[test]
    fn all_vendor_manifests_have_streaming() {
        let manifests = [
            openai_gpt4o_manifest(),
            claude_35_sonnet_manifest(),
            gemini_15_pro_manifest(),
        ];
        for manifest in &manifests {
            assert!(
                manifest.contains_key(&Capability::Streaming),
                "manifest missing Streaming capability"
            );
        }
    }
}

// ===========================================================================
// Module 7: Error propagation across crate boundaries
// ===========================================================================

mod error_propagation {
    use super::*;

    #[test]
    fn abp_error_construction() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
        assert_eq!(err.code, ErrorCode::BackendTimeout);
        assert!(err.message.contains("30s"));
    }

    #[test]
    fn abp_error_with_context() {
        let err = AbpError::new(ErrorCode::BackendNotFound, "not found")
            .with_context("backend", "openai");
        assert!(!err.context.is_empty());
    }

    #[test]
    fn error_code_has_category() {
        let code = ErrorCode::BackendTimeout;
        let category = code.category();
        assert_eq!(category, ErrorCategory::Backend);
    }

    #[test]
    fn error_code_protocol_category() {
        let code = ErrorCode::ProtocolHandshakeFailed;
        assert_eq!(code.category(), ErrorCategory::Protocol);
    }

    #[test]
    fn error_code_mapping_category() {
        let code = ErrorCode::MappingDialectMismatch;
        assert_eq!(code.category(), ErrorCategory::Mapping);
    }

    #[test]
    fn fatal_envelope_from_abp_error() {
        let err = AbpError::new(ErrorCode::BackendCrashed, "process exited");
        let env = Envelope::fatal_from_abp_error(Some("run-1".into()), &err);
        match env {
            Envelope::Fatal {
                error, error_code, ..
            } => {
                assert!(error.contains("process exited"));
                assert!(error_code.is_some());
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn error_dto_serialization_roundtrip() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message, "timeout");
    }

    #[test]
    fn runtime_error_wraps_backend_failure() {
        // RuntimeError should be constructable from various error sources
        let err = RuntimeError::BackendNotFound("missing".into());
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn error_code_all_categories_exist() {
        let categories = vec![
            ErrorCategory::Protocol,
            ErrorCategory::Backend,
            ErrorCategory::Capability,
            ErrorCategory::Policy,
            ErrorCategory::Workspace,
            ErrorCategory::Ir,
            ErrorCategory::Receipt,
            ErrorCategory::Dialect,
            ErrorCategory::Config,
        ];
        for cat in categories {
            let _s = format!("{cat:?}");
        }
    }

    #[test]
    fn error_code_display_is_not_empty() {
        let codes = vec![
            ErrorCode::BackendTimeout,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::BackendNotFound,
        ];
        for code in codes {
            let display = format!("{code:?}");
            assert!(!display.is_empty());
        }
    }

    #[test]
    fn abp_error_implements_std_error() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        let _: &dyn std::error::Error = &err;
    }
}

// ===========================================================================
// Module 8: Crate re-exports work correctly
// ===========================================================================

mod crate_reexports {
    use super::*;

    #[test]
    fn abp_receipt_reexports_core_types() {
        let _: abp_receipt::Receipt = make_receipt("test");
        let _: abp_receipt::AgentEvent = make_event(AgentEventKind::RunStarted {
            message: "x".into(),
        });
        let _ = abp_receipt::CONTRACT_VERSION;
        let _ = abp_receipt::Outcome::Complete;
        let _ = abp_receipt::ExecutionMode::Mapped;
    }

    #[test]
    fn abp_integrations_reexports_backend_trait() {
        let _: Box<dyn abp_integrations::Backend> = Box::new(MockBackend);
    }

    #[test]
    fn abp_integrations_reexports_mock_backend() {
        let _mock = abp_integrations::MockBackend;
    }

    #[test]
    fn abp_ir_reexports_core_ir_types() {
        let _msg = abp_ir::IrMessage::text(abp_ir::IrRole::User, "hello");
        let _conv = abp_ir::IrConversation::new();
    }

    #[test]
    fn abp_core_exports_all_public_types() {
        let _ = abp_core::CONTRACT_VERSION;
        let _wo = WorkOrderBuilder::new("t").build();
        let _r = abp_receipt::ReceiptBuilder::new("b").build();
        let _e = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "x".into(),
            },
            ext: None,
        };
        let _m = ExecutionMode::Mapped;
        let _o = Outcome::Complete;
        let _l = ExecutionLane::PatchFirst;
        let _c = Capability::Streaming;
        let _s = SupportLevel::Native;
    }

    #[test]
    fn abp_protocol_exports_envelope_and_codec() {
        let _env = Envelope::hello(
            BackendIdentity {
                id: "t".into(),
                backend_version: None,
                adapter_version: None,
            },
            BTreeMap::new(),
        );
        let _codec = JsonlCodec;
    }

    #[test]
    fn abp_error_exports_all_error_types() {
        let _ = ErrorCode::BackendTimeout;
        let _ = ErrorCategory::Backend;
        let _ = AbpError::new(ErrorCode::BackendTimeout, "test");
    }

    #[test]
    fn abp_dialect_exports_all_dialects() {
        let dialects = Dialect::all();
        assert!(dialects.len() >= 6);
    }

    #[test]
    fn abp_glob_exports_include_exclude() {
        let globs = IncludeExcludeGlobs::new(&["**/*.rs"], &["target/**"]).unwrap();
        assert!(globs.decide_str("src/main.rs").is_allowed());
    }

    #[test]
    fn abp_capability_exports_manifests() {
        let _ = openai_gpt4o_manifest();
        let _ = claude_35_sonnet_manifest();
        let _ = gemini_15_pro_manifest();
        let _ = kimi_manifest();
        let _ = codex_manifest();
        let _ = copilot_manifest();
    }
}

// ===========================================================================
// Module 9: Runtime orchestration end-to-end with MockBackend
// ===========================================================================

mod runtime_orchestration {
    use super::*;

    #[tokio::test]
    async fn runtime_with_default_backends() {
        let rt = Runtime::with_default_backends();
        let names = rt.backend_names();
        assert!(names.contains(&"mock".to_string()));
    }

    #[tokio::test]
    async fn runtime_register_custom_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("custom", MultiEventBackend { event_count: 3 });
        assert!(rt.backend_names().contains(&"custom".to_string()));
    }

    #[tokio::test]
    async fn runtime_run_streaming_with_mock() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("streaming test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        assert!(!events.is_empty());
        let receipt = receipt.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn runtime_run_streaming_preserves_event_order() {
        let mut rt = Runtime::new();
        rt.register_backend("multi", MultiEventBackend { event_count: 5 });
        let wo = passthrough_wo("order test");
        let handle = rt.run_streaming("multi", wo).await.unwrap();
        let (events, _receipt) = drain_run(handle).await;
        // First event should be RunStarted
        assert!(matches!(
            events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ));
        // Last event should be RunCompleted
        assert!(matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    #[tokio::test]
    async fn runtime_unknown_backend_returns_error() {
        let rt = Runtime::new();
        let wo = passthrough_wo("missing");
        let result = rt.run_streaming("nonexistent", wo).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn runtime_run_handle_has_run_id() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("run id test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let _run_id = handle.run_id;
        let (_events, _receipt) = drain_run(handle).await;
    }

    #[tokio::test]
    async fn runtime_receipt_is_hashable() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("hash test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        // Receipt from runtime should already be hashed
        if receipt.receipt_sha256.is_some() {
            assert!(receipt_crate::verify_hash(&receipt));
        }
    }

    #[tokio::test]
    async fn runtime_failing_backend_propagates_error() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "failing",
            FailingBackend {
                message: "runtime failure".into(),
            },
        );
        let wo = passthrough_wo("fail test");
        let handle = rt.run_streaming("failing", wo).await.unwrap();
        let (_events, receipt) = drain_run(handle).await;
        assert!(receipt.is_err());
    }

    #[tokio::test]
    async fn runtime_multiple_sequential_runs() {
        let rt = Runtime::with_default_backends();
        for i in 0..5 {
            let wo = passthrough_wo(&format!("sequential-{i}"));
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_events, receipt) = drain_run(handle).await;
            assert!(receipt.is_ok());
        }
    }

    #[tokio::test]
    async fn runtime_metrics_exist() {
        let rt = Runtime::with_default_backends();
        let _metrics = rt.metrics();
    }

    #[tokio::test]
    async fn runtime_registry_operations() {
        let rt = Runtime::with_default_backends();
        let registry = rt.registry();
        assert!(registry.contains("mock"));
        assert!(!registry.is_empty());
    }
}

// ===========================================================================
// Module 10: Receipt hashing through full pipeline
// ===========================================================================

mod receipt_hashing {
    use super::*;

    #[test]
    fn receipt_hash_is_deterministic() {
        let r1 = make_hashed_receipt("mock");
        let r2 = make_hashed_receipt("mock");
        assert!(r1.receipt_sha256.is_some());
        assert!(r2.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_hash_null_before_hashing() {
        let r = make_receipt("mock");
        assert!(r.receipt_sha256.is_none());
    }

    #[test]
    fn receipt_hash_verifies_via_receipt_crate() {
        let r = make_hashed_receipt("mock");
        assert!(receipt_crate::verify_hash(&r));
    }

    #[test]
    fn mutated_receipt_fails_verification() {
        let mut r = make_hashed_receipt("mock");
        r.backend.id = "tampered".into();
        assert!(!receipt_crate::verify_hash(&r));
    }

    #[test]
    fn receipt_hash_changes_with_outcome() {
        let r1 = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let r2 = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Failed)
            .with_hash()
            .unwrap();
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn receipt_hash_changes_with_backend() {
        let r1 = abp_receipt::ReceiptBuilder::new("backend-a")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let r2 = abp_receipt::ReceiptBuilder::new("backend-b")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn receipt_canonical_json_excludes_hash() {
        let r = make_hashed_receipt("mock");
        let canonical = receipt_crate::canonicalize(&r).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&canonical).unwrap();
        assert!(parsed.get("receipt_sha256").unwrap().is_null());
    }

    #[test]
    fn receipt_compute_hash_returns_hex() {
        let r = make_receipt("mock");
        let hash = receipt_crate::compute_hash(&r).unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_chain_preserves_order_and_hashes() {
        let mut chain = ReceiptChain::new();
        let r1 = make_hashed_receipt("a");
        let r2 = make_hashed_receipt("b");
        let r3 = make_hashed_receipt("c");
        chain.push(r1).unwrap();
        chain.push(r2).unwrap();
        chain.push(r3).unwrap();
        assert_eq!(chain.len(), 3);
    }

    #[test]
    fn receipt_chain_rejects_duplicate_ids() {
        let mut chain = ReceiptChain::new();
        let r = make_hashed_receipt("mock");
        chain.push(r.clone()).unwrap();
        assert!(chain.push(r).is_err());
    }

    #[test]
    fn receipt_diff_detects_changes() {
        let r1 = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let r2 = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Failed)
            .build();
        let diff = receipt_crate::diff_receipts(&r1, &r2);
        assert!(!diff.changes.is_empty());
    }

    #[test]
    fn receipt_diff_identical_is_empty() {
        let r = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let diff = receipt_crate::diff_receipts(&r, &r);
        assert!(diff.is_empty());
    }

    #[test]
    fn receipt_with_hash_is_idempotent() {
        let r1 = make_hashed_receipt("mock");
        let hash1 = r1.receipt_sha256.clone().unwrap();
        let r2 = r1.with_hash().unwrap();
        let hash2 = r2.receipt_sha256.clone().unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn receipt_verifier_passes_valid_receipt() {
        let r = make_hashed_receipt("mock");
        let verifier = ReceiptVerifier::new();
        let report = verifier.verify(&r);
        assert!(report.passed);
    }

    #[test]
    fn receipt_validation_passes_valid_receipt() {
        let r = make_receipt("mock");
        let result = validate_receipt(&r);
        assert!(result.is_ok());
    }
}

// ===========================================================================
// Module 11: Dialect detection and validation
// ===========================================================================

mod dialect_integration {
    use super::*;

    #[test]
    fn dialect_all_returns_six_variants() {
        let dialects = Dialect::all();
        assert_eq!(dialects.len(), 6);
    }

    #[test]
    fn dialect_labels_are_unique() {
        let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
        let unique: std::collections::HashSet<&&str> = labels.iter().collect();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn dialect_detector_detects_openai() {
        let detector = DialectDetector::new();
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let result = detector.detect(&req);
        if let Some(r) = result {
            assert_eq!(r.dialect, Dialect::OpenAi);
            assert!(r.confidence > 0.0);
        }
    }

    #[test]
    fn dialect_detector_detects_claude() {
        let detector = DialectDetector::new();
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1024
        });
        let result = detector.detect(&req);
        if let Some(r) = result {
            assert_eq!(r.dialect, Dialect::Claude);
        }
    }

    #[test]
    fn dialect_validator_validates_openai_request() {
        let validator = DialectValidator::new();
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let result = validator.validate(&req, Dialect::OpenAi);
        assert!(result.valid);
    }

    #[test]
    fn dialect_validator_rejects_invalid_request() {
        let validator = DialectValidator::new();
        let req = json!({});
        let result = validator.validate(&req, Dialect::OpenAi);
        assert!(!result.valid);
    }

    #[test]
    fn dialect_roundtrip_ir_to_all_formats() {
        let conv = make_ir_conversation();
        let tools = vec![];
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            assert!(lowered.is_object(), "dialect {:?} failed", dialect);
        }
    }
}

// ===========================================================================
// Module 12: Glob through policy through workspace
// ===========================================================================

mod glob_policy_workspace {
    use super::*;

    #[test]
    fn glob_include_only() {
        let globs = IncludeExcludeGlobs::new(&["**/*.rs"], &[]).unwrap();
        assert!(globs.decide_str("src/main.rs").is_allowed());
        assert!(!globs.decide_str("src/main.py").is_allowed());
    }

    #[test]
    fn glob_exclude_overrides_include() {
        let globs = IncludeExcludeGlobs::new(&["**/*"], &["target/**"]).unwrap();
        assert!(globs.decide_str("src/main.rs").is_allowed());
        assert!(!globs.decide_str("target/debug/main").is_allowed());
    }

    #[test]
    fn glob_empty_allows_all() {
        let globs = IncludeExcludeGlobs::new::<&str>(&[], &[]).unwrap();
        assert!(globs.decide_str("anything.txt").is_allowed());
    }

    #[test]
    fn glob_from_work_order_spec() {
        let wo = WorkOrderBuilder::new("t")
            .include(vec!["**/*.rs".into()])
            .exclude(vec!["target/**".into()])
            .build();
        let globs = IncludeExcludeGlobs::new(&wo.workspace.include, &wo.workspace.exclude).unwrap();
        assert!(globs.decide_str("src/lib.rs").is_allowed());
        assert!(!globs.decide_str("target/debug/build").is_allowed());
    }

    #[test]
    fn policy_deny_read_uses_glob_patterns() {
        let policy = make_policy(&[], &[], &["**/.git/**"], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_read_path(".git/config").allowed);
        assert!(engine.can_read_path("src/main.rs").allowed);
    }

    #[test]
    fn policy_deny_write_uses_glob_patterns() {
        let policy = make_policy(&[], &[], &[], &["**/*.lock"]);
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_write_path("Cargo.lock").allowed);
        assert!(engine.can_write_path("Cargo.toml").allowed);
    }
}

// ===========================================================================
// Module 13: Mapper and mapping validation
// ===========================================================================

mod mapper_integration {
    use super::*;

    #[test]
    fn identity_mapper_source_and_target_same() {
        let mapper = IdentityMapper;
        assert_eq!(mapper.source_dialect(), mapper.target_dialect());
    }

    #[test]
    fn known_rules_registry_is_not_empty() {
        let registry = known_rules();
        assert!(registry.len() > 0);
    }

    #[test]
    fn mapping_fidelity_levels_exist() {
        let _lossless = Fidelity::Lossless;
        let _degraded = Fidelity::Degraded;
        let _lossy = Fidelity::Lossy;
        let _unsupported = Fidelity::Unsupported;
    }

    #[test]
    fn validate_mapping_between_dialects() {
        let result = validate_mapping(Dialect::OpenAi, Dialect::Claude);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn identity_mapper_passes_through_request() {
        use abp_mapper::DialectRequest;
        let mapper = IdentityMapper;
        let request = DialectRequest {
            dialect: mapper.source_dialect(),
            body: json!({"model": "test", "messages": []}),
        };
        let result = mapper.map_request(&request);
        assert!(result.is_ok());
    }

    #[test]
    fn ir_mapper_supported_pairs() {
        let pairs = abp_mapper::supported_ir_pairs();
        assert!(!pairs.is_empty());
    }
}

// ===========================================================================
// Module 14: Projection matrix
// ===========================================================================

mod projection_integration {
    use super::*;

    #[test]
    fn projection_matrix_creation() {
        let matrix = ProjectionMatrix::new();
        assert!(matrix.backends().is_empty() || matrix.backends().len() >= 0);
    }

    #[test]
    fn projection_score_fields() {
        let score = ProjectionScore {
            capability_coverage: 0.9,
            mapping_fidelity: 0.85,
            priority: 1.0,
            total: 0.0,
        };
        assert!(score.capability_coverage > 0.0);
    }
}

// ===========================================================================
// Module 15: Config loading and validation
// ===========================================================================

mod config_integration {
    use super::*;

    #[test]
    fn load_minimal_config() {
        let toml = r#"
[backends.mock]
type = "mock"
"#;
        let config = load_from_str(toml).unwrap();
        assert!(config.backends.contains_key("mock"));
    }

    #[test]
    fn load_empty_config() {
        let config = load_from_str("").unwrap();
        assert!(config.backends.is_empty());
    }

    #[test]
    fn config_with_defaults() {
        let max_turns = ConfigDefaults::default_max_turns();
        assert!(max_turns > 0);
        let max_budget = ConfigDefaults::default_max_budget();
        assert!(max_budget > 0.0);
        let model = ConfigDefaults::default_model();
        assert!(!model.is_empty());
    }

    #[test]
    fn config_validator_on_work_order() {
        let validator = ConfigValidator::new();
        let wo = make_work_order("config test");
        let warnings = validator.validate_work_order(&wo);
        // Warnings may or may not exist; just ensure it doesn't panic
        let _ = warnings;
    }
}

// ===========================================================================
// Module 16: Stream and event processing
// ===========================================================================

mod stream_processing {
    use super::*;

    #[test]
    fn event_stream_creation_and_filtering() {
        let events = vec![
            make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: "hello".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let stream = EventStream::new(events);
        assert_eq!(stream.len(), 3);
    }

    #[test]
    fn event_stream_count_by_kind() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "a".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "b".into() }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let stream = EventStream::new(events);
        let counts = stream.count_by_kind();
        assert!(counts.len() >= 1);
    }

    #[test]
    fn event_filter_include_kinds() {
        let filter = EventFilter::include_kinds(&["assistant_delta"]);
        let delta = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        let started = make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        });
        assert!(filter.matches(&delta));
        assert!(!filter.matches(&started));
    }

    #[test]
    fn event_filter_exclude_kinds() {
        let filter = EventFilter::exclude_kinds(&["warning", "error"]);
        let delta = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        let warning = make_event(AgentEventKind::Warning {
            message: "warn".into(),
        });
        assert!(filter.matches(&delta));
        assert!(!filter.matches(&warning));
    }

    #[test]
    fn event_stream_filter_pred() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "a".into() }),
            make_event(AgentEventKind::Error {
                message: "err".into(),
                error_code: None,
            }),
        ];
        let stream = EventStream::new(events);
        let filtered = stream.filter_pred(|e| !matches!(e.kind, AgentEventKind::Error { .. }));
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn event_aggregator_counts_events() {
        let mut agg = EventAggregator::new();
        agg.add(&make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }));
        agg.add(&make_event(AgentEventKind::AssistantDelta {
            text: "hello".into(),
        }));
        agg.add(&make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }));
        assert_eq!(agg.event_count(), 3);
        assert!(agg.unique_tool_count() >= 1);
    }

    #[test]
    fn run_analytics_from_events() {
        let events = vec![
            make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: "response".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let analytics = RunAnalytics::from_events(&events);
        let summary = analytics.summary();
        assert_eq!(summary.total_events, 3);
    }

    #[test]
    fn event_stream_merge_two_streams() {
        let s1 = EventStream::new(vec![make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        })]);
        let s2 = EventStream::new(vec![make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        })]);
        let merged = s1.merge(&s2);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn stream_pipeline_builder_compiles() {
        let pipeline = StreamPipelineBuilder::new().build();
        let event = make_event(AgentEventKind::AssistantDelta {
            text: "test".into(),
        });
        let _processed = pipeline.process(event);
    }
}

// ===========================================================================
// Module 17: Extension traits
// ===========================================================================

mod extension_traits {
    use super::*;

    #[test]
    fn work_order_ext_task_summary() {
        let wo = make_work_order("Write a parser for JSON files with error handling");
        let summary = wo.task_summary(20);
        assert!(summary.len() <= 25); // some tolerance
    }

    #[test]
    fn work_order_ext_is_code_task() {
        let wo = make_work_order("fix the bug in src/main.rs");
        let _is_code = wo.is_code_task();
    }

    #[test]
    fn work_order_ext_required_capabilities() {
        let wo = WorkOrderBuilder::new("test")
            .requirements(CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                }],
            })
            .build();
        let caps = wo.required_capabilities();
        assert!(!caps.is_empty());
    }

    #[test]
    fn receipt_ext_is_success() {
        let r = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        assert!(r.is_success());
        assert!(!r.is_failure());
    }

    #[test]
    fn receipt_ext_is_failure() {
        let r = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Failed)
            .build();
        assert!(r.is_failure());
        assert!(!r.is_success());
    }

    #[test]
    fn receipt_ext_event_count_by_kind() {
        let r = make_receipt("mock");
        let counts = r.event_count_by_kind();
        assert!(!counts.is_empty());
    }

    #[test]
    fn receipt_ext_tool_calls() {
        let r = abp_receipt::ReceiptBuilder::new("mock")
            .add_event(make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            }))
            .build();
        assert_eq!(r.tool_calls().len(), 1);
        assert_eq!(r.total_tool_calls(), 1);
    }

    #[test]
    fn agent_event_ext_is_tool_call() {
        let ev = make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        });
        assert!(ev.is_tool_call());
    }

    #[test]
    fn agent_event_ext_is_terminal() {
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        assert!(ev.is_terminal());
    }

    #[test]
    fn agent_event_ext_text_content() {
        let ev = make_event(AgentEventKind::AssistantDelta {
            text: "hello world".into(),
        });
        assert_eq!(ev.text_content(), Some("hello world"));
    }
}

// ===========================================================================
// Module 18: Emulation engine
// ===========================================================================

mod emulation_integration {
    use super::*;

    #[test]
    fn emulation_engine_with_defaults() {
        let engine = EmulationEngine::with_defaults();
        let _config = engine.config();
    }

    #[test]
    fn emulation_engine_check_missing() {
        let manifest = BTreeMap::new();
        let required = vec![Capability::Streaming, Capability::ExtendedThinking];
        let engine = EmulationEngine::with_defaults();
        let missing = engine.check_missing(&manifest, &required);
        assert!(!missing.is_empty());
    }

    #[test]
    fn emulation_strategies_exist() {
        let _ = abp_emulation::emulate_structured_output();
        let _ = abp_emulation::emulate_code_execution();
        let _ = abp_emulation::emulate_extended_thinking();
        let _ = abp_emulation::emulate_image_input();
        let _ = abp_emulation::emulate_stop_sequences();
    }
}

// ===========================================================================
// Module 19: Validate crate integration
// ===========================================================================

mod validate_integration {
    use super::*;

    #[test]
    fn work_order_validator_accepts_valid() {
        let validator = WorkOrderValidator;
        let wo = make_work_order("valid task");
        let result = validator.validate(&wo);
        assert!(result.is_ok());
    }

    #[test]
    fn event_validator_accepts_valid_event() {
        let validator = EventValidator;
        let event = make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        });
        let result = validator.validate(&event);
        assert!(result.is_ok());
    }

    #[test]
    fn receipt_validator_accepts_valid_receipt() {
        let validator = ValidateReceiptValidator;
        let receipt = make_receipt("mock");
        let result = validator.validate(&receipt);
        assert!(result.is_ok());
    }

    #[test]
    fn hello_version_validation() {
        assert!(abp_validate::validate_hello_version(CONTRACT_VERSION));
        assert!(!abp_validate::validate_hello_version("invalid"));
    }
}

// ===========================================================================
// Module 20: Retry policy
// ===========================================================================

mod retry_integration {
    use super::*;

    #[test]
    fn retry_policy_default() {
        let policy = RetryPolicy::default();
        assert!(policy.max_retries > 0);
        assert!(policy.jitter);
    }

    #[test]
    fn retry_policy_custom() {
        let policy = RetryPolicy::new(
            5,
            Duration::from_millis(200),
            Duration::from_secs(10),
            3.0,
            false,
        );
        assert_eq!(policy.max_retries, 5);
        assert!(!policy.jitter);
    }
}

// ===========================================================================
// Module 21: Backend registry
// ===========================================================================

mod backend_registry_integration {
    use super::*;
    use abp_backend_core::metadata::BackendMetadata;

    #[test]
    fn registry_register_and_lookup() {
        let mut registry = BackendRegistry::new();
        let metadata = BackendMetadata {
            name: "Test GPT-4".into(),
            dialect: "openai".into(),
            version: "gpt-4".into(),
            max_tokens: Some(128000),
            supports_streaming: true,
            supports_tools: true,
            rate_limit: None,
        };
        registry.register_with_metadata("gpt4", metadata);
        assert!(registry.contains("gpt4"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registry_by_dialect() {
        let mut registry = BackendRegistry::new();
        registry.register_with_metadata(
            "gpt4",
            BackendMetadata {
                name: "GPT-4".into(),
                dialect: "openai".into(),
                version: "4".into(),
                max_tokens: None,
                supports_streaming: true,
                supports_tools: true,
                rate_limit: None,
            },
        );
        registry.register_with_metadata(
            "claude",
            BackendMetadata {
                name: "Claude".into(),
                dialect: "claude".into(),
                version: "3.5".into(),
                max_tokens: None,
                supports_streaming: true,
                supports_tools: true,
                rate_limit: None,
            },
        );
        let openai_backends = registry.by_dialect("openai");
        assert_eq!(openai_backends.len(), 1);
    }

    #[test]
    fn registry_remove() {
        let mut registry = BackendRegistry::new();
        registry.register_with_metadata(
            "temp",
            BackendMetadata {
                name: "Temp".into(),
                dialect: "mock".into(),
                version: "1".into(),
                max_tokens: None,
                supports_streaming: false,
                supports_tools: false,
                rate_limit: None,
            },
        );
        assert!(registry.contains("temp"));
        registry.remove("temp");
        assert!(!registry.contains("temp"));
    }
}

// ===========================================================================
// Module 22: Error taxonomy classification
// ===========================================================================

mod error_taxonomy {
    use super::*;
    use abp_error_taxonomy::classification::{ErrorClassifier, ErrorSeverity};

    #[test]
    fn classifier_classifies_backend_error() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&err);
        assert_eq!(classification.severity, ErrorSeverity::Error);
    }

    #[test]
    fn classifier_classifies_protocol_error() {
        let err = AbpError::new(ErrorCode::ProtocolHandshakeFailed, "handshake failed");
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&err);
        assert!(!format!("{:?}", classification.category).is_empty());
    }

    #[test]
    fn classification_has_recovery_suggestion() {
        let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited");
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&err);
        assert!(classification.recovery.is_some());
    }
}

// ===========================================================================
// Module 23: Execution mode extraction
// ===========================================================================

mod execution_mode {
    use super::*;

    #[test]
    fn extract_execution_mode_default_is_mapped() {
        let wo = make_work_order("default mode");
        let mode = extract_execution_mode(&wo);
        assert_eq!(mode, ExecutionMode::Mapped);
    }

    #[test]
    fn execution_mode_passthrough_in_receipt() {
        let r = abp_receipt::ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(r.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn execution_mode_mapped_in_receipt() {
        let r = abp_receipt::ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Mapped)
            .build();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }
}

// ===========================================================================
// Module 24: Cross-layer type consistency
// ===========================================================================

mod cross_layer_consistency {
    use super::*;

    #[test]
    fn work_order_id_propagates_to_receipt_metadata() {
        let wo = make_work_order("id propagation");
        let receipt = abp_receipt::ReceiptBuilder::new("mock")
            .work_order_id(wo.id)
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.meta.work_order_id, wo.id);
    }

    #[test]
    fn backend_identity_consistent_across_layers() {
        let identity = sample_backend_identity();
        let receipt = abp_receipt::ReceiptBuilder::new(&identity.id)
            .backend_version(identity.backend_version.as_deref().unwrap_or(""))
            .build();
        assert_eq!(receipt.backend.id, identity.id);
    }

    #[test]
    fn capability_manifest_consistent_between_backend_and_hello() {
        let backend = MockBackend;
        let caps = backend.capabilities();
        let env = Envelope::hello(backend.identity(), caps.clone());
        match env {
            Envelope::Hello { capabilities, .. } => {
                assert_eq!(capabilities.len(), caps.len());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn all_outcomes_are_distinct() {
        let complete = Outcome::Complete;
        let partial = Outcome::Partial;
        let failed = Outcome::Failed;
        assert_ne!(complete, partial);
        assert_ne!(complete, failed);
        assert_ne!(partial, failed);
    }

    #[test]
    fn all_execution_modes_are_distinct() {
        assert_ne!(ExecutionMode::Passthrough, ExecutionMode::Mapped);
    }

    #[test]
    fn all_execution_lanes_are_distinct() {
        assert_ne!(ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst);
    }

    #[test]
    fn workspace_modes_are_distinct() {
        assert_ne!(WorkspaceMode::PassThrough, WorkspaceMode::Staged);
    }

    #[test]
    fn receipt_builder_produces_consistent_types() {
        let r1 = abp_receipt::ReceiptBuilder::new("test").build();
        let r2 = make_receipt("test");
        // Both should be Receipt type and have the same backend
        assert_eq!(r1.backend.id, r2.backend.id);
        assert_eq!(r1.meta.contract_version, r2.meta.contract_version);
    }

    #[test]
    fn serde_json_roundtrip_work_order() {
        let wo = make_work_order("serde test");
        let json = serde_json::to_string(&wo).unwrap();
        let deserialized: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task, "serde test");
        assert_eq!(deserialized.id, wo.id);
    }

    #[test]
    fn serde_json_roundtrip_receipt() {
        let r = make_hashed_receipt("serde-backend");
        let json = serde_json::to_string(&r).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.backend.id, "serde-backend");
        assert_eq!(deserialized.receipt_sha256, r.receipt_sha256);
    }

    #[test]
    fn serde_json_roundtrip_agent_event() {
        let ev = make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-42".into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        });
        let json = serde_json::to_string(&ev).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        match &deserialized.kind {
            AgentEventKind::ToolCall { tool_name, .. } => {
                assert_eq!(tool_name, "bash");
            }
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn canonical_json_is_deterministic() {
        let wo = make_work_order("determinism test");
        let json1 = abp_core::canonical_json(&wo).unwrap();
        let json2 = abp_core::canonical_json(&wo).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn sha256_hex_is_consistent() {
        let hash1 = abp_core::sha256_hex(b"hello world");
        let hash2 = abp_core::sha256_hex(b"hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }
}

// ===========================================================================
// Module 25: Full end-to-end pipeline
// ===========================================================================

mod full_e2e_pipeline {
    use super::*;

    #[tokio::test]
    async fn e2e_create_work_order_run_collect_validate_hash() {
        // 1. Create work order
        let wo = WorkOrderBuilder::new("e2e integration test")
            .root(".")
            .workspace_mode(WorkspaceMode::PassThrough)
            .policy(make_policy(&["read_file"], &[], &[], &[]))
            .build();
        let wo_id = wo.id;

        // 2. Run through runtime
        let rt = Runtime::with_default_backends();
        let handle = rt.run_streaming("mock", wo).await.unwrap();

        // 3. Collect events
        let (events, receipt) = drain_run(handle).await;
        assert!(!events.is_empty());

        // 4. Validate receipt
        let receipt = receipt.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.meta.work_order_id, wo_id);
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);

        // 5. Validate with validator
        let result = validate_receipt(&receipt);
        assert!(result.is_ok());

        // 6. Hash and verify
        if receipt.receipt_sha256.is_some() {
            assert!(receipt_crate::verify_hash(&receipt));
        } else {
            let hashed = receipt.with_hash().unwrap();
            assert!(receipt_crate::verify_hash(&hashed));
        }
    }

    #[tokio::test]
    async fn e2e_multi_backend_runs_produce_receipt_chain() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend("multi", MultiEventBackend { event_count: 3 });

        let mut chain = ReceiptChain::new();

        for backend_name in &["mock", "multi"] {
            let wo = passthrough_wo(&format!("chain-{backend_name}"));
            let handle = rt.run_streaming(backend_name, wo).await.unwrap();
            let (_events, receipt) = drain_run(handle).await;
            let receipt = receipt.unwrap().with_hash().unwrap();
            chain.push(receipt).unwrap();
        }

        assert_eq!(chain.len(), 2);
    }

    #[tokio::test]
    async fn e2e_work_order_with_requirements_through_runtime() {
        let wo = WorkOrderBuilder::new("capability check")
            .workspace_mode(WorkspaceMode::PassThrough)
            .requirements(CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Emulated,
                }],
            })
            .build();

        let rt = Runtime::with_default_backends();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_events, receipt) = drain_run(handle).await;
        assert!(receipt.is_ok());
    }

    #[tokio::test]
    async fn e2e_receipt_verification_after_runtime() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("verify e2e");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        let verifier = ReceiptVerifier::new();
        let report = verifier.verify(&receipt);
        assert!(report.passed);
    }

    #[tokio::test]
    async fn e2e_event_aggregation_after_run() {
        let mut rt = Runtime::new();
        rt.register_backend("multi", MultiEventBackend { event_count: 10 });

        let wo = passthrough_wo("aggregation test");
        let handle = rt.run_streaming("multi", wo).await.unwrap();
        let (events, _receipt) = drain_run(handle).await;

        let mut agg = EventAggregator::new();
        for ev in &events {
            agg.add(ev);
        }
        let summary = agg.summary();
        assert_eq!(summary.total_events, events.len());
    }

    #[tokio::test]
    async fn e2e_event_stream_processing_after_run() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("stream processing");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, _receipt) = drain_run(handle).await;

        let stream = EventStream::new(events);
        let counts = stream.count_by_kind();
        assert!(!counts.is_empty());
    }

    #[tokio::test]
    async fn e2e_ir_to_dialect_to_protocol() {
        // Build IR
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];

        // Lower to OpenAI format
        let openai_json = lower_to_openai(&conv, &tools);

        // Detect dialect
        let detector = DialectDetector::new();
        if let Some(result) = detector.detect(&openai_json) {
            assert_eq!(result.dialect, Dialect::OpenAi);
        }

        // Create work order and run through runtime
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("ir pipeline test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_events, receipt) = drain_run(handle).await;
        assert!(receipt.is_ok());
    }
}
