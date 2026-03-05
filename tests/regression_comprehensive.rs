#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive regression tests guarding exact behavior guarantees.

use std::collections::BTreeMap;
use std::path::Path;

use abp_capability::{
    check_capability, negotiate, negotiate_capabilities, NegotiationResult,
    SupportLevel as CapSupportLevel,
};
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RuntimeConfig, SupportLevel as CoreSupportLevel, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec};
use abp_receipt::{self, verify_hash, ReceiptBuilder as ReceiptReceiptBuilder};
use chrono::Utc;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Helper: build a receipt with fixed timestamps for deterministic hashing
// ═══════════════════════════════════════════════════════════════════════════

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build()
}

fn make_fixed_receipt() -> Receipt {
    let ts = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    abp_receipt::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build()
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 1: Receipt hashing regression (18 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rh_sha256_field_null_before_hashing() {
    // The receipt_sha256 field must be set to null before hashing
    // (self-referential prevention).
    let base = make_receipt();
    let mut with_value = base.clone();
    with_value.receipt_sha256 = Some("some_existing_hash".into());
    assert_eq!(
        receipt_hash(&base).unwrap(),
        receipt_hash(&with_value).unwrap(),
        "receipt_hash must ignore receipt_sha256 field"
    );
}

#[test]
fn rh_same_receipt_same_hash() {
    let r = make_fixed_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn rh_hash_format_64_hex() {
    let h = receipt_hash(&make_receipt()).unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex must be exactly 64 characters");
    assert!(
        h.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be lowercase hex"
    );
}

#[test]
fn rh_hash_is_lowercase_hex() {
    let h = receipt_hash(&make_receipt()).unwrap();
    assert_eq!(h, h.to_lowercase(), "hash must use lowercase hex");
}

#[test]
fn rh_hash_changes_on_outcome_change() {
    let r1 = make_fixed_receipt();
    let mut r2 = make_fixed_receipt();
    r2.outcome = Outcome::Failed;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn rh_hash_changes_on_backend_change() {
    let r1 = make_fixed_receipt();
    let mut r2 = make_fixed_receipt();
    r2.backend.id = "different-backend".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn rh_hash_changes_on_mode_change() {
    let r1 = make_fixed_receipt();
    let mut r2 = make_fixed_receipt();
    r2.mode = ExecutionMode::Passthrough;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn rh_hash_changes_on_trace_change() {
    let r1 = make_fixed_receipt();
    let mut r2 = make_fixed_receipt();
    r2.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "test".into(),
        },
        ext: None,
    });
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn rh_empty_receipt_valid_hash() {
    // Receipt with all defaults still produces a valid hash.
    let r = make_receipt();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn rh_receipt_with_all_optional_fields() {
    let ts = Utc::now();
    let r = abp_receipt::ReceiptBuilder::new("full-backend")
        .outcome(Outcome::Partial)
        .backend_version("2.0.0")
        .adapter_version("1.0.0")
        .work_order_id(Uuid::new_v4())
        .started_at(ts)
        .finished_at(ts + chrono::Duration::seconds(5))
        .usage_tokens(100, 200)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: Some(BTreeMap::new()),
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .verification(VerificationReport {
            git_diff: Some("diff here".into()),
            git_status: Some("M src/lib.rs".into()),
            harness_ok: true,
        })
        .build();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn rh_verify_hash_roundtrips() {
    let r = make_fixed_receipt();
    let mut hashed = r.clone();
    hashed.receipt_sha256 = Some(receipt_hash(&hashed).unwrap());
    assert!(verify_hash(&hashed));
}

#[test]
fn rh_verify_hash_detects_tamper() {
    let mut r = make_fixed_receipt();
    r.receipt_sha256 = Some(receipt_hash(&r).unwrap());
    r.outcome = Outcome::Failed; // tamper
    assert!(!verify_hash(&r));
}

#[test]
fn rh_verify_hash_none_is_valid() {
    let r = make_receipt();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn rh_verify_hash_bogus_string() {
    let mut r = make_receipt();
    r.receipt_sha256 = Some("not-a-valid-hash".into());
    assert!(!verify_hash(&r));
}

#[test]
fn rh_with_hash_idempotent() {
    let r = make_receipt().with_hash().unwrap();
    let h1 = r.receipt_sha256.clone().unwrap();
    let r2 = r.with_hash().unwrap();
    assert_eq!(h1, r2.receipt_sha256.unwrap());
}

#[test]
fn rh_with_hash_produces_some() {
    let r = make_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn rh_deterministic_after_json_roundtrip() {
    let r = make_fixed_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt_hash(&r).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn rh_receipt_hash_ignores_different_sha256_values() {
    let base = make_fixed_receipt();
    let mut a = base.clone();
    a.receipt_sha256 = Some("aaaa".into());
    let mut b = base.clone();
    b.receipt_sha256 = Some("bbbb".into());
    assert_eq!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 2: Contract version regression (12 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cv_exact_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn cv_not_v1() {
    assert_ne!(CONTRACT_VERSION, "abp/v1.0");
}

#[test]
fn cv_not_plain_version() {
    assert_ne!(CONTRACT_VERSION, "0.1");
    assert_ne!(CONTRACT_VERSION, "v0.1");
}

#[test]
fn cv_starts_with_abp_prefix() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
}

#[test]
fn cv_used_in_receipt_metadata() {
    let r = make_receipt();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn cv_used_in_hello_envelope() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn cv_parse_version_accepts() {
    assert_eq!(parse_version(CONTRACT_VERSION), Some((0, 1)));
}

#[test]
fn cv_parse_version_rejects_malformed() {
    assert_eq!(parse_version("garbage"), None);
    assert_eq!(parse_version(""), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/v."), None);
    assert_eq!(parse_version("abp/va.b"), None);
}

#[test]
fn cv_parse_version_higher_versions() {
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn cv_compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.99"));
}

#[test]
fn cv_incompatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn cv_incompatible_malformed() {
    assert!(!is_compatible_version("garbage", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 3: Serde regression (22 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_envelope_tag_is_t() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
    assert!(!json.contains(r#""type":"fatal""#));
}

#[test]
fn serde_envelope_decode_rejects_type_tag() {
    let bad = r#"{"type":"fatal","ref_id":null,"error":"boom"}"#;
    assert!(JsonlCodec::decode(bad).is_err());
}

#[test]
fn serde_agent_event_kind_tag_is_type() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        json.contains(r#""type":"assistant_message""#),
        "AgentEventKind must use tag=\"type\": {json}"
    );
}

#[test]
fn serde_agent_event_kind_not_t_tag() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains(r#""t":"run_started""#));
}

#[test]
fn serde_rename_all_snake_case_outcome() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(json, r#""complete""#);
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    assert_eq!(json, r#""partial""#);
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    assert_eq!(json, r#""failed""#);
}

#[test]
fn serde_rename_all_snake_case_execution_mode() {
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
fn serde_rename_all_snake_case_execution_lane() {
    assert_eq!(
        serde_json::to_string(&ExecutionLane::PatchFirst).unwrap(),
        r#""patch_first""#
    );
    assert_eq!(
        serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap(),
        r#""workspace_first""#
    );
}

#[test]
fn serde_rename_all_snake_case_workspace_mode() {
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::PassThrough).unwrap(),
        r#""pass_through""#
    );
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::Staged).unwrap(),
        r#""staged""#
    );
}

#[test]
fn serde_rename_all_snake_case_capability() {
    assert_eq!(
        serde_json::to_string(&Capability::ToolRead).unwrap(),
        r#""tool_read""#
    );
    assert_eq!(
        serde_json::to_string(&Capability::ExtendedThinking).unwrap(),
        r#""extended_thinking""#
    );
    assert_eq!(
        serde_json::to_string(&Capability::HooksPreToolUse).unwrap(),
        r#""hooks_pre_tool_use""#
    );
}

#[test]
fn serde_rename_all_snake_case_support_level() {
    assert_eq!(
        serde_json::to_string(&CoreSupportLevel::Native).unwrap(),
        r#""native""#
    );
    assert_eq!(
        serde_json::to_string(&CoreSupportLevel::Emulated).unwrap(),
        r#""emulated""#
    );
    assert_eq!(
        serde_json::to_string(&CoreSupportLevel::Unsupported).unwrap(),
        r#""unsupported""#
    );
}

#[test]
fn serde_btreemap_deterministic_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, CoreSupportLevel::Native);
    caps.insert(Capability::ToolRead, CoreSupportLevel::Native);
    caps.insert(Capability::Streaming, CoreSupportLevel::Emulated);
    let json1 = serde_json::to_string(&caps).unwrap();
    let json2 = serde_json::to_string(&caps).unwrap();
    assert_eq!(json1, json2);
    // Keys should be sorted alphabetically
    let streaming_pos = json1.find("streaming").unwrap();
    let tool_read_pos = json1.find("tool_read").unwrap();
    assert!(streaming_pos < tool_read_pos);
}

#[test]
fn serde_btreemap_deterministic_vendor() {
    let mut vendor = BTreeMap::new();
    vendor.insert("z".to_string(), serde_json::json!(1));
    vendor.insert("a".to_string(), serde_json::json!(2));
    let json = serde_json::to_string(&vendor).unwrap();
    assert!(json.find("\"a\"").unwrap() < json.find("\"z\"").unwrap());
}

#[test]
fn serde_default_execution_mode_omitted_deserializes() {
    // ExecutionMode has #[serde(default)], so missing field should work
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn serde_optional_none_omitted_from_json() {
    let r = make_receipt();
    let json = serde_json::to_string(&r).unwrap();
    // receipt_sha256 is None, and since it's Option<String> it should serialize as null
    // (serde default for Option is to include null)
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("receipt_sha256").is_some()); // field exists
    assert!(v["receipt_sha256"].is_null()); // as null
}

#[test]
fn serde_error_code_omitted_when_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(
        !json.contains("error_code"),
        "error_code should be skipped when None"
    );
}

#[test]
fn serde_envelope_hello_variant_name() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn serde_envelope_all_variant_names_snake_case() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(JsonlCodec::encode(&hello)
        .unwrap()
        .contains(r#""t":"hello""#));

    let run_env = Envelope::Run {
        id: "r".into(),
        work_order: WorkOrderBuilder::new("t").build(),
    };
    assert!(JsonlCodec::encode(&run_env)
        .unwrap()
        .contains(r#""t":"run""#));

    let final_env = Envelope::Final {
        ref_id: "r".into(),
        receipt: make_receipt(),
    };
    assert!(JsonlCodec::encode(&final_env)
        .unwrap()
        .contains(r#""t":"final""#));

    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "e".into(),
        error_code: None,
    };
    assert!(JsonlCodec::encode(&fatal)
        .unwrap()
        .contains(r#""t":"fatal""#));
}

#[test]
fn serde_agent_event_kind_all_variants_snake_case() {
    let variants: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted { message: "".into() },
        AgentEventKind::RunCompleted { message: "".into() },
        AgentEventKind::AssistantDelta { text: "".into() },
        AgentEventKind::AssistantMessage { text: "".into() },
        AgentEventKind::Warning { message: "".into() },
        AgentEventKind::Error {
            message: "".into(),
            error_code: None,
        },
        AgentEventKind::FileChanged {
            path: "".into(),
            summary: "".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "".into(),
            exit_code: None,
            output_preview: None,
        },
    ];
    let expected = [
        "run_started",
        "run_completed",
        "assistant_delta",
        "assistant_message",
        "warning",
        "error",
        "file_changed",
        "command_executed",
    ];
    for (kind, expected_name) in variants.into_iter().zip(expected.iter()) {
        let event = AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let needle = format!(r#""type":"{}""#, expected_name);
        assert!(json.contains(&needle), "Expected {needle} in {json}");
    }
}

#[test]
fn serde_canonical_json_sorts_keys() {
    let v = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let s = canonical_json(&v).unwrap();
    assert!(s.find("\"a\"").unwrap() < s.find("\"m\"").unwrap());
    assert!(s.find("\"m\"").unwrap() < s.find("\"z\"").unwrap());
}

#[test]
fn serde_ext_field_omitted_when_none() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("ext"), "ext=None should be skipped");
}

#[test]
fn serde_roundtrip_receipt_preserves_all_fields() {
    let r = make_receipt().with_hash().unwrap();
    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.meta.contract_version, r2.meta.contract_version);
    assert_eq!(r.mode, r2.mode);
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 4: Protocol regression (17 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn proto_hello_uses_contract_version() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""contract_version":"abp/v0.1""#));
}

#[test]
fn proto_run_includes_work_order() {
    let wo = WorkOrderBuilder::new("do stuff").build();
    let wo_id = wo.id;
    let env = Envelope::Run {
        id: "run-42".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(&wo_id.to_string()));
    assert!(json.contains("do stuff"));
}

#[test]
fn proto_event_ref_id_correlates() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-42".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""ref_id":"run-42""#));
}

#[test]
fn proto_final_includes_receipt() {
    let r = make_receipt();
    let env = Envelope::Final {
        ref_id: "run-42".into(),
        receipt: r,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#));
    assert!(json.contains(r#""ref_id":"run-42""#));
    assert!(json.contains("receipt_sha256"));
}

#[test]
fn proto_fatal_includes_error() {
    let env = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("out of memory"));
    assert!(json.contains(r#""ref_id":"run-42""#));
}

#[test]
fn proto_fatal_with_error_code() {
    let env =
        Envelope::fatal_with_code(Some("run-1".into()), "timed out", ErrorCode::BackendTimeout);
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("backend_timeout"));
    assert!(json.contains("timed out"));
}

#[test]
fn proto_fatal_ref_id_optional() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "early failure".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""ref_id":null"#));
}

#[test]
fn proto_encode_ends_with_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    assert!(JsonlCodec::encode(&env).unwrap().ends_with('\n'));
}

#[test]
fn proto_decode_empty_fails() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn proto_decode_random_json_fails() {
    assert!(JsonlCodec::decode(r#"{"foo":"bar"}"#).is_err());
}

#[test]
fn proto_decode_array_fails() {
    assert!(JsonlCodec::decode("[1,2,3]").is_err());
}

#[test]
fn proto_roundtrip_hello() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    let re = JsonlCodec::encode(&decoded).unwrap();
    assert_eq!(encoded, re);
}

#[test]
fn proto_roundtrip_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: "err".into(),
        error_code: Some(ErrorCode::Internal),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    let re = JsonlCodec::encode(&decoded).unwrap();
    assert_eq!(encoded, re);
}

#[test]
fn proto_decode_stream_skips_blank_lines() {
    let input = "\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\n\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let envs: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envs.len(), 2);
}

#[test]
fn proto_hello_default_mode_is_mapped() {
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
fn proto_hello_with_explicit_mode() {
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
fn proto_error_code_method() {
    let env = Envelope::fatal_with_code(None, "err", ErrorCode::BackendCrashed);
    assert_eq!(env.error_code(), Some(ErrorCode::BackendCrashed));

    let env2 = Envelope::Fatal {
        ref_id: None,
        error: "e".into(),
        error_code: None,
    };
    assert_eq!(env2.error_code(), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 5: Error code regression (18 tests)
// ═══════════════════════════════════════════════════════════════════════════

const ALL_ERROR_CODES: &[ErrorCode] = &[
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

#[test]
fn err_all_as_str_are_snake_case() {
    for code in ALL_ERROR_CODES {
        let s = code.as_str();
        assert!(!s.is_empty(), "{code:?} has empty as_str()");
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{code:?}.as_str() = {s:?} is not snake_case"
        );
    }
}

#[test]
fn err_as_str_unique() {
    let mut seen = std::collections::HashSet::new();
    for code in ALL_ERROR_CODES {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str: {s}");
    }
}

#[test]
fn err_display_is_human_message() {
    // Display for ErrorCode returns the human message, not the code string
    let display = format!("{}", ErrorCode::BackendTimeout);
    assert_eq!(display, "backend timed out");
}

#[test]
fn err_abp_error_display_format() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    let display = err.to_string();
    assert_eq!(display, "[backend_not_found] no such backend");
}

#[test]
fn err_abp_error_display_with_context() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    let display = err.to_string();
    assert!(display.starts_with("[backend_timeout] timed out"));
    assert!(display.contains("openai"));
}

#[test]
fn err_error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "denied by policy");
    assert_eq!(info.to_string(), "[policy_denied] denied by policy");
}

#[test]
fn err_category_protocol() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolMissingRefId.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn err_category_backend() {
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
    assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
    assert_eq!(
        ErrorCode::BackendRateLimited.category(),
        ErrorCategory::Backend
    );
}

#[test]
fn err_category_policy() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn err_category_workspace() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.category(),
        ErrorCategory::Workspace
    );
}

#[test]
fn err_category_receipt() {
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.category(),
        ErrorCategory::Receipt
    );
}

#[test]
fn err_category_contract() {
    assert_eq!(
        ErrorCode::ContractVersionMismatch.category(),
        ErrorCategory::Contract
    );
    assert_eq!(
        ErrorCode::ContractSchemaViolation.category(),
        ErrorCategory::Contract
    );
    assert_eq!(
        ErrorCode::ContractInvalidReceipt.category(),
        ErrorCategory::Contract
    );
}

#[test]
fn err_category_internal() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn err_retryable_codes() {
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(ErrorCode::BackendCrashed.is_retryable());
}

#[test]
fn err_non_retryable_codes() {
    assert!(!ErrorCode::BackendNotFound.is_retryable());
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::Internal.is_retryable());
    assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
}

#[test]
fn err_abp_error_category_shorthand() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(err.category(), ErrorCategory::Policy);
}

#[test]
fn err_error_code_serde_roundtrip() {
    for code in ALL_ERROR_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back);
    }
}

#[test]
fn err_error_code_serializes_as_snake_case_string() {
    let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 6: Capability regression (14 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cap_native_satisfies_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn cap_native_satisfies_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn cap_emulated_does_not_satisfy_native() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn cap_emulated_satisfies_emulated() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn cap_restricted_satisfies_emulated() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

#[test]
fn cap_restricted_does_not_satisfy_native() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
}

#[test]
fn cap_unsupported_satisfies_nothing() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn cap_manifest_is_btreemap() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolWrite, CoreSupportLevel::Native);
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    m.insert(Capability::ToolRead, CoreSupportLevel::Native);
    let keys: Vec<_> = m.keys().collect();
    // BTreeMap should sort: Streaming < ToolRead < ToolWrite
    assert!(keys.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn cap_negotiate_native_in_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    let result = negotiate_capabilities(&[Capability::Streaming], &manifest);
    assert!(result.is_viable());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn cap_negotiate_unsupported_not_viable() {
    let manifest = CapabilityManifest::new(); // empty
    let result = negotiate_capabilities(&[Capability::Streaming], &manifest);
    assert!(!result.is_viable());
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn cap_negotiate_emulated_in_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, CoreSupportLevel::Emulated);
    let result = negotiate_capabilities(&[Capability::ToolRead], &manifest);
    assert!(result.is_viable());
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn cap_negotiate_with_min_support() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Emulated);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let result = negotiate(&manifest, &reqs);
    // Emulated does not satisfy Native minimum
    assert!(!result.is_viable());
}

#[test]
fn cap_negotiate_empty_requirements_is_viable() {
    let manifest = CapabilityManifest::new();
    let reqs = CapabilityRequirements { required: vec![] };
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_viable());
    assert_eq!(result.total(), 0);
}

#[test]
fn cap_check_missing_capability_is_unsupported() {
    let manifest = CapabilityManifest::new();
    let level = check_capability(&manifest, &Capability::Streaming);
    assert!(matches!(level, CapSupportLevel::Unsupported { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 7: Workspace regression (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ws_staging_excludes_git_dir() {
    let tmp_src = tempfile::tempdir().unwrap();
    let git_dir = tmp_src.path().join(".git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    std::fs::write(tmp_src.path().join("file.txt"), "content").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // The original .git directory content should NOT be copied
    assert!(
        !ws.path().join(".git").join("HEAD").exists()
            || std::fs::read_to_string(ws.path().join(".git").join("HEAD"))
                .map(|s| !s.contains("ref: refs/heads/main"))
                .unwrap_or(true),
        "original .git/HEAD should not be copied verbatim"
    );
    assert!(ws.path().join("file.txt").exists());
}

#[test]
fn ws_staging_preserves_file_content() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::write(tmp_src.path().join("data.txt"), "exact content 123").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let content = std::fs::read_to_string(ws.path().join("data.txt")).unwrap();
    assert_eq!(content, "exact content 123");
}

#[test]
fn ws_staging_preserves_nested_structure() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp_src.path().join("a").join("b")).unwrap();
    std::fs::write(
        tmp_src.path().join("a").join("b").join("deep.rs"),
        "fn main() {}",
    )
    .unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(ws.path().join("a").join("b").join("deep.rs")).unwrap(),
        "fn main() {}"
    );
}

#[test]
fn ws_git_init_creates_git_dir() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::write(tmp_src.path().join("hello.txt"), "hi").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(
        ws.path().join(".git").exists(),
        "git init should create .git"
    );
}

#[test]
fn ws_git_init_creates_baseline_commit() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::write(tmp_src.path().join("code.rs"), "fn main() {}").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    // git log should show at least one commit
    let output = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(ws.path())
        .output();

    if let Ok(out) = output {
        let log = String::from_utf8_lossy(&out.stdout);
        assert!(
            log.contains("baseline"),
            "baseline commit should exist: {log}"
        );
    }
}

#[test]
fn ws_diff_shows_changes() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::write(tmp_src.path().join("file.txt"), "original").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    // Modify the file in the staged workspace
    std::fs::write(ws.path().join("file.txt"), "modified").unwrap();

    let diff = abp_workspace::WorkspaceManager::git_diff(ws.path());
    if let Some(d) = diff {
        assert!(d.contains("modified") || d.contains("file.txt"));
    }
}

#[test]
fn ws_status_shows_changes() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::write(tmp_src.path().join("file.txt"), "original").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    std::fs::write(ws.path().join("file.txt"), "changed").unwrap();
    let status = abp_workspace::WorkspaceManager::git_status(ws.path());
    if let Some(s) = status {
        assert!(!s.is_empty(), "status should show modified file");
    }
}

#[test]
fn ws_stager_exclude_glob() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::write(tmp_src.path().join("keep.rs"), "keep").unwrap();
    std::fs::write(tmp_src.path().join("skip.log"), "skip").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("skip.log").exists());
}

#[test]
fn ws_stager_include_glob() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp_src.path().join("src")).unwrap();
    std::fs::write(tmp_src.path().join("src").join("lib.rs"), "lib").unwrap();
    std::fs::write(tmp_src.path().join("README.md"), "readme").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .include(vec!["src/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("src").join("lib.rs").exists());
    // README.md should be excluded because it doesn't match include
    assert!(!ws.path().join("README.md").exists());
}

#[test]
fn ws_stager_missing_source_errors() {
    let result = abp_workspace::WorkspaceStager::new()
        .source_root("/nonexistent/path/that/should/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn ws_stager_no_source_errors() {
    let result = abp_workspace::WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn ws_passthrough_uses_original_path() {
    let spec = abp_core::WorkspaceSpec {
        root: "/some/path".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = abp_workspace::WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path().to_str().unwrap(), "/some/path");
}

#[test]
fn ws_empty_source_stages_empty() {
    let tmp_src = tempfile::tempdir().unwrap();
    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Should succeed even with empty source
    assert!(ws.path().exists());
}

#[test]
fn ws_preserves_binary_content() {
    let tmp_src = tempfile::tempdir().unwrap();
    let binary_data: Vec<u8> = (0..255).collect();
    std::fs::write(tmp_src.path().join("binary.bin"), &binary_data).unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let read_back = std::fs::read(ws.path().join("binary.bin")).unwrap();
    assert_eq!(read_back, binary_data);
}

#[test]
fn ws_prepared_workspace_path_method() {
    let tmp_src = tempfile::tempdir().unwrap();
    std::fs::write(tmp_src.path().join("x.txt"), "x").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 8: Policy engine regression (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pol_empty_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("anything").allowed);
    assert!(engine.can_read_path(Path::new("any/path")).allowed);
    assert!(engine.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn pol_disallow_beats_allow() {
    let p = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn pol_deny_read_blocks_path() {
    let p = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn pol_deny_write_blocks_path() {
    let p = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn pol_allowlist_blocks_unlisted_tool() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn pol_glob_patterns_in_tool_rules() {
    let p = PolicyProfile {
        disallowed_tools: vec!["Bash*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_use_tool("BashExec").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn pol_multiple_deny_patterns() {
    let p = PolicyProfile {
        deny_read: vec!["*.env".into(), "*.secret".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    assert!(!engine.can_read_path(Path::new("db.env")).allowed);
    assert!(!engine.can_read_path(Path::new("api.secret")).allowed);
    assert!(engine.can_read_path(Path::new("code.rs")).allowed);
}

#[test]
fn pol_decision_reason_on_deny() {
    let p = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&p).unwrap();
    let d = engine.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 9: Glob pattern regression (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn glob_empty_allows_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
}

#[test]
fn glob_exclude_takes_precedence() {
    let g = IncludeExcludeGlobs::new(&["**".into()], &["*.log".into()]).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("app.rs"), MatchDecision::Allowed);
}

#[test]
fn glob_include_gates() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_path_and_str_agree() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &["src/gen/**".into()]).unwrap();
    for p in ["src/lib.rs", "src/gen/out.rs", "README.md"] {
        assert_eq!(g.decide_str(p), g.decide_path(Path::new(p)));
    }
}

#[test]
fn glob_is_allowed_helper() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn glob_invalid_pattern_errors() {
    assert!(IncludeExcludeGlobs::new(&["[".into()], &[]).is_err());
}

#[test]
fn glob_deep_paths() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c/d/e/f.txt"), MatchDecision::Allowed);
}

#[test]
fn glob_unicode_paths() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("données/日本語.txt"), MatchDecision::Allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 10: Miscellaneous serde/builder regression (remaining tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn misc_sha256_hex_64_chars() {
    assert_eq!(sha256_hex(b"test").len(), 64);
}

#[test]
fn misc_sha256_hex_deterministic() {
    assert_eq!(sha256_hex(b"hello"), sha256_hex(b"hello"));
}

#[test]
fn misc_sha256_hex_differs() {
    assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
}

#[test]
fn misc_execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn misc_outcome_serde_roundtrip() {
    for o in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<Outcome>(&json).unwrap(), o);
    }
}

#[test]
fn misc_execution_mode_serde_roundtrip() {
    for m in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<ExecutionMode>(&json).unwrap(), m);
    }
}

#[test]
fn misc_receipt_builder_defaults() {
    let r = ReceiptBuilder::new("test-backend").build();
    assert_eq!(r.backend.id, "test-backend");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_with_trace() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("mock").add_trace_event(event).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_with_artifact() {
    let r = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn receipt_builder_with_hash_shortcut() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn envelope_fatal_with_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "timeout",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn envelope_error_code_none_for_non_fatal() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(env.error_code().is_none());
}

#[test]
fn work_order_builder_include_exclude() {
    let wo = WorkOrderBuilder::new("test")
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_builder_context_packet() {
    let ctx = ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("test").context(ctx).build();
    assert_eq!(wo.context.files, vec!["README.md"]);
}

#[test]
fn canonical_json_empty_object() {
    let s = canonical_json(&serde_json::json!({})).unwrap();
    assert_eq!(s, "{}");
}

#[test]
fn canonical_json_nested_sorting() {
    let v = serde_json::json!({"b": {"d": 1, "c": 2}, "a": 3});
    let s = canonical_json(&v).unwrap();
    assert!(s.find("\"a\"").unwrap() < s.find("\"b\"").unwrap());
    assert!(s.find("\"c\"").unwrap() < s.find("\"d\"").unwrap());
}

#[test]
fn decode_stream_handles_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn decode_stream_multiple_messages() {
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\
                 {\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 2);
}
