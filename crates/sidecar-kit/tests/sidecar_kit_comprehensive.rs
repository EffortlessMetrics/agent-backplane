#![allow(clippy::all)]
#![allow(unknown_lints)]

use std::collections::BTreeMap;
use std::io::BufReader;

use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

use sidecar_kit::builders::*;
use sidecar_kit::cancel::CancelToken;
use sidecar_kit::codec::JsonlCodec;
use sidecar_kit::diagnostics::*;
use sidecar_kit::error::SidecarError;
use sidecar_kit::frame::Frame;
use sidecar_kit::framing::*;
use sidecar_kit::middleware::{
    ErrorWrapMiddleware, EventMiddleware, FilterMiddleware as ValueFilterMiddleware,
    LoggingMiddleware as ValueLoggingMiddleware, MiddlewareChain, TimingMiddleware,
};
use sidecar_kit::pipeline::*;
use sidecar_kit::protocol_state::*;
use sidecar_kit::spec::ProcessSpec;
use sidecar_kit::transform::*;
use sidecar_kit::typed_middleware::{
    MetricsMiddleware, MiddlewareAction, RateLimitMiddleware, SidecarMiddleware,
    SidecarMiddlewareChain,
};

use abp_core::{AgentEvent, AgentEventKind};

// ═══════════════════════════════════════════════════════════════════════
// Frame – serde round-trips
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn frame_hello_round_trip() {
    let f = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Hello { contract_version, .. } if contract_version == "abp/v0.1"));
}

#[test]
fn frame_run_round_trip() {
    let f = Frame::Run {
        id: "run-1".into(),
        work_order: json!({"task": "test"}),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Run { id, .. } if id == "run-1"));
}

#[test]
fn frame_event_round_trip() {
    let f = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"type": "assistant_delta", "text": "hi"}),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Event { ref_id, .. } if ref_id == "r1"));
}

#[test]
fn frame_final_round_trip() {
    let f = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Final { ref_id, .. } if ref_id == "r1"));
}

#[test]
fn frame_fatal_round_trip() {
    let f = Frame::Fatal {
        ref_id: Some("r1".into()),
        error: "boom".into(),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Fatal { ref_id: Some(r), error } if r == "r1" && error == "boom"));
}

#[test]
fn frame_fatal_no_ref_id_round_trip() {
    let f = Frame::Fatal {
        ref_id: None,
        error: "early failure".into(),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Fatal { ref_id: None, .. }));
}

#[test]
fn frame_cancel_round_trip() {
    let f = Frame::Cancel {
        ref_id: "r1".into(),
        reason: Some("user abort".into()),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(
        matches!(f2, Frame::Cancel { ref_id, reason: Some(r) } if ref_id == "r1" && r == "user abort")
    );
}

#[test]
fn frame_cancel_no_reason_round_trip() {
    let f = Frame::Cancel {
        ref_id: "r1".into(),
        reason: None,
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Cancel { reason: None, .. }));
}

#[test]
fn frame_ping_round_trip() {
    let f = Frame::Ping { seq: 42 };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Ping { seq: 42 }));
}

#[test]
fn frame_pong_round_trip() {
    let f = Frame::Pong { seq: 99 };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Pong { seq: 99 }));
}

#[test]
fn frame_hello_discriminator_is_t() {
    let f = hello_frame("test-backend");
    let s = serde_json::to_string(&f).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["t"], "hello");
}

#[test]
fn frame_event_discriminator_is_t() {
    let f = event_frame("r1", json!({}));
    let v: Value = serde_json::to_value(&f).unwrap();
    assert_eq!(v["t"], "event");
}

#[test]
fn frame_final_discriminator_is_t() {
    let f = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({}),
    };
    let v: Value = serde_json::to_value(&f).unwrap();
    assert_eq!(v["t"], "final");
}

#[test]
fn frame_fatal_discriminator_is_t() {
    let f = fatal_frame(Some("r1"), "oops");
    let v: Value = serde_json::to_value(&f).unwrap();
    assert_eq!(v["t"], "fatal");
}

// ═══════════════════════════════════════════════════════════════════════
// Frame – try_event / try_final
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn try_event_extracts_typed_value() {
    let f = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"text": "hello"}),
    };
    let (rid, val): (String, Value) = f.try_event().unwrap();
    assert_eq!(rid, "r1");
    assert_eq!(val["text"], "hello");
}

#[test]
fn try_event_on_non_event_fails() {
    let f = Frame::Ping { seq: 1 };
    let result: Result<(String, Value), _> = f.try_event();
    assert!(result.is_err());
}

#[test]
fn try_final_extracts_typed_value() {
    let f = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let (rid, val): (String, Value) = f.try_final().unwrap();
    assert_eq!(rid, "r1");
    assert_eq!(val["outcome"], "complete");
}

#[test]
fn try_final_on_non_final_fails() {
    let f = Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    };
    let result: Result<(String, Value), _> = f.try_final();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// JsonlCodec
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codec_encode_ends_with_newline() {
    let f = Frame::Ping { seq: 1 };
    let s = JsonlCodec::encode(&f).unwrap();
    assert!(s.ends_with('\n'));
}

#[test]
fn codec_encode_is_single_line() {
    let f = hello_frame("test");
    let s = JsonlCodec::encode(&f).unwrap();
    assert_eq!(s.matches('\n').count(), 1);
}

#[test]
fn codec_decode_round_trip() {
    let f = Frame::Ping { seq: 7 };
    let encoded = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Frame::Ping { seq: 7 }));
}

#[test]
fn codec_decode_invalid_json_fails() {
    let result = JsonlCodec::decode("not json at all");
    assert!(result.is_err());
}

#[test]
fn codec_decode_empty_object_fails() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn codec_decode_missing_tag_fails() {
    let result = JsonlCodec::decode(r#"{"ref_id": "r1"}"#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// Builders – hello_frame
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_frame_sets_contract_version() {
    let f = hello_frame("my-backend");
    match &f {
        Frame::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_frame_sets_backend_id() {
    let f = hello_frame("claude-sidecar");
    match &f {
        Frame::Hello { backend, .. } => {
            assert_eq!(backend["id"], "claude-sidecar");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_frame_capabilities_default_empty() {
    let f = hello_frame("test");
    match &f {
        Frame::Hello { capabilities, .. } => {
            assert_eq!(capabilities, &json!({}));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_frame_mode_default_null() {
    let f = hello_frame("test");
    match &f {
        Frame::Hello { mode, .. } => {
            assert!(mode.is_null());
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Builders – event helpers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_text_delta_has_type() {
    let v = event_text_delta("chunk");
    assert_eq!(v["type"], "assistant_delta");
    assert_eq!(v["text"], "chunk");
}

#[test]
fn event_text_delta_has_timestamp() {
    let v = event_text_delta("x");
    assert!(v["ts"].is_string());
}

#[test]
fn event_text_message_has_type() {
    let v = event_text_message("full msg");
    assert_eq!(v["type"], "assistant_message");
    assert_eq!(v["text"], "full msg");
}

#[test]
fn event_tool_call_has_fields() {
    let v = event_tool_call("read_file", Some("tc-1"), json!({"path": "/tmp"}));
    assert_eq!(v["type"], "tool_call");
    assert_eq!(v["tool_name"], "read_file");
    assert_eq!(v["tool_use_id"], "tc-1");
    assert_eq!(v["input"]["path"], "/tmp");
}

#[test]
fn event_tool_call_no_tool_use_id() {
    let v = event_tool_call("bash", None, json!({}));
    assert!(v["tool_use_id"].is_null());
}

#[test]
fn event_tool_result_has_fields() {
    let v = event_tool_result("read_file", Some("tc-1"), json!("content"), false);
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["tool_name"], "read_file");
    assert_eq!(v["is_error"], false);
}

#[test]
fn event_tool_result_is_error() {
    let v = event_tool_result("bash", None, json!("failed"), true);
    assert_eq!(v["is_error"], true);
}

#[test]
fn event_error_has_type_and_message() {
    let v = event_error("something broke");
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "something broke");
}

#[test]
fn event_warning_has_type_and_message() {
    let v = event_warning("be careful");
    assert_eq!(v["type"], "warning");
    assert_eq!(v["message"], "be careful");
}

#[test]
fn event_run_started_has_type() {
    let v = event_run_started("starting up");
    assert_eq!(v["type"], "run_started");
    assert_eq!(v["message"], "starting up");
}

#[test]
fn event_run_completed_has_type() {
    let v = event_run_completed("done");
    assert_eq!(v["type"], "run_completed");
    assert_eq!(v["message"], "done");
}

#[test]
fn event_file_changed_has_fields() {
    let v = event_file_changed("src/main.rs", "added logging");
    assert_eq!(v["type"], "file_changed");
    assert_eq!(v["path"], "src/main.rs");
    assert_eq!(v["summary"], "added logging");
}

#[test]
fn event_command_executed_full() {
    let v = event_command_executed("cargo test", Some(0), Some("all passed"));
    assert_eq!(v["type"], "command_executed");
    assert_eq!(v["command"], "cargo test");
    assert_eq!(v["exit_code"], 0);
    assert_eq!(v["output_preview"], "all passed");
}

#[test]
fn event_command_executed_minimal() {
    let v = event_command_executed("ls", None, None);
    assert!(v["exit_code"].is_null());
    assert!(v["output_preview"].is_null());
}

// ═══════════════════════════════════════════════════════════════════════
// Builders – event_frame / fatal_frame
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_frame_wraps_value() {
    let f = event_frame("r1", json!({"type": "warning"}));
    match f {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "r1");
            assert_eq!(event["type"], "warning");
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn fatal_frame_with_ref_id() {
    let f = fatal_frame(Some("r1"), "crash");
    match f {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.unwrap(), "r1");
            assert_eq!(error, "crash");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_frame_without_ref_id() {
    let f = fatal_frame(None, "early crash");
    match f {
        Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "early crash");
        }
        _ => panic!("expected Fatal"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ReceiptBuilder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_default_outcome_complete() {
    let r = ReceiptBuilder::new("run-1", "backend-1").build();
    assert_eq!(r["outcome"], "complete");
}

#[test]
fn receipt_builder_failed() {
    let r = ReceiptBuilder::new("run-1", "b").failed().build();
    assert_eq!(r["outcome"], "failed");
}

#[test]
fn receipt_builder_partial() {
    let r = ReceiptBuilder::new("run-1", "b").partial().build();
    assert_eq!(r["outcome"], "partial");
}

#[test]
fn receipt_builder_meta_fields() {
    let r = ReceiptBuilder::new("run-42", "mock").build();
    assert_eq!(r["meta"]["run_id"], "run-42");
    assert_eq!(r["meta"]["work_order_id"], "run-42");
    assert_eq!(r["meta"]["contract_version"], "abp/v0.1");
}

#[test]
fn receipt_builder_backend_id() {
    let r = ReceiptBuilder::new("r1", "claude-3").build();
    assert_eq!(r["backend"]["id"], "claude-3");
}

#[test]
fn receipt_builder_events() {
    let r = ReceiptBuilder::new("r1", "b")
        .event(json!({"type": "run_started"}))
        .event(json!({"type": "run_completed"}))
        .build();
    assert_eq!(r["trace"].as_array().unwrap().len(), 2);
}

#[test]
fn receipt_builder_artifacts() {
    let r = ReceiptBuilder::new("r1", "b")
        .artifact("file", "output.txt")
        .build();
    let arts = r["artifacts"].as_array().unwrap();
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0]["kind"], "file");
    assert_eq!(arts[0]["path"], "output.txt");
}

#[test]
fn receipt_builder_usage_tokens() {
    let r = ReceiptBuilder::new("r1", "b")
        .input_tokens(100)
        .output_tokens(50)
        .build();
    assert_eq!(r["usage"]["input_tokens"], 100);
    assert_eq!(r["usage"]["output_tokens"], 50);
}

#[test]
fn receipt_builder_usage_raw() {
    let raw = json!({"prompt_tokens": 200});
    let r = ReceiptBuilder::new("r1", "b")
        .usage_raw(raw.clone())
        .build();
    assert_eq!(r["usage_raw"], raw);
}

#[test]
fn receipt_builder_receipt_sha256_null() {
    let r = ReceiptBuilder::new("r1", "b").build();
    assert!(r["receipt_sha256"].is_null());
}

#[test]
fn receipt_builder_chaining() {
    let r = ReceiptBuilder::new("r1", "b")
        .failed()
        .input_tokens(10)
        .output_tokens(20)
        .event(json!({}))
        .artifact("diff", "patch.diff")
        .build();
    assert_eq!(r["outcome"], "failed");
    assert_eq!(r["usage"]["input_tokens"], 10);
    assert_eq!(r["trace"].as_array().unwrap().len(), 1);
    assert_eq!(r["artifacts"].as_array().unwrap().len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// FrameWriter / FrameReader
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn frame_writer_write_and_count() {
    let mut buf = Vec::new();
    let mut w = FrameWriter::new(&mut buf);
    w.write_frame(&Frame::Ping { seq: 1 }).unwrap();
    w.write_frame(&Frame::Pong { seq: 1 }).unwrap();
    assert_eq!(w.frames_written(), 2);
}

#[test]
fn frame_writer_exceeds_max_size() {
    let mut buf = Vec::new();
    let mut w = FrameWriter::with_max_size(&mut buf, 10);
    let f = hello_frame("test");
    let result = w.write_frame(&f);
    assert!(result.is_err());
}

#[test]
fn frame_writer_inner_access() {
    let buf = Vec::<u8>::new();
    let w = FrameWriter::new(buf);
    assert!(w.inner().is_empty());
    let inner = w.into_inner();
    assert!(inner.is_empty());
}

#[test]
fn frame_reader_reads_frames() {
    let mut buf = Vec::new();
    {
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(&Frame::Ping { seq: 1 }).unwrap();
        w.write_frame(&Frame::Pong { seq: 1 }).unwrap();
        w.flush().unwrap();
    }
    let mut r = FrameReader::new(BufReader::new(&buf[..]));
    let f1 = r.read_frame().unwrap().unwrap();
    assert!(matches!(f1, Frame::Ping { seq: 1 }));
    let f2 = r.read_frame().unwrap().unwrap();
    assert!(matches!(f2, Frame::Pong { seq: 1 }));
    assert!(r.read_frame().unwrap().is_none());
    assert_eq!(r.frames_read(), 2);
}

#[test]
fn frame_reader_skips_blank_lines() {
    let data = b"\n\n{\"t\":\"ping\",\"seq\":1}\n\n";
    let mut r = FrameReader::new(BufReader::new(&data[..]));
    let f = r.read_frame().unwrap().unwrap();
    assert!(matches!(f, Frame::Ping { seq: 1 }));
}

#[test]
fn frame_reader_exceeds_max_size() {
    let data = b"{\"t\":\"ping\",\"seq\":1}\n";
    let mut r = FrameReader::with_max_size(BufReader::new(&data[..]), 5);
    let result = r.read_frame();
    assert!(result.is_err());
}

#[test]
fn frame_reader_eof_returns_none() {
    let data: &[u8] = b"";
    let mut r = FrameReader::new(BufReader::new(data));
    assert!(r.read_frame().unwrap().is_none());
}

#[test]
fn frame_iter_collects_all() {
    let mut buf = Vec::new();
    {
        let mut w = FrameWriter::new(&mut buf);
        for i in 0..5 {
            w.write_frame(&Frame::Ping { seq: i }).unwrap();
        }
        w.flush().unwrap();
    }
    let r = FrameReader::new(BufReader::new(&buf[..]));
    let frames: Vec<_> = r.frames().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(frames.len(), 5);
}

// ═══════════════════════════════════════════════════════════════════════
// Framing convenience functions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn write_frames_returns_count() {
    let mut buf = Vec::new();
    let frames = vec![Frame::Ping { seq: 0 }, Frame::Pong { seq: 0 }];
    let count = write_frames(&mut buf, &frames).unwrap();
    assert_eq!(count, 2);
}

#[test]
fn read_all_frames_collects() {
    let mut buf = Vec::new();
    let frames = vec![Frame::Ping { seq: 1 }, Frame::Ping { seq: 2 }];
    write_frames(&mut buf, &frames).unwrap();
    let read = read_all_frames(BufReader::new(&buf[..])).unwrap();
    assert_eq!(read.len(), 2);
}

#[test]
fn frame_to_json_no_newline() {
    let f = Frame::Ping { seq: 1 };
    let s = frame_to_json(&f).unwrap();
    assert!(!s.contains('\n'));
}

#[test]
fn json_to_frame_parses() {
    let json_str = r#"{"t":"pong","seq":42}"#;
    let f = json_to_frame(json_str).unwrap();
    assert!(matches!(f, Frame::Pong { seq: 42 }));
}

#[test]
fn json_to_frame_invalid_returns_err() {
    assert!(json_to_frame("garbage").is_err());
}

#[test]
fn buf_reader_from_bytes_works() {
    let data = b"hello world\n";
    let _reader = buf_reader_from_bytes(data);
}

// ═══════════════════════════════════════════════════════════════════════
// validate_frame
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_valid_hello_frame() {
    let f = hello_frame("test");
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
    assert!(v.issues.is_empty());
}

#[test]
fn validate_hello_empty_contract_version() {
    let f = Frame::Hello {
        contract_version: "".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("contract_version")));
}

#[test]
fn validate_hello_bad_version_prefix() {
    let f = Frame::Hello {
        contract_version: "v1.0".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("abp/v")));
}

#[test]
fn validate_hello_missing_backend_id() {
    let f = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("backend.id")));
}

#[test]
fn validate_run_empty_id() {
    let f = Frame::Run {
        id: "".into(),
        work_order: json!({}),
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_event_empty_ref_id() {
    let f = Frame::Event {
        ref_id: "".into(),
        event: json!({}),
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_final_empty_ref_id() {
    let f = Frame::Final {
        ref_id: "".into(),
        receipt: json!({}),
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_fatal_empty_error() {
    let f = Frame::Fatal {
        ref_id: Some("r1".into()),
        error: "".into(),
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_cancel_empty_ref_id() {
    let f = Frame::Cancel {
        ref_id: "".into(),
        reason: None,
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_ping_always_valid() {
    let f = Frame::Ping { seq: 0 };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
}

#[test]
fn validate_pong_always_valid() {
    let f = Frame::Pong { seq: 0 };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
}

#[test]
fn validate_frame_exceeds_size() {
    let f = hello_frame("test");
    let v = validate_frame(&f, 5);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("exceeds limit")));
}

// ═══════════════════════════════════════════════════════════════════════
// ProtocolState
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_state_initial_phase() {
    let s = ProtocolState::new();
    assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
    assert!(!s.is_terminal());
}

#[test]
fn protocol_state_default_is_awaiting_hello() {
    let s = ProtocolState::default();
    assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
}

#[test]
fn protocol_happy_path() {
    let mut s = ProtocolState::new();

    s.advance(&hello_frame("test")).unwrap();
    assert_eq!(s.phase(), ProtocolPhase::AwaitingRun);

    s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    })
    .unwrap();
    assert_eq!(s.phase(), ProtocolPhase::Streaming);
    assert_eq!(s.run_id(), Some("r1"));

    s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    })
    .unwrap();
    assert_eq!(s.events_seen(), 1);

    s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    })
    .unwrap();
    assert_eq!(s.events_seen(), 2);

    s.advance(&Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({}),
    })
    .unwrap();
    assert_eq!(s.phase(), ProtocolPhase::Completed);
    assert!(s.is_terminal());
}

#[test]
fn protocol_fatal_during_streaming() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    })
    .unwrap();
    s.advance(&Frame::Fatal {
        ref_id: Some("r1".into()),
        error: "crash".into(),
    })
    .unwrap();
    assert_eq!(s.phase(), ProtocolPhase::Completed);
}

#[test]
fn protocol_fatal_without_ref_id_during_streaming() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    })
    .unwrap();
    s.advance(&Frame::Fatal {
        ref_id: None,
        error: "crash".into(),
    })
    .unwrap();
    assert_eq!(s.phase(), ProtocolPhase::Completed);
}

#[test]
fn protocol_fatal_before_run() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    s.advance(&Frame::Fatal {
        ref_id: None,
        error: "cannot start".into(),
    })
    .unwrap();
    assert_eq!(s.phase(), ProtocolPhase::Completed);
}

#[test]
fn protocol_event_before_hello_faults() {
    let mut s = ProtocolState::new();
    let result = s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    });
    assert!(result.is_err());
    assert_eq!(s.phase(), ProtocolPhase::Faulted);
    assert!(s.fault_reason().is_some());
    assert!(s.is_terminal());
}

#[test]
fn protocol_run_before_hello_faults() {
    let mut s = ProtocolState::new();
    let result = s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    });
    assert!(result.is_err());
    assert_eq!(s.phase(), ProtocolPhase::Faulted);
}

#[test]
fn protocol_event_before_run_faults() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    let result = s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    });
    assert!(result.is_err());
    assert_eq!(s.phase(), ProtocolPhase::Faulted);
}

#[test]
fn protocol_double_hello_faults() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    let result = s.advance(&hello_frame("test2"));
    assert!(result.is_err());
    assert_eq!(s.phase(), ProtocolPhase::Faulted);
}

#[test]
fn protocol_faulted_state_rejects_all() {
    let mut s = ProtocolState::new();
    let _ = s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    });
    assert_eq!(s.phase(), ProtocolPhase::Faulted);
    let result = s.advance(&hello_frame("test"));
    assert!(result.is_err());
}

#[test]
fn protocol_reset() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    })
    .unwrap();
    s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    })
    .unwrap();

    s.reset();
    assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
    assert!(s.run_id().is_none());
    assert_eq!(s.events_seen(), 0);
    assert!(s.fault_reason().is_none());
}

#[test]
fn protocol_reset_after_fault() {
    let mut s = ProtocolState::new();
    let _ = s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    });
    assert_eq!(s.phase(), ProtocolPhase::Faulted);
    s.reset();
    assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
    s.advance(&hello_frame("test")).unwrap();
    assert_eq!(s.phase(), ProtocolPhase::AwaitingRun);
}

#[test]
fn protocol_ref_id_mismatch_errors() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    })
    .unwrap();
    let result = s.advance(&Frame::Event {
        ref_id: "wrong-id".into(),
        event: json!({}),
    });
    assert!(result.is_err());
}

#[test]
fn protocol_ping_pong_allowed_during_streaming() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    })
    .unwrap();
    s.advance(&Frame::Ping { seq: 1 }).unwrap();
    s.advance(&Frame::Pong { seq: 1 }).unwrap();
    assert_eq!(s.phase(), ProtocolPhase::Streaming);
}

#[test]
fn protocol_completed_rejects_new_frames() {
    let mut s = ProtocolState::new();
    s.advance(&hello_frame("test")).unwrap();
    s.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    })
    .unwrap();
    s.advance(&Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({}),
    })
    .unwrap();
    let result = s.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    });
    assert!(result.is_err());
    assert_eq!(s.phase(), ProtocolPhase::Faulted);
}

// ═══════════════════════════════════════════════════════════════════════
// ProcessSpec
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_spec_new_defaults() {
    let spec = ProcessSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn process_spec_with_args() {
    let mut spec = ProcessSpec::new("python");
    spec.args = vec!["script.py".into(), "--flag".into()];
    assert_eq!(spec.args.len(), 2);
}

#[test]
fn process_spec_with_env() {
    let mut spec = ProcessSpec::new("node");
    spec.env.insert("API_KEY".into(), "secret".into());
    assert_eq!(spec.env["API_KEY"], "secret");
}

#[test]
fn process_spec_with_cwd() {
    let mut spec = ProcessSpec::new("bash");
    spec.cwd = Some("/tmp/workspace".into());
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/workspace"));
}

// ═══════════════════════════════════════════════════════════════════════
// CancelToken
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancel_token_not_cancelled_initially() {
    let token = CancelToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancel_token_default_not_cancelled() {
    let token = CancelToken::default();
    assert!(!token.is_cancelled());
}

#[test]
fn cancel_token_cancel_sets_flag() {
    let token = CancelToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancel_token_clones_share_state() {
    let t1 = CancelToken::new();
    let t2 = t1.clone();
    t1.cancel();
    assert!(t2.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancelled_returns_immediately_if_set() {
    let token = CancelToken::new();
    token.cancel();
    token.cancelled().await;
    assert!(token.is_cancelled());
}

// ═══════════════════════════════════════════════════════════════════════
// Middleware (value-based)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn logging_middleware_passes_through() {
    let mw = ValueLoggingMiddleware::new();
    let ev = json!({"type": "run_started", "message": "hi"});
    let result = mw.process(&ev);
    assert!(result.is_some());
    assert_eq!(result.unwrap()["type"], "run_started");
}

#[test]
fn filter_middleware_include() {
    let mw = ValueFilterMiddleware::include_kinds(&["assistant_delta"]);
    let pass = json!({"type": "assistant_delta", "text": "hi"});
    let drop = json!({"type": "error", "message": "bad"});
    assert!(mw.process(&pass).is_some());
    assert!(mw.process(&drop).is_none());
}

#[test]
fn filter_middleware_include_case_insensitive() {
    let mw = ValueFilterMiddleware::include_kinds(&["Assistant_Delta"]);
    let ev = json!({"type": "assistant_delta", "text": "hi"});
    assert!(mw.process(&ev).is_some());
}

#[test]
fn filter_middleware_exclude() {
    let mw = ValueFilterMiddleware::exclude_kinds(&["error"]);
    let pass = json!({"type": "assistant_delta", "text": "hi"});
    let drop = json!({"type": "error", "message": "bad"});
    assert!(mw.process(&pass).is_some());
    assert!(mw.process(&drop).is_none());
}

#[test]
fn filter_middleware_include_empty_list_drops_all() {
    let mw = ValueFilterMiddleware::include_kinds(&[]);
    assert!(mw.process(&json!({"type": "anything"})).is_none());
}

#[test]
fn filter_middleware_exclude_empty_list_passes_all() {
    let mw = ValueFilterMiddleware::exclude_kinds(&[]);
    assert!(mw.process(&json!({"type": "anything"})).is_some());
}

#[test]
fn timing_middleware_adds_processing_us() {
    let mw = TimingMiddleware::new();
    let ev = json!({"type": "test"});
    let result = mw.process(&ev).unwrap();
    assert!(result.get("_processing_us").is_some());
}

#[test]
fn timing_middleware_non_object_passes_through() {
    let mw = TimingMiddleware::new();
    let ev = json!("just a string");
    let result = mw.process(&ev).unwrap();
    // Non-object, no _processing_us added, but still returned
    assert!(result.is_string());
}

#[test]
fn error_wrap_middleware_passes_objects() {
    let mw = ErrorWrapMiddleware::new();
    let ev = json!({"type": "run_started"});
    let result = mw.process(&ev).unwrap();
    assert_eq!(result["type"], "run_started");
}

#[test]
fn error_wrap_middleware_wraps_non_objects() {
    let mw = ErrorWrapMiddleware::new();
    let ev = json!(42);
    let result = mw.process(&ev).unwrap();
    assert_eq!(result["type"], "error");
    assert!(result["message"].as_str().unwrap().contains("non-object"));
    assert_eq!(result["_original"], 42);
}

#[test]
fn middleware_chain_empty_passthrough() {
    let chain = MiddlewareChain::new();
    let ev = json!({"type": "test"});
    let result = chain.process(&ev).unwrap();
    assert_eq!(result, ev);
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn middleware_chain_with_builder() {
    let chain = MiddlewareChain::new()
        .with(ValueLoggingMiddleware::new())
        .with(TimingMiddleware::new());
    assert_eq!(chain.len(), 2);
    assert!(!chain.is_empty());
}

#[test]
fn middleware_chain_filter_drops() {
    let chain =
        MiddlewareChain::new().with(ValueFilterMiddleware::include_kinds(&["assistant_delta"]));
    assert!(chain.process(&json!({"type": "error"})).is_none());
    assert!(chain.process(&json!({"type": "assistant_delta"})).is_some());
}

#[test]
fn middleware_chain_push() {
    let mut chain = MiddlewareChain::new();
    chain.push(ValueLoggingMiddleware::new());
    assert_eq!(chain.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Pipeline (stage-based)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_empty_passthrough() {
    let pipeline = EventPipeline::new();
    let ev = json!({"type": "test"});
    let result = pipeline.process(ev.clone()).unwrap();
    assert_eq!(result.unwrap(), ev);
    assert_eq!(pipeline.stage_count(), 0);
}

#[test]
fn pipeline_default_empty() {
    let pipeline = EventPipeline::default();
    assert_eq!(pipeline.stage_count(), 0);
}

#[test]
fn timestamp_stage_adds_processed_at() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(TimestampStage::new()));
    let ev = json!({"type": "test"});
    let result = pipeline.process(ev).unwrap().unwrap();
    assert!(result.get("processed_at").is_some());
}

#[test]
fn timestamp_stage_non_object_returns_error() {
    let stage = TimestampStage::new();
    let result = stage.process(json!("string"));
    assert!(result.is_err());
}

#[test]
fn timestamp_stage_name() {
    let stage = TimestampStage::new();
    assert_eq!(stage.name(), "timestamp");
}

#[test]
fn redact_stage_removes_fields() {
    let stage = RedactStage::new(vec!["secret".into(), "password".into()]);
    let ev = json!({"type": "test", "secret": "key123", "password": "pass", "name": "ok"});
    let result = stage.process(ev).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert!(result.get("password").is_none());
    assert_eq!(result["name"], "ok");
}

#[test]
fn redact_stage_non_object_returns_error() {
    let stage = RedactStage::new(vec!["x".into()]);
    assert!(stage.process(json!(42)).is_err());
}

#[test]
fn redact_stage_name() {
    let stage = RedactStage::new(vec![]);
    assert_eq!(stage.name(), "redact");
}

#[test]
fn validate_stage_passes_valid() {
    let stage = ValidateStage::new(vec!["type".into(), "ts".into()]);
    let ev = json!({"type": "test", "ts": "2024-01-01"});
    let result = stage.process(ev).unwrap();
    assert!(result.is_some());
}

#[test]
fn validate_stage_fails_missing_field() {
    let stage = ValidateStage::new(vec!["type".into(), "ts".into()]);
    let ev = json!({"type": "test"});
    let result = stage.process(ev);
    assert!(result.is_err());
}

#[test]
fn validate_stage_non_object_error() {
    let stage = ValidateStage::new(vec!["type".into()]);
    assert!(stage.process(json!(42)).is_err());
}

#[test]
fn validate_stage_name() {
    let stage = ValidateStage::new(vec![]);
    assert_eq!(stage.name(), "validate");
}

#[test]
fn pipeline_multiple_stages() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
    pipeline.add_stage(Box::new(RedactStage::new(vec!["secret".into()])));
    pipeline.add_stage(Box::new(TimestampStage::new()));
    assert_eq!(pipeline.stage_count(), 3);

    let ev = json!({"type": "test", "secret": "key"});
    let result = pipeline.process(ev).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert!(result.get("processed_at").is_some());
}

#[test]
fn pipeline_error_display() {
    let e = PipelineError::StageError {
        stage: "validate".into(),
        message: "missing field".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("validate"));
    assert!(s.contains("missing field"));
}

#[test]
fn pipeline_invalid_event_display() {
    let e = PipelineError::InvalidEvent;
    let s = format!("{e}");
    assert!(s.contains("not a valid JSON object"));
}

// ═══════════════════════════════════════════════════════════════════════
// Transform (typed, abp-core based)
// ═══════════════════════════════════════════════════════════════════════

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[test]
fn redact_transformer_redacts_text() {
    let rt = RedactTransformer::new(vec!["SECRET123".into()]);
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "my key is SECRET123 ok".into(),
    });
    let result = rt.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(text.contains("[REDACTED]"));
            assert!(!text.contains("SECRET123"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_name() {
    let rt = RedactTransformer::new(vec![]);
    assert_eq!(rt.name(), "redact");
}

#[test]
fn redact_transformer_redacts_delta() {
    let rt = RedactTransformer::new(vec!["API_KEY".into()]);
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "token API_KEY found".into(),
    });
    let result = rt.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => {
            assert!(!text.contains("API_KEY"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_redacts_tool_call_input() {
    let rt = RedactTransformer::new(vec!["password123".into()]);
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "auth".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({"pass": "password123"}),
    });
    let result = rt.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert!(!input.to_string().contains("password123"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_redacts_command() {
    let rt = RedactTransformer::new(vec!["TOKEN".into()]);
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "curl -H TOKEN".into(),
        exit_code: Some(0),
        output_preview: Some("TOKEN appeared".into()),
    });
    let result = rt.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::CommandExecuted {
            command,
            output_preview,
            ..
        } => {
            assert!(!command.contains("TOKEN"));
            assert!(!output_preview.as_ref().unwrap().contains("TOKEN"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn throttle_transformer_limits_events() {
    let tt = ThrottleTransformer::new(2);
    let e1 = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
    let e2 = make_event(AgentEventKind::AssistantDelta { text: "b".into() });
    let e3 = make_event(AgentEventKind::AssistantDelta { text: "c".into() });
    assert!(tt.transform(e1).is_some());
    assert!(tt.transform(e2).is_some());
    assert!(tt.transform(e3).is_none()); // over limit
}

#[test]
fn throttle_transformer_per_kind() {
    let tt = ThrottleTransformer::new(1);
    let e1 = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
    let e2 = make_event(AgentEventKind::Warning {
        message: "w".into(),
    });
    assert!(tt.transform(e1).is_some());
    assert!(tt.transform(e2).is_some()); // different kind
}

#[test]
fn throttle_transformer_name() {
    let tt = ThrottleTransformer::new(10);
    assert_eq!(tt.name(), "throttle");
}

#[test]
fn enrich_transformer_adds_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("env".into(), "test".into());
    meta.insert("region".into(), "us-east".into());
    let et = EnrichTransformer::new(meta);

    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let result = et.transform(ev).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext["env"], json!("test"));
    assert_eq!(ext["region"], json!("us-east"));
}

#[test]
fn enrich_transformer_name() {
    let et = EnrichTransformer::new(BTreeMap::new());
    assert_eq!(et.name(), "enrich");
}

#[test]
fn filter_transformer_passes_matching() {
    let ft = FilterTransformer::new(Box::new(|ev: &AgentEvent| {
        matches!(ev.kind, AgentEventKind::Warning { .. })
    }));
    let ev = make_event(AgentEventKind::Warning {
        message: "w".into(),
    });
    assert!(ft.transform(ev).is_some());
}

#[test]
fn filter_transformer_drops_non_matching() {
    let ft = FilterTransformer::new(Box::new(|ev: &AgentEvent| {
        matches!(ev.kind, AgentEventKind::Warning { .. })
    }));
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(ft.transform(ev).is_none());
}

#[test]
fn filter_transformer_name() {
    let ft = FilterTransformer::new(Box::new(|_| true));
    assert_eq!(ft.name(), "filter");
}

#[test]
fn timestamp_transformer_fixes_epoch() {
    let tt = TimestampTransformer::new();
    let ev = AgentEvent {
        ts: Utc.timestamp_opt(0, 0).unwrap(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let result = tt.transform(ev).unwrap();
    assert!(result.ts.timestamp() > 0);
}

#[test]
fn timestamp_transformer_preserves_good_ts() {
    let tt = TimestampTransformer::new();
    let now = Utc::now();
    let ev = AgentEvent {
        ts: now,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let result = tt.transform(ev).unwrap();
    assert_eq!(result.ts, now);
}

#[test]
fn timestamp_transformer_name() {
    let tt = TimestampTransformer::new();
    assert_eq!(tt.name(), "timestamp");
}

#[test]
fn transformer_chain_empty_passthrough() {
    let chain = TransformerChain::new();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let result = chain.process(ev);
    assert!(result.is_some());
}

#[test]
fn transformer_chain_default_empty() {
    let chain = TransformerChain::default();
    let ev = make_event(AgentEventKind::Warning {
        message: "w".into(),
    });
    assert!(chain.process(ev).is_some());
}

#[test]
fn transformer_chain_with_builder() {
    let chain = TransformerChain::new()
        .with(Box::new(TimestampTransformer::new()))
        .with(Box::new(RedactTransformer::new(vec!["SECRET".into()])));

    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "my SECRET data".into(),
    });
    let result = chain.process(ev).unwrap();
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(!text.contains("SECRET"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn transformer_chain_filter_drops() {
    let chain = TransformerChain::new().with(Box::new(FilterTransformer::new(Box::new(
        |ev: &AgentEvent| matches!(ev.kind, AgentEventKind::Error { .. }),
    ))));
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(chain.process(ev).is_none());
}

#[test]
fn transformer_chain_process_batch() {
    let chain = TransformerChain::new().with(Box::new(FilterTransformer::new(Box::new(
        |ev: &AgentEvent| matches!(ev.kind, AgentEventKind::Warning { .. }),
    ))));
    let events = vec![
        make_event(AgentEventKind::Warning {
            message: "w1".into(),
        }),
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::Warning {
            message: "w2".into(),
        }),
    ];
    let results = chain.process_batch(events);
    assert_eq!(results.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Typed Middleware
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn typed_middleware_chain_empty() {
    let chain = SidecarMiddlewareChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn typed_middleware_chain_default_empty() {
    let chain = SidecarMiddlewareChain::default();
    assert!(chain.is_empty());
}

#[test]
fn typed_middleware_chain_process_continue() {
    let chain = SidecarMiddlewareChain::new();
    let mut ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert_eq!(chain.process(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn metrics_middleware_counts() {
    let mw = MetricsMiddleware::new();
    let mut ev1 = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
    let mut ev2 = make_event(AgentEventKind::AssistantDelta { text: "b".into() });
    let mut ev3 = make_event(AgentEventKind::Warning {
        message: "w".into(),
    });
    mw.on_event(&mut ev1);
    mw.on_event(&mut ev2);
    mw.on_event(&mut ev3);
    assert_eq!(mw.total(), 3);
    let counts = mw.counts();
    assert_eq!(counts["assistant_delta"], 2);
    assert_eq!(counts["warning"], 1);
}

#[test]
fn metrics_middleware_default() {
    let mw = MetricsMiddleware::default();
    assert_eq!(mw.total(), 0);
    assert!(mw.timings().is_empty());
}

#[test]
fn rate_limit_middleware_allows_under_limit() {
    let mw = RateLimitMiddleware::new(100);
    let mut ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn middleware_action_equality() {
    assert_eq!(MiddlewareAction::Continue, MiddlewareAction::Continue);
    assert_eq!(MiddlewareAction::Skip, MiddlewareAction::Skip);
    assert_eq!(
        MiddlewareAction::Error("x".into()),
        MiddlewareAction::Error("x".into())
    );
    assert_ne!(MiddlewareAction::Continue, MiddlewareAction::Skip);
}

#[test]
fn typed_middleware_chain_with_builder() {
    let chain = SidecarMiddlewareChain::new()
        .with(MetricsMiddleware::new())
        .with(RateLimitMiddleware::new(1000));
    assert_eq!(chain.len(), 2);
}

#[test]
fn typed_middleware_chain_push() {
    let mut chain = SidecarMiddlewareChain::new();
    chain.push(MetricsMiddleware::new());
    assert_eq!(chain.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Diagnostics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_collector_empty() {
    let dc = DiagnosticCollector::new();
    assert!(!dc.has_errors());
    assert_eq!(dc.error_count(), 0);
    assert!(dc.diagnostics().is_empty());
}

#[test]
fn diagnostic_collector_add_info() {
    let mut dc = DiagnosticCollector::new();
    dc.add_info("SK001", "sidecar started");
    assert_eq!(dc.diagnostics().len(), 1);
    assert_eq!(dc.diagnostics()[0].code, "SK001");
    assert_eq!(dc.diagnostics()[0].level, DiagnosticLevel::Info);
}

#[test]
fn diagnostic_collector_add_warning() {
    let mut dc = DiagnosticCollector::new();
    dc.add_warning("SK002", "slow response");
    assert_eq!(dc.diagnostics().len(), 1);
    assert_eq!(dc.diagnostics()[0].level, DiagnosticLevel::Warning);
}

#[test]
fn diagnostic_collector_add_error() {
    let mut dc = DiagnosticCollector::new();
    dc.add_error("SK003", "timeout");
    assert!(dc.has_errors());
    assert_eq!(dc.error_count(), 1);
}

#[test]
fn diagnostic_collector_summary() {
    let mut dc = DiagnosticCollector::new();
    dc.add_info("I1", "info");
    dc.add_info("I2", "info2");
    dc.add_warning("W1", "warn");
    dc.add_error("E1", "err");
    dc.add(Diagnostic {
        level: DiagnosticLevel::Debug,
        code: "D1".into(),
        message: "debug".into(),
        source: None,
        timestamp: "2024-01-01T00:00:00Z".into(),
    });

    let s = dc.summary();
    assert_eq!(s.debug_count, 1);
    assert_eq!(s.info_count, 2);
    assert_eq!(s.warning_count, 1);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.total, 5);
}

#[test]
fn diagnostic_collector_by_level() {
    let mut dc = DiagnosticCollector::new();
    dc.add_info("I1", "info");
    dc.add_error("E1", "err");
    dc.add_error("E2", "err2");
    let errors = dc.by_level(DiagnosticLevel::Error);
    assert_eq!(errors.len(), 2);
    let infos = dc.by_level(DiagnosticLevel::Info);
    assert_eq!(infos.len(), 1);
}

#[test]
fn diagnostic_collector_clear() {
    let mut dc = DiagnosticCollector::new();
    dc.add_info("I1", "info");
    dc.add_error("E1", "err");
    dc.clear();
    assert!(dc.diagnostics().is_empty());
    assert!(!dc.has_errors());
}

#[test]
fn diagnostic_level_ordering() {
    assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
    assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
    assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
}

#[test]
fn diagnostic_level_serde() {
    let level = DiagnosticLevel::Warning;
    let s = serde_json::to_string(&level).unwrap();
    assert_eq!(s, r#""warning""#);
    let back: DiagnosticLevel = serde_json::from_str(&s).unwrap();
    assert_eq!(back, DiagnosticLevel::Warning);
}

#[test]
fn diagnostic_summary_default() {
    let s = DiagnosticSummary::default();
    assert_eq!(s.total, 0);
    assert_eq!(s.error_count, 0);
}

#[test]
fn sidecar_diagnostics_serde() {
    let sd = SidecarDiagnostics {
        run_id: "r1".into(),
        diagnostics: vec![],
        pipeline_stages: vec!["validate".into()],
        transform_count: 3,
    };
    let s = serde_json::to_string(&sd).unwrap();
    let back: SidecarDiagnostics = serde_json::from_str(&s).unwrap();
    assert_eq!(back.run_id, "r1");
    assert_eq!(back.pipeline_stages.len(), 1);
    assert_eq!(back.transform_count, 3);
}

#[test]
fn diagnostic_with_source() {
    let d = Diagnostic {
        level: DiagnosticLevel::Info,
        code: "SK010".into(),
        message: "initialized".into(),
        source: Some("sidecar-kit".into()),
        timestamp: "2024-01-01T00:00:00Z".into(),
    };
    assert_eq!(d.source.as_deref(), Some("sidecar-kit"));
}

// ═══════════════════════════════════════════════════════════════════════
// SidecarError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_error_protocol_display() {
    let e = SidecarError::Protocol("bad frame".into());
    let s = format!("{e}");
    assert!(s.contains("protocol violation"));
    assert!(s.contains("bad frame"));
}

#[test]
fn sidecar_error_fatal_display() {
    let e = SidecarError::Fatal("sidecar crashed".into());
    let s = format!("{e}");
    assert!(s.contains("sidecar fatal error"));
}

#[test]
fn sidecar_error_timeout_display() {
    let e = SidecarError::Timeout;
    let s = format!("{e}");
    assert!(s.contains("timed out"));
}

#[test]
fn sidecar_error_exited_display() {
    let e = SidecarError::Exited(Some(1));
    let s = format!("{e}");
    assert!(s.contains("exited unexpectedly"));
}

#[test]
fn sidecar_error_exited_none_display() {
    let e = SidecarError::Exited(None);
    let s = format!("{e}");
    assert!(s.contains("exited unexpectedly"));
}

// ═══════════════════════════════════════════════════════════════════════
// Contract version compatibility
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_frame_uses_current_contract_version() {
    let f = hello_frame("test");
    match f {
        Frame::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, abp_core::CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn receipt_builder_uses_current_contract_version() {
    let r = ReceiptBuilder::new("r1", "b").build();
    assert_eq!(r["meta"]["contract_version"], abp_core::CONTRACT_VERSION);
}

#[test]
fn validate_frame_accepts_current_contract_version() {
    let f = Frame::Hello {
        contract_version: abp_core::CONTRACT_VERSION.into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// HelloData
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_data_backend_as() {
    use sidecar_kit::client::HelloData;
    let hd = HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test", "version": "1.0"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let map: serde_json::Map<String, Value> = hd.backend_as().unwrap();
    assert_eq!(map["id"], "test");
}

#[test]
fn hello_data_capabilities_as() {
    use sidecar_kit::client::HelloData;
    let hd = HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({"tools": true}),
        mode: Value::Null,
    };
    let map: serde_json::Map<String, Value> = hd.capabilities_as().unwrap();
    assert_eq!(map["tools"], true);
}

// ═══════════════════════════════════════════════════════════════════════
// Integration: encode → decode → validate cycle
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encode_decode_validate_hello() {
    let f = hello_frame("integration-test");
    let encoded = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    let v = validate_frame(&decoded, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
}

#[test]
fn encode_decode_validate_event() {
    let ev = event_text_delta("hello world");
    let f = event_frame("run-42", ev);
    let encoded = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    let v = validate_frame(&decoded, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
}

#[test]
fn full_protocol_sequence_via_framing() {
    let mut buf = Vec::new();
    let frames = vec![
        hello_frame("test-sidecar"),
        Frame::Run {
            id: "r1".into(),
            work_order: json!({"task": "test"}),
        },
        event_frame("r1", event_run_started("starting")),
        event_frame("r1", event_text_delta("chunk")),
        event_frame("r1", event_run_completed("done")),
        Frame::Final {
            ref_id: "r1".into(),
            receipt: ReceiptBuilder::new("r1", "test-sidecar").build(),
        },
    ];
    write_frames(&mut buf, &frames).unwrap();

    let read = read_all_frames(BufReader::new(&buf[..])).unwrap();
    assert_eq!(read.len(), 6);

    // Validate protocol state machine
    let mut state = ProtocolState::new();
    for frame in &read {
        state.advance(frame).unwrap();
    }
    assert_eq!(state.phase(), ProtocolPhase::Completed);
    assert_eq!(state.events_seen(), 3);
}

#[test]
fn write_read_many_events() {
    let mut buf = Vec::new();
    let frames: Vec<Frame> = (0..100)
        .map(|i| event_frame("r1", event_text_delta(&format!("chunk-{i}"))))
        .collect();
    write_frames(&mut buf, &frames).unwrap();
    let read = read_all_frames(BufReader::new(&buf[..])).unwrap();
    assert_eq!(read.len(), 100);
}
