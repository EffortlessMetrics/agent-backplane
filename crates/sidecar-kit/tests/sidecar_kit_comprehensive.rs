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
    self, ErrorWrapMiddleware, EventMiddleware, MiddlewareChain, TimingMiddleware,
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
        work_order: json!({"task": "hello"}),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Run { id, .. } if id == "run-1"));
}

#[test]
fn frame_event_round_trip() {
    let f = Frame::Event {
        ref_id: "run-1".into(),
        event: json!({"type": "assistant_delta", "text": "hi"}),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Event { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn frame_final_round_trip() {
    let f = Frame::Final {
        ref_id: "run-1".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let s = serde_json::to_string(&f).unwrap();
    assert!(matches!(serde_json::from_str::<Frame>(&s).unwrap(), Frame::Final { .. }));
}

#[test]
fn frame_fatal_round_trip() {
    let f = Frame::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
    };
    let s = serde_json::to_string(&f).unwrap();
    assert!(matches!(serde_json::from_str::<Frame>(&s).unwrap(), Frame::Fatal { error, .. } if error == "boom"));
}

#[test]
fn frame_fatal_no_ref_id() {
    let f = Frame::Fatal {
        ref_id: None,
        error: "oops".into(),
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Fatal { ref_id: None, .. }));
}

#[test]
fn frame_cancel_round_trip() {
    let f = Frame::Cancel {
        ref_id: "run-1".into(),
        reason: Some("timeout".into()),
    };
    let s = serde_json::to_string(&f).unwrap();
    assert!(matches!(serde_json::from_str::<Frame>(&s).unwrap(), Frame::Cancel { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn frame_cancel_no_reason() {
    let f = Frame::Cancel {
        ref_id: "run-1".into(),
        reason: None,
    };
    let s = serde_json::to_string(&f).unwrap();
    let f2: Frame = serde_json::from_str(&s).unwrap();
    assert!(matches!(f2, Frame::Cancel { reason: None, .. }));
}

#[test]
fn frame_ping_pong_round_trip() {
    let ping = Frame::Ping { seq: 42 };
    let pong = Frame::Pong { seq: 42 };
    let s1 = serde_json::to_string(&ping).unwrap();
    let s2 = serde_json::to_string(&pong).unwrap();
    assert!(matches!(serde_json::from_str::<Frame>(&s1).unwrap(), Frame::Ping { seq: 42 }));
    assert!(matches!(serde_json::from_str::<Frame>(&s2).unwrap(), Frame::Pong { seq: 42 }));
}

#[test]
fn frame_tag_is_t() {
    let f = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "x"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let v: Value = serde_json::to_value(&f).unwrap();
    assert_eq!(v["t"], "hello");
}

// ── Frame::try_event / try_final ────────────────────────────────────

#[test]
fn try_event_success() {
    let f = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"key": "value"}),
    };
    let (rid, val): (String, Value) = f.try_event().unwrap();
    assert_eq!(rid, "r1");
    assert_eq!(val["key"], "value");
}

#[test]
fn try_event_wrong_frame() {
    let f = Frame::Ping { seq: 1 };
    let res: Result<(String, Value), _> = f.try_event();
    assert!(res.is_err());
}

#[test]
fn try_final_success() {
    let f = Frame::Final {
        ref_id: "r2".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let (rid, val): (String, Value) = f.try_final().unwrap();
    assert_eq!(rid, "r2");
    assert_eq!(val["outcome"], "complete");
}

#[test]
fn try_final_wrong_frame() {
    let f = Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    };
    let res: Result<(String, Value), _> = f.try_final();
    assert!(res.is_err());
}

#[test]
fn try_event_type_mismatch() {
    let f = Frame::Event {
        ref_id: "r1".into(),
        event: json!("just a string"),
    };
    // Trying to deserialize a string as a map should fail
    let res: Result<(String, std::collections::HashMap<String, String>), _> = f.try_event();
    assert!(res.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// Codec
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codec_encode_ends_with_newline() {
    let f = Frame::Ping { seq: 1 };
    let encoded = JsonlCodec::encode(&f).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn codec_round_trip() {
    let f = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"type": "warning", "message": "test"}),
    };
    let line = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Frame::Event { ref_id, .. } if ref_id == "r1"));
}

#[test]
fn codec_decode_invalid_json() {
    let res = JsonlCodec::decode("not json");
    assert!(res.is_err());
}

#[test]
fn codec_decode_wrong_tag() {
    let res = JsonlCodec::decode(r#"{"t":"unknown_variant"}"#);
    assert!(res.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// Framing – FrameWriter / FrameReader
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn frame_writer_basic() {
    let mut buf = Vec::new();
    let mut w = FrameWriter::new(&mut buf);
    w.write_frame(&Frame::Ping { seq: 1 }).unwrap();
    w.flush().unwrap();
    assert_eq!(w.frames_written(), 1);
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains(r#""t":"ping"#));
}

#[test]
fn frame_writer_counts_multiple() {
    let mut buf = Vec::new();
    let mut w = FrameWriter::new(&mut buf);
    for i in 0..5 {
        w.write_frame(&Frame::Ping { seq: i }).unwrap();
    }
    assert_eq!(w.frames_written(), 5);
}

#[test]
fn frame_writer_size_limit() {
    let mut buf = Vec::new();
    let mut w = FrameWriter::with_max_size(&mut buf, 10);
    let f = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let res = w.write_frame(&f);
    assert!(res.is_err());
}

#[test]
fn frame_writer_inner_accessors() {
    let buf = Vec::<u8>::new();
    let w = FrameWriter::new(buf);
    assert!(w.inner().is_empty());
    let recovered = w.into_inner();
    assert!(recovered.is_empty());
}

#[test]
fn frame_reader_basic() {
    let data = r#"{"t":"ping","seq":10}
{"t":"pong","seq":10}
"#;
    let mut r = FrameReader::new(BufReader::new(data.as_bytes()));
    let f1 = r.read_frame().unwrap().unwrap();
    assert!(matches!(f1, Frame::Ping { seq: 10 }));
    let f2 = r.read_frame().unwrap().unwrap();
    assert!(matches!(f2, Frame::Pong { seq: 10 }));
    assert!(r.read_frame().unwrap().is_none()); // EOF
    assert_eq!(r.frames_read(), 2);
}

#[test]
fn frame_reader_skips_blank_lines() {
    let data = "\n\n{\"t\":\"ping\",\"seq\":1}\n\n\n";
    let mut r = FrameReader::new(BufReader::new(data.as_bytes()));
    let f = r.read_frame().unwrap().unwrap();
    assert!(matches!(f, Frame::Ping { seq: 1 }));
    assert!(r.read_frame().unwrap().is_none());
}

#[test]
fn frame_reader_size_limit() {
    let data = "{\"t\":\"ping\",\"seq\":999999}\n";
    let mut r = FrameReader::with_max_size(BufReader::new(data.as_bytes()), 5);
    let res = r.read_frame();
    assert!(res.is_err());
}

#[test]
fn frame_reader_invalid_json() {
    let data = "not-json\n";
    let mut r = FrameReader::new(BufReader::new(data.as_bytes()));
    let res = r.read_frame();
    assert!(res.is_err());
}

#[test]
fn frame_reader_empty_input() {
    let data = "";
    let mut r = FrameReader::new(BufReader::new(data.as_bytes()));
    assert!(r.read_frame().unwrap().is_none());
}

#[test]
fn frame_iter_collects() {
    let data = r#"{"t":"ping","seq":1}
{"t":"ping","seq":2}
{"t":"ping","seq":3}
"#;
    let r = FrameReader::new(BufReader::new(data.as_bytes()));
    let frames: Vec<Frame> = r.frames().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(frames.len(), 3);
}

#[test]
fn frame_iter_stops_on_error() {
    let data = "{\"t\":\"ping\",\"seq\":1}\nnot-json\n";
    let r = FrameReader::new(BufReader::new(data.as_bytes()));
    let results: Vec<_> = r.frames().collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

// ── Convenience helpers ─────────────────────────────────────────────

#[test]
fn write_frames_helper() {
    let mut buf = Vec::new();
    let frames = vec![Frame::Ping { seq: 1 }, Frame::Pong { seq: 1 }];
    let count = write_frames(&mut buf, &frames).unwrap();
    assert_eq!(count, 2);
}

#[test]
fn read_all_frames_helper() {
    let data = r#"{"t":"ping","seq":1}
{"t":"pong","seq":1}
"#;
    let frames = read_all_frames(BufReader::new(data.as_bytes())).unwrap();
    assert_eq!(frames.len(), 2);
}

#[test]
fn frame_to_json_helper() {
    let f = Frame::Ping { seq: 42 };
    let json_str = frame_to_json(&f).unwrap();
    assert!(!json_str.ends_with('\n'));
    assert!(json_str.contains("42"));
}

#[test]
fn json_to_frame_helper() {
    let f = json_to_frame(r#"{"t":"pong","seq":7}"#).unwrap();
    assert!(matches!(f, Frame::Pong { seq: 7 }));
}

#[test]
fn json_to_frame_invalid() {
    assert!(json_to_frame("garbage").is_err());
}

#[test]
fn buf_reader_from_bytes_helper() {
    let br = buf_reader_from_bytes(b"hello\n");
    let mut r = FrameReader::new(br);
    assert!(r.read_frame().is_err()); // not valid JSON, but it reads
}

#[test]
fn write_then_read_frames_round_trip() {
    let frames = vec![
        hello_frame("test-backend"),
        Frame::Run { id: "r1".into(), work_order: json!({"task": "x"}) },
        event_frame("r1", event_text_delta("chunk")),
        Frame::Final { ref_id: "r1".into(), receipt: json!({"outcome": "complete"}) },
    ];
    let mut buf = Vec::new();
    write_frames(&mut buf, &frames).unwrap();
    let read_back = read_all_frames(BufReader::new(buf.as_slice())).unwrap();
    assert_eq!(read_back.len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════
// Frame validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_valid_hello() {
    let f = hello_frame("my-backend");
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid, "issues: {:?}", v.issues);
}

#[test]
fn validate_hello_empty_version() {
    let f = Frame::Hello {
        contract_version: "".into(),
        backend: json!({"id": "x"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("contract_version is empty")));
}

#[test]
fn validate_hello_bad_version_prefix() {
    let f = Frame::Hello {
        contract_version: "v0.1".into(),
        backend: json!({"id": "x"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("does not start with")));
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
    let f = Frame::Run { id: "".into(), work_order: json!({}) };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("run id is empty")));
}

#[test]
fn validate_event_empty_ref_id() {
    let f = Frame::Event { ref_id: "".into(), event: json!({}) };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_final_empty_ref_id() {
    let f = Frame::Final { ref_id: "".into(), receipt: json!({}) };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_fatal_empty_error() {
    let f = Frame::Fatal { ref_id: None, error: "".into() };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("fatal error message is empty")));
}

#[test]
fn validate_cancel_empty_ref_id() {
    let f = Frame::Cancel { ref_id: "".into(), reason: None };
    let v = validate_frame(&f, DEFAULT_MAX_FRAME_SIZE);
    assert!(!v.valid);
}

#[test]
fn validate_ping_always_valid() {
    let v = validate_frame(&Frame::Ping { seq: 0 }, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
}

#[test]
fn validate_pong_always_valid() {
    let v = validate_frame(&Frame::Pong { seq: u64::MAX }, DEFAULT_MAX_FRAME_SIZE);
    assert!(v.valid);
}

#[test]
fn validate_size_exceeded() {
    let f = Frame::Ping { seq: 1 };
    let v = validate_frame(&f, 5);
    assert!(!v.valid);
    assert!(v.issues.iter().any(|i| i.contains("exceeds limit")));
}

// ═══════════════════════════════════════════════════════════════════════
// Error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_display_spawn() {
    let err = SidecarError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "nope"));
    let msg = err.to_string();
    assert!(msg.contains("spawn"));
}

#[test]
fn error_display_protocol() {
    let err = SidecarError::Protocol("bad state".into());
    assert!(err.to_string().contains("protocol violation"));
}

#[test]
fn error_display_fatal() {
    let err = SidecarError::Fatal("critical".into());
    assert!(err.to_string().contains("critical"));
}

#[test]
fn error_display_timeout() {
    let err = SidecarError::Timeout;
    assert!(err.to_string().contains("timed out"));
}

#[test]
fn error_display_exited() {
    let err = SidecarError::Exited(Some(1));
    assert!(err.to_string().contains("1"));
}

#[test]
fn error_display_exited_none() {
    let err = SidecarError::Exited(None);
    assert!(err.to_string().contains("None"));
}

// ═══════════════════════════════════════════════════════════════════════
// Protocol state machine
// ═══════════════════════════════════════════════════════════════════════

fn make_hello() -> Frame {
    hello_frame("test")
}

fn make_run(id: &str) -> Frame {
    Frame::Run { id: id.into(), work_order: json!({}) }
}

fn make_event(ref_id: &str) -> Frame {
    Frame::Event { ref_id: ref_id.into(), event: json!({}) }
}

fn make_final(ref_id: &str) -> Frame {
    Frame::Final { ref_id: ref_id.into(), receipt: json!({}) }
}

#[test]
fn protocol_happy_path() {
    let mut ps = ProtocolState::new();
    assert_eq!(ps.phase(), ProtocolPhase::AwaitingHello);
    ps.advance(&make_hello()).unwrap();
    assert_eq!(ps.phase(), ProtocolPhase::AwaitingRun);
    ps.advance(&make_run("r1")).unwrap();
    assert_eq!(ps.phase(), ProtocolPhase::Streaming);
    assert_eq!(ps.run_id(), Some("r1"));
    ps.advance(&make_event("r1")).unwrap();
    ps.advance(&make_event("r1")).unwrap();
    assert_eq!(ps.events_seen(), 2);
    ps.advance(&make_final("r1")).unwrap();
    assert_eq!(ps.phase(), ProtocolPhase::Completed);
    assert!(ps.is_terminal());
}

#[test]
fn protocol_default_is_awaiting_hello() {
    let ps = ProtocolState::default();
    assert_eq!(ps.phase(), ProtocolPhase::AwaitingHello);
}

#[test]
fn protocol_event_before_hello_faults() {
    let mut ps = ProtocolState::new();
    let res = ps.advance(&make_event("r1"));
    assert!(res.is_err());
    assert_eq!(ps.phase(), ProtocolPhase::Faulted);
    assert!(ps.fault_reason().is_some());
    assert!(ps.is_terminal());
}

#[test]
fn protocol_run_before_hello_faults() {
    let mut ps = ProtocolState::new();
    assert!(ps.advance(&make_run("r1")).is_err());
    assert_eq!(ps.phase(), ProtocolPhase::Faulted);
}

#[test]
fn protocol_event_before_run_faults() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    assert!(ps.advance(&make_event("r1")).is_err());
    assert_eq!(ps.phase(), ProtocolPhase::Faulted);
}

#[test]
fn protocol_fatal_during_awaiting_run() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&Frame::Fatal { ref_id: None, error: "die".into() }).unwrap();
    assert_eq!(ps.phase(), ProtocolPhase::Completed);
}

#[test]
fn protocol_fatal_during_streaming() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    ps.advance(&Frame::Fatal { ref_id: Some("r1".into()), error: "die".into() }).unwrap();
    assert_eq!(ps.phase(), ProtocolPhase::Completed);
}

#[test]
fn protocol_fatal_streaming_no_ref_id() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    ps.advance(&Frame::Fatal { ref_id: None, error: "die".into() }).unwrap();
    assert_eq!(ps.phase(), ProtocolPhase::Completed);
}

#[test]
fn protocol_ref_id_mismatch() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    let res = ps.advance(&make_event("r2"));
    assert!(res.is_err());
}

#[test]
fn protocol_final_ref_id_mismatch() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    let res = ps.advance(&make_final("wrong"));
    assert!(res.is_err());
}

#[test]
fn protocol_ping_during_streaming() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    ps.advance(&Frame::Ping { seq: 1 }).unwrap();
    ps.advance(&Frame::Pong { seq: 1 }).unwrap();
    assert_eq!(ps.phase(), ProtocolPhase::Streaming);
}

#[test]
fn protocol_after_completed_faults() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    ps.advance(&make_final("r1")).unwrap();
    assert!(ps.advance(&make_event("r1")).is_err());
    assert_eq!(ps.phase(), ProtocolPhase::Faulted);
}

#[test]
fn protocol_faulted_stays_faulted() {
    let mut ps = ProtocolState::new();
    let _ = ps.advance(&make_event("r1"));
    assert_eq!(ps.phase(), ProtocolPhase::Faulted);
    assert!(ps.advance(&make_hello()).is_err());
}

#[test]
fn protocol_reset() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    ps.advance(&make_event("r1")).unwrap();
    ps.reset();
    assert_eq!(ps.phase(), ProtocolPhase::AwaitingHello);
    assert_eq!(ps.run_id(), None);
    assert_eq!(ps.events_seen(), 0);
    assert!(ps.fault_reason().is_none());
}

#[test]
fn protocol_reset_from_faulted() {
    let mut ps = ProtocolState::new();
    let _ = ps.advance(&make_event("r1"));
    assert_eq!(ps.phase(), ProtocolPhase::Faulted);
    ps.reset();
    assert_eq!(ps.phase(), ProtocolPhase::AwaitingHello);
    ps.advance(&make_hello()).unwrap();
}

#[test]
fn protocol_hello_during_streaming_faults() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    assert!(ps.advance(&make_hello()).is_err());
    assert_eq!(ps.phase(), ProtocolPhase::Faulted);
}

#[test]
fn protocol_run_during_streaming_faults() {
    let mut ps = ProtocolState::new();
    ps.advance(&make_hello()).unwrap();
    ps.advance(&make_run("r1")).unwrap();
    assert!(ps.advance(&make_run("r2")).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// ProcessSpec
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_spec_new() {
    let s = ProcessSpec::new("echo");
    assert_eq!(s.command, "echo");
    assert!(s.args.is_empty());
    assert!(s.env.is_empty());
    assert!(s.cwd.is_none());
}

#[test]
fn process_spec_with_fields() {
    let mut s = ProcessSpec::new("node");
    s.args = vec!["index.js".into()];
    s.env.insert("FOO".into(), "bar".into());
    s.cwd = Some("/tmp".into());
    assert_eq!(s.args.len(), 1);
    assert_eq!(s.env["FOO"], "bar");
    assert_eq!(s.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn process_spec_clone() {
    let s = ProcessSpec::new("python");
    let s2 = s.clone();
    assert_eq!(s2.command, "python");
}

// ═══════════════════════════════════════════════════════════════════════
// CancelToken
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancel_token_new_not_cancelled() {
    let ct = CancelToken::new();
    assert!(!ct.is_cancelled());
}

#[test]
fn cancel_token_default_not_cancelled() {
    let ct = CancelToken::default();
    assert!(!ct.is_cancelled());
}

#[test]
fn cancel_token_cancel_signals() {
    let ct = CancelToken::new();
    ct.cancel();
    assert!(ct.is_cancelled());
}

#[test]
fn cancel_token_clone_shares_state() {
    let ct = CancelToken::new();
    let ct2 = ct.clone();
    ct.cancel();
    assert!(ct2.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancelled_future_returns_when_cancelled() {
    let ct = CancelToken::new();
    ct.cancel();
    // Should return immediately since already cancelled
    ct.cancelled().await;
    assert!(ct.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancelled_future_waits() {
    let ct = CancelToken::new();
    let ct2 = ct.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        ct2.cancel();
    });
    ct.cancelled().await;
    assert!(ct.is_cancelled());
}

// ═══════════════════════════════════════════════════════════════════════
// Builders – event helpers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builder_event_text_delta() {
    let v = event_text_delta("hello");
    assert_eq!(v["type"], "assistant_delta");
    assert_eq!(v["text"], "hello");
    assert!(v["ts"].is_string());
}

#[test]
fn builder_event_text_message() {
    let v = event_text_message("world");
    assert_eq!(v["type"], "assistant_message");
    assert_eq!(v["text"], "world");
}

#[test]
fn builder_event_tool_call() {
    let v = event_tool_call("bash", Some("tc-1"), json!({"command": "ls"}));
    assert_eq!(v["type"], "tool_call");
    assert_eq!(v["tool_name"], "bash");
    assert_eq!(v["tool_use_id"], "tc-1");
    assert!(v["input"].is_object());
}

#[test]
fn builder_event_tool_call_no_id() {
    let v = event_tool_call("bash", None, json!({}));
    assert!(v["tool_use_id"].is_null());
}

#[test]
fn builder_event_tool_result() {
    let v = event_tool_result("bash", Some("tc-1"), json!("output"), false);
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["is_error"], false);
}

#[test]
fn builder_event_tool_result_error() {
    let v = event_tool_result("bash", None, json!("err"), true);
    assert_eq!(v["is_error"], true);
}

#[test]
fn builder_event_error() {
    let v = event_error("something broke");
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "something broke");
}

#[test]
fn builder_event_warning() {
    let v = event_warning("heads up");
    assert_eq!(v["type"], "warning");
    assert_eq!(v["message"], "heads up");
}

#[test]
fn builder_event_run_started() {
    let v = event_run_started("beginning run");
    assert_eq!(v["type"], "run_started");
}

#[test]
fn builder_event_run_completed() {
    let v = event_run_completed("done");
    assert_eq!(v["type"], "run_completed");
}

#[test]
fn builder_event_file_changed() {
    let v = event_file_changed("src/main.rs", "added fn");
    assert_eq!(v["type"], "file_changed");
    assert_eq!(v["path"], "src/main.rs");
    assert_eq!(v["summary"], "added fn");
}

#[test]
fn builder_event_command_executed() {
    let v = event_command_executed("ls -la", Some(0), Some("total 10"));
    assert_eq!(v["type"], "command_executed");
    assert_eq!(v["command"], "ls -la");
    assert_eq!(v["exit_code"], 0);
    assert_eq!(v["output_preview"], "total 10");
}

#[test]
fn builder_event_command_executed_no_optionals() {
    let v = event_command_executed("foo", None, None);
    assert!(v["exit_code"].is_null());
    assert!(v["output_preview"].is_null());
}

// ── Frame helpers ───────────────────────────────────────────────────

#[test]
fn builder_event_frame() {
    let f = event_frame("r1", event_text_delta("x"));
    assert!(matches!(f, Frame::Event { ref_id, .. } if ref_id == "r1"));
}

#[test]
fn builder_fatal_frame() {
    let f = fatal_frame(Some("r1"), "boom");
    assert!(matches!(f, Frame::Fatal { ref_id: Some(r), error } if r == "r1" && error == "boom"));
}

#[test]
fn builder_fatal_frame_no_ref() {
    let f = fatal_frame(None, "oops");
    assert!(matches!(f, Frame::Fatal { ref_id: None, .. }));
}

#[test]
fn builder_hello_frame() {
    let f = hello_frame("my-backend");
    match f {
        Frame::Hello { contract_version, backend, .. } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "my-backend");
        }
        _ => panic!("expected Hello"),
    }
}

// ── ReceiptBuilder ──────────────────────────────────────────────────

#[test]
fn receipt_builder_basic() {
    let r = ReceiptBuilder::new("r1", "test-be").build();
    assert_eq!(r["meta"]["run_id"], "r1");
    assert_eq!(r["backend"]["id"], "test-be");
    assert_eq!(r["outcome"], "complete");
    assert!(r["receipt_sha256"].is_null());
}

#[test]
fn receipt_builder_failed() {
    let r = ReceiptBuilder::new("r1", "be").failed().build();
    assert_eq!(r["outcome"], "failed");
}

#[test]
fn receipt_builder_partial() {
    let r = ReceiptBuilder::new("r1", "be").partial().build();
    assert_eq!(r["outcome"], "partial");
}

#[test]
fn receipt_builder_with_events() {
    let r = ReceiptBuilder::new("r1", "be")
        .event(event_text_delta("hello"))
        .event(event_text_delta("world"))
        .build();
    assert_eq!(r["trace"].as_array().unwrap().len(), 2);
}

#[test]
fn receipt_builder_with_artifacts() {
    let r = ReceiptBuilder::new("r1", "be")
        .artifact("file", "src/main.rs")
        .build();
    let arts = r["artifacts"].as_array().unwrap();
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0]["kind"], "file");
    assert_eq!(arts[0]["path"], "src/main.rs");
}

#[test]
fn receipt_builder_with_usage() {
    let r = ReceiptBuilder::new("r1", "be")
        .input_tokens(100)
        .output_tokens(200)
        .usage_raw(json!({"total": 300}))
        .build();
    assert_eq!(r["usage"]["input_tokens"], 100);
    assert_eq!(r["usage"]["output_tokens"], 200);
    assert_eq!(r["usage_raw"]["total"], 300);
}

#[test]
fn receipt_builder_has_contract_version() {
    let r = ReceiptBuilder::new("r1", "be").build();
    assert_eq!(r["meta"]["contract_version"], "abp/v0.1");
}

// ═══════════════════════════════════════════════════════════════════════
// Middleware (value-based)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn logging_middleware_passthrough() {
    let mw = middleware::LoggingMiddleware::new();
    let ev = json!({"type": "warning", "message": "test"});
    let out = mw.process(&ev);
    assert!(out.is_some());
    assert_eq!(out.unwrap(), ev);
}

#[test]
fn filter_middleware_include() {
    let mw = middleware::FilterMiddleware::include_kinds(&["warning", "error"]);
    assert!(mw.process(&json!({"type": "warning"})).is_some());
    assert!(mw.process(&json!({"type": "error"})).is_some());
    assert!(mw.process(&json!({"type": "assistant_delta"})).is_none());
}

#[test]
fn filter_middleware_include_case_insensitive() {
    let mw = middleware::FilterMiddleware::include_kinds(&["Warning"]);
    assert!(mw.process(&json!({"type": "warning"})).is_some());
    assert!(mw.process(&json!({"type": "WARNING"})).is_some());
}

#[test]
fn filter_middleware_include_empty_blocks_all() {
    let mw = middleware::FilterMiddleware::include_kinds(&[]);
    assert!(mw.process(&json!({"type": "warning"})).is_none());
}

#[test]
fn filter_middleware_exclude() {
    let mw = middleware::FilterMiddleware::exclude_kinds(&["assistant_delta"]);
    assert!(mw.process(&json!({"type": "warning"})).is_some());
    assert!(mw.process(&json!({"type": "assistant_delta"})).is_none());
}

#[test]
fn filter_middleware_exclude_empty_passes_all() {
    let mw = middleware::FilterMiddleware::exclude_kinds(&[]);
    assert!(mw.process(&json!({"type": "anything"})).is_some());
}

#[test]
fn filter_middleware_missing_type_field() {
    let mw = middleware::FilterMiddleware::include_kinds(&["warning"]);
    assert!(mw.process(&json!({"no_type": true})).is_none());
}

#[test]
fn timing_middleware_adds_field() {
    let mw = TimingMiddleware::new();
    let ev = json!({"type": "test"});
    let out = mw.process(&ev).unwrap();
    assert!(out.get("_processing_us").is_some());
}

#[test]
fn timing_middleware_non_object() {
    let mw = TimingMiddleware::new();
    let ev = json!("string");
    let out = mw.process(&ev).unwrap();
    // Non-objects don't get the field
    assert!(out.get("_processing_us").is_none());
}

#[test]
fn error_wrap_middleware_passes_objects() {
    let mw = ErrorWrapMiddleware::new();
    let ev = json!({"type": "test"});
    let out = mw.process(&ev).unwrap();
    assert_eq!(out, ev);
}

#[test]
fn error_wrap_middleware_wraps_non_objects() {
    let mw = ErrorWrapMiddleware::new();
    let ev = json!(42);
    let out = mw.process(&ev).unwrap();
    assert_eq!(out["type"], "error");
    assert!(out["message"].as_str().unwrap().contains("non-object"));
    assert_eq!(out["_original"], 42);
}

#[test]
fn middleware_chain_empty_is_passthrough() {
    let chain = MiddlewareChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    let ev = json!({"type": "test"});
    assert_eq!(chain.process(&ev).unwrap(), ev);
}

#[test]
fn middleware_chain_with_builder() {
    let chain = MiddlewareChain::new()
        .with(middleware::LoggingMiddleware::new())
        .with(TimingMiddleware::new());
    assert_eq!(chain.len(), 2);
    let ev = json!({"type": "test"});
    let out = chain.process(&ev).unwrap();
    assert!(out.get("_processing_us").is_some());
}

#[test]
fn middleware_chain_filter_drops() {
    let chain = MiddlewareChain::new()
        .with(middleware::FilterMiddleware::include_kinds(&["error"]))
        .with(middleware::LoggingMiddleware::new());
    assert!(chain.process(&json!({"type": "warning"})).is_none());
    assert!(chain.process(&json!({"type": "error"})).is_some());
}

#[test]
fn middleware_chain_push() {
    let mut chain = MiddlewareChain::new();
    chain.push(middleware::LoggingMiddleware::new());
    assert_eq!(chain.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Pipeline
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_empty_is_passthrough() {
    let p = EventPipeline::new();
    assert_eq!(p.stage_count(), 0);
    let ev = json!({"type": "test"});
    let out = p.process(ev.clone()).unwrap().unwrap();
    assert_eq!(out, ev);
}

#[test]
fn pipeline_default() {
    let p = EventPipeline::default();
    assert_eq!(p.stage_count(), 0);
}

#[test]
fn timestamp_stage_adds_processed_at() {
    let stage = TimestampStage::new();
    assert_eq!(stage.name(), "timestamp");
    let ev = json!({"type": "test"});
    let out = stage.process(ev).unwrap().unwrap();
    assert!(out.get("processed_at").is_some());
}

#[test]
fn timestamp_stage_rejects_non_object() {
    let stage = TimestampStage::new();
    let res = stage.process(json!("not an object"));
    assert!(matches!(res, Err(PipelineError::InvalidEvent)));
}

#[test]
fn redact_stage_removes_fields() {
    let stage = RedactStage::new(vec!["secret".into(), "password".into()]);
    assert_eq!(stage.name(), "redact");
    let ev = json!({"type": "test", "secret": "abc", "password": "123", "safe": true});
    let out = stage.process(ev).unwrap().unwrap();
    assert!(out.get("secret").is_none());
    assert!(out.get("password").is_none());
    assert_eq!(out["safe"], true);
}

#[test]
fn redact_stage_rejects_non_object() {
    let stage = RedactStage::new(vec!["x".into()]);
    assert!(matches!(stage.process(json!(42)), Err(PipelineError::InvalidEvent)));
}

#[test]
fn validate_stage_passes_when_fields_present() {
    let stage = ValidateStage::new(vec!["type".into(), "ts".into()]);
    assert_eq!(stage.name(), "validate");
    let ev = json!({"type": "test", "ts": "now"});
    assert!(stage.process(ev).unwrap().is_some());
}

#[test]
fn validate_stage_fails_on_missing_field() {
    let stage = ValidateStage::new(vec!["type".into(), "required_field".into()]);
    let ev = json!({"type": "test"});
    let res = stage.process(ev);
    assert!(matches!(res, Err(PipelineError::StageError { stage, message })
        if stage == "validate" && message.contains("required_field")));
}

#[test]
fn validate_stage_rejects_non_object() {
    let stage = ValidateStage::new(vec![]);
    assert!(matches!(stage.process(json!("str")), Err(PipelineError::InvalidEvent)));
}

#[test]
fn pipeline_multi_stage() {
    let mut p = EventPipeline::new();
    p.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
    p.add_stage(Box::new(RedactStage::new(vec!["secret".into()])));
    p.add_stage(Box::new(TimestampStage::new()));
    assert_eq!(p.stage_count(), 3);

    let ev = json!({"type": "test", "secret": "key"});
    let out = p.process(ev).unwrap().unwrap();
    assert!(out.get("secret").is_none());
    assert!(out.get("processed_at").is_some());
}

#[test]
fn pipeline_error_display() {
    let err = PipelineError::StageError {
        stage: "validate".into(),
        message: "missing field".into(),
    };
    let s = err.to_string();
    assert!(s.contains("validate"));
    assert!(s.contains("missing field"));

    let err2 = PipelineError::InvalidEvent;
    assert!(err2.to_string().contains("not a valid JSON object"));
}

// ═══════════════════════════════════════════════════════════════════════
// Diagnostics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_level_ordering() {
    assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
    assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
    assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
}

#[test]
fn diagnostic_collector_new_empty() {
    let c = DiagnosticCollector::new();
    assert_eq!(c.diagnostics().len(), 0);
    assert!(!c.has_errors());
    assert_eq!(c.error_count(), 0);
}

#[test]
fn diagnostic_collector_add_info() {
    let mut c = DiagnosticCollector::new();
    c.add_info("SK001", "info message");
    assert_eq!(c.diagnostics().len(), 1);
    assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Info);
    assert_eq!(c.diagnostics()[0].code, "SK001");
}

#[test]
fn diagnostic_collector_add_warning() {
    let mut c = DiagnosticCollector::new();
    c.add_warning("SK002", "warning message");
    assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Warning);
}

#[test]
fn diagnostic_collector_add_error() {
    let mut c = DiagnosticCollector::new();
    c.add_error("SK003", "error message");
    assert!(c.has_errors());
    assert_eq!(c.error_count(), 1);
}

#[test]
fn diagnostic_collector_by_level() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "info");
    c.add_warning("W1", "warn");
    c.add_error("E1", "err1");
    c.add_error("E2", "err2");
    assert_eq!(c.by_level(DiagnosticLevel::Info).len(), 1);
    assert_eq!(c.by_level(DiagnosticLevel::Warning).len(), 1);
    assert_eq!(c.by_level(DiagnosticLevel::Error).len(), 2);
    assert_eq!(c.by_level(DiagnosticLevel::Debug).len(), 0);
}

#[test]
fn diagnostic_collector_summary() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "info");
    c.add_info("I2", "info");
    c.add_warning("W1", "warn");
    c.add_error("E1", "err");

    let s = c.summary();
    assert_eq!(s.info_count, 2);
    assert_eq!(s.warning_count, 1);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.debug_count, 0);
    assert_eq!(s.total, 4);
}

#[test]
fn diagnostic_collector_clear() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "info");
    c.add_error("E1", "err");
    c.clear();
    assert_eq!(c.diagnostics().len(), 0);
    assert!(!c.has_errors());
}

#[test]
fn diagnostic_collector_add_custom() {
    let mut c = DiagnosticCollector::new();
    c.add(Diagnostic {
        level: DiagnosticLevel::Debug,
        code: "DBG01".into(),
        message: "debug detail".into(),
        source: Some("test".into()),
        timestamp: "2024-01-01T00:00:00Z".into(),
    });
    assert_eq!(c.diagnostics()[0].source, Some("test".into()));
    let s = c.summary();
    assert_eq!(s.debug_count, 1);
}

#[test]
fn diagnostic_summary_default() {
    let s = DiagnosticSummary::default();
    assert_eq!(s.total, 0);
    assert_eq!(s.debug_count, 0);
}

#[test]
fn sidecar_diagnostics_struct() {
    let sd = SidecarDiagnostics {
        run_id: "r1".into(),
        diagnostics: vec![],
        pipeline_stages: vec!["timestamp".into()],
        transform_count: 3,
    };
    assert_eq!(sd.run_id, "r1");
    assert_eq!(sd.pipeline_stages.len(), 1);
    assert_eq!(sd.transform_count, 3);
}

#[test]
fn diagnostic_level_serde_round_trip() {
    let s = serde_json::to_string(&DiagnosticLevel::Warning).unwrap();
    assert_eq!(s, "\"warning\"");
    let d: DiagnosticLevel = serde_json::from_str(&s).unwrap();
    assert_eq!(d, DiagnosticLevel::Warning);
}

// ═══════════════════════════════════════════════════════════════════════
// Transform (typed, abp-core)
// ═══════════════════════════════════════════════════════════════════════

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[test]
fn redact_transformer_text_delta() {
    let rt = RedactTransformer::new(vec!["SECRET_KEY".into()]);
    assert_eq!(rt.name(), "redact");
    let ev = make_agent_event(AgentEventKind::AssistantDelta {
        text: "my SECRET_KEY is here".into(),
    });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::AssistantDelta { text } => {
            assert!(text.contains("[REDACTED]"));
            assert!(!text.contains("SECRET_KEY"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_text_message() {
    let rt = RedactTransformer::new(vec!["pw123".into()]);
    let ev = make_agent_event(AgentEventKind::AssistantMessage {
        text: "password is pw123".into(),
    });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::AssistantMessage { text } => assert!(!text.contains("pw123")),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_run_started() {
    let rt = RedactTransformer::new(vec!["secret".into()]);
    let ev = make_agent_event(AgentEventKind::RunStarted { message: "secret launch".into() });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::RunStarted { message } => assert!(message.contains("[REDACTED]")),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_run_completed() {
    let rt = RedactTransformer::new(vec!["token".into()]);
    let ev = make_agent_event(AgentEventKind::RunCompleted { message: "used token abc".into() });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::RunCompleted { message } => assert!(message.contains("[REDACTED]")),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_tool_call_input() {
    let rt = RedactTransformer::new(vec!["api_key_123".into()]);
    let ev = make_agent_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({"key": "api_key_123"}),
    });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::ToolCall { input, .. } => {
            let s = serde_json::to_string(input).unwrap();
            assert!(!s.contains("api_key_123"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_tool_result_output() {
    let rt = RedactTransformer::new(vec!["secret".into()]);
    let ev = make_agent_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!({"result": "secret data"}),
        is_error: false,
    });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::ToolResult { output, .. } => {
            let s = serde_json::to_string(output).unwrap();
            assert!(!s.contains("secret"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_warning() {
    let rt = RedactTransformer::new(vec!["pw".into()]);
    let ev = make_agent_event(AgentEventKind::Warning { message: "pw exposed".into() });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::Warning { message } => assert!(!message.contains("pw")),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_error() {
    let rt = RedactTransformer::new(vec!["token".into()]);
    let ev = make_agent_event(AgentEventKind::Error {
        message: "token invalid".into(),
        error_code: None,
    });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::Error { message, .. } => assert!(!message.contains("token")),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_file_changed() {
    let rt = RedactTransformer::new(vec!["secret".into()]);
    let ev = make_agent_event(AgentEventKind::FileChanged {
        path: "file.txt".into(),
        summary: "secret content".into(),
    });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::FileChanged { path, summary } => {
            assert_eq!(path, "file.txt"); // path not redacted
            assert!(summary.contains("[REDACTED]"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_command_executed() {
    let rt = RedactTransformer::new(vec!["API_KEY".into()]);
    let ev = make_agent_event(AgentEventKind::CommandExecuted {
        command: "curl -H API_KEY".into(),
        exit_code: Some(0),
        output_preview: Some("API_KEY in output".into()),
    });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::CommandExecuted { command, output_preview, .. } => {
            assert!(!command.contains("API_KEY"));
            assert!(!output_preview.as_ref().unwrap().contains("API_KEY"));
        }
        _ => panic!("wrong kind"),
    }
}

#[test]
fn redact_transformer_empty_pattern_ignored() {
    let rt = RedactTransformer::new(vec!["".into(), "x".into()]);
    let ev = make_agent_event(AgentEventKind::AssistantDelta { text: "xyz".into() });
    let out = rt.transform(ev).unwrap();
    match &out.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "[REDACTED]yz"),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn throttle_transformer_allows_up_to_max() {
    let tt = ThrottleTransformer::new(2);
    assert_eq!(tt.name(), "throttle");
    let ev1 = make_agent_event(AgentEventKind::AssistantDelta { text: "a".into() });
    let ev2 = make_agent_event(AgentEventKind::AssistantDelta { text: "b".into() });
    let ev3 = make_agent_event(AgentEventKind::AssistantDelta { text: "c".into() });
    assert!(tt.transform(ev1).is_some());
    assert!(tt.transform(ev2).is_some());
    assert!(tt.transform(ev3).is_none());
}

#[test]
fn throttle_transformer_per_kind() {
    let tt = ThrottleTransformer::new(1);
    let ev_delta = make_agent_event(AgentEventKind::AssistantDelta { text: "a".into() });
    let ev_warn = make_agent_event(AgentEventKind::Warning { message: "w".into() });
    assert!(tt.transform(ev_delta).is_some());
    assert!(tt.transform(ev_warn).is_some());
    // Second of each kind gets throttled
    let ev_delta2 = make_agent_event(AgentEventKind::AssistantDelta { text: "b".into() });
    let ev_warn2 = make_agent_event(AgentEventKind::Warning { message: "w2".into() });
    assert!(tt.transform(ev_delta2).is_none());
    assert!(tt.transform(ev_warn2).is_none());
}

#[test]
fn enrich_transformer_adds_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("env".into(), "test".into());
    meta.insert("version".into(), "1.0".into());
    let et = EnrichTransformer::new(meta);
    assert_eq!(et.name(), "enrich");

    let ev = make_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    let out = et.transform(ev).unwrap();
    let ext = out.ext.unwrap();
    assert_eq!(ext["env"], json!("test"));
    assert_eq!(ext["version"], json!("1.0"));
}

#[test]
fn enrich_transformer_preserves_existing_ext() {
    let mut meta = BTreeMap::new();
    meta.insert("new_key".into(), "new_val".into());
    let et = EnrichTransformer::new(meta);

    let mut existing_ext = BTreeMap::new();
    existing_ext.insert("old_key".into(), json!("old_val"));
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(existing_ext),
    };
    let out = et.transform(ev).unwrap();
    let ext = out.ext.unwrap();
    assert_eq!(ext["old_key"], json!("old_val"));
    assert_eq!(ext["new_key"], json!("new_val"));
}

#[test]
fn filter_transformer_passes_matching() {
    let ft = FilterTransformer::new(Box::new(|ev: &AgentEvent| {
        matches!(&ev.kind, AgentEventKind::Warning { .. })
    }));
    assert_eq!(ft.name(), "filter");
    let warn = make_agent_event(AgentEventKind::Warning { message: "x".into() });
    assert!(ft.transform(warn).is_some());
}

#[test]
fn filter_transformer_drops_non_matching() {
    let ft = FilterTransformer::new(Box::new(|ev: &AgentEvent| {
        matches!(&ev.kind, AgentEventKind::Warning { .. })
    }));
    let delta = make_agent_event(AgentEventKind::AssistantDelta { text: "x".into() });
    assert!(ft.transform(delta).is_none());
}

#[test]
fn timestamp_transformer_fixes_epoch() {
    let tt = TimestampTransformer::new();
    assert_eq!(tt.name(), "timestamp");
    let ev = AgentEvent {
        ts: Utc.timestamp_opt(0, 0).unwrap(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: None,
    };
    let out = tt.transform(ev).unwrap();
    assert!(out.ts.timestamp() > 0);
}

#[test]
fn timestamp_transformer_leaves_valid() {
    let tt = TimestampTransformer::new();
    let now = Utc::now();
    let ev = AgentEvent {
        ts: now,
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: None,
    };
    let out = tt.transform(ev).unwrap();
    // Should be the same (or very close)
    assert!((out.ts - now).num_milliseconds().abs() < 100);
}

#[test]
fn transformer_chain_empty_passthrough() {
    let chain = TransformerChain::new();
    let ev = make_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    assert!(chain.process(ev).is_some());
}

#[test]
fn transformer_chain_processes_in_order() {
    let chain = TransformerChain::new()
        .with(Box::new(RedactTransformer::new(vec!["SECRET".into()])))
        .with(Box::new(TimestampTransformer::new()));
    let ev = AgentEvent {
        ts: Utc.timestamp_opt(0, 0).unwrap(),
        kind: AgentEventKind::AssistantDelta { text: "my SECRET".into() },
        ext: None,
    };
    let out = chain.process(ev).unwrap();
    match &out.kind {
        AgentEventKind::AssistantDelta { text } => assert!(!text.contains("SECRET")),
        _ => panic!("wrong kind"),
    }
    assert!(out.ts.timestamp() > 0);
}

#[test]
fn transformer_chain_short_circuits_on_filter() {
    let chain = TransformerChain::new()
        .with(Box::new(ThrottleTransformer::new(0)))
        .with(Box::new(TimestampTransformer::new()));
    let ev = make_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    assert!(chain.process(ev).is_none());
}

#[test]
fn transformer_chain_process_batch() {
    let chain = TransformerChain::new()
        .with(Box::new(ThrottleTransformer::new(1)));
    let events = vec![
        make_agent_event(AgentEventKind::AssistantDelta { text: "a".into() }),
        make_agent_event(AgentEventKind::AssistantDelta { text: "b".into() }),
        make_agent_event(AgentEventKind::Warning { message: "w".into() }),
    ];
    let results = chain.process_batch(events);
    // 1 delta allowed, 1 warning allowed
    assert_eq!(results.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Typed middleware
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn typed_logging_middleware_continues() {
    let mw = sidecar_kit::typed_middleware::LoggingMiddleware::new();
    let mut ev = make_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn metrics_middleware_counts() {
    let mw = MetricsMiddleware::new();
    let mut ev1 = make_agent_event(AgentEventKind::AssistantDelta { text: "a".into() });
    let mut ev2 = make_agent_event(AgentEventKind::AssistantDelta { text: "b".into() });
    let mut ev3 = make_agent_event(AgentEventKind::Warning { message: "w".into() });
    mw.on_event(&mut ev1);
    mw.on_event(&mut ev2);
    mw.on_event(&mut ev3);
    let counts = mw.counts();
    assert_eq!(counts["assistant_delta"], 2);
    assert_eq!(counts["warning"], 1);
    assert_eq!(mw.total(), 3);
    assert!(!mw.timings().is_empty());
}

#[test]
fn metrics_middleware_default() {
    let mw = MetricsMiddleware::default();
    assert_eq!(mw.total(), 0);
    assert!(mw.timings().is_empty());
}

#[test]
fn typed_filter_middleware_drops_matching() {
    let mw = sidecar_kit::typed_middleware::FilterMiddleware::new(|ev| {
        matches!(&ev.kind, AgentEventKind::Warning { .. })
    });
    let mut warn = make_agent_event(AgentEventKind::Warning { message: "w".into() });
    assert_eq!(mw.on_event(&mut warn), MiddlewareAction::Skip);

    let mut delta = make_agent_event(AgentEventKind::AssistantDelta { text: "x".into() });
    assert_eq!(mw.on_event(&mut delta), MiddlewareAction::Continue);
}

#[test]
fn rate_limit_middleware_allows_within_limit() {
    let mw = RateLimitMiddleware::new(100);
    let mut ev = make_agent_event(AgentEventKind::AssistantDelta { text: "x".into() });
    assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn rate_limit_middleware_skips_over_limit() {
    let mw = RateLimitMiddleware::new(2);
    let mut ev1 = make_agent_event(AgentEventKind::AssistantDelta { text: "a".into() });
    let mut ev2 = make_agent_event(AgentEventKind::AssistantDelta { text: "b".into() });
    let mut ev3 = make_agent_event(AgentEventKind::AssistantDelta { text: "c".into() });
    assert_eq!(mw.on_event(&mut ev1), MiddlewareAction::Continue);
    assert_eq!(mw.on_event(&mut ev2), MiddlewareAction::Continue);
    assert_eq!(mw.on_event(&mut ev3), MiddlewareAction::Skip);
}

#[test]
fn error_recovery_middleware_passes_through_normal() {
    let inner = sidecar_kit::typed_middleware::LoggingMiddleware::new();
    let mw = sidecar_kit::typed_middleware::ErrorRecoveryMiddleware::wrap(inner);
    let mut ev = make_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn sidecar_middleware_chain_empty() {
    let chain = SidecarMiddlewareChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    let mut ev = make_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    assert_eq!(chain.process(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn sidecar_middleware_chain_default() {
    let chain = SidecarMiddlewareChain::default();
    assert!(chain.is_empty());
}

#[test]
fn sidecar_middleware_chain_builder() {
    let chain = SidecarMiddlewareChain::new()
        .with(MetricsMiddleware::new())
        .with(sidecar_kit::typed_middleware::LoggingMiddleware::new());
    assert_eq!(chain.len(), 2);
}

#[test]
fn sidecar_middleware_chain_push() {
    let mut chain = SidecarMiddlewareChain::new();
    chain.push(MetricsMiddleware::new());
    assert_eq!(chain.len(), 1);
}

#[test]
fn sidecar_middleware_chain_short_circuits_on_skip() {
    let chain = SidecarMiddlewareChain::new()
        .with(sidecar_kit::typed_middleware::FilterMiddleware::new(|_| true))
        .with(MetricsMiddleware::new());

    let mut ev = make_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    assert_eq!(chain.process(&mut ev), MiddlewareAction::Skip);
}

#[test]
fn middleware_action_eq() {
    assert_eq!(MiddlewareAction::Continue, MiddlewareAction::Continue);
    assert_eq!(MiddlewareAction::Skip, MiddlewareAction::Skip);
    assert_eq!(
        MiddlewareAction::Error("x".into()),
        MiddlewareAction::Error("x".into())
    );
    assert_ne!(MiddlewareAction::Continue, MiddlewareAction::Skip);
}

// ═══════════════════════════════════════════════════════════════════════
// HelloData
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_data_backend_as() {
    let hd = sidecar_kit::client::HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test", "version": "1.0"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let map: std::collections::HashMap<String, String> = hd.backend_as().unwrap();
    assert_eq!(map["id"], "test");
}

#[test]
fn hello_data_backend_as_wrong_type() {
    let hd = sidecar_kit::client::HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!("not an object"),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let res: Result<std::collections::HashMap<String, String>, _> = hd.backend_as();
    assert!(res.is_err());
}

#[test]
fn hello_data_capabilities_as() {
    let hd = sidecar_kit::client::HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!({}),
        capabilities: json!({"tools": true}),
        mode: Value::Null,
    };
    let map: std::collections::HashMap<String, bool> = hd.capabilities_as().unwrap();
    assert_eq!(map["tools"], true);
}
