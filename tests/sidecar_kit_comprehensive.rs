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
//! Comprehensive tests for the `sidecar-kit` crate.

use std::collections::BTreeMap;

use abp_core::{AgentEvent, AgentEventKind};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use sidecar_kit::*;

// ═══════════════════════════════════════════════════════════════════════
// 1. Frame types and construction
// ═══════════════════════════════════════════════════════════════════════

mod frame_construction {
    use super::*;

    #[test]
    fn hello_frame_has_correct_contract_version() {
        let f = hello_frame("test-backend");
        match &f {
            Frame::Hello {
                contract_version, ..
            } => assert_eq!(contract_version, "abp/v0.1"),
            _ => panic!("expected Hello frame"),
        }
    }

    #[test]
    fn hello_frame_backend_contains_id() {
        let f = hello_frame("my-backend");
        match &f {
            Frame::Hello { backend, .. } => {
                assert_eq!(backend["id"], "my-backend");
            }
            _ => panic!("expected Hello frame"),
        }
    }

    #[test]
    fn hello_frame_capabilities_default_empty() {
        let f = hello_frame("x");
        match &f {
            Frame::Hello { capabilities, .. } => {
                assert_eq!(capabilities, &json!({}));
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_frame_mode_is_null_by_default() {
        let f = hello_frame("x");
        match &f {
            Frame::Hello { mode, .. } => assert!(mode.is_null()),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn event_frame_sets_ref_id_and_event() {
        let ev = json!({"type": "test"});
        let f = event_frame("run-1", ev.clone());
        match &f {
            Frame::Event { ref_id, event } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(event, &ev);
            }
            _ => panic!("expected Event frame"),
        }
    }

    #[test]
    fn fatal_frame_with_ref_id() {
        let f = fatal_frame(Some("run-1"), "boom");
        match &f {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id.as_deref(), Some("run-1"));
                assert_eq!(error, "boom");
            }
            _ => panic!("expected Fatal frame"),
        }
    }

    #[test]
    fn fatal_frame_without_ref_id() {
        let f = fatal_frame(None, "unknown error");
        match &f {
            Frame::Fatal { ref_id, error } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "unknown error");
            }
            _ => panic!("expected Fatal frame"),
        }
    }

    #[test]
    fn run_frame_construction() {
        let f = Frame::Run {
            id: "r1".into(),
            work_order: json!({"task": "hello"}),
        };
        match &f {
            Frame::Run { id, work_order } => {
                assert_eq!(id, "r1");
                assert_eq!(work_order["task"], "hello");
            }
            _ => panic!("expected Run frame"),
        }
    }

    #[test]
    fn final_frame_construction() {
        let f = Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({"outcome": "complete"}),
        };
        match &f {
            Frame::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(receipt["outcome"], "complete");
            }
            _ => panic!("expected Final frame"),
        }
    }

    #[test]
    fn cancel_frame_with_reason() {
        let f = Frame::Cancel {
            ref_id: "r1".into(),
            reason: Some("timeout".into()),
        };
        match &f {
            Frame::Cancel { ref_id, reason } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(reason.as_deref(), Some("timeout"));
            }
            _ => panic!("expected Cancel frame"),
        }
    }

    #[test]
    fn cancel_frame_without_reason() {
        let f = Frame::Cancel {
            ref_id: "r1".into(),
            reason: None,
        };
        match &f {
            Frame::Cancel { reason, .. } => assert!(reason.is_none()),
            _ => panic!("expected Cancel frame"),
        }
    }

    #[test]
    fn ping_frame_construction() {
        let f = Frame::Ping { seq: 42 };
        match &f {
            Frame::Ping { seq } => assert_eq!(*seq, 42),
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn pong_frame_construction() {
        let f = Frame::Pong { seq: 99 };
        match &f {
            Frame::Pong { seq } => assert_eq!(*seq, 99),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn frame_try_event_on_event_frame() {
        let ev = json!({"key": "value"});
        let f = Frame::Event {
            ref_id: "r1".into(),
            event: ev,
        };
        let result: Result<(String, serde_json::Map<String, Value>), _> = f.try_event();
        let (ref_id, map) = result.unwrap();
        assert_eq!(ref_id, "r1");
        assert_eq!(map["key"], "value");
    }

    #[test]
    fn frame_try_event_on_non_event_returns_error() {
        let f = Frame::Ping { seq: 1 };
        let result: Result<(String, Value), _> = f.try_event();
        assert!(result.is_err());
    }

    #[test]
    fn frame_try_final_on_final_frame() {
        let f = Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({"outcome": "done"}),
        };
        let result: Result<(String, serde_json::Map<String, Value>), _> = f.try_final();
        let (ref_id, map) = result.unwrap();
        assert_eq!(ref_id, "r1");
        assert_eq!(map["outcome"], "done");
    }

    #[test]
    fn frame_try_final_on_non_final_returns_error() {
        let f = Frame::Ping { seq: 1 };
        let result: Result<(String, Value), _> = f.try_final();
        assert!(result.is_err());
    }

    #[test]
    fn frame_clone() {
        let f = hello_frame("test");
        let f2 = f.clone();
        let json1 = serde_json::to_string(&f).unwrap();
        let json2 = serde_json::to_string(&f2).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn frame_debug_impl() {
        let f = Frame::Ping { seq: 1 };
        let debug = format!("{f:?}");
        assert!(debug.contains("Ping"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Middleware pipeline (value-based)
// ═══════════════════════════════════════════════════════════════════════

mod middleware_tests {
    use super::*;

    #[test]
    fn logging_middleware_passes_through() {
        let mw = LoggingMiddleware::new();
        let ev = json!({"type": "test", "data": 1});
        let result = mw.process(&ev);
        assert_eq!(result, Some(ev));
    }

    #[test]
    fn filter_include_passes_matching() {
        let mw = FilterMiddleware::include_kinds(&["error", "warning"]);
        let ev = json!({"type": "error", "message": "oops"});
        assert!(mw.process(&ev).is_some());
    }

    #[test]
    fn filter_include_drops_non_matching() {
        let mw = FilterMiddleware::include_kinds(&["error"]);
        let ev = json!({"type": "warning", "message": "hmm"});
        assert!(mw.process(&ev).is_none());
    }

    #[test]
    fn filter_include_empty_list_drops_all() {
        let mw = FilterMiddleware::include_kinds(&[]);
        let ev = json!({"type": "anything"});
        assert!(mw.process(&ev).is_none());
    }

    #[test]
    fn filter_exclude_drops_matching() {
        let mw = FilterMiddleware::exclude_kinds(&["debug"]);
        let ev = json!({"type": "debug", "msg": "verbose"});
        assert!(mw.process(&ev).is_none());
    }

    #[test]
    fn filter_exclude_passes_non_matching() {
        let mw = FilterMiddleware::exclude_kinds(&["debug"]);
        let ev = json!({"type": "error", "msg": "bad"});
        assert!(mw.process(&ev).is_some());
    }

    #[test]
    fn filter_exclude_empty_list_passes_all() {
        let mw = FilterMiddleware::exclude_kinds(&[]);
        let ev = json!({"type": "anything"});
        assert!(mw.process(&ev).is_some());
    }

    #[test]
    fn filter_case_insensitive() {
        let mw = FilterMiddleware::include_kinds(&["Error"]);
        let ev = json!({"type": "error"});
        assert!(mw.process(&ev).is_some());
    }

    #[test]
    fn filter_missing_type_field() {
        let mw = FilterMiddleware::include_kinds(&["error"]);
        let ev = json!({"msg": "no type field"});
        assert!(mw.process(&ev).is_none());
    }

    #[test]
    fn filter_exclude_missing_type_passes() {
        let mw = FilterMiddleware::exclude_kinds(&["error"]);
        let ev = json!({"msg": "no type"});
        // missing type means type_name is "", which is not in the exclude list
        assert!(mw.process(&ev).is_some());
    }

    #[test]
    fn timing_middleware_adds_processing_us() {
        let mw = TimingMiddleware::new();
        let ev = json!({"type": "test"});
        let result = mw.process(&ev).unwrap();
        assert!(result.get("_processing_us").is_some());
        assert!(result["_processing_us"].is_number());
    }

    #[test]
    fn timing_middleware_non_object_passes_through() {
        let mw = TimingMiddleware::new();
        let ev = json!("just a string");
        let result = mw.process(&ev).unwrap();
        assert!(result.get("_processing_us").is_none());
    }

    #[test]
    fn error_wrap_middleware_passes_objects() {
        let mw = ErrorWrapMiddleware::new();
        let ev = json!({"type": "test"});
        let result = mw.process(&ev).unwrap();
        assert_eq!(result["type"], "test");
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
    fn error_wrap_wraps_string_value() {
        let mw = ErrorWrapMiddleware::new();
        let ev = json!("hello");
        let result = mw.process(&ev).unwrap();
        assert_eq!(result["type"], "error");
        assert_eq!(result["_original"], "hello");
    }

    #[test]
    fn error_wrap_wraps_array_value() {
        let mw = ErrorWrapMiddleware::new();
        let ev = json!([1, 2, 3]);
        let result = mw.process(&ev).unwrap();
        assert_eq!(result["type"], "error");
    }

    #[test]
    fn error_wrap_wraps_null_value() {
        let mw = ErrorWrapMiddleware::new();
        let ev = json!(null);
        let result = mw.process(&ev).unwrap();
        assert_eq!(result["type"], "error");
    }

    #[test]
    fn error_wrap_wraps_bool_value() {
        let mw = ErrorWrapMiddleware::new();
        let ev = json!(true);
        let result = mw.process(&ev).unwrap();
        assert_eq!(result["type"], "error");
    }

    #[test]
    fn middleware_chain_empty_is_passthrough() {
        let chain = MiddlewareChain::new();
        let ev = json!({"type": "test"});
        assert_eq!(chain.process(&ev), Some(ev));
    }

    #[test]
    fn middleware_chain_is_empty() {
        let chain = MiddlewareChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn middleware_chain_push_increases_len() {
        let mut chain = MiddlewareChain::new();
        chain.push(LoggingMiddleware::new());
        assert_eq!(chain.len(), 1);
        assert!(!chain.is_empty());
    }

    #[test]
    fn middleware_chain_with_builder() {
        let chain = MiddlewareChain::new()
            .with(LoggingMiddleware::new())
            .with(TimingMiddleware::new());
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn middleware_chain_processes_in_order() {
        let chain = MiddlewareChain::new()
            .with(ErrorWrapMiddleware::new())
            .with(TimingMiddleware::new());

        let ev = json!(42);
        let result = chain.process(&ev).unwrap();
        // ErrorWrap converts to object, then Timing adds _processing_us
        assert_eq!(result["type"], "error");
        assert!(result.get("_processing_us").is_some());
    }

    #[test]
    fn middleware_chain_short_circuits_on_drop() {
        let chain = MiddlewareChain::new()
            .with(FilterMiddleware::include_kinds(&["error"]))
            .with(TimingMiddleware::new());

        let ev = json!({"type": "info"});
        assert!(chain.process(&ev).is_none());
    }

    #[test]
    fn middleware_chain_default() {
        let chain = MiddlewareChain::default();
        assert!(chain.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Message serialization (codec)
// ═══════════════════════════════════════════════════════════════════════

mod serialization_tests {
    use super::*;

    #[test]
    fn codec_encode_hello_ends_with_newline() {
        let f = hello_frame("test");
        let s = JsonlCodec::encode(&f).unwrap();
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn codec_roundtrip_hello() {
        let f = hello_frame("test-be");
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Hello {
                contract_version,
                backend,
                ..
            } => {
                assert_eq!(contract_version, "abp/v0.1");
                assert_eq!(backend["id"], "test-be");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn codec_roundtrip_event() {
        let f = event_frame("r1", json!({"type": "text_delta", "text": "hi"}));
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Event { ref_id, event } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(event["text"], "hi");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn codec_roundtrip_fatal() {
        let f = fatal_frame(Some("r1"), "error msg");
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id, Some("r1".into()));
                assert_eq!(error, "error msg");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn codec_roundtrip_run() {
        let f = Frame::Run {
            id: "r1".into(),
            work_order: json!({"task": "do stuff"}),
        };
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Run { id, work_order } => {
                assert_eq!(id, "r1");
                assert_eq!(work_order["task"], "do stuff");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn codec_roundtrip_final() {
        let f = Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({"outcome": "complete"}),
        };
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(receipt["outcome"], "complete");
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn codec_roundtrip_cancel() {
        let f = Frame::Cancel {
            ref_id: "r1".into(),
            reason: Some("user cancelled".into()),
        };
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Cancel { ref_id, reason } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(reason.as_deref(), Some("user cancelled"));
            }
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn codec_roundtrip_ping() {
        let f = Frame::Ping { seq: 7 };
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Ping { seq } => assert_eq!(seq, 7),
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn codec_roundtrip_pong() {
        let f = Frame::Pong { seq: 7 };
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Pong { seq } => assert_eq!(seq, 7),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn codec_decode_invalid_json() {
        let result = JsonlCodec::decode("not json{");
        assert!(result.is_err());
    }

    #[test]
    fn codec_decode_unknown_tag() {
        let result = JsonlCodec::decode(r#"{"t":"unknown_variant","data":1}"#);
        assert!(result.is_err());
    }

    #[test]
    fn codec_encode_produces_single_line() {
        let f = hello_frame("test");
        let encoded = JsonlCodec::encode(&f).unwrap();
        let lines: Vec<&str> = encoded.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn hello_frame_tag_is_t() {
        let f = hello_frame("test");
        let json_str = serde_json::to_string(&f).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["t"], "hello");
    }

    #[test]
    fn event_frame_tag_is_t() {
        let f = event_frame("r1", json!({}));
        let json_str = serde_json::to_string(&f).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["t"], "event");
    }

    #[test]
    fn fatal_frame_tag_is_t() {
        let f = fatal_frame(None, "err");
        let json_str = serde_json::to_string(&f).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["t"], "fatal");
    }

    #[test]
    fn run_frame_tag_is_t() {
        let f = Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        };
        let v: Value = serde_json::to_value(&f).unwrap();
        assert_eq!(v["t"], "run");
    }

    #[test]
    fn cancel_frame_tag_is_t() {
        let f = Frame::Cancel {
            ref_id: "r1".into(),
            reason: None,
        };
        let v: Value = serde_json::to_value(&f).unwrap();
        assert_eq!(v["t"], "cancel");
    }

    #[test]
    fn ping_frame_tag_is_t() {
        let f = Frame::Ping { seq: 1 };
        let v: Value = serde_json::to_value(&f).unwrap();
        assert_eq!(v["t"], "ping");
    }

    #[test]
    fn pong_frame_tag_is_t() {
        let f = Frame::Pong { seq: 1 };
        let v: Value = serde_json::to_value(&f).unwrap();
        assert_eq!(v["t"], "pong");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Stream / Pipeline stages
// ═══════════════════════════════════════════════════════════════════════

mod pipeline_tests {
    use super::*;

    #[test]
    fn empty_pipeline_is_passthrough() {
        let pipe = EventPipeline::new();
        let ev = json!({"type": "test"});
        let result = pipe.process(ev.clone()).unwrap();
        assert_eq!(result, Some(ev));
    }

    #[test]
    fn pipeline_default_is_empty() {
        let pipe = EventPipeline::default();
        assert_eq!(pipe.stage_count(), 0);
    }

    #[test]
    fn pipeline_add_stage_increases_count() {
        let mut pipe = EventPipeline::new();
        pipe.add_stage(Box::new(TimestampStage::new()));
        assert_eq!(pipe.stage_count(), 1);
    }

    #[test]
    fn timestamp_stage_name() {
        let s = TimestampStage::new();
        assert_eq!(s.name(), "timestamp");
    }

    #[test]
    fn timestamp_stage_adds_processed_at() {
        let s = TimestampStage::new();
        let ev = json!({"type": "test"});
        let result = s.process(ev).unwrap().unwrap();
        assert!(result.get("processed_at").is_some());
        assert!(result["processed_at"].is_number());
    }

    #[test]
    fn timestamp_stage_rejects_non_object() {
        let s = TimestampStage::new();
        let result = s.process(json!(42));
        assert!(result.is_err());
    }

    #[test]
    fn redact_stage_name() {
        let s = RedactStage::new(vec!["secret".into()]);
        assert_eq!(s.name(), "redact");
    }

    #[test]
    fn redact_stage_removes_fields() {
        let s = RedactStage::new(vec!["password".into(), "token".into()]);
        let ev = json!({"type": "test", "password": "hunter2", "token": "abc", "ok": true});
        let result = s.process(ev).unwrap().unwrap();
        assert!(result.get("password").is_none());
        assert!(result.get("token").is_none());
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn redact_stage_no_op_when_fields_absent() {
        let s = RedactStage::new(vec!["missing".into()]);
        let ev = json!({"type": "test"});
        let result = s.process(ev.clone()).unwrap().unwrap();
        assert_eq!(result, ev);
    }

    #[test]
    fn redact_stage_rejects_non_object() {
        let s = RedactStage::new(vec![]);
        let result = s.process(json!("string"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_stage_name() {
        let s = ValidateStage::new(vec!["type".into()]);
        assert_eq!(s.name(), "validate");
    }

    #[test]
    fn validate_stage_passes_when_fields_present() {
        let s = ValidateStage::new(vec!["type".into(), "data".into()]);
        let ev = json!({"type": "test", "data": 42});
        let result = s.process(ev).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn validate_stage_fails_when_field_missing() {
        let s = ValidateStage::new(vec!["type".into(), "data".into()]);
        let ev = json!({"type": "test"});
        let result = s.process(ev);
        assert!(result.is_err());
    }

    #[test]
    fn validate_stage_error_message_contains_field_name() {
        let s = ValidateStage::new(vec!["missing_field".into()]);
        let ev = json!({"type": "test"});
        let err = s.process(ev).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("missing_field"));
    }

    #[test]
    fn validate_stage_rejects_non_object() {
        let s = ValidateStage::new(vec![]);
        let result = s.process(json!(null));
        assert!(result.is_err());
    }

    #[test]
    fn pipeline_multiple_stages() {
        let mut pipe = EventPipeline::new();
        pipe.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
        pipe.add_stage(Box::new(RedactStage::new(vec!["secret".into()])));
        pipe.add_stage(Box::new(TimestampStage::new()));

        let ev = json!({"type": "test", "secret": "value", "data": 1});
        let result = pipe.process(ev).unwrap().unwrap();

        assert_eq!(result["type"], "test");
        assert!(result.get("secret").is_none());
        assert!(result.get("processed_at").is_some());
    }

    #[test]
    fn pipeline_short_circuits_on_filter() {
        let mut pipe = EventPipeline::new();
        pipe.add_stage(Box::new(ValidateStage::new(vec!["required".into()])));
        pipe.add_stage(Box::new(TimestampStage::new()));

        let ev = json!({"type": "test"});
        let result = pipe.process(ev);
        assert!(result.is_err());
    }

    #[test]
    fn pipeline_error_display() {
        let err = PipelineError::StageError {
            stage: "validate".into(),
            message: "missing field".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("validate"));
        assert!(msg.contains("missing field"));
    }

    #[test]
    fn pipeline_invalid_event_display() {
        let err = PipelineError::InvalidEvent;
        let msg = format!("{err}");
        assert!(msg.contains("not a valid JSON object"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Error types
// ═══════════════════════════════════════════════════════════════════════

mod error_tests {
    use super::*;

    #[test]
    fn error_spawn_display() {
        let e = SidecarError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "nope"));
        let msg = format!("{e}");
        assert!(msg.contains("spawn"));
    }

    #[test]
    fn error_stdout_display() {
        let e = SidecarError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
        let msg = format!("{e}");
        assert!(msg.contains("stdout"));
    }

    #[test]
    fn error_stdin_display() {
        let e = SidecarError::Stdin(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
        let msg = format!("{e}");
        assert!(msg.contains("stdin"));
    }

    #[test]
    fn error_protocol_display() {
        let e = SidecarError::Protocol("bad frame".into());
        let msg = format!("{e}");
        assert!(msg.contains("protocol"));
        assert!(msg.contains("bad frame"));
    }

    #[test]
    fn error_fatal_display() {
        let e = SidecarError::Fatal("kaboom".into());
        let msg = format!("{e}");
        assert!(msg.contains("fatal"));
        assert!(msg.contains("kaboom"));
    }

    #[test]
    fn error_exited_display() {
        let e = SidecarError::Exited(Some(1));
        let msg = format!("{e}");
        assert!(msg.contains("exited"));
    }

    #[test]
    fn error_exited_none_display() {
        let e = SidecarError::Exited(None);
        let msg = format!("{e}");
        assert!(msg.contains("exited"));
    }

    #[test]
    fn error_timeout_display() {
        let e = SidecarError::Timeout;
        let msg = format!("{e}");
        assert!(msg.contains("timed out"));
    }

    #[test]
    fn error_debug_impl() {
        let e = SidecarError::Timeout;
        let debug = format!("{e:?}");
        assert!(debug.contains("Timeout"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Configuration (ProcessSpec, CancelToken, Builders)
// ═══════════════════════════════════════════════════════════════════════

mod config_tests {
    use super::*;

    #[test]
    fn process_spec_new() {
        let spec = ProcessSpec::new("python3");
        assert_eq!(spec.command, "python3");
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn process_spec_with_args() {
        let mut spec = ProcessSpec::new("node");
        spec.args = vec!["script.js".into()];
        assert_eq!(spec.args.len(), 1);
    }

    #[test]
    fn process_spec_with_env() {
        let mut spec = ProcessSpec::new("python");
        spec.env.insert("KEY".into(), "VALUE".into());
        assert_eq!(spec.env["KEY"], "VALUE");
    }

    #[test]
    fn process_spec_with_cwd() {
        let mut spec = ProcessSpec::new("node");
        spec.cwd = Some("/tmp".into());
        assert_eq!(spec.cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn process_spec_debug() {
        let spec = ProcessSpec::new("cmd");
        let debug = format!("{spec:?}");
        assert!(debug.contains("cmd"));
    }

    #[test]
    fn process_spec_clone() {
        let mut spec = ProcessSpec::new("node");
        spec.args = vec!["a".into()];
        let spec2 = spec.clone();
        assert_eq!(spec.command, spec2.command);
        assert_eq!(spec.args, spec2.args);
    }
}

mod cancel_token_tests {
    use super::*;

    #[test]
    fn cancel_token_initial_state() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_token_cancel() {
        let token = CancelToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_token_clone_shares_state() {
        let token = CancelToken::new();
        let token2 = token.clone();
        token.cancel();
        assert!(token2.is_cancelled());
    }

    #[test]
    fn cancel_token_default() {
        let token = CancelToken::default();
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_token_cancelled_future_resolves() {
        let token = CancelToken::new();
        let t2 = token.clone();
        tokio::spawn(async move {
            t2.cancel();
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
            .await
            .expect("cancelled future should resolve");
    }

    #[tokio::test]
    async fn cancel_token_already_cancelled_returns_immediately() {
        let token = CancelToken::new();
        token.cancel();
        // Should return immediately since already cancelled.
        token.cancelled().await;
        assert!(token.is_cancelled());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Protocol compliance (builders)
// ═══════════════════════════════════════════════════════════════════════

mod builder_tests {
    use super::*;

    #[test]
    fn event_text_delta_has_correct_type() {
        let ev = event_text_delta("hello");
        assert_eq!(ev["type"], "assistant_delta");
        assert_eq!(ev["text"], "hello");
        assert!(ev["ts"].is_string());
    }

    #[test]
    fn event_text_message_has_correct_type() {
        let ev = event_text_message("full message");
        assert_eq!(ev["type"], "assistant_message");
        assert_eq!(ev["text"], "full message");
    }

    #[test]
    fn event_tool_call_shape() {
        let ev = event_tool_call("read_file", Some("tc-1"), json!({"path": "/etc/hosts"}));
        assert_eq!(ev["type"], "tool_call");
        assert_eq!(ev["tool_name"], "read_file");
        assert_eq!(ev["tool_use_id"], "tc-1");
        assert_eq!(ev["input"]["path"], "/etc/hosts");
    }

    #[test]
    fn event_tool_call_no_use_id() {
        let ev = event_tool_call("shell", None, json!({}));
        assert!(ev["tool_use_id"].is_null());
    }

    #[test]
    fn event_tool_result_shape() {
        let ev = event_tool_result("read_file", Some("tc-1"), json!("contents"), false);
        assert_eq!(ev["type"], "tool_result");
        assert_eq!(ev["tool_name"], "read_file");
        assert_eq!(ev["is_error"], false);
    }

    #[test]
    fn event_tool_result_with_error() {
        let ev = event_tool_result("shell", None, json!("fail"), true);
        assert_eq!(ev["is_error"], true);
    }

    #[test]
    fn event_error_shape() {
        let ev = event_error("something broke");
        assert_eq!(ev["type"], "error");
        assert_eq!(ev["message"], "something broke");
    }

    #[test]
    fn event_warning_shape() {
        let ev = event_warning("be careful");
        assert_eq!(ev["type"], "warning");
        assert_eq!(ev["message"], "be careful");
    }

    #[test]
    fn event_run_started_shape() {
        let ev = event_run_started("starting...");
        assert_eq!(ev["type"], "run_started");
        assert_eq!(ev["message"], "starting...");
    }

    #[test]
    fn event_run_completed_shape() {
        let ev = event_run_completed("done!");
        assert_eq!(ev["type"], "run_completed");
        assert_eq!(ev["message"], "done!");
    }

    #[test]
    fn event_file_changed_shape() {
        let ev = event_file_changed("src/main.rs", "added function");
        assert_eq!(ev["type"], "file_changed");
        assert_eq!(ev["path"], "src/main.rs");
        assert_eq!(ev["summary"], "added function");
    }

    #[test]
    fn event_command_executed_shape() {
        let ev = event_command_executed("cargo build", Some(0), Some("compiled"));
        assert_eq!(ev["type"], "command_executed");
        assert_eq!(ev["command"], "cargo build");
        assert_eq!(ev["exit_code"], 0);
        assert_eq!(ev["output_preview"], "compiled");
    }

    #[test]
    fn event_command_executed_no_exit_code() {
        let ev = event_command_executed("kill -9", None, None);
        assert!(ev["exit_code"].is_null());
        assert!(ev["output_preview"].is_null());
    }

    #[test]
    fn receipt_builder_default_outcome() {
        let r = ReceiptBuilder::new("r1", "be1").build();
        assert_eq!(r["outcome"], "complete");
        assert_eq!(r["meta"]["run_id"], "r1");
        assert_eq!(r["backend"]["id"], "be1");
        assert_eq!(r["meta"]["contract_version"], "abp/v0.1");
    }

    #[test]
    fn receipt_builder_failed() {
        let r = ReceiptBuilder::new("r1", "be1").failed().build();
        assert_eq!(r["outcome"], "failed");
    }

    #[test]
    fn receipt_builder_partial() {
        let r = ReceiptBuilder::new("r1", "be1").partial().build();
        assert_eq!(r["outcome"], "partial");
    }

    #[test]
    fn receipt_builder_events() {
        let r = ReceiptBuilder::new("r1", "be1")
            .event(json!({"type": "delta"}))
            .event(json!({"type": "message"}))
            .build();
        assert_eq!(r["trace"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn receipt_builder_artifacts() {
        let r = ReceiptBuilder::new("r1", "be1")
            .artifact("file", "src/main.rs")
            .build();
        let arts = r["artifacts"].as_array().unwrap();
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0]["kind"], "file");
        assert_eq!(arts[0]["path"], "src/main.rs");
    }

    #[test]
    fn receipt_builder_usage_raw() {
        let r = ReceiptBuilder::new("r1", "be1")
            .usage_raw(json!({"prompt_tokens": 100}))
            .build();
        assert_eq!(r["usage_raw"]["prompt_tokens"], 100);
    }

    #[test]
    fn receipt_builder_token_counts() {
        let r = ReceiptBuilder::new("r1", "be1")
            .input_tokens(500)
            .output_tokens(200)
            .build();
        assert_eq!(r["usage"]["input_tokens"], 500);
        assert_eq!(r["usage"]["output_tokens"], 200);
    }

    #[test]
    fn receipt_builder_receipt_sha256_is_null() {
        let r = ReceiptBuilder::new("r1", "be1").build();
        assert!(r["receipt_sha256"].is_null());
    }

    #[test]
    fn receipt_builder_mode_is_mapped() {
        let r = ReceiptBuilder::new("r1", "be1").build();
        assert_eq!(r["mode"], "mapped");
    }

    #[test]
    fn receipt_builder_has_timestamps() {
        let r = ReceiptBuilder::new("r1", "be1").build();
        assert!(r["meta"]["started_at"].is_string());
        assert!(r["meta"]["finished_at"].is_string());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Edge cases
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn empty_string_backend_in_hello() {
        let f = hello_frame("");
        match &f {
            Frame::Hello { backend, .. } => assert_eq!(backend["id"], ""),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn very_long_error_message_in_fatal() {
        let long_msg = "x".repeat(10_000);
        let f = fatal_frame(None, &long_msg);
        match &f {
            Frame::Fatal { error, .. } => assert_eq!(error.len(), 10_000),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn unicode_in_event_payload() {
        let ev = json!({"type": "test", "text": "日本語テスト 🎉"});
        let f = event_frame("r1", ev);
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                assert_eq!(event["text"], "日本語テスト 🎉");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn nested_json_in_work_order() {
        let wo = json!({
            "task": "build",
            "config": {
                "nested": {
                    "deeply": {
                        "value": [1, 2, 3]
                    }
                }
            }
        });
        let f = Frame::Run {
            id: "r1".into(),
            work_order: wo.clone(),
        };
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Run { work_order, .. } => {
                assert_eq!(work_order, wo);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn special_characters_in_ref_id() {
        let f = Frame::Event {
            ref_id: "run/2024-01-01/abc#1".into(),
            event: json!({}),
        };
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Event { ref_id, .. } => {
                assert_eq!(ref_id, "run/2024-01-01/abc#1");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn empty_event_object() {
        let f = event_frame("r1", json!({}));
        let encoded = JsonlCodec::encode(&f).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert!(event.as_object().unwrap().is_empty()),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn pipeline_with_no_stages_preserves_event() {
        let pipe = EventPipeline::new();
        let ev = json!({"type": "complex", "nested": {"a": [1,2,3]}});
        let result = pipe.process(ev.clone()).unwrap().unwrap();
        assert_eq!(result, ev);
    }

    #[test]
    fn middleware_chain_multiple_filters() {
        let chain = MiddlewareChain::new()
            .with(FilterMiddleware::exclude_kinds(&["debug"]))
            .with(FilterMiddleware::exclude_kinds(&["trace"]));

        assert!(chain.process(&json!({"type": "error"})).is_some());
        assert!(chain.process(&json!({"type": "debug"})).is_none());
        assert!(chain.process(&json!({"type": "trace"})).is_none());
    }

    #[test]
    fn receipt_builder_chain_all_methods() {
        let r = ReceiptBuilder::new("r1", "be1")
            .failed()
            .event(json!({"type": "error"}))
            .artifact("log", "run.log")
            .usage_raw(json!({}))
            .input_tokens(1)
            .output_tokens(2)
            .build();
        assert_eq!(r["outcome"], "failed");
        assert_eq!(r["trace"].as_array().unwrap().len(), 1);
        assert_eq!(r["artifacts"].as_array().unwrap().len(), 1);
        assert_eq!(r["usage"]["input_tokens"], 1);
        assert_eq!(r["usage"]["output_tokens"], 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Transform module (typed, abp-core based)
// ═══════════════════════════════════════════════════════════════════════

mod transform_tests {
    use super::*;

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    #[test]
    fn redact_transformer_redacts_text_delta() {
        let rt = RedactTransformer::new(vec!["secret123".into()]);
        let ev = make_event(AgentEventKind::AssistantDelta {
            text: "key is secret123 here".into(),
        });
        let result = rt.transform(ev).unwrap();
        match &result.kind {
            AgentEventKind::AssistantDelta { text } => {
                assert!(!text.contains("secret123"));
                assert!(text.contains("[REDACTED]"));
            }
            _ => panic!("expected AssistantDelta"),
        }
    }

    #[test]
    fn redact_transformer_redacts_message() {
        let rt = RedactTransformer::new(vec!["password".into()]);
        let ev = make_event(AgentEventKind::AssistantMessage {
            text: "your password is here".into(),
        });
        let result = rt.transform(ev).unwrap();
        match &result.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains("[REDACTED]"));
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn redact_transformer_redacts_error() {
        let rt = RedactTransformer::new(vec!["api_key_abc".into()]);
        let ev = make_event(AgentEventKind::Error {
            message: "failed with api_key_abc".into(),
            error_code: None,
        });
        let result = rt.transform(ev).unwrap();
        match &result.kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains("[REDACTED]"));
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn redact_transformer_redacts_warning() {
        let rt = RedactTransformer::new(vec!["token".into()]);
        let ev = make_event(AgentEventKind::Warning {
            message: "invalid token".into(),
        });
        let result = rt.transform(ev).unwrap();
        match &result.kind {
            AgentEventKind::Warning { message } => {
                assert!(message.contains("[REDACTED]"));
            }
            _ => panic!("expected Warning"),
        }
    }

    #[test]
    fn redact_transformer_redacts_run_started() {
        let rt = RedactTransformer::new(vec!["secret".into()]);
        let ev = make_event(AgentEventKind::RunStarted {
            message: "run secret start".into(),
        });
        let result = rt.transform(ev).unwrap();
        match &result.kind {
            AgentEventKind::RunStarted { message } => {
                assert!(message.contains("[REDACTED]"));
            }
            _ => panic!("expected RunStarted"),
        }
    }

    #[test]
    fn redact_transformer_redacts_command_executed() {
        let rt = RedactTransformer::new(vec!["pw123".into()]);
        let ev = make_event(AgentEventKind::CommandExecuted {
            command: "echo pw123".into(),
            exit_code: Some(0),
            output_preview: Some("pw123 was used".into()),
        });
        let result = rt.transform(ev).unwrap();
        match &result.kind {
            AgentEventKind::CommandExecuted {
                command,
                output_preview,
                ..
            } => {
                assert!(command.contains("[REDACTED]"));
                assert!(output_preview.as_ref().unwrap().contains("[REDACTED]"));
            }
            _ => panic!("expected CommandExecuted"),
        }
    }

    #[test]
    fn redact_transformer_multiple_patterns() {
        let rt = RedactTransformer::new(vec!["aaa".into(), "bbb".into()]);
        let ev = make_event(AgentEventKind::AssistantDelta {
            text: "aaa and bbb".into(),
        });
        let result = rt.transform(ev).unwrap();
        match &result.kind {
            AgentEventKind::AssistantDelta { text } => {
                assert!(!text.contains("aaa"));
                assert!(!text.contains("bbb"));
            }
            _ => panic!("expected AssistantDelta"),
        }
    }

    #[test]
    fn redact_transformer_name() {
        let rt = RedactTransformer::new(vec![]);
        assert_eq!(rt.name(), "redact");
    }

    #[test]
    fn throttle_transformer_allows_up_to_max() {
        let tt = ThrottleTransformer::new(2);
        let ev1 = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        let ev2 = make_event(AgentEventKind::AssistantDelta { text: "b".into() });
        let ev3 = make_event(AgentEventKind::AssistantDelta { text: "c".into() });

        assert!(tt.transform(ev1).is_some());
        assert!(tt.transform(ev2).is_some());
        assert!(tt.transform(ev3).is_none());
    }

    #[test]
    fn throttle_transformer_independent_per_kind() {
        let tt = ThrottleTransformer::new(1);
        let delta = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        let msg = make_event(AgentEventKind::AssistantMessage { text: "b".into() });
        let delta2 = make_event(AgentEventKind::AssistantDelta { text: "c".into() });

        assert!(tt.transform(delta).is_some());
        assert!(tt.transform(msg).is_some());
        assert!(tt.transform(delta2).is_none()); // second delta exceeds limit
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
        meta.insert("version".into(), "1.0".into());
        let et = EnrichTransformer::new(meta);

        let ev = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        let result = et.transform(ev).unwrap();
        let ext = result.ext.unwrap();
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

        let result = et.transform(ev).unwrap();
        let ext = result.ext.unwrap();
        assert_eq!(ext["old_key"], json!("old_val"));
        assert_eq!(ext["new_key"], json!("new_val"));
    }

    #[test]
    fn enrich_transformer_name() {
        let et = EnrichTransformer::new(BTreeMap::new());
        assert_eq!(et.name(), "enrich");
    }

    #[test]
    fn filter_transformer_passes_matching() {
        let ft = FilterTransformer::new(Box::new(|ev: &AgentEvent| {
            matches!(&ev.kind, AgentEventKind::AssistantDelta { .. })
        }));
        let ev = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        assert!(ft.transform(ev).is_some());
    }

    #[test]
    fn filter_transformer_drops_non_matching() {
        let ft = FilterTransformer::new(Box::new(|ev: &AgentEvent| {
            matches!(&ev.kind, AgentEventKind::AssistantDelta { .. })
        }));
        let ev = make_event(AgentEventKind::Warning {
            message: "warn".into(),
        });
        assert!(ft.transform(ev).is_none());
    }

    #[test]
    fn filter_transformer_name() {
        let ft = FilterTransformer::new(Box::new(|_| true));
        assert_eq!(ft.name(), "filter");
    }

    #[test]
    fn timestamp_transformer_replaces_epoch_zero() {
        let tt = TimestampTransformer::new();
        let ev = AgentEvent {
            ts: Utc.timestamp_opt(0, 0).unwrap(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        };
        let result = tt.transform(ev).unwrap();
        assert!(result.ts.timestamp() > 0);
    }

    #[test]
    fn timestamp_transformer_keeps_valid_ts() {
        let tt = TimestampTransformer::new();
        let now = Utc::now();
        let ev = AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
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
        let ev = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        let result = chain.process(ev);
        assert!(result.is_some());
    }

    #[test]
    fn transformer_chain_processes_in_order() {
        let chain = TransformerChain::new()
            .with(Box::new(RedactTransformer::new(vec!["secret".into()])))
            .with(Box::new(TimestampTransformer::new()));

        let ev = AgentEvent {
            ts: Utc.timestamp_opt(0, 0).unwrap(),
            kind: AgentEventKind::AssistantDelta {
                text: "secret data".into(),
            },
            ext: None,
        };
        let result = chain.process(ev).unwrap();
        match &result.kind {
            AgentEventKind::AssistantDelta { text } => {
                assert!(text.contains("[REDACTED]"));
            }
            _ => panic!("expected AssistantDelta"),
        }
        assert!(result.ts.timestamp() > 0);
    }

    #[test]
    fn transformer_chain_short_circuits() {
        let chain = TransformerChain::new()
            .with(Box::new(ThrottleTransformer::new(0)))
            .with(Box::new(TimestampTransformer::new()));

        let ev = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        assert!(chain.process(ev).is_none());
    }

    #[test]
    fn transformer_chain_process_batch() {
        let chain = TransformerChain::new().with(Box::new(ThrottleTransformer::new(1)));

        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "a".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "b".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "c".into() }),
        ];
        let results = chain.process_batch(events);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn transformer_chain_default() {
        let chain = TransformerChain::default();
        let ev = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        assert!(chain.process(ev).is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Typed middleware system
// ═══════════════════════════════════════════════════════════════════════

mod typed_middleware_tests {
    use super::*;
    use sidecar_kit::{
        MetricsMiddleware, MiddlewareAction, RateLimitMiddleware, SidecarMiddleware,
        SidecarMiddlewareChain, TypedErrorRecoveryMiddleware,
    };

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    #[test]
    fn middleware_action_continue_eq() {
        assert_eq!(MiddlewareAction::Continue, MiddlewareAction::Continue);
    }

    #[test]
    fn middleware_action_skip_eq() {
        assert_eq!(MiddlewareAction::Skip, MiddlewareAction::Skip);
    }

    #[test]
    fn middleware_action_error_eq() {
        assert_eq!(
            MiddlewareAction::Error("x".into()),
            MiddlewareAction::Error("x".into())
        );
    }

    #[test]
    fn middleware_action_ne() {
        assert_ne!(MiddlewareAction::Continue, MiddlewareAction::Skip);
    }

    #[test]
    fn middleware_action_debug() {
        let a = MiddlewareAction::Continue;
        assert!(format!("{a:?}").contains("Continue"));
    }

    #[test]
    fn middleware_action_clone() {
        let a = MiddlewareAction::Error("test".into());
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn metrics_middleware_counts_events() {
        let mw = MetricsMiddleware::new();
        let mut ev = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        let action = mw.on_event(&mut ev);
        assert_eq!(action, MiddlewareAction::Continue);
        assert_eq!(mw.total(), 1);
        assert_eq!(mw.counts()["assistant_delta"], 1);
    }

    #[test]
    fn metrics_middleware_multiple_kinds() {
        let mw = MetricsMiddleware::new();
        let mut delta = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        let mut warn = make_event(AgentEventKind::Warning {
            message: "w".into(),
        });
        mw.on_event(&mut delta);
        mw.on_event(&mut warn);
        assert_eq!(mw.total(), 2);
        assert_eq!(mw.counts()["assistant_delta"], 1);
        assert_eq!(mw.counts()["warning"], 1);
    }

    #[test]
    fn metrics_middleware_timings_recorded() {
        let mw = MetricsMiddleware::new();
        let mut ev = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        mw.on_event(&mut ev);
        assert_eq!(mw.timings().len(), 1);
    }

    #[test]
    fn metrics_middleware_default() {
        let mw = MetricsMiddleware::default();
        assert_eq!(mw.total(), 0);
    }

    #[test]
    fn sidecar_middleware_chain_empty() {
        let chain = SidecarMiddlewareChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn sidecar_middleware_chain_default() {
        let chain = SidecarMiddlewareChain::default();
        assert!(chain.is_empty());
    }

    #[test]
    fn sidecar_middleware_chain_process_continue() {
        let chain = SidecarMiddlewareChain::new().with(MetricsMiddleware::new());
        let mut ev = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        let action = chain.process(&mut ev);
        assert_eq!(action, MiddlewareAction::Continue);
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn sidecar_middleware_chain_push() {
        let mut chain = SidecarMiddlewareChain::new();
        chain.push(MetricsMiddleware::new());
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn typed_filter_middleware_drops_matching() {
        use sidecar_kit::typed_middleware::FilterMiddleware as TypedFilterMiddleware;

        let mw = TypedFilterMiddleware::new(|ev: &AgentEvent| {
            matches!(&ev.kind, AgentEventKind::Warning { .. })
        });
        let mut ev = make_event(AgentEventKind::Warning {
            message: "w".into(),
        });
        assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Skip);
    }

    #[test]
    fn typed_filter_middleware_passes_non_matching() {
        use sidecar_kit::typed_middleware::FilterMiddleware as TypedFilterMiddleware;

        let mw = TypedFilterMiddleware::new(|ev: &AgentEvent| {
            matches!(&ev.kind, AgentEventKind::Warning { .. })
        });
        let mut ev = make_event(AgentEventKind::AssistantDelta { text: "ok".into() });
        assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
    }

    #[test]
    fn rate_limit_middleware_allows_within_limit() {
        let mw = RateLimitMiddleware::new(100);
        let mut ev = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
    }

    #[test]
    fn rate_limit_middleware_skips_over_limit() {
        let mw = RateLimitMiddleware::new(1);
        let mut ev1 = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        let mut ev2 = make_event(AgentEventKind::AssistantDelta { text: "b".into() });
        assert_eq!(mw.on_event(&mut ev1), MiddlewareAction::Continue);
        assert_eq!(mw.on_event(&mut ev2), MiddlewareAction::Skip);
    }

    #[test]
    fn error_recovery_continues_on_normal() {
        let mw = TypedErrorRecoveryMiddleware::wrap(MetricsMiddleware::new());
        let mut ev = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
    }

    #[test]
    fn sidecar_middleware_chain_short_circuits_on_skip() {
        use sidecar_kit::typed_middleware::FilterMiddleware as TypedFilterMiddleware;

        let chain = SidecarMiddlewareChain::new()
            .with(TypedFilterMiddleware::new(|_| true)) // drops all
            .with(MetricsMiddleware::new());

        let mut ev = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
        assert_eq!(chain.process(&mut ev), MiddlewareAction::Skip);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Diagnostics
// ═══════════════════════════════════════════════════════════════════════

mod diagnostics_tests {
    use sidecar_kit::diagnostics::*;

    #[test]
    fn diagnostic_level_ordering() {
        assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
        assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
        assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
    }

    #[test]
    fn diagnostic_level_eq() {
        assert_eq!(DiagnosticLevel::Debug, DiagnosticLevel::Debug);
    }

    #[test]
    fn diagnostic_level_serde_roundtrip() {
        let level = DiagnosticLevel::Warning;
        let json = serde_json::to_string(&level).unwrap();
        let back: DiagnosticLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, level);
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
        c.add_info("SK001", "hello");
        assert_eq!(c.diagnostics().len(), 1);
        assert_eq!(c.diagnostics()[0].code, "SK001");
        assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Info);
    }

    #[test]
    fn diagnostic_collector_add_warning() {
        let mut c = DiagnosticCollector::new();
        c.add_warning("SK002", "caution");
        assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Warning);
    }

    #[test]
    fn diagnostic_collector_add_error() {
        let mut c = DiagnosticCollector::new();
        c.add_error("SK003", "boom");
        assert!(c.has_errors());
        assert_eq!(c.error_count(), 1);
    }

    #[test]
    fn diagnostic_collector_by_level() {
        let mut c = DiagnosticCollector::new();
        c.add_info("I1", "info1");
        c.add_warning("W1", "warn1");
        c.add_error("E1", "err1");
        c.add_info("I2", "info2");

        assert_eq!(c.by_level(DiagnosticLevel::Info).len(), 2);
        assert_eq!(c.by_level(DiagnosticLevel::Warning).len(), 1);
        assert_eq!(c.by_level(DiagnosticLevel::Error).len(), 1);
    }

    #[test]
    fn diagnostic_collector_clear() {
        let mut c = DiagnosticCollector::new();
        c.add_error("E1", "err");
        c.clear();
        assert!(!c.has_errors());
        assert!(c.diagnostics().is_empty());
    }

    #[test]
    fn diagnostic_collector_summary() {
        let mut c = DiagnosticCollector::new();
        c.add_info("I1", "a");
        c.add_warning("W1", "b");
        c.add_error("E1", "c");
        c.add_error("E2", "d");

        let s = c.summary();
        assert_eq!(s.info_count, 1);
        assert_eq!(s.warning_count, 1);
        assert_eq!(s.error_count, 2);
        assert_eq!(s.debug_count, 0);
        assert_eq!(s.total, 4);
    }

    #[test]
    fn diagnostic_summary_default() {
        let s = DiagnosticSummary::default();
        assert_eq!(s.total, 0);
        assert_eq!(s.debug_count, 0);
    }

    #[test]
    fn diagnostic_summary_eq() {
        let s1 = DiagnosticSummary::default();
        let s2 = DiagnosticSummary::default();
        assert_eq!(s1, s2);
    }

    #[test]
    fn diagnostic_serde_roundtrip() {
        let d = Diagnostic {
            level: DiagnosticLevel::Error,
            code: "SK999".into(),
            message: "test error".into(),
            source: Some("test".into()),
            timestamp: "2024-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, "SK999");
        assert_eq!(back.level, DiagnosticLevel::Error);
        assert_eq!(back.source.as_deref(), Some("test"));
    }

    #[test]
    fn sidecar_diagnostics_serde() {
        let sd = SidecarDiagnostics {
            run_id: "r1".into(),
            diagnostics: vec![],
            pipeline_stages: vec!["validate".into()],
            transform_count: 3,
        };
        let json = serde_json::to_string(&sd).unwrap();
        let back: SidecarDiagnostics = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, "r1");
        assert_eq!(back.transform_count, 3);
        assert_eq!(back.pipeline_stages, vec!["validate"]);
    }

    #[test]
    fn diagnostic_collector_add_custom() {
        let mut c = DiagnosticCollector::new();
        c.add(Diagnostic {
            level: DiagnosticLevel::Debug,
            code: "DBG1".into(),
            message: "debug msg".into(),
            source: Some("custom".into()),
            timestamp: "ts".into(),
        });
        assert_eq!(c.by_level(DiagnosticLevel::Debug).len(), 1);
        assert_eq!(c.diagnostics()[0].source.as_deref(), Some("custom"));
    }

    #[test]
    fn diagnostic_collector_clone() {
        let mut c = DiagnosticCollector::new();
        c.add_info("I1", "msg");
        let c2 = c.clone();
        assert_eq!(c2.diagnostics().len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. HelloData
// ═══════════════════════════════════════════════════════════════════════

mod hello_data_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct BackendId {
        id: String,
    }

    #[test]
    fn hello_data_backend_as() {
        let hd = HelloData {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "test-be"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let be: BackendId = hd.backend_as().unwrap();
        assert_eq!(be.id, "test-be");
    }

    #[test]
    fn hello_data_backend_as_invalid() {
        let hd = HelloData {
            contract_version: "abp/v0.1".into(),
            backend: json!("not an object"),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let result: Result<BackendId, _> = hd.backend_as();
        assert!(result.is_err());
    }

    #[test]
    fn hello_data_capabilities_as() {
        #[derive(Debug, Deserialize)]
        struct Caps {
            tools: bool,
        }
        let hd = HelloData {
            contract_version: "abp/v0.1".into(),
            backend: json!({}),
            capabilities: json!({"tools": true}),
            mode: Value::Null,
        };
        let caps: Caps = hd.capabilities_as().unwrap();
        assert!(caps.tools);
    }

    #[test]
    fn hello_data_clone() {
        let hd = HelloData {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "x"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let hd2 = hd.clone();
        assert_eq!(hd.contract_version, hd2.contract_version);
    }

    #[test]
    fn hello_data_debug() {
        let hd = HelloData {
            contract_version: "abp/v0.1".into(),
            backend: json!({}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let debug = format!("{hd:?}");
        assert!(debug.contains("HelloData"));
    }
}
