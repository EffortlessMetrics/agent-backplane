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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive end-to-end tests for `abp-sidecar-sdk` and `sidecar-kit`.

use std::collections::BTreeMap;

use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;
use serde_json::{Value, json};
use sidecar_kit::diagnostics::{
    Diagnostic, DiagnosticCollector, DiagnosticLevel, DiagnosticSummary, SidecarDiagnostics,
};
use sidecar_kit::middleware::{
    ErrorWrapMiddleware, EventMiddleware, FilterMiddleware, LoggingMiddleware, TimingMiddleware,
};
use sidecar_kit::pipeline::PipelineStage;
use sidecar_kit::transform::{
    EnrichTransformer, EventTransformer, FilterTransformer, RedactTransformer, ThrottleTransformer,
    TimestampTransformer, TransformerChain,
};
use sidecar_kit::typed_middleware::{
    ErrorRecoveryMiddleware, MetricsMiddleware, MiddlewareAction, RateLimitMiddleware,
    SidecarMiddleware, SidecarMiddlewareChain,
};
use sidecar_kit::{
    CancelToken, EventPipeline, Frame, JsonlCodec, MiddlewareChain, PipelineError, ProcessSpec,
    ReceiptBuilder, RedactStage, SidecarError, TimestampStage, ValidateStage,
    event_command_executed, event_error, event_file_changed, event_frame, event_run_completed,
    event_run_started, event_text_delta, event_text_message, event_tool_call, event_tool_result,
    event_warning, fatal_frame, hello_frame,
};

// ── Helpers ─────────────────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_delta(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn make_message(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage {
        text: text.to_string(),
    })
}

fn make_warning_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Warning {
        message: msg.to_string(),
    })
}

fn make_error_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: None,
    })
}

// ════════════════════════════════════════════════════════════════════
// 1. Frame serde roundtrip
// ════════════════════════════════════════════════════════════════════

#[test]
fn frame_hello_roundtrip() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    assert!(matches!(decoded, Frame::Hello { .. }));
}

#[test]
fn frame_run_roundtrip() {
    let frame = Frame::Run {
        id: "run-1".into(),
        work_order: json!({"task": "hello"}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Run { id, work_order } => {
            assert_eq!(id, "run-1");
            assert_eq!(work_order["task"], "hello");
        }
        _ => panic!("expected Run frame"),
    }
}

#[test]
fn frame_event_roundtrip() {
    let frame = Frame::Event {
        ref_id: "run-1".into(),
        event: json!({"type": "assistant_delta", "text": "hi"}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(event["text"], "hi");
        }
        _ => panic!("expected Event frame"),
    }
}

#[test]
fn frame_final_roundtrip() {
    let frame = Frame::Final {
        ref_id: "run-1".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(receipt["outcome"], "complete");
        }
        _ => panic!("expected Final frame"),
    }
}

#[test]
fn frame_fatal_roundtrip() {
    let frame = Frame::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "boom");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn frame_fatal_no_ref_id_roundtrip() {
    let frame = Frame::Fatal {
        ref_id: None,
        error: "startup failure".into(),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "startup failure");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn frame_cancel_roundtrip() {
    let frame = Frame::Cancel {
        ref_id: "run-1".into(),
        reason: Some("timeout".into()),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Cancel { ref_id, reason } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(reason.as_deref(), Some("timeout"));
        }
        _ => panic!("expected Cancel frame"),
    }
}

#[test]
fn frame_cancel_no_reason_roundtrip() {
    let frame = Frame::Cancel {
        ref_id: "run-2".into(),
        reason: None,
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Cancel { ref_id, reason } => {
            assert_eq!(ref_id, "run-2");
            assert!(reason.is_none());
        }
        _ => panic!("expected Cancel frame"),
    }
}

#[test]
fn frame_ping_roundtrip() {
    let frame = Frame::Ping { seq: 42 };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Ping { seq } => assert_eq!(seq, 42),
        _ => panic!("expected Ping frame"),
    }
}

#[test]
fn frame_pong_roundtrip() {
    let frame = Frame::Pong { seq: 99 };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Pong { seq } => assert_eq!(seq, 99),
        _ => panic!("expected Pong frame"),
    }
}

// ════════════════════════════════════════════════════════════════════
// 2. Codec specifics
// ════════════════════════════════════════════════════════════════════

#[test]
fn codec_encode_ends_with_newline() {
    let frame = Frame::Ping { seq: 1 };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn codec_decode_invalid_json() {
    let result = JsonlCodec::decode("not json");
    assert!(result.is_err());
}

#[test]
fn codec_decode_empty_object() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn codec_decode_unknown_tag() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_tag"}"#);
    assert!(result.is_err());
}

#[test]
fn codec_decode_missing_required_field() {
    // Hello without contract_version
    let result = JsonlCodec::decode(r#"{"t":"hello","backend":{},"capabilities":{}}"#);
    assert!(result.is_err());
}

#[test]
fn codec_roundtrip_preserves_nested_json() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"nested": {"deep": [1, 2, 3]}}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Frame::Event { event, .. } = decoded {
        assert_eq!(event["nested"]["deep"], json!([1, 2, 3]));
    } else {
        panic!("expected Event");
    }
}

// ════════════════════════════════════════════════════════════════════
// 3. Frame::try_event / try_final
// ════════════════════════════════════════════════════════════════════

#[test]
fn try_event_on_event_frame() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"key": "value"}),
    };
    let (ref_id, val): (String, serde_json::Map<String, Value>) = frame.try_event().unwrap();
    assert_eq!(ref_id, "r1");
    assert_eq!(val["key"], "value");
}

#[test]
fn try_event_on_non_event_frame() {
    let frame = Frame::Ping { seq: 1 };
    let result: Result<(String, Value), _> = frame.try_event();
    assert!(result.is_err());
}

#[test]
fn try_final_on_final_frame() {
    let frame = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({"outcome": "complete"}),
    };
    let (ref_id, val): (String, serde_json::Map<String, Value>) = frame.try_final().unwrap();
    assert_eq!(ref_id, "r1");
    assert_eq!(val["outcome"], "complete");
}

#[test]
fn try_final_on_non_final_frame() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    };
    let result: Result<(String, Value), _> = frame.try_final();
    assert!(result.is_err());
}

#[test]
fn try_event_type_mismatch() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!("a plain string"),
    };
    // Trying to extract as a Map should fail
    let result: Result<(String, serde_json::Map<String, Value>), _> = frame.try_event();
    assert!(result.is_err());
}

// ════════════════════════════════════════════════════════════════════
// 4. Builder helpers (event values)
// ════════════════════════════════════════════════════════════════════

#[test]
fn event_text_delta_has_correct_type() {
    let v = event_text_delta("hello");
    assert_eq!(v["type"], "assistant_delta");
    assert_eq!(v["text"], "hello");
    assert!(v["ts"].is_string());
}

#[test]
fn event_text_message_has_correct_type() {
    let v = event_text_message("world");
    assert_eq!(v["type"], "assistant_message");
    assert_eq!(v["text"], "world");
}

#[test]
fn event_tool_call_fields() {
    let v = event_tool_call("read_file", Some("tc-1"), json!({"path": "/a"}));
    assert_eq!(v["type"], "tool_call");
    assert_eq!(v["tool_name"], "read_file");
    assert_eq!(v["tool_use_id"], "tc-1");
    assert_eq!(v["input"]["path"], "/a");
}

#[test]
fn event_tool_call_no_use_id() {
    let v = event_tool_call("write_file", None, json!({}));
    assert!(v["tool_use_id"].is_null());
}

#[test]
fn event_tool_result_fields() {
    let v = event_tool_result("read_file", Some("tc-1"), json!("contents"), false);
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["tool_name"], "read_file");
    assert_eq!(v["is_error"], false);
}

#[test]
fn event_tool_result_error_flag() {
    let v = event_tool_result("cmd", None, json!("err"), true);
    assert_eq!(v["is_error"], true);
}

#[test]
fn event_error_fields() {
    let v = event_error("something broke");
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "something broke");
}

#[test]
fn event_warning_fields() {
    let v = event_warning("watch out");
    assert_eq!(v["type"], "warning");
    assert_eq!(v["message"], "watch out");
}

#[test]
fn event_run_started_fields() {
    let v = event_run_started("beginning");
    assert_eq!(v["type"], "run_started");
    assert_eq!(v["message"], "beginning");
}

#[test]
fn event_run_completed_fields() {
    let v = event_run_completed("done");
    assert_eq!(v["type"], "run_completed");
    assert_eq!(v["message"], "done");
}

#[test]
fn event_file_changed_fields() {
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
fn event_command_executed_nulls() {
    let v = event_command_executed("ls", None, None);
    assert!(v["exit_code"].is_null());
    assert!(v["output_preview"].is_null());
}

// ════════════════════════════════════════════════════════════════════
// 5. Frame helpers
// ════════════════════════════════════════════════════════════════════

#[test]
fn event_frame_wraps_value() {
    let val = event_text_delta("hi");
    let frame = event_frame("run-1", val.clone());
    match frame {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(event["type"], "assistant_delta");
        }
        _ => panic!("expected Event frame"),
    }
}

#[test]
fn fatal_frame_with_ref_id() {
    let frame = fatal_frame(Some("run-1"), "crash");
    match frame {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "crash");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn fatal_frame_without_ref_id() {
    let frame = fatal_frame(None, "init error");
    match frame {
        Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "init error");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn hello_frame_defaults() {
    let frame = hello_frame("my-backend");
    match frame {
        Frame::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "my-backend");
            assert_eq!(capabilities, json!({}));
            assert!(mode.is_null());
        }
        _ => panic!("expected Hello frame"),
    }
}

// ════════════════════════════════════════════════════════════════════
// 6. ReceiptBuilder
// ════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_default_outcome() {
    let receipt = ReceiptBuilder::new("run-1", "backend-a").build();
    assert_eq!(receipt["outcome"], "complete");
    assert_eq!(receipt["meta"]["run_id"], "run-1");
    assert_eq!(receipt["backend"]["id"], "backend-a");
    assert_eq!(receipt["meta"]["contract_version"], "abp/v0.1");
}

#[test]
fn receipt_builder_failed_outcome() {
    let receipt = ReceiptBuilder::new("run-1", "b").failed().build();
    assert_eq!(receipt["outcome"], "failed");
}

#[test]
fn receipt_builder_partial_outcome() {
    let receipt = ReceiptBuilder::new("run-1", "b").partial().build();
    assert_eq!(receipt["outcome"], "partial");
}

#[test]
fn receipt_builder_with_events() {
    let receipt = ReceiptBuilder::new("r1", "b")
        .event(event_text_delta("tok1"))
        .event(event_text_delta("tok2"))
        .build();
    let trace = receipt["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 2);
}

#[test]
fn receipt_builder_with_artifacts() {
    let receipt = ReceiptBuilder::new("r1", "b")
        .artifact("file", "src/main.rs")
        .artifact("diff", "patch.diff")
        .build();
    let artifacts = receipt["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0]["kind"], "file");
    assert_eq!(artifacts[1]["path"], "patch.diff");
}

#[test]
fn receipt_builder_usage_tokens() {
    let receipt = ReceiptBuilder::new("r1", "b")
        .input_tokens(100)
        .output_tokens(50)
        .build();
    assert_eq!(receipt["usage"]["input_tokens"], 100);
    assert_eq!(receipt["usage"]["output_tokens"], 50);
}

#[test]
fn receipt_builder_usage_raw() {
    let raw = json!({"prompt_tokens": 200, "completion_tokens": 80});
    let receipt = ReceiptBuilder::new("r1", "b")
        .usage_raw(raw.clone())
        .build();
    assert_eq!(receipt["usage_raw"], raw);
}

#[test]
fn receipt_builder_has_null_receipt_sha256() {
    let receipt = ReceiptBuilder::new("r1", "b").build();
    assert!(receipt["receipt_sha256"].is_null());
}

#[test]
fn receipt_builder_meta_has_timestamps() {
    let receipt = ReceiptBuilder::new("r1", "b").build();
    assert!(receipt["meta"]["started_at"].is_string());
    assert!(receipt["meta"]["finished_at"].is_string());
}

#[test]
fn receipt_builder_chain_all_methods() {
    let receipt = ReceiptBuilder::new("r1", "b")
        .failed()
        .event(event_error("oops"))
        .artifact("log", "run.log")
        .usage_raw(json!({}))
        .input_tokens(10)
        .output_tokens(5)
        .build();
    assert_eq!(receipt["outcome"], "failed");
    assert_eq!(receipt["trace"].as_array().unwrap().len(), 1);
    assert_eq!(receipt["artifacts"].as_array().unwrap().len(), 1);
}

// ════════════════════════════════════════════════════════════════════
// 7. Middleware chain (value-based)
// ════════════════════════════════════════════════════════════════════

#[test]
fn middleware_chain_empty_passthrough() {
    let chain = MiddlewareChain::new();
    assert!(chain.is_empty());
    let ev = json!({"type": "assistant_delta", "text": "hi"});
    let result = chain.process(&ev);
    assert_eq!(result, Some(ev));
}

#[test]
fn middleware_chain_len() {
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware::new())
        .with(TimingMiddleware::new());
    assert_eq!(chain.len(), 2);
}

#[test]
fn logging_middleware_passthrough() {
    let m = LoggingMiddleware::new();
    let ev = json!({"type": "test"});
    assert_eq!(m.process(&ev), Some(ev));
}

#[test]
fn timing_middleware_adds_field() {
    let m = TimingMiddleware::new();
    let ev = json!({"type": "test"});
    let result = m.process(&ev).unwrap();
    assert!(result.get("_processing_us").is_some());
}

#[test]
fn timing_middleware_non_object_passthrough() {
    let m = TimingMiddleware::new();
    let ev = json!("just a string");
    let result = m.process(&ev).unwrap();
    // Non-objects don't get the field
    assert!(result.get("_processing_us").is_none());
}

#[test]
fn filter_middleware_include_passes_matching() {
    let f = FilterMiddleware::include_kinds(&["assistant_delta"]);
    let ev = json!({"type": "assistant_delta", "text": "x"});
    assert!(f.process(&ev).is_some());
}

#[test]
fn filter_middleware_include_drops_non_matching() {
    let f = FilterMiddleware::include_kinds(&["assistant_delta"]);
    let ev = json!({"type": "error", "message": "x"});
    assert!(f.process(&ev).is_none());
}

#[test]
fn filter_middleware_exclude_drops_matching() {
    let f = FilterMiddleware::exclude_kinds(&["warning"]);
    let ev = json!({"type": "warning", "message": "x"});
    assert!(f.process(&ev).is_none());
}

#[test]
fn filter_middleware_exclude_passes_non_matching() {
    let f = FilterMiddleware::exclude_kinds(&["warning"]);
    let ev = json!({"type": "assistant_delta", "text": "x"});
    assert!(f.process(&ev).is_some());
}

#[test]
fn filter_middleware_case_insensitive() {
    let f = FilterMiddleware::include_kinds(&["Assistant_Delta"]);
    let ev = json!({"type": "assistant_delta", "text": "x"});
    assert!(f.process(&ev).is_some());
}

#[test]
fn filter_middleware_empty_include_drops_all() {
    let f = FilterMiddleware::include_kinds(&[]);
    let ev = json!({"type": "anything"});
    assert!(f.process(&ev).is_none());
}

#[test]
fn filter_middleware_empty_exclude_passes_all() {
    let f = FilterMiddleware::exclude_kinds(&[]);
    let ev = json!({"type": "anything"});
    assert!(f.process(&ev).is_some());
}

#[test]
fn error_wrap_middleware_passes_objects() {
    let m = ErrorWrapMiddleware::new();
    let ev = json!({"type": "test"});
    let result = m.process(&ev).unwrap();
    assert_eq!(result["type"], "test");
}

#[test]
fn error_wrap_middleware_wraps_non_objects() {
    let m = ErrorWrapMiddleware::new();
    let ev = json!(42);
    let result = m.process(&ev).unwrap();
    assert_eq!(result["type"], "error");
    assert!(result["message"].as_str().unwrap().contains("non-object"));
    assert_eq!(result["_original"], 42);
}

#[test]
fn middleware_chain_short_circuits_on_none() {
    let chain = MiddlewareChain::new()
        .with(FilterMiddleware::include_kinds(&["assistant_delta"]))
        .with(TimingMiddleware::new());
    let ev = json!({"type": "error", "message": "x"});
    assert!(chain.process(&ev).is_none());
}

#[test]
fn middleware_chain_with_builder() {
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware::new())
        .with(ErrorWrapMiddleware::new());
    assert_eq!(chain.len(), 2);
    assert!(!chain.is_empty());
}

// ════════════════════════════════════════════════════════════════════
// 8. Pipeline stages
// ════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_empty_passthrough() {
    let p = EventPipeline::new();
    assert_eq!(p.stage_count(), 0);
    let ev = json!({"type": "test"});
    let result = p.process(ev.clone()).unwrap();
    assert_eq!(result, Some(ev));
}

#[test]
fn timestamp_stage_adds_processed_at() {
    let s = TimestampStage::new();
    assert_eq!(s.name(), "timestamp");
    let result = s.process(json!({"type": "test"})).unwrap().unwrap();
    assert!(result.get("processed_at").is_some());
}

#[test]
fn timestamp_stage_rejects_non_object() {
    let s = TimestampStage::new();
    let result = s.process(json!("string"));
    assert!(result.is_err());
}

#[test]
fn redact_stage_removes_fields() {
    let s = RedactStage::new(vec!["secret".into(), "token".into()]);
    assert_eq!(s.name(), "redact");
    let ev = json!({"type": "test", "secret": "abc", "token": "xyz", "keep": true});
    let result = s.process(ev).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert!(result.get("token").is_none());
    assert_eq!(result["keep"], true);
}

#[test]
fn redact_stage_rejects_non_object() {
    let s = RedactStage::new(vec!["x".into()]);
    assert!(s.process(json!(123)).is_err());
}

#[test]
fn validate_stage_passes_with_all_fields() {
    let s = ValidateStage::new(vec!["type".into(), "ts".into()]);
    assert_eq!(s.name(), "validate");
    let ev = json!({"type": "test", "ts": "2025-01-01"});
    assert!(s.process(ev).unwrap().is_some());
}

#[test]
fn validate_stage_fails_on_missing_field() {
    let s = ValidateStage::new(vec!["type".into(), "ts".into()]);
    let ev = json!({"type": "test"});
    let err = s.process(ev).unwrap_err();
    match err {
        PipelineError::StageError { stage, message } => {
            assert_eq!(stage, "validate");
            assert!(message.contains("ts"));
        }
        _ => panic!("expected StageError"),
    }
}

#[test]
fn validate_stage_rejects_non_object() {
    let s = ValidateStage::new(vec!["type".into()]);
    let err = s.process(json!(null)).unwrap_err();
    assert!(matches!(err, PipelineError::InvalidEvent));
}

#[test]
fn pipeline_multi_stage() {
    let mut p = EventPipeline::new();
    p.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
    p.add_stage(Box::new(TimestampStage::new()));
    p.add_stage(Box::new(RedactStage::new(vec!["secret".into()])));
    assert_eq!(p.stage_count(), 3);

    let ev = json!({"type": "test", "secret": "key"});
    let result = p.process(ev).unwrap().unwrap();
    assert!(result.get("processed_at").is_some());
    assert!(result.get("secret").is_none());
}

#[test]
fn pipeline_error_display() {
    let err = PipelineError::StageError {
        stage: "validate".into(),
        message: "missing field".into(),
    };
    let s = format!("{err}");
    assert!(s.contains("validate"));
    assert!(s.contains("missing field"));

    let err2 = PipelineError::InvalidEvent;
    let s2 = format!("{err2}");
    assert!(s2.contains("not a valid JSON object"));
}

// ════════════════════════════════════════════════════════════════════
// 9. Typed transformers (AgentEvent-based)
// ════════════════════════════════════════════════════════════════════

#[test]
fn redact_transformer_redacts_text() {
    let t = RedactTransformer::new(vec!["secret123".into()]);
    assert_eq!(t.name(), "redact");
    let ev = make_message("my key is secret123 ok?");
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(!text.contains("secret123"));
            assert!(text.contains("[REDACTED]"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_redacts_delta() {
    let t = RedactTransformer::new(vec!["API_KEY".into()]);
    let ev = make_delta("token API_KEY here");
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => {
            assert!(!text.contains("API_KEY"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_redacts_warning() {
    let t = RedactTransformer::new(vec!["password".into()]);
    let ev = make_warning_event("password leaked");
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::Warning { message } => {
            assert!(!message.contains("password"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_redacts_error() {
    let t = RedactTransformer::new(vec!["token".into()]);
    let ev = make_error_event("invalid token");
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::Error { message, .. } => {
            assert!(!message.contains("token"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_multiple_patterns() {
    let t = RedactTransformer::new(vec!["aaa".into(), "bbb".into()]);
    let ev = make_message("aaa and bbb are secrets");
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(!text.contains("aaa"));
            assert!(!text.contains("bbb"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn throttle_transformer_allows_up_to_max() {
    let t = ThrottleTransformer::new(2);
    assert_eq!(t.name(), "throttle");
    assert!(t.transform(make_delta("a")).is_some());
    assert!(t.transform(make_delta("b")).is_some());
    assert!(t.transform(make_delta("c")).is_none());
}

#[test]
fn throttle_transformer_tracks_per_kind() {
    let t = ThrottleTransformer::new(1);
    // One delta allowed
    assert!(t.transform(make_delta("a")).is_some());
    assert!(t.transform(make_delta("b")).is_none());
    // One warning still allowed
    assert!(t.transform(make_warning_event("w")).is_some());
    assert!(t.transform(make_warning_event("w2")).is_none());
}

#[test]
fn enrich_transformer_adds_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("env".into(), "test".into());
    meta.insert("version".into(), "1.0".into());
    let t = EnrichTransformer::new(meta);
    assert_eq!(t.name(), "enrich");

    let ev = make_delta("hi");
    let result = t.transform(ev).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext["env"], json!("test"));
    assert_eq!(ext["version"], json!("1.0"));
}

#[test]
fn enrich_transformer_merges_with_existing_ext() {
    let mut meta = BTreeMap::new();
    meta.insert("new_key".into(), "new_val".into());
    let t = EnrichTransformer::new(meta);

    let mut existing_ext = BTreeMap::new();
    existing_ext.insert("old_key".into(), json!("old_val"));
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(existing_ext),
    };
    let result = t.transform(ev).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext["old_key"], json!("old_val"));
    assert_eq!(ext["new_key"], json!("new_val"));
}

#[test]
fn filter_transformer_passes_matching() {
    let t = FilterTransformer::new(Box::new(|e: &AgentEvent| {
        matches!(&e.kind, AgentEventKind::AssistantDelta { .. })
    }));
    assert_eq!(t.name(), "filter");
    assert!(t.transform(make_delta("a")).is_some());
}

#[test]
fn filter_transformer_drops_non_matching() {
    let t = FilterTransformer::new(Box::new(|e: &AgentEvent| {
        matches!(&e.kind, AgentEventKind::AssistantDelta { .. })
    }));
    assert!(t.transform(make_warning_event("w")).is_none());
}

#[test]
fn timestamp_transformer_fixes_epoch_zero() {
    let t = TimestampTransformer::new();
    assert_eq!(t.name(), "timestamp");
    let mut ev = make_delta("hi");
    ev.ts = chrono::DateTime::UNIX_EPOCH;
    let result = t.transform(ev).unwrap();
    assert!(result.ts.timestamp() > 0);
}

#[test]
fn timestamp_transformer_preserves_valid_ts() {
    let t = TimestampTransformer::new();
    let ev = make_delta("hi");
    let original_ts = ev.ts;
    let result = t.transform(ev).unwrap();
    assert_eq!(result.ts, original_ts);
}

#[test]
fn transformer_chain_empty_passthrough() {
    let chain = TransformerChain::new();
    let ev = make_delta("test");
    assert!(chain.process(ev).is_some());
}

#[test]
fn transformer_chain_processes_in_order() {
    let chain = TransformerChain::new()
        .with(Box::new(RedactTransformer::new(vec!["SECRET".into()])))
        .with(Box::new(TimestampTransformer::new()));

    let ev = make_message("my SECRET data");
    let result = chain.process(ev).unwrap();
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(!text.contains("SECRET"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn transformer_chain_short_circuits() {
    let chain = TransformerChain::new()
        .with(Box::new(ThrottleTransformer::new(1)))
        .with(Box::new(TimestampTransformer::new()));

    assert!(chain.process(make_delta("a")).is_some());
    assert!(chain.process(make_delta("b")).is_none());
}

#[test]
fn transformer_chain_process_batch() {
    let chain = TransformerChain::new().with(Box::new(ThrottleTransformer::new(2)));

    let events = vec![make_delta("a"), make_delta("b"), make_delta("c")];
    let result = chain.process_batch(events);
    assert_eq!(result.len(), 2);
}

// ════════════════════════════════════════════════════════════════════
// 10. Typed middleware (SidecarMiddleware)
// ════════════════════════════════════════════════════════════════════

#[test]
fn typed_logging_middleware_continues() {
    let m = sidecar_kit::typed_middleware::LoggingMiddleware::new();
    let mut ev = make_delta("hi");
    assert_eq!(m.on_event(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn metrics_middleware_counts_events() {
    let m = MetricsMiddleware::new();
    let mut ev1 = make_delta("a");
    let mut ev2 = make_delta("b");
    let mut ev3 = make_warning_event("w");

    m.on_event(&mut ev1);
    m.on_event(&mut ev2);
    m.on_event(&mut ev3);

    assert_eq!(m.total(), 3);
    let counts = m.counts();
    assert_eq!(*counts.get("assistant_delta").unwrap(), 2);
    assert_eq!(*counts.get("warning").unwrap(), 1);
}

#[test]
fn metrics_middleware_records_timings() {
    let m = MetricsMiddleware::new();
    let mut ev = make_delta("x");
    m.on_event(&mut ev);
    assert_eq!(m.timings().len(), 1);
}

#[test]
fn typed_filter_middleware_skips_matching() {
    let m = sidecar_kit::typed_middleware::FilterMiddleware::new(|e: &AgentEvent| {
        matches!(&e.kind, AgentEventKind::Warning { .. })
    });
    let mut ev = make_warning_event("w");
    assert_eq!(m.on_event(&mut ev), MiddlewareAction::Skip);
}

#[test]
fn typed_filter_middleware_continues_non_matching() {
    let m = sidecar_kit::typed_middleware::FilterMiddleware::new(|e: &AgentEvent| {
        matches!(&e.kind, AgentEventKind::Warning { .. })
    });
    let mut ev = make_delta("d");
    assert_eq!(m.on_event(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn rate_limit_middleware_allows_under_limit() {
    let m = RateLimitMiddleware::new(100);
    let mut ev = make_delta("x");
    assert_eq!(m.on_event(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn sidecar_middleware_chain_empty() {
    let chain = SidecarMiddlewareChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    let mut ev = make_delta("x");
    assert_eq!(chain.process(&mut ev), MiddlewareAction::Continue);
}

#[test]
fn sidecar_middleware_chain_with_builder() {
    let chain = SidecarMiddlewareChain::new()
        .with(sidecar_kit::typed_middleware::LoggingMiddleware::new())
        .with(MetricsMiddleware::new());
    assert_eq!(chain.len(), 2);
}

#[test]
fn sidecar_middleware_chain_short_circuits_on_skip() {
    let chain = SidecarMiddlewareChain::new()
        .with(sidecar_kit::typed_middleware::FilterMiddleware::new(
            |_: &AgentEvent| true,
        ))
        .with(MetricsMiddleware::new());

    let mut ev = make_delta("x");
    assert_eq!(chain.process(&mut ev), MiddlewareAction::Skip);
}

#[test]
fn error_recovery_middleware_catches_panic() {
    struct PanicMiddleware;
    impl SidecarMiddleware for PanicMiddleware {
        fn on_event(&self, _event: &mut AgentEvent) -> MiddlewareAction {
            panic!("intentional test panic");
        }
    }

    let m = ErrorRecoveryMiddleware::wrap(PanicMiddleware);
    let mut ev = make_delta("x");
    let result = m.on_event(&mut ev);
    match result {
        MiddlewareAction::Error(msg) => {
            assert!(msg.contains("intentional test panic"));
        }
        _ => panic!("expected Error action"),
    }
}

#[test]
fn error_recovery_middleware_passes_through_normal() {
    let m = ErrorRecoveryMiddleware::wrap(sidecar_kit::typed_middleware::LoggingMiddleware::new());
    let mut ev = make_delta("x");
    assert_eq!(m.on_event(&mut ev), MiddlewareAction::Continue);
}

// ════════════════════════════════════════════════════════════════════
// 11. CancelToken
// ════════════════════════════════════════════════════════════════════

#[test]
fn cancel_token_default_not_cancelled() {
    let t = CancelToken::new();
    assert!(!t.is_cancelled());
}

#[test]
fn cancel_token_cancel_signals() {
    let t = CancelToken::new();
    t.cancel();
    assert!(t.is_cancelled());
}

#[test]
fn cancel_token_clone_shares_state() {
    let t1 = CancelToken::new();
    let t2 = t1.clone();
    t1.cancel();
    assert!(t2.is_cancelled());
}

#[test]
fn cancel_token_default_impl() {
    let t = CancelToken::default();
    assert!(!t.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancelled_future_returns_immediately_when_cancelled() {
    let t = CancelToken::new();
    t.cancel();
    // Should return immediately since already cancelled
    t.cancelled().await;
    assert!(t.is_cancelled());
}

// ════════════════════════════════════════════════════════════════════
// 12. ProcessSpec
// ════════════════════════════════════════════════════════════════════

#[test]
fn process_spec_new_defaults() {
    let spec = ProcessSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn process_spec_with_fields() {
    let mut spec = ProcessSpec::new("python3");
    spec.args = vec!["script.py".into()];
    spec.env.insert("KEY".into(), "VAL".into());
    spec.cwd = Some("/tmp".into());

    assert_eq!(spec.command, "python3");
    assert_eq!(spec.args, vec!["script.py"]);
    assert_eq!(spec.env["KEY"], "VAL");
    assert_eq!(spec.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn process_spec_from_string() {
    let spec = ProcessSpec::new(String::from("bash"));
    assert_eq!(spec.command, "bash");
}

// ════════════════════════════════════════════════════════════════════
// 13. Diagnostics
// ════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_level_ordering() {
    assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
    assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
    assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
}

#[test]
fn diagnostic_level_serde_roundtrip() {
    let level = DiagnosticLevel::Warning;
    let json = serde_json::to_string(&level).unwrap();
    let back: DiagnosticLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, level);
}

#[test]
fn diagnostic_serde_roundtrip() {
    let d = Diagnostic {
        level: DiagnosticLevel::Error,
        code: "SK001".into(),
        message: "test error".into(),
        source: Some("sidecar".into()),
        timestamp: "2025-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: Diagnostic = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, "SK001");
    assert_eq!(back.level, DiagnosticLevel::Error);
}

#[test]
fn diagnostic_collector_empty() {
    let c = DiagnosticCollector::new();
    assert!(!c.has_errors());
    assert_eq!(c.error_count(), 0);
    assert!(c.diagnostics().is_empty());
}

#[test]
fn diagnostic_collector_add_info() {
    let mut c = DiagnosticCollector::new();
    c.add_info("SK100", "info message");
    assert_eq!(c.diagnostics().len(), 1);
    assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Info);
}

#[test]
fn diagnostic_collector_add_warning() {
    let mut c = DiagnosticCollector::new();
    c.add_warning("SK200", "warning message");
    assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Warning);
}

#[test]
fn diagnostic_collector_add_error() {
    let mut c = DiagnosticCollector::new();
    c.add_error("SK300", "error message");
    assert!(c.has_errors());
    assert_eq!(c.error_count(), 1);
}

#[test]
fn diagnostic_collector_by_level() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "info");
    c.add_warning("W1", "warn");
    c.add_error("E1", "err");
    c.add_error("E2", "err2");

    assert_eq!(c.by_level(DiagnosticLevel::Info).len(), 1);
    assert_eq!(c.by_level(DiagnosticLevel::Warning).len(), 1);
    assert_eq!(c.by_level(DiagnosticLevel::Error).len(), 2);
    assert_eq!(c.by_level(DiagnosticLevel::Debug).len(), 0);
}

#[test]
fn diagnostic_collector_clear() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "info");
    c.add_error("E1", "err");
    c.clear();
    assert!(c.diagnostics().is_empty());
    assert!(!c.has_errors());
}

#[test]
fn diagnostic_collector_summary() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "i");
    c.add_info("I2", "i");
    c.add_warning("W1", "w");
    c.add_error("E1", "e");

    let s = c.summary();
    assert_eq!(s.info_count, 2);
    assert_eq!(s.warning_count, 1);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.debug_count, 0);
    assert_eq!(s.total, 4);
}

#[test]
fn diagnostic_summary_default() {
    let s = DiagnosticSummary::default();
    assert_eq!(s.total, 0);
    assert_eq!(s.debug_count, 0);
    assert_eq!(s.info_count, 0);
    assert_eq!(s.warning_count, 0);
    assert_eq!(s.error_count, 0);
}

#[test]
fn diagnostic_summary_serde_roundtrip() {
    let s = DiagnosticSummary {
        debug_count: 1,
        info_count: 2,
        warning_count: 3,
        error_count: 4,
        total: 10,
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: DiagnosticSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn sidecar_diagnostics_serde_roundtrip() {
    let sd = SidecarDiagnostics {
        run_id: "r1".into(),
        diagnostics: vec![Diagnostic {
            level: DiagnosticLevel::Info,
            code: "SK100".into(),
            message: "test".into(),
            source: None,
            timestamp: "2025-01-01T00:00:00Z".into(),
        }],
        pipeline_stages: vec!["validate".into(), "redact".into()],
        transform_count: 3,
    };
    let json = serde_json::to_string(&sd).unwrap();
    let back: SidecarDiagnostics = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_id, "r1");
    assert_eq!(back.diagnostics.len(), 1);
    assert_eq!(back.pipeline_stages.len(), 2);
    assert_eq!(back.transform_count, 3);
}

// ════════════════════════════════════════════════════════════════════
// 14. SidecarError
// ════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_error_protocol_display() {
    let e = SidecarError::Protocol("bad handshake".into());
    let msg = format!("{e}");
    assert!(msg.contains("protocol violation"));
    assert!(msg.contains("bad handshake"));
}

#[test]
fn sidecar_error_fatal_display() {
    let e = SidecarError::Fatal("crash".into());
    let msg = format!("{e}");
    assert!(msg.contains("fatal"));
    assert!(msg.contains("crash"));
}

#[test]
fn sidecar_error_timeout_display() {
    let e = SidecarError::Timeout;
    let msg = format!("{e}");
    assert!(msg.contains("timed out"));
}

#[test]
fn sidecar_error_exited_display() {
    let e = SidecarError::Exited(Some(1));
    let msg = format!("{e}");
    assert!(msg.contains("exited unexpectedly"));
}

#[test]
fn sidecar_error_exited_no_code() {
    let e = SidecarError::Exited(None);
    let msg = format!("{e}");
    assert!(msg.contains("None"));
}

// ════════════════════════════════════════════════════════════════════
// 15. abp-sidecar-sdk: sidecar_script
// ════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_script_joins_paths() {
    use std::path::Path;
    let result = abp_sidecar_sdk::sidecar_script(Path::new("/root"), "hosts/node/index.js");
    assert!(result.ends_with("index.js"));
    assert!(result.starts_with("/root"));
}

#[test]
fn sidecar_script_relative_path() {
    use std::path::Path;
    let result = abp_sidecar_sdk::sidecar_script(Path::new("hosts"), "node/index.js");
    let s = result.to_string_lossy();
    assert!(s.contains("node"));
    assert!(s.contains("index.js"));
}

// ════════════════════════════════════════════════════════════════════
// 16. Protocol compliance edge cases
// ════════════════════════════════════════════════════════════════════

#[test]
fn frame_hello_with_mode_roundtrip() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test", "version": "1.0"}),
        capabilities: json!({"tools": true, "streaming": true}),
        mode: json!("passthrough"),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Hello {
            mode, capabilities, ..
        } => {
            assert_eq!(mode, "passthrough");
            assert_eq!(capabilities["tools"], true);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn frame_discriminator_is_t_not_type() {
    let frame = Frame::Ping { seq: 1 };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""t":"ping"#));
    assert!(!json.contains(r#""type":"ping"#));
}

#[test]
fn frame_tag_uses_snake_case() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""t":"hello"#));

    let event_frame = Frame::Event {
        ref_id: "r".into(),
        event: json!({}),
    };
    let json2 = serde_json::to_string(&event_frame).unwrap();
    assert!(json2.contains(r#""t":"event"#));
}

#[test]
fn frame_final_tag() {
    let frame = Frame::Final {
        ref_id: "r".into(),
        receipt: json!({}),
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""t":"final"#));
}

#[test]
fn decode_frame_from_raw_json() {
    let raw =
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test"},"capabilities":{}}"#;
    let frame = JsonlCodec::decode(raw).unwrap();
    assert!(matches!(frame, Frame::Hello { .. }));
}

#[test]
fn decode_event_frame_from_raw_json() {
    let raw = r#"{"t":"event","ref_id":"run-1","event":{"type":"assistant_delta","text":"hi"}}"#;
    let frame = JsonlCodec::decode(raw).unwrap();
    assert!(matches!(frame, Frame::Event { .. }));
}

#[test]
fn decode_fatal_frame_from_raw_json() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":"out of memory"}"#;
    let frame = JsonlCodec::decode(raw).unwrap();
    match frame {
        Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn codec_handles_unicode() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"text": "こんにちは 🎉"}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Frame::Event { event, .. } = decoded {
        assert_eq!(event["text"], "こんにちは 🎉");
    }
}

#[test]
fn codec_handles_empty_strings() {
    let frame = Frame::Event {
        ref_id: "".into(),
        event: json!({"text": ""}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Frame::Event { ref_id, event } = decoded {
        assert_eq!(ref_id, "");
        assert_eq!(event["text"], "");
    }
}

#[test]
fn codec_handles_large_payload() {
    let big_text = "x".repeat(100_000);
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"text": big_text}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Frame::Event { event, .. } = decoded {
        assert_eq!(event["text"].as_str().unwrap().len(), 100_000);
    }
}

// ════════════════════════════════════════════════════════════════════
// 17. Middleware action equality
// ════════════════════════════════════════════════════════════════════

#[test]
fn middleware_action_eq() {
    assert_eq!(MiddlewareAction::Continue, MiddlewareAction::Continue);
    assert_eq!(MiddlewareAction::Skip, MiddlewareAction::Skip);
    assert_eq!(
        MiddlewareAction::Error("x".into()),
        MiddlewareAction::Error("x".into())
    );
    assert_ne!(MiddlewareAction::Continue, MiddlewareAction::Skip);
    assert_ne!(
        MiddlewareAction::Error("a".into()),
        MiddlewareAction::Error("b".into())
    );
}

#[test]
fn middleware_action_debug() {
    let a = MiddlewareAction::Continue;
    let s = format!("{a:?}");
    assert!(s.contains("Continue"));
}

// ════════════════════════════════════════════════════════════════════
// 18. HelloData
// ════════════════════════════════════════════════════════════════════

#[test]
fn hello_data_backend_as_typed() {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Backend {
        id: String,
    }
    let data = sidecar_kit::HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test-backend"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let b: Backend = data.backend_as().unwrap();
    assert_eq!(b.id, "test-backend");
}

#[test]
fn hello_data_capabilities_as_typed() {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Caps {
        tools: bool,
    }
    let data = sidecar_kit::HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!({}),
        capabilities: json!({"tools": true}),
        mode: Value::Null,
    };
    let c: Caps = data.capabilities_as().unwrap();
    assert!(c.tools);
}

#[test]
fn hello_data_backend_as_wrong_type_fails() {
    let data = sidecar_kit::HelloData {
        contract_version: "abp/v0.1".into(),
        backend: json!("not an object"),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let result: Result<serde_json::Map<String, Value>, _> = data.backend_as();
    assert!(result.is_err());
}

// ════════════════════════════════════════════════════════════════════
// 19. Redact transformer on tool events
// ════════════════════════════════════════════════════════════════════

#[test]
fn redact_transformer_on_tool_call() {
    let t = RedactTransformer::new(vec!["API_KEY_123".into()]);
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "http".into(),
        tool_use_id: Some("tc1".into()),
        parent_tool_use_id: None,
        input: json!({"header": "Bearer API_KEY_123"}),
    });
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::ToolCall { input, .. } => {
            let header = input["header"].as_str().unwrap();
            assert!(!header.contains("API_KEY_123"));
            assert!(header.contains("[REDACTED]"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_on_tool_result() {
    let t = RedactTransformer::new(vec!["secret_data".into()]);
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: None,
        output: json!({"body": "has secret_data here"}),
        is_error: false,
    });
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::ToolResult { output, .. } => {
            let body = output["body"].as_str().unwrap();
            assert!(!body.contains("secret_data"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_on_command_executed() {
    let t = RedactTransformer::new(vec!["password".into()]);
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "echo password".into(),
        exit_code: Some(0),
        output_preview: Some("password visible".into()),
    });
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::CommandExecuted {
            command,
            output_preview,
            ..
        } => {
            assert!(!command.contains("password"));
            assert!(!output_preview.as_ref().unwrap().contains("password"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_on_file_changed() {
    let t = RedactTransformer::new(vec!["token".into()]);
    let ev = make_event(AgentEventKind::FileChanged {
        path: "config.yml".into(),
        summary: "added token value".into(),
    });
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::FileChanged { summary, path } => {
            assert!(!summary.contains("token"));
            // Path is not redacted
            assert_eq!(path, "config.yml");
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_on_run_started() {
    let t = RedactTransformer::new(vec!["secret".into()]);
    let ev = make_event(AgentEventKind::RunStarted {
        message: "secret mission".into(),
    });
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::RunStarted { message } => {
            assert!(!message.contains("secret"));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn redact_transformer_on_run_completed() {
    let t = RedactTransformer::new(vec!["key".into()]);
    let ev = make_event(AgentEventKind::RunCompleted {
        message: "done with key".into(),
    });
    let result = t.transform(ev).unwrap();
    match &result.kind {
        AgentEventKind::RunCompleted { message } => {
            assert!(!message.contains("key"));
        }
        _ => panic!("unexpected kind"),
    }
}

// ════════════════════════════════════════════════════════════════════
// 20. Diagnostic collector: add raw diagnostic
// ════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_collector_add_raw() {
    let mut c = DiagnosticCollector::new();
    c.add(Diagnostic {
        level: DiagnosticLevel::Debug,
        code: "DBG".into(),
        message: "debug msg".into(),
        source: Some("test".into()),
        timestamp: "2025-01-01T00:00:00Z".into(),
    });
    assert_eq!(c.diagnostics().len(), 1);
    assert_eq!(c.by_level(DiagnosticLevel::Debug).len(), 1);
    assert_eq!(c.summary().debug_count, 1);
}

// ════════════════════════════════════════════════════════════════════
// 21. ReceiptBuilder to Frame::Final roundtrip
// ════════════════════════════════════════════════════════════════════

#[test]
fn receipt_as_final_frame_roundtrip() {
    let receipt = ReceiptBuilder::new("r1", "b1")
        .event(event_text_message("hello"))
        .build();
    let frame = Frame::Final {
        ref_id: "r1".into(),
        receipt: receipt.clone(),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Frame::Final {
            ref_id,
            receipt: dec_receipt,
        } => {
            assert_eq!(ref_id, "r1");
            assert_eq!(dec_receipt["outcome"], "complete");
            assert_eq!(dec_receipt["trace"].as_array().unwrap().len(), 1);
        }
        _ => panic!("expected Final"),
    }
}

// ════════════════════════════════════════════════════════════════════
// 22. Pipeline default impl
// ════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_default_is_empty() {
    let p = EventPipeline::default();
    assert_eq!(p.stage_count(), 0);
}

#[test]
fn middleware_chain_default_is_empty() {
    let c = MiddlewareChain::default();
    assert!(c.is_empty());
}

#[test]
fn sidecar_middleware_chain_default_is_empty() {
    let c = SidecarMiddlewareChain::default();
    assert!(c.is_empty());
}

#[test]
fn transformer_chain_default_is_empty() {
    let c = TransformerChain::default();
    // Should pass an event through unchanged
    let ev = make_delta("x");
    assert!(c.process(ev).is_some());
}
