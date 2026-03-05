#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ExecutionMode,
    Outcome, Receipt, ReceiptBuilder, SupportLevel, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_sidecar_utils::codec::{CodecMetrics, StreamingCodec, DEFAULT_MAX_LINE_LEN};
use abp_sidecar_utils::event_stream::{EventStreamError, EventStreamProcessor, EventStreamStats};
use abp_sidecar_utils::frame::{
    backend_identity, contract_version, decode_envelope, encode_envelope, encode_event,
    encode_fatal, encode_final, encode_hello,
};
use abp_sidecar_utils::handshake::{
    HandshakeError, HandshakeManager, HelloInfo, DEFAULT_HANDSHAKE_TIMEOUT,
};
use abp_sidecar_utils::health::{
    ProtocolHealth, DEFAULT_CONNECTION_TIMEOUT, DEFAULT_HEARTBEAT_INTERVAL,
};
use abp_sidecar_utils::testing::{mock_event, mock_fatal, mock_final, mock_hello, mock_work_order};
use abp_sidecar_utils::validate::{validate_hello, validate_ref_id, validate_sequence};
use chrono::Utc;
use tokio::io::BufReader;

// =========================================================================
// Helper functions
// =========================================================================

fn fatal_line(msg: &str) -> String {
    format!("{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{msg}\"}}\n")
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_envelope(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: make_agent_event(kind),
    }
}

fn make_hello_line(version: &str) -> String {
    let env = Envelope::Hello {
        contract_version: version.to_string(),
        backend: BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    JsonlCodec::encode(&env).unwrap()
}

// =========================================================================
// Module: codec
// =========================================================================

#[test]
fn codec_new_defaults() {
    let codec = StreamingCodec::new();
    assert_eq!(codec.buffered_len(), 0);
    assert_eq!(codec.metrics().bytes_read, 0);
    assert_eq!(codec.metrics().lines_parsed, 0);
    assert_eq!(codec.metrics().errors_skipped, 0);
}

#[test]
fn codec_default_trait() {
    let codec = StreamingCodec::default();
    assert_eq!(codec.buffered_len(), 0);
}

#[test]
fn codec_default_max_line_len_constant() {
    assert_eq!(DEFAULT_MAX_LINE_LEN, 10 * 1024 * 1024);
}

#[test]
fn codec_with_max_line_len() {
    let codec = StreamingCodec::with_max_line_len(512);
    assert_eq!(codec.buffered_len(), 0);
}

#[test]
fn codec_push_single_line() {
    let mut codec = StreamingCodec::new();
    let line = fatal_line("boom");
    let envs = codec.push(line.as_bytes());
    assert_eq!(envs.len(), 1);
    assert!(matches!(&envs[0], Envelope::Fatal { error, .. } if error == "boom"));
    assert_eq!(codec.metrics().lines_parsed, 1);
    assert_eq!(codec.metrics().bytes_read, line.len() as u64);
}

#[test]
fn codec_push_multiple_lines() {
    let mut codec = StreamingCodec::new();
    let data = format!("{}{}{}", fatal_line("a"), fatal_line("b"), fatal_line("c"));
    let envs = codec.push(data.as_bytes());
    assert_eq!(envs.len(), 3);
    assert_eq!(codec.metrics().lines_parsed, 3);
}

#[test]
fn codec_chunked_reading() {
    let mut codec = StreamingCodec::new();
    let line = fatal_line("chunked");
    let (a, b) = line.as_bytes().split_at(10);
    assert!(codec.push(a).is_empty());
    assert!(codec.buffered_len() > 0);
    let envs = codec.push(b);
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.metrics().lines_parsed, 1);
}

#[test]
fn codec_line_length_limit_exceeded() {
    let mut codec = StreamingCodec::with_max_line_len(20);
    let long = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"this is way too long\"}\n";
    let envs = codec.push(long.as_bytes());
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn codec_error_recovery_skips_bad_lines() {
    let mut codec = StreamingCodec::new();
    let data = format!("not valid json\n{}", fatal_line("ok"));
    let envs = codec.push(data.as_bytes());
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.metrics().errors_skipped, 1);
    assert_eq!(codec.metrics().lines_parsed, 1);
}

#[test]
fn codec_blank_lines_skipped() {
    let mut codec = StreamingCodec::new();
    let data = format!("\n\n{}\n\n", fatal_line("ok").trim_end());
    let envs = codec.push(data.as_bytes());
    assert_eq!(envs.len(), 1);
}

#[test]
fn codec_finish_flushes_partial() {
    let mut codec = StreamingCodec::new();
    let line = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"eof\"}";
    codec.push(line.as_bytes());
    assert_eq!(codec.buffered_len(), line.len());
    let envs = codec.finish();
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.buffered_len(), 0);
}

#[test]
fn codec_finish_empty_buffer() {
    let mut codec = StreamingCodec::new();
    let envs = codec.finish();
    assert!(envs.is_empty());
}

#[test]
fn codec_non_utf8_skipped() {
    let mut codec = StreamingCodec::new();
    let mut data = vec![0xFF, 0xFE, b'\n'];
    data.extend_from_slice(fatal_line("ok").as_bytes());
    let envs = codec.push(&data);
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn codec_reset_clears_state() {
    let mut codec = StreamingCodec::new();
    codec.push(fatal_line("x").as_bytes());
    assert!(codec.metrics().lines_parsed > 0);
    codec.reset();
    assert_eq!(codec.buffered_len(), 0);
    assert_eq!(codec.metrics().lines_parsed, 0);
    assert_eq!(codec.metrics().bytes_read, 0);
    assert_eq!(codec.metrics().errors_skipped, 0);
}

#[test]
fn codec_metrics_struct_default() {
    let metrics = CodecMetrics::default();
    assert_eq!(metrics.bytes_read, 0);
    assert_eq!(metrics.lines_parsed, 0);
    assert_eq!(metrics.errors_skipped, 0);
}

#[test]
fn codec_metrics_struct_clone() {
    let mut codec = StreamingCodec::new();
    codec.push(fatal_line("x").as_bytes());
    let m1 = codec.metrics().clone();
    assert_eq!(m1.lines_parsed, 1);
}

#[test]
fn codec_push_accumulates_bytes_read() {
    let mut codec = StreamingCodec::new();
    let chunk1 = b"abc";
    let chunk2 = b"def\n";
    codec.push(chunk1);
    codec.push(chunk2);
    assert_eq!(codec.metrics().bytes_read, 7);
}

#[test]
fn codec_multiple_bad_lines() {
    let mut codec = StreamingCodec::new();
    let data = "bad1\nbad2\nbad3\n";
    let envs = codec.push(data.as_bytes());
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().errors_skipped, 3);
}

// =========================================================================
// Module: frame
// =========================================================================

#[test]
fn frame_encode_hello_basic() {
    let line = encode_hello("my-backend", "1.0", &["streaming"]);
    assert!(line.contains("\"t\":\"hello\""));
    assert!(line.contains("my-backend"));
}

#[test]
fn frame_encode_hello_no_capabilities() {
    let line = encode_hello("test", "0.1", &[]);
    assert!(line.contains("\"t\":\"hello\""));
}

#[test]
fn frame_encode_hello_unknown_cap_ignored() {
    let line = encode_hello("test", "0.1", &["not_a_real_capability_xyz"]);
    assert!(line.contains("\"t\":\"hello\""));
}

#[test]
fn frame_encode_event() {
    let event = make_agent_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let line = encode_event("run-1", &event);
    assert!(line.contains("\"t\":\"event\""));
    assert!(line.contains("run-1"));
}

#[test]
fn frame_encode_final() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let line = encode_final("run-1", &receipt);
    assert!(line.contains("\"t\":\"final\""));
    assert!(line.contains("run-1"));
}

#[test]
fn frame_encode_fatal() {
    let line = encode_fatal("run-1", "out of memory");
    assert!(line.contains("\"t\":\"fatal\""));
    assert!(line.contains("out of memory"));
}

#[test]
fn frame_encode_decode_roundtrip() {
    let hello = mock_hello("test");
    let encoded = encode_envelope(&hello).unwrap();
    let decoded = decode_envelope(&encoded).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn frame_decode_with_trailing_newline() {
    let line = fatal_line("test");
    let decoded = decode_envelope(&line).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn frame_decode_invalid_json() {
    let result = decode_envelope("not json at all");
    assert!(result.is_err());
}

#[test]
fn frame_encode_envelope_event() {
    let env = make_event_envelope(
        "run-1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let encoded = encode_envelope(&env).unwrap();
    assert!(encoded.contains("\"t\":\"event\""));
}

#[test]
fn frame_backend_identity_helper() {
    let bi = backend_identity("my-name", "2.0");
    assert_eq!(bi.id, "my-name");
    assert_eq!(bi.backend_version, Some("2.0".to_string()));
    assert!(bi.adapter_version.is_none());
}

#[test]
fn frame_contract_version() {
    let v = contract_version();
    assert_eq!(v, CONTRACT_VERSION);
    assert!(v.starts_with("abp/"));
}

#[test]
fn frame_encode_fatal_roundtrip_decode() {
    let line = encode_fatal("r1", "kaboom");
    let env = decode_envelope(&line).unwrap();
    match env {
        Envelope::Fatal { error, ref_id, .. } => {
            assert_eq!(error, "kaboom");
            assert_eq!(ref_id, Some("r1".to_string()));
        }
        _ => panic!("expected Fatal"),
    }
}

// =========================================================================
// Module: testing
// =========================================================================

#[test]
fn testing_mock_hello() {
    let hello = mock_hello("test-backend");
    match &hello {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "test-backend");
            assert_eq!(backend.backend_version, Some("0.1.0".to_string()));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn testing_mock_event() {
    let event = mock_event("run-1", "hello world");
    match &event {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(matches!(
                &event.kind,
                AgentEventKind::AssistantMessage { text } if text == "hello world"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn testing_mock_final() {
    let final_env = mock_final("run-1");
    match &final_env {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn testing_mock_fatal() {
    let fatal = mock_fatal("run-1", "out of memory");
    match &fatal {
        Envelope::Fatal { error, ref_id, .. } => {
            assert_eq!(error, "out of memory");
            assert_eq!(ref_id.as_deref(), Some("run-1"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn testing_mock_work_order() {
    let line = mock_work_order("fix the bug");
    assert!(line.contains("\"t\":\"run\""));
    assert!(line.contains("fix the bug"));
}

#[test]
fn testing_mock_work_order_decode() {
    let line = mock_work_order("test task");
    let env = decode_envelope(&line).unwrap();
    assert!(matches!(env, Envelope::Run { .. }));
}

#[test]
fn testing_mock_hello_with_contract_version() {
    let hello = mock_hello("v-test");
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// Module: validate
// =========================================================================

#[test]
fn validate_hello_valid() {
    let hello = mock_hello("backend");
    assert!(validate_hello(&hello).is_ok());
}

#[test]
fn validate_hello_incompatible_version() {
    let hello = Envelope::Hello {
        contract_version: "abp/v99.0".into(),
        backend: BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = validate_hello(&hello).unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_hello_not_hello_envelope() {
    let fatal = mock_fatal("r1", "err");
    let err = validate_hello(&fatal).unwrap_err();
    assert!(matches!(err, ProtocolError::UnexpectedMessage { .. }));
}

#[test]
fn validate_hello_with_event_envelope() {
    let event = mock_event("r1", "hi");
    let err = validate_hello(&event).unwrap_err();
    assert!(matches!(err, ProtocolError::UnexpectedMessage { .. }));
}

#[test]
fn validate_ref_id_event_match() {
    let event = mock_event("run-1", "text");
    assert!(validate_ref_id(&event, "run-1").is_ok());
}

#[test]
fn validate_ref_id_event_mismatch() {
    let event = mock_event("run-1", "text");
    let err = validate_ref_id(&event, "run-2").unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_ref_id_final_match() {
    let final_env = mock_final("run-1");
    assert!(validate_ref_id(&final_env, "run-1").is_ok());
}

#[test]
fn validate_ref_id_final_mismatch() {
    let final_env = mock_final("run-1");
    let err = validate_ref_id(&final_env, "run-2").unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_ref_id_fatal_with_ref_id_match() {
    let fatal = mock_fatal("run-1", "err");
    assert!(validate_ref_id(&fatal, "run-1").is_ok());
}

#[test]
fn validate_ref_id_fatal_with_ref_id_mismatch() {
    let fatal = mock_fatal("run-1", "err");
    let err = validate_ref_id(&fatal, "run-2").unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_ref_id_fatal_no_ref_id_always_ok() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    assert!(validate_ref_id(&fatal, "anything").is_ok());
}

#[test]
fn validate_ref_id_hello_always_ok() {
    let hello = mock_hello("x");
    assert!(validate_ref_id(&hello, "anything").is_ok());
}

#[test]
fn validate_sequence_valid_hello_event_final() {
    let seq = vec![mock_hello("x"), mock_event("r1", "hi"), mock_final("r1")];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_valid_hello_fatal() {
    let seq = vec![mock_hello("x"), mock_fatal("r1", "err")];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_valid_hello_multiple_events_final() {
    let seq = vec![
        mock_hello("x"),
        mock_event("r1", "a"),
        mock_event("r1", "b"),
        mock_event("r1", "c"),
        mock_final("r1"),
    ];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_empty() {
    let err = validate_sequence(&[]).unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_sequence_no_terminal() {
    let seq = vec![mock_hello("x")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_sequence_no_hello_first() {
    let seq = vec![mock_event("r1", "hi"), mock_final("r1")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_sequence_hello_in_middle() {
    let seq = vec![mock_hello("x"), mock_hello("y"), mock_final("r1")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_sequence_multiple_terminals() {
    let seq = vec![mock_hello("x"), mock_final("r1"), mock_fatal("r1", "extra")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

#[test]
fn validate_sequence_fatal_in_middle() {
    let seq = vec![
        mock_hello("x"),
        mock_fatal("r1", "err"),
        mock_event("r1", "hi"),
        mock_final("r1"),
    ];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(matches!(err, ProtocolError::Violation(_)));
}

// =========================================================================
// Module: event_stream
// =========================================================================

#[test]
fn event_stream_new() {
    let proc = EventStreamProcessor::new("run-1".into());
    assert!(!proc.is_terminal());
    assert_eq!(proc.stats().events_processed, 0);
    assert_eq!(proc.stats().ref_id_mismatches, 0);
    assert_eq!(proc.stats().events_after_terminal, 0);
    assert!(proc.stats().counts_by_type.is_empty());
}

#[test]
fn event_stream_stats_default() {
    let stats = EventStreamStats::default();
    assert_eq!(stats.events_processed, 0);
    assert_eq!(stats.ref_id_mismatches, 0);
    assert_eq!(stats.events_after_terminal, 0);
    assert!(stats.counts_by_type.is_empty());
}

#[test]
fn event_stream_process_valid_event() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let env = make_event_envelope(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
    );
    let result = proc.process_envelope(&env).unwrap();
    assert!(result.is_some());
    assert_eq!(proc.stats().events_processed, 1);
}

#[test]
fn event_stream_process_ref_id_mismatch() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let env = make_event_envelope(
        "run-other",
        AgentEventKind::RunStarted {
            message: "hi".into(),
        },
    );
    let err = proc.process_envelope(&env).unwrap_err();
    assert!(matches!(err, EventStreamError::RefIdMismatch { .. }));
    assert_eq!(proc.stats().ref_id_mismatches, 1);
}

#[test]
fn event_stream_event_after_final() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let final_env = mock_final("run-1");
    proc.process_envelope(&final_env).unwrap();
    assert!(proc.is_terminal());

    let event = make_event_envelope(
        "run-1",
        AgentEventKind::RunCompleted {
            message: "late".into(),
        },
    );
    let err = proc.process_envelope(&event).unwrap_err();
    assert!(matches!(err, EventStreamError::EventAfterTerminal));
    assert_eq!(proc.stats().events_after_terminal, 1);
}

#[test]
fn event_stream_event_after_fatal() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let fatal = mock_fatal("run-1", "boom");
    proc.process_envelope(&fatal).unwrap();
    assert!(proc.is_terminal());

    let event = make_event_envelope("run-1", AgentEventKind::AssistantDelta { text: "x".into() });
    let err = proc.process_envelope(&event).unwrap_err();
    assert!(matches!(err, EventStreamError::EventAfterTerminal));
}

#[test]
fn event_stream_final_marks_terminal() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let final_env = mock_final("run-1");
    let result = proc.process_envelope(&final_env).unwrap();
    assert!(result.is_none());
    assert!(proc.is_terminal());
}

#[test]
fn event_stream_fatal_marks_terminal() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let fatal = mock_fatal("run-1", "err");
    let result = proc.process_envelope(&fatal).unwrap();
    assert!(result.is_none());
    assert!(proc.is_terminal());
}

#[test]
fn event_stream_unexpected_hello() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let hello = mock_hello("backend");
    let err = proc.process_envelope(&hello).unwrap_err();
    assert!(matches!(err, EventStreamError::UnexpectedEnvelope(_)));
}

#[test]
fn event_stream_unexpected_run() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let wo = WorkOrderBuilder::new("task").build();
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let err = proc.process_envelope(&run).unwrap_err();
    assert!(matches!(err, EventStreamError::UnexpectedEnvelope(_)));
}

#[test]
fn event_stream_counts_by_type() {
    let mut proc = EventStreamProcessor::new("run-1".into());

    for _ in 0..3 {
        let env = make_event_envelope(
            "run-1",
            AgentEventKind::AssistantDelta { text: "tok".into() },
        );
        proc.process_envelope(&env).unwrap();
    }

    let env = make_event_envelope(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "full".into(),
        },
    );
    proc.process_envelope(&env).unwrap();

    assert_eq!(proc.stats().counts_by_type.get("assistant_delta"), Some(&3));
    assert_eq!(
        proc.stats().counts_by_type.get("assistant_message"),
        Some(&1)
    );
    assert_eq!(proc.stats().events_processed, 4);
}

#[test]
fn event_stream_process_many() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let envs = vec![
        make_event_envelope(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event_envelope(
            "run-1",
            AgentEventKind::AssistantMessage { text: "hi".into() },
        ),
        make_event_envelope(
            "run-1",
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
    ];
    let results = proc.process_many(&envs);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_ok()));
    assert_eq!(proc.stats().events_processed, 3);
}

#[test]
fn event_stream_process_many_with_errors() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let envs = vec![
        make_event_envelope(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event_envelope(
            "run-wrong",
            AgentEventKind::AssistantMessage { text: "bad".into() },
        ),
        make_event_envelope(
            "run-1",
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
    ];
    let results = proc.process_many(&envs);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
    assert_eq!(proc.stats().events_processed, 2);
    assert_eq!(proc.stats().ref_id_mismatches, 1);
}

#[test]
fn event_stream_process_many_empty() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let results = proc.process_many(&[]);
    assert!(results.is_empty());
}

#[test]
fn event_stream_stats_clone() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let env = make_event_envelope("run-1", AgentEventKind::AssistantDelta { text: "t".into() });
    proc.process_envelope(&env).unwrap();
    let stats = proc.stats().clone();
    assert_eq!(stats.events_processed, 1);
}

#[test]
fn event_stream_multiple_ref_id_mismatches() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    for i in 0..5 {
        let env = make_event_envelope(
            &format!("wrong-{i}"),
            AgentEventKind::AssistantDelta { text: "x".into() },
        );
        let _ = proc.process_envelope(&env);
    }
    assert_eq!(proc.stats().ref_id_mismatches, 5);
    assert_eq!(proc.stats().events_processed, 0);
}

#[test]
fn event_stream_warning_event_type() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let env = make_event_envelope(
        "run-1",
        AgentEventKind::Warning {
            message: "watch out".into(),
        },
    );
    proc.process_envelope(&env).unwrap();
    assert_eq!(proc.stats().counts_by_type.get("warning"), Some(&1));
}

#[test]
fn event_stream_error_event_type() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let env = make_event_envelope(
        "run-1",
        AgentEventKind::Error {
            message: "bad".into(),
            error_code: None,
        },
    );
    proc.process_envelope(&env).unwrap();
    assert_eq!(proc.stats().counts_by_type.get("error"), Some(&1));
}

// =========================================================================
// Module: health
// =========================================================================

#[test]
fn health_initial_state() {
    let h = ProtocolHealth::with_defaults();
    assert!(!h.is_timed_out());
    assert!(!h.is_heartbeat_overdue());
    assert_eq!(h.heartbeats_received(), 0);
    assert!(!h.is_shutdown_signalled());
}

#[test]
fn health_default_trait() {
    let h = ProtocolHealth::default();
    assert_eq!(h.heartbeat_interval(), DEFAULT_HEARTBEAT_INTERVAL);
    assert_eq!(h.connection_timeout(), DEFAULT_CONNECTION_TIMEOUT);
}

#[test]
fn health_default_constants() {
    assert_eq!(DEFAULT_HEARTBEAT_INTERVAL, Duration::from_secs(30));
    assert_eq!(DEFAULT_CONNECTION_TIMEOUT, Duration::from_secs(90));
}

#[test]
fn health_custom_settings() {
    let h = ProtocolHealth::new(Duration::from_secs(15), Duration::from_secs(45));
    assert_eq!(h.heartbeat_interval(), Duration::from_secs(15));
    assert_eq!(h.connection_timeout(), Duration::from_secs(45));
}

#[test]
fn health_record_heartbeat() {
    let mut h = ProtocolHealth::with_defaults();
    h.record_heartbeat();
    assert_eq!(h.heartbeats_received(), 1);
    h.record_heartbeat();
    assert_eq!(h.heartbeats_received(), 2);
    h.record_heartbeat();
    assert_eq!(h.heartbeats_received(), 3);
}

#[test]
fn health_timeout_detection() {
    let h = ProtocolHealth::new(Duration::from_millis(10), Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(5));
    assert!(h.is_timed_out());
}

#[test]
fn health_heartbeat_resets_timeout() {
    let mut h = ProtocolHealth::new(Duration::from_secs(60), Duration::from_secs(120));
    assert!(!h.is_timed_out());
    h.record_heartbeat();
    assert!(!h.is_timed_out());
}

#[test]
fn health_time_since_last_heartbeat() {
    let h = ProtocolHealth::with_defaults();
    let elapsed = h.time_since_last_heartbeat();
    // Just created, should be very small
    assert!(elapsed < Duration::from_secs(1));
}

#[test]
fn health_shutdown_signaling() {
    let h = ProtocolHealth::with_defaults();
    assert!(!h.is_shutdown_signalled());
    h.signal_shutdown();
    assert!(h.is_shutdown_signalled());
}

#[test]
fn health_shutdown_receiver() {
    let h = ProtocolHealth::with_defaults();
    let mut rx = h.shutdown_receiver();
    assert!(!*rx.borrow_and_update());
    h.signal_shutdown();
    assert!(*rx.borrow_and_update());
}

#[test]
fn health_multiple_shutdown_signals_idempotent() {
    let h = ProtocolHealth::with_defaults();
    h.signal_shutdown();
    h.signal_shutdown();
    assert!(h.is_shutdown_signalled());
}

#[test]
fn health_heartbeat_overdue_detection() {
    let h = ProtocolHealth::new(Duration::from_millis(1), Duration::from_secs(60));
    std::thread::sleep(Duration::from_millis(5));
    assert!(h.is_heartbeat_overdue());
    assert!(!h.is_timed_out());
}

#[test]
fn health_not_overdue_initially() {
    let h = ProtocolHealth::new(Duration::from_secs(60), Duration::from_secs(120));
    assert!(!h.is_heartbeat_overdue());
}

#[tokio::test]
async fn health_wait_for_next_heartbeat_returns() {
    let h = ProtocolHealth::new(Duration::from_millis(10), Duration::from_secs(60));
    // Should return quickly since the interval is only 10ms
    h.wait_for_next_heartbeat().await;
}

// =========================================================================
// Module: handshake
// =========================================================================

#[test]
fn handshake_default_timeout_constant() {
    assert_eq!(DEFAULT_HANDSHAKE_TIMEOUT, Duration::from_secs(10));
}

#[tokio::test]
async fn handshake_await_hello_success() {
    let hello = make_hello_line(CONTRACT_VERSION);
    let reader = BufReader::new(hello.as_bytes());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.backend.id, "test-sidecar");
    assert_eq!(info.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn handshake_await_hello_incompatible_version() {
    let hello = make_hello_line("abp/v99.0");
    let reader = BufReader::new(hello.as_bytes());
    let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::IncompatibleVersion { .. }));
}

#[tokio::test]
async fn handshake_await_hello_unexpected_message() {
    let fatal = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"nope\"}\n";
    let reader = BufReader::new(fatal.as_bytes());
    let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::UnexpectedMessage(_)));
}

#[tokio::test]
async fn handshake_await_hello_peer_closed() {
    let reader = BufReader::new(&b""[..]);
    let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::PeerClosed));
}

#[tokio::test]
async fn handshake_await_hello_timeout() {
    let (reader, _writer) = tokio::io::duplex(64);
    let reader = BufReader::new(reader);
    let err = HandshakeManager::await_hello(reader, Duration::from_millis(50))
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::Timeout(_)));
}

#[tokio::test]
async fn handshake_send_hello_roundtrip() {
    let mut buf = Vec::new();
    let backend = BackendIdentity {
        id: "roundtrip".into(),
        backend_version: None,
        adapter_version: None,
    };
    HandshakeManager::send_hello(&mut buf, backend, CapabilityManifest::new())
        .await
        .unwrap();

    let reader = BufReader::new(buf.as_slice());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.backend.id, "roundtrip");
}

#[tokio::test]
async fn handshake_send_hello_with_capabilities() {
    let mut buf = Vec::new();
    let backend = BackendIdentity {
        id: "cap-test".into(),
        backend_version: Some("1.0".into()),
        adapter_version: None,
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    HandshakeManager::send_hello(&mut buf, backend, caps)
        .await
        .unwrap();

    let reader = BufReader::new(buf.as_slice());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.backend.id, "cap-test");
    assert!(info.capabilities.contains_key(&Capability::Streaming));
}

#[tokio::test]
async fn handshake_hello_info_fields() {
    let hello = make_hello_line(CONTRACT_VERSION);
    let reader = BufReader::new(hello.as_bytes());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.contract_version, CONTRACT_VERSION);
    assert_eq!(info.backend.backend_version, Some("1.0".to_string()));
    assert!(info.capabilities.is_empty());
    assert_eq!(info.mode, ExecutionMode::default());
}

#[tokio::test]
async fn handshake_protocol_error_on_invalid_json() {
    let bad = "this is not json\n";
    let reader = BufReader::new(bad.as_bytes());
    let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::Protocol(_)));
}

// =========================================================================
// Integration / cross-module tests
// =========================================================================

#[test]
fn integration_codec_feeds_event_stream() {
    let mut codec = StreamingCodec::new();
    let mut proc = EventStreamProcessor::new("run-1".into());

    let event = make_agent_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let line = encode_event("run-1", &event);
    let envs = codec.push(line.as_bytes());
    assert_eq!(envs.len(), 1);

    let result = proc.process_envelope(&envs[0]).unwrap();
    assert!(result.is_some());
    assert_eq!(proc.stats().events_processed, 1);
}

#[test]
fn integration_full_protocol_sequence() {
    let mut codec = StreamingCodec::new();

    // Build full sequence as JSONL lines
    let hello_line = encode_hello("test-be", "1.0", &[]);
    let event = make_agent_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let event_line = encode_event("run-1", &event);
    let receipt = ReceiptBuilder::new("test-be")
        .outcome(Outcome::Complete)
        .build();
    let final_line = encode_final("run-1", &receipt);

    let all_lines = format!("{hello_line}{event_line}{final_line}");
    let envs = codec.push(all_lines.as_bytes());
    assert_eq!(envs.len(), 3);

    // Validate the sequence
    assert!(validate_sequence(&envs).is_ok());
    assert!(validate_hello(&envs[0]).is_ok());
    assert!(validate_ref_id(&envs[1], "run-1").is_ok());
    assert!(validate_ref_id(&envs[2], "run-1").is_ok());
}

#[test]
fn integration_mock_helpers_validate() {
    let seq = vec![
        mock_hello("test"),
        mock_event("r1", "hi"),
        mock_event("r1", "there"),
        mock_final("r1"),
    ];
    assert!(validate_sequence(&seq).is_ok());
    assert!(validate_hello(&seq[0]).is_ok());
}

#[test]
fn integration_fatal_sequence_validates() {
    let seq = vec![mock_hello("test"), mock_fatal("r1", "err")];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn integration_event_stream_with_codec_and_validation() {
    let mut codec = StreamingCodec::new();
    let mut proc = EventStreamProcessor::new("run-1".into());

    let event = make_agent_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    let event_line = encode_event("run-1", &event);
    let final_line = encode_fatal("run-1", "something broke");

    let all = format!("{event_line}{final_line}");
    let envs = codec.push(all.as_bytes());
    assert_eq!(envs.len(), 2);

    // Process through event stream
    let r1 = proc.process_envelope(&envs[0]).unwrap();
    assert!(r1.is_some());

    let r2 = proc.process_envelope(&envs[1]).unwrap();
    assert!(r2.is_none());
    assert!(proc.is_terminal());
}

#[test]
fn integration_encode_decode_all_envelope_types() {
    // Hello
    let hello = mock_hello("be");
    let encoded = encode_envelope(&hello).unwrap();
    let decoded = decode_envelope(&encoded).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));

    // Event
    let event = mock_event("r1", "text");
    let encoded = encode_envelope(&event).unwrap();
    let decoded = decode_envelope(&encoded).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));

    // Final
    let final_env = mock_final("r1");
    let encoded = encode_envelope(&final_env).unwrap();
    let decoded = decode_envelope(&encoded).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));

    // Fatal
    let fatal = mock_fatal("r1", "err");
    let encoded = encode_envelope(&fatal).unwrap();
    let decoded = decode_envelope(&encoded).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn integration_work_order_roundtrip() {
    let line = mock_work_order("test task");
    let env = decode_envelope(&line).unwrap();
    match env {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run"),
    }
}
