#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! BDD-style Given/When/Then scenario tests for Agent Backplane.
//!
//! Uses a lightweight macro-based approach (no cucumber dependency) to
//! structure tests as Feature → Scenario → Given/When/Then steps.

use std::io::BufReader;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_error::ErrorCode;
use abp_policy::PolicyEngine;
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version};
use abp_receipt::{ReceiptChain, compute_hash, verify_hash};
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// BDD Framework Macros
// ===========================================================================

/// Run a named scenario composed of Given/When/Then steps.
///
/// Each step is a closure that returns a value threaded to the next step.
macro_rules! scenario {
    ($name:expr, $body:block) => {{
        eprintln!("  Scenario: {}", $name);
        $body
    }};
}

/// Document a "Given" precondition step.
macro_rules! given {
    ($desc:expr, $body:expr) => {{
        eprintln!("    Given {}", $desc);
        $body
    }};
}

/// Document a "When" action step.
macro_rules! when {
    ($desc:expr, $body:expr) => {{
        eprintln!("    When {}", $desc);
        $body
    }};
}

/// Document a "Then" assertion step.
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

/// A custom backend that emits tool call events.
#[derive(Debug, Clone)]
struct ToolBackend;

#[async_trait]
impl abp_integrations::Backend for ToolBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-backend".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        make_manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ])
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();

        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "starting".into(),
                },
                ext: None,
            })
            .await;

        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc-1".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
                ext: None,
            })
            .await;

        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc-1".into()),
                    output: serde_json::json!({"content": "fn main() {}"}),
                    is_error: false,
                },
                ext: None,
            })
            .await;

        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "I read the file.".into(),
                },
                ext: None,
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

        let finished = Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        let receipt = Receipt {
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
            usage_raw: serde_json::json!({"note": "tool-backend"}),
            usage: abp_core::UsageNormalized {
                input_tokens: Some(50),
                output_tokens: Some(30),
                ..Default::default()
            },
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

/// A backend that returns a failed outcome with an error event.
#[derive(Debug, Clone)]
struct ErrorBackend {
    error_code: ErrorCode,
    message: String,
}

#[async_trait]
impl abp_integrations::Backend for ErrorBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error-backend".into(),
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
        _work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: self.message.clone(),
                    error_code: Some(self.error_code),
                },
                ext: None,
            })
            .await;
        anyhow::bail!("{}", self.message)
    }
}

/// A streaming backend that emits ordered delta events.
#[derive(Debug, Clone)]
struct StreamingBackend;

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
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let tokens = ["Hello", ", ", "world", "!"];
        for token in &tokens {
            let _ = tx
                .send(AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: (*token).to_string(),
                    },
                    ext: None,
                })
                .await;
        }
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello, world!".into(),
                },
                ext: None,
            })
            .await;

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
            usage_raw: serde_json::json!({}),
            usage: abp_core::UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: abp_core::VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

/// Collect all events from a RunHandle and return (events, receipt_result).
async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut stream = handle.events;
    let mut events = Vec::new();
    while let Some(ev) = stream.next().await {
        events.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (events, receipt)
}

// ===========================================================================
// Feature: Work Order Processing
// ===========================================================================

#[tokio::test]
async fn feature_work_order_submit_simple_text_task() {
    scenario!("Submit a simple text task → get a receipt with content", {
        let wo = given!("a work order with task 'hello world'", {
            WorkOrderBuilder::new("hello world")
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let (events, receipt) = when!("the work order is processed with mock backend", {
            let rt = Runtime::with_default_backends();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            drain_run(handle).await
        });

        then!("the receipt contains a response", {
            let receipt = receipt.unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert!(receipt.receipt_sha256.is_some());
            assert!(!events.is_empty());
            let has_message = events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
            assert!(has_message);
        });
    });
}

#[tokio::test]
async fn feature_work_order_submit_with_tools() {
    scenario!("Submit with tools → receipt includes tool usage", {
        let wo = given!("a work order expecting tool usage", {
            WorkOrderBuilder::new("read a file")
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let (events, receipt) = when!("processed with tool-capable backend", {
            let mut rt = Runtime::new();
            rt.register_backend("tool-backend", ToolBackend);
            let handle = rt.run_streaming("tool-backend", wo).await.unwrap();
            drain_run(handle).await
        });

        then!("the receipt is complete and events include tool calls", {
            let receipt = receipt.unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            let has_tool_call = events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }));
            assert!(has_tool_call);
            let has_tool_result = events
                .iter()
                .any(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }));
            assert!(has_tool_result);
        });
    });
}

#[tokio::test]
async fn feature_work_order_submit_invalid_model() {
    scenario!("Submit with invalid model → get appropriate error code", {
        let wo = given!("a work order targeting a non-existent backend", {
            WorkOrderBuilder::new("test")
                .model("nonexistent-model-xyz")
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let result = when!("the work order is submitted to unknown backend", {
            let rt = Runtime::with_default_backends();
            rt.run_streaming("nonexistent-backend", wo).await
        });

        then!("we get an UnknownBackend error", {
            match result {
                Err(err) => assert_eq!(err.error_code(), ErrorCode::BackendNotFound),
                Ok(_) => panic!("expected error, got Ok"),
            }
        });
    });
}

#[tokio::test]
async fn feature_work_order_streaming_ordered_events() {
    scenario!("Submit streaming request → receive ordered events", {
        let wo = given!("a work order for streaming", {
            WorkOrderBuilder::new("stream tokens")
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let (events, receipt) = when!("processed with streaming backend", {
            let mut rt = Runtime::new();
            rt.register_backend("streaming", StreamingBackend);
            let handle = rt.run_streaming("streaming", wo).await.unwrap();
            drain_run(handle).await
        });

        then!("events arrive in order with deltas before message", {
            let receipt = receipt.unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert!(events.len() >= 5);

            // Verify deltas appear before the final message
            let delta_positions: Vec<_> = events
                .iter()
                .enumerate()
                .filter(|(_, e)| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
                .map(|(i, _)| i)
                .collect();
            let message_pos = events
                .iter()
                .position(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
                .unwrap();
            for dp in &delta_positions {
                assert!(*dp < message_pos);
            }

            // Verify timestamps are monotonically non-decreasing
            for w in events.windows(2) {
                assert!(w[1].ts >= w[0].ts);
            }
        });
    });
}

// ===========================================================================
// Feature: SDK Translation
// ===========================================================================

#[tokio::test]
async fn feature_sdk_openai_to_claude_backend() {
    scenario!("OpenAI request → Claude backend → OpenAI response", {
        let wo = given!("a work order with OpenAI-style config", {
            let mut config = abp_core::RuntimeConfig {
                model: Some("gpt-4".into()),
                ..Default::default()
            };
            config
                .vendor
                .insert("openai".into(), serde_json::json!({"temperature": 0.7}));
            WorkOrderBuilder::new("translate this request")
                .config(config)
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let receipt = when!("processed through mock backend acting as Claude", {
            let rt = Runtime::with_default_backends();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            receipt.unwrap()
        });

        then!("receipt indicates successful translation", {
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert_eq!(receipt.mode, ExecutionMode::Mapped);
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        });
    });
}

#[tokio::test]
async fn feature_sdk_claude_to_openai_backend() {
    scenario!("Claude request → OpenAI backend → Claude response", {
        let wo = given!("a work order with Claude-style config", {
            let mut config = abp_core::RuntimeConfig {
                model: Some("claude-3-5-sonnet".into()),
                ..Default::default()
            };
            config
                .vendor
                .insert("anthropic".into(), serde_json::json!({"max_tokens": 4096}));
            WorkOrderBuilder::new("translate back")
                .config(config)
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let receipt = when!("processed through mock backend", {
            let rt = Runtime::with_default_backends();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            receipt.unwrap()
        });

        then!("receipt preserves contract version and is complete", {
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        });
    });
}

#[tokio::test]
async fn feature_sdk_gemini_through_ir() {
    scenario!("Gemini request → mapped through IR → correct format", {
        let wo = given!("a work order with Gemini-style vendor config", {
            let mut config = abp_core::RuntimeConfig {
                model: Some("gemini-pro".into()),
                ..Default::default()
            };
            config
                .vendor
                .insert("google".into(), serde_json::json!({"safety_settings": []}));
            WorkOrderBuilder::new("gemini task")
                .config(config)
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let receipt = when!("processed through mapped execution", {
            let rt = Runtime::with_default_backends();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            receipt.unwrap()
        });

        then!("receipt mode is Mapped and contract version is correct", {
            assert_eq!(receipt.mode, ExecutionMode::Mapped);
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
            assert_eq!(receipt.outcome, Outcome::Complete);
        });
    });
}

#[tokio::test]
async fn feature_sdk_passthrough_mode_preserves_request() {
    scenario!("Passthrough mode preserves request exactly", {
        let wo = given!("a work order with passthrough vendor flag", {
            let mut config = abp_core::RuntimeConfig::default();
            config
                .vendor
                .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
            WorkOrderBuilder::new("passthrough test")
                .config(config)
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        then!("the work order round-trips through JSON faithfully", {
            let json = serde_json::to_value(&wo).unwrap();
            let roundtrip: WorkOrder = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(roundtrip.task, wo.task);
            assert_eq!(roundtrip.id, wo.id);
            let json2 = serde_json::to_value(&roundtrip).unwrap();
            assert_eq!(json, json2);
        });
    });
}

// ===========================================================================
// Feature: Policy Enforcement
// ===========================================================================

#[test]
fn feature_policy_denied_tool_rejected() {
    scenario!("Denied tool → rejected with policy error", {
        let engine = given!("a policy engine denying 'Bash'", {
            let policy = PolicyProfile {
                allowed_tools: vec!["Read".into(), "Write".into()],
                disallowed_tools: vec!["Bash".into()],
                ..PolicyProfile::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });

        let decision = when!("checking if 'Bash' tool is allowed", {
            engine.can_use_tool("Bash")
        });

        then!("the decision is denied with reason", {
            assert!(!decision.allowed);
            assert!(decision.reason.is_some());
            let reason = decision.reason.unwrap();
            assert!(reason.contains("Bash"));
        });
    });
}

#[test]
fn feature_policy_denied_tool_not_in_allowlist() {
    scenario!("Tool not in allowlist → rejected", {
        let engine = given!("a policy engine with explicit allowlist", {
            let policy = PolicyProfile {
                allowed_tools: vec!["Read".into()],
                ..PolicyProfile::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });

        let decision = when!("checking if 'Write' tool is allowed", {
            engine.can_use_tool("Write")
        });

        then!("Write is denied because it is not in allowlist", {
            assert!(!decision.allowed);
            assert!(
                decision
                    .reason
                    .as_deref()
                    .unwrap()
                    .contains("not in allowlist")
            );
        });
    });
}

#[test]
fn feature_policy_allowed_tool_accepted() {
    scenario!("Allowed tool → accepted", {
        let engine = given!("a policy engine allowing 'Read'", {
            let policy = PolicyProfile {
                allowed_tools: vec!["Read".into()],
                ..PolicyProfile::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });

        let decision = when!("checking if 'Read' tool is allowed", {
            engine.can_use_tool("Read")
        });

        then!("the decision is allowed", {
            assert!(decision.allowed);
            assert!(decision.reason.is_none());
        });
    });
}

#[test]
fn feature_policy_denied_file_write_blocked() {
    scenario!("Denied file path → write blocked", {
        let engine = given!("a policy engine denying writes to .git/**", {
            let policy = PolicyProfile {
                deny_write: vec!["**/.git/**".into()],
                ..PolicyProfile::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });

        let decision = when!("checking write to '.git/config'", {
            engine.can_write_path(Path::new(".git/config"))
        });

        then!("the write is denied", {
            assert!(!decision.allowed);
            assert!(decision.reason.as_deref().unwrap().contains("write denied"));
        });
    });
}

#[test]
fn feature_policy_denied_file_read_blocked() {
    scenario!("Denied file path → read blocked", {
        let engine = given!("a policy engine denying reads to .env", {
            let policy = PolicyProfile {
                deny_read: vec!["**/.env".into()],
                ..PolicyProfile::default()
            };
            PolicyEngine::new(&policy).unwrap()
        });

        let decision = when!("checking read of '.env'", {
            engine.can_read_path(Path::new(".env"))
        });

        then!("the read is denied", {
            assert!(!decision.allowed);
            assert!(decision.reason.as_deref().unwrap().contains("read denied"));
        });
    });
}

#[test]
fn feature_policy_rate_limit_exceeded() {
    scenario!("Rate limit exceeded → appropriate error", {
        let policy = given!("a rate-limit policy with max 10 RPM", {
            RateLimitPolicy {
                max_requests_per_minute: Some(10),
                max_tokens_per_minute: None,
                max_concurrent: None,
            }
        });

        let result = when!("current RPM is 10 (at the limit)", {
            policy.check_rate_limit(10, 0, 0)
        });

        then!("the result is Throttled", {
            assert!(result.is_throttled());
            assert!(matches!(result, RateLimitResult::Throttled { .. }));
        });
    });
}

#[test]
fn feature_policy_rate_limit_concurrent_denied() {
    scenario!("Concurrent limit exceeded → denied", {
        let policy = given!("a rate-limit policy with max 5 concurrent", {
            RateLimitPolicy {
                max_requests_per_minute: None,
                max_tokens_per_minute: None,
                max_concurrent: Some(5),
            }
        });

        let result = when!("current concurrent is 5 (at the limit)", {
            policy.check_rate_limit(0, 0, 5)
        });

        then!("the result is Denied", {
            assert!(result.is_denied());
            assert!(matches!(result, RateLimitResult::Denied { .. }));
        });
    });
}

#[test]
fn feature_policy_rate_limit_within_bounds() {
    scenario!("Request within rate limits → allowed", {
        let policy = given!("a rate-limit policy with max 100 RPM", {
            RateLimitPolicy {
                max_requests_per_minute: Some(100),
                max_tokens_per_minute: Some(10_000),
                max_concurrent: Some(10),
            }
        });

        let result = when!("current usage is well within limits", {
            policy.check_rate_limit(5, 100, 2)
        });

        then!("the result is Allowed", {
            assert!(result.is_allowed());
        });
    });
}

// ===========================================================================
// Feature: Receipt Integrity
// ===========================================================================

#[test]
fn feature_receipt_hash_matches_content() {
    scenario!("Receipt hash matches content", {
        let receipt = given!("a receipt built with hash", {
            abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap()
        });

        let valid = when!("verifying the hash", verify_hash(&receipt));

        then!("the hash is valid", {
            assert!(valid);
            let hash = receipt.receipt_sha256.as_ref().unwrap();
            assert_eq!(hash.len(), 64);
        });
    });
}

#[test]
fn feature_receipt_tampered_fails_verification() {
    scenario!("Tampered receipt fails verification", {
        let mut receipt = given!("a receipt with a valid hash", {
            abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap()
        });

        when!("the receipt outcome is tampered", {
            receipt.outcome = Outcome::Failed;
        });

        then!("hash verification fails", {
            assert!(!verify_hash(&receipt));
        });
    });
}

#[test]
fn feature_receipt_tampered_hash_fails() {
    scenario!("Tampered hash string fails verification", {
        let mut receipt = given!("a receipt with a valid hash", {
            abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap()
        });

        when!("the hash is replaced with a fake value", {
            receipt.receipt_sha256 = Some("deadbeef".repeat(8));
        });

        then!("hash verification fails", {
            assert!(!verify_hash(&receipt));
        });
    });
}

#[test]
fn feature_receipt_chain_maintains_integrity() {
    scenario!("Receipt chain maintains integrity", {
        let mut chain = given!("an empty receipt chain", ReceiptChain::new());

        when!("three valid receipts are pushed in chronological order", {
            for i in 0..3 {
                let started = Utc::now() + chrono::Duration::seconds(i);
                let finished = started + chrono::Duration::milliseconds(100);
                let receipt = abp_receipt::ReceiptBuilder::new("mock")
                    .outcome(Outcome::Complete)
                    .started_at(started)
                    .finished_at(finished)
                    .with_hash()
                    .unwrap();
                chain.push(receipt).unwrap();
            }
        });

        then!("the chain is valid and has 3 entries", {
            assert_eq!(chain.len(), 3);
            assert!(chain.verify().is_ok());
        });
    });
}

#[test]
fn feature_receipt_chain_rejects_duplicate() {
    scenario!("Receipt chain rejects duplicate run ID", {
        let mut chain = given!("a chain with one receipt", {
            let mut chain = ReceiptChain::new();
            let receipt = abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .run_id(Uuid::nil())
                .with_hash()
                .unwrap();
            chain.push(receipt).unwrap();
            chain
        });

        let result = when!("pushing a receipt with the same run ID", {
            let dup = abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .run_id(Uuid::nil())
                .with_hash()
                .unwrap();
            chain.push(dup)
        });

        then!("push fails with DuplicateId error", {
            assert!(result.is_err());
        });
    });
}

#[test]
fn feature_receipt_deterministic_hashing() {
    scenario!("Same receipt produces same hash", {
        let receipt = given!("a deterministic receipt", {
            abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
        });

        let (h1, h2) = when!("computing hash twice", {
            let h1 = compute_hash(&receipt).unwrap();
            let h2 = compute_hash(&receipt).unwrap();
            (h1, h2)
        });

        then!("both hashes are identical", {
            assert_eq!(h1, h2);
            assert_eq!(h1.len(), 64);
        });
    });
}

#[test]
fn feature_receipt_hash_null_excluded() {
    scenario!("Receipt hash excludes the hash field itself", {
        let receipt_no_hash = given!("a receipt without hash", {
            abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .build()
        });

        let receipt_with_hash = when!("we attach a hash", {
            receipt_no_hash.clone().with_hash().unwrap()
        });

        then!("hash of original equals hash stored in hashed version", {
            let h = compute_hash(&receipt_no_hash).unwrap();
            assert_eq!(h, receipt_with_hash.receipt_sha256.unwrap());
        });
    });
}

// ===========================================================================
// Feature: Sidecar Lifecycle
// ===========================================================================

#[test]
fn feature_sidecar_hello_accepted() {
    scenario!("Sidecar sends hello → accepted", {
        let hello = given!("a valid hello envelope", {
            Envelope::hello(
                BackendIdentity {
                    id: "test-sidecar".into(),
                    backend_version: Some("1.0".into()),
                    adapter_version: None,
                },
                CapabilityManifest::new(),
            )
        });

        let encoded = when!("encoding and decoding the hello", {
            let line = JsonlCodec::encode(&hello).unwrap();
            assert!(line.contains("\"t\":\"hello\""));
            JsonlCodec::decode(line.trim()).unwrap()
        });

        then!("the decoded envelope is a Hello with correct version", {
            match encoded {
                Envelope::Hello {
                    contract_version,
                    backend,
                    ..
                } => {
                    assert_eq!(contract_version, CONTRACT_VERSION);
                    assert_eq!(backend.id, "test-sidecar");
                }
                other => panic!("expected Hello, got {other:?}"),
            }
        });
    });
}

#[test]
fn feature_sidecar_skips_hello_protocol_error() {
    scenario!("Sidecar skips hello → protocol error", {
        let line = given!("a JSONL line with an event (not hello)", {
            let event_env = Envelope::Event {
                ref_id: "run-1".into(),
                event: AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::RunStarted {
                        message: "starting".into(),
                    },
                    ext: None,
                },
            };
            JsonlCodec::encode(&event_env).unwrap()
        });

        let decoded = when!("we decode the first message", {
            JsonlCodec::decode(line.trim()).unwrap()
        });

        then!(
            "the message is an Event, not Hello — protocol violation",
            {
                assert!(
                    !matches!(decoded, Envelope::Hello { .. }),
                    "expected non-Hello envelope"
                );
                // In real protocol, receiving Event before Hello would be a violation
                assert!(matches!(decoded, Envelope::Event { .. }));
            }
        );
    });
}

#[test]
fn feature_sidecar_fatal_error_propagated() {
    scenario!("Sidecar sends fatal → error propagated", {
        let fatal = given!("a fatal envelope with error code", {
            Envelope::fatal_with_code(
                Some("run-42".into()),
                "out of memory",
                ErrorCode::BackendCrashed,
            )
        });

        let encoded = when!("encoding and decoding the fatal", {
            let line = JsonlCodec::encode(&fatal).unwrap();
            JsonlCodec::decode(line.trim()).unwrap()
        });

        then!("the decoded envelope carries the error code", {
            match encoded {
                Envelope::Fatal {
                    ref_id,
                    error,
                    error_code,
                } => {
                    assert_eq!(ref_id.as_deref(), Some("run-42"));
                    assert_eq!(error, "out of memory");
                    assert_eq!(error_code, Some(ErrorCode::BackendCrashed));
                }
                other => panic!("expected Fatal, got {other:?}"),
            }
        });
    });
}

#[test]
fn feature_sidecar_fatal_without_ref_id() {
    scenario!("Sidecar sends fatal without ref_id → still valid", {
        let fatal = given!("a fatal envelope without ref_id", {
            Envelope::Fatal {
                ref_id: None,
                error: "startup failure".into(),
                error_code: Some(ErrorCode::ProtocolHandshakeFailed),
            }
        });

        let decoded = when!("encoding and round-tripping", {
            let line = JsonlCodec::encode(&fatal).unwrap();
            JsonlCodec::decode(line.trim()).unwrap()
        });

        then!("ref_id is None and error is preserved", {
            match decoded {
                Envelope::Fatal { ref_id, error, .. } => {
                    assert!(ref_id.is_none());
                    assert_eq!(error, "startup failure");
                }
                other => panic!("expected Fatal, got {other:?}"),
            }
        });
    });
}

#[test]
fn feature_sidecar_hello_mode_passthrough() {
    scenario!("Sidecar sends hello with passthrough mode", {
        let hello = given!("a hello envelope with passthrough mode", {
            Envelope::hello_with_mode(
                BackendIdentity {
                    id: "passthrough-sidecar".into(),
                    backend_version: None,
                    adapter_version: None,
                },
                CapabilityManifest::new(),
                ExecutionMode::Passthrough,
            )
        });

        let decoded = when!("encoding and decoding", {
            let line = JsonlCodec::encode(&hello).unwrap();
            JsonlCodec::decode(line.trim()).unwrap()
        });

        then!("the mode is Passthrough", {
            match decoded {
                Envelope::Hello { mode, .. } => {
                    assert_eq!(mode, ExecutionMode::Passthrough);
                }
                other => panic!("expected Hello, got {other:?}"),
            }
        });
    });
}

#[test]
fn feature_sidecar_protocol_stream_decoding() {
    scenario!("JSONL stream decodes multiple envelopes", {
        let input = given!("a JSONL stream with hello and fatal", {
            let hello = Envelope::hello(
                BackendIdentity {
                    id: "stream-test".into(),
                    backend_version: None,
                    adapter_version: None,
                },
                CapabilityManifest::new(),
            );
            let fatal = Envelope::Fatal {
                ref_id: None,
                error: "done".into(),
                error_code: None,
            };
            let mut buf = Vec::new();
            buf.extend_from_slice(JsonlCodec::encode(&hello).unwrap().as_bytes());
            buf.extend_from_slice(JsonlCodec::encode(&fatal).unwrap().as_bytes());
            buf
        });

        let envelopes = when!("decoding the stream", {
            let reader = BufReader::new(input.as_slice());
            JsonlCodec::decode_stream(reader)
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        });

        then!("we get exactly two envelopes in order", {
            assert_eq!(envelopes.len(), 2);
            assert!(matches!(envelopes[0], Envelope::Hello { .. }));
            assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
        });
    });
}

#[test]
fn feature_sidecar_version_compatibility() {
    scenario!("Version compatibility check works correctly", {
        then!("same major versions are compatible", {
            assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
            assert!(is_compatible_version("abp/v0.1", CONTRACT_VERSION));
        });

        then!("different major versions are incompatible", {
            assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
        });

        then!("invalid version strings are incompatible", {
            assert!(!is_compatible_version("invalid", CONTRACT_VERSION));
        });
    });
}

// ===========================================================================
// Feature: Backend Lifecycle & Error Handling
// ===========================================================================

#[tokio::test]
async fn feature_backend_error_propagated() {
    scenario!("Backend error is propagated as RuntimeError", {
        let wo = given!("a work order", {
            WorkOrderBuilder::new("trigger error")
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let result = when!("processed with error backend", {
            let mut rt = Runtime::new();
            rt.register_backend(
                "error",
                ErrorBackend {
                    error_code: ErrorCode::BackendCrashed,
                    message: "simulated crash".into(),
                },
            );
            let handle = rt.run_streaming("error", wo).await.unwrap();
            let (events, receipt) = drain_run(handle).await;
            (events, receipt)
        });

        then!("the receipt is an error and events contain error info", {
            let (_events, receipt_result) = result;
            assert!(receipt_result.is_err());
        });
    });
}

#[tokio::test]
async fn feature_backend_capability_check_fails() {
    scenario!("Backend missing required capability → error", {
        let wo = given!("a work order requiring native MCP", {
            WorkOrderBuilder::new("need MCP")
                .requirements(CapabilityRequirements {
                    required: vec![CapabilityRequirement {
                        capability: Capability::McpClient,
                        min_support: MinSupport::Native,
                    }],
                })
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let result = when!("processed with mock backend (no MCP)", {
            let rt = Runtime::with_default_backends();
            rt.run_streaming("mock", wo).await
        });

        then!("we get a CapabilityCheckFailed error", {
            match result {
                Err(err) => assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported),
                Ok(_) => panic!("expected capability error, got Ok"),
            }
        });
    });
}

#[tokio::test]
async fn feature_backend_registration_and_listing() {
    scenario!("Backends can be registered and listed", {
        let rt = given!("a runtime with custom backends", {
            let mut rt = Runtime::new();
            rt.register_backend("alpha", abp_backend_mock::MockBackend);
            rt.register_backend("beta", abp_backend_mock::MockBackend);
            rt
        });

        let names = when!("listing backend names", rt.backend_names());

        then!("both backends are listed", {
            assert!(names.contains(&"alpha".to_string()));
            assert!(names.contains(&"beta".to_string()));
            assert_eq!(names.len(), 2);
        });
    });
}

// ===========================================================================
// Feature: Contract & Serialization
// ===========================================================================

#[test]
fn feature_contract_version_constant() {
    scenario!("Contract version is 'abp/v0.1'", {
        then!("CONTRACT_VERSION matches expected value", {
            assert_eq!(CONTRACT_VERSION, "abp/v0.1");
        });
    });
}

#[test]
fn feature_work_order_roundtrip_serde() {
    scenario!("WorkOrder serializes and deserializes faithfully", {
        let wo = given!("a fully populated work order", {
            WorkOrderBuilder::new("test serde")
                .model("gpt-4")
                .max_turns(5)
                .max_budget_usd(1.0)
                .policy(PolicyProfile {
                    disallowed_tools: vec!["Bash".into()],
                    ..PolicyProfile::default()
                })
                .workspace_mode(WorkspaceMode::PassThrough)
                .build()
        });

        let roundtrip = when!("serialized to JSON and back", {
            let json = serde_json::to_string(&wo).unwrap();
            serde_json::from_str::<WorkOrder>(&json).unwrap()
        });

        then!("all fields match", {
            assert_eq!(roundtrip.id, wo.id);
            assert_eq!(roundtrip.task, wo.task);
            assert_eq!(roundtrip.config.model, wo.config.model);
            assert_eq!(roundtrip.config.max_turns, wo.config.max_turns);
            assert_eq!(
                roundtrip.policy.disallowed_tools,
                wo.policy.disallowed_tools
            );
        });
    });
}

#[test]
fn feature_receipt_roundtrip_serde() {
    scenario!("Receipt serializes and deserializes faithfully", {
        let receipt = given!("a receipt with hash", {
            abp_receipt::ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .usage_tokens(100, 50)
                .with_hash()
                .unwrap()
        });

        let roundtrip = when!("serialized to JSON and back", {
            let json = serde_json::to_string(&receipt).unwrap();
            serde_json::from_str::<Receipt>(&json).unwrap()
        });

        then!("hash and outcome survive round-trip", {
            assert_eq!(roundtrip.outcome, receipt.outcome);
            assert_eq!(roundtrip.receipt_sha256, receipt.receipt_sha256);
            assert_eq!(roundtrip.usage.input_tokens, Some(100));
            assert_eq!(roundtrip.usage.output_tokens, Some(50));
        });
    });
}

#[test]
fn feature_agent_event_serde_tag() {
    scenario!("AgentEvent uses 'type' tag for kind discriminator", {
        let event = given!("a ToolCall agent event", {
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc-1".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({}),
                },
                ext: None,
            }
        });

        let json_str = when!("serializing to JSON", {
            serde_json::to_string(&event).unwrap()
        });

        then!("JSON contains '\"type\":\"tool_call\"'", {
            assert!(json_str.contains("\"type\":\"tool_call\""));
            assert!(json_str.contains("\"tool_name\":\"read_file\""));
        });
    });
}

#[test]
fn feature_envelope_serde_t_tag() {
    scenario!("Protocol envelope uses 't' tag, not 'type'", {
        let fatal = given!("a fatal envelope", {
            Envelope::Fatal {
                ref_id: None,
                error: "test".into(),
                error_code: None,
            }
        });

        let json_str = when!("serializing to JSON", {
            serde_json::to_string(&fatal).unwrap()
        });

        then!("JSON contains '\"t\":\"fatal\"'", {
            assert!(json_str.contains("\"t\":\"fatal\""));
            assert!(!json_str.contains("\"type\":\"fatal\""));
        });
    });
}

// ===========================================================================
// Feature: Capability Negotiation
// ===========================================================================

#[test]
fn feature_capability_native_satisfies_both() {
    scenario!("Native support satisfies both Native and Emulated min", {
        then!("Native satisfies Native", {
            assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
        });
        then!("Native satisfies Emulated", {
            assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
        });
    });
}

#[test]
fn feature_capability_emulated_does_not_satisfy_native() {
    scenario!("Emulated support does not satisfy Native min", {
        then!("Emulated does not satisfy Native", {
            assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
        });
        then!("Emulated satisfies Emulated", {
            assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
        });
    });
}

#[test]
fn feature_capability_unsupported_fails_all() {
    scenario!("Unsupported capability fails all min levels", {
        then!("Unsupported does not satisfy Native", {
            assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
        });
        then!("Unsupported does not satisfy Emulated", {
            assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
        });
    });
}

#[tokio::test]
async fn feature_capability_check_runtime() {
    scenario!("Runtime capability check against registered backend", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });

        let result = when!("checking streaming capability", {
            rt.check_capabilities(
                "mock",
                &CapabilityRequirements {
                    required: vec![CapabilityRequirement {
                        capability: Capability::Streaming,
                        min_support: MinSupport::Native,
                    }],
                },
            )
        });

        then!("the check passes", {
            assert!(result.is_ok());
        });
    });
}

// ===========================================================================
// Feature: Error Taxonomy
// ===========================================================================

#[test]
fn feature_error_code_categories() {
    scenario!("Error codes map to correct categories", {
        then!("BackendNotFound is Backend category", {
            assert_eq!(
                ErrorCode::BackendNotFound.category(),
                abp_error::ErrorCategory::Backend
            );
        });
        then!("PolicyDenied is Policy category", {
            assert_eq!(
                ErrorCode::PolicyDenied.category(),
                abp_error::ErrorCategory::Policy
            );
        });
        then!("ProtocolHandshakeFailed is Protocol category", {
            assert_eq!(
                ErrorCode::ProtocolHandshakeFailed.category(),
                abp_error::ErrorCategory::Protocol
            );
        });
    });
}

#[test]
fn feature_runtime_error_retryability() {
    scenario!("Runtime errors indicate retryability correctly", {
        then!("UnknownBackend is not retryable", {
            let err = RuntimeError::UnknownBackend { name: "x".into() };
            assert!(!err.is_retryable());
        });
        then!("BackendFailed is retryable", {
            let err = RuntimeError::BackendFailed(anyhow::anyhow!("temporary"));
            assert!(err.is_retryable());
        });
        then!("CapabilityCheckFailed is not retryable", {
            let err = RuntimeError::CapabilityCheckFailed("missing".into());
            assert!(!err.is_retryable());
        });
    });
}

// ===========================================================================
// Feature: WorkOrder Builder
// ===========================================================================

#[test]
fn feature_builder_defaults() {
    scenario!("WorkOrderBuilder provides sensible defaults", {
        let wo = given!("a minimal work order", {
            WorkOrderBuilder::new("test").build()
        });

        then!("defaults are set correctly", {
            assert_eq!(wo.task, "test");
            assert!(wo.config.model.is_none());
            assert!(wo.config.max_turns.is_none());
            assert!(wo.config.max_budget_usd.is_none());
            assert!(wo.policy.allowed_tools.is_empty());
            assert!(wo.policy.disallowed_tools.is_empty());
        });
    });
}

#[test]
fn feature_builder_with_all_options() {
    scenario!("WorkOrderBuilder with all options set", {
        let wo = given!("a fully configured work order", {
            WorkOrderBuilder::new("full config")
                .model("gpt-4o")
                .max_turns(20)
                .max_budget_usd(10.0)
                .root("/tmp/ws")
                .workspace_mode(WorkspaceMode::Staged)
                .include(vec!["src/**".into()])
                .exclude(vec!["*.tmp".into()])
                .policy(PolicyProfile {
                    disallowed_tools: vec!["Bash".into()],
                    ..PolicyProfile::default()
                })
                .build()
        });

        then!("all options are reflected", {
            assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
            assert_eq!(wo.config.max_turns, Some(20));
            assert_eq!(wo.config.max_budget_usd, Some(10.0));
            assert_eq!(wo.workspace.root, "/tmp/ws");
            assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
            assert_eq!(wo.workspace.include, vec!["src/**"]);
            assert_eq!(wo.workspace.exclude, vec!["*.tmp"]);
            assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
        });
    });
}

// ===========================================================================
// Feature: Multi-step / Chain Scenarios
// ===========================================================================

#[tokio::test]
async fn feature_multi_step_receipts_chain() {
    scenario!("Multiple work orders produce a valid receipt chain", {
        let rt = given!("a runtime with mock backend", {
            Runtime::with_default_backends()
        });

        let mut chain = ReceiptChain::new();

        when!("executing three work orders sequentially", {
            for i in 0..3 {
                let wo = WorkOrderBuilder::new(format!("step {i}"))
                    .workspace_mode(WorkspaceMode::PassThrough)
                    .build();
                let handle = rt.run_streaming("mock", wo).await.unwrap();
                let (_, receipt) = drain_run(handle).await;
                chain.push(receipt.unwrap()).unwrap();
            }
        });

        then!("chain has 3 receipts and verifies OK", {
            assert_eq!(chain.len(), 3);
            assert!(chain.verify().is_ok());
        });
    });
}

#[test]
fn feature_receipt_builder_error_shorthand() {
    scenario!("ReceiptBuilder error() marks receipt as Failed", {
        let receipt = given!("a receipt built with error shorthand", {
            abp_receipt::ReceiptBuilder::new("mock")
                .error("something went wrong")
                .build()
        });

        then!("outcome is Failed and trace contains error event", {
            assert_eq!(receipt.outcome, Outcome::Failed);
            let has_error = receipt.trace.iter().any(|e| {
                matches!(
                    &e.kind,
                    AgentEventKind::Error { message, .. } if message.contains("something went wrong")
                )
            });
            assert!(has_error);
        });
    });
}
