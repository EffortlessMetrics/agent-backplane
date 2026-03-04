#![allow(clippy::all)]
#![allow(clippy::useless_vec)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for `abp-sidecar-utils`.

use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};
use abp_sidecar_utils::codec::{StreamingCodec, DEFAULT_MAX_LINE_LEN};
use abp_sidecar_utils::event_stream::{EventStreamError, EventStreamProcessor};
use abp_sidecar_utils::frame::{
    backend_identity, contract_version, decode_envelope, encode_envelope, encode_event,
    encode_fatal, encode_final, encode_hello,
};
use abp_sidecar_utils::handshake::{
    HandshakeError, HandshakeManager, DEFAULT_HANDSHAKE_TIMEOUT,
};
use abp_sidecar_utils::health::{
    ProtocolHealth, DEFAULT_CONNECTION_TIMEOUT, DEFAULT_HEARTBEAT_INTERVAL,
};
use abp_sidecar_utils::testing::{mock_event, mock_fatal, mock_final, mock_hello, mock_work_order};
use abp_sidecar_utils::validate::{validate_hello, validate_ref_id, validate_sequence};
use chrono::Utc;
use tokio::io::BufReader;

// ========================================================================
// Helper functions
// ========================================================================

fn fatal_json(msg: &str) -> String {
    format!("{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{msg}\"}}\n")
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
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

// ========================================================================
// Module: codec — StreamingCodec
// ========================================================================

#[test]
fn codec_new_default_metrics() {
    let codec = StreamingCodec::new();
    let m = codec.metrics();
    assert_eq!(m.bytes_read, 0);
    assert_eq!(m.lines_parsed, 0);
    assert_eq!(m.errors_skipped, 0);
    assert_eq!(codec.buffered_len(), 0);
}

#[test]
fn codec_default_trait() {
    let codec = StreamingCodec::default();
    assert_eq!(codec.buffered_len(), 0);
    assert_eq!(codec.metrics().lines_parsed, 0);
}

#[test]
fn codec_with_max_line_len_respects_limit() {
    let mut codec = StreamingCodec::with_max_line_len(50);
    let short = fatal_json("ok");
    let envs = codec.push(short.as_bytes());
    assert_eq!(envs.len(), 1);

    // Build a line that exceeds 50 bytes
    let long_msg = "a".repeat(100);
    let long_line = format!("{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{long_msg}\"}}\n");
    let envs = codec.push(long_line.as_bytes());
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn codec_default_max_line_len_constant() {
    assert_eq!(DEFAULT_MAX_LINE_LEN, 10 * 1024 * 1024);
}

#[test]
fn codec_push_empty_bytes() {
    let mut codec = StreamingCodec::new();
    let envs = codec.push(b"");
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().bytes_read, 0);
}

#[test]
fn codec_push_only_newlines() {
    let mut codec = StreamingCodec::new();
    let envs = codec.push(b"\n\n\n");
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().bytes_read, 3);
    assert_eq!(codec.metrics().lines_parsed, 0);
    assert_eq!(codec.metrics().errors_skipped, 0);
}

#[test]
fn codec_push_single_complete_line() {
    let mut codec = StreamingCodec::new();
    let line = fatal_json("test");
    let envs = codec.push(line.as_bytes());
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.metrics().lines_parsed, 1);
    assert_eq!(codec.metrics().bytes_read, line.len() as u64);
}

#[test]
fn codec_push_multiple_complete_lines() {
    let mut codec = StreamingCodec::new();
    let data = format!("{}{}{}", fatal_json("a"), fatal_json("b"), fatal_json("c"));
    let envs = codec.push(data.as_bytes());
    assert_eq!(envs.len(), 3);
    assert_eq!(codec.metrics().lines_parsed, 3);
}

#[test]
fn codec_push_partial_then_complete() {
    let mut codec = StreamingCodec::new();
    let line = fatal_json("chunked");
    let bytes = line.as_bytes();
    let mid = bytes.len() / 2;

    let envs = codec.push(&bytes[..mid]);
    assert!(envs.is_empty());
    assert!(codec.buffered_len() > 0);

    let envs = codec.push(&bytes[mid..]);
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.buffered_len(), 0);
}

#[test]
fn codec_push_byte_at_a_time() {
    let mut codec = StreamingCodec::new();
    let line = fatal_json("byte");
    let bytes = line.as_bytes();

    let mut total_envs = 0;
    for &b in bytes {
        total_envs += codec.push(&[b]).len();
    }
    assert_eq!(total_envs, 1);
    assert_eq!(codec.metrics().lines_parsed, 1);
}

#[test]
fn codec_push_malformed_json_skipped() {
    let mut codec = StreamingCodec::new();
    let data = format!("{{not valid}}\n{}", fatal_json("ok"));
    let envs = codec.push(data.as_bytes());
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.metrics().errors_skipped, 1);
    assert_eq!(codec.metrics().lines_parsed, 1);
}

#[test]
fn codec_push_non_utf8_skipped() {
    let mut codec = StreamingCodec::new();
    let mut data = vec![0xFF, 0xFE, 0xFD, b'\n'];
    data.extend_from_slice(fatal_json("after-binary").as_bytes());
    let envs = codec.push(&data);
    assert_eq!(envs.len(), 1);
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn codec_push_blank_lines_between_valid() {
    let mut codec = StreamingCodec::new();
    let data = format!("\n{}\n\n{}\n", fatal_json("a").trim(), fatal_json("b").trim());
    let envs = codec.push(data.as_bytes());
    assert_eq!(envs.len(), 2);
    assert_eq!(codec.metrics().errors_skipped, 0);
}

#[test]
fn codec_finish_flushes_unterminated_line() {
    let mut codec = StreamingCodec::new();
    let partial = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"eof\"}";
    codec.push(partial.as_bytes());
    assert!(codec.buffered_len() > 0);

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
fn codec_finish_already_terminated() {
    let mut codec = StreamingCodec::new();
    codec.push(fatal_json("done").as_bytes());
    let envs = codec.finish();
    assert!(envs.is_empty());
}

#[test]
fn codec_reset_clears_everything() {
    let mut codec = StreamingCodec::new();
    codec.push(fatal_json("x").as_bytes());
    // push partial data
    codec.push(b"partial data without newline");

    assert!(codec.metrics().lines_parsed > 0 || codec.buffered_len() > 0);

    codec.reset();
    assert_eq!(codec.buffered_len(), 0);
    assert_eq!(codec.metrics().lines_parsed, 0);
    assert_eq!(codec.metrics().bytes_read, 0);
    assert_eq!(codec.metrics().errors_skipped, 0);
}

#[test]
fn codec_metrics_accumulate_across_pushes() {
    let mut codec = StreamingCodec::new();
    codec.push(fatal_json("a").as_bytes());
    codec.push(fatal_json("b").as_bytes());
    codec.push(b"bad json\n");
    assert_eq!(codec.metrics().lines_parsed, 2);
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn codec_large_valid_line_within_default_limit() {
    let mut codec = StreamingCodec::new();
    let big_text = "x".repeat(1000);
    let line = format!("{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{big_text}\"}}\n");
    let envs = codec.push(line.as_bytes());
    assert_eq!(envs.len(), 1);
}

#[test]
fn codec_oversized_line_with_tiny_limit() {
    let mut codec = StreamingCodec::with_max_line_len(5);
    let line = fatal_json("toolong");
    let envs = codec.push(line.as_bytes());
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn codec_interleaved_valid_and_invalid() {
    let mut codec = StreamingCodec::new();
    let data = format!(
        "{}invalid line\n{}also bad\n{}",
        fatal_json("one"),
        fatal_json("two"),
        fatal_json("three")
    );
    let envs = codec.push(data.as_bytes());
    assert_eq!(envs.len(), 3);
    assert_eq!(codec.metrics().errors_skipped, 2);
}

#[test]
fn codec_whitespace_only_lines_ignored() {
    let mut codec = StreamingCodec::new();
    let data = "   \n\t\n  \t  \n";
    let envs = codec.push(data.as_bytes());
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().errors_skipped, 0);
}

// ========================================================================
// Module: event_stream — EventStreamProcessor
// ========================================================================

#[test]
fn esp_new_initial_state() {
    let proc = EventStreamProcessor::new("run-1".into());
    assert!(!proc.is_terminal());
    assert_eq!(proc.stats().events_processed, 0);
    assert_eq!(proc.stats().ref_id_mismatches, 0);
    assert_eq!(proc.stats().events_after_terminal, 0);
    assert!(proc.stats().counts_by_type.is_empty());
}

#[test]
fn esp_process_valid_event() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let env = mock_event("run-1", "hello");
    let result = proc.process_envelope(&env).unwrap();
    assert!(result.is_some());
    let event = result.unwrap();
    assert!(matches!(
        event.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert_eq!(proc.stats().events_processed, 1);
}

#[test]
fn esp_process_multiple_events() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    for i in 0..10 {
        let env = mock_event("run-1", &format!("msg-{i}"));
        proc.process_envelope(&env).unwrap();
    }
    assert_eq!(proc.stats().events_processed, 10);
    assert_eq!(
        proc.stats().counts_by_type.get("assistant_message"),
        Some(&10)
    );
}

#[test]
fn esp_ref_id_mismatch_returns_error() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let env = mock_event("run-OTHER", "msg");
    let err = proc.process_envelope(&env).unwrap_err();
    assert!(matches!(err, EventStreamError::RefIdMismatch { .. }));
    if let EventStreamError::RefIdMismatch { expected, got } = err {
        assert_eq!(expected, "run-1");
        assert_eq!(got, "run-OTHER");
    }
    assert_eq!(proc.stats().ref_id_mismatches, 1);
    assert_eq!(proc.stats().events_processed, 0);
}

#[test]
fn esp_multiple_ref_id_mismatches() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    for _ in 0..5 {
        let _ = proc.process_envelope(&mock_event("wrong", "msg"));
    }
    assert_eq!(proc.stats().ref_id_mismatches, 5);
}

#[test]
fn esp_final_marks_terminal() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let fin = mock_final("run-1");
    let result = proc.process_envelope(&fin).unwrap();
    assert!(result.is_none());
    assert!(proc.is_terminal());
}

#[test]
fn esp_fatal_marks_terminal() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let fat = mock_fatal("run-1", "error");
    let result = proc.process_envelope(&fat).unwrap();
    assert!(result.is_none());
    assert!(proc.is_terminal());
}

#[test]
fn esp_fatal_without_ref_id_marks_terminal() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let fat = Envelope::Fatal {
        ref_id: None,
        error: "crash".into(),
        error_code: None,
    };
    let result = proc.process_envelope(&fat).unwrap();
    assert!(result.is_none());
    assert!(proc.is_terminal());
}

#[test]
fn esp_event_after_final_returns_error() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    proc.process_envelope(&mock_final("run-1")).unwrap();
    let err = proc
        .process_envelope(&mock_event("run-1", "late"))
        .unwrap_err();
    assert!(matches!(err, EventStreamError::EventAfterTerminal));
    assert_eq!(proc.stats().events_after_terminal, 1);
}

#[test]
fn esp_event_after_fatal_returns_error() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    proc.process_envelope(&mock_fatal("run-1", "fail")).unwrap();
    let err = proc
        .process_envelope(&mock_event("run-1", "late"))
        .unwrap_err();
    assert!(matches!(err, EventStreamError::EventAfterTerminal));
}

#[test]
fn esp_hello_returns_unexpected() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let hello = mock_hello("backend");
    let err = proc.process_envelope(&hello).unwrap_err();
    assert!(matches!(err, EventStreamError::UnexpectedEnvelope(_)));
    if let EventStreamError::UnexpectedEnvelope(name) = err {
        assert_eq!(name, "hello");
    }
}

#[test]
fn esp_run_returns_unexpected() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let wo = WorkOrderBuilder::new("task").build();
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let err = proc.process_envelope(&run).unwrap_err();
    assert!(matches!(err, EventStreamError::UnexpectedEnvelope(_)));
    if let EventStreamError::UnexpectedEnvelope(name) = err {
        assert_eq!(name, "run");
    }
}

#[test]
fn esp_counts_by_type_tracks_different_kinds() {
    let mut proc = EventStreamProcessor::new("run-1".into());

    let msg_env = Envelope::Event {
        ref_id: "run-1".into(),
        event: make_agent_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        }),
    };
    let delta_env = Envelope::Event {
        ref_id: "run-1".into(),
        event: make_agent_event(AgentEventKind::AssistantDelta {
            text: "tok".into(),
        }),
    };
    let started_env = Envelope::Event {
        ref_id: "run-1".into(),
        event: make_agent_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };

    proc.process_envelope(&msg_env).unwrap();
    proc.process_envelope(&delta_env).unwrap();
    proc.process_envelope(&delta_env).unwrap();
    proc.process_envelope(&started_env).unwrap();

    assert_eq!(
        proc.stats().counts_by_type.get("assistant_message"),
        Some(&1)
    );
    assert_eq!(
        proc.stats().counts_by_type.get("assistant_delta"),
        Some(&2)
    );
    assert_eq!(proc.stats().counts_by_type.get("run_started"), Some(&1));
    assert_eq!(proc.stats().events_processed, 4);
}

#[test]
fn esp_process_many_all_valid() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let envs = vec![
        mock_event("run-1", "a"),
        mock_event("run-1", "b"),
        mock_event("run-1", "c"),
    ];
    let results = proc.process_many(&envs);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_ok()));
    assert_eq!(proc.stats().events_processed, 3);
}

#[test]
fn esp_process_many_with_errors() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let envs = vec![
        mock_event("run-1", "ok"),
        mock_event("run-WRONG", "bad"),
        mock_event("run-1", "ok2"),
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
fn esp_process_many_empty() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let results = proc.process_many(&[]);
    assert!(results.is_empty());
}

#[test]
fn esp_process_many_with_terminal_mid_stream() {
    let mut proc = EventStreamProcessor::new("run-1".into());
    let envs = vec![
        mock_event("run-1", "before"),
        mock_final("run-1"),
        mock_event("run-1", "after"),
    ];
    let results = proc.process_many(&envs);
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());
    assert!(results[2].is_err());
}

#[test]
fn esp_error_display_messages() {
    let mismatch = EventStreamError::RefIdMismatch {
        expected: "a".into(),
        got: "b".into(),
    };
    assert!(mismatch.to_string().contains("a"));
    assert!(mismatch.to_string().contains("b"));

    let after_terminal = EventStreamError::EventAfterTerminal;
    assert!(after_terminal.to_string().contains("terminal"));

    let unexpected = EventStreamError::UnexpectedEnvelope("hello".into());
    assert!(unexpected.to_string().contains("hello"));
}

// ========================================================================
// Module: frame — encode/decode helpers
// ========================================================================

#[test]
fn frame_encode_hello_basic() {
    let line = encode_hello("test-backend", "2.0", &["streaming"]);
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"hello\""));
    assert!(line.contains("test-backend"));
    assert!(line.contains(CONTRACT_VERSION));
}

#[test]
fn frame_encode_hello_no_capabilities() {
    let line = encode_hello("backend", "1.0", &[]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { capabilities, .. } = env {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn frame_encode_hello_multiple_valid_capabilities() {
    let line = encode_hello("b", "1.0", &["streaming", "tool_read", "tool_write"]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { capabilities, .. } = env {
        assert_eq!(capabilities.len(), 3);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn frame_encode_hello_unknown_caps_silently_skipped() {
    let line = encode_hello("b", "1.0", &["streaming", "does_not_exist_xyz"]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { capabilities, .. } = env {
        assert_eq!(capabilities.len(), 1);
        assert!(capabilities.contains_key(&Capability::Streaming));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn frame_encode_hello_all_unknown_caps() {
    let line = encode_hello("b", "1.0", &["fake1", "fake2", "fake3"]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { capabilities, .. } = env {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn frame_encode_hello_empty_backend_name() {
    let line = encode_hello("", "1.0", &[]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { backend, .. } = env {
        assert_eq!(backend.id, "");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn frame_encode_event_roundtrip() {
    let event = make_agent_event(AgentEventKind::AssistantMessage {
        text: "test msg".into(),
    });
    let line = encode_event("run-99", &event);
    assert!(line.ends_with('\n'));

    let env = decode_envelope(&line).unwrap();
    if let Envelope::Event { ref_id, event: e } = env {
        assert_eq!(ref_id, "run-99");
        assert!(matches!(
            e.kind,
            AgentEventKind::AssistantMessage { ref text } if text == "test msg"
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn frame_encode_event_with_tool_call() {
    let event = make_agent_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tool-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/tmp/file"}),
    });
    let line = encode_event("run-1", &event);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Event { event: e, .. } = env {
        assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn frame_encode_event_with_empty_text() {
    let event = make_agent_event(AgentEventKind::AssistantMessage { text: "".into() });
    let line = encode_event("run-1", &event);
    let env = decode_envelope(&line).unwrap();
    assert!(matches!(env, Envelope::Event { .. }));
}

#[test]
fn frame_encode_final_roundtrip() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();
    let line = encode_final("run-42", &receipt);
    assert!(line.ends_with('\n'));

    let env = decode_envelope(&line).unwrap();
    if let Envelope::Final { ref_id, receipt: r } = env {
        assert_eq!(ref_id, "run-42");
        assert_eq!(r.outcome, Outcome::Complete);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn frame_encode_final_with_partial_outcome() {
    let receipt = ReceiptBuilder::new("backend")
        .outcome(Outcome::Partial)
        .build();
    let line = encode_final("run-1", &receipt);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Final { receipt: r, .. } = env {
        assert_eq!(r.outcome, Outcome::Partial);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn frame_encode_final_with_failed_outcome() {
    let receipt = ReceiptBuilder::new("backend")
        .outcome(Outcome::Failed)
        .build();
    let line = encode_final("run-1", &receipt);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Final { receipt: r, .. } = env {
        assert_eq!(r.outcome, Outcome::Failed);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn frame_encode_fatal_roundtrip() {
    let line = encode_fatal("run-10", "out of memory");
    assert!(line.ends_with('\n'));
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = env
    {
        assert_eq!(ref_id.as_deref(), Some("run-10"));
        assert_eq!(error, "out of memory");
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn frame_encode_fatal_empty_error() {
    let line = encode_fatal("run-1", "");
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Fatal { error, .. } = env {
        assert_eq!(error, "");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn frame_encode_fatal_unicode_error() {
    let line = encode_fatal("run-1", "错误：内存不足 🚨");
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Fatal { error, .. } = env {
        assert_eq!(error, "错误：内存不足 🚨");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn frame_decode_envelope_trims_whitespace() {
    let line = encode_fatal("r", "err");
    let padded = format!("   {}   ", line.trim());
    let env = decode_envelope(&padded).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn frame_decode_envelope_invalid_json() {
    assert!(decode_envelope("not json").is_err());
    assert!(decode_envelope("").is_err());
    assert!(decode_envelope("{}").is_err());
    assert!(decode_envelope("{\"t\":\"unknown_type\"}").is_err());
}

#[test]
fn frame_decode_envelope_with_newline() {
    let line = fatal_json("test");
    let env = decode_envelope(&line).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn frame_encode_envelope_generic() {
    let hello = mock_hello("test");
    let line = encode_envelope(&hello).unwrap();
    assert!(line.ends_with('\n'));
    let decoded = decode_envelope(&line).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn frame_backend_identity_helper() {
    let bi = backend_identity("my-backend", "3.0");
    assert_eq!(bi.id, "my-backend");
    assert_eq!(bi.backend_version.as_deref(), Some("3.0"));
    assert!(bi.adapter_version.is_none());
}

#[test]
fn frame_backend_identity_empty_strings() {
    let bi = backend_identity("", "");
    assert_eq!(bi.id, "");
    assert_eq!(bi.backend_version.as_deref(), Some(""));
}

#[test]
fn frame_contract_version_matches_core() {
    assert_eq!(contract_version(), CONTRACT_VERSION);
}

// ========================================================================
// Module: handshake — HandshakeManager
// ========================================================================

#[tokio::test]
async fn handshake_await_hello_success() {
    let line = make_hello_line(CONTRACT_VERSION);
    let reader = BufReader::new(line.as_bytes());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.backend.id, "test-sidecar");
    assert_eq!(info.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn handshake_await_hello_incompatible_version() {
    let line = make_hello_line("abp/v99.0");
    let reader = BufReader::new(line.as_bytes());
    let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::IncompatibleVersion { .. }));
    let msg = err.to_string();
    assert!(msg.contains("incompatible"));
    assert!(msg.contains("abp/v99.0"));
}

#[tokio::test]
async fn handshake_await_hello_non_hello_message() {
    let fatal = fatal_json("nope");
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
    assert!(err.to_string().contains("closed"));
}

#[tokio::test]
async fn handshake_await_hello_timeout() {
    let (reader, _writer) = tokio::io::duplex(64);
    let reader = BufReader::new(reader);
    let timeout = Duration::from_millis(50);
    let err = HandshakeManager::await_hello(reader, timeout)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::Timeout(_)));
    assert!(err.to_string().contains("timed out"));
}

#[tokio::test]
async fn handshake_await_hello_invalid_json() {
    let data = b"this is not json\n";
    let reader = BufReader::new(&data[..]);
    let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::Protocol(_)));
}

#[tokio::test]
async fn handshake_send_hello_roundtrip() {
    let mut buf = Vec::new();
    let backend = BackendIdentity {
        id: "roundtrip-test".into(),
        backend_version: Some("1.0".into()),
        adapter_version: None,
    };
    HandshakeManager::send_hello(&mut buf, backend, CapabilityManifest::new())
        .await
        .unwrap();

    let reader = BufReader::new(buf.as_slice());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.backend.id, "roundtrip-test");
    assert_eq!(info.backend.backend_version.as_deref(), Some("1.0"));
}

#[tokio::test]
async fn handshake_send_hello_with_capabilities() {
    let mut buf = Vec::new();
    let backend = BackendIdentity {
        id: "cap-test".into(),
        backend_version: None,
        adapter_version: None,
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    HandshakeManager::send_hello(&mut buf, backend, caps)
        .await
        .unwrap();

    let reader = BufReader::new(buf.as_slice());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.capabilities.len(), 2);
}

#[tokio::test]
async fn handshake_default_timeout_constant() {
    assert_eq!(DEFAULT_HANDSHAKE_TIMEOUT, Duration::from_secs(10));
}

#[tokio::test]
async fn handshake_hello_info_fields() {
    let line = make_hello_line(CONTRACT_VERSION);
    let reader = BufReader::new(line.as_bytes());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();

    assert_eq!(info.contract_version, CONTRACT_VERSION);
    assert_eq!(info.backend.id, "test-sidecar");
    assert_eq!(info.backend.backend_version.as_deref(), Some("1.0"));
    assert!(info.capabilities.is_empty());
    assert_eq!(info.mode, ExecutionMode::default());
}

#[tokio::test]
async fn handshake_await_hello_with_event_envelope() {
    let event_line = encode_event(
        "run-1",
        &make_agent_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        }),
    );
    let reader = BufReader::new(event_line.as_bytes());
    let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(err, HandshakeError::UnexpectedMessage(_)));
}

// ========================================================================
// Module: health — ProtocolHealth
// ========================================================================

#[test]
fn health_initial_state() {
    let h = ProtocolHealth::with_defaults();
    assert!(!h.is_timed_out());
    assert!(!h.is_heartbeat_overdue());
    assert_eq!(h.heartbeats_received(), 0);
    assert!(!h.is_shutdown_signalled());
}

#[test]
fn health_custom_intervals() {
    let h = ProtocolHealth::new(Duration::from_secs(15), Duration::from_secs(45));
    assert_eq!(h.heartbeat_interval(), Duration::from_secs(15));
    assert_eq!(h.connection_timeout(), Duration::from_secs(45));
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
fn health_record_heartbeat_increments() {
    let mut h = ProtocolHealth::with_defaults();
    for i in 1..=5 {
        h.record_heartbeat();
        assert_eq!(h.heartbeats_received(), i);
    }
}

#[test]
fn health_record_heartbeat_resets_timer() {
    let mut h = ProtocolHealth::new(Duration::from_secs(60), Duration::from_secs(120));
    std::thread::sleep(Duration::from_millis(5));
    h.record_heartbeat();
    assert!(h.time_since_last_heartbeat() < Duration::from_millis(50));
}

#[test]
fn health_timeout_detection_immediate_tiny_timeout() {
    let h = ProtocolHealth::new(Duration::from_millis(1), Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(5));
    assert!(h.is_timed_out());
}

#[test]
fn health_not_timed_out_with_large_timeout() {
    let h = ProtocolHealth::new(Duration::from_secs(600), Duration::from_secs(600));
    assert!(!h.is_timed_out());
}

#[test]
fn health_heartbeat_overdue_between_interval_and_timeout() {
    let h = ProtocolHealth::new(Duration::from_millis(1), Duration::from_secs(600));
    std::thread::sleep(Duration::from_millis(5));
    assert!(h.is_heartbeat_overdue());
    assert!(!h.is_timed_out());
}

#[test]
fn health_not_overdue_immediately() {
    let h = ProtocolHealth::new(Duration::from_secs(600), Duration::from_secs(1200));
    assert!(!h.is_heartbeat_overdue());
}

#[test]
fn health_shutdown_signaling() {
    let h = ProtocolHealth::with_defaults();
    assert!(!h.is_shutdown_signalled());
    h.signal_shutdown();
    assert!(h.is_shutdown_signalled());
}

#[test]
fn health_shutdown_receiver_gets_update() {
    let h = ProtocolHealth::with_defaults();
    let mut rx = h.shutdown_receiver();
    assert!(!*rx.borrow());
    h.signal_shutdown();
    assert!(*rx.borrow_and_update());
}

#[test]
fn health_multiple_shutdown_receivers() {
    let h = ProtocolHealth::with_defaults();
    let rx1 = h.shutdown_receiver();
    let rx2 = h.shutdown_receiver();
    h.signal_shutdown();
    assert!(*rx1.borrow());
    assert!(*rx2.borrow());
}

#[test]
fn health_double_shutdown_signal() {
    let h = ProtocolHealth::with_defaults();
    h.signal_shutdown();
    h.signal_shutdown();
    assert!(h.is_shutdown_signalled());
}

#[test]
fn health_time_since_last_heartbeat_grows() {
    let h = ProtocolHealth::with_defaults();
    let t1 = h.time_since_last_heartbeat();
    std::thread::sleep(Duration::from_millis(10));
    let t2 = h.time_since_last_heartbeat();
    assert!(t2 > t1);
}

#[tokio::test]
async fn health_wait_for_next_heartbeat_returns_quickly() {
    let h = ProtocolHealth::new(Duration::from_millis(10), Duration::from_secs(60));
    let start = std::time::Instant::now();
    h.wait_for_next_heartbeat().await;
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn health_wait_for_next_heartbeat_after_elapsed() {
    let h = ProtocolHealth::new(Duration::from_millis(1), Duration::from_secs(60));
    std::thread::sleep(Duration::from_millis(5));
    // Should return immediately since interval already elapsed
    let start = std::time::Instant::now();
    h.wait_for_next_heartbeat().await;
    assert!(start.elapsed() < Duration::from_millis(50));
}

// ========================================================================
// Module: testing — mock helpers
// ========================================================================

#[test]
fn mock_hello_contains_contract_version() {
    let env = mock_hello("backend-x");
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn mock_hello_backend_id_preserved() {
    for name in &["a", "my-backend", "backend-with-dashes", "x".repeat(100).as_str()] {
        let env = mock_hello(name);
        if let Envelope::Hello { backend, .. } = &env {
            assert_eq!(backend.id, *name);
        }
    }
}

#[test]
fn mock_event_ref_id_and_text() {
    let env = mock_event("run-abc", "some text");
    if let Envelope::Event { ref_id, event } = &env {
        assert_eq!(ref_id, "run-abc");
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "some text");
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn mock_event_empty_text() {
    let env = mock_event("r", "");
    if let Envelope::Event { event, .. } = &env {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "");
        }
    }
}

#[test]
fn mock_event_unicode_text() {
    let env = mock_event("r", "日本語テスト 🎉");
    if let Envelope::Event { event, .. } = &env {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "日本語テスト 🎉");
        }
    }
}

#[test]
fn mock_final_has_complete_outcome() {
    let env = mock_final("run-5");
    if let Envelope::Final { ref_id, receipt } = &env {
        assert_eq!(ref_id, "run-5");
        assert_eq!(receipt.outcome, Outcome::Complete);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn mock_fatal_has_ref_id_and_error() {
    let env = mock_fatal("run-6", "kaboom");
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = &env
    {
        assert_eq!(ref_id.as_deref(), Some("run-6"));
        assert_eq!(error, "kaboom");
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn mock_work_order_contains_task() {
    let line = mock_work_order("implement feature X");
    assert!(line.ends_with('\n'));
    assert!(line.contains("implement feature X"));
    assert!(line.contains("\"t\":\"run\""));
}

#[test]
fn mock_work_order_unique_ids_across_calls() {
    let ids: Vec<String> = (0..10)
        .map(|i| {
            let line = mock_work_order(&format!("task-{i}"));
            let env = JsonlCodec::decode(line.trim()).unwrap();
            if let Envelope::Run { id, .. } = env {
                id
            } else {
                panic!("expected Run");
            }
        })
        .collect();
    // All IDs should be unique
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn mock_work_order_roundtrip_jsonl() {
    let line = mock_work_order("test task");
    let env = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(env, Envelope::Run { .. }));
    if let Envelope::Run { work_order, .. } = env {
        assert!(work_order.task.contains("test task"));
    }
}

// ========================================================================
// Module: validate — validation helpers
// ========================================================================

#[test]
fn validate_hello_accepts_valid_hello() {
    let hello = mock_hello("test");
    assert!(validate_hello(&hello).is_ok());
}

#[test]
fn validate_hello_rejects_event() {
    let event = mock_event("r", "text");
    let err = validate_hello(&event).unwrap_err();
    assert!(err.to_string().contains("event"));
}

#[test]
fn validate_hello_rejects_final() {
    let fin = mock_final("r");
    let err = validate_hello(&fin).unwrap_err();
    assert!(err.to_string().contains("final"));
}

#[test]
fn validate_hello_rejects_fatal() {
    let fat = mock_fatal("r", "err");
    let err = validate_hello(&fat).unwrap_err();
    assert!(err.to_string().contains("fatal"));
}

#[test]
fn validate_hello_rejects_incompatible_version() {
    let hello = Envelope::Hello {
        contract_version: "abp/v99.0".into(),
        backend: BackendIdentity {
            id: "bad".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = validate_hello(&hello).unwrap_err();
    assert!(err.to_string().contains("incompatible"));
}

#[test]
fn validate_hello_rejects_empty_version() {
    let hello = Envelope::Hello {
        contract_version: "".into(),
        backend: BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = validate_hello(&hello).unwrap_err();
    assert!(err.to_string().contains("incompatible"));
}

#[test]
fn validate_ref_id_match_event() {
    let env = mock_event("run-1", "text");
    assert!(validate_ref_id(&env, "run-1").is_ok());
}

#[test]
fn validate_ref_id_mismatch_event() {
    let env = mock_event("run-1", "text");
    let err = validate_ref_id(&env, "run-2").unwrap_err();
    assert!(err.to_string().contains("mismatch"));
    assert!(err.to_string().contains("run-1"));
    assert!(err.to_string().contains("run-2"));
}

#[test]
fn validate_ref_id_match_final() {
    let fin = mock_final("run-1");
    assert!(validate_ref_id(&fin, "run-1").is_ok());
}

#[test]
fn validate_ref_id_mismatch_final() {
    let fin = mock_final("run-1");
    assert!(validate_ref_id(&fin, "run-2").is_err());
}

#[test]
fn validate_ref_id_match_fatal_with_ref() {
    let fat = mock_fatal("run-1", "err");
    assert!(validate_ref_id(&fat, "run-1").is_ok());
}

#[test]
fn validate_ref_id_mismatch_fatal_with_ref() {
    let fat = mock_fatal("run-1", "err");
    assert!(validate_ref_id(&fat, "run-2").is_err());
}

#[test]
fn validate_ref_id_fatal_without_ref_always_passes() {
    let fat = Envelope::Fatal {
        ref_id: None,
        error: "crash".into(),
        error_code: None,
    };
    assert!(validate_ref_id(&fat, "any-ref").is_ok());
    assert!(validate_ref_id(&fat, "").is_ok());
}

#[test]
fn validate_ref_id_hello_always_passes() {
    let hello = mock_hello("b");
    assert!(validate_ref_id(&hello, "any-ref").is_ok());
    assert!(validate_ref_id(&hello, "").is_ok());
}

#[test]
fn validate_ref_id_run_always_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    assert!(validate_ref_id(&run, "whatever").is_ok());
}

#[test]
fn validate_sequence_valid_hello_events_final() {
    let seq = vec![
        mock_hello("b"),
        mock_event("r", "a"),
        mock_event("r", "b"),
        mock_final("r"),
    ];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_minimal_hello_final() {
    let seq = vec![mock_hello("b"), mock_final("r")];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_hello_fatal() {
    let seq = vec![mock_hello("b"), mock_fatal("r", "err")];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_hello_events_fatal() {
    let seq = vec![
        mock_hello("b"),
        mock_event("r", "partial"),
        mock_fatal("r", "crashed"),
    ];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_empty_rejected() {
    let err = validate_sequence(&[]).unwrap_err();
    assert!(err.to_string().contains("empty"));
}

#[test]
fn validate_sequence_no_hello_first() {
    let seq = vec![mock_event("r", "text"), mock_final("r")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(err.to_string().contains("hello"));
}

#[test]
fn validate_sequence_hello_only_no_terminal() {
    let seq = vec![mock_hello("b")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(err.to_string().contains("terminal") || err.to_string().contains("at least"));
}

#[test]
fn validate_sequence_no_terminal_after_events() {
    let seq = vec![mock_hello("b"), mock_event("r", "text")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(
        err.to_string().contains("final")
            || err.to_string().contains("fatal")
            || err.to_string().contains("terminal")
    );
}

#[test]
fn validate_sequence_duplicate_final() {
    let seq = vec![mock_hello("b"), mock_final("r"), mock_final("r")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(err.to_string().contains("multiple") || err.to_string().contains("terminal"));
}

#[test]
fn validate_sequence_duplicate_hello() {
    let seq = vec![mock_hello("b"), mock_hello("b2"), mock_final("r")];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(err.to_string().contains("hello") || err.to_string().contains("unexpected"));
}

#[test]
fn validate_sequence_final_in_middle() {
    let seq = vec![
        mock_hello("b"),
        mock_final("r"),
        mock_event("r", "after"),
        mock_final("r"),
    ];
    let err = validate_sequence(&seq).unwrap_err();
    assert!(err.to_string().contains("multiple") || err.to_string().contains("terminal"));
}

#[test]
fn validate_sequence_fatal_not_last() {
    let seq = vec![
        mock_hello("b"),
        mock_fatal("r", "err"),
        mock_event("r", "after"),
    ];
    let err = validate_sequence(&seq).unwrap_err();
    // Either detects fatal in middle or event after terminal
    assert!(
        err.to_string().contains("terminal")
            || err.to_string().contains("fatal")
            || err.to_string().contains("last")
    );
}

#[test]
fn validate_sequence_many_events() {
    let mut seq = vec![mock_hello("b")];
    for i in 0..100 {
        seq.push(mock_event("r", &format!("msg-{i}")));
    }
    seq.push(mock_final("r"));
    assert!(validate_sequence(&seq).is_ok());
}

// ========================================================================
// Cross-module: serialization roundtrips
// ========================================================================

#[test]
fn serde_roundtrip_all_mock_types() {
    let envelopes = vec![
        mock_hello("rt-test"),
        mock_event("run-1", "hello"),
        mock_final("run-1"),
        mock_fatal("run-1", "err"),
    ];

    for env in &envelopes {
        let json = JsonlCodec::encode(env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        let json2 = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(json, json2, "roundtrip failed for {env:?}");
    }
}

#[test]
fn serde_roundtrip_via_codec() {
    let mut codec = StreamingCodec::new();
    let envelopes = vec![
        mock_hello("codec-test"),
        mock_event("run-1", "text"),
        mock_final("run-1"),
    ];

    let mut data = String::new();
    for env in &envelopes {
        data.push_str(&encode_envelope(env).unwrap());
    }

    let decoded = codec.push(data.as_bytes());
    assert_eq!(decoded.len(), 3);
    assert!(matches!(&decoded[0], Envelope::Hello { .. }));
    assert!(matches!(&decoded[1], Envelope::Event { .. }));
    assert!(matches!(&decoded[2], Envelope::Final { .. }));
}

#[test]
fn serde_roundtrip_event_with_special_chars() {
    let event = make_agent_event(AgentEventKind::AssistantMessage {
        text: "line1\nline2\ttab \"quotes\" \\backslash".into(),
    });
    let line = encode_event("run-1", &event);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Event { event: e, .. } = env {
        if let AgentEventKind::AssistantMessage { text } = &e.kind {
            assert_eq!(text, "line1\nline2\ttab \"quotes\" \\backslash");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

// ========================================================================
// Cross-module: full protocol session
// ========================================================================

#[test]
fn full_protocol_session_through_codec_and_processor() {
    let mut codec = StreamingCodec::new();
    let mut proc = EventStreamProcessor::new("run-1".into());

    // Build a full session as JSONL
    let hello_line = encode_hello("integration-test", "1.0", &["streaming"]);
    let event1_line = encode_event(
        "run-1",
        &make_agent_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
    );
    let event2_line = encode_event(
        "run-1",
        &make_agent_event(AgentEventKind::AssistantMessage {
            text: "result".into(),
        }),
    );
    let final_line = encode_final(
        "run-1",
        &ReceiptBuilder::new("integration-test")
            .outcome(Outcome::Complete)
            .build(),
    );

    let data = format!("{hello_line}{event1_line}{event2_line}{final_line}");
    let envelopes = codec.push(data.as_bytes());
    assert_eq!(envelopes.len(), 4);
    assert_eq!(codec.metrics().lines_parsed, 4);

    // Validate hello
    assert!(validate_hello(&envelopes[0]).is_ok());

    // Process events through the processor (skip hello)
    for env in &envelopes[1..] {
        let _ = proc.process_envelope(env);
    }
    assert_eq!(proc.stats().events_processed, 2);
    assert!(proc.is_terminal());

    // Validate complete sequence
    assert!(validate_sequence(&envelopes).is_ok());
}

#[tokio::test]
async fn full_handshake_then_event_processing() {
    let mut buf = Vec::new();
    let backend = BackendIdentity {
        id: "full-test".into(),
        backend_version: Some("1.0".into()),
        adapter_version: None,
    };
    HandshakeManager::send_hello(&mut buf, backend, CapabilityManifest::new())
        .await
        .unwrap();

    // Read back the hello
    let reader = BufReader::new(buf.as_slice());
    let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(info.backend.id, "full-test");

    // Now simulate event processing
    let mut proc = EventStreamProcessor::new("run-1".into());
    let events = vec![
        mock_event("run-1", "processing"),
        mock_event("run-1", "done"),
    ];
    let results = proc.process_many(&events);
    assert!(results.iter().all(|r| r.is_ok()));
    assert_eq!(proc.stats().events_processed, 2);
}

// ========================================================================
// Edge cases: large / boundary inputs
// ========================================================================

#[test]
fn codec_max_line_len_zero_rejects_everything() {
    let mut codec = StreamingCodec::with_max_line_len(0);
    let line = fatal_json("x");
    let envs = codec.push(line.as_bytes());
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn codec_exactly_at_max_line_len() {
    // Create a line that is exactly max_line_len bytes (excluding newline)
    let base = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"";
    let suffix = "\"}";
    let overhead = base.len() + suffix.len();
    let limit = overhead + 10;
    let pad = "x".repeat(10);
    let line = format!("{base}{pad}{suffix}\n");

    let mut codec = StreamingCodec::with_max_line_len(limit);
    let envs = codec.push(line.as_bytes());
    assert_eq!(envs.len(), 1);
}

#[test]
fn codec_one_byte_over_max_line_len() {
    let base = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"";
    let suffix = "\"}";
    let overhead = base.len() + suffix.len();
    let limit = overhead + 10;
    let pad = "x".repeat(11); // one byte over
    let line = format!("{base}{pad}{suffix}\n");

    let mut codec = StreamingCodec::with_max_line_len(limit);
    let envs = codec.push(line.as_bytes());
    assert!(envs.is_empty());
    assert_eq!(codec.metrics().errors_skipped, 1);
}

#[test]
fn encode_hello_very_long_backend_name() {
    let long_name = "x".repeat(10_000);
    let line = encode_hello(&long_name, "1.0", &[]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { backend, .. } = env {
        assert_eq!(backend.id, long_name);
    }
}

#[test]
fn encode_fatal_very_long_error() {
    let long_error = "e".repeat(50_000);
    let line = encode_fatal("r", &long_error);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Fatal { error, .. } = env {
        assert_eq!(error, long_error);
    }
}

#[test]
fn mock_work_order_empty_task() {
    let line = mock_work_order("");
    let env = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(env, Envelope::Run { .. }));
}

#[test]
fn mock_work_order_very_long_task() {
    let long_task = "t".repeat(100_000);
    let line = mock_work_order(&long_task);
    let env = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = env {
        assert!(work_order.task.contains(&long_task));
    }
}

// ========================================================================
// Error type quality checks
// ========================================================================

#[test]
fn handshake_error_display_timeout() {
    let err = HandshakeError::Timeout(Duration::from_secs(10));
    let msg = err.to_string();
    assert!(msg.contains("timed out"));
    assert!(msg.contains("10"));
}

#[test]
fn handshake_error_display_incompatible() {
    let err = HandshakeError::IncompatibleVersion {
        got: "abp/v99.0".into(),
        expected: "abp/v0.1".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("incompatible"));
    assert!(msg.contains("abp/v99.0"));
    assert!(msg.contains("abp/v0.1"));
}

#[test]
fn handshake_error_display_unexpected() {
    let err = HandshakeError::UnexpectedMessage("Fatal { .. }".into());
    assert!(err.to_string().contains("expected hello"));
}

#[test]
fn handshake_error_display_peer_closed() {
    let err = HandshakeError::PeerClosed;
    assert!(err.to_string().contains("closed"));
}

#[test]
fn event_stream_error_display_ref_id_mismatch() {
    let err = EventStreamError::RefIdMismatch {
        expected: "run-A".into(),
        got: "run-B".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("run-A"));
    assert!(msg.contains("run-B"));
}

#[test]
fn event_stream_error_display_event_after_terminal() {
    let err = EventStreamError::EventAfterTerminal;
    assert!(err.to_string().contains("terminal"));
}

#[test]
fn event_stream_error_display_unexpected_envelope() {
    let err = EventStreamError::UnexpectedEnvelope("run".into());
    assert!(err.to_string().contains("run"));
}
