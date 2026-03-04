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
//! Structured fuzzing tests for the Agent Backplane.
//!
//! These tests use proptest-driven random inputs and hand-crafted edge cases
//! to verify that ABP types never panic on malformed, truncated, or adversarial
//! input. The goal is robustness: every code path must return a Result or
//! silently ignore bad data — never abort.

use proptest::prelude::*;
use std::io::BufReader;
use std::path::Path;

use abp_core::{
    AgentEvent, Outcome, PolicyProfile, Receipt, ReceiptBuilder, WorkOrder, receipt_hash,
};
use abp_error::{AbpError, AbpErrorDto, ErrorCode, ErrorInfo};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{compute_hash, verify_hash};

// ── Config ─────────────────────────────────────────────────────────────

fn cfg() -> ProptestConfig {
    ProptestConfig {
        cases: 50,
        ..ProptestConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1  JSONL protocol fuzzing (15 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(cfg())]

    // ── 1.1  Random JSON lines fed to envelope parser ──────────────────

    #[test]
    fn jsonl_random_ascii_no_panic(s in "[\\x20-\\x7e]{0,300}") {
        let _ = JsonlCodec::decode(&s);
    }

    #[test]
    fn jsonl_random_printable_no_panic(s in "\\PC{0,500}") {
        let _ = JsonlCodec::decode(&s);
    }

    #[test]
    fn jsonl_random_json_value_no_panic(
        key in "[a-z]{1,8}",
        val in prop_oneof![
            Just("true".to_string()),
            Just("null".to_string()),
            Just("42".to_string()),
            "[a-z ]{0,20}".prop_map(|s| format!("\"{}\"", s)),
        ]
    ) {
        let line = format!("{{\"{key}\":{val}}}");
        let _ = JsonlCodec::decode(&line);
    }

    // ── 1.2  Truncated JSON ────────────────────────────────────────────

    #[test]
    fn jsonl_truncated_fatal_returns_error(cut in 1usize..40) {
        let full = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
        let truncated = &full[..cut.min(full.len())];
        let result = JsonlCodec::decode(truncated);
        prop_assert!(result.is_err());
    }

    #[test]
    fn jsonl_truncated_hello_returns_error(cut in 1usize..60) {
        let full = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let truncated = &full[..cut.min(full.len())];
        let result = JsonlCodec::decode(truncated);
        prop_assert!(result.is_err());
    }

    // ── 1.3  Very large payloads ───────────────────────────────────────

    #[test]
    fn jsonl_large_error_field_no_panic(n in 100usize..5000) {
        let big = "X".repeat(n);
        let line = format!(r#"{{"t":"fatal","ref_id":null,"error":"{big}"}}"#);
        let result = JsonlCodec::decode(&line);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn jsonl_large_garbage_no_panic(n in 100usize..10_000) {
        let garbage = "a".repeat(n);
        let _ = JsonlCodec::decode(&garbage);
    }

    // ── 1.4  Malformed UTF-8 ───────────────────────────────────────────

    #[test]
    fn jsonl_arbitrary_bytes_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..500)) {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            let _ = JsonlCodec::decode(s);
        }
        // Non-UTF-8 bytes can't even be a &str, so the parser never sees them.
    }

    // ── 1.5  Mix of valid and invalid envelopes ────────────────────────

    #[test]
    fn jsonl_stream_mixed_lines(garbage in "[a-z!@#]{1,30}") {
        let valid = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
        let input = format!("{garbage}\n{valid}\n{garbage}\n");
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        // The valid line must parse successfully
        prop_assert!(results.iter().any(|r| r.is_ok()));
        // The garbage lines must be errors, not panics
        prop_assert!(results.iter().any(|r| r.is_err()));
    }

    #[test]
    fn jsonl_stream_all_garbage(lines in prop::collection::vec("\\PC{1,80}", 1..20)) {
        let input = lines.join("\n");
        let reader = BufReader::new(input.as_bytes());
        for result in JsonlCodec::decode_stream(reader) {
            let _ = result; // must not panic
        }
    }

    #[test]
    fn jsonl_stream_blank_lines_skipped(count in 1usize..30) {
        let blanks = "\n".repeat(count);
        let valid = r#"{"t":"fatal","ref_id":null,"error":"x"}"#;
        let input = format!("{blanks}{valid}\n{blanks}");
        let reader = BufReader::new(input.as_bytes());
        let ok_count = JsonlCodec::decode_stream(reader)
            .filter(|r| r.is_ok())
            .count();
        prop_assert_eq!(ok_count, 1);
    }

    #[test]
    fn jsonl_stream_whitespace_only_lines(
        ws in prop::collection::vec("[ \\t]{1,20}", 1..10)
    ) {
        let input = ws.join("\n");
        let reader = BufReader::new(input.as_bytes());
        for result in JsonlCodec::decode_stream(reader) {
            let _ = result;
        }
    }

    #[test]
    fn jsonl_encode_roundtrip_fatal(msg in "[a-zA-Z0-9 ]{1,50}") {
        let envelope = Envelope::Fatal {
            ref_id: None,
            error: msg.clone(),
            error_code: None,
        };
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        prop_assert!(encoded.ends_with('\n'));
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error, .. } => prop_assert_eq!(error, msg),
            _ => prop_assert!(false, "expected Fatal"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  WorkOrder deserialization fuzzing (10 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(cfg())]

    // ── 2.1  Random JSON objects → WorkOrder::deserialize ──────────────

    #[test]
    fn work_order_random_string_no_panic(s in "\\PC{0,300}") {
        let _ = serde_json::from_str::<WorkOrder>(&s);
    }

    #[test]
    fn work_order_random_bytes_no_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..500)
    ) {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            let _ = serde_json::from_str::<WorkOrder>(s);
        }
    }

    // ── 2.2  Missing required fields → proper error ────────────────────

    #[test]
    fn work_order_missing_task_is_error(extra in "[a-z]{1,10}") {
        let json = format!(r#"{{"id":"00000000-0000-0000-0000-000000000000","{extra}":"v"}}"#);
        let result = serde_json::from_str::<WorkOrder>(&json);
        prop_assert!(result.is_err());
    }

    #[test]
    fn work_order_partial_json_is_error(task in "[a-zA-Z ]{1,30}") {
        let json = format!(r#"{{"task":"{task}"}}"#);
        let result = serde_json::from_str::<WorkOrder>(&json);
        prop_assert!(result.is_err());
    }

    #[test]
    fn work_order_null_fields_no_panic(_i in 0..10u32) {
        let json = r#"{"task":null,"id":null,"lane":null}"#;
        let _ = serde_json::from_str::<WorkOrder>(json);
    }

    // ── 2.3  Extra unknown fields → ignored ────────────────────────────

    #[test]
    fn work_order_extra_fields_ignored(
        key in "[a-z_]{1,15}",
        val in "[a-z0-9]{1,10}"
    ) {
        // Build a valid WorkOrder, serialize it, inject an extra field, re-parse
        let wo = abp_core::WorkOrderBuilder::new("test task").build();
        let mut v: serde_json::Value = serde_json::to_value(&wo).unwrap();
        v.as_object_mut().unwrap().insert(key, serde_json::Value::String(val));
        let result = serde_json::from_value::<WorkOrder>(v);
        prop_assert!(result.is_ok());
    }

    // ── 2.4  Deeply nested vendor config ───────────────────────────────

    #[test]
    fn work_order_deep_vendor_config(depth in 1usize..50) {
        let wo = abp_core::WorkOrderBuilder::new("deep test").build();
        let mut v: serde_json::Value = serde_json::to_value(&wo).unwrap();

        // Build a deeply nested JSON value
        let mut nested = serde_json::json!("leaf");
        for _ in 0..depth {
            nested = serde_json::json!({"inner": nested});
        }
        v["config"]["vendor"]["deep"] = nested;

        let result = serde_json::from_value::<WorkOrder>(v);
        prop_assert!(result.is_ok());
    }

    #[test]
    fn work_order_wrong_type_fields_no_panic(
        val in prop_oneof![
            Just(serde_json::json!(42)),
            Just(serde_json::json!(true)),
            Just(serde_json::json!([])),
            Just(serde_json::json!(null)),
        ]
    ) {
        let json = serde_json::json!({"task": val, "id": val});
        let _ = serde_json::from_value::<WorkOrder>(json);
    }

    #[test]
    fn work_order_roundtrip_preserves_task(task in "[a-zA-Z0-9 ]{1,50}") {
        let wo = abp_core::WorkOrderBuilder::new(task.clone()).build();
        let json = serde_json::to_string(&wo).unwrap();
        let parsed: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(parsed.task, task);
    }

    #[test]
    fn work_order_empty_json_object_is_error(_i in 0..5u32) {
        let result = serde_json::from_str::<WorkOrder>("{}");
        prop_assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Receipt hash fuzzing (10 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(cfg())]

    // ── 3.1  Random receipts → with_hash() never panics ────────────────

    #[test]
    fn receipt_with_hash_never_panics(
        backend in "[a-z]{1,20}",
        outcome in prop_oneof![
            Just(Outcome::Complete),
            Just(Outcome::Partial),
            Just(Outcome::Failed),
        ]
    ) {
        let receipt = ReceiptBuilder::new(backend)
            .outcome(outcome)
            .build()
            .with_hash()
            .unwrap();
        prop_assert!(receipt.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_hash_never_panics_on_builder(backend in "[a-z]{1,30}") {
        let receipt = ReceiptBuilder::new(backend).build();
        let result = receipt_hash(&receipt);
        prop_assert!(result.is_ok());
    }

    // ── 3.2  Hash is always 64 hex chars ───────────────────────────────

    #[test]
    fn receipt_hash_always_64_hex(backend in "[a-zA-Z0-9_]{1,20}") {
        let receipt = ReceiptBuilder::new(backend).build();
        let hash = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(hash.len(), 64);
        prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_compute_hash_always_64_hex(backend in "[a-zA-Z0-9]{1,15}") {
        let receipt = ReceiptBuilder::new(backend).build();
        let hash = compute_hash(&receipt).unwrap();
        prop_assert_eq!(hash.len(), 64);
        prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_hash_deterministic(backend in "[a-z]{1,10}") {
        let receipt = ReceiptBuilder::new(backend).build();
        let h1 = receipt_hash(&receipt).unwrap();
        let h2 = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // ── 3.3  Modified receipt → different hash ─────────────────────────

    #[test]
    fn receipt_modified_outcome_changes_hash(_i in 0..10u32) {
        let r1 = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
        let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_modified_backend_changes_hash(
        b1 in "[a-z]{1,10}",
        b2 in "[a-z]{1,10}"
    ) {
        prop_assume!(b1 != b2);
        let r1 = ReceiptBuilder::new(&b1).build();
        let r2 = ReceiptBuilder::new(&b2).build();
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_verify_hash_after_with_hash(_i in 0..10u32) {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        prop_assert!(verify_hash(&receipt));
    }

    #[test]
    fn receipt_verify_hash_detects_tamper(_i in 0..10u32) {
        let mut receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        receipt.receipt_sha256 = Some("0000000000000000000000000000000000000000000000000000000000000000".into());
        prop_assert!(!verify_hash(&receipt));
    }

    #[test]
    fn receipt_no_hash_verify_returns_true(_i in 0..10u32) {
        let receipt = ReceiptBuilder::new("mock").build();
        prop_assert!(receipt.receipt_sha256.is_none());
        prop_assert!(verify_hash(&receipt));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Policy engine fuzzing (10 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(cfg())]

    // ── 4.1  Random glob patterns → PolicyEngine — no panics ───────────

    #[test]
    fn policy_random_tool_pattern_no_panic(pattern in "[a-zA-Z*?]{1,20}") {
        let policy = PolicyProfile {
            disallowed_tools: vec![pattern],
            ..PolicyProfile::default()
        };
        let _ = PolicyEngine::new(&policy);
    }

    #[test]
    fn policy_random_path_pattern_no_panic(pattern in "[a-zA-Z0-9/*?]{1,30}") {
        let policy = PolicyProfile {
            deny_read: vec![pattern.clone()],
            deny_write: vec![pattern],
            ..PolicyProfile::default()
        };
        let _ = PolicyEngine::new(&policy);
    }

    // ── 4.2  Random paths checked against random policies ──────────────

    #[test]
    fn policy_random_path_check_no_panic(
        path in "[a-zA-Z0-9_./]{1,40}",
        deny in "[a-zA-Z*]{1,15}"
    ) {
        let policy = PolicyProfile {
            deny_write: vec![deny],
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let _ = engine.can_write_path(Path::new(&path));
            let _ = engine.can_read_path(Path::new(&path));
        }
    }

    #[test]
    fn policy_random_tool_check_no_panic(
        tool in "[a-zA-Z]{1,20}",
        allow in prop::collection::vec("[a-zA-Z*]{1,10}", 0..5),
        deny in prop::collection::vec("[a-zA-Z*]{1,10}", 0..5),
    ) {
        let policy = PolicyProfile {
            allowed_tools: allow,
            disallowed_tools: deny,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let _ = engine.can_use_tool(&tool);
        }
    }

    // ── 4.3  Empty policies — everything allowed ───────────────────────

    #[test]
    fn policy_empty_allows_tool(tool in "[a-zA-Z]{1,20}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    #[test]
    fn policy_empty_allows_read(path in "[a-zA-Z0-9_./]{1,30}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_read_path(Path::new(&path)).allowed);
    }

    #[test]
    fn policy_empty_allows_write(path in "[a-zA-Z0-9_./]{1,30}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }

    #[test]
    fn policy_deny_then_check_no_panic(
        patterns in prop::collection::vec("[a-z*?/]{1,15}", 1..10),
        path in "[a-z/]{1,20}"
    ) {
        let policy = PolicyProfile {
            deny_read: patterns.clone(),
            deny_write: patterns,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let _ = engine.can_read_path(Path::new(&path));
            let _ = engine.can_write_path(Path::new(&path));
        }
    }

    #[test]
    fn policy_wildcard_deny_blocks(tool in "[a-zA-Z]{1,10}") {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec![tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }

    #[test]
    fn policy_multiple_patterns_no_panic(
        allowed in prop::collection::vec("[A-Z][a-z]{0,8}", 0..5),
        denied in prop::collection::vec("[A-Z][a-z]{0,8}", 0..5),
        deny_read in prop::collection::vec("[a-z*/.]{1,12}", 0..5),
        deny_write in prop::collection::vec("[a-z*/.]{1,12}", 0..5),
    ) {
        let policy = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            deny_read,
            deny_write,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let _ = engine.can_use_tool("Bash");
            let _ = engine.can_read_path(Path::new("src/lib.rs"));
            let _ = engine.can_write_path(Path::new("src/lib.rs"));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5  Error roundtrip fuzzing (5 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(cfg())]

    // ── 5.1  ErrorCode roundtrip through serde ─────────────────────────

    #[test]
    fn error_code_roundtrip(
        code in prop_oneof![
            Just(ErrorCode::ProtocolInvalidEnvelope),
            Just(ErrorCode::ProtocolHandshakeFailed),
            Just(ErrorCode::ProtocolMissingRefId),
            Just(ErrorCode::ProtocolUnexpectedMessage),
            Just(ErrorCode::ProtocolVersionMismatch),
            Just(ErrorCode::BackendNotFound),
            Just(ErrorCode::BackendUnavailable),
            Just(ErrorCode::BackendTimeout),
            Just(ErrorCode::BackendRateLimited),
            Just(ErrorCode::BackendAuthFailed),
            Just(ErrorCode::BackendModelNotFound),
            Just(ErrorCode::BackendCrashed),
            Just(ErrorCode::PolicyDenied),
            Just(ErrorCode::PolicyInvalid),
            Just(ErrorCode::CapabilityUnsupported),
            Just(ErrorCode::CapabilityEmulationFailed),
            Just(ErrorCode::Internal),
            Just(ErrorCode::WorkspaceInitFailed),
            Just(ErrorCode::WorkspaceStagingFailed),
            Just(ErrorCode::IrLoweringFailed),
            Just(ErrorCode::IrInvalid),
            Just(ErrorCode::ReceiptHashMismatch),
            Just(ErrorCode::ReceiptChainBroken),
            Just(ErrorCode::DialectUnknown),
            Just(ErrorCode::DialectMappingFailed),
            Just(ErrorCode::ConfigInvalid),
        ]
    ) {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(code, back);
    }

    #[test]
    fn error_code_as_str_stable(
        code in prop_oneof![
            Just(ErrorCode::BackendNotFound),
            Just(ErrorCode::PolicyDenied),
            Just(ErrorCode::Internal),
        ]
    ) {
        let s = code.as_str();
        prop_assert!(!s.is_empty());
        let json = serde_json::to_string(&code).unwrap();
        // The JSON representation is the quoted snake_case string
        prop_assert_eq!(json, format!("\"{}\"", s));
    }

    // ── 5.2  AbpError / ErrorInfo survive serialization ────────────────

    #[test]
    fn error_info_roundtrip(
        msg in "[a-zA-Z0-9 ]{1,50}",
        code in prop_oneof![
            Just(ErrorCode::Internal),
            Just(ErrorCode::BackendTimeout),
            Just(ErrorCode::PolicyDenied),
        ]
    ) {
        let info = ErrorInfo::new(code, msg.clone());
        let json = serde_json::to_string(&info).unwrap();
        let back: ErrorInfo = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.code, code);
        prop_assert_eq!(back.message, msg);
    }

    #[test]
    fn abp_error_dto_roundtrip(
        msg in "[a-zA-Z0-9 ]{1,50}",
        code in prop_oneof![
            Just(ErrorCode::Internal),
            Just(ErrorCode::BackendCrashed),
            Just(ErrorCode::ProtocolInvalidEnvelope),
        ]
    ) {
        let err = AbpError::new(code, msg.clone());
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.code, code);
        prop_assert_eq!(back.message, msg);
    }

    #[test]
    fn abp_error_display_no_panic(
        msg in "\\PC{0,100}",
        code in prop_oneof![
            Just(ErrorCode::Internal),
            Just(ErrorCode::BackendTimeout),
        ]
    ) {
        let err = AbpError::new(code, msg)
            .with_context("key", "value");
        let display = format!("{err}");
        prop_assert!(!display.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6  Hand-crafted edge cases (supplements proptest coverage)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn jsonl_empty_string_is_error() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn jsonl_bare_null_is_error() {
    assert!(JsonlCodec::decode("null").is_err());
}

#[test]
fn jsonl_bare_number_is_error() {
    assert!(JsonlCodec::decode("42").is_err());
}

#[test]
fn jsonl_bare_array_is_error() {
    assert!(JsonlCodec::decode("[]").is_err());
}

#[test]
fn jsonl_nested_braces_no_panic() {
    let deep = "{".repeat(100) + &"}".repeat(100);
    let _ = JsonlCodec::decode(&deep);
}

#[test]
fn jsonl_unicode_escape_no_panic() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"\u0000\uFFFF"}"#;
    let _ = JsonlCodec::decode(line);
}

#[test]
fn work_order_from_empty_object_is_error() {
    assert!(serde_json::from_str::<WorkOrder>("{}").is_err());
}

#[test]
fn work_order_from_array_is_error() {
    assert!(serde_json::from_str::<WorkOrder>("[]").is_err());
}

#[test]
fn receipt_from_random_garbage_no_panic() {
    for input in &[
        "",
        "null",
        "[]",
        "{}",
        r#"{"outcome":"complete"}"#,
        r#"{"meta":null}"#,
        "true",
        "999999",
    ] {
        let _ = serde_json::from_str::<Receipt>(input);
    }
}

#[test]
fn receipt_hash_with_empty_trace() {
    let receipt = ReceiptBuilder::new("test").build();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_hash_with_large_trace() {
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;

    let mut builder = ReceiptBuilder::new("test");
    for i in 0..100 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn policy_invalid_glob_syntax_returns_error() {
    let policy = PolicyProfile {
        deny_read: vec!["[invalid".to_string()],
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&policy).is_err());
}

#[test]
fn policy_empty_string_pattern_no_panic() {
    let policy = PolicyProfile {
        disallowed_tools: vec![String::new()],
        ..PolicyProfile::default()
    };
    // May succeed or fail depending on glob implementation — must not panic
    let _ = PolicyEngine::new(&policy);
}

#[test]
fn error_code_unknown_string_is_error() {
    let result = serde_json::from_str::<ErrorCode>(r#""not_a_real_code""#);
    assert!(result.is_err());
}

#[test]
fn error_code_all_variants_have_category() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendNotFound,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::DialectUnknown,
        ErrorCode::ConfigInvalid,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
    ];
    for code in &codes {
        let cat = code.category();
        let _ = format!("{cat}"); // Display must not panic
    }
}

#[test]
fn error_info_with_detail_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out")
        .with_detail("ms", 30_000)
        .with_detail("backend", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, ErrorCode::BackendTimeout);
    assert_eq!(back.details.len(), 2);
}

#[test]
fn agent_event_random_json_no_panic() {
    let inputs = [
        r#"{}"#,
        r#"{"ts":"2025-01-01T00:00:00Z"}"#,
        r#"{"ts":"2025-01-01T00:00:00Z","type":"unknown_type"}"#,
        r#"{"ts":"not-a-date","type":"run_started","message":"hi"}"#,
        r#"null"#,
        r#"[]"#,
    ];
    for input in &inputs {
        let _ = serde_json::from_str::<AgentEvent>(input);
    }
}

#[test]
fn envelope_all_variant_types_with_missing_fields() {
    let types = ["hello", "run", "event", "final", "fatal"];
    for t in &types {
        let json = format!(r#"{{"t":"{t}"}}"#);
        let result = JsonlCodec::decode(&json);
        // "fatal" with default error could work, others should fail
        // But none should panic
        let _ = result;
    }
}

#[test]
fn jsonl_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty());
}

#[test]
fn jsonl_stream_single_newline() {
    let reader = BufReader::new("\n".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty());
}
