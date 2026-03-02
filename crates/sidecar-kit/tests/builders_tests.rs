// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the builders module and new middleware utilities.

use serde_json::{Value, json};
use sidecar_kit::builders::{
    ReceiptBuilder, event_command_executed, event_error, event_file_changed, event_frame,
    event_run_completed, event_run_started, event_text_delta, event_text_message, event_tool_call,
    event_tool_result, event_warning, fatal_frame, hello_frame,
};
use sidecar_kit::middleware::{ErrorWrapMiddleware, EventMiddleware, TimingMiddleware};
use sidecar_kit::{Frame, JsonlCodec, MiddlewareChain};

// ── Event builder helpers ───────────────────────────────────────────

#[test]
fn text_delta_has_correct_type() {
    let ev = event_text_delta("hello");
    assert_eq!(ev["type"], "assistant_delta");
    assert_eq!(ev["text"], "hello");
    assert!(ev["ts"].is_string());
}

#[test]
fn text_message_has_correct_type() {
    let ev = event_text_message("world");
    assert_eq!(ev["type"], "assistant_message");
    assert_eq!(ev["text"], "world");
}

#[test]
fn tool_call_has_all_fields() {
    let input = json!({"file": "foo.rs"});
    let ev = event_tool_call("read_file", Some("tc-1"), input.clone());
    assert_eq!(ev["type"], "tool_call");
    assert_eq!(ev["tool_name"], "read_file");
    assert_eq!(ev["tool_use_id"], "tc-1");
    assert_eq!(ev["input"], input);
    assert!(ev["parent_tool_use_id"].is_null());
}

#[test]
fn tool_call_with_no_id() {
    let ev = event_tool_call("bash", None, json!({}));
    assert!(ev["tool_use_id"].is_null());
}

#[test]
fn tool_result_success() {
    let ev = event_tool_result("read_file", Some("tc-1"), json!("contents"), false);
    assert_eq!(ev["type"], "tool_result");
    assert_eq!(ev["tool_name"], "read_file");
    assert_eq!(ev["is_error"], false);
}

#[test]
fn tool_result_error() {
    let ev = event_tool_result("bash", None, json!("not found"), true);
    assert_eq!(ev["is_error"], true);
}

#[test]
fn error_event_has_message() {
    let ev = event_error("something broke");
    assert_eq!(ev["type"], "error");
    assert_eq!(ev["message"], "something broke");
}

#[test]
fn warning_event_has_message() {
    let ev = event_warning("be careful");
    assert_eq!(ev["type"], "warning");
    assert_eq!(ev["message"], "be careful");
}

#[test]
fn run_started_event() {
    let ev = event_run_started("beginning work");
    assert_eq!(ev["type"], "run_started");
    assert_eq!(ev["message"], "beginning work");
}

#[test]
fn run_completed_event() {
    let ev = event_run_completed("done");
    assert_eq!(ev["type"], "run_completed");
    assert_eq!(ev["message"], "done");
}

#[test]
fn file_changed_event() {
    let ev = event_file_changed("src/main.rs", "added function");
    assert_eq!(ev["type"], "file_changed");
    assert_eq!(ev["path"], "src/main.rs");
    assert_eq!(ev["summary"], "added function");
}

#[test]
fn command_executed_event_with_all_fields() {
    let ev = event_command_executed("cargo test", Some(0), Some("ok"));
    assert_eq!(ev["type"], "command_executed");
    assert_eq!(ev["command"], "cargo test");
    assert_eq!(ev["exit_code"], 0);
    assert_eq!(ev["output_preview"], "ok");
}

#[test]
fn command_executed_event_with_none_fields() {
    let ev = event_command_executed("ls", None, None);
    assert!(ev["exit_code"].is_null());
    assert!(ev["output_preview"].is_null());
}

// ── Event builders produce valid JSONL lines ────────────────────────

#[test]
fn event_builders_produce_codec_compatible_frames() {
    let events = vec![
        event_text_delta("hi"),
        event_error("oops"),
        event_run_started("go"),
    ];
    for ev in events {
        let frame = event_frame("run-1", ev);
        let encoded = JsonlCodec::encode(&frame).unwrap();
        assert!(encoded.ends_with('\n'));
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Event { ref_id, .. } => assert_eq!(ref_id, "run-1"),
            other => panic!("expected Event frame, got {other:?}"),
        }
    }
}

// ── Frame helpers ───────────────────────────────────────────────────

#[test]
fn hello_frame_has_contract_version() {
    let frame = hello_frame("my-backend");
    match &frame {
        Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "my-backend");
        }
        other => panic!("expected Hello, got {other:?}"),
    }
    // round-trips through codec
    let line = JsonlCodec::encode(&frame).unwrap();
    let _ = JsonlCodec::decode(line.trim_end()).unwrap();
}

#[test]
fn fatal_frame_with_ref_id() {
    let frame = fatal_frame(Some("run-1"), "boom");
    match frame {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "boom");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_frame_without_ref_id() {
    let frame = fatal_frame(None, "startup failure");
    match frame {
        Frame::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ── ReceiptBuilder ──────────────────────────────────────────────────

#[test]
fn receipt_builder_default_outcome_is_complete() {
    let receipt = ReceiptBuilder::new("run-1", "mock").build();
    assert_eq!(receipt["outcome"], "complete");
}

#[test]
fn receipt_builder_failed_outcome() {
    let receipt = ReceiptBuilder::new("run-1", "mock").failed().build();
    assert_eq!(receipt["outcome"], "failed");
}

#[test]
fn receipt_builder_partial_outcome() {
    let receipt = ReceiptBuilder::new("run-1", "mock").partial().build();
    assert_eq!(receipt["outcome"], "partial");
}

#[test]
fn receipt_builder_with_events() {
    let receipt = ReceiptBuilder::new("r1", "b1")
        .event(event_text_delta("hi"))
        .event(event_run_completed("done"))
        .build();
    let trace = receipt["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0]["type"], "assistant_delta");
    assert_eq!(trace[1]["type"], "run_completed");
}

#[test]
fn receipt_builder_with_artifacts() {
    let receipt = ReceiptBuilder::new("r1", "b1")
        .artifact("patch", "changes.patch")
        .build();
    let arts = receipt["artifacts"].as_array().unwrap();
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0]["kind"], "patch");
    assert_eq!(arts[0]["path"], "changes.patch");
}

#[test]
fn receipt_builder_with_usage() {
    let receipt = ReceiptBuilder::new("r1", "b1")
        .input_tokens(100)
        .output_tokens(200)
        .usage_raw(json!({"prompt_tokens": 100}))
        .build();
    assert_eq!(receipt["usage"]["input_tokens"], 100);
    assert_eq!(receipt["usage"]["output_tokens"], 200);
    assert_eq!(receipt["usage_raw"]["prompt_tokens"], 100);
}

#[test]
fn receipt_builder_has_meta_fields() {
    let receipt = ReceiptBuilder::new("r-42", "my-sidecar").build();
    assert_eq!(receipt["meta"]["run_id"], "r-42");
    assert_eq!(receipt["meta"]["contract_version"], "abp/v0.1");
    assert_eq!(receipt["backend"]["id"], "my-sidecar");
    assert!(receipt["receipt_sha256"].is_null());
}

#[test]
fn receipt_builder_wraps_in_final_frame() {
    let receipt = ReceiptBuilder::new("r1", "b1").build();
    let frame = Frame::Final {
        ref_id: "r1".to_string(),
        receipt,
    };
    let line = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(line.trim_end()).unwrap();
    match decoded {
        Frame::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "r1");
            assert_eq!(receipt["outcome"], "complete");
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

// ── TimingMiddleware ────────────────────────────────────────────────

#[test]
fn timing_middleware_adds_processing_us() {
    let mw = TimingMiddleware::new();
    let event = json!({"type": "assistant_delta", "text": "hi"});
    let result = mw.process(&event).unwrap();
    assert!(result.get("_processing_us").is_some());
    assert!(result["_processing_us"].is_u64());
}

#[test]
fn timing_middleware_preserves_original_fields() {
    let mw = TimingMiddleware::new();
    let event = json!({"type": "error", "message": "oops"});
    let result = mw.process(&event).unwrap();
    assert_eq!(result["type"], "error");
    assert_eq!(result["message"], "oops");
}

#[test]
fn timing_middleware_on_non_object_still_returns_some() {
    let mw = TimingMiddleware::new();
    let event = json!("bare string");
    let result = mw.process(&event).unwrap();
    // non-object: no field injected, but value still passes through
    assert!(result.get("_processing_us").is_none());
    assert_eq!(result, json!("bare string"));
}

#[test]
fn timing_middleware_default_trait() {
    let mw = TimingMiddleware;
    let ev = json!({"x": 1});
    assert!(mw.process(&ev).is_some());
}

// ── ErrorWrapMiddleware ─────────────────────────────────────────────

#[test]
fn error_wrap_passes_objects_through() {
    let mw = ErrorWrapMiddleware::new();
    let event = json!({"type": "assistant_delta", "text": "hi"});
    let result = mw.process(&event).unwrap();
    assert_eq!(result["type"], "assistant_delta");
}

#[test]
fn error_wrap_converts_string_to_error_event() {
    let mw = ErrorWrapMiddleware::new();
    let event = json!("bad data");
    let result = mw.process(&event).unwrap();
    assert_eq!(result["type"], "error");
    assert!(result["message"].as_str().unwrap().contains("non-object"));
    assert_eq!(result["_original"], "bad data");
}

#[test]
fn error_wrap_converts_number_to_error_event() {
    let mw = ErrorWrapMiddleware::new();
    let result = mw.process(&json!(42)).unwrap();
    assert_eq!(result["type"], "error");
    assert_eq!(result["_original"], 42);
}

#[test]
fn error_wrap_converts_array_to_error_event() {
    let mw = ErrorWrapMiddleware::new();
    let result = mw.process(&json!([1, 2, 3])).unwrap();
    assert_eq!(result["type"], "error");
}

#[test]
fn error_wrap_converts_null_to_error_event() {
    let mw = ErrorWrapMiddleware::new();
    let result = mw.process(&Value::Null).unwrap();
    assert_eq!(result["type"], "error");
}

// ── Middleware chain integration ─────────────────────────────────────

#[test]
fn chain_error_wrap_then_timing() {
    let chain = MiddlewareChain::new()
        .with(ErrorWrapMiddleware::new())
        .with(TimingMiddleware::new());

    // Object event flows through both
    let ev = json!({"type": "warning", "message": "hmm"});
    let result = chain.process(&ev).unwrap();
    assert_eq!(result["type"], "warning");
    assert!(result.get("_processing_us").is_some());

    // Non-object gets wrapped then timed
    let bad = json!(42);
    let result = chain.process(&bad).unwrap();
    assert_eq!(result["type"], "error");
    assert!(result.get("_processing_us").is_some());
}

#[test]
fn chain_timing_multiple_events() {
    let chain = MiddlewareChain::new().with(TimingMiddleware::new());
    for i in 0..5 {
        let ev = json!({"seq": i});
        let result = chain.process(&ev).unwrap();
        assert_eq!(result["seq"], i);
        assert!(result.get("_processing_us").is_some());
    }
}

// ── JSONL line construction and parsing ─────────────────────────────

#[test]
fn jsonl_line_is_single_line() {
    let frame = event_frame("r1", event_text_delta("hello\nworld"));
    let line = JsonlCodec::encode(&frame).unwrap();
    // Must be exactly one newline at the end
    assert_eq!(line.matches('\n').count(), 1);
    assert!(line.ends_with('\n'));
}

#[test]
fn jsonl_roundtrip_with_unicode() {
    let frame = event_frame("r1", event_text_message("日本語テスト 🦀"));
    let line = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(line.trim_end()).unwrap();
    match decoded {
        Frame::Event { event, .. } => {
            assert_eq!(event["text"], "日本語テスト 🦀");
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn jsonl_roundtrip_with_special_chars() {
    let ev = event_text_delta("tab\there\nnewline\"quote\\backslash");
    let frame = event_frame("r1", ev);
    let line = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(line.trim_end()).unwrap();
    match decoded {
        Frame::Event { event, .. } => {
            let text = event["text"].as_str().unwrap();
            assert!(text.contains("tab\there"));
            assert!(text.contains("newline"));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}
