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
//! Comprehensive conformance test harness validating ABP behavior against the
//! specification: contract stability, receipt correctness, protocol compliance,
//! capability negotiation, execution modes, and error taxonomy.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_capability::{check_capability, negotiate_capabilities, SupportLevel as CapSupportLevel};
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, BackendIdentity,
    Capability, CapabilityManifest, ExecutionMode, MinSupport, Outcome, Receipt, ReceiptBuilder,
    RuntimeConfig, SupportLevel as CoreSupportLevel, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use abp_receipt::{canonicalize, compute_hash, verify_hash};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════

fn fixed_ts() -> chrono::DateTime<Utc> {
    "2025-01-01T00:00:00Z".parse().unwrap()
}

fn sample_events() -> Vec<AgentEvent> {
    vec![
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ]
}

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn receipt_with_events(events: Vec<AgentEvent>) -> Receipt {
    let mut r = ReceiptBuilder::new("conformance")
        .outcome(Outcome::Complete)
        .build();
    r.trace = events;
    r
}

fn manifest_with(caps: &[Capability]) -> CapabilityManifest {
    let mut m = BTreeMap::new();
    for c in caps {
        m.insert(c.clone(), CoreSupportLevel::Native);
    }
    m
}

fn manifest_with_level(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn make_hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_run_envelope(run_id: &str) -> Envelope {
    let wo = WorkOrderBuilder::new("test task").build();
    Envelope::Run {
        id: run_id.into(),
        work_order: wo,
    }
}

fn make_event_envelope(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        },
    }
}

fn make_final_envelope(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: minimal_receipt(),
    }
}

fn make_fatal_envelope(ref_id: Option<&str>) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: "fatal error".into(),
        error_code: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  1. CONTRACT STABILITY
// ═══════════════════════════════════════════════════════════════════════

mod contract_stability {
    use super::*;

    #[test]
    fn contract_version_is_abp_v0_1() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn contract_version_parseable() {
        let parsed = parse_version(CONTRACT_VERSION);
        assert_eq!(parsed, Some((0, 1)));
    }

    #[test]
    fn work_order_roundtrip() {
        let wo = WorkOrderBuilder::new("refactor auth").build();
        let json = serde_json::to_string(&wo).unwrap();
        let wo2: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(wo.task, wo2.task);
        assert_eq!(wo.id, wo2.id);
    }

    #[test]
    fn receipt_roundtrip() {
        let r = minimal_receipt();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r.outcome, r2.outcome);
        assert_eq!(r.backend.id, r2.backend.id);
    }

    #[test]
    fn agent_event_all_variants_serialize() {
        let variants: Vec<AgentEventKind> = vec![
            AgentEventKind::RunStarted {
                message: "s".into(),
            },
            AgentEventKind::RunCompleted {
                message: "d".into(),
            },
            AgentEventKind::AssistantDelta { text: "tok".into() },
            AgentEventKind::AssistantMessage { text: "msg".into() },
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                parent_tool_use_id: None,
                input: json!({}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                output: json!({}),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "a.rs".into(),
                summary: "edit".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: None,
            },
            AgentEventKind::Warning {
                message: "warn".into(),
            },
            AgentEventKind::Error {
                message: "err".into(),
                error_code: Some(ErrorCode::Internal),
            },
        ];
        for kind in variants {
            let event = AgentEvent {
                ts: fixed_ts(),
                kind,
                ext: None,
            };
            let json = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event.ts, back.ts);
        }
    }

    #[test]
    fn agent_event_kind_uses_type_discriminator() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains(r#""type":"run_started"#),
            "AgentEventKind must use 'type' discriminator, got: {json}"
        );
    }

    #[test]
    fn outcome_serialize_snake_case() {
        let json_c = serde_json::to_string(&Outcome::Complete).unwrap();
        let json_p = serde_json::to_string(&Outcome::Partial).unwrap();
        let json_f = serde_json::to_string(&Outcome::Failed).unwrap();
        assert_eq!(json_c, r#""complete""#);
        assert_eq!(json_p, r#""partial""#);
        assert_eq!(json_f, r#""failed""#);
    }

    #[test]
    fn outcome_deserialize_snake_case() {
        let c: Outcome = serde_json::from_str(r#""complete""#).unwrap();
        let p: Outcome = serde_json::from_str(r#""partial""#).unwrap();
        let f: Outcome = serde_json::from_str(r#""failed""#).unwrap();
        assert_eq!(c, Outcome::Complete);
        assert_eq!(p, Outcome::Partial);
        assert_eq!(f, Outcome::Failed);
    }

    #[test]
    fn execution_mode_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&ExecutionMode::Passthrough).unwrap(),
            r#""passthrough""#
        );
        assert_eq!(
            serde_json::to_string(&ExecutionMode::Mapped).unwrap(),
            r#""mapped""#
        );
    }

    #[test]
    fn execution_mode_default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn capability_serialize_snake_case() {
        let json = serde_json::to_string(&Capability::ToolRead).unwrap();
        assert_eq!(json, r#""tool_read""#);
    }

    #[test]
    fn support_level_serialize_snake_case() {
        let json_n = serde_json::to_string(&CoreSupportLevel::Native).unwrap();
        let json_e = serde_json::to_string(&CoreSupportLevel::Emulated).unwrap();
        let json_u = serde_json::to_string(&CoreSupportLevel::Unsupported).unwrap();
        assert_eq!(json_n, r#""native""#);
        assert_eq!(json_e, r#""emulated""#);
        assert_eq!(json_u, r#""unsupported""#);
    }

    #[test]
    fn schema_generation_work_order() {
        let schema = schemars::schema_for!(abp_core::WorkOrder);
        let val = serde_json::to_value(&schema).unwrap();
        assert!(val.is_object());
        assert!(val.get("properties").is_some() || val.get("$defs").is_some());
    }

    #[test]
    fn schema_generation_receipt() {
        let schema = schemars::schema_for!(Receipt);
        let val = serde_json::to_value(&schema).unwrap();
        assert!(val.is_object());
    }

    #[test]
    fn schema_generation_agent_event() {
        let schema = schemars::schema_for!(AgentEvent);
        let val = serde_json::to_value(&schema).unwrap();
        assert!(val.is_object());
    }

    #[test]
    fn receipt_meta_embeds_contract_version() {
        let r = minimal_receipt();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn work_order_builder_generates_unique_ids() {
        let wo1 = WorkOrderBuilder::new("a").build();
        let wo2 = WorkOrderBuilder::new("b").build();
        assert_ne!(wo1.id, wo2.id);
    }

    #[test]
    fn receipt_builder_generates_unique_run_ids() {
        let r1 = ReceiptBuilder::new("m").build();
        let r2 = ReceiptBuilder::new("m").build();
        assert_ne!(r1.meta.run_id, r2.meta.run_id);
    }

    #[test]
    fn canonical_json_is_valid_json() {
        let r = minimal_receipt();
        let cj = canonical_json(&r).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&cj).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn sha256_hex_produces_64_char_hex() {
        let h = sha256_hex(b"hello world");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  2. RECEIPT CORRECTNESS
// ═══════════════════════════════════════════════════════════════════════

mod receipt_correctness {
    use super::*;

    #[test]
    fn receipt_hash_is_deterministic() {
        let r = minimal_receipt();
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_hash_is_sha256_hex_length() {
        let r = minimal_receipt();
        let h = compute_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn receipt_hash_hex_only_chars() {
        let r = minimal_receipt();
        let h = compute_hash(&r).unwrap();
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn canonicalize_forces_null_sha256() {
        let mut r = minimal_receipt();
        r.receipt_sha256 = Some("fake".into());
        let j = canonicalize(&r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(v["receipt_sha256"].is_null());
    }

    #[test]
    fn canonicalize_is_deterministic() {
        let r = minimal_receipt();
        assert_eq!(canonicalize(&r).unwrap(), canonicalize(&r).unwrap());
    }

    #[test]
    fn canonicalize_sorts_keys() {
        let mut r = minimal_receipt();
        r.usage_raw = json!({"z": 1, "a": 2});
        let j = canonicalize(&r).unwrap();
        let z_pos = j.find("\"z\"").unwrap();
        let a_pos = j.find("\"a\"").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn with_hash_populates_sha256() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(r.receipt_sha256.is_some());
    }

    #[test]
    fn with_hash_value_matches_compute_hash() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let recomputed = compute_hash(&r).unwrap();
        assert_eq!(r.receipt_sha256.as_ref().unwrap(), &recomputed);
    }

    #[test]
    fn verify_hash_valid_receipt() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn verify_hash_fails_tampered() {
        let mut r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        r.outcome = Outcome::Failed;
        assert!(!verify_hash(&r));
    }

    #[test]
    fn verify_hash_passes_when_none() {
        let r = minimal_receipt();
        assert!(r.receipt_sha256.is_none());
        assert!(verify_hash(&r));
    }

    #[test]
    fn hash_differs_on_outcome_change() {
        let r1 = ReceiptBuilder::new("m").outcome(Outcome::Complete).build();
        let r2 = ReceiptBuilder::new("m").outcome(Outcome::Failed).build();
        assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
    }

    #[test]
    fn hash_differs_on_backend_change() {
        let r1 = ReceiptBuilder::new("a").build();
        let r2 = ReceiptBuilder::new("b").build();
        assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
    }

    #[test]
    fn hash_differs_on_trace_change() {
        let r1 = receipt_with_events(vec![]);
        let r2 = receipt_with_events(sample_events());
        assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
    }

    #[test]
    fn receipt_hash_nullifies_sha256_before_hashing() {
        let mut r = minimal_receipt();
        r.receipt_sha256 = Some("should_be_ignored".into());
        let h1 = compute_hash(&r).unwrap();
        r.receipt_sha256 = None;
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2, "sha256 field must not affect hash");
    }

    #[test]
    fn receipt_hash_function_consistent_with_compute_hash() {
        let r = minimal_receipt();
        let from_core = receipt_hash(&r).unwrap();
        let from_receipt = compute_hash(&r).unwrap();
        assert_eq!(from_core, from_receipt);
    }

    #[test]
    fn fresh_receipt_sha256_is_none() {
        let r = minimal_receipt();
        assert!(r.receipt_sha256.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  3. PROTOCOL COMPLIANCE
// ═══════════════════════════════════════════════════════════════════════

mod protocol_compliance {
    use super::*;

    #[test]
    fn envelope_uses_t_discriminator() {
        let hello = make_hello_envelope();
        let json = serde_json::to_string(&hello).unwrap();
        assert!(
            json.contains(r#""t":"hello"#),
            "Envelope must use 't' not 'type': {json}"
        );
    }

    #[test]
    fn envelope_does_not_use_type_discriminator() {
        let hello = make_hello_envelope();
        let json = serde_json::to_string(&hello).unwrap();
        // "t":"hello" should be present, not "type":"hello"
        assert!(!json.contains(r#""type":"hello"#));
    }

    #[test]
    fn hello_envelope_roundtrip() {
        let hello = make_hello_envelope();
        let json = JsonlCodec::encode(&hello).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn jsonl_encode_newline_terminated() {
        let env = make_hello_envelope();
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn hello_carries_contract_version() {
        let hello = make_hello_envelope();
        if let Envelope::Hello {
            contract_version, ..
        } = hello
        {
            assert_eq!(contract_version, CONTRACT_VERSION);
        } else {
            panic!("expected Hello");
        }
    }

    #[test]
    fn run_envelope_roundtrip() {
        let run = make_run_envelope("run-1");
        let json = JsonlCodec::encode(&run).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Run { .. }));
    }

    #[test]
    fn event_envelope_ref_id_preserved() {
        let env = make_event_envelope("run-42");
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        if let Envelope::Event { ref_id, .. } = decoded {
            assert_eq!(ref_id, "run-42");
        } else {
            panic!("expected Event");
        }
    }

    #[test]
    fn final_envelope_ref_id_preserved() {
        let env = make_final_envelope("run-42");
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        if let Envelope::Final { ref_id, .. } = decoded {
            assert_eq!(ref_id, "run-42");
        } else {
            panic!("expected Final");
        }
    }

    #[test]
    fn fatal_envelope_roundtrip() {
        let env = make_fatal_envelope(Some("run-1"));
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        if let Envelope::Fatal { ref_id, error, .. } = decoded {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "fatal error");
        } else {
            panic!("expected Fatal");
        }
    }

    #[test]
    fn fatal_with_code_carries_error_code() {
        let env =
            Envelope::fatal_with_code(Some("run-x".into()), "timeout", ErrorCode::BackendTimeout);
        assert_eq!(env.error_code(), Some(ErrorCode::BackendTimeout));
    }

    #[test]
    fn fatal_from_abp_error_preserves_code() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        let env = Envelope::fatal_from_abp_error(Some("r1".into()), &err);
        assert_eq!(env.error_code(), Some(ErrorCode::PolicyDenied));
    }

    #[test]
    fn non_fatal_envelopes_have_no_error_code() {
        assert!(make_hello_envelope().error_code().is_none());
        assert!(make_event_envelope("r").error_code().is_none());
        assert!(make_final_envelope("r").error_code().is_none());
    }

    #[test]
    fn decode_stream_reads_multiple_lines() {
        let hello = make_hello_envelope();
        let fatal = make_fatal_envelope(None);
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &[hello, fatal]).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
        assert!(matches!(envelopes[0], Envelope::Hello { .. }));
        assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let input = format!(
            "{}\n\n{}\n",
            JsonlCodec::encode(&make_hello_envelope()).unwrap().trim(),
            JsonlCodec::encode(&make_fatal_envelope(None))
                .unwrap()
                .trim(),
        );
        let reader = BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn invalid_json_returns_protocol_error() {
        let result = JsonlCodec::decode("not json");
        assert!(matches!(result, Err(ProtocolError::Json(_))));
    }

    #[test]
    fn ref_id_correlates_event_to_run() {
        let run_id = "run-99";
        let run = make_run_envelope(run_id);
        let event = make_event_envelope(run_id);
        let fin = make_final_envelope(run_id);

        // All envelopes for the same run share the same ref_id
        if let Envelope::Run { id, .. } = &run {
            assert_eq!(id, run_id);
        }
        if let Envelope::Event { ref_id, .. } = &event {
            assert_eq!(ref_id, run_id);
        }
        if let Envelope::Final { ref_id, .. } = &fin {
            assert_eq!(ref_id, run_id);
        }
    }

    #[test]
    fn version_compatibility_same_major() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    }

    #[test]
    fn version_incompatibility_different_major() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    }

    #[test]
    fn version_parse_invalid_returns_none() {
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version("v0.1"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn envelope_hello_default_mode_is_mapped() {
        let hello = Envelope::hello(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        if let Envelope::Hello { mode, .. } = hello {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
    }

    #[test]
    fn envelope_hello_with_passthrough_mode() {
        let hello = Envelope::hello_with_mode(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        if let Envelope::Hello { mode, .. } = hello {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
    }

    #[test]
    fn protocol_error_has_error_code_for_violation() {
        let err = ProtocolError::Violation("bad".into());
        assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
    }

    #[test]
    fn protocol_error_has_error_code_for_unexpected() {
        let err = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        };
        assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  4. CAPABILITY NEGOTIATION
// ═══════════════════════════════════════════════════════════════════════

mod capability_negotiation {
    use super::*;

    #[test]
    fn native_satisfies_native_min() {
        assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_satisfies_emulated_min() {
        assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_satisfies_emulated_min() {
        assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_does_not_satisfy_native_min() {
        assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn unsupported_never_satisfies_native() {
        assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn unsupported_never_satisfies_emulated() {
        assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_satisfies_emulated_min() {
        let restricted = CoreSupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(restricted.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_does_not_satisfy_native_min() {
        let restricted = CoreSupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(!restricted.satisfies(&MinSupport::Native));
    }

    #[test]
    fn check_capability_native_in_manifest() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let result = check_capability(&manifest, &Capability::Streaming);
        assert!(matches!(result, CapSupportLevel::Native));
    }

    #[test]
    fn check_capability_missing_returns_unsupported() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let result = check_capability(&manifest, &Capability::ExtendedThinking);
        assert!(matches!(result, CapSupportLevel::Unsupported { .. }));
    }

    #[test]
    fn check_capability_emulated_in_manifest() {
        let manifest = manifest_with_level(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
        let result = check_capability(&manifest, &Capability::ToolRead);
        assert!(matches!(result, CapSupportLevel::Emulated { .. }));
    }

    #[test]
    fn check_capability_restricted_in_manifest() {
        let manifest = manifest_with_level(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let result = check_capability(&manifest, &Capability::ToolBash);
        assert!(matches!(result, CapSupportLevel::Restricted { .. }));
    }

    #[test]
    fn negotiate_all_native_is_viable() {
        let manifest = manifest_with(&[Capability::Streaming, Capability::ToolUse]);
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 2);
        assert!(result.emulated.is_empty());
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn negotiate_with_unsupported_not_viable() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let required = vec![Capability::Streaming, Capability::ExtendedThinking];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(!result.is_viable());
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn negotiate_emulated_caps_viable() {
        let manifest = manifest_with_level(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
        let required = vec![Capability::ToolRead];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
        assert!(result.native.is_empty());
        assert_eq!(result.emulated.len(), 1);
    }

    #[test]
    fn negotiate_total_counts_all() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let required = vec![Capability::Streaming, Capability::ImageInput];
        let result = negotiate_capabilities(&required, &manifest);
        assert_eq!(result.total(), 2);
    }

    #[test]
    fn negotiate_unsupported_caps_list() {
        let manifest = manifest_with(&[]);
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        let unsup = result.unsupported_caps();
        assert!(unsup.contains(&Capability::Streaming));
        assert!(unsup.contains(&Capability::ToolUse));
    }

    #[test]
    fn negotiate_empty_requirements_viable() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let result = negotiate_capabilities(&[], &manifest);
        assert!(result.is_viable());
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn negotiate_empty_manifest_all_unsupported() {
        let manifest = CapabilityManifest::new();
        let required = vec![Capability::Streaming];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(!result.is_viable());
        assert_eq!(result.unsupported.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  5. EXECUTION MODES
// ═══════════════════════════════════════════════════════════════════════

mod execution_modes {
    use super::*;

    #[test]
    fn passthrough_mode_serializes_correctly() {
        let mode = ExecutionMode::Passthrough;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""passthrough""#);
    }

    #[test]
    fn mapped_mode_serializes_correctly() {
        let mode = ExecutionMode::Mapped;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""mapped""#);
    }

    #[test]
    fn passthrough_mode_deserializes() {
        let mode: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
        assert_eq!(mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn mapped_mode_deserializes() {
        let mode: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
        assert_eq!(mode, ExecutionMode::Mapped);
    }

    #[test]
    fn default_mode_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn receipt_builder_default_mode_is_mapped() {
        let r = ReceiptBuilder::new("m").build();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn receipt_builder_passthrough_mode() {
        let r = ReceiptBuilder::new("m")
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(r.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn hello_envelope_default_mode() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        if let Envelope::Hello { mode, .. } = env {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
    }

    #[test]
    fn hello_envelope_explicit_passthrough() {
        let env = Envelope::hello_with_mode(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        if let Envelope::Hello { mode, .. } = env {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
    }

    #[test]
    fn mode_from_work_order_config() {
        let mut config = RuntimeConfig::default();
        config
            .vendor
            .insert("abp".into(), json!({"mode": "passthrough"}));
        let wo = WorkOrderBuilder::new("test").config(config).build();
        let mode_val = wo.config.vendor.get("abp").unwrap();
        assert_eq!(mode_val["mode"], "passthrough");
    }

    #[test]
    fn execution_mode_roundtrip_in_receipt() {
        let r = ReceiptBuilder::new("m")
            .mode(ExecutionMode::Passthrough)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn execution_mode_equality() {
        assert_eq!(ExecutionMode::Passthrough, ExecutionMode::Passthrough);
        assert_eq!(ExecutionMode::Mapped, ExecutionMode::Mapped);
        assert_ne!(ExecutionMode::Passthrough, ExecutionMode::Mapped);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  6. ERROR TAXONOMY
// ═══════════════════════════════════════════════════════════════════════

mod error_taxonomy {
    use super::*;

    const ALL_CODES: &[ErrorCode] = &[
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];

    const ALL_CATEGORIES: &[ErrorCategory] = &[
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Mapping,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Internal,
    ];

    #[test]
    fn all_error_codes_have_snake_case_str() {
        for &code in ALL_CODES {
            let s = code.as_str();
            assert!(!s.is_empty(), "as_str() empty for {code:?}");
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "as_str() not snake_case for {code:?}: {s}"
            );
        }
    }

    #[test]
    fn all_error_codes_have_category() {
        for &code in ALL_CODES {
            let _ = code.category();
        }
    }

    #[test]
    fn all_error_codes_have_message() {
        for &code in ALL_CODES {
            let m = code.message();
            assert!(!m.is_empty(), "message() empty for {code:?}");
        }
    }

    #[test]
    fn all_categories_covered_by_codes() {
        let covered: std::collections::BTreeSet<_> = ALL_CODES
            .iter()
            .map(|c| format!("{:?}", c.category()))
            .collect();
        for cat in ALL_CATEGORIES {
            assert!(
                covered.contains(&format!("{cat:?}")),
                "category {cat:?} has no error codes"
            );
        }
    }

    #[test]
    fn error_code_as_str_known_values() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.as_str(),
            "protocol_invalid_envelope"
        );
        assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
        assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
        assert_eq!(ErrorCode::Internal.as_str(), "internal");
        assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
        assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
    }

    #[test]
    fn protocol_codes_have_protocol_category() {
        let proto_codes = &[
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
        ];
        for &code in proto_codes {
            assert_eq!(code.category(), ErrorCategory::Protocol);
        }
    }

    #[test]
    fn backend_codes_have_backend_category() {
        let codes = &[
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::BackendCrashed,
        ];
        for &code in codes {
            assert_eq!(code.category(), ErrorCategory::Backend);
        }
    }

    #[test]
    fn retryable_are_transient_backend_errors() {
        assert!(ErrorCode::BackendUnavailable.is_retryable());
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(ErrorCode::BackendCrashed.is_retryable());
    }

    #[test]
    fn non_transient_errors_not_retryable() {
        let non_retryable = &[
            ErrorCode::PolicyDenied,
            ErrorCode::Internal,
            ErrorCode::ContractVersionMismatch,
            ErrorCode::BackendNotFound,
            ErrorCode::BackendAuthFailed,
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ConfigInvalid,
        ];
        for &code in non_retryable {
            assert!(!code.is_retryable(), "{code:?} should not be retryable");
        }
    }

    #[test]
    fn error_code_serde_roundtrip() {
        for &code in ALL_CODES {
            let json = serde_json::to_string(&code).unwrap();
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(code, back);
        }
    }

    #[test]
    fn error_code_serializes_as_snake_case_string() {
        let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
        assert_eq!(json, r#""backend_timeout""#);
    }

    #[test]
    fn error_category_serde_roundtrip() {
        for &cat in ALL_CATEGORIES {
            let json = serde_json::to_string(&cat).unwrap();
            let back: ErrorCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(cat, back);
        }
    }

    #[test]
    fn abp_error_construction() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
            .with_context("backend", "openai")
            .with_context("timeout_ms", 30_000);
        assert_eq!(err.code, ErrorCode::BackendTimeout);
        assert_eq!(err.message, "timed out");
        assert_eq!(err.context.len(), 2);
    }

    #[test]
    fn abp_error_to_info() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        let info = err.to_info();
        assert_eq!(info.code, ErrorCode::PolicyDenied);
        assert_eq!(info.message, "denied");
    }

    #[test]
    fn error_info_serde_roundtrip() {
        let info =
            ErrorInfo::new(ErrorCode::BackendTimeout, "timeout").with_detail("backend", "openai");
        let json = serde_json::to_string(&info).unwrap();
        let back: ErrorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn error_info_retryable_from_code() {
        let info = ErrorInfo::new(ErrorCode::BackendTimeout, "t");
        assert!(info.is_retryable);
        let info2 = ErrorInfo::new(ErrorCode::PolicyDenied, "d");
        assert!(!info2.is_retryable);
    }

    #[test]
    fn abp_error_dto_from_abp_error() {
        let err = AbpError::new(ErrorCode::Internal, "broke").with_context("key", "value");
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.code, ErrorCode::Internal);
        assert_eq!(dto.message, "broke");
        assert!(dto.context.contains_key("key"));
    }

    #[test]
    fn error_display_includes_code_str() {
        let err = AbpError::new(ErrorCode::ReceiptHashMismatch, "hash mismatch");
        let display = err.to_string();
        assert!(
            display.contains(ErrorCode::ReceiptHashMismatch.as_str()),
            "Display should contain error code string: {display}"
        );
    }

    #[test]
    fn all_as_str_values_unique() {
        let strs: Vec<&str> = ALL_CODES.iter().map(|c| c.as_str()).collect();
        let unique: std::collections::BTreeSet<&str> = strs.iter().copied().collect();
        assert_eq!(strs.len(), unique.len(), "as_str() values must be unique");
    }
}
