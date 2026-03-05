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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for sidecar-kit: construction, protocol, and serde.

use serde_json::{Value, json};
use sidecar_kit::{
    CancelToken, Frame, JsonlCodec, ProcessSpec, ReceiptBuilder,
    builders::{
        event_command_executed, event_error, event_file_changed, event_frame, event_run_completed,
        event_run_started, event_text_delta, event_text_message, event_tool_call,
        event_tool_result, event_warning, fatal_frame, hello_frame,
    },
    diagnostics::{Diagnostic, DiagnosticCollector, DiagnosticLevel, DiagnosticSummary},
    pipeline::{EventPipeline, RedactStage, TimestampStage, ValidateStage},
};

// ═══════════════════════════════════════════════════════════════════════
// Module: kit_construction (~8 tests)
// ═══════════════════════════════════════════════════════════════════════
mod kit_construction {
    use super::*;

    #[test]
    fn process_spec_defaults_are_empty() {
        let spec = ProcessSpec::new("my-sidecar");
        assert_eq!(spec.command, "my-sidecar");
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn process_spec_with_args_and_env() {
        let mut spec = ProcessSpec::new("node");
        spec.args = vec!["--harmony".into(), "index.js".into()];
        spec.env.insert("NODE_ENV".into(), "production".into());
        spec.env.insert("PORT".into(), "3000".into());
        spec.cwd = Some("/app".into());

        assert_eq!(spec.args.len(), 2);
        assert_eq!(spec.env.len(), 2);
        assert_eq!(spec.cwd.as_deref(), Some("/app"));
    }

    #[test]
    fn process_spec_clone_is_independent() {
        let mut orig = ProcessSpec::new("python");
        orig.args.push("main.py".into());
        let mut cloned = orig.clone();
        cloned.args.push("--verbose".into());

        assert_eq!(orig.args.len(), 1);
        assert_eq!(cloned.args.len(), 2);
    }

    #[test]
    fn process_spec_env_btreemap_sorted() {
        let mut spec = ProcessSpec::new("cmd");
        spec.env.insert("Z_VAR".into(), "z".into());
        spec.env.insert("A_VAR".into(), "a".into());
        spec.env.insert("M_VAR".into(), "m".into());

        let keys: Vec<&String> = spec.env.keys().collect();
        assert_eq!(keys, vec!["A_VAR", "M_VAR", "Z_VAR"]);
    }

    #[test]
    fn cancel_token_multiple_cancels_idempotent() {
        let token = CancelToken::new();
        token.cancel();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_token_clone_chain() {
        let t1 = CancelToken::new();
        let t2 = t1.clone();
        let t3 = t2.clone();
        assert!(!t3.is_cancelled());
        t1.cancel();
        assert!(t2.is_cancelled());
        assert!(t3.is_cancelled());
    }

    #[test]
    fn receipt_builder_default_outcome_is_complete() {
        let receipt = ReceiptBuilder::new("run-1", "mock").build();
        assert_eq!(receipt["outcome"], "complete");
    }

    #[test]
    fn receipt_builder_chained_configuration() {
        let receipt = ReceiptBuilder::new("run-2", "openai")
            .failed()
            .input_tokens(100)
            .output_tokens(50)
            .usage_raw(json!({"prompt_tokens": 100}))
            .artifact("file", "output.txt")
            .event(json!({"type": "log", "msg": "started"}))
            .build();

        assert_eq!(receipt["outcome"], "failed");
        assert_eq!(receipt["usage"]["input_tokens"], 100);
        assert_eq!(receipt["usage"]["output_tokens"], 50);
        assert_eq!(receipt["usage_raw"]["prompt_tokens"], 100);
        assert_eq!(receipt["artifacts"][0]["kind"], "file");
        assert_eq!(receipt["artifacts"][0]["path"], "output.txt");
        assert_eq!(receipt["trace"][0]["type"], "log");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: kit_protocol (~8 tests)
// ═══════════════════════════════════════════════════════════════════════
mod kit_protocol {
    use super::*;

    #[test]
    fn hello_frame_builder_uses_contract_version() {
        let frame = hello_frame("my-backend");
        match frame {
            Frame::Hello {
                contract_version,
                backend,
                capabilities,
                mode,
            } => {
                assert_eq!(contract_version, "abp/v0.1");
                assert_eq!(backend, json!({"id": "my-backend"}));
                assert_eq!(capabilities, json!({}));
                assert_eq!(mode, Value::Null);
            }
            _ => panic!("expected Hello frame"),
        }
    }

    #[test]
    fn event_frame_builder_wraps_payload() {
        let payload = event_text_delta("hello world");
        let frame = event_frame("run-99", payload.clone());
        match frame {
            Frame::Event { ref_id, event } => {
                assert_eq!(ref_id, "run-99");
                assert_eq!(event["type"], "assistant_delta");
                assert_eq!(event["text"], "hello world");
            }
            _ => panic!("expected Event frame"),
        }
    }

    #[test]
    fn fatal_frame_builder_with_ref_id() {
        let frame = fatal_frame(Some("run-1"), "oom");
        match frame {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id, Some("run-1".to_string()));
                assert_eq!(error, "oom");
            }
            _ => panic!("expected Fatal frame"),
        }
    }

    #[test]
    fn fatal_frame_builder_without_ref_id() {
        let frame = fatal_frame(None, "global crash");
        match frame {
            Frame::Fatal { ref_id, error } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "global crash");
            }
            _ => panic!("expected Fatal frame"),
        }
    }

    #[test]
    fn run_frame_encodes_with_t_tag() {
        let frame = Frame::Run {
            id: "r-abc".into(),
            work_order: json!({"task": "summarize", "input": "text"}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let parsed: Value = serde_json::from_str(encoded.trim()).unwrap();
        assert_eq!(parsed["t"], "run");
        assert_eq!(parsed["id"], "r-abc");
        assert_eq!(parsed["work_order"]["task"], "summarize");
    }

    #[test]
    fn cancel_frame_round_trip_with_reason() {
        let frame = Frame::Cancel {
            ref_id: "run-7".into(),
            reason: Some("timeout exceeded".into()),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Cancel { ref_id, reason } => {
                assert_eq!(ref_id, "run-7");
                assert_eq!(reason.as_deref(), Some("timeout exceeded"));
            }
            _ => panic!("expected Cancel frame"),
        }
    }

    #[test]
    fn cancel_frame_round_trip_no_reason() {
        let frame = Frame::Cancel {
            ref_id: "run-8".into(),
            reason: None,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Cancel { ref_id, reason } => {
                assert_eq!(ref_id, "run-8");
                assert!(reason.is_none());
            }
            _ => panic!("expected Cancel frame"),
        }
    }

    #[test]
    fn codec_decode_rejects_wrong_variant_fields() {
        // Valid JSON with correct tag but missing required fields for Run
        let raw = r#"{"t":"run"}"#;
        let result = JsonlCodec::decode(raw);
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: kit_serde (~6 tests)
// ═══════════════════════════════════════════════════════════════════════
mod kit_serde {
    use super::*;

    #[test]
    fn all_event_builders_produce_valid_json_objects() {
        let builders: Vec<Value> = vec![
            event_text_delta("delta"),
            event_text_message("msg"),
            event_tool_call("read_file", Some("tc-1"), json!({"path": "/tmp"})),
            event_tool_result("read_file", Some("tc-1"), json!("contents"), false),
            event_error("something failed"),
            event_warning("watch out"),
            event_run_started("starting"),
            event_run_completed("done"),
            event_file_changed("src/main.rs", "added fn"),
            event_command_executed("cargo build", Some(0), Some("ok")),
        ];

        for (i, val) in builders.iter().enumerate() {
            assert!(val.is_object(), "builder {i} did not produce a JSON object");
            assert!(val.get("ts").is_some(), "builder {i} missing 'ts' field");
            assert!(
                val.get("type").is_some(),
                "builder {i} missing 'type' field"
            );
        }
    }

    #[test]
    fn event_builders_type_field_values() {
        assert_eq!(event_text_delta("x")["type"], "assistant_delta");
        assert_eq!(event_text_message("x")["type"], "assistant_message");
        assert_eq!(event_error("x")["type"], "error");
        assert_eq!(event_warning("x")["type"], "warning");
        assert_eq!(event_run_started("x")["type"], "run_started");
        assert_eq!(event_run_completed("x")["type"], "run_completed");
        assert_eq!(event_file_changed("p", "s")["type"], "file_changed");
        assert_eq!(
            event_command_executed("c", None, None)["type"],
            "command_executed"
        );
    }

    #[test]
    fn tool_call_event_optional_fields() {
        let with_id = event_tool_call("bash", Some("tc-5"), json!({}));
        assert_eq!(with_id["tool_use_id"], "tc-5");

        let without_id = event_tool_call("bash", None, json!({}));
        assert!(without_id["tool_use_id"].is_null());
    }

    #[test]
    fn tool_result_is_error_flag() {
        let ok = event_tool_result("bash", None, json!("output"), false);
        assert_eq!(ok["is_error"], false);

        let err = event_tool_result("bash", None, json!("fail"), true);
        assert_eq!(err["is_error"], true);
    }

    #[test]
    fn receipt_builder_meta_fields() {
        let receipt = ReceiptBuilder::new("run-abc", "claude").build();
        assert_eq!(receipt["meta"]["run_id"], "run-abc");
        assert_eq!(receipt["meta"]["work_order_id"], "run-abc");
        assert_eq!(receipt["meta"]["contract_version"], "abp/v0.1");
        assert!(receipt["meta"]["started_at"].is_string());
        assert!(receipt["meta"]["finished_at"].is_string());
        assert_eq!(receipt["backend"]["id"], "claude");
        assert_eq!(receipt["mode"], "mapped");
        assert!(receipt["receipt_sha256"].is_null());
    }

    #[test]
    fn receipt_builder_partial_outcome() {
        let receipt = ReceiptBuilder::new("r-1", "mock").partial().build();
        assert_eq!(receipt["outcome"], "partial");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Bonus: diagnostics & pipeline integration tests
// ═══════════════════════════════════════════════════════════════════════
mod kit_diagnostics {
    use super::*;

    #[test]
    fn diagnostic_collector_summary_counts() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("SK001", "started");
        collector.add_info("SK002", "running");
        collector.add_warning("SK003", "slow");
        collector.add_error("SK004", "failed");
        collector.add_error("SK005", "crash");

        let summary = collector.summary();
        assert_eq!(summary.info_count, 2);
        assert_eq!(summary.warning_count, 1);
        assert_eq!(summary.error_count, 2);
        assert_eq!(summary.total, 5);
        assert!(collector.has_errors());
        assert_eq!(collector.error_count(), 2);
    }

    #[test]
    fn diagnostic_collector_by_level() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("I1", "info");
        collector.add_warning("W1", "warn");
        collector.add_error("E1", "err");

        let warnings = collector.by_level(DiagnosticLevel::Warning);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "W1");
    }

    #[test]
    fn diagnostic_collector_clear_resets() {
        let mut collector = DiagnosticCollector::new();
        collector.add_error("E1", "error");
        assert!(collector.has_errors());

        collector.clear();
        assert!(!collector.has_errors());
        assert_eq!(collector.diagnostics().len(), 0);
    }

    #[test]
    fn diagnostic_level_ordering() {
        assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
        assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
        assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
    }

    #[test]
    fn diagnostic_serde_round_trip() {
        let diag = Diagnostic {
            level: DiagnosticLevel::Warning,
            code: "SK100".to_string(),
            message: "something happened".to_string(),
            source: Some("pipeline".to_string()),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
        };
        let json_str = serde_json::to_string(&diag).unwrap();
        let parsed: Diagnostic = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.level, DiagnosticLevel::Warning);
        assert_eq!(parsed.code, "SK100");
        assert_eq!(parsed.message, "something happened");
        assert_eq!(parsed.source.as_deref(), Some("pipeline"));
    }

    #[test]
    fn diagnostic_summary_default_is_zeroed() {
        let summary = DiagnosticSummary::default();
        assert_eq!(summary.debug_count, 0);
        assert_eq!(summary.info_count, 0);
        assert_eq!(summary.warning_count, 0);
        assert_eq!(summary.error_count, 0);
        assert_eq!(summary.total, 0);
    }
}

mod kit_pipeline {
    use super::*;

    #[test]
    fn empty_pipeline_passes_through() {
        let pipeline = EventPipeline::new();
        assert_eq!(pipeline.stage_count(), 0);
        let event = json!({"type": "test", "data": 42});
        let result = pipeline.process(event.clone()).unwrap();
        assert_eq!(result, Some(event));
    }

    #[test]
    fn validate_stage_rejects_missing_fields() {
        let mut pipeline = EventPipeline::new();
        pipeline.add_stage(Box::new(ValidateStage::new(vec![
            "type".to_string(),
            "ts".to_string(),
        ])));

        let event = json!({"type": "test"});
        let result = pipeline.process(event);
        assert!(result.is_err());
    }

    #[test]
    fn redact_stage_removes_fields() {
        let mut pipeline = EventPipeline::new();
        pipeline.add_stage(Box::new(RedactStage::new(vec![
            "secret".to_string(),
            "token".to_string(),
        ])));

        let event = json!({"type": "test", "secret": "abc", "token": "xyz", "keep": true});
        let result = pipeline.process(event).unwrap().unwrap();
        assert!(result.get("secret").is_none());
        assert!(result.get("token").is_none());
        assert_eq!(result["keep"], true);
    }

    #[test]
    fn timestamp_stage_adds_processed_at() {
        let mut pipeline = EventPipeline::new();
        pipeline.add_stage(Box::new(TimestampStage::new()));

        let event = json!({"type": "test"});
        let result = pipeline.process(event).unwrap().unwrap();
        assert!(result.get("processed_at").is_some());
    }

    #[test]
    fn pipeline_non_object_event_fails_validation() {
        let mut pipeline = EventPipeline::new();
        pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".to_string()])));

        let result = pipeline.process(json!("not an object"));
        assert!(result.is_err());
    }

    #[test]
    fn pipeline_multi_stage_composition() {
        let mut pipeline = EventPipeline::new();
        pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".to_string()])));
        pipeline.add_stage(Box::new(RedactStage::new(vec!["password".to_string()])));
        pipeline.add_stage(Box::new(TimestampStage::new()));

        assert_eq!(pipeline.stage_count(), 3);

        let event = json!({"type": "login", "password": "secret123", "user": "alice"});
        let result = pipeline.process(event).unwrap().unwrap();
        assert!(result.get("password").is_none());
        assert_eq!(result["user"], "alice");
        assert!(result.get("processed_at").is_some());
    }
}
