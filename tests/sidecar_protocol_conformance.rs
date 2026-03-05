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
//! Exhaustive validation of the JSONL protocol lifecycle, envelope ordering,
//! field semantics, error handling, heartbeat, graceful shutdown, version
//! negotiation, capability advertisement, and edge cases across the full
//! hello → run → event* → final/fatal flow.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, ReceiptBuilder, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_protocol::capability_advertisement::{
    CapabilityAdvertisement, ContentType, Dialect, StreamingMode, ToolSupportLevel,
};
use abp_protocol::graceful_shutdown::{
    GoodbyeResponse, GoodbyeStatus, ShutdownCoordinator, ShutdownReason, ShutdownRequest,
};
use abp_protocol::heartbeat::{HeartbeatConfig, HeartbeatMonitor, HeartbeatState, Ping, Pong};
use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
use abp_protocol::version_negotiation::{
    NegotiationError, VersionOffer, VersionSelection, negotiate,
};
use abp_protocol::{
    Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version,
    validate::{EnvelopeValidator, SequenceError, ValidationError, ValidationWarning},
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

fn hashed_receipt() -> Receipt {
    ReceiptBuilder::new("conformance-sidecar")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap()
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

fn final_env_hashed(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: hashed_receipt(),
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

fn v(major: u32, minor: u32) -> ProtocolVersion {
    ProtocolVersion { major, minor }
}

// =========================================================================
// 1. JSONL wire format (tests 1–8)
// =========================================================================

#[test]
fn wire_each_line_is_valid_json() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let envelopes = [
        hello(),
        run_env(&wo),
        event_env(&rid, AgentEventKind::AssistantDelta { text: "t".into() }),
        final_env(&rid),
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let text = String::from_utf8(buf).unwrap();
    for line in text.lines() {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("not valid JSON: {e}\nline: {line}"));
        assert!(parsed.is_object());
    }
}

#[test]
fn wire_tag_field_is_t_on_all_variants() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let envelopes: Vec<Envelope> = vec![
        hello(),
        run_env(&wo),
        event_env(
            &rid,
            AgentEventKind::RunStarted {
                message: "g".into(),
            },
        ),
        final_env(&rid),
        fatal_env(None, "err"),
    ];
    for env in &envelopes {
        let v = to_value(env);
        assert!(v.get("t").is_some(), "missing 't' field: {v}");
        assert!(
            v.get("type").is_none(),
            "must not use 'type' at envelope level"
        );
    }
}

#[test]
fn wire_hello_has_all_required_fields() {
    let v = to_value(&hello());
    assert_eq!(v["t"], "hello");
    assert!(v.get("contract_version").is_some());
    assert!(v.get("backend").is_some());
    assert!(v.get("capabilities").is_some());
    assert!(v.get("mode").is_some());
}

#[test]
fn wire_run_has_all_required_fields() {
    let wo = work_order();
    let v = to_value(&run_env(&wo));
    assert_eq!(v["t"], "run");
    assert!(v.get("id").is_some());
    assert!(v.get("work_order").is_some());
}

#[test]
fn wire_event_has_all_required_fields() {
    let env = event_env("r1", AgentEventKind::AssistantDelta { text: "x".into() });
    let v = to_value(&env);
    assert_eq!(v["t"], "event");
    assert!(v.get("ref_id").is_some());
    assert!(v.get("event").is_some());
}

#[test]
fn wire_final_has_all_required_fields() {
    let v = to_value(&final_env("r1"));
    assert_eq!(v["t"], "final");
    assert!(v.get("ref_id").is_some());
    assert!(v.get("receipt").is_some());
}

#[test]
fn wire_fatal_has_all_required_fields() {
    let v = to_value(&fatal_env(Some("r1"), "boom"));
    assert_eq!(v["t"], "fatal");
    assert!(v.get("ref_id").is_some());
    assert!(v.get("error").is_some());
}

#[test]
fn wire_encode_appends_exactly_one_newline() {
    let json = JsonlCodec::encode(&hello()).unwrap();
    assert!(json.ends_with('\n'));
    assert!(!json.ends_with("\n\n"));
}

// =========================================================================
// 2. Hello handshake (tests 9–16)
// =========================================================================

#[test]
fn hello_roundtrip_preserves_variant() {
    let decoded = roundtrip(&hello());
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_must_be_first_in_sequence() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![hello(), run_env(&wo), final_env(&rid)];
    let errors = EnvelopeValidator::new().validate_sequence(&seq);
    assert!(errors.is_empty(), "valid sequence: {errors:?}");
}

#[test]
fn hello_preserves_backend_identity() {
    let v = to_value(&hello());
    assert_eq!(v["backend"]["id"], "conformance-sidecar");
    assert_eq!(v["backend"]["backend_version"], "2.0.0");
    assert_eq!(v["backend"]["adapter_version"], "0.2.0");
}

#[test]
fn hello_contains_contract_version() {
    let v = to_value(&hello());
    assert_eq!(v["contract_version"], CONTRACT_VERSION);
}

#[test]
fn hello_capabilities_included() {
    let v = to_value(&hello());
    let caps_val = &v["capabilities"];
    assert!(caps_val.is_object());
    assert!(caps_val.get("tool_read").is_some());
    assert!(caps_val.get("tool_write").is_some());
    assert!(caps_val.get("streaming").is_some());
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
    if let Envelope::Hello { mode, .. } = roundtrip(&env) {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_not_first_detected_by_validator() {
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

// =========================================================================
// 3. Run command (tests 17–22)
// =========================================================================

#[test]
fn run_envelope_has_correct_tag() {
    let wo = work_order();
    let v = to_value(&run_env(&wo));
    assert_eq!(v["t"], "run");
}

#[test]
fn run_ref_id_correlation() {
    let wo = work_order();
    let decoded = roundtrip(&run_env(&wo));
    if let Envelope::Run {
        id,
        work_order: dwo,
    } = decoded
    {
        assert_eq!(id, wo.id.to_string());
        assert_eq!(dwo.id, wo.id);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_work_order_serialization_preserves_task() {
    let wo = WorkOrderBuilder::new("important task with special chars: <>&\"").build();
    if let Envelope::Run {
        work_order: dwo, ..
    } = roundtrip(&run_env(&wo))
    {
        assert_eq!(dwo.task, "important task with special chars: <>&\"");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_config_passthrough_model() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
    if let Envelope::Run {
        work_order: dwo, ..
    } = roundtrip(&run_env(&wo))
    {
        assert_eq!(dwo.config.model.as_deref(), Some("gpt-4"));
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_config_passthrough_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(5).build();
    if let Envelope::Run {
        work_order: dwo, ..
    } = roundtrip(&run_env(&wo))
    {
        assert_eq!(dwo.config.max_turns, Some(5));
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_config_passthrough_vendor_flags() {
    let mut config = abp_core::RuntimeConfig::default();
    config
        .vendor
        .insert("custom_key".into(), serde_json::json!(42));
    let wo = WorkOrderBuilder::new("test").config(config).build();
    if let Envelope::Run {
        work_order: dwo, ..
    } = roundtrip(&run_env(&wo))
    {
        assert_eq!(dwo.config.vendor["custom_key"], 42);
    } else {
        panic!("expected Run");
    }
}

// =========================================================================
// 4. Event streaming (tests 23–32)
// =========================================================================

#[test]
fn event_envelope_carries_ref_id() {
    let v = to_value(&event_env(
        "run-42",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    ));
    assert_eq!(v["ref_id"], "run-42");
    assert_eq!(v["t"], "event");
}

#[test]
fn event_assistant_delta_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::AssistantDelta {
            text: "Hello ".into(),
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
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
fn event_assistant_message_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        assert!(matches!(
            event.kind,
            AgentEventKind::AssistantMessage { .. }
        ));
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
    if let Envelope::Event { event, .. } = roundtrip(&env) {
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
            output: serde_json::json!("file contents"),
            is_error: false,
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        if let AgentEventKind::ToolResult { is_error, .. } = &event.kind {
            assert!(!is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_error_flag_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!("exit code 1"),
            is_error: true,
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        if let AgentEventKind::ToolResult { is_error, .. } = &event.kind {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_error_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::Error {
            message: "rate limited".into(),
            error_code: Some(abp_error::ErrorCode::BackendRateLimited),
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        if let AgentEventKind::Error {
            message,
            error_code,
        } = &event.kind
        {
            assert_eq!(message, "rate limited");
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendRateLimited));
        } else {
            panic!("expected Error");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_warning_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::Warning {
            message: "slow".into(),
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_file_changed_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added function".into(),
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        if let AgentEventKind::FileChanged { path, summary } = &event.kind {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(summary, "added function");
        } else {
            panic!("expected FileChanged");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_command_executed_roundtrip() {
    let env = event_env(
        "r1",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all tests passed".into()),
        },
    );
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        if let AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } = &event.kind
        {
            assert_eq!(command, "cargo test");
            assert_eq!(*exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("all tests passed"));
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event");
    }
}

// =========================================================================
// 5. Final envelope with receipt (tests 33–40)
// =========================================================================

#[test]
fn final_envelope_contains_receipt() {
    let v = to_value(&final_env("run-1"));
    assert_eq!(v["t"], "final");
    assert_eq!(v["ref_id"], "run-1");
    assert!(v.get("receipt").is_some());
}

#[test]
fn final_receipt_has_outcome() {
    let v = to_value(&final_env("run-1"));
    assert_eq!(v["receipt"]["outcome"], "complete");
}

#[test]
fn final_receipt_has_meta_fields() {
    let v = to_value(&final_env("run-1"));
    let meta = &v["receipt"]["meta"];
    assert!(meta.get("run_id").is_some());
    assert!(meta.get("work_order_id").is_some());
    assert!(meta.get("contract_version").is_some());
    assert!(meta.get("started_at").is_some());
    assert!(meta.get("finished_at").is_some());
    assert!(meta.get("duration_ms").is_some());
}

#[test]
fn final_receipt_has_backend_identity() {
    let v = to_value(&final_env("run-1"));
    assert_eq!(v["receipt"]["backend"]["id"], "conformance-sidecar");
}

#[test]
fn final_receipt_contract_version_matches() {
    let v = to_value(&final_env("run-1"));
    assert_eq!(v["receipt"]["meta"]["contract_version"], CONTRACT_VERSION);
}

#[test]
fn final_receipt_sha256_valid_when_hashed() {
    let r = hashed_receipt();
    let hash = r.receipt_sha256.as_ref().expect("hash should be set");
    assert_eq!(hash.len(), 64, "SHA-256 hex digest is 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hex chars only"
    );
}

#[test]
fn final_receipt_hash_is_deterministic() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn final_envelope_roundtrip_preserves_receipt() {
    if let Envelope::Final { ref_id, receipt: r } = roundtrip(&final_env_hashed("run-1")) {
        assert_eq!(ref_id, "run-1");
        assert_eq!(r.outcome, Outcome::Complete);
        assert!(r.receipt_sha256.is_some());
    } else {
        panic!("expected Final");
    }
}

// =========================================================================
// 6. Fatal envelope (tests 41–46)
// =========================================================================

#[test]
fn fatal_with_ref_id() {
    let v = to_value(&fatal_env(Some("run-1"), "out of memory"));
    assert_eq!(v["t"], "fatal");
    assert_eq!(v["ref_id"], "run-1");
    assert_eq!(v["error"], "out of memory");
}

#[test]
fn fatal_without_ref_id() {
    let v = to_value(&fatal_env(None, "startup crash"));
    assert!(v["ref_id"].is_null());
}

#[test]
fn fatal_roundtrip() {
    if let Envelope::Fatal { ref_id, error, .. } = roundtrip(&fatal_env(Some("r99"), "timeout")) {
        assert_eq!(ref_id.as_deref(), Some("r99"));
        assert_eq!(error, "timeout");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "version mismatch",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    let v = to_value(&env);
    assert_eq!(v["error_code"], "protocol_version_mismatch");

    if let Envelope::Fatal { error_code, .. } = roundtrip(&env) {
        assert_eq!(
            error_code,
            Some(abp_error::ErrorCode::ProtocolVersionMismatch)
        );
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_error_code_accessor() {
    let env = Envelope::fatal_with_code(
        None,
        "bad envelope",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn fatal_error_code_none_for_non_fatal() {
    assert!(hello().error_code().is_none());
}

// =========================================================================
// 7. Protocol version negotiation (tests 47–55)
// =========================================================================

#[test]
fn parse_version_rejects_garbage() {
    assert!(parse_version("garbage").is_none());
    assert!(parse_version("").is_none());
    assert!(parse_version("v0.1").is_none());
    assert!(parse_version("abp/0.1").is_none());
}

#[test]
fn parse_version_accepts_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v99.42"), Some((99, 42)));
}

#[test]
fn version_compat_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.99"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn protocol_version_parse_and_display() {
    let pv = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(pv.major, 0);
    assert_eq!(pv.minor, 1);
    assert_eq!(format!("{pv}"), "abp/v0.1");
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current, ProtocolVersion::parse(CONTRACT_VERSION).unwrap());
}

#[test]
fn negotiate_version_same_versions() {
    let local = v(0, 1);
    let remote = v(0, 1);
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result, v(0, 1));
}

#[test]
fn negotiate_version_picks_min_minor() {
    let local = v(0, 2);
    let remote = v(0, 1);
    assert_eq!(negotiate_version(&local, &remote).unwrap(), v(0, 1));
}

#[test]
fn negotiate_version_mismatch_major_fails() {
    let err = negotiate_version(&v(0, 1), &v(1, 0)).unwrap_err();
    assert!(matches!(
        err,
        abp_protocol::version::VersionError::Incompatible { .. }
    ));
}

#[test]
fn extended_negotiate_picks_highest_common() {
    let host = VersionOffer::new(vec![v(0, 1), v(0, 2), v(0, 3)]);
    let sidecar = VersionOffer::new(vec![v(0, 2), v(0, 3), v(0, 4)]);
    let sel = negotiate(&host, &sidecar).unwrap();
    assert_eq!(sel.selected, v(0, 3));
}

// =========================================================================
// 8. Capability advertisement (tests 56–63)
// =========================================================================

#[test]
fn capability_ad_default_has_generic_dialect() {
    let ad = CapabilityAdvertisement::default();
    assert_eq!(ad.dialects(), &[Dialect::Generic]);
    assert_eq!(*ad.tool_support(), ToolSupportLevel::None);
    assert_eq!(ad.streaming_modes(), &[StreamingMode::Jsonl]);
}

#[test]
fn capability_ad_builder_sets_fields() {
    let ad = CapabilityAdvertisement::builder()
        .dialect(Dialect::Anthropic)
        .dialect(Dialect::OpenAi)
        .tool_support(ToolSupportLevel::ParallelCalls)
        .streaming_mode(StreamingMode::Sse)
        .max_context_length(200_000)
        .content_type(ContentType::Text)
        .content_type(ContentType::Image)
        .build();
    assert_eq!(ad.dialects().len(), 2);
    assert_eq!(ad.max_context_length(), Some(200_000));
    assert!(ad.supports_content_type(&ContentType::Image));
}

#[test]
fn capability_ad_serde_roundtrip() {
    let ad = CapabilityAdvertisement::builder()
        .dialect(Dialect::Gemini)
        .tool_support(ToolSupportLevel::StreamingCalls)
        .extension("model", serde_json::json!("gemini-pro"))
        .build();
    let json = serde_json::to_string(&ad).unwrap();
    let decoded: CapabilityAdvertisement = serde_json::from_str(&json).unwrap();
    assert_eq!(ad, decoded);
}

#[test]
fn capability_ad_negotiate_dialect() {
    let a = CapabilityAdvertisement::builder()
        .dialect(Dialect::Anthropic)
        .dialect(Dialect::OpenAi)
        .build();
    let b = CapabilityAdvertisement::builder()
        .dialect(Dialect::OpenAi)
        .dialect(Dialect::Gemini)
        .build();
    assert_eq!(a.negotiate_dialect(&b), Some(Dialect::OpenAi));
}

#[test]
fn capability_ad_negotiate_dialect_none_when_disjoint() {
    let a = CapabilityAdvertisement::builder()
        .dialect(Dialect::Anthropic)
        .build();
    let b = CapabilityAdvertisement::builder()
        .dialect(Dialect::Gemini)
        .build();
    assert_eq!(a.negotiate_dialect(&b), None);
}

#[test]
fn capability_ad_custom_dialect() {
    let ad = CapabilityAdvertisement::builder()
        .dialect(Dialect::Custom("my-vendor".into()))
        .build();
    assert!(ad.supports_dialect(&Dialect::Custom("my-vendor".into())));
    assert!(!ad.supports_dialect(&Dialect::Custom("other".into())));
}

#[test]
fn capability_ad_extensions_preserved() {
    let ad = CapabilityAdvertisement::builder()
        .extension("tier", serde_json::json!(1))
        .extension("region", serde_json::json!("us-east"))
        .build();
    assert_eq!(ad.extensions().len(), 2);
    assert_eq!(ad.extensions()["tier"], 1);
}

#[test]
fn capability_ad_deduplicates_dialects() {
    let ad = CapabilityAdvertisement::builder()
        .dialect(Dialect::OpenAi)
        .dialect(Dialect::OpenAi)
        .build();
    assert_eq!(ad.dialects().len(), 1);
}

// =========================================================================
// 9. Graceful shutdown (tests 64–72)
// =========================================================================

#[test]
fn shutdown_request_basic() {
    let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(30));
    assert_eq!(req.reason(), &ShutdownReason::Normal);
    assert_eq!(req.deadline(), Duration::from_secs(30));
    assert!(req.message().is_none());
}

#[test]
fn shutdown_request_with_message() {
    let req = ShutdownRequest::new(ShutdownReason::HostShutdown, Duration::from_secs(5))
        .with_message("shutting down");
    assert_eq!(req.message(), Some("shutting down"));
}

#[test]
fn shutdown_request_is_expired() {
    let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_millis(10));
    assert!(!req.is_expired(Duration::from_millis(5)));
    assert!(req.is_expired(Duration::from_millis(10)));
    assert!(req.is_expired(Duration::from_millis(15)));
}

#[test]
fn goodbye_response_clean() {
    let resp = GoodbyeResponse::new(GoodbyeStatus::Clean);
    assert!(resp.is_clean());
    assert_eq!(resp.completed_requests(), 0);
    assert_eq!(resp.abandoned_requests(), 0);
    assert!(resp.error().is_none());
}

#[test]
fn goodbye_response_partial_with_counts() {
    let resp = GoodbyeResponse::new(GoodbyeStatus::Partial)
        .with_completed(3)
        .with_abandoned(1);
    assert!(!resp.is_clean());
    assert_eq!(resp.completed_requests(), 3);
    assert_eq!(resp.abandoned_requests(), 1);
}

#[test]
fn goodbye_response_error_with_message() {
    let resp = GoodbyeResponse::new(GoodbyeStatus::Error).with_error("disk full");
    assert_eq!(resp.error(), Some("disk full"));
    assert!(!resp.is_clean());
}

#[test]
fn shutdown_coordinator_lifecycle() {
    let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(60));
    let mut coord = ShutdownCoordinator::new(req);
    assert!(!coord.is_complete());
    assert!(!coord.is_expired());

    coord.record_response(GoodbyeResponse::new(GoodbyeStatus::Clean));
    assert!(coord.is_complete());
    assert!(!coord.is_expired());
    assert!(coord.response().unwrap().is_clean());
}

#[test]
fn shutdown_reason_serde_roundtrip() {
    for reason in [
        ShutdownReason::Normal,
        ShutdownReason::ResourceLimit,
        ShutdownReason::Replacement,
        ShutdownReason::HostShutdown,
        ShutdownReason::PolicyViolation,
        ShutdownReason::Custom("test".into()),
    ] {
        let json = serde_json::to_string(&reason).unwrap();
        let decoded: ShutdownReason = serde_json::from_str(&json).unwrap();
        assert_eq!(reason, decoded);
    }
}

#[test]
fn shutdown_request_serde_roundtrip() {
    let req = ShutdownRequest::new(ShutdownReason::ResourceLimit, Duration::from_secs(30))
        .with_message("memory exceeded");
    let json = serde_json::to_string(&req).unwrap();
    let decoded: ShutdownRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, decoded);
}

// =========================================================================
// 10. Heartbeat: Ping/Pong (tests 73–82)
// =========================================================================

#[test]
fn heartbeat_initial_state_is_idle() {
    let mon = HeartbeatMonitor::new(HeartbeatConfig::default());
    assert_eq!(*mon.state(), HeartbeatState::Idle);
    assert!(!mon.is_stalled());
    assert!(!mon.is_alive());
}

#[test]
fn heartbeat_ping_pong_becomes_alive() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);
    let ping = mon.next_ping();
    assert_eq!(ping.seq, 0);
    mon.record_pong(ping.seq);
    assert!(mon.is_alive());
    assert_eq!(mon.consecutive_missed(), 0);
}

#[test]
fn heartbeat_miss_becomes_degraded() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);
    let _ping = mon.next_ping();
    mon.record_miss();
    assert_eq!(*mon.state(), HeartbeatState::Degraded { missed: 1 });
}

#[test]
fn heartbeat_max_misses_becomes_stalled() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);
    for _ in 0..3 {
        let _p = mon.next_ping();
        mon.record_miss();
    }
    assert!(mon.is_stalled());
    assert_eq!(*mon.state(), HeartbeatState::Stalled { missed: 3 });
}

#[test]
fn heartbeat_pong_resets_after_misses() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);
    let _p = mon.next_ping();
    mon.record_miss();
    let _p = mon.next_ping();
    mon.record_miss();
    assert_eq!(mon.consecutive_missed(), 2);
    let ping = mon.next_ping();
    mon.record_pong(ping.seq);
    assert!(mon.is_alive());
    assert_eq!(mon.consecutive_missed(), 0);
}

#[test]
fn heartbeat_wrong_seq_ignored() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);
    let _ping = mon.next_ping();
    mon.record_pong(999);
    assert_eq!(*mon.state(), HeartbeatState::Idle);
}

#[test]
fn heartbeat_stall_threshold_calculation() {
    let cfg = HeartbeatConfig::new(Duration::from_secs(5), Duration::from_secs(2), 4);
    assert_eq!(cfg.stall_threshold(), Duration::from_secs(8));
}

#[test]
fn heartbeat_ping_serde_roundtrip() {
    let ping = Ping {
        seq: 42,
        timestamp_ms: 1_700_000_000_000,
    };
    let json = serde_json::to_string(&ping).unwrap();
    let decoded: Ping = serde_json::from_str(&json).unwrap();
    assert_eq!(ping, decoded);
}

#[test]
fn heartbeat_pong_serde_roundtrip() {
    let pong = Pong {
        seq: 42,
        timestamp_ms: 1_700_000_000_001,
    };
    let json = serde_json::to_string(&pong).unwrap();
    let decoded: Pong = serde_json::from_str(&json).unwrap();
    assert_eq!(pong, decoded);
}

#[test]
fn heartbeat_reset_clears_state() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);
    let ping = mon.next_ping();
    mon.record_pong(ping.seq);
    assert!(mon.is_alive());
    mon.reset();
    assert_eq!(*mon.state(), HeartbeatState::Idle);
    assert_eq!(mon.total_pings(), 0);
    assert_eq!(mon.total_pongs(), 0);
}

// =========================================================================
// Extended version negotiation (tests 83–87)
// =========================================================================

#[test]
fn extended_negotiate_no_overlap_fails() {
    let host = VersionOffer::new(vec![v(0, 1)]);
    let sidecar = VersionOffer::new(vec![v(1, 0)]);
    let err = negotiate(&host, &sidecar).unwrap_err();
    assert!(matches!(err, NegotiationError::NoOverlap { .. }));
}

#[test]
fn extended_negotiate_empty_offer_fails() {
    let host = VersionOffer::new(vec![]);
    let sidecar = VersionOffer::new(vec![v(0, 1)]);
    assert!(matches!(
        negotiate(&host, &sidecar).unwrap_err(),
        NegotiationError::EmptyOffer
    ));
}

#[test]
fn version_offer_from_range() {
    let range = VersionRange {
        min: v(0, 1),
        max: v(0, 3),
    };
    let offer = VersionOffer::from_range(&range).unwrap();
    assert_eq!(offer.versions().len(), 3);
    assert!(offer.contains(&v(0, 1)));
    assert!(offer.contains(&v(0, 2)));
    assert!(offer.contains(&v(0, 3)));
}

#[test]
fn version_offer_from_range_cross_major_fails() {
    let range = VersionRange {
        min: v(0, 1),
        max: v(1, 0),
    };
    assert!(VersionOffer::from_range(&range).is_none());
}

#[test]
fn version_selection_serde_roundtrip() {
    let host = VersionOffer::new(vec![v(0, 1), v(0, 2)]);
    let sidecar = VersionOffer::new(vec![v(0, 2)]);
    let sel = negotiate(&host, &sidecar).unwrap();
    let json = serde_json::to_string(&sel).unwrap();
    let decoded: VersionSelection = serde_json::from_str(&json).unwrap();
    assert_eq!(sel, decoded);
}

// =========================================================================
// Sequence validation (tests 88–93)
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
        event_env(&rid, AgentEventKind::AssistantDelta { text: "tok".into() }),
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
    assert!(EnvelopeValidator::new().validate_sequence(&seq).is_empty());
}

#[test]
fn sequence_missing_hello() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![run_env(&wo), final_env(&rid)];
    assert!(
        EnvelopeValidator::new()
            .validate_sequence(&seq)
            .contains(&SequenceError::MissingHello)
    );
}

#[test]
fn sequence_missing_terminal() {
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
    ];
    assert!(
        EnvelopeValidator::new()
            .validate_sequence(&seq)
            .contains(&SequenceError::MissingTerminal)
    );
}

#[test]
fn sequence_event_before_run_out_of_order() {
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
    assert!(
        EnvelopeValidator::new()
            .validate_sequence(&seq)
            .contains(&SequenceError::OutOfOrderEvents)
    );
}

#[test]
fn sequence_multiple_terminals_invalid() {
    let wo = work_order();
    let rid = wo.id.to_string();
    let seq = vec![
        hello(),
        run_env(&wo),
        final_env(&rid),
        fatal_env(Some(&rid), "extra"),
    ];
    assert!(
        EnvelopeValidator::new()
            .validate_sequence(&seq)
            .contains(&SequenceError::MultipleTerminals)
    );
}

// =========================================================================
// ref_id correlation (tests 94–96)
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
        final_env(&rid),
    ];
    assert!(EnvelopeValidator::new().validate_sequence(&seq).is_empty());
}

#[test]
fn mismatched_event_ref_id_detected() {
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
    assert!(
        EnvelopeValidator::new()
            .validate_sequence(&seq)
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn mismatched_final_ref_id_detected() {
    let wo = work_order();
    let seq = vec![hello(), run_env(&wo), final_env("wrong-final-ref")];
    assert!(
        EnvelopeValidator::new()
            .validate_sequence(&seq)
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

// =========================================================================
// Malformed input / edge cases (tests 97–103)
// =========================================================================

#[test]
fn unknown_envelope_type_returns_error() {
    let raw = r#"{"t":"unknown_type","data":"something"}"#;
    assert!(matches!(
        JsonlCodec::decode(raw).unwrap_err(),
        ProtocolError::Json(_)
    ));
}

#[test]
fn missing_t_field_returns_error() {
    let raw = r#"{"ref_id":"run-1","error":"no discriminator"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn malformed_json_returns_error() {
    assert!(matches!(
        JsonlCodec::decode("not json").unwrap_err(),
        ProtocolError::Json(_)
    ));
}

#[test]
fn truncated_json_returns_error() {
    assert!(JsonlCodec::decode(r#"{"t":"hello","contract_version":"#).is_err());
}

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
fn decode_stream_only_blank_lines_yields_nothing() {
    let reader = BufReader::new("\n\n  \n\t\n".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn large_payload_triggers_validation_warning() {
    let big_text = "z".repeat(15_000_000);
    let env = event_env("r1", AgentEventKind::AssistantMessage { text: big_text });
    let result = EnvelopeValidator::new().validate(&env);
    assert!(result.valid);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::LargePayload { .. }))
    );
}

// =========================================================================
// Validation (tests 104–109)
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
    let env = Envelope::Run {
        id: String::new(),
        work_order: work_order(),
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
}

#[test]
fn validator_rejects_empty_event_ref_id() {
    let env = event_env(
        "",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    assert!(!EnvelopeValidator::new().validate(&env).valid);
}

#[test]
fn validator_rejects_empty_fatal_error() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: String::new(),
        error_code: None,
    };
    assert!(!EnvelopeValidator::new().validate(&env).valid);
}

#[test]
fn validator_warns_missing_optional_backend_fields() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    let result = EnvelopeValidator::new().validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version")));
    assert!(result.warnings.iter().any(|w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "backend.adapter_version")));
}

// =========================================================================
// Event kind discriminator (tests 110–112)
// =========================================================================

#[test]
fn event_kind_uses_type_not_t() {
    let v = to_value(&event_env(
        "r1",
        AgentEventKind::AssistantMessage { text: "hi".into() },
    ));
    assert_eq!(v["event"]["type"], "assistant_message");
    assert!(v["event"].get("t").is_none());
}

#[test]
fn all_event_kinds_have_type_field() {
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
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: serde_json::json!(null),
            is_error: false,
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
fn event_kind_uses_snake_case() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "a.rs".into(),
            summary: "added".into(),
        },
        ext: None,
    };
    assert_eq!(serde_json::to_value(&ev).unwrap()["type"], "file_changed");
}

// =========================================================================
// Extension fields (tests 113–116)
// =========================================================================

#[test]
fn ext_field_roundtrips_on_event() {
    let mut ext = BTreeMap::new();
    ext.insert("vendor_trace_id".into(), serde_json::json!("abc-123"));
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        assert_eq!(event.ext.as_ref().unwrap()["vendor_trace_id"], "abc-123");
    } else {
        panic!("expected Event");
    }
}

#[test]
fn ext_none_omitted_in_json() {
    let json = JsonlCodec::encode(&event_env(
        "r1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    ))
    .unwrap();
    assert!(!json.contains("\"ext\""));
}

#[test]
fn ext_with_nested_object() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "hi"}]}),
    );
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    if let Envelope::Event { event, .. } = roundtrip(&env) {
        assert_eq!(
            event.ext.as_ref().unwrap()["raw_message"]["role"],
            "assistant"
        );
    } else {
        panic!("expected Event");
    }
}

#[test]
fn ext_empty_map_does_not_fail() {
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
    let decoded = roundtrip(&env);
    assert!(matches!(decoded, Envelope::Event { .. }));
}

// =========================================================================
// encode_to_writer (tests 117–118)
// =========================================================================

#[test]
fn encode_to_writer_produces_valid_jsonl() {
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello()).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(matches!(
        JsonlCodec::decode(s.trim()).unwrap(),
        Envelope::Hello { .. }
    ));
}

#[test]
fn encode_many_to_writer_decodes_correctly() {
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
    let decoded: Vec<_> = JsonlCodec::decode_stream(BufReader::new(buf.as_slice()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

// =========================================================================
// Capability manifest in hello (tests 119–122)
// =========================================================================

#[test]
fn hello_capabilities_roundtrip() {
    if let Envelope::Hello { capabilities, .. } = roundtrip(&hello()) {
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
    if let Envelope::Hello { capabilities, .. } = roundtrip(&hello()) {
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
    if let Envelope::Hello { capabilities, .. } = roundtrip(&env) {
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
    if let Envelope::Hello { capabilities, .. } = roundtrip(&env) {
        if let Some(SupportLevel::Restricted { reason }) = capabilities.get(&Capability::ToolBash) {
            assert_eq!(reason, "sandbox only");
        } else {
            panic!("expected Restricted");
        }
    } else {
        panic!("expected Hello");
    }
}
