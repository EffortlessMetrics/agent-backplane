// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the sidecar-kit SDK enhancements:
//! harness, capabilities, work_order, and enhanced builders.

use serde_json::{json, Value};
use std::io::BufReader;

use sidecar_kit::builders::{
    event_error, event_frame, event_run_completed, event_run_started, event_text_delta,
    event_text_message, event_tool_call, event_tool_result, event_warning, fatal_frame,
    final_frame, hello_frame, EventBuilder, ReceiptBuilder,
};
use sidecar_kit::capabilities::{default_streaming_capabilities, CapabilitySet};
use sidecar_kit::codec::JsonlCodec;
use sidecar_kit::frame::Frame;
use sidecar_kit::harness::{HandlerContext, SidecarHandler, SidecarHarness};
use sidecar_kit::work_order::WorkOrderView;

// ── CapabilitySet tests ─────────────────────────────────────────────

#[test]
fn capability_set_empty() {
    let caps = CapabilitySet::new();
    assert!(caps.is_empty());
    assert_eq!(caps.len(), 0);
    assert_eq!(caps.build(), json!({}));
}

#[test]
fn capability_set_native() {
    let caps = CapabilitySet::new().native("streaming").native("tool_use");
    assert_eq!(caps.len(), 2);
    assert!(caps.contains("streaming"));
    assert!(!caps.contains("vision"));
    let v = caps.build();
    assert_eq!(v["streaming"], json!("native"));
    assert_eq!(v["tool_use"], json!("native"));
}

#[test]
fn capability_set_mixed_levels() {
    let v = CapabilitySet::new()
        .native("streaming")
        .emulated("vision")
        .unsupported("audio")
        .restricted("tool_bash", "sandboxed")
        .build();
    assert_eq!(v["streaming"], json!("native"));
    assert_eq!(v["vision"], json!("emulated"));
    assert_eq!(v["audio"], json!("unsupported"));
    assert_eq!(
        v["tool_bash"],
        json!({"restricted": {"reason": "sandboxed"}})
    );
}

#[test]
fn capability_set_with_custom_value() {
    let v = CapabilitySet::new()
        .with("custom_cap", json!({"level": "experimental"}))
        .build();
    assert_eq!(v["custom_cap"]["level"], "experimental");
}

#[test]
fn capability_set_overwrite() {
    // Setting same capability twice keeps the last value
    let v = CapabilitySet::new()
        .native("streaming")
        .emulated("streaming")
        .build();
    assert_eq!(v["streaming"], json!("emulated"));
}

#[test]
fn default_streaming_capabilities_has_expected() {
    let caps = default_streaming_capabilities();
    assert!(caps.contains("streaming"));
    assert!(caps.contains("tool_use"));
    assert_eq!(caps.len(), 2);
}

// ── WorkOrderView tests ─────────────────────────────────────────────

fn sample_work_order() -> Value {
    json!({
        "id": "wo-42",
        "task": "implement auth",
        "lane": "workspace_first",
        "workspace": {
            "root": "/home/user/project",
            "mode": "copy"
        },
        "context": {
            "files": [{"path": "src/main.rs", "content": "fn main() {}"}]
        },
        "policy": {
            "tools": { "allow": ["*"], "deny": [] }
        },
        "config": {
            "model": {
                "model_id": "claude-sonnet-4-20250514",
                "system_prompt": "Be concise."
            },
            "budget": {
                "max_turns": 20,
                "max_tokens": 200000
            },
            "vendor": {
                "abp": { "mode": "passthrough" }
            }
        }
    })
}

#[test]
fn work_order_view_basic_fields() {
    let wo = sample_work_order();
    let v = WorkOrderView::new(&wo);
    assert_eq!(v.id(), Some("wo-42"));
    assert_eq!(v.task(), Some("implement auth"));
    assert_eq!(v.lane(), Some("workspace_first"));
}

#[test]
fn work_order_view_workspace() {
    let wo = sample_work_order();
    let v = WorkOrderView::new(&wo);
    assert_eq!(v.workspace_root(), Some("/home/user/project"));
}

#[test]
fn work_order_view_model_config() {
    let wo = sample_work_order();
    let v = WorkOrderView::new(&wo);
    assert_eq!(v.model_id(), Some("claude-sonnet-4-20250514"));
    assert_eq!(v.system_prompt(), Some("Be concise."));
}

#[test]
fn work_order_view_budget() {
    let wo = sample_work_order();
    let v = WorkOrderView::new(&wo);
    assert_eq!(v.max_turns(), Some(20));
    assert_eq!(v.max_tokens(), Some(200000));
}

#[test]
fn work_order_view_get_path() {
    let wo = sample_work_order();
    let v = WorkOrderView::new(&wo);
    assert_eq!(
        v.get_path("config.vendor.abp.mode").and_then(Value::as_str),
        Some("passthrough")
    );
}

#[test]
fn work_order_view_missing_fields() {
    let wo = json!({});
    let v = WorkOrderView::new(&wo);
    assert_eq!(v.id(), None);
    assert_eq!(v.task(), None);
    assert_eq!(v.model_id(), None);
    assert_eq!(v.max_turns(), None);
    assert_eq!(v.workspace_root(), None);
    assert_eq!(v.system_prompt(), None);
    assert_eq!(v.get_path("nonexistent.path"), None);
}

#[test]
fn work_order_view_policy_and_context() {
    let wo = sample_work_order();
    let v = WorkOrderView::new(&wo);
    assert!(v.policy().is_some());
    assert!(v.context().is_some());
}

#[test]
fn work_order_view_raw_returns_original() {
    let wo = sample_work_order();
    let v = WorkOrderView::new(&wo);
    assert_eq!(v.raw(), &wo);
}

// ── EventBuilder tests ──────────────────────────────────────────────

#[test]
fn event_builder_basic() {
    let event = EventBuilder::new("assistant_message")
        .text("Hello, world!")
        .build();
    assert_eq!(event["type"], "assistant_message");
    assert_eq!(event["text"], "Hello, world!");
    assert!(event.get("ts").is_some());
}

#[test]
fn event_builder_with_message() {
    let event = EventBuilder::new("run_started")
        .message("starting run")
        .build();
    assert_eq!(event["type"], "run_started");
    assert_eq!(event["message"], "starting run");
}

#[test]
fn event_builder_custom_fields() {
    let event = EventBuilder::new("tool_call")
        .field("tool_name", "read_file")
        .field("tool_use_id", "tu-1")
        .field("input", json!({"path": "/tmp/file.txt"}))
        .build();
    assert_eq!(event["type"], "tool_call");
    assert_eq!(event["tool_name"], "read_file");
    assert_eq!(event["tool_use_id"], "tu-1");
    assert_eq!(event["input"]["path"], "/tmp/file.txt");
}

#[test]
fn event_builder_has_timestamp() {
    let event = EventBuilder::new("warning").message("watch out").build();
    let ts = event["ts"].as_str().unwrap();
    // Should be an RFC 3339 timestamp
    assert!(ts.contains('T'));
}

// ── Enhanced builders tests ─────────────────────────────────────────

#[test]
fn final_frame_creates_correct_frame() {
    let receipt = json!({"outcome": "complete"});
    let frame = final_frame("run-1", receipt.clone());
    match frame {
        Frame::Final { ref_id, receipt: r } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(r, receipt);
        }
        _ => panic!("expected Final frame"),
    }
}

#[test]
fn final_frame_roundtrips_through_jsonl() {
    let receipt = ReceiptBuilder::new("run-1", "test-backend").build();
    let frame = final_frame("run-1", receipt);
    let json = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Frame::Final { ref_id, .. } => assert_eq!(ref_id, "run-1"),
        _ => panic!("expected Final frame after roundtrip"),
    }
}

#[test]
fn receipt_builder_defaults() {
    let r = ReceiptBuilder::new("r1", "backend-x").build();
    assert_eq!(r["outcome"], "complete");
    assert_eq!(r["meta"]["run_id"], "r1");
    assert_eq!(r["backend"]["id"], "backend-x");
    assert_eq!(r["meta"]["contract_version"], "abp/v0.1");
}

#[test]
fn receipt_builder_failed() {
    let r = ReceiptBuilder::new("r1", "b").failed().build();
    assert_eq!(r["outcome"], "failed");
}

#[test]
fn receipt_builder_partial() {
    let r = ReceiptBuilder::new("r1", "b").partial().build();
    assert_eq!(r["outcome"], "partial");
}

#[test]
fn receipt_builder_with_tokens() {
    let r = ReceiptBuilder::new("r1", "b")
        .input_tokens(1000)
        .output_tokens(500)
        .build();
    assert_eq!(r["usage"]["input_tokens"], 1000);
    assert_eq!(r["usage"]["output_tokens"], 500);
}

#[test]
fn receipt_builder_with_events() {
    let r = ReceiptBuilder::new("r1", "b")
        .event(event_text_message("hello"))
        .event(event_run_completed("done"))
        .build();
    let trace = r["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 2);
}

#[test]
fn receipt_builder_with_artifacts() {
    let r = ReceiptBuilder::new("r1", "b")
        .artifact("patch", "diff.patch")
        .artifact("log", "run.log")
        .build();
    let artifacts = r["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0]["kind"], "patch");
    assert_eq!(artifacts[0]["path"], "diff.patch");
}

#[test]
fn receipt_builder_with_usage_raw() {
    let raw = json!({"total_tokens": 1500});
    let r = ReceiptBuilder::new("r1", "b")
        .usage_raw(raw.clone())
        .build();
    assert_eq!(r["usage_raw"], raw);
}

// ── Existing builder function tests ─────────────────────────────────

#[test]
fn event_text_delta_has_type() {
    let e = event_text_delta("chunk");
    assert_eq!(e["type"], "assistant_delta");
    assert_eq!(e["text"], "chunk");
}

#[test]
fn event_text_message_has_type() {
    let e = event_text_message("full message");
    assert_eq!(e["type"], "assistant_message");
}

#[test]
fn event_tool_call_structure() {
    let e = event_tool_call("write_file", Some("tu-1"), json!({"path": "a.txt"}));
    assert_eq!(e["type"], "tool_call");
    assert_eq!(e["tool_name"], "write_file");
    assert_eq!(e["tool_use_id"], "tu-1");
}

#[test]
fn event_tool_result_error() {
    let e = event_tool_result("bash", Some("tu-2"), json!("permission denied"), true);
    assert_eq!(e["type"], "tool_result");
    assert!(e["is_error"].as_bool().unwrap());
}

#[test]
fn event_error_has_message() {
    let e = event_error("something broke");
    assert_eq!(e["type"], "error");
    assert_eq!(e["message"], "something broke");
}

#[test]
fn event_warning_has_message() {
    let e = event_warning("heads up");
    assert_eq!(e["type"], "warning");
    assert_eq!(e["message"], "heads up");
}

#[test]
fn event_run_started_has_message() {
    let e = event_run_started("begin");
    assert_eq!(e["type"], "run_started");
}

#[test]
fn event_run_completed_has_message() {
    let e = event_run_completed("done");
    assert_eq!(e["type"], "run_completed");
}

#[test]
fn event_frame_wraps_correctly() {
    let ev = event_text_delta("hi");
    let frame = event_frame("ref-1", ev.clone());
    match frame {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "ref-1");
            assert_eq!(event, ev);
        }
        _ => panic!("expected Event frame"),
    }
}

#[test]
fn hello_frame_defaults() {
    let frame = hello_frame("test-sidecar");
    match frame {
        Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "test-sidecar");
        }
        _ => panic!("expected Hello frame"),
    }
}

#[test]
fn fatal_frame_with_ref() {
    let frame = fatal_frame(Some("run-1"), "oops");
    match frame {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id, Some("run-1".to_string()));
            assert_eq!(error, "oops");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn fatal_frame_without_ref() {
    let frame = fatal_frame(None, "early crash");
    match frame {
        Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "early crash");
        }
        _ => panic!("expected Fatal frame"),
    }
}

// ── Harness tests ───────────────────────────────────────────────────

struct EchoSidecar;

impl SidecarHandler for EchoSidecar {
    fn backend_id(&self) -> &str {
        "echo"
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::new().native("streaming")
    }

    fn handle_run(&self, ctx: HandlerContext) -> Result<Value, String> {
        let task = ctx
            .work_order_view()
            .task()
            .unwrap_or("unknown")
            .to_string();
        ctx.emit_event(event_text_message(&format!("echo: {task}")));
        Ok(ReceiptBuilder::new(&ctx.run_id, "echo").build())
    }
}

struct FailingSidecar;

impl SidecarHandler for FailingSidecar {
    fn backend_id(&self) -> &str {
        "failer"
    }

    fn handle_run(&self, _ctx: HandlerContext) -> Result<Value, String> {
        Err("something went wrong".to_string())
    }
}

/// Simulate a protocol exchange by building the input JSONL and parsing the output.
fn run_harness<H: SidecarHandler>(handler: H, work_order: Value) -> (Vec<Frame>, Vec<Frame>) {
    let run_frame = Frame::Run {
        id: "test-run".to_string(),
        work_order,
    };
    let input = JsonlCodec::encode(&run_frame).unwrap();

    let mut output = Vec::new();
    let reader = BufReader::new(input.as_bytes());
    let harness = SidecarHarness::new(handler);
    harness.run(reader, &mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    let output_frames: Vec<Frame> = output_str
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| JsonlCodec::decode(l).unwrap())
        .collect();

    // Separate: hello should be first, then events/final
    let hello_frames: Vec<Frame> = output_frames
        .iter()
        .filter(|f| matches!(f, Frame::Hello { .. }))
        .cloned()
        .collect();
    let other_frames: Vec<Frame> = output_frames
        .into_iter()
        .filter(|f| !matches!(f, Frame::Hello { .. }))
        .collect();

    (hello_frames, other_frames)
}

#[test]
fn harness_sends_hello_first() {
    let (hellos, _) = run_harness(EchoSidecar, json!({"task": "test"}));
    assert_eq!(hellos.len(), 1);
    match &hellos[0] {
        Frame::Hello {
            contract_version,
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "echo");
            assert_eq!(capabilities["streaming"], json!("native"));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn harness_streams_events_and_final() {
    let (_, frames) = run_harness(EchoSidecar, json!({"task": "greet"}));
    assert!(frames.len() >= 2, "expected at least event + final");

    // Should have at least one event
    let events: Vec<_> = frames
        .iter()
        .filter(|f| matches!(f, Frame::Event { .. }))
        .collect();
    assert!(!events.is_empty(), "expected at least one event");

    // Last frame should be Final
    let last = frames.last().unwrap();
    assert!(
        matches!(last, Frame::Final { ref_id, .. } if ref_id == "test-run"),
        "last frame should be Final"
    );
}

#[test]
fn harness_sends_fatal_on_handler_error() {
    let (_, frames) = run_harness(FailingSidecar, json!({"task": "fail"}));
    assert!(!frames.is_empty());
    let last = frames.last().unwrap();
    match last {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("test-run"));
            assert_eq!(error, "something went wrong");
        }
        _ => panic!("expected Fatal frame, got {last:?}"),
    }
}

#[test]
fn harness_clean_eof_before_run() {
    // If stdin closes before sending a Run frame, harness should exit cleanly
    let harness = SidecarHarness::new(EchoSidecar);
    let mut output = Vec::new();
    let result = harness.run(BufReader::new(&b""[..]), &mut output);
    // Should succeed (clean EOF)
    assert!(result.is_ok());
}

#[test]
fn harness_handler_context_work_order_view() {
    struct InspectSidecar;

    impl SidecarHandler for InspectSidecar {
        fn backend_id(&self) -> &str {
            "inspect"
        }

        fn handle_run(&self, ctx: HandlerContext) -> Result<Value, String> {
            let view = ctx.work_order_view();
            assert_eq!(view.task(), Some("inspect me"));
            assert_eq!(view.model_id(), Some("gpt-4"));
            Ok(ReceiptBuilder::new(&ctx.run_id, "inspect").build())
        }
    }

    let wo = json!({
        "task": "inspect me",
        "config": { "model": { "model_id": "gpt-4" } }
    });
    let (_, frames) = run_harness(InspectSidecar, wo);
    // If assertions in handler passed, we'll get a Final frame
    assert!(frames.iter().any(|f| matches!(f, Frame::Final { .. })));
}

#[test]
fn harness_handler_emits_multiple_events() {
    struct MultiEventSidecar;

    impl SidecarHandler for MultiEventSidecar {
        fn backend_id(&self) -> &str {
            "multi"
        }

        fn handle_run(&self, ctx: HandlerContext) -> Result<Value, String> {
            ctx.emit_event(event_run_started("starting"));
            ctx.emit_event(event_text_delta("chunk 1"));
            ctx.emit_event(event_text_delta("chunk 2"));
            ctx.emit_event(event_text_message("full text"));
            ctx.emit_event(event_run_completed("done"));
            Ok(ReceiptBuilder::new(&ctx.run_id, "multi").build())
        }
    }

    let (_, frames) = run_harness(MultiEventSidecar, json!({"task": "stream"}));
    let events: Vec<_> = frames
        .iter()
        .filter(|f| matches!(f, Frame::Event { .. }))
        .collect();
    assert_eq!(events.len(), 5);
}

// ── Frame roundtrip tests ───────────────────────────────────────────

#[test]
fn hello_frame_roundtrips() {
    let frame = hello_frame("test");
    let json = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "test");
        }
        _ => panic!("roundtrip failed"),
    }
}

#[test]
fn event_frame_roundtrips() {
    let ev = event_text_message("test msg");
    let frame = event_frame("ref-1", ev);
    let json = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "ref-1");
            assert_eq!(event["type"], "assistant_message");
        }
        _ => panic!("roundtrip failed"),
    }
}

#[test]
fn fatal_frame_roundtrips() {
    let frame = fatal_frame(Some("run-1"), "boom");
    let json = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id, Some("run-1".to_string()));
            assert_eq!(error, "boom");
        }
        _ => panic!("roundtrip failed"),
    }
}

// ── Integration: full protocol sequence ─────────────────────────────

#[test]
fn full_protocol_sequence() {
    struct FullSidecar;

    impl SidecarHandler for FullSidecar {
        fn backend_id(&self) -> &str {
            "full-test"
        }

        fn capabilities(&self) -> CapabilitySet {
            CapabilitySet::new()
                .native("streaming")
                .native("tool_use")
                .emulated("vision")
        }

        fn handle_run(&self, ctx: HandlerContext) -> Result<Value, String> {
            let task = ctx.work_order_view().task().unwrap_or("?").to_string();

            ctx.emit_event(event_run_started(&format!("handling: {task}")));
            ctx.emit_event(event_tool_call(
                "read_file",
                Some("tu-1"),
                json!({"path": "a.txt"}),
            ));
            ctx.emit_event(event_tool_result(
                "read_file",
                Some("tu-1"),
                json!("file contents"),
                false,
            ));
            ctx.emit_event(event_text_message("I read the file."));
            ctx.emit_event(event_run_completed("all done"));

            Ok(ReceiptBuilder::new(&ctx.run_id, "full-test")
                .input_tokens(500)
                .output_tokens(200)
                .event(event_run_started("handling"))
                .artifact("patch", "changes.patch")
                .build())
        }
    }

    let wo = json!({
        "id": "wo-integration",
        "task": "integration test",
        "config": {
            "model": { "model_id": "test-model" },
            "budget": { "max_turns": 5 }
        }
    });

    let (hellos, frames) = run_harness(FullSidecar, wo);

    // Verify hello
    assert_eq!(hellos.len(), 1);
    match &hellos[0] {
        Frame::Hello { capabilities, .. } => {
            assert_eq!(capabilities["streaming"], json!("native"));
            assert_eq!(capabilities["tool_use"], json!("native"));
            assert_eq!(capabilities["vision"], json!("emulated"));
        }
        _ => panic!("expected Hello"),
    }

    // Verify events
    let events: Vec<_> = frames
        .iter()
        .filter(|f| matches!(f, Frame::Event { .. }))
        .collect();
    assert_eq!(events.len(), 5);

    // Verify final
    let final_frame = frames.last().unwrap();
    match final_frame {
        Frame::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "test-run");
            assert_eq!(receipt["outcome"], "complete");
            assert_eq!(receipt["usage"]["input_tokens"], 500);
            assert_eq!(receipt["usage"]["output_tokens"], 200);
        }
        _ => panic!("expected Final frame"),
    }
}
