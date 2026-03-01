// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended unit tests for sidecar-kit pure Rust code paths.

use serde_json::{Value, json};
use sidecar_kit::{
    CancelToken, EventMiddleware, EventPipeline, FilterMiddleware, Frame, JsonlCodec,
    LoggingMiddleware, MiddlewareChain, PipelineError, PipelineStage, ProcessSpec, RedactStage,
    SidecarError, TimestampStage, ValidateStage,
};
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// 1. ProcessSpec construction and validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_spec_defaults_are_empty() {
    let spec = ProcessSpec::new("bash");
    assert_eq!(spec.command, "bash");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn process_spec_accepts_string_and_str() {
    let from_str = ProcessSpec::new("node");
    let from_string = ProcessSpec::new(String::from("node"));
    assert_eq!(from_str.command, from_string.command);
}

#[test]
fn process_spec_with_args_and_cwd() {
    let mut spec = ProcessSpec::new("python3");
    spec.args = vec!["-u".into(), "script.py".into()];
    spec.cwd = Some("/workspace".into());
    assert_eq!(spec.args.len(), 2);
    assert_eq!(spec.cwd.as_deref(), Some("/workspace"));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Environment variable merging
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn env_vars_btreemap_is_sorted() {
    let mut spec = ProcessSpec::new("sh");
    spec.env.insert("Z_VAR".into(), "z".into());
    spec.env.insert("A_VAR".into(), "a".into());
    spec.env.insert("M_VAR".into(), "m".into());
    let keys: Vec<&String> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A_VAR", "M_VAR", "Z_VAR"]);
}

#[test]
fn env_var_overwrite_replaces_value() {
    let mut spec = ProcessSpec::new("sh");
    spec.env.insert("KEY".into(), "old".into());
    spec.env.insert("KEY".into(), "new".into());
    assert_eq!(spec.env.get("KEY").unwrap(), "new");
    assert_eq!(spec.env.len(), 1);
}

#[test]
fn env_merge_two_specs() {
    let mut base = ProcessSpec::new("node");
    base.env.insert("NODE_ENV".into(), "production".into());
    base.env.insert("PORT".into(), "3000".into());

    let mut overlay = BTreeMap::new();
    overlay.insert("PORT".into(), "8080".into());
    overlay.insert("DEBUG".into(), "true".into());

    base.env.extend(overlay);
    assert_eq!(base.env["NODE_ENV"], "production");
    assert_eq!(base.env["PORT"], "8080"); // overwritten
    assert_eq!(base.env["DEBUG"], "true"); // new
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Command argument building
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn args_preserve_order() {
    let mut spec = ProcessSpec::new("cargo");
    spec.args = vec!["test".into(), "--".into(), "--nocapture".into()];
    assert_eq!(spec.args[0], "test");
    assert_eq!(spec.args[1], "--");
    assert_eq!(spec.args[2], "--nocapture");
}

#[test]
fn args_with_spaces_and_special_chars() {
    let mut spec = ProcessSpec::new("echo");
    spec.args = vec!["hello world".into(), "--flag=value with spaces".into()];
    assert_eq!(spec.args[0], "hello world");
    assert_eq!(spec.args[1], "--flag=value with spaces");
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Backend identity construction (via Frame::Hello)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_frame_with_full_backend_identity() {
    let backend = json!({
        "id": "my-sidecar",
        "backend_version": "2.0.0",
        "adapter_version": "1.0.0"
    });
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: backend.clone(),
        capabilities: json!({}),
        mode: json!("mapped"),
    };
    match &frame {
        Frame::Hello { backend: b, .. } => {
            assert_eq!(b["id"], "my-sidecar");
            assert_eq!(b["backend_version"], "2.0.0");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_frame_minimal_backend() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "minimal"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Hello { backend, .. } => {
            assert_eq!(backend["id"], "minimal");
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Capability declaration (via hello frame)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capabilities_as_rich_object() {
    let caps = json!({
        "streaming": "native",
        "tool_read": "emulated",
        "tool_write": "unsupported"
    });
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: caps.clone(),
        mode: json!("mapped"),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Hello { capabilities, .. } => {
            assert_eq!(capabilities["streaming"], "native");
            assert_eq!(capabilities["tool_write"], "unsupported");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn empty_capabilities_round_trips() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: json!("mapped"),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Hello { capabilities, .. } => {
            assert!(capabilities.as_object().unwrap().is_empty());
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Serde roundtrip for all Frame types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_frame_variants_serde_roundtrip() {
    let frames: Vec<Frame> = vec![
        Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "rt"}),
            capabilities: json!({"streaming": "native"}),
            mode: json!("passthrough"),
        },
        Frame::Run {
            id: "run-abc".into(),
            work_order: json!({"task": "build", "params": [1, 2, 3]}),
        },
        Frame::Event {
            ref_id: "run-abc".into(),
            event: json!({"type": "assistant_delta", "text": "hi"}),
        },
        Frame::Final {
            ref_id: "run-abc".into(),
            receipt: json!({"outcome": "complete"}),
        },
        Frame::Fatal {
            ref_id: Some("run-abc".into()),
            error: "oom".into(),
        },
        Frame::Fatal {
            ref_id: None,
            error: "global error".into(),
        },
        Frame::Cancel {
            ref_id: "run-abc".into(),
            reason: Some("timeout".into()),
        },
        Frame::Ping { seq: 42 },
        Frame::Pong { seq: 42 },
    ];

    for frame in &frames {
        let encoded = JsonlCodec::encode(frame).unwrap();
        assert!(
            encoded.ends_with('\n'),
            "encoded line must end with newline"
        );
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        // Verify discriminator tag
        let v: Value = serde_json::to_value(&decoded).unwrap();
        assert!(v.get("t").is_some(), "all frames must have 't' tag");
    }
}

#[test]
fn frame_serde_preserves_nested_json_types() {
    let event = json!({
        "string_val": "hello",
        "int_val": 42,
        "float_val": 2.72,
        "bool_val": true,
        "null_val": null,
        "array_val": [1, "two", null],
        "object_val": {"a": 1}
    });
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: event.clone(),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Event {
            event: decoded_event,
            ..
        } => {
            assert_eq!(decoded_event, event);
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. CancelToken extended tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancel_token_multiple_cancels_are_idempotent() {
    let token = CancelToken::new();
    token.cancel();
    token.cancel();
    token.cancel();
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn cancel_token_multiple_clones_all_see_cancel() {
    let t1 = CancelToken::new();
    let t2 = t1.clone();
    let t3 = t1.clone();
    t2.cancel();
    assert!(t1.is_cancelled());
    assert!(t3.is_cancelled());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Middleware chain tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn middleware_chain_empty_is_passthrough() {
    let chain = MiddlewareChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    let event = json!({"type": "test", "data": 1});
    let result = chain.process(&event);
    assert_eq!(result, Some(event));
}

#[test]
fn middleware_chain_with_builder() {
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware::new())
        .with(FilterMiddleware::include_kinds(&["assistant_delta"]));
    assert_eq!(chain.len(), 2);
}

#[test]
fn filter_middleware_include_passes_matching() {
    let filter = FilterMiddleware::include_kinds(&["assistant_delta", "run_started"]);
    let event = json!({"type": "assistant_delta", "text": "hi"});
    assert!(filter.process(&event).is_some());
}

#[test]
fn filter_middleware_include_drops_non_matching() {
    let filter = FilterMiddleware::include_kinds(&["assistant_delta"]);
    let event = json!({"type": "error", "message": "bad"});
    assert!(filter.process(&event).is_none());
}

#[test]
fn filter_middleware_exclude_drops_matching() {
    let filter = FilterMiddleware::exclude_kinds(&["error", "warning"]);
    let event = json!({"type": "error", "message": "fail"});
    assert!(filter.process(&event).is_none());
}

#[test]
fn filter_middleware_exclude_passes_non_matching() {
    let filter = FilterMiddleware::exclude_kinds(&["error"]);
    let event = json!({"type": "assistant_delta", "text": "ok"});
    assert!(filter.process(&event).is_some());
}

#[test]
fn filter_middleware_is_case_insensitive() {
    let filter = FilterMiddleware::include_kinds(&["assistant_delta"]);
    let event = json!({"type": "ASSISTANT_DELTA", "text": "hi"});
    assert!(filter.process(&event).is_some());
}

#[test]
fn middleware_chain_short_circuits_on_none() {
    let chain = MiddlewareChain::new()
        .with(FilterMiddleware::include_kinds(&["keep_me"]))
        .with(LoggingMiddleware::new()); // would process if reached
    let event = json!({"type": "drop_me"});
    assert!(chain.process(&event).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Pipeline stage tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_empty_is_passthrough() {
    let pipeline = EventPipeline::new();
    assert_eq!(pipeline.stage_count(), 0);
    let event = json!({"type": "test"});
    let result = pipeline.process(event.clone()).unwrap();
    assert_eq!(result, Some(event));
}

#[test]
fn timestamp_stage_adds_processed_at() {
    let stage = TimestampStage::new();
    let event = json!({"type": "test"});
    let result = stage.process(event).unwrap().unwrap();
    assert!(result.get("processed_at").is_some());
}

#[test]
fn redact_stage_removes_fields() {
    let stage = RedactStage::new(vec!["secret".into(), "token".into()]);
    let event = json!({"type": "test", "secret": "abc", "token": "xyz", "keep": true});
    let result = stage.process(event).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert!(result.get("token").is_none());
    assert_eq!(result["keep"], true);
}

#[test]
fn validate_stage_passes_when_all_fields_present() {
    let stage = ValidateStage::new(vec!["type".into(), "data".into()]);
    let event = json!({"type": "test", "data": 42});
    let result = stage.process(event).unwrap();
    assert!(result.is_some());
}

#[test]
fn validate_stage_errors_on_missing_field() {
    let stage = ValidateStage::new(vec!["type".into(), "required_field".into()]);
    let event = json!({"type": "test"});
    let result = stage.process(event);
    assert!(result.is_err());
    match result.unwrap_err() {
        PipelineError::StageError {
            stage: name,
            message,
        } => {
            assert_eq!(name, "validate");
            assert!(message.contains("required_field"));
        }
        other => panic!("expected StageError, got {other:?}"),
    }
}

#[test]
fn pipeline_non_object_returns_invalid_event() {
    let stage = TimestampStage::new();
    let result = stage.process(json!("not an object"));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), PipelineError::InvalidEvent));
}

#[test]
fn pipeline_chained_stages_execute_in_order() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
    pipeline.add_stage(Box::new(RedactStage::new(vec!["sensitive".into()])));
    pipeline.add_stage(Box::new(TimestampStage::new()));
    assert_eq!(pipeline.stage_count(), 3);

    let event = json!({"type": "test", "sensitive": "secret", "data": 1});
    let result = pipeline.process(event).unwrap().unwrap();
    assert!(result.get("sensitive").is_none());
    assert!(result.get("processed_at").is_some());
    assert_eq!(result["data"], 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Error display tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_error_variants_have_display() {
    let errors: Vec<SidecarError> = vec![
        SidecarError::Protocol("test".into()),
        SidecarError::Fatal("crash".into()),
        SidecarError::Timeout,
        SidecarError::Exited(Some(1)),
        SidecarError::Exited(None),
    ];
    for e in &errors {
        let display = e.to_string();
        assert!(!display.is_empty(), "error display should not be empty");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Frame::try_event and Frame::try_final typed extraction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn try_event_with_custom_type() {
    #[derive(serde::Deserialize, Debug)]
    struct MyEvent {
        count: u32,
        label: String,
    }
    let frame = Frame::Event {
        ref_id: "run-1".into(),
        event: json!({"count": 10, "label": "progress"}),
    };
    let (ref_id, my_event): (String, MyEvent) = frame.try_event().unwrap();
    assert_eq!(ref_id, "run-1");
    assert_eq!(my_event.count, 10);
    assert_eq!(my_event.label, "progress");
}

#[test]
fn try_final_with_custom_type() {
    #[derive(serde::Deserialize, Debug)]
    struct MyReceipt {
        status: String,
    }
    let frame = Frame::Final {
        ref_id: "run-1".into(),
        receipt: json!({"status": "done"}),
    };
    let (ref_id, receipt): (String, MyReceipt) = frame.try_final().unwrap();
    assert_eq!(ref_id, "run-1");
    assert_eq!(receipt.status, "done");
}

#[test]
fn try_event_type_mismatch_returns_error() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"wrong": "shape"}),
    };
    #[derive(serde::Deserialize)]
    struct Strict {
        #[allow(dead_code)]
        required_field: String,
    }
    let result: Result<(String, Strict), _> = frame.try_event();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Unicode handling in frames
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn frame_with_unicode_content() {
    let frame = Frame::Event {
        ref_id: "run-日本語".into(),
        event: json!({
            "message": "こんにちは世界 🌍 émojis 中文",
            "path": "src/数据.rs"
        }),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-日本語");
            assert_eq!(event["message"], "こんにちは世界 🌍 émojis 中文");
            assert_eq!(event["path"], "src/数据.rs");
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Large payload handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn frame_large_payload_round_trips() {
    let large_text = "x".repeat(100_000);
    let frame = Frame::Event {
        ref_id: "run-big".into(),
        event: json!({"data": large_text}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Event { event, .. } => {
            assert_eq!(event["data"].as_str().unwrap().len(), 100_000);
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Diagnostics collector
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn diagnostics_collector_summary() {
    use sidecar_kit::diagnostics::{DiagnosticCollector, DiagnosticLevel};
    let mut collector = DiagnosticCollector::new();
    collector.add_info("SK001", "started");
    collector.add_info("SK002", "processing");
    collector.add_warning("SK100", "slow");
    collector.add_error("SK200", "failed");
    collector.add_error("SK201", "also failed");

    let summary = collector.summary();
    assert_eq!(summary.info_count, 2);
    assert_eq!(summary.warning_count, 1);
    assert_eq!(summary.error_count, 2);
    assert_eq!(summary.total, 5);
    assert!(collector.has_errors());
    assert_eq!(collector.error_count(), 2);

    let warnings = collector.by_level(DiagnosticLevel::Warning);
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "SK100");
}

#[test]
fn diagnostics_collector_clear() {
    use sidecar_kit::diagnostics::DiagnosticCollector;
    let mut collector = DiagnosticCollector::new();
    collector.add_error("E1", "err");
    assert!(collector.has_errors());
    collector.clear();
    assert!(!collector.has_errors());
    assert_eq!(collector.diagnostics().len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Transformer tests (abp-core typed)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn enrich_transformer_adds_metadata() {
    use abp_core::{AgentEvent, AgentEventKind};
    use sidecar_kit::transform::{EnrichTransformer, EventTransformer};

    let mut metadata = BTreeMap::new();
    metadata.insert("env".into(), "test".into());
    metadata.insert("version".into(), "1.0".into());

    let transformer = EnrichTransformer::new(metadata);
    assert_eq!(transformer.name(), "enrich");

    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };

    let result = transformer.transform(event).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext["env"], serde_json::Value::String("test".into()));
    assert_eq!(ext["version"], serde_json::Value::String("1.0".into()));
}

#[test]
fn throttle_transformer_limits_per_kind() {
    use abp_core::{AgentEvent, AgentEventKind};
    use sidecar_kit::transform::{EventTransformer, ThrottleTransformer};

    let throttle = ThrottleTransformer::new(2);
    assert_eq!(throttle.name(), "throttle");

    let make_delta = || AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };

    assert!(throttle.transform(make_delta()).is_some()); // 1st
    assert!(throttle.transform(make_delta()).is_some()); // 2nd
    assert!(throttle.transform(make_delta()).is_none()); // 3rd → filtered
}

#[test]
fn redact_transformer_replaces_patterns() {
    use abp_core::{AgentEvent, AgentEventKind};
    use sidecar_kit::transform::{EventTransformer, RedactTransformer};

    let redactor = RedactTransformer::new(vec!["SECRET_KEY".into(), "password123".into()]);
    assert_eq!(redactor.name(), "redact");

    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Use SECRET_KEY and password123 to login".into(),
        },
        ext: None,
    };

    let result = redactor.transform(event).unwrap();
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(text.contains("[REDACTED]"));
            assert!(!text.contains("SECRET_KEY"));
            assert!(!text.contains("password123"));
        }
        _ => panic!("expected AssistantMessage"),
    }
}

#[test]
fn transformer_chain_processes_in_order() {
    use abp_core::{AgentEvent, AgentEventKind};
    use sidecar_kit::TransformerChain;
    use sidecar_kit::transform::{EnrichTransformer, TimestampTransformer};

    let mut metadata = BTreeMap::new();
    metadata.insert("source".into(), "test".into());

    let chain = TransformerChain::new()
        .with(Box::new(TimestampTransformer::new()))
        .with(Box::new(EnrichTransformer::new(metadata)));

    let event = AgentEvent {
        ts: chrono::DateTime::UNIX_EPOCH,
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: None,
    };

    let result = chain.process(event).unwrap();
    // Timestamp should have been updated from epoch
    assert!(result.ts > chrono::DateTime::UNIX_EPOCH);
    // Enrichment should have added ext
    let ext = result.ext.unwrap();
    assert_eq!(ext["source"], serde_json::Value::String("test".into()));
}

#[test]
fn transformer_chain_batch_processes_all() {
    use abp_core::{AgentEvent, AgentEventKind};
    use sidecar_kit::TransformerChain;
    use sidecar_kit::transform::TimestampTransformer;

    let chain = TransformerChain::new().with(Box::new(TimestampTransformer::new()));

    let events: Vec<AgentEvent> = (0..5)
        .map(|i| AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("tok-{i}"),
            },
            ext: None,
        })
        .collect();

    let results = chain.process_batch(events);
    assert_eq!(results.len(), 5);
}

// ═══════════════════════════════════════════════════════════════════════
// 16. SidecarDiagnostics serde roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_diagnostics_serde_roundtrip() {
    use sidecar_kit::diagnostics::{Diagnostic, DiagnosticLevel, SidecarDiagnostics};

    let diags = SidecarDiagnostics {
        run_id: "run-1".into(),
        diagnostics: vec![Diagnostic {
            level: DiagnosticLevel::Info,
            code: "SK001".into(),
            message: "test".into(),
            source: Some("unit_test".into()),
            timestamp: "2024-01-01T00:00:00Z".into(),
        }],
        pipeline_stages: vec!["validate".into(), "timestamp".into()],
        transform_count: 3,
    };

    let json = serde_json::to_string(&diags).unwrap();
    let deserialized: SidecarDiagnostics = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.run_id, "run-1");
    assert_eq!(deserialized.diagnostics.len(), 1);
    assert_eq!(deserialized.pipeline_stages.len(), 2);
    assert_eq!(deserialized.transform_count, 3);
}

// ═══════════════════════════════════════════════════════════════════════
// 17. DiagnosticLevel ordering and serde
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_level_ordering() {
    use sidecar_kit::diagnostics::DiagnosticLevel;
    assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
    assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
    assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
}

#[test]
fn diagnostic_level_serde_roundtrip() {
    use sidecar_kit::diagnostics::DiagnosticLevel;
    let levels = vec![
        DiagnosticLevel::Debug,
        DiagnosticLevel::Info,
        DiagnosticLevel::Warning,
        DiagnosticLevel::Error,
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let deserialized: DiagnosticLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, level);
    }
}
