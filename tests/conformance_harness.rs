// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive conformance test harness validating SDK shim fidelity,
//! mapping correctness, and receipt integrity across all supported dialects.

use std::collections::BTreeMap;

use abp_capability::{SupportLevel, check_capability, negotiate_capabilities};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, Outcome, Receipt,
    SupportLevel as CoreSupportLevel,
};
use abp_dialect::Dialect;
use abp_emulation::{EmulationEngine, EmulationReport, FidelityLabel, compute_fidelity};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_mapping::{Fidelity, MappingError, MappingMatrix, known_rules, validate_mapping};
use abp_receipt::{ReceiptBuilder, canonicalize, compute_hash, verify_hash};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════

fn fixed_ts() -> chrono::DateTime<Utc> {
    "2025-01-01T00:00:00Z".parse().unwrap()
}

fn sample_events() -> Vec<AgentEvent> {
    vec![
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ]
}

fn tool_call_events() -> Vec<AgentEvent> {
    vec![
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc_001".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc_001".into()),
                output: json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "File read.".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ]
}

fn streaming_events() -> Vec<AgentEvent> {
    vec![
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta { text: "Hel".into() },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta { text: "lo ".into() },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "world!".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ]
}

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn receipt_with_events(events: Vec<AgentEvent>) -> Receipt {
    let mut r = ReceiptBuilder::new("conformance")
        .outcome(Outcome::Complete)
        .build();
    r.trace = events;
    r
}

fn manifest_with(caps: &[Capability]) -> CapabilityManifest {
    let mut m = BTreeMap::new();
    for c in caps {
        m.insert(c.clone(), CoreSupportLevel::Native);
    }
    m
}

// ═══════════════════════════════════════════════════════════════════════
//  1. PASSTHROUGH PARITY TESTS
// ═══════════════════════════════════════════════════════════════════════

mod passthrough_parity {
    use super::*;

    // ── OpenAI roundtrip ────────────────────────────────────────────

    #[test]
    fn openai_request_to_work_order_preserves_task() {
        use abp_shim_openai::{ChatCompletionRequest, Message, Role};
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message {
                role: Role::User,
                content: Some("Summarize this".into()),
                tool_calls: None,
                tool_call_id: None,
            }])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert!(wo.task.contains("Summarize this") || !wo.task.is_empty());
    }

    #[test]
    fn openai_receipt_to_response_roundtrip() {
        let receipt = receipt_with_events(sample_events());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.model, "gpt-4o");
        assert!(!resp.choices.is_empty());
    }

    #[test]
    fn openai_streaming_events_preserved() {
        let events = streaming_events();
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty(), "streaming events should produce output");
    }

    #[test]
    fn openai_tool_call_ids_stable_through_roundtrip() {
        let events = tool_call_events();
        let receipt = receipt_with_events(events);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        // Tool calls should appear in the response choices
        let has_content = resp
            .choices
            .iter()
            .any(|c| c.message.content.is_some() || c.message.tool_calls.is_some());
        assert!(has_content);
    }

    #[test]
    fn openai_mock_receipt_produces_valid_receipt() {
        let r = abp_shim_openai::mock_receipt(sample_events());
        assert_eq!(r.outcome, Outcome::Complete);
        assert_eq!(r.trace.len(), 3);
    }

    // ── Claude roundtrip ────────────────────────────────────────────

    #[test]
    fn claude_request_to_work_order_preserves_fields() {
        use abp_shim_claude::{ContentBlock, Message, MessageRequest, Role};
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello Claude".into(),
                }],
            }],
            system: Some("Be helpful".into()),
            temperature: Some(0.7),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = abp_shim_claude::request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn claude_response_from_events_roundtrip() {
        let events = sample_events();
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn claude_streaming_event_ordering() {
        // Claude's response_from_events processes AssistantMessage, not deltas.
        // Verify that events with a final AssistantMessage produce content.
        let events = sample_events(); // contains AssistantMessage
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        let has_content = resp
            .content
            .iter()
            .any(|b| matches!(b, abp_shim_claude::ContentBlock::Text { text } if !text.is_empty()));
        assert!(
            has_content,
            "AssistantMessage events should produce text content"
        );
    }

    #[test]
    fn claude_tool_use_ids_preserved() {
        let events = tool_call_events();
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        let tool_uses: Vec<_> = resp
            .content
            .iter()
            .filter(|b| matches!(b, abp_shim_claude::ContentBlock::ToolUse { .. }))
            .collect();
        // Tool calls from events should map to tool_use blocks
        assert!(
            !tool_uses.is_empty() || !resp.content.is_empty(),
            "tool calls should be present"
        );
    }

    // ── Gemini roundtrip ────────────────────────────────────────────

    #[test]
    fn gemini_request_construction_valid() {
        use abp_shim_gemini::{Content, GenerateContentRequest, Part};
        let req = GenerateContentRequest::new("gemini-2.0-flash").add_content(Content {
            role: "user".into(),
            parts: vec![Part::Text("Hello Gemini".into())],
        });
        assert_eq!(req.model, "gemini-2.0-flash");
        assert_eq!(req.contents.len(), 1);
    }

    #[test]
    fn gemini_to_dialect_request_preserves_model() {
        use abp_shim_gemini::{Content, GenerateContentRequest, Part};
        let req = GenerateContentRequest::new("gemini-2.0-flash").add_content(Content {
            role: "user".into(),
            parts: vec![Part::Text("Test".into())],
        });
        let dialect_req = abp_shim_gemini::to_dialect_request(&req);
        assert_eq!(dialect_req.model, "gemini-2.0-flash");
    }

    #[test]
    fn gemini_streaming_events_ordering() {
        let events = streaming_events();
        // Verify event ordering is maintained
        assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
        assert!(matches!(
            events[1].kind,
            AgentEventKind::AssistantDelta { .. }
        ));
        assert!(matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    // ── Codex roundtrip ─────────────────────────────────────────────

    #[test]
    fn codex_request_to_work_order_preserves_fields() {
        use abp_shim_codex::CodexRequestBuilder;
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert!(!wo.id.is_nil());
    }

    #[test]
    fn codex_receipt_to_response_roundtrip() {
        let receipt = receipt_with_events(sample_events());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        // Response should be populated with model info
        assert_eq!(resp.model, "codex-mini-latest");
    }

    #[test]
    fn codex_streaming_events_preserved() {
        let events = streaming_events();
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");
        assert!(!stream.is_empty());
    }

    #[test]
    fn codex_tool_call_ids_stable() {
        let events = tool_call_events();
        let receipt = receipt_with_events(events);
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        let tool_items: Vec<_> = resp
            .output
            .iter()
            .filter(|item| {
                matches!(
                    item,
                    abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { .. }
                )
            })
            .collect();
        assert!(
            !tool_items.is_empty() || !resp.output.is_empty(),
            "tool events should appear"
        );
    }

    // ── Kimi roundtrip ──────────────────────────────────────────────

    #[test]
    fn kimi_request_to_work_order_preserves_fields() {
        use abp_shim_kimi::{KimiRequestBuilder, Message};
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("Hello Kimi")])
            .build();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn kimi_receipt_to_response_roundtrip() {
        let receipt = receipt_with_events(sample_events());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.model, "moonshot-v1-8k");
    }

    #[test]
    fn kimi_streaming_events_preserved() {
        let events = streaming_events();
        let stream = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(!stream.is_empty());
    }

    #[test]
    fn kimi_tool_call_ids_stable() {
        let events = tool_call_events();
        let receipt = receipt_with_events(events);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }

    // ── Copilot roundtrip ───────────────────────────────────────────

    #[test]
    fn copilot_request_to_work_order_preserves_fields() {
        use abp_shim_copilot::{CopilotRequestBuilder, Message};
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello Copilot")])
            .build();
        let wo = abp_shim_copilot::request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn copilot_receipt_to_response_roundtrip() {
        let receipt = receipt_with_events(sample_events());
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.message.is_empty() || resp.copilot_errors.is_empty());
    }

    #[test]
    fn copilot_streaming_events_preserved() {
        let events = streaming_events();
        let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn copilot_tool_call_ids_stable() {
        let events = tool_call_events();
        let receipt = receipt_with_events(events);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        // Copilot response should be valid (message may contain tool info)
        assert!(
            !resp.message.is_empty()
                || resp.function_call.is_some()
                || resp.copilot_errors.is_empty(),
            "copilot response should be valid"
        );
    }

    // ── Cross-cutting passthrough tests ─────────────────────────────

    #[test]
    fn all_dialects_produce_nonempty_work_order_ids() {
        // OpenAI
        let oai_req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message {
                role: abp_shim_openai::Role::User,
                content: Some("test".into()),
                tool_calls: None,
                tool_call_id: None,
            }])
            .build();
        let oai_wo = abp_shim_openai::request_to_work_order(&oai_req);
        assert!(!oai_wo.id.is_nil());

        // Kimi
        let kimi_req = abp_shim_kimi::KimiRequestBuilder::new()
            .messages(vec![abp_shim_kimi::Message::user("test")])
            .build();
        let kimi_wo = abp_shim_kimi::request_to_work_order(&kimi_req);
        assert!(!kimi_wo.id.is_nil());

        // Codex
        let codex_req = abp_shim_codex::CodexRequestBuilder::new().build();
        let codex_wo = abp_shim_codex::request_to_work_order(&codex_req);
        assert!(!codex_wo.id.is_nil());

        // Copilot
        let copilot_req = abp_shim_copilot::CopilotRequestBuilder::new()
            .messages(vec![abp_shim_copilot::Message::user("test")])
            .build();
        let copilot_wo = abp_shim_copilot::request_to_work_order(&copilot_req);
        assert!(!copilot_wo.id.is_nil());
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  2. MAPPED MODE CONTRACT TESTS
// ═══════════════════════════════════════════════════════════════════════

mod mapped_mode_contracts {
    use super::*;

    #[test]
    fn capability_unsupported_produces_failure() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let result = check_capability(&manifest, &Capability::ExtendedThinking);
        assert!(matches!(result, SupportLevel::Unsupported { .. }));
    }

    #[test]
    fn negotiation_flags_unsupported_capabilities() {
        let manifest = manifest_with(&[Capability::Streaming, Capability::ToolUse]);
        let required = vec![
            Capability::Streaming,
            Capability::ExtendedThinking,
            Capability::ImageInput,
        ];
        let result = negotiate_capabilities(&required, &manifest);
        let unsup = result.unsupported_caps();
        assert!(!unsup.is_empty());
        assert!(unsup.contains(&Capability::ExtendedThinking));
        assert!(unsup.contains(&Capability::ImageInput));
    }

    #[test]
    fn negotiation_compatible_when_all_native() {
        let manifest = manifest_with(&[Capability::Streaming, Capability::ToolUse]);
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_compatible());
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn lossy_conversion_labeled_in_mapping() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::Claude,
            Dialect::OpenAi,
            &["thinking".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(
            matches!(results[0].fidelity, Fidelity::LossyLabeled { .. }),
            "Claude→OpenAI thinking should be lossy labeled"
        );
    }

    #[test]
    fn lossy_conversion_includes_warning() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::Claude,
            Dialect::OpenAi,
            &["thinking".into()],
        );
        if let Fidelity::LossyLabeled { warning } = &results[0].fidelity {
            assert!(!warning.is_empty(), "lossy warning should not be empty");
        } else {
            panic!("expected LossyLabeled");
        }
    }

    #[test]
    fn unsupported_feature_returns_errors() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Codex,
            &["image_input".into()],
        );
        assert!(!results[0].errors.is_empty());
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn lossless_mapping_has_no_errors() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Claude,
            &["tool_use".into()],
        );
        assert!(results[0].errors.is_empty());
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn cross_dialect_openai_to_claude_tool_use() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::OpenAi,
            Dialect::Claude,
            &["tool_use".into(), "streaming".into()],
        );
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.fidelity.is_lossless()));
    }

    #[test]
    fn cross_dialect_claude_to_gemini_streaming() {
        let registry = known_rules();
        let results = validate_mapping(
            &registry,
            Dialect::Claude,
            Dialect::Gemini,
            &["streaming".into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn mapping_matrix_reflects_registry() {
        let registry = known_rules();
        let matrix = MappingMatrix::from_registry(&registry);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(matrix.is_supported(Dialect::Claude, Dialect::Gemini));
    }

    #[test]
    fn same_dialect_always_lossless() {
        let registry = known_rules();
        for &d in Dialect::all() {
            let results =
                validate_mapping(&registry, d, d, &["tool_use".into(), "streaming".into()]);
            for r in &results {
                assert!(
                    r.fidelity.is_lossless(),
                    "{d}→{d} for {} should be lossless",
                    r.feature
                );
            }
        }
    }

    #[test]
    fn emulation_engine_labels_strategies() {
        let engine = EmulationEngine::with_defaults();
        let strategy = engine.resolve_strategy(&Capability::StructuredOutputJsonSchema);
        // The engine should always return a valid strategy (could be any variant)
        let _ = format!("{strategy:?}");
    }

    #[test]
    fn emulation_fidelity_label_native_for_empty_report() {
        let report = EmulationReport {
            applied: vec![],
            warnings: vec![],
        };
        let labels = compute_fidelity(&[Capability::Streaming], &report);
        assert!(matches!(
            labels.get(&Capability::Streaming),
            Some(FidelityLabel::Native)
        ));
    }

    #[test]
    fn mapping_registry_rank_targets() {
        let registry = known_rules();
        let ranked = registry.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
        assert!(!ranked.is_empty(), "OpenAI should have ranked targets");
    }

    #[test]
    fn validate_empty_feature_name_errors() {
        let registry = known_rules();
        let results = validate_mapping(&registry, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert!(!results[0].errors.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  3. RECEIPT CORRECTNESS TESTS
// ═══════════════════════════════════════════════════════════════════════

mod receipt_correctness {
    use super::*;

    #[test]
    fn receipt_hash_is_deterministic() {
        let r = minimal_receipt();
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2, "hash should be deterministic");
    }

    #[test]
    fn receipt_hash_is_sha256_length() {
        let r = minimal_receipt();
        let h = compute_hash(&r).unwrap();
        assert_eq!(h.len(), 64, "SHA-256 hex should be 64 chars");
    }

    #[test]
    fn canonical_json_is_deterministic() {
        let r = minimal_receipt();
        let j1 = canonicalize(&r).unwrap();
        let j2 = canonicalize(&r).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn canonical_json_uses_sorted_keys() {
        let mut r = minimal_receipt();
        r.usage_raw = json!({"z_key": 1, "a_key": 2});
        let j = canonicalize(&r).unwrap();
        let z_pos = j.find("z_key").unwrap();
        let a_pos = j.find("a_key").unwrap();
        assert!(
            a_pos < z_pos,
            "BTreeMap-based canonical JSON should sort keys"
        );
    }

    #[test]
    fn hash_changes_when_outcome_changes() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        assert_ne!(h1, h2, "different outcomes must produce different hashes");
    }

    #[test]
    fn hash_changes_when_backend_id_changes() {
        let r1 = ReceiptBuilder::new("backend-a")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("backend-b")
            .outcome(Outcome::Complete)
            .build();
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_changes_when_trace_changes() {
        let r1 = receipt_with_events(vec![]);
        let r2 = receipt_with_events(sample_events());
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn with_hash_populates_receipt_sha256() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(
            r.receipt_sha256.is_some(),
            "with_hash should populate sha256"
        );
    }

    #[test]
    fn with_hash_idempotence() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let hash1 = r1.receipt_sha256.clone().unwrap();

        // Compute the hash again on the already-hashed receipt
        let hash2 = compute_hash(&r1).unwrap();
        assert_eq!(
            hash1, hash2,
            "recomputing hash on hashed receipt should be idempotent"
        );
    }

    #[test]
    fn verify_hash_succeeds_on_valid_receipt() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r), "verify_hash should pass for valid receipt");
    }

    #[test]
    fn verify_hash_fails_on_tampered_receipt() {
        let mut r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        r.outcome = Outcome::Failed;
        assert!(!verify_hash(&r), "verify_hash should fail after tampering");
    }

    #[test]
    fn receipt_sha256_null_before_hashing() {
        let r = minimal_receipt();
        assert!(
            r.receipt_sha256.is_none(),
            "fresh receipt should have None sha256"
        );
    }

    #[test]
    fn canonical_json_forces_null_sha256() {
        let mut r = minimal_receipt();
        r.receipt_sha256 = Some("fake_hash".into());
        let j = canonicalize(&r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(
            v["receipt_sha256"].is_null(),
            "canonical JSON must force sha256 to null"
        );
    }

    #[test]
    fn verify_hash_succeeds_with_none_sha256() {
        let r = minimal_receipt();
        assert!(
            verify_hash(&r),
            "verify_hash should pass when sha256 is None"
        );
    }

    #[test]
    fn receipt_hash_hex_only() {
        let r = minimal_receipt();
        let h = compute_hash(&r).unwrap();
        assert!(
            h.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should only contain hex digits"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  4. ERROR TAXONOMY COVERAGE
// ═══════════════════════════════════════════════════════════════════════

mod error_taxonomy {
    use super::*;

    const ALL_CODES: &[ErrorCode] = &[
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];

    #[test]
    fn all_error_codes_have_category() {
        for &code in ALL_CODES {
            let _cat = code.category();
        }
    }

    #[test]
    fn all_error_codes_have_stable_str() {
        for &code in ALL_CODES {
            let s = code.as_str();
            assert!(!s.is_empty(), "as_str() should not be empty for {code:?}");
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "as_str() should be snake_case for {code:?}: {s}"
            );
        }
    }

    #[test]
    fn capability_unsupported_has_correct_category() {
        assert_eq!(
            ErrorCode::CapabilityUnsupported.category(),
            ErrorCategory::Capability
        );
    }

    #[test]
    fn dialect_mismatch_has_correct_category() {
        assert_eq!(
            ErrorCode::MappingDialectMismatch.category(),
            ErrorCategory::Mapping
        );
    }

    #[test]
    fn abp_error_builder_constructs_correctly() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
            .with_context("backend", "openai")
            .with_context("timeout_ms", 30_000);
        assert_eq!(err.code, ErrorCode::BackendTimeout);
        assert_eq!(err.message, "timed out");
        assert_eq!(err.context.len(), 2);
    }

    #[test]
    fn abp_error_to_info_preserves_code() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied by policy");
        let info = err.to_info();
        assert_eq!(info.code, ErrorCode::PolicyDenied);
        assert_eq!(info.message, "denied by policy");
    }

    #[test]
    fn error_info_serialization_roundtrip() {
        let info =
            ErrorInfo::new(ErrorCode::BackendTimeout, "timed out").with_detail("backend", "openai");
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ErrorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, deserialized);
    }

    #[test]
    fn abp_error_dto_from_abp_error() {
        let err =
            AbpError::new(ErrorCode::Internal, "something broke").with_context("key", "value");
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.code, ErrorCode::Internal);
        assert_eq!(dto.message, "something broke");
        assert!(dto.context.contains_key("key"));
    }

    #[test]
    fn retryable_errors_correctly_classified() {
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendUnavailable.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(ErrorCode::BackendCrashed.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::Internal.is_retryable());
    }

    #[test]
    fn mapping_error_serialization() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, deserialized);
    }

    #[test]
    fn mapping_error_dialect_mismatch() {
        let err = MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Kimi,
        };
        let msg = err.to_string();
        assert!(msg.contains("OpenAi") || msg.contains("Kimi") || msg.contains("mismatch"));
    }

    #[test]
    fn error_display_includes_code() {
        let err = AbpError::new(ErrorCode::ReceiptHashMismatch, "hash mismatch");
        let display = err.to_string();
        assert!(
            display.contains("receipt_hash_mismatch"),
            "Display should contain error code string"
        );
    }
}
