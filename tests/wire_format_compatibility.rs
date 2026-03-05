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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Wire format compatibility tests for the JSONL sidecar protocol.
//!
//! Ensures Rust types parse JSON exactly as Node/Python sidecars produce it,
//! and that encoded JSON matches what those sidecars expect to receive.

use std::io::BufReader;

use abp_core::{
    AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome, ReceiptBuilder,
    WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::{
    is_compatible_version, parse_version,
    version::{negotiate_version, ProtocolVersion, VersionError, VersionRange},
    Envelope, JsonlCodec, ProtocolError,
};

// =========================================================================
// Helpers
// =========================================================================

/// Decode a JSON string into an Envelope, panicking with a clear message on failure.
fn must_decode(json: &str) -> Envelope {
    JsonlCodec::decode(json).unwrap_or_else(|e| panic!("failed to decode: {e}\ninput: {json}"))
}

// =========================================================================
// 1. Fixed wire format vectors (15+ tests)
// =========================================================================

#[test]
fn parse_hello_from_node_sidecar() {
    // Exact JSON the Node sidecar emits (hosts/node/host.js lines 33-38)
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"example_node_sidecar","backend_version":"v20.0.0","adapter_version":"0.1"},"capabilities":{"streaming":"native","tool_read":"emulated","tool_write":"emulated","tool_edit":"emulated","structured_output_json_schema":"emulated"}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend.id, "example_node_sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("v20.0.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("0.1"));
            assert!(!capabilities.is_empty());
            // mode defaults to Mapped when absent
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn parse_hello_from_python_sidecar() {
    // Exact JSON the Python sidecar emits (hosts/python/host.py lines 579-587)
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"python_sidecar","backend_version":"3.12.0","adapter_version":"0.2.0"},"capabilities":{"streaming":"native","tool_read":"emulated","tool_write":"emulated","tool_edit":"emulated","structured_output_json_schema":"emulated","hooks_pre_tool_use":"native","hooks_post_tool_use":"native","session_resume":"emulated"},"mode":"mapped"}"#;
    let env = must_decode(json);
    match env {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend.id, "python_sidecar");
            assert_eq!(backend.adapter_version.as_deref(), Some("0.2.0"));
            assert!(capabilities.len() >= 5);
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn parse_event_assistant_message() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_message","text":"Hello from sidecar"}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { ref text } if text == "Hello from sidecar"
            ));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_run_started() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"run_started","message":"node sidecar starting: fix bug"}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(
                matches!(event.kind, AgentEventKind::RunStarted { ref message } if message.contains("sidecar"))
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_run_completed() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"run_completed","message":"done"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => {
            assert!(matches!(&event.kind, AgentEventKind::RunCompleted { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_tool_call() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"tool_call","tool_name":"read_file","tool_use_id":"tu-1","parent_tool_use_id":null,"input":{"path":"src/main.rs"}}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                assert_eq!(input["path"], "src/main.rs");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_tool_result() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"tool_result","tool_name":"read_file","tool_use_id":"tu-1","output":"fn main() {}","is_error":false}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                output,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(!is_error);
                assert_eq!(output, "fn main() {}");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_assistant_delta() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_delta","text":"tok"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(&event.kind, AgentEventKind::AssistantDelta { text } if text == "tok")
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_fatal_with_null_ref_id() {
    // Node sidecar emits null ref_id when JSON parse fails
    let json = r#"{"t":"fatal","ref_id":null,"error":"invalid json: SyntaxError"}"#;
    let env = must_decode(json);
    match env {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert!(ref_id.is_none());
            assert!(error.contains("invalid json"));
            assert!(error_code.is_none());
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn parse_fatal_with_ref_id() {
    let json = r#"{"t":"fatal","ref_id":"run-1","error":"run failed: adapter error"}"#;
    let env = must_decode(json);
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert!(error.contains("run failed"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn encode_hello_contains_discriminator_t() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
    assert!(json.contains(&format!(r#""contract_version":"{CONTRACT_VERSION}""#)));
    assert!(json.ends_with('\n'));
}

#[test]
fn encode_fatal_contains_discriminator_t() {
    let env = Envelope::Fatal {
        ref_id: Some("r-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
    assert!(json.contains(r#""error":"boom""#));
}

#[test]
fn roundtrip_hello_encoding() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "roundtrip-test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = must_decode(json.trim());
    match decoded {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "roundtrip-test");
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn roundtrip_run_envelope() {
    let wo = WorkOrderBuilder::new("test task").build();
    let env = Envelope::Run {
        id: "run-42".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = must_decode(json.trim());
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-42");
            assert_eq!(work_order.task, "test task");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn roundtrip_final_with_receipt() {
    let receipt = ReceiptBuilder::new("mock").build();
    let env = Envelope::Final {
        ref_id: "run-42".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = must_decode(json.trim());
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-42");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

// =========================================================================
// 2. Version negotiation (10+ tests)
// =========================================================================

#[test]
fn parse_version_current() {
    assert_eq!(parse_version(CONTRACT_VERSION), Some((0, 1)));
}

#[test]
fn parse_version_v2_3() {
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid_format() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn parse_version_non_numeric() {
    assert_eq!(parse_version("abp/va.b"), None);
    assert_eq!(parse_version("abp/v1.x"), None);
}

#[test]
fn compatible_versions_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.99", "abp/v0.1"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v2.0", "abp/v3.0"));
}

#[test]
fn incompatible_when_unparseable() {
    assert!(!is_compatible_version("garbage", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
    assert!(!is_compatible_version("garbage", "garbage"));
}

#[test]
fn protocol_version_parse_and_display() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    assert_eq!(format!("{v}"), "abp/v0.1");
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn negotiate_version_same() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v01, &v01).unwrap();
    assert_eq!(result, v01);
}

#[test]
fn negotiate_version_picks_minimum_minor() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result, v01);
}

#[test]
fn negotiate_version_rejects_different_major() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = negotiate_version(&v01, &v10).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.1").unwrap()));
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.2").unwrap()));
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.3").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.0").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.4").unwrap()));
}

#[test]
fn version_range_rejects_cross_major() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(!range.is_compatible(&ProtocolVersion::parse("abp/v1.1").unwrap()));
}

// =========================================================================
// 3. Streaming semantics (10+ tests)
// =========================================================================

#[test]
fn decode_stream_multiple_lines() {
    let input = format!(
        "{}\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"e1"}"#,
        r#"{"t":"fatal","ref_id":null,"error":"e2"}"#,
    );
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_skips_empty_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"e1\"}\n\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"e2\"}\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_skips_whitespace_only_lines() {
    let input = "   \n\t\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\n  \t  \n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn decode_stream_unicode_values() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_message","text":"こんにちは世界 🌍"}}"#;
    let input = format!("{json}\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
    match &envelopes[0] {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains("こんにちは"));
                assert!(text.contains('🌍'));
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn decode_stream_special_chars_in_error() {
    let json =
        r#"{"t":"fatal","ref_id":null,"error":"path: C:\\Users\\test\\file.txt\nnewline\ttab"}"#;
    let env = must_decode(json);
    match env {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("C:\\Users\\test\\file.txt"));
            assert!(error.contains('\n'));
            assert!(error.contains('\t'));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn decode_stream_emoji_in_message() {
    let json = r#"{"t":"event","ref_id":"r","event":{"ts":"2025-01-01T00:00:00Z","type":"warning","message":"⚠️ Deprecated API"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(&event.kind, AgentEventKind::Warning { message } if message.contains("⚠️"))
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn encode_produces_single_newline_terminated_line() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    // Must end with exactly one newline
    assert!(encoded.ends_with('\n'));
    assert!(!encoded.ends_with("\n\n"));
    // No embedded newlines in the JSON part
    assert_eq!(encoded.trim_end().matches('\n').count(), 0);
}

#[test]
fn decode_stream_full_session_flow() {
    // Simulate a complete hello → event → event → final session
    let hello = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let ev1 = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"run_started","message":"start"}}"#;
    let ev2 = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:01Z","type":"assistant_message","text":"hi"}}"#;
    let fin = r#"{"t":"final","ref_id":"r1","receipt":{"meta":{"run_id":"00000000-0000-0000-0000-000000000000","work_order_id":"00000000-0000-0000-0000-000000000000","contract_version":"abp/v0.1","started_at":"2025-01-01T00:00:00Z","finished_at":"2025-01-01T00:00:01Z","duration_ms":1000},"backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"usage_raw":{},"usage":{"input_tokens":null,"output_tokens":null,"cache_read_tokens":null,"cache_write_tokens":null,"request_units":null,"estimated_cost_usd":null},"trace":[],"artifacts":[],"verification":{"git_diff":null,"git_status":null,"harness_ok":true},"outcome":"complete","receipt_sha256":null}}"#;

    let input = format!("{hello}\n{ev1}\n{ev2}\n{fin}\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 4);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Event { .. }));
    assert!(matches!(envelopes[2], Envelope::Event { .. }));
    assert!(matches!(envelopes[3], Envelope::Final { .. }));
}

#[test]
fn decode_stream_returns_error_for_bad_line() {
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\nnot valid json\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<Result<Envelope, ProtocolError>> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let envelopes: Vec<Result<Envelope, ProtocolError>> =
        JsonlCodec::decode_stream(reader).collect();
    assert!(envelopes.is_empty());
}

// =========================================================================
// 4. Forward compatibility (15+ tests)
// =========================================================================

#[test]
fn unknown_envelope_type_returns_error() {
    let json = r#"{"t":"ping","payload":"test"}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn unknown_envelope_type_does_not_panic() {
    let json = r#"{"t":"future_type","data":123}"#;
    let result = std::panic::catch_unwind(|| JsonlCodec::decode(json));
    assert!(result.is_ok()); // no panic
    assert!(result.unwrap().is_err()); // returns Err
}

#[test]
fn extra_fields_in_hello_ignored() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"extra_field":"ignored","another":42}"#;
    let env = must_decode(json);
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn extra_fields_in_event_ignored() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_message","text":"hi","extra":"ignored"},"debug_info":"also_ignored"}"#;
    let env = must_decode(json);
    assert!(matches!(env, Envelope::Event { .. }));
}

#[test]
fn extra_fields_in_fatal_ignored() {
    let json = r#"{"t":"fatal","ref_id":"r1","error":"boom","stack_trace":"...","retry_after":30}"#;
    let env = must_decode(json);
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error, "boom"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn extra_fields_in_backend_identity_ignored() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":"1.0","adapter_version":null,"display_name":"Test Backend","region":"us-east-1"},"capabilities":{}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "test");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0"));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn missing_optional_backend_version() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"minimal","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Hello { backend, .. } => {
            assert!(backend.backend_version.is_none());
            assert!(backend.adapter_version.is_none());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn missing_optional_mode_defaults_to_mapped() {
    // When mode field is absent, it defaults to Mapped
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn explicit_passthrough_mode() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"passthrough"}"#;
    let env = must_decode(json);
    match env {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn empty_capabilities_map_accepted() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"bare","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.is_empty());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn missing_optional_tool_use_id_in_tool_call() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"tool_call","tool_name":"bash","tool_use_id":null,"parent_tool_use_id":null,"input":{"command":"ls"}}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolCall { tool_use_id, .. } => {
                assert!(tool_use_id.is_none());
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_ext_field_preserved() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_message","text":"hi","ext":{"raw_message":{"role":"assistant","content":"hi"}}}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            let ext = event.ext.as_ref().unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_ext_absent_is_none() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_message","text":"no ext"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_none());
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn fatal_error_code_optional() {
    // Without error_code
    let json1 = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env1 = must_decode(json1);
    assert!(matches!(
        env1,
        Envelope::Fatal {
            error_code: None,
            ..
        }
    ));

    // With error_code
    let json2 =
        r#"{"t":"fatal","ref_id":null,"error":"boom","error_code":"protocol_invalid_envelope"}"#;
    let env2 = must_decode(json2);
    match env2 {
        Envelope::Fatal { error_code, .. } => {
            assert!(error_code.is_some());
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn error_code_not_serialized_when_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(!json.contains("error_code"));
}

#[test]
fn parse_node_sidecar_final_receipt() {
    // Structure matching the Node sidecar receipt format
    let json = r#"{"t":"final","ref_id":"run-abc","receipt":{"meta":{"run_id":"00000000-0000-0000-0000-000000000001","work_order_id":"00000000-0000-0000-0000-000000000002","contract_version":"abp/v0.1","started_at":"2025-06-01T10:00:00Z","finished_at":"2025-06-01T10:00:01Z","duration_ms":1000},"backend":{"id":"example_node_sidecar","backend_version":"v20.0.0","adapter_version":"0.1"},"capabilities":{"streaming":"native"},"usage_raw":{"note":"example_node_sidecar"},"usage":{"input_tokens":null,"output_tokens":null,"cache_read_tokens":null,"cache_write_tokens":null,"request_units":null,"estimated_cost_usd":null},"trace":[],"artifacts":[],"verification":{"git_diff":null,"git_status":null,"harness_ok":true},"outcome":"complete","receipt_sha256":null}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-abc");
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert_eq!(receipt.backend.id, "example_node_sidecar");
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
            assert!(receipt.receipt_sha256.is_none());
            assert!(receipt.verification.harness_ok);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn event_warning_type() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"warning","message":"something is deprecated"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(&event.kind, AgentEventKind::Warning { message } if message.contains("deprecated"))
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_error_type() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"error","message":"something failed"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(&event.kind, AgentEventKind::Error { message, .. } if message.contains("failed"))
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_file_changed() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"file_changed","path":"src/main.rs","summary":"added fn main"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "added fn main");
            }
            other => panic!("expected FileChanged, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_command_executed() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"command_executed","command":"cargo test","exit_code":0,"output_preview":"test result: ok"}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(*exit_code, Some(0));
                assert_eq!(output_preview.as_deref(), Some("test result: ok"));
            }
            other => panic!("expected CommandExecuted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn command_executed_null_optional_fields() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"command_executed","command":"echo","exit_code":null,"output_preview":null}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::CommandExecuted {
                exit_code,
                output_preview,
                ..
            } => {
                assert!(exit_code.is_none());
                assert!(output_preview.is_none());
            }
            other => panic!("expected CommandExecuted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn encode_to_writer_and_decode() {
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "test".into(),
        error_code: None,
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
    let decoded = must_decode(output.trim());
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn encode_many_to_writer_produces_valid_jsonl() {
    let envs = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "e1".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e2".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let reader = BufReader::new(output.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

#[test]
fn decode_invalid_json_returns_protocol_error() {
    let result = JsonlCodec::decode("{malformed");
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn decode_valid_json_but_wrong_shape() {
    let result = JsonlCodec::decode(r#"{"foo":"bar"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_missing_discriminator() {
    let result = JsonlCodec::decode(r#"{"contract_version":"abp/v0.1"}"#);
    assert!(result.is_err());
}

#[test]
fn receipt_outcome_variants_serde() {
    for (s, expected) in [
        ("\"complete\"", Outcome::Complete),
        ("\"partial\"", Outcome::Partial),
        ("\"failed\"", Outcome::Failed),
    ] {
        let o: Outcome = serde_json::from_str(s).unwrap();
        assert_eq!(o, expected);
    }
}

#[test]
fn execution_mode_serde() {
    let mapped: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(mapped, ExecutionMode::Mapped);
    let passthrough: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
    assert_eq!(passthrough, ExecutionMode::Passthrough);
}

#[test]
fn hello_with_passthrough_mode_roundtrip() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt-test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""mode":"passthrough""#));
    let decoded = must_decode(json.trim());
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn fatal_with_error_code_roundtrip() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "bad envelope",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("error_code"));
    let decoded = must_decode(json.trim());
    assert!(decoded.error_code().is_some());
}

#[test]
fn protocol_error_codes_from_variants() {
    let violation = ProtocolError::Violation("test".into());
    assert_eq!(
        violation.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );

    let unexpected = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        unexpected.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn version_error_display() {
    let err = VersionError::InvalidFormat;
    let msg = format!("{err}");
    assert!(msg.contains("invalid version format"));
}

#[test]
fn parse_version_large_numbers() {
    assert_eq!(parse_version("abp/v999.999"), Some((999, 999)));
}

#[test]
fn parse_version_zero_zero() {
    assert_eq!(parse_version("abp/v0.0"), Some((0, 0)));
}

#[test]
fn decode_event_with_deeply_nested_tool_input() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"tool_call","tool_name":"edit","tool_use_id":"tu-99","parent_tool_use_id":"tu-50","input":{"path":"src/lib.rs","changes":[{"old":"fn a()","new":"fn b()"},{"old":"fn c()","new":"fn d()"}]}}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                parent_tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "edit");
                assert_eq!(parent_tool_use_id.as_deref(), Some("tu-50"));
                assert!(input["changes"].is_array());
                assert_eq!(input["changes"].as_array().unwrap().len(), 2);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn extra_fields_in_receipt_meta_ignored() {
    let json = r#"{"t":"final","ref_id":"r1","receipt":{"meta":{"run_id":"00000000-0000-0000-0000-000000000001","work_order_id":"00000000-0000-0000-0000-000000000002","contract_version":"abp/v0.1","started_at":"2025-01-01T00:00:00Z","finished_at":"2025-01-01T00:00:01Z","duration_ms":500,"extra_meta":"ignored"},"backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"usage_raw":{},"usage":{"input_tokens":null,"output_tokens":null,"cache_read_tokens":null,"cache_write_tokens":null,"request_units":null,"estimated_cost_usd":null},"trace":[],"artifacts":[],"verification":{"git_diff":null,"git_status":null,"harness_ok":true},"outcome":"complete","receipt_sha256":null}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.meta.duration_ms, 500);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn tool_result_with_complex_output() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"tool_result","tool_name":"search","tool_use_id":"tu-5","output":{"matches":[{"file":"a.rs","line":10},{"file":"b.rs","line":20}],"total":2},"is_error":false}}"#;
    let env = must_decode(json);
    match &env {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert!(output.is_object());
                assert_eq!(output["total"], 2);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn receipt_with_mode_field() {
    let json = r#"{"t":"final","ref_id":"r1","receipt":{"meta":{"run_id":"00000000-0000-0000-0000-000000000001","work_order_id":"00000000-0000-0000-0000-000000000002","contract_version":"abp/v0.1","started_at":"2025-01-01T00:00:00Z","finished_at":"2025-01-01T00:00:01Z","duration_ms":100},"backend":{"id":"py","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"passthrough","usage_raw":{},"usage":{"input_tokens":null,"output_tokens":null,"cache_read_tokens":null,"cache_write_tokens":null,"request_units":null,"estimated_cost_usd":null},"trace":[],"artifacts":[],"verification":{"git_diff":null,"git_status":null,"harness_ok":true},"outcome":"complete","receipt_sha256":null}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.mode, ExecutionMode::Passthrough);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn receipt_mode_defaults_to_mapped() {
    let json = r#"{"t":"final","ref_id":"r1","receipt":{"meta":{"run_id":"00000000-0000-0000-0000-000000000001","work_order_id":"00000000-0000-0000-0000-000000000002","contract_version":"abp/v0.1","started_at":"2025-01-01T00:00:00Z","finished_at":"2025-01-01T00:00:01Z","duration_ms":100},"backend":{"id":"nd","backend_version":null,"adapter_version":null},"capabilities":{},"usage_raw":{},"usage":{"input_tokens":null,"output_tokens":null,"cache_read_tokens":null,"cache_write_tokens":null,"request_units":null,"estimated_cost_usd":null},"trace":[],"artifacts":[],"verification":{"git_diff":null,"git_status":null,"harness_ok":true},"outcome":"complete","receipt_sha256":null}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn usage_normalized_with_values() {
    let json = r#"{"t":"final","ref_id":"r1","receipt":{"meta":{"run_id":"00000000-0000-0000-0000-000000000001","work_order_id":"00000000-0000-0000-0000-000000000002","contract_version":"abp/v0.1","started_at":"2025-01-01T00:00:00Z","finished_at":"2025-01-01T00:00:01Z","duration_ms":100},"backend":{"id":"nd","backend_version":null,"adapter_version":null},"capabilities":{},"usage_raw":{"input_tokens":100},"usage":{"input_tokens":100,"output_tokens":50,"cache_read_tokens":null,"cache_write_tokens":null,"request_units":null,"estimated_cost_usd":0.01},"trace":[],"artifacts":[],"verification":{"git_diff":null,"git_status":null,"harness_ok":true},"outcome":"complete","receipt_sha256":null}}"#;
    let env = must_decode(json);
    match env {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.usage.input_tokens, Some(100));
            assert_eq!(receipt.usage.output_tokens, Some(50));
            assert_eq!(receipt.usage.estimated_cost_usd, Some(0.01));
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn discriminator_field_is_t_not_type() {
    // The envelope discriminator must be "t", not "type"
    let with_type = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(with_type);
    // "type" is not the discriminator, so this should fail to parse as an envelope
    assert!(result.is_err());
}

#[test]
fn event_type_uses_type_not_t() {
    // Inside AgentEvent, the discriminator for AgentEventKind is "type" (not "t")
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"run_started","message":"go"}}"#;
    let env = must_decode(json);
    assert!(matches!(env, Envelope::Event { .. }));
}

#[test]
fn json_with_trailing_whitespace() {
    let json = "  {\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}  ";
    let env = must_decode(json.trim());
    assert!(matches!(env, Envelope::Fatal { .. }));
}
