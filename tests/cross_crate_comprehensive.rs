// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive cross-crate integration tests for Agent Backplane.
//!
//! Tests the integration between abp-core, abp-protocol, abp-policy,
//! abp-glob, abp-dialect, and abp-mapping crates.

use std::io::BufReader;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionLane, ExecutionMode,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    CONTRACT_VERSION,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_mapping::{
    features, known_rules, validate_mapping, Fidelity, MappingMatrix, MappingRegistry, MappingRule,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════════
// 1. WorkOrder → Policy check → Backend dispatch flow
// ═══════════════════════════════════════════════════════════════════════════

mod work_order_policy_dispatch {
    use super::*;

    #[test]
    fn work_order_with_empty_policy_allows_all_tools() {
        let wo = WorkOrderBuilder::new("test task").build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
        assert!(engine.can_use_tool("Write").allowed);
    }

    #[test]
    fn work_order_with_restrictive_policy_denies_bash() {
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        };
        let wo = WorkOrderBuilder::new("test task").policy(policy).build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
    }

    #[test]
    fn work_order_with_allowlist_only_permits_listed_tools() {
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".into(), "Grep".into()],
            ..PolicyProfile::default()
        };
        let wo = WorkOrderBuilder::new("test task").policy(policy).build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(engine.can_use_tool("Read").allowed);
        assert!(engine.can_use_tool("Grep").allowed);
        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(!engine.can_use_tool("Write").allowed);
    }

    #[test]
    fn work_order_deny_write_paths_enforced() {
        let policy = PolicyProfile {
            deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
            ..PolicyProfile::default()
        };
        let wo = WorkOrderBuilder::new("test task").policy(policy).build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
        assert!(!engine.can_write_path(Path::new("node_modules/pkg/index.js")).allowed);
        assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn work_order_deny_read_paths_enforced() {
        let policy = PolicyProfile {
            deny_read: vec!["**/.env".into(), "**/.env.*".into()],
            ..PolicyProfile::default()
        };
        let wo = WorkOrderBuilder::new("test task").policy(policy).build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(!engine.can_read_path(Path::new(".env.production")).allowed);
        assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn work_order_complex_policy_combination() {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".into()],
            disallowed_tools: vec!["Bash*".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/locked/**".into()],
            ..PolicyProfile::default()
        };
        let wo = WorkOrderBuilder::new("test task").policy(policy).build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(!engine.can_use_tool("BashExec").allowed);
        assert!(engine.can_use_tool("Read").allowed);
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(!engine.can_write_path(Path::new("locked/data.txt")).allowed);
    }

    #[test]
    fn work_order_builder_sets_model_and_turns() {
        let wo = WorkOrderBuilder::new("hello")
            .model("gpt-4")
            .max_turns(10)
            .build();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
        assert_eq!(wo.config.max_turns, Some(10));
    }

    #[test]
    fn work_order_builder_sets_lane() {
        let wo = WorkOrderBuilder::new("hello")
            .lane(ExecutionLane::WorkspaceFirst)
            .build();
        assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    }

    #[test]
    fn work_order_builder_sets_workspace_mode() {
        let wo = WorkOrderBuilder::new("hello")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    }

    #[test]
    fn work_order_serde_roundtrip() {
        let wo = WorkOrderBuilder::new("test task")
            .model("claude-3")
            .max_turns(5)
            .build();
        let json = serde_json::to_string(&wo).unwrap();
        let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(wo2.task, "test task");
        assert_eq!(wo2.config.model.as_deref(), Some("claude-3"));
    }

    #[test]
    fn work_order_workspace_globs_match_policy_globs() {
        let wo = WorkOrderBuilder::new("test")
            .include(vec!["src/**".into()])
            .exclude(vec!["src/generated/**".into()])
            .build();
        let ws_globs =
            IncludeExcludeGlobs::new(&wo.workspace.include, &wo.workspace.exclude).unwrap();
        assert_eq!(ws_globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            ws_globs.decide_str("src/generated/out.rs"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            ws_globs.decide_str("README.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn work_order_policy_then_workspace_globs() {
        let policy = PolicyProfile {
            deny_write: vec!["**/*.lock".into()],
            ..PolicyProfile::default()
        };
        let wo = WorkOrderBuilder::new("test")
            .policy(policy)
            .include(vec!["src/**".into()])
            .build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
        let ws_globs =
            IncludeExcludeGlobs::new(&wo.workspace.include, &wo.workspace.exclude).unwrap();
        assert_eq!(
            ws_globs.decide_str("Cargo.lock"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn work_order_with_budget_constraint() {
        let wo = WorkOrderBuilder::new("expensive task")
            .max_budget_usd(10.0)
            .build();
        assert_eq!(wo.config.max_budget_usd, Some(10.0));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Dialect detection → IR translation → Mapping → Response
// ═══════════════════════════════════════════════════════════════════════════

mod dialect_detection_mapping {
    use super::*;

    #[test]
    fn detect_openai_dialect() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7
        });
        let result = detector.detect(&msg).unwrap();
        assert_eq!(result.dialect, Dialect::OpenAi);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn detect_claude_dialect() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "type": "message",
            "model": "claude-3",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
            "stop_reason": "end_turn"
        });
        let result = detector.detect(&msg).unwrap();
        assert_eq!(result.dialect, Dialect::Claude);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn detect_gemini_dialect() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "contents": [{"parts": [{"text": "hello"}]}],
            "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
        });
        let result = detector.detect(&msg).unwrap();
        assert_eq!(result.dialect, Dialect::Gemini);
    }

    #[test]
    fn detect_codex_dialect() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "items": [{"type": "message", "text": "hello"}],
            "status": "completed",
            "object": "response"
        });
        let result = detector.detect(&msg).unwrap();
        assert_eq!(result.dialect, Dialect::Codex);
    }

    #[test]
    fn detect_kimi_dialect() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "messages": [{"role": "user", "content": "hello"}],
            "refs": ["doc1"],
            "search_plus": true
        });
        let result = detector.detect(&msg).unwrap();
        assert_eq!(result.dialect, Dialect::Kimi);
    }

    #[test]
    fn detect_copilot_dialect() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "references": [{"type": "file", "path": "test.rs"}],
            "confirmations": [],
            "agent_mode": true
        });
        let result = detector.detect(&msg).unwrap();
        assert_eq!(result.dialect, Dialect::Copilot);
    }

    #[test]
    fn detect_all_returns_multiple_matches() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let results = detector.detect_all(&msg);
        assert!(!results.is_empty());
    }

    #[test]
    fn detect_returns_none_for_non_object() {
        let detector = DialectDetector::new();
        assert!(detector.detect(&serde_json::json!("string")).is_none());
        assert!(detector.detect(&serde_json::json!(42)).is_none());
        assert!(detector.detect(&serde_json::json!([])).is_none());
    }

    #[test]
    fn detection_then_mapping_validation() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7
        });
        let result = detector.detect(&msg).unwrap();
        let registry = known_rules();
        let validations = validate_mapping(
            &registry,
            result.dialect,
            Dialect::Claude,
            &["tool_use".into(), "streaming".into()],
        );
        assert_eq!(validations.len(), 2);
        // OpenAI → Claude tool_use is lossless
        assert!(validations[0].fidelity.is_lossless());
        // OpenAI → Claude streaming is lossless
        assert!(validations[1].fidelity.is_lossless());
    }

    #[test]
    fn detection_then_mapping_with_unsupported_feature() {
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7
        });
        let result = detector.detect(&msg).unwrap();
        let registry = known_rules();
        let validations = validate_mapping(
            &registry,
            result.dialect,
            Dialect::Codex,
            &["image_input".into()],
        );
        assert_eq!(validations.len(), 1);
        assert!(validations[0].fidelity.is_unsupported());
    }

    #[test]
    fn dialect_all_returns_six_dialects() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn dialect_labels_are_unique() {
        let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn dialect_display_matches_label() {
        for &d in Dialect::all() {
            assert_eq!(format!("{d}"), d.label());
        }
    }

    #[test]
    fn dialect_serde_roundtrip() {
        for &d in Dialect::all() {
            let json = serde_json::to_string(&d).unwrap();
            let d2: Dialect = serde_json::from_str(&json).unwrap();
            assert_eq!(d, d2);
        }
    }

    #[test]
    fn validator_rejects_non_object() {
        let validator = DialectValidator::new();
        let result = validator.validate(&serde_json::json!("not an object"), Dialect::OpenAi);
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Protocol envelope → Backend → Receipt → Hash
// ═══════════════════════════════════════════════════════════════════════════

mod protocol_receipt_hash {
    use super::*;

    #[test]
    fn hello_envelope_roundtrip() {
        let hello = Envelope::hello(
            BackendIdentity {
                id: "mock".into(),
                backend_version: Some("1.0".into()),
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let line = JsonlCodec::encode(&hello).unwrap();
        assert!(line.contains("\"t\":\"hello\""));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn run_envelope_with_work_order() {
        let wo = WorkOrderBuilder::new("do stuff").build();
        let env = Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.contains("\"t\":\"run\""));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Run { .. }));
    }

    #[test]
    fn event_envelope_roundtrip() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "run-1".into(),
            event,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.contains("\"t\":\"event\""));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Event { .. }));
    }

    #[test]
    fn final_envelope_with_receipt() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let env = Envelope::Final {
            ref_id: "run-1".into(),
            receipt,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.contains("\"t\":\"final\""));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Final { .. }));
    }

    #[test]
    fn fatal_envelope_roundtrip() {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "something broke".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.contains("\"t\":\"fatal\""));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Fatal { .. }));
    }

    #[test]
    fn receipt_hash_deterministic() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let h1 = abp_core::receipt_hash(&receipt).unwrap();
        let h2 = abp_core::receipt_hash(&receipt).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn receipt_with_hash_fills_sha256() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
        assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn receipt_hash_excludes_sha256_field() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let hash_before = abp_core::receipt_hash(&receipt).unwrap();
        let hashed_receipt = receipt.with_hash().unwrap();
        let hash_after = abp_core::receipt_hash(&hashed_receipt).unwrap();
        assert_eq!(hash_before, hash_after);
    }

    #[test]
    fn receipt_different_outcomes_different_hashes() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Failed)
            .build();
        let h1 = abp_core::receipt_hash(&r1).unwrap();
        let h2 = abp_core::receipt_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_different_backends_different_hashes() {
        let r1 = ReceiptBuilder::new("mock-a").build();
        let r2 = ReceiptBuilder::new("mock-b").build();
        let h1 = abp_core::receipt_hash(&r1).unwrap();
        let h2 = abp_core::receipt_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn envelope_encode_ends_with_newline() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn decode_stream_multiple_envelopes() {
        let input = format!(
            "{}\n{}\n",
            r#"{"t":"fatal","ref_id":null,"error":"a"}"#,
            r#"{"t":"fatal","ref_id":null,"error":"b"}"#,
        );
        let reader = BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let input = format!(
            "\n{}\n\n{}\n\n",
            r#"{"t":"fatal","ref_id":null,"error":"a"}"#,
            r#"{"t":"fatal","ref_id":null,"error":"b"}"#,
        );
        let reader = BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn receipt_builder_with_trace_events() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hi".into(),
            },
            ext: None,
        };
        let receipt = ReceiptBuilder::new("mock")
            .add_trace_event(event)
            .build();
        assert_eq!(receipt.trace.len(), 1);
    }

    #[test]
    fn receipt_serde_roundtrip() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        let json = serde_json::to_string(&receipt).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.receipt_sha256, receipt.receipt_sha256);
        assert_eq!(r2.outcome, receipt.outcome);
    }

    #[test]
    fn hello_envelope_with_mode() {
        let hello = Envelope::hello_with_mode(
            BackendIdentity {
                id: "sidecar".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        let line = JsonlCodec::encode(&hello).unwrap();
        assert!(line.contains("passthrough"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Full pipeline: build work order, compile policy, detect dialect,
//    map, generate receipt
// ═══════════════════════════════════════════════════════════════════════════

mod full_pipeline {
    use super::*;

    #[test]
    fn end_to_end_pipeline() {
        // 1. Build work order with policy
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        };
        let wo = WorkOrderBuilder::new("Refactor module")
            .policy(policy)
            .model("gpt-4")
            .build();

        // 2. Compile policy
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(engine.can_use_tool("Read").allowed);
        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(!engine.can_write_path(Path::new(".git/config")).allowed);

        // 3. Detect dialect
        let detector = DialectDetector::new();
        let msg = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Refactor module"}],
            "temperature": 0.7
        });
        let detection = detector.detect(&msg).unwrap();
        assert_eq!(detection.dialect, Dialect::OpenAi);

        // 4. Validate mapping
        let registry = known_rules();
        let validations = validate_mapping(
            &registry,
            detection.dialect,
            Dialect::Claude,
            &["tool_use".into(), "streaming".into()],
        );
        assert!(validations.iter().all(|v| v.errors.is_empty()));

        // 5. Generate receipt
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .work_order_id(wo.id)
            .build()
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
        assert_eq!(receipt.meta.work_order_id, wo.id);
    }

    #[test]
    fn pipeline_with_streaming_events() {
        let wo = WorkOrderBuilder::new("Generate code").build();

        // Simulate streaming events
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "Starting".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "fn ".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "main()".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "Done".into(),
                },
                ext: None,
            },
        ];

        // Wrap in envelopes
        let envelopes: Vec<Envelope> = events
            .iter()
            .map(|e| Envelope::Event {
                ref_id: wo.id.to_string(),
                event: e.clone(),
            })
            .collect();

        // Encode/decode roundtrip
        for env in &envelopes {
            let line = JsonlCodec::encode(env).unwrap();
            let decoded = JsonlCodec::decode(line.trim()).unwrap();
            assert!(matches!(decoded, Envelope::Event { .. }));
        }

        // Build receipt with trace
        let mut builder = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .work_order_id(wo.id);
        for e in events {
            builder = builder.add_trace_event(e);
        }
        let receipt = builder.build().with_hash().unwrap();
        assert_eq!(receipt.trace.len(), 4);
        assert!(receipt.receipt_sha256.is_some());
    }

    #[test]
    fn pipeline_tool_call_and_result() {
        let wo = WorkOrderBuilder::new("Run tests").build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();

        // Check tool call is allowed
        assert!(engine.can_use_tool("Read").allowed);

        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "Read".into(),
                    tool_use_id: Some("tc-1".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"path": "src/lib.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "Read".into(),
                    tool_use_id: Some("tc-1".into()),
                    output: serde_json::json!({"content": "fn main() {}"}),
                    is_error: false,
                },
                ext: None,
            },
        ];

        let mut builder = ReceiptBuilder::new("mock").work_order_id(wo.id);
        for e in events {
            builder = builder.add_trace_event(e);
        }
        let receipt = builder.build().with_hash().unwrap();
        assert_eq!(receipt.trace.len(), 2);
    }

    #[test]
    fn pipeline_with_file_changed_events() {
        let wo = WorkOrderBuilder::new("Edit files").build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);

        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "Added new function".into(),
            },
            ext: None,
        };
        let receipt = ReceiptBuilder::new("mock")
            .work_order_id(wo.id)
            .add_trace_event(event)
            .build();
        assert_eq!(receipt.trace.len(), 1);
    }

    #[test]
    fn pipeline_dialect_to_mapping_matrix() {
        let registry = known_rules();
        let matrix = MappingMatrix::from_registry(&registry);

        // OpenAI → Claude should be supported
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        // Claude → OpenAI should be supported
        assert!(matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
    }

    #[test]
    fn pipeline_complete_protocol_flow() {
        // 1. Hello
        let hello = Envelope::hello(
            BackendIdentity {
                id: "test-sidecar".into(),
                backend_version: Some("0.1.0".into()),
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );

        // 2. Run
        let wo = WorkOrderBuilder::new("hello world").build();
        let run = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo.clone(),
        };

        // 3. Events
        let event = Envelope::Event {
            ref_id: wo.id.to_string(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello!".into(),
                },
                ext: None,
            },
        };

        // 4. Final
        let receipt = ReceiptBuilder::new("test-sidecar")
            .work_order_id(wo.id)
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        let final_env = Envelope::Final {
            ref_id: wo.id.to_string(),
            receipt,
        };

        // Verify all encode/decode
        for env in [&hello, &run, &event, &final_env] {
            let line = JsonlCodec::encode(env).unwrap();
            assert!(line.ends_with('\n'));
            let decoded = JsonlCodec::decode(line.trim()).unwrap();
            let re_encoded = JsonlCodec::encode(&decoded).unwrap();
            // Both should parse
            JsonlCodec::decode(re_encoded.trim()).unwrap();
        }
    }

    #[test]
    fn pipeline_detection_validation_then_receipt() {
        let detector = DialectDetector::new();
        let validator = DialectValidator::new();

        let msg = serde_json::json!({
            "type": "message",
            "model": "claude-3",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });

        let detection = detector.detect(&msg).unwrap();
        assert_eq!(detection.dialect, Dialect::Claude);

        let validation = validator.validate(&msg, detection.dialect);
        // Validation should at least not crash
        let _ = validation.valid;

        let receipt = ReceiptBuilder::new("claude-backend")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Error propagation across crate boundaries
// ═══════════════════════════════════════════════════════════════════════════

mod error_propagation {
    use super::*;

    #[test]
    fn invalid_glob_in_policy_returns_error() {
        let policy = PolicyProfile {
            disallowed_tools: vec!["[invalid".into()],
            ..PolicyProfile::default()
        };
        let result = PolicyEngine::new(&policy);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_glob_in_deny_read_returns_error() {
        let policy = PolicyProfile {
            deny_read: vec!["[".into()],
            ..PolicyProfile::default()
        };
        let result = PolicyEngine::new(&policy);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_glob_in_deny_write_returns_error() {
        let policy = PolicyProfile {
            deny_write: vec!["[".into()],
            ..PolicyProfile::default()
        };
        let result = PolicyEngine::new(&policy);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_json_decode_returns_protocol_error() {
        let result = JsonlCodec::decode("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn wrong_envelope_type_decode_returns_error() {
        let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":"x"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn mapping_error_for_unsupported_feature() {
        let registry = known_rules();
        let validations = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Codex,
            &["image_input".into()],
        );
        assert_eq!(validations.len(), 1);
        assert!(validations[0].fidelity.is_unsupported());
        assert!(!validations[0].errors.is_empty());
    }

    #[test]
    fn mapping_error_for_empty_feature_name() {
        let registry = known_rules();
        let validations = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Claude,
            &[String::new()],
        );
        assert_eq!(validations.len(), 1);
        assert!(validations[0].fidelity.is_unsupported());
        assert!(!validations[0].errors.is_empty());
    }

    #[test]
    fn mapping_error_for_unknown_feature() {
        let registry = known_rules();
        let validations = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Claude,
            &["nonexistent_feature".into()],
        );
        assert_eq!(validations.len(), 1);
        assert!(validations[0].fidelity.is_unsupported());
    }

    #[test]
    fn glob_invalid_pattern_error() {
        let result = IncludeExcludeGlobs::new(&["[".into()], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn glob_invalid_exclude_pattern_error() {
        let result = IncludeExcludeGlobs::new(&[], &["[".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn fatal_envelope_error_propagation() {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "backend crashed".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        if let Envelope::Fatal { error, .. } = JsonlCodec::decode(line.trim()).unwrap() {
            assert_eq!(error, "backend crashed");
        } else {
            panic!("expected Fatal envelope");
        }
    }

    #[test]
    fn mapping_error_display() {
        let err = abp_mapping::MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("Claude"));
        assert!(msg.contains("Gemini"));
    }

    #[test]
    fn mapping_error_dialect_mismatch() {
        let err = abp_mapping::MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        assert!(err.to_string().contains("dialect mismatch"));
    }

    #[test]
    fn mapping_error_invalid_input() {
        let err = abp_mapping::MappingError::InvalidInput {
            reason: "bad data".into(),
        };
        assert!(err.to_string().contains("invalid input"));
    }

    #[test]
    fn agent_event_error_kind() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something failed".into(),
                error_code: None,
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("something failed"));
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded.kind, AgentEventKind::Error { .. }));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Contract version consistency across crates
// ═══════════════════════════════════════════════════════════════════════════

mod contract_version {
    use super::*;

    #[test]
    fn contract_version_is_abp_v01() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn receipt_contains_contract_version() {
        let receipt = ReceiptBuilder::new("mock").build();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn hello_envelope_contains_contract_version() {
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
            panic!("expected Hello envelope");
        }
    }

    #[test]
    fn protocol_version_parsing() {
        assert_eq!(
            abp_protocol::parse_version(CONTRACT_VERSION),
            Some((0, 1))
        );
    }

    #[test]
    fn protocol_version_compatibility_same_major() {
        assert!(abp_protocol::is_compatible_version(
            CONTRACT_VERSION,
            "abp/v0.2"
        ));
    }

    #[test]
    fn protocol_version_incompatibility_different_major() {
        assert!(!abp_protocol::is_compatible_version(
            CONTRACT_VERSION,
            "abp/v1.0"
        ));
    }

    #[test]
    fn protocol_version_parse_invalid() {
        assert_eq!(abp_protocol::parse_version("invalid"), None);
        assert_eq!(abp_protocol::parse_version(""), None);
        assert_eq!(abp_protocol::parse_version("v0.1"), None);
    }

    #[test]
    fn receipt_contract_version_in_serialized_form() {
        let receipt = ReceiptBuilder::new("mock").build();
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(json.contains(CONTRACT_VERSION));
    }

    #[test]
    fn hello_envelope_serialized_contains_version() {
        let hello = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let line = JsonlCodec::encode(&hello).unwrap();
        assert!(line.contains(CONTRACT_VERSION));
    }

    #[test]
    fn contract_version_format() {
        assert!(CONTRACT_VERSION.starts_with("abp/v"));
        let (major, minor) = abp_protocol::parse_version(CONTRACT_VERSION).unwrap();
        assert_eq!(major, 0);
        assert_eq!(minor, 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional cross-crate integration tests
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_registry_integration {
    use super::*;

    #[test]
    fn known_rules_is_not_empty() {
        let registry = known_rules();
        assert!(!registry.is_empty());
    }

    #[test]
    fn same_dialect_mapping_is_lossless() {
        let registry = known_rules();
        for &d in Dialect::all() {
            let rule = registry.lookup(d, d, features::TOOL_USE);
            assert!(rule.is_some(), "missing self-mapping for {d}");
            assert!(rule.unwrap().fidelity.is_lossless());
        }
    }

    #[test]
    fn same_dialect_streaming_is_lossless() {
        let registry = known_rules();
        for &d in Dialect::all() {
            let rule = registry.lookup(d, d, features::STREAMING);
            assert!(rule.is_some());
            assert!(rule.unwrap().fidelity.is_lossless());
        }
    }

    #[test]
    fn openai_to_claude_tool_use_lossless() {
        let registry = known_rules();
        let rule = registry
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn openai_to_codex_image_input_unsupported() {
        let registry = known_rules();
        let rule = registry
            .lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }

    #[test]
    fn mapping_matrix_from_registry() {
        let registry = known_rules();
        let matrix = MappingMatrix::from_registry(&registry);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(matrix.is_supported(Dialect::Claude, Dialect::Gemini));
    }

    #[test]
    fn mapping_matrix_manual_set_and_get() {
        let mut matrix = MappingMatrix::new();
        matrix.set(Dialect::OpenAi, Dialect::Claude, true);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(!matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
        assert_eq!(matrix.get(Dialect::OpenAi, Dialect::Claude), Some(true));
        assert_eq!(matrix.get(Dialect::Claude, Dialect::OpenAi), None);
    }

    #[test]
    fn registry_insert_replaces_existing() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "test".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 1);

        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "test".into(),
            fidelity: Fidelity::Unsupported {
                reason: "changed".into(),
            },
        });
        assert_eq!(reg.len(), 1);
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "test")
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }

    #[test]
    fn registry_lookup_returns_none_for_missing() {
        let reg = MappingRegistry::new();
        assert!(reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "nonexistent")
            .is_none());
    }

    #[test]
    fn rank_targets_from_openai() {
        let registry = known_rules();
        let ranked = registry.rank_targets(
            Dialect::OpenAi,
            &[features::TOOL_USE, features::STREAMING],
        );
        assert!(!ranked.is_empty());
        // First result should have the most lossless features
        assert!(ranked[0].1 >= ranked.last().unwrap().1);
    }

    #[test]
    fn fidelity_predicates() {
        assert!(Fidelity::Lossless.is_lossless());
        assert!(!Fidelity::Lossless.is_unsupported());
        assert!(!Fidelity::LossyLabeled {
            warning: "w".into()
        }
        .is_lossless());
        assert!(!Fidelity::LossyLabeled {
            warning: "w".into()
        }
        .is_unsupported());
        assert!(!Fidelity::Unsupported {
            reason: "r".into()
        }
        .is_lossless());
        assert!(Fidelity::Unsupported {
            reason: "r".into()
        }
        .is_unsupported());
    }

    #[test]
    fn fidelity_serde_roundtrip() {
        let lossless = Fidelity::Lossless;
        let json = serde_json::to_string(&lossless).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(lossless, f2);

        let lossy = Fidelity::LossyLabeled {
            warning: "test".into(),
        };
        let json = serde_json::to_string(&lossy).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(lossy, f2);
    }

    #[test]
    fn mapping_rule_serde_roundtrip() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let r2: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, r2);
    }
}

mod glob_policy_integration {
    use super::*;

    #[test]
    fn glob_decides_policy_paths_consistently() {
        let policy = PolicyProfile {
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let globs = IncludeExcludeGlobs::new(&[], &["**/.git/**".into()]).unwrap();

        let test_paths = &[".git/config", ".git/HEAD", "src/lib.rs"];
        for &p in test_paths {
            let policy_allows = engine.can_write_path(Path::new(p)).allowed;
            let glob_allows = globs.decide_str(p).is_allowed();
            assert_eq!(
                policy_allows, glob_allows,
                "mismatch for path: {p}"
            );
        }
    }

    #[test]
    fn workspace_globs_and_policy_globs_independent() {
        let wo = WorkOrderBuilder::new("test")
            .include(vec!["src/**".into()])
            .policy(PolicyProfile {
                deny_write: vec!["src/secret/**".into()],
                ..PolicyProfile::default()
            })
            .build();

        let ws_globs =
            IncludeExcludeGlobs::new(&wo.workspace.include, &wo.workspace.exclude).unwrap();
        let engine = PolicyEngine::new(&wo.policy).unwrap();

        // In workspace scope
        assert_eq!(
            ws_globs.decide_str("src/lib.rs"),
            MatchDecision::Allowed
        );
        // Policy allows writes
        assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);

        // In workspace scope but policy denies writes
        assert_eq!(
            ws_globs.decide_str("src/secret/key.pem"),
            MatchDecision::Allowed
        );
        assert!(!engine
            .can_write_path(Path::new("src/secret/key.pem"))
            .allowed);
    }

    #[test]
    fn match_decision_variants() {
        assert!(MatchDecision::Allowed.is_allowed());
        assert!(!MatchDecision::DeniedByExclude.is_allowed());
        assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
    }
}

mod agent_event_serde {
    use super::*;

    #[test]
    fn run_started_event_serde() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("run_started"));
        let _: AgentEvent = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn tool_call_event_serde() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "file.txt"}),
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("tool_call"));
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded.kind, AgentEventKind::ToolCall { .. }));
    }

    #[test]
    fn warning_event_serde() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "careful".into(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("warning"));
        let _: AgentEvent = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn command_executed_event_serde() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("all tests passed".into()),
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("command_executed"));
        let _: AgentEvent = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn event_with_ext_field() {
        let mut ext = std::collections::BTreeMap::new();
        ext.insert(
            "raw_message".to_string(),
            serde_json::json!({"original": true}),
        );
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "test".into(),
            },
            ext: Some(ext),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("raw_message"));
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        assert!(decoded.ext.is_some());
    }
}

mod canonical_json_and_hashing {
    use super::*;

    #[test]
    fn canonical_json_sorts_keys() {
        let json = abp_core::canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
        assert!(json.starts_with(r#"{"a":1"#));
    }

    #[test]
    fn sha256_hex_length() {
        let hex = abp_core::sha256_hex(b"hello");
        assert_eq!(hex.len(), 64);
    }

    #[test]
    fn sha256_hex_deterministic() {
        let h1 = abp_core::sha256_hex(b"test");
        let h2 = abp_core::sha256_hex(b"test");
        assert_eq!(h1, h2);
    }

    #[test]
    fn sha256_hex_different_inputs() {
        let h1 = abp_core::sha256_hex(b"a");
        let h2 = abp_core::sha256_hex(b"b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_hash_with_trace_events() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        };
        let receipt = ReceiptBuilder::new("mock")
            .add_trace_event(event)
            .build();
        let hash = abp_core::receipt_hash(&receipt).unwrap();
        assert_eq!(hash.len(), 64);
    }
}

mod execution_mode_integration {
    use super::*;

    #[test]
    fn default_execution_mode_is_mapped() {
        let mode = ExecutionMode::default();
        assert_eq!(mode, ExecutionMode::Mapped);
    }

    #[test]
    fn execution_mode_serde_roundtrip() {
        for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
            let json = serde_json::to_string(&mode).unwrap();
            let m2: ExecutionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, m2);
        }
    }

    #[test]
    fn receipt_builder_with_mode() {
        let receipt = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn hello_envelope_default_mode_is_mapped() {
        let hello = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        if let Envelope::Hello { mode, .. } = hello {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
    }
}
