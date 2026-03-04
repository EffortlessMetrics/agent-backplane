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
//! Comprehensive end-to-end tests for the JSONL protocol codec layer.
//!
//! Covers: Envelope serde, JsonlCodec, StreamingCodec, StreamParser,
//! batch processing, sequence validation, routing, version negotiation,
//! error handling, and edge cases.

use std::collections::BTreeMap;
use std::io::{BufReader, Write};

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, ReceiptBuilder, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_protocol::batch::{
    BatchItemStatus, BatchProcessor, BatchRequest, BatchValidationError, MAX_BATCH_SIZE,
};
use abp_protocol::builder::EnvelopeBuilder;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::router::{MessageRoute, MessageRouter, RouteTable};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
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
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_run() -> (String, Envelope) {
    let wo = make_work_order("do something");
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    (id, env)
}

fn make_event_envelope(ref_id: &str, msg: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: make_event(AgentEventKind::AssistantMessage {
            text: msg.to_string(),
        }),
    }
}

fn make_final_envelope(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: make_receipt("test-sidecar"),
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn make_full_sequence() -> Vec<Envelope> {
    let (id, run) = make_run();
    vec![
        make_hello(),
        run,
        make_event_envelope(&id, "hello world"),
        make_final_envelope(&id),
    ]
}

// ===========================================================================
// 1. Envelope serialization/deserialization
// ===========================================================================

mod envelope_serde {
    use super::*;

    #[test]
    fn hello_roundtrip() {
        let env = make_hello();
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn hello_contains_tag_t() {
        let env = make_hello();
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"hello""#));
        assert!(!json.contains(r#""type":"hello""#));
    }

    #[test]
    fn hello_contract_version_matches() {
        let env = make_hello();
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(CONTRACT_VERSION));
    }

    #[test]
    fn hello_with_passthrough_mode() {
        let env = Envelope::hello_with_mode(
            BackendIdentity {
                id: "pt-sidecar".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains("passthrough"));
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_default_mode_is_mapped() {
        let env = make_hello();
        match &env {
            Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Mapped),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn run_roundtrip() {
        let (id, env) = make_run();
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"run""#));
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run {
                id: decoded_id,
                work_order,
            } => {
                assert_eq!(decoded_id, id);
                assert_eq!(work_order.task, "do something");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_preserves_work_order_id() {
        let wo = make_work_order("task");
        let original_wo_id = wo.id;
        let env = Envelope::Run {
            id: "r1".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.id, original_wo_id),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn event_roundtrip_assistant_message() {
        let env = make_event_envelope("run-1", "hello");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"event""#));
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { ref_id, event } => {
                assert_eq!(ref_id, "run-1");
                assert!(matches!(
                    event.kind,
                    AgentEventKind::AssistantMessage { .. }
                ));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_assistant_delta() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::AssistantDelta { text } => assert_eq!(text, "chunk"),
                _ => panic!("expected AssistantDelta"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_tool_call() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/file.txt"}),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    ..
                } => {
                    assert_eq!(tool_name, "read_file");
                    assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                }
                _ => panic!("expected ToolCall"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_tool_result() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!("file contents here"),
                is_error: false,
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::ToolResult {
                    tool_name,
                    is_error,
                    ..
                } => {
                    assert_eq!(tool_name, "read_file");
                    assert!(!is_error);
                }
                _ => panic!("expected ToolResult"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_run_started() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "starting".into(),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(
            decoded,
            Envelope::Event {
                event: AgentEvent {
                    kind: AgentEventKind::RunStarted { .. },
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn event_roundtrip_run_completed() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(
            decoded,
            Envelope::Event {
                event: AgentEvent {
                    kind: AgentEventKind::RunCompleted { .. },
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn event_roundtrip_file_changed() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added fn main".into(),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::FileChanged { path, summary } => {
                    assert_eq!(path, "src/main.rs");
                    assert_eq!(summary, "added fn main");
                }
                _ => panic!("expected FileChanged"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_command_executed() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::CommandExecuted {
                command: "cargo build".into(),
                exit_code: Some(0),
                output_preview: Some("Compiling...".into()),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::CommandExecuted {
                    command,
                    exit_code,
                    output_preview,
                } => {
                    assert_eq!(command, "cargo build");
                    assert_eq!(*exit_code, Some(0));
                    assert_eq!(output_preview.as_deref(), Some("Compiling..."));
                }
                _ => panic!("expected CommandExecuted"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_warning() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::Warning {
                message: "deprecated API".into(),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::Warning { message } => assert_eq!(message, "deprecated API"),
                _ => panic!("expected Warning"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_error() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::Error {
                message: "something broke".into(),
                error_code: None,
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::Error { message, .. } => assert_eq!(message, "something broke"),
                _ => panic!("expected Error"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn final_roundtrip() {
        let env = make_final_envelope("run-1");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"final""#));
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(receipt.outcome, Outcome::Complete);
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn fatal_with_ref_id_roundtrip() {
        let env = make_fatal(Some("run-1"), "out of memory");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"fatal""#));
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal {
                ref_id,
                error,
                error_code,
            } => {
                assert_eq!(ref_id.as_deref(), Some("run-1"));
                assert_eq!(error, "out of memory");
                assert!(error_code.is_none());
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_without_ref_id_roundtrip() {
        let env = make_fatal(None, "startup failure");
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "startup failure");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_with_error_code() {
        let env = Envelope::fatal_with_code(
            Some("run-1".into()),
            "invalid envelope",
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
        );
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(decoded.error_code().is_some());
    }

    #[test]
    fn encode_ends_with_newline() {
        let env = make_hello();
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.ends_with('\n'));
        assert_eq!(json.matches('\n').count(), 1);
    }

    #[test]
    fn decode_handles_trimmed_input() {
        let env = make_hello();
        let json = JsonlCodec::encode(&env).unwrap();
        // Decode with trailing whitespace
        let decoded = JsonlCodec::decode(&format!("  {}  ", json.trim())).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn all_variants_have_tag_t() {
        let (id, run) = make_run();
        let envelopes: Vec<Envelope> = vec![
            make_hello(),
            run,
            make_event_envelope(&id, "msg"),
            make_final_envelope(&id),
            make_fatal(None, "err"),
        ];
        let expected_tags = ["hello", "run", "event", "final", "fatal"];
        for (env, tag) in envelopes.iter().zip(expected_tags.iter()) {
            let json = JsonlCodec::encode(env).unwrap();
            let expected = format!(r#""t":"{}""#, tag);
            assert!(json.contains(&expected), "missing tag {} in {}", tag, json);
        }
    }
}

// ===========================================================================
// 2. JSONL codec: byte streams
// ===========================================================================

mod jsonl_codec {
    use super::*;

    #[test]
    fn encode_to_writer_basic() {
        let mut buf = Vec::new();
        let env = make_hello();
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        assert!(output.contains(r#""t":"hello""#));
    }

    #[test]
    fn encode_many_to_writer() {
        let seq = make_full_sequence();
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &seq).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let line_count = output.lines().count();
        assert_eq!(line_count, seq.len());
    }

    #[test]
    fn decode_stream_basic() {
        let seq = make_full_sequence();
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &seq).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded.len(), seq.len());
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let hello_json = JsonlCodec::encode(&make_hello()).unwrap();
        let input = format!("\n\n{}\n\n{}\n\n", hello_json.trim(), hello_json.trim());
        let reader = BufReader::new(input.as_bytes());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn decode_stream_reports_errors_inline() {
        let hello_json = JsonlCodec::encode(&make_hello()).unwrap();
        let input = format!("{}{}\n{}", hello_json, "not valid json", hello_json);
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        // Should have at least one error and some successes
        let errors = results.iter().filter(|r| r.is_err()).count();
        assert!(errors >= 1);
    }

    #[test]
    fn writer_flush_produces_valid_output() {
        let env = make_hello();
        let mut buf: Vec<u8> = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        buf.flush().unwrap();
        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded.len(), 1);
    }

    #[test]
    fn multiple_envelopes_stay_on_separate_lines() {
        let seq = make_full_sequence();
        let mut buf = Vec::new();
        for env in &seq {
            JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        for line in output.lines() {
            assert!(!line.is_empty());
            // Each line should be valid JSON
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }
}

// ===========================================================================
// 3. StreamingCodec batch operations
// ===========================================================================

mod streaming_codec {
    use super::*;

    #[test]
    fn encode_batch_basic() {
        let seq = make_full_sequence();
        let batch = StreamingCodec::encode_batch(&seq);
        assert_eq!(batch.lines().count(), seq.len());
    }

    #[test]
    fn decode_batch_basic() {
        let seq = make_full_sequence();
        let batch = StreamingCodec::encode_batch(&seq);
        let results = StreamingCodec::decode_batch(&batch);
        assert_eq!(results.len(), seq.len());
        for r in &results {
            assert!(r.is_ok());
        }
    }

    #[test]
    fn line_count_matches() {
        let seq = make_full_sequence();
        let batch = StreamingCodec::encode_batch(&seq);
        assert_eq!(StreamingCodec::line_count(&batch), seq.len());
    }

    #[test]
    fn line_count_skips_blank_lines() {
        let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n";
        assert_eq!(StreamingCodec::line_count(input), 1);
    }

    #[test]
    fn validate_jsonl_valid_input() {
        let seq = make_full_sequence();
        let batch = StreamingCodec::encode_batch(&seq);
        let errors = StreamingCodec::validate_jsonl(&batch);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_jsonl_reports_bad_lines() {
        let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\nnot json\n{}\n";
        let errors = StreamingCodec::validate_jsonl(input);
        assert!(errors.len() >= 2);
    }

    #[test]
    fn encode_decode_roundtrip_batch() {
        let envs: Vec<Envelope> = (0..20)
            .map(|i| make_fatal(None, &format!("error-{}", i)))
            .collect();
        let batch = StreamingCodec::encode_batch(&envs);
        let results = StreamingCodec::decode_batch(&batch);
        assert_eq!(results.len(), 20);
        for (i, r) in results.into_iter().enumerate() {
            let env = r.unwrap();
            match env {
                Envelope::Fatal { error, .. } => assert_eq!(error, format!("error-{}", i)),
                _ => panic!("expected Fatal"),
            }
        }
    }

    #[test]
    fn empty_batch_encodes_to_empty() {
        let batch = StreamingCodec::encode_batch(&[]);
        assert!(batch.is_empty());
        assert_eq!(StreamingCodec::line_count(&batch), 0);
    }
}

// ===========================================================================
// 4. Batch processing
// ===========================================================================

mod batch_processing {
    use super::*;

    fn make_batch_request(envelopes: Vec<Envelope>) -> BatchRequest {
        BatchRequest {
            id: "batch-1".into(),
            envelopes,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn process_valid_batch() {
        let processor = BatchProcessor::new();
        let envs = vec![make_hello(), make_fatal(None, "err")];
        let req = make_batch_request(envs);
        let resp = processor.process(req);
        assert_eq!(resp.request_id, "batch-1");
        assert_eq!(resp.results.len(), 2);
        for r in &resp.results {
            assert_eq!(r.status, BatchItemStatus::Success);
            assert!(r.envelope.is_some());
        }
    }

    #[test]
    fn validate_empty_batch() {
        let processor = BatchProcessor::new();
        let req = make_batch_request(vec![]);
        let errors = processor.validate_batch(&req);
        assert!(errors.contains(&BatchValidationError::EmptyBatch));
    }

    #[test]
    fn validate_oversized_batch() {
        let processor = BatchProcessor::new();
        let envs: Vec<Envelope> = (0..MAX_BATCH_SIZE + 1)
            .map(|i| make_fatal(None, &format!("err-{}", i)))
            .collect();
        let req = make_batch_request(envs);
        let errors = processor.validate_batch(&req);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, BatchValidationError::TooManyItems { .. }))
        );
    }

    #[test]
    fn process_preserves_indices() {
        let processor = BatchProcessor::new();
        let envs: Vec<Envelope> = (0..5)
            .map(|i| make_fatal(None, &format!("err-{}", i)))
            .collect();
        let req = make_batch_request(envs);
        let resp = processor.process(req);
        for (i, r) in resp.results.iter().enumerate() {
            assert_eq!(r.index, i);
        }
    }

    #[test]
    fn batch_processor_default_trait() {
        let processor = BatchProcessor;
        let req = make_batch_request(vec![make_hello()]);
        let resp = processor.process(req);
        assert_eq!(resp.results.len(), 1);
    }

    #[test]
    fn batch_response_has_duration() {
        let processor = BatchProcessor::new();
        let req = make_batch_request(vec![make_hello()]);
        let resp = processor.process(req);
        // Duration should be a non-negative value (could be 0 on fast machines)
        assert!(resp.total_duration_ms < 10_000);
    }

    #[test]
    fn batch_request_serialization() {
        let req = make_batch_request(vec![make_fatal(None, "err")]);
        let json = serde_json::to_string(&req).unwrap();
        let decoded: BatchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "batch-1");
        assert_eq!(decoded.envelopes.len(), 1);
    }

    #[test]
    fn batch_item_status_variants() {
        let success = BatchItemStatus::Success;
        let failed = BatchItemStatus::Failed {
            error: "bad".into(),
        };
        let skipped = BatchItemStatus::Skipped {
            reason: "not needed".into(),
        };
        assert_eq!(success, BatchItemStatus::Success);
        assert_ne!(success, failed);
        assert_ne!(failed, skipped);
    }

    #[test]
    fn batch_validation_error_display() {
        let e = BatchValidationError::EmptyBatch;
        assert_eq!(format!("{e}"), "batch is empty");

        let e = BatchValidationError::TooManyItems {
            count: 2000,
            max: 1000,
        };
        assert!(format!("{e}").contains("2000"));

        let e = BatchValidationError::InvalidEnvelope {
            index: 3,
            error: "bad json".into(),
        };
        assert!(format!("{e}").contains("3"));
    }
}

// ===========================================================================
// 5. Protocol sequence validation
// ===========================================================================

mod sequence_validation {
    use super::*;

    #[test]
    fn valid_full_sequence() {
        let validator = EnvelopeValidator::new();
        let seq = make_full_sequence();
        let errors = validator.validate_sequence(&seq);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn empty_sequence_errors() {
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[]);
        assert!(errors.contains(&SequenceError::MissingHello));
        assert!(errors.contains(&SequenceError::MissingTerminal));
    }

    #[test]
    fn missing_hello_detected() {
        let validator = EnvelopeValidator::new();
        let (id, run) = make_run();
        let seq = vec![run, make_final_envelope(&id)];
        let errors = validator.validate_sequence(&seq);
        assert!(errors.contains(&SequenceError::MissingHello));
    }

    #[test]
    fn missing_terminal_detected() {
        let validator = EnvelopeValidator::new();
        let (id, run) = make_run();
        let seq = vec![make_hello(), run, make_event_envelope(&id, "msg")];
        let errors = validator.validate_sequence(&seq);
        assert!(errors.contains(&SequenceError::MissingTerminal));
    }

    #[test]
    fn hello_not_first_detected() {
        let validator = EnvelopeValidator::new();
        let (id, run) = make_run();
        let seq = vec![run, make_hello(), make_final_envelope(&id)];
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
        );
    }

    #[test]
    fn multiple_terminals_detected() {
        let validator = EnvelopeValidator::new();
        let (id, run) = make_run();
        let seq = vec![
            make_hello(),
            run,
            make_final_envelope(&id),
            make_fatal(Some(&id), "also fatal"),
        ];
        let errors = validator.validate_sequence(&seq);
        assert!(errors.contains(&SequenceError::MultipleTerminals));
    }

    #[test]
    fn ref_id_mismatch_in_event() {
        let validator = EnvelopeValidator::new();
        let (id, run) = make_run();
        let seq = vec![
            make_hello(),
            run,
            make_event_envelope("wrong-id", "msg"),
            make_final_envelope(&id),
        ];
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
        );
    }

    #[test]
    fn ref_id_mismatch_in_final() {
        let validator = EnvelopeValidator::new();
        let (_id, run) = make_run();
        let seq = vec![make_hello(), run, make_final_envelope("wrong-ref")];
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
        );
    }

    #[test]
    fn event_before_run_is_out_of_order() {
        let validator = EnvelopeValidator::new();
        let (id, run) = make_run();
        let seq = vec![
            make_hello(),
            make_event_envelope(&id, "too early"),
            run,
            make_final_envelope(&id),
        ];
        let errors = validator.validate_sequence(&seq);
        assert!(errors.contains(&SequenceError::OutOfOrderEvents));
    }

    #[test]
    fn fatal_only_sequence() {
        let validator = EnvelopeValidator::new();
        let seq = vec![make_hello(), make_fatal(None, "crash")];
        let errors = validator.validate_sequence(&seq);
        // Should be valid (no run is required if fatal happens early)
        assert!(
            !errors.contains(&SequenceError::MissingHello)
                && !errors.contains(&SequenceError::MissingTerminal)
        );
    }

    #[test]
    fn sequence_error_display() {
        let err = SequenceError::MissingHello;
        assert!(!format!("{err}").is_empty());
        let err = SequenceError::MissingTerminal;
        assert!(!format!("{err}").is_empty());
        let err = SequenceError::HelloNotFirst { position: 2 };
        assert!(format!("{err}").contains("2"));
        let err = SequenceError::RefIdMismatch {
            expected: "a".into(),
            found: "b".into(),
        };
        assert!(format!("{err}").contains("a"));
    }
}

// ===========================================================================
// 5b. Individual envelope validation
// ===========================================================================

mod envelope_validation {
    use super::*;

    #[test]
    fn valid_hello_passes() {
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&make_hello());
        assert!(result.valid);
    }

    #[test]
    fn hello_empty_contract_version() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Hello {
            contract_version: String::new(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result.errors.iter().any(
            |e| matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
        ));
    }

    #[test]
    fn hello_invalid_version_format() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Hello {
            contract_version: "invalid".into(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
        );
    }

    #[test]
    fn hello_empty_backend_id() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.into(),
            backend: BackendIdentity {
                id: String::new(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn hello_warns_missing_optional_fields() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.into(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let result = validator.validate(&env);
        assert!(result.valid);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| matches!(w, ValidationWarning::MissingOptionalField { .. }))
        );
    }

    #[test]
    fn run_empty_id_invalid() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Run {
            id: String::new(),
            work_order: make_work_order("task"),
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn run_empty_task_invalid() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Run {
            id: "run-1".into(),
            work_order: make_work_order(""),
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn event_empty_ref_id_invalid() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Event {
            ref_id: String::new(),
            event: make_event(AgentEventKind::AssistantMessage { text: "msg".into() }),
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn final_empty_ref_id_invalid() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Final {
            ref_id: String::new(),
            receipt: make_receipt("test"),
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn fatal_empty_error_invalid() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: String::new(),
            error_code: None,
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn fatal_missing_ref_id_warns() {
        let validator = EnvelopeValidator::new();
        let env = make_fatal(None, "error msg");
        let result = validator.validate(&env);
        assert!(result.valid);
        assert!(result.warnings.iter().any(
            |w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "ref_id")
        ));
    }

    #[test]
    fn validation_result_display() {
        let err = ValidationError::MissingField { field: "x".into() };
        assert!(format!("{err}").contains("x"));
        let err = ValidationError::InvalidVersion {
            version: "bad".into(),
        };
        assert!(format!("{err}").contains("bad"));
        let warn = ValidationWarning::DeprecatedField {
            field: "old".into(),
        };
        assert!(format!("{warn}").contains("old"));
    }
}

// ===========================================================================
// 6. Error handling
// ===========================================================================

mod error_handling {
    use super::*;

    #[test]
    fn decode_malformed_json() {
        let err = JsonlCodec::decode("not valid json").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_empty_object() {
        let err = JsonlCodec::decode("{}").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_missing_discriminator() {
        let err = JsonlCodec::decode(r#"{"type":"hello"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_unknown_variant() {
        let err = JsonlCodec::decode(r#"{"t":"unknown_type"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_missing_required_fields() {
        // Hello without backend field
        let err = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_hello_missing_capabilities() {
        let err =
            JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null}}"#)
                .unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_run_missing_work_order() {
        let err = JsonlCodec::decode(r#"{"t":"run","id":"r1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_event_missing_event_field() {
        let err = JsonlCodec::decode(r#"{"t":"event","ref_id":"r1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_final_missing_receipt() {
        let err = JsonlCodec::decode(r#"{"t":"final","ref_id":"r1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_empty_string() {
        let err = JsonlCodec::decode("").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_just_whitespace() {
        let err = JsonlCodec::decode("   ").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_number() {
        let err = JsonlCodec::decode("42").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_array() {
        let err = JsonlCodec::decode("[1,2,3]").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_null() {
        let err = JsonlCodec::decode("null").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn protocol_error_display() {
        let err = ProtocolError::Violation("bad sequence".into());
        assert!(format!("{err}").contains("bad sequence"));
        let err = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        };
        assert!(format!("{err}").contains("hello"));
        assert!(format!("{err}").contains("run"));
    }

    #[test]
    fn protocol_error_code_for_violation() {
        let err = ProtocolError::Violation("test".into());
        assert!(err.error_code().is_some());
    }

    #[test]
    fn protocol_error_code_for_unexpected_message() {
        let err = ProtocolError::UnexpectedMessage {
            expected: "a".into(),
            got: "b".into(),
        };
        assert!(err.error_code().is_some());
    }

    #[test]
    fn protocol_error_code_for_json_is_none() {
        let err = ProtocolError::Json(serde_json::from_str::<()>("bad").unwrap_err());
        assert!(err.error_code().is_none());
    }
}

// ===========================================================================
// 7. Streaming: StreamParser partial reads
// ===========================================================================

mod stream_parser {
    use super::*;

    #[test]
    fn new_parser_is_empty() {
        let parser = StreamParser::new();
        assert!(parser.is_empty());
        assert_eq!(parser.buffered_len(), 0);
    }

    #[test]
    fn default_parser_is_empty() {
        let parser = StreamParser::default();
        assert!(parser.is_empty());
    }

    #[test]
    fn push_partial_line_yields_nothing() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&make_hello()).unwrap();
        let (first, _) = line.as_bytes().split_at(10);
        let results = parser.push(first);
        assert!(results.is_empty());
        assert!(!parser.is_empty());
    }

    #[test]
    fn push_complete_line_yields_envelope() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&make_hello()).unwrap();
        let results = parser.push(line.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        assert!(parser.is_empty());
    }

    #[test]
    fn split_across_two_pushes() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
        let mid = line.len() / 2;
        let (first, second) = line.as_bytes().split_at(mid);

        assert!(parser.push(first).is_empty());
        let results = parser.push(second);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn multiple_lines_in_one_push() {
        let mut parser = StreamParser::new();
        let hello_line = JsonlCodec::encode(&make_hello()).unwrap();
        let fatal_line = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();
        let combined = format!("{}{}", hello_line, fatal_line);
        let results = parser.push(combined.as_bytes());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn finish_drains_unterminated_line() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&make_hello()).unwrap();
        let unterminated = line.trim(); // no trailing newline
        parser.push(unterminated.as_bytes());
        assert!(!parser.is_empty());
        let results = parser.finish();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        assert!(parser.is_empty());
    }

    #[test]
    fn finish_on_empty_parser() {
        let mut parser = StreamParser::new();
        let results = parser.finish();
        assert!(results.is_empty());
    }

    #[test]
    fn blank_lines_skipped() {
        let mut parser = StreamParser::new();
        let hello_line = JsonlCodec::encode(&make_hello()).unwrap();
        let input = format!("\n\n{}\n\n", hello_line.trim());
        let results = parser.push(input.as_bytes());
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn reset_clears_buffer() {
        let mut parser = StreamParser::new();
        parser.push(b"partial data without newline");
        assert!(!parser.is_empty());
        parser.reset();
        assert!(parser.is_empty());
        assert_eq!(parser.buffered_len(), 0);
    }

    #[test]
    fn invalid_json_produces_error() {
        let mut parser = StreamParser::new();
        let results = parser.push(b"not valid json\n");
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn max_line_length_enforcement() {
        let mut parser = StreamParser::with_max_line_len(50);
        let long_line = format!(
            "{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{}\"}}\n",
            "x".repeat(100)
        );
        let results = parser.push(long_line.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn invalid_utf8_produces_error() {
        let mut parser = StreamParser::new();
        // Invalid UTF-8 byte sequence followed by newline
        let mut data: Vec<u8> = vec![0xFF, 0xFE, 0x80];
        data.push(b'\n');
        let results = parser.push(&data);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn buffered_len_tracks_correctly() {
        let mut parser = StreamParser::new();
        assert_eq!(parser.buffered_len(), 0);
        parser.push(b"hello");
        assert_eq!(parser.buffered_len(), 5);
        parser.push(b" world\n");
        // After consuming the line, buffer should be empty
        assert_eq!(parser.buffered_len(), 0);
    }

    #[test]
    fn interleaved_valid_and_invalid() {
        let mut parser = StreamParser::new();
        let hello_json = JsonlCodec::encode(&make_hello()).unwrap();
        let input = format!("{}garbage\n{}", hello_json, hello_json);
        let results = parser.push(input.as_bytes());
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
    }
}

// ===========================================================================
// 8. Routing
// ===========================================================================

mod routing {
    use super::*;

    #[test]
    fn route_by_envelope_type() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "hello".into(),
            destination: "handshake-handler".into(),
            priority: 10,
        });
        let env = make_hello();
        let matched = router.route(&env);
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().destination, "handshake-handler");
    }

    #[test]
    fn route_by_ref_id_prefix() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "run-".into(),
            destination: "run-handler".into(),
            priority: 5,
        });
        let env = make_event_envelope("run-123", "msg");
        let matched = router.route(&env);
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().destination, "run-handler");
    }

    #[test]
    fn priority_order() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "fatal".into(),
            destination: "low-priority".into(),
            priority: 1,
        });
        router.add_route(MessageRoute {
            pattern: "fatal".into(),
            destination: "high-priority".into(),
            priority: 100,
        });
        let env = make_fatal(None, "err");
        let matched = router.route(&env).unwrap();
        assert_eq!(matched.destination, "high-priority");
    }

    #[test]
    fn no_match_returns_none() {
        let router = MessageRouter::new();
        let env = make_hello();
        assert!(router.route(&env).is_none());
    }

    #[test]
    fn remove_route() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "hello".into(),
            destination: "handler".into(),
            priority: 1,
        });
        assert_eq!(router.route_count(), 1);
        router.remove_route("handler");
        assert_eq!(router.route_count(), 0);
    }

    #[test]
    fn route_all_matches_multiple() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "hello".into(),
            destination: "h".into(),
            priority: 1,
        });
        router.add_route(MessageRoute {
            pattern: "fatal".into(),
            destination: "f".into(),
            priority: 1,
        });
        let envs = vec![make_hello(), make_fatal(None, "err")];
        let matches = router.route_all(&envs);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn route_table_insert_and_lookup() {
        let mut table = RouteTable::new();
        table.insert("hello", "hello-handler");
        table.insert("fatal", "error-handler");
        assert_eq!(table.lookup("hello"), Some("hello-handler"));
        assert_eq!(table.lookup("fatal"), Some("error-handler"));
        assert_eq!(table.lookup("run"), None);
    }

    #[test]
    fn route_table_entries() {
        let mut table = RouteTable::new();
        table.insert("hello", "h");
        table.insert("fatal", "f");
        let entries = table.entries();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn route_table_serialization() {
        let mut table = RouteTable::new();
        table.insert("hello", "h");
        let json = serde_json::to_string(&table).unwrap();
        let decoded: RouteTable = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.lookup("hello"), Some("h"));
    }
}

// ===========================================================================
// 9. Version negotiation
// ===========================================================================

mod version_negotiation {
    use super::*;

    #[test]
    fn parse_valid_version() {
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
        assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
    }

    #[test]
    fn parse_invalid_versions() {
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("abp/v"), None);
        assert_eq!(parse_version("abp/v1"), None);
        assert_eq!(parse_version("v0.1"), None);
        assert_eq!(parse_version("abp/vx.1"), None);
    }

    #[test]
    fn compatible_versions() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
        assert!(is_compatible_version("abp/v1.0", "abp/v1.5"));
    }

    #[test]
    fn incompatible_versions() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
        assert!(!is_compatible_version("invalid", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "invalid"));
    }

    #[test]
    fn protocol_version_parse() {
        let v = ProtocolVersion::parse("abp/v0.1").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
    }

    #[test]
    fn protocol_version_parse_errors() {
        assert!(ProtocolVersion::parse("invalid").is_err());
        assert!(ProtocolVersion::parse("abp/vx.1").is_err());
        assert!(ProtocolVersion::parse("abp/v1.x").is_err());
    }

    #[test]
    fn protocol_version_display() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        assert_eq!(format!("{v}"), "abp/v0.1");
    }

    #[test]
    fn protocol_version_current() {
        let v = ProtocolVersion::current();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
    }

    #[test]
    fn protocol_version_is_compatible() {
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        let v10 = ProtocolVersion { major: 1, minor: 0 };
        assert!(v01.is_compatible(&v02));
        assert!(!v01.is_compatible(&v10));
    }

    #[test]
    fn version_range_contains() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 5 },
        };
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 6 }));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    }

    #[test]
    fn version_range_is_compatible() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 5 },
        };
        assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 3 }));
        assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 3 }));
    }

    #[test]
    fn negotiate_same_version() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&v, &v).unwrap();
        assert_eq!(result, v);
    }

    #[test]
    fn negotiate_picks_minimum() {
        let local = ProtocolVersion { major: 0, minor: 2 };
        let remote = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&local, &remote).unwrap();
        assert_eq!(result.minor, 1);
    }

    #[test]
    fn negotiate_incompatible_fails() {
        let local = ProtocolVersion { major: 0, minor: 1 };
        let remote = ProtocolVersion { major: 1, minor: 0 };
        assert!(negotiate_version(&local, &remote).is_err());
    }
}

// ===========================================================================
// 10. Builder pattern
// ===========================================================================

mod builder_tests {
    use super::*;

    #[test]
    fn hello_builder_minimal() {
        let env = EnvelopeBuilder::hello().backend("test").build().unwrap();
        match env {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, "test"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_builder_all_fields() {
        let env = EnvelopeBuilder::hello()
            .backend("sidecar")
            .version("2.0")
            .adapter_version("1.0")
            .mode(ExecutionMode::Passthrough)
            .capabilities(CapabilityManifest::new())
            .build()
            .unwrap();
        match env {
            Envelope::Hello {
                backend,
                mode,
                contract_version,
                ..
            } => {
                assert_eq!(backend.id, "sidecar");
                assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
                assert_eq!(backend.adapter_version.as_deref(), Some("1.0"));
                assert_eq!(mode, ExecutionMode::Passthrough);
                assert_eq!(contract_version, CONTRACT_VERSION);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_builder_missing_backend_fails() {
        let err = EnvelopeBuilder::hello().build().unwrap_err();
        assert_eq!(
            err,
            abp_protocol::builder::BuilderError::MissingField("backend")
        );
    }

    #[test]
    fn run_builder() {
        let wo = make_work_order("test task");
        let env = EnvelopeBuilder::run(wo)
            .ref_id("custom-id")
            .build()
            .unwrap();
        match env {
            Envelope::Run { id, .. } => assert_eq!(id, "custom-id"),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn event_builder() {
        let event = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
        let env = EnvelopeBuilder::event(event)
            .ref_id("run-1")
            .build()
            .unwrap();
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-1"),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_builder_missing_ref_id_fails() {
        let event = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
        assert!(EnvelopeBuilder::event(event).build().is_err());
    }

    #[test]
    fn final_builder() {
        let receipt = make_receipt("test");
        let env = EnvelopeBuilder::final_receipt(receipt)
            .ref_id("run-1")
            .build()
            .unwrap();
        match env {
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "run-1"),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_builder_missing_ref_id_fails() {
        let receipt = make_receipt("test");
        assert!(EnvelopeBuilder::final_receipt(receipt).build().is_err());
    }

    #[test]
    fn fatal_builder() {
        let env = EnvelopeBuilder::fatal("boom")
            .ref_id("run-1")
            .build()
            .unwrap();
        match env {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("run-1"));
                assert_eq!(error, "boom");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_builder_no_ref_id() {
        let env = EnvelopeBuilder::fatal("startup crash").build().unwrap();
        match env {
            Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
            _ => panic!("expected Fatal"),
        }
    }
}

// ===========================================================================
// 11. Edge cases
// ===========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn unicode_in_error_message() {
        let env = make_fatal(None, "エラー: 致命的な問題 🔥");
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error, .. } => assert_eq!(error, "エラー: 致命的な問題 🔥"),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn unicode_in_task() {
        let wo = make_work_order("修复一个错误 🐛");
        let env = Envelope::Run {
            id: "r1".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.task, "修复一个错误 🐛"),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn special_characters_in_strings() {
        let env = make_fatal(None, r#"error with "quotes" and \backslash and tab	here"#);
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error, .. } => {
                assert!(error.contains("quotes"));
                assert!(error.contains("\\backslash"));
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn newlines_in_error_message() {
        let env = make_fatal(None, "line1\nline2\nline3");
        let json = JsonlCodec::encode(&env).unwrap();
        // The JSON should be on a single line (newlines escaped)
        assert_eq!(json.trim().lines().count(), 1);
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error, .. } => {
                assert!(error.contains("line1\nline2\nline3"));
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn empty_string_fields() {
        // Empty ref_id in event - should serialize fine
        let env = Envelope::Event {
            ref_id: String::new(),
            event: make_event(AgentEventKind::AssistantMessage {
                text: String::new(),
            }),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Event { .. }));
    }

    #[test]
    fn very_long_error_message() {
        let long_msg = "x".repeat(100_000);
        let env = make_fatal(None, &long_msg);
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100_000),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn very_long_task_description() {
        let long_task = "a".repeat(50_000);
        let wo = make_work_order(&long_task);
        let env = Envelope::Run {
            id: "r1".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.task.len(), 50_000),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn event_with_ext_field() {
        let mut ext = BTreeMap::new();
        ext.insert("vendor_field".to_string(), serde_json::json!("custom"));
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "hi".into() },
                ext: Some(ext),
            },
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => {
                assert!(event.ext.is_some());
                assert!(event.ext.unwrap().contains_key("vendor_field"));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn large_batch_roundtrip() {
        let envs: Vec<Envelope> = (0..500)
            .map(|i| make_fatal(None, &format!("error-{}", i)))
            .collect();
        let batch = StreamingCodec::encode_batch(&envs);
        let results = StreamingCodec::decode_batch(&batch);
        assert_eq!(results.len(), 500);
        for r in &results {
            assert!(r.is_ok());
        }
    }

    #[test]
    fn envelope_clone_works() {
        let env = make_hello();
        let cloned = env.clone();
        let json1 = JsonlCodec::encode(&env).unwrap();
        let json2 = JsonlCodec::encode(&cloned).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn envelope_debug_format() {
        let env = make_hello();
        let debug = format!("{:?}", env);
        assert!(debug.contains("Hello"));
    }

    #[test]
    fn contract_version_constant() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn decode_raw_json_hello() {
        let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
        let env = JsonlCodec::decode(raw).unwrap();
        match env {
            Envelope::Hello {
                contract_version,
                backend,
                mode,
                ..
            } => {
                assert_eq!(contract_version, "abp/v0.1");
                assert_eq!(backend.id, "test");
                assert_eq!(mode, ExecutionMode::Mapped);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn decode_raw_json_fatal() {
        let raw = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
        let env = JsonlCodec::decode(raw).unwrap();
        assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
    }

    #[test]
    fn stream_parser_with_many_chunks() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&make_hello()).unwrap();
        let bytes = line.as_bytes();
        // Feed one byte at a time
        for (i, &b) in bytes.iter().enumerate() {
            let results = parser.push(&[b]);
            if i < bytes.len() - 1 {
                assert!(results.is_empty(), "unexpected result at byte {}", i);
            } else {
                // Last byte should be newline, triggering decode
                assert_eq!(results.len(), 1);
                assert!(results[0].is_ok());
            }
        }
    }

    #[test]
    fn writer_to_reader_roundtrip() {
        let seq = make_full_sequence();
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &seq).unwrap();

        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(decoded.len(), seq.len());
        // Verify variant types match
        for (orig, dec) in seq.iter().zip(decoded.iter()) {
            assert_eq!(std::mem::discriminant(orig), std::mem::discriminant(dec));
        }
    }

    #[test]
    fn stream_parser_to_batch_pipeline() {
        let seq = make_full_sequence();
        let batch = StreamingCodec::encode_batch(&seq);

        let mut parser = StreamParser::new();
        let results = parser.push(batch.as_bytes());
        assert_eq!(results.len(), seq.len());
        for r in &results {
            assert!(r.is_ok());
        }
    }
}
