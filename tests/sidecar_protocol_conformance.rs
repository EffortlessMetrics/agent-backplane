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
//! Sidecar protocol conformance tests.
//!
//! Validates the JSONL protocol lifecycle, envelope ordering, field
//! semantics, error handling, and edge cases across the full
//! hello → run → event* → final/fatal flow.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, ReceiptBuilder, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_protocol::{
    Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version,
    validate::{EnvelopeValidator, SequenceError, ValidationError},
};
use chrono::Utc;

// =========================================================================
// Helpers
// =========================================================================

fn backend() -> BackendIdentity {
    BackendIdentity {
        id: "conformance-sidecar".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: Some("0.2.0".into()),
    }
}

fn caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    m
}

fn work_order() -> WorkOrder {
    WorkOrderBuilder::new("conformance task").build()
}

fn receipt() -> Receipt {
    ReceiptBuilder::new("conformance-sidecar")
        .outcome(Outcome::Complete)
        .build()
}

fn hello() -> Envelope {
    Envelope::hello(backend(), caps())
}

fn run_env(wo: &WorkOrder) -> Envelope {
    Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo.clone(),
    }
}

fn event_env(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: receipt(),
    }
}

fn fatal_env(ref_id: Option<&str>, msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(Into::into),
        error: msg.into(),
        error_code: None,
    }
}

/// Encode then decode, returning the decoded envelope.
fn roundtrip(env: &Envelope) -> Envelope {
    let json = JsonlCodec::encode(env).unwrap();
    JsonlCodec::decode(json.trim()).unwrap()
}

/// Encode to serde_json::Value for field inspection.
fn to_value(env: &Envelope) -> serde_json::Value {
    let json = JsonlCodec::encode(env).unwrap();
    serde_json::from_str(json.trim()).unwrap()
}

// =========================================================================
// 1. Hello handshake (tests 1-6)
// =========================================================================

#[test]
fn hello_roundtrip_preserves_variant() {
    let decoded = roundtrip(&hello());
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_must_be_first_in_sequence() {
    let wo = work_order();
    let seq = vec![
        hello(),
        run_env(&wo),
        event_env(
            &wo.id.to_string(),
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        final_env(&wo.id.to_string()),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "valid sequence should have no errors: {errors:?}"
    );
}

#[test]
fn hello_preserves_backend_identity() {
    let v = to_value(&hello());
    assert_eq!(v["backend"]["id"], "conformance-sidecar");
    assert_eq!(v["backend"]["backend_version"], "2.0.0");
    assert_eq!(v["backend"]["adapter_version"], "0.2.0");
}

#[test]
fn hello_discriminator_field_is_t() {
    let v = to_value(&hello());
    assert_eq!(v["t"], "hello");
    // Must not use "type" as top-level discriminator
    assert!(v.get("type").is_none());
}

#[test]
fn hello_default_mode_is_mapped() {
    let v = to_value(&hello());
    assert_eq!(v["mode"], "mapped");
}

#[test]
fn hello_passthrough_mode_roundtrips() {
    let env = Envelope::hello_with_mode(backend(), caps(), ExecutionMode::Passthrough);
    let v = to_value(&env);
    assert_eq!(v["mode"], "passthrough");
    let decoded = roundtrip(&env);
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

// =========================================================================
// 2. Contract version (tests 7-10)
// =========================================================================

#[test]
fn hello_contains_correct_contract_version() {
    let v = to_value(&hello());
    assert_eq!(v["contract_version"], CONTRACT_VERSION);
}

#[test]
fn contract_version_format_is_parseable() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}

#[test]
fn contract_version_compatible_with_self() {
    assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
}

#[test]
fn contract_version_incompatible_across_major() {
    assert!(!is_compatible_version("abp/v1.0", CONTRACT_VERSION));
}

// =========================================================================
// 3. Run envelope (tests 11-13)
// =========================================================================

#[test]
fn run_envelope_contains_work_order() {
    let wo = work_order();
    let v = to_value(&run_env(&wo));
    assert_eq!(v["t"], "run");
    assert_eq!(v["id"], wo.id.to_string());
    assert_eq!(v["work_order"]["task"], "conformance task");
}

#[test]
fn run_envelope_roundtrips_work_order_id() {
    let wo = work_order();
    let decoded = roundtrip(&run_env(&wo));
    if let Envelope::Run {
        id,
        work_order: decoded_wo,
    } = decoded
    {
        assert_eq!(id, wo.id.to_string());
        assert_eq!(decoded_wo.id, wo.id);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_envelope_preserves_task_field() {
    let wo = WorkOrderBuilder::new("important task with special chars: <>&\"").build();
    let decoded = roundtrip(&run_env(&wo));
    if let Envelope::Run {
        work_order: dwo, ..
    } = decoded
    {
        assert_eq!(dwo.task, "important task with special chars: <>&\"");
    } else {
        panic!("expected Run");
    }
}

// =========================================================================
// 4. Event streaming (tests 14-18)
// =========================================================================

#[test]
fn event_envelope_carries_ref_id() {
    let env = event_env(
        "run-42",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let v = to_value(&env);
    assert_eq!(v["ref_id"], "run-42");
    assert_eq!(v["t"], "event");
}

#[test]
fn event_run_started_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_assistant_delta_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::AssistantDelta {
            text: "Hello ".into(),
        },
    );
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantDelta { text } = &event.kind {
            assert_eq!(text, "Hello ");
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_call_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
    );
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } = &event.kind
        {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("tu_001"));
            assert_eq!(input["path"], "src/main.rs");
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            output: serde_json::json!("file contents here"),
            is_error: false,
        },
    );
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolResult { is_error, .. } = &event.kind {
            assert!(!is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

// =========================================================================
// 5. Final envelope (tests 19-21)
// =========================================================================

#[test]
fn final_envelope_contains_receipt() {
    let env = final_env("run-1");
    let v = to_value(&env);
    assert_eq!(v["t"], "final");
    assert_eq!(v["ref_id"], "run-1");
    assert!(v.get("receipt").is_some());
}

#[test]
fn final_envelope_receipt_has_outcome() {
    let env = final_env("run-1");
    let v = to_value(&env);
    assert_eq!(v["receipt"]["outcome"], "complete");
}

#[test]
fn final_envelope_roundtrip_preserves_receipt() {
    let env = final_env("run-1");
    let decoded = roundtrip(&env);
    if let Envelope::Final { ref_id, receipt: r } = decoded {
        assert_eq!(ref_id, "run-1");
        assert_eq!(r.outcome, Outcome::Complete);
        assert_eq!(r.backend.id, "conformance-sidecar");
    } else {
        panic!("expected Final");
    }
}

// =========================================================================
// 6. Fatal envelope (tests 22-24)
// =========================================================================

#[test]
fn fatal_envelope_with_ref_id() {
    let env = fatal_env(Some("run-1"), "out of memory");
    let v = to_value(&env);
    assert_eq!(v["t"], "fatal");
    assert_eq!(v["ref_id"], "run-1");
    assert_eq!(v["error"], "out of memory");
}

#[test]
fn fatal_envelope_without_ref_id() {
    let env = fatal_env(None, "startup crash");
    let v = to_value(&env);
    assert!(v["ref_id"].is_null());
    assert_eq!(v["error"], "startup crash");
}

#[test]
fn fatal_envelope_roundtrip() {
    let env = fatal_env(Some("r99"), "timeout");
    let decoded = roundtrip(&env);
    if let Envelope::Fatal { ref_id, error, .. } = decoded {
        assert_eq!(ref_id.as_deref(), Some("r99"));
        assert_eq!(error, "timeout");
    } else {
        panic!("expected Fatal");
    }
}

// =========================================================================
// 7. Envelope ordering: hello → run → event* → final/fatal (tests 25-27)
// =========================================================================

#[test]
fn valid_sequence_hello_run_events_final() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![
        hello(),
        run_env(&wo),
        event_env(
            &rid,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        event_env(
            &rid,
            AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
        ),
        event_env(
            &rid,
            AgentEventKind::RunCompleted {
                message: "ok".into(),
            },
        ),
        final_env(&rid),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(errors.is_empty(), "should be valid: {errors:?}");
}

#[test]
fn valid_sequence_hello_run_fatal() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![
        hello(),
        run_env(&wo),
        fatal_env(Some(&rid), "backend error"),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(errors.is_empty(), "should be valid: {errors:?}");
}

#[test]
fn sequence_without_hello_is_invalid() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![run_env(&wo), final_env(&rid)];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

// =========================================================================
// 8. Invalid ordering (tests 28-30)
// =========================================================================

#[test]
fn event_before_run_is_out_of_order() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![
        hello(),
        event_env(
            &rid,
            AgentEventKind::RunStarted {
                message: "too early".into(),
            },
        ),
        run_env(&wo),
        final_env(&rid),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

#[test]
fn hello_not_first_is_detected() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![run_env(&wo), hello(), final_env(&rid)];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
    );
}

#[test]
fn multiple_terminals_is_invalid() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![
        hello(),
        run_env(&wo),
        final_env(&rid),
        fatal_env(Some(&rid), "extra terminal"),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

// =========================================================================
// 9. Unknown envelope type / graceful handling (tests 31-32)
// =========================================================================

#[test]
fn unknown_envelope_type_returns_error() {
    let raw = r#"{"t":"unknown_type","data":"something"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn missing_t_field_returns_error() {
    let raw = r#"{"ref_id":"run-1","error":"no discriminator"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err());
}

// =========================================================================
// 10. Malformed JSONL (tests 33-34)
// =========================================================================

#[test]
fn malformed_json_returns_error() {
    let result = JsonlCodec::decode("this is not json at all");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn truncated_json_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"#);
    assert!(result.is_err());
}

// =========================================================================
// 11. Empty lines are skipped (tests 35-36)
// =========================================================================

#[test]
fn decode_stream_skips_empty_lines() {
    let hello_json = JsonlCodec::encode(&hello()).unwrap();
    let fatal_json = JsonlCodec::encode(&fatal_env(None, "err")).unwrap();
    let input = format!("\n\n{hello_json}\n  \n{fatal_json}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_handles_only_blank_lines() {
    let input = "\n\n  \n\t\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

// =========================================================================
// 12. Multiple sequential runs after final (tests 37-38)
// =========================================================================

#[test]
fn two_sequential_runs_produce_independent_streams() {
    let wo1 = work_order();
    let wo2 = work_order();
    let rid1 = wo1.id.to_string();
    let rid2 = wo2.id.to_string();

    // Encode first run
    let mut buf = Vec::new();
    let envelopes_run1 = [
        hello(),
        run_env(&wo1),
        event_env(
            &rid1,
            AgentEventKind::RunStarted {
                message: "run1".into(),
            },
        ),
        final_env(&rid1),
    ];
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes_run1).unwrap();

    // Decode first run
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 4);

    // Second run has independent ref_id
    let mut buf2 = Vec::new();
    let envelopes_run2 = [
        run_env(&wo2),
        event_env(
            &rid2,
            AgentEventKind::RunStarted {
                message: "run2".into(),
            },
        ),
        final_env(&rid2),
    ];
    JsonlCodec::encode_many_to_writer(&mut buf2, &envelopes_run2).unwrap();
    let reader2 = BufReader::new(buf2.as_slice());
    let decoded2: Vec<_> = JsonlCodec::decode_stream(reader2)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded2.len(), 3);

    // Verify ref_ids are distinct
    assert_ne!(rid1, rid2);
}

#[test]
fn sequential_runs_ref_ids_do_not_collide() {
    let wo1 = work_order();
    let wo2 = work_order();
    assert_ne!(wo1.id, wo2.id);
}

// =========================================================================
// 13. Large payloads (tests 39-41)
// =========================================================================

#[test]
fn large_work_order_task_roundtrips() {
    let big_task = "x".repeat(100_000);
    let wo = WorkOrderBuilder::new(&big_task).build();
    let env = run_env(&wo);
    let decoded = roundtrip(&env);
    if let Envelope::Run {
        work_order: dwo, ..
    } = decoded
    {
        assert_eq!(dwo.task.len(), 100_000);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn large_event_text_roundtrips() {
    let big_text = "y".repeat(200_000);
    let env = event_env(
        "r1",
        AgentEventKind::AssistantMessage {
            text: big_text.clone(),
        },
    );
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text.len(), 200_000);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_payload_triggers_validation_warning() {
    let big_text = "z".repeat(15_000_000);
    let env = event_env("r1", AgentEventKind::AssistantMessage { text: big_text });
    let result = EnvelopeValidator::new().validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| {
        matches!(
            w,
            abp_protocol::validate::ValidationWarning::LargePayload { .. }
        )
    }));
}

// =========================================================================
// 14. ref_id correlation (tests 42-44)
// =========================================================================

#[test]
fn all_events_share_run_ref_id() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![
        hello(),
        run_env(&wo),
        event_env(
            &rid,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        event_env(&rid, AgentEventKind::AssistantDelta { text: "tok".into() }),
        event_env(
            &rid,
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!("ls"),
            },
        ),
        event_env(
            &rid,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        final_env(&rid),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(errors.is_empty(), "all ref_ids match: {errors:?}");
}

#[test]
fn mismatched_ref_id_detected() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![
        hello(),
        run_env(&wo),
        event_env(
            "wrong-ref-id",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        final_env(&rid),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn final_ref_id_must_match_run_id() {
    let wo = work_order();
    let seq = vec![
        hello(),
        run_env(&wo),
        event_env(
            &wo.id.to_string(),
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        final_env("wrong-final-ref"),
    ];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

// =========================================================================
// 15. Capability declaration (tests 45-48)
// =========================================================================

#[test]
fn hello_capabilities_roundtrip() {
    let decoded = roundtrip(&hello());
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert_eq!(capabilities.len(), 3);
        assert!(capabilities.contains_key(&Capability::ToolRead));
        assert!(capabilities.contains_key(&Capability::ToolWrite));
        assert!(capabilities.contains_key(&Capability::Streaming));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_capabilities_support_levels_preserved() {
    let decoded = roundtrip(&hello());
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(matches!(
            capabilities[&Capability::ToolRead],
            SupportLevel::Native
        ));
        assert!(matches!(
            capabilities[&Capability::Streaming],
            SupportLevel::Emulated
        ));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn empty_capabilities_valid() {
    let env = Envelope::hello(backend(), BTreeMap::new());
    let decoded = roundtrip(&env);
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn restricted_capability_roundtrip() {
    let mut m = BTreeMap::new();
    m.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    );
    let env = Envelope::hello(backend(), m);
    let decoded = roundtrip(&env);
    if let Envelope::Hello { capabilities, .. } = decoded {
        if let Some(SupportLevel::Restricted { reason }) = capabilities.get(&Capability::ToolBash) {
            assert_eq!(reason, "sandbox only");
        } else {
            panic!("expected Restricted");
        }
    } else {
        panic!("expected Hello");
    }
}

// =========================================================================
// 16. Extension fields (tests 49-52)
// =========================================================================

#[test]
fn ext_field_roundtrips_on_event() {
    let mut ext = BTreeMap::new();
    ext.insert("vendor_trace_id".into(), serde_json::json!("abc-123"));
    ext.insert("custom_flag".into(), serde_json::json!(true));
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        let ext = event.ext.as_ref().expect("ext should be present");
        assert_eq!(ext["vendor_trace_id"], "abc-123");
        assert_eq!(ext["custom_flag"], true);
    } else {
        panic!("expected Event");
    }
}

#[test]
fn ext_field_none_omitted_in_json() {
    let env = event_env(
        "r1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(!json.contains("\"ext\""));
}

#[test]
fn ext_field_with_nested_object() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "hello"}]
        }),
    );
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        },
    };
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        let ext = event.ext.as_ref().unwrap();
        let raw = &ext["raw_message"];
        assert_eq!(raw["role"], "assistant");
        assert!(raw["content"].is_array());
    } else {
        panic!("expected Event");
    }
}

#[test]
fn ext_field_with_empty_map() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: Some(BTreeMap::new()),
        },
    };
    // Empty ext map should still roundtrip
    let decoded = roundtrip(&env);
    if let Envelope::Event { event, .. } = decoded {
        // Empty BTreeMap might be present or absent depending on serde behavior;
        // key point is it doesn't fail.
        assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
    } else {
        panic!("expected Event");
    }
}

// =========================================================================
// Additional conformance: validation, encoding, and edge cases (53-58)
// =========================================================================

#[test]
fn validator_rejects_empty_backend_id() {
    let env = Envelope::hello(
        BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "backend.id"))
    );
}

#[test]
fn validator_rejects_invalid_contract_version() {
    let env = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: backend(),
        capabilities: caps(),
        mode: ExecutionMode::default(),
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn validator_rejects_empty_run_id() {
    let wo = work_order();
    let env = Envelope::Run {
        id: String::new(),
        work_order: wo,
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "id"))
    );
}

#[test]
fn validator_rejects_empty_event_ref_id() {
    let env = event_env(
        "",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
}

#[test]
fn validator_rejects_empty_fatal_error() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: String::new(),
        error_code: None,
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
}

#[test]
fn encode_appends_newline() {
    let env = hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.ends_with('\n'));
    // Exactly one trailing newline
    assert!(!json.ends_with("\n\n"));
}

// =========================================================================
// Event kind discriminator (tests 59-61)
// =========================================================================

#[test]
fn event_kind_uses_type_discriminator_not_t() {
    let env = event_env("r1", AgentEventKind::AssistantMessage { text: "hi".into() });
    let v = to_value(&env);
    // The flattened event should contain "type" for the kind discriminator
    assert_eq!(v["event"]["type"], "assistant_message");
    // Inner event must NOT use "t"
    assert!(v["event"].get("t").is_none());
}

#[test]
fn all_event_kinds_serialize_with_type_field() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "m".into(),
        },
        AgentEventKind::RunCompleted {
            message: "m".into(),
        },
        AgentEventKind::AssistantDelta { text: "t".into() },
        AgentEventKind::AssistantMessage { text: "t".into() },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
        AgentEventKind::FileChanged {
            path: "p".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: None,
            output_preview: None,
        },
    ];
    for kind in kinds {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert!(
            v.get("type").is_some(),
            "kind should have 'type' field: {v}"
        );
    }
}

#[test]
fn event_kind_rename_snake_case() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "a.rs".into(),
            summary: "added".into(),
        },
        ext: None,
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "file_changed");
}

// =========================================================================
// encode_to_writer / encode_many_to_writer (tests 62-63)
// =========================================================================

#[test]
fn encode_to_writer_produces_valid_jsonl() {
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello()).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    let decoded = JsonlCodec::decode(s.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn encode_many_to_writer_multiple_envelopes() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let envs = vec![
        hello(),
        run_env(&wo),
        event_env(
            &rid,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        final_env(&rid),
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

// =========================================================================
// Version negotiation edge cases (tests 64-66)
// =========================================================================

#[test]
fn parse_version_rejects_garbage() {
    assert!(parse_version("garbage").is_none());
    assert!(parse_version("").is_none());
    assert!(parse_version("v0.1").is_none());
    assert!(parse_version("abp/0.1").is_none());
}

#[test]
fn parse_version_accepts_valid_formats() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v99.42"), Some((99, 42)));
}

#[test]
fn version_compatibility_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.99"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
}
