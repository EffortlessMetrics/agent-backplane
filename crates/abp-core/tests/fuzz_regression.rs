// SPDX-License-Identifier: MIT OR Apache-2.0
//! Regression tests for tricky/edge-case inputs found during fuzzing.
//!
//! Run with: cargo test -p abp-core --test fuzz_regression

use abp_core::*;
use chrono::Utc;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ---------------------------------------------------------------------------
// 1. Empty strings everywhere
// ---------------------------------------------------------------------------

#[test]
fn empty_strings_in_work_order() {
    let wo = WorkOrderBuilder::new("").root("").model("").build();
    assert!(wo.task.is_empty());
    assert!(wo.workspace.root.is_empty());
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, "");
}

#[test]
fn empty_backend_id_fails_validation() {
    let mut receipt = minimal_receipt();
    receipt.backend.id = String::new();
    let errs = validate::validate_receipt(&receipt).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, validate::ValidationError::EmptyBackendId))
    );
}

#[test]
fn empty_strings_in_receipt_fields() {
    let receipt = ReceiptBuilder::new("").outcome(Outcome::Complete).build();
    // Hashing must not panic on empty backend id.
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
}

// ---------------------------------------------------------------------------
// 2. Very long strings (10KB+)
// ---------------------------------------------------------------------------

#[test]
fn long_task_string_in_work_order() {
    let long_str = "A".repeat(10_240);
    let wo = WorkOrderBuilder::new(&long_str).build();
    assert_eq!(wo.task.len(), 10_240);

    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task.len(), 10_240);
}

#[test]
fn long_backend_id_in_receipt() {
    let long_id = "x".repeat(10_240);
    let receipt = ReceiptBuilder::new(&long_id)
        .outcome(Outcome::Complete)
        .build();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
    let _ = validate::validate_receipt(&receipt);
}

// ---------------------------------------------------------------------------
// 3. Unicode edge cases
// ---------------------------------------------------------------------------

#[test]
fn null_bytes_in_task() {
    let task = "hello\0world";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

#[test]
fn rtl_and_bidi_characters() {
    let task = "hello \u{202E}dlrow\u{202C} test";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

#[test]
fn zero_width_joiner_in_strings() {
    // Emoji family: 👨‍👩‍👧‍👦 is made of codepoints joined by ZWJ.
    let zwj_str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
    let wo = WorkOrderBuilder::new(zwj_str).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, zwj_str);
}

#[test]
fn mixed_unicode_scripts_and_surrogates() {
    let task = "مرحبا こんにちは 你好 🎉 \u{FEFF}BOM\u{FFFD}REPLACEMENT";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

// ---------------------------------------------------------------------------
// 4. Nested JSON in string fields
// ---------------------------------------------------------------------------

#[test]
fn nested_json_in_task_field() {
    let nested = r#"{"key": "value", "nested": {"a": [1,2,3]}}"#;
    let wo = WorkOrderBuilder::new(nested).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, nested);
}

#[test]
fn nested_json_in_backend_id() {
    let nested_id = r#"{"type":"evil"}"#;
    let receipt = ReceiptBuilder::new(nested_id)
        .outcome(Outcome::Complete)
        .build();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn deeply_nested_json_in_vendor_config() {
    let deep_val = serde_json::json!({
        "a": {"b": {"c": {"d": {"e": {"f": {"g": "deep"}}}}}}
    });
    let mut config = RuntimeConfig::default();
    config.vendor.insert("deep".into(), deep_val.clone());
    let wo = WorkOrderBuilder::new("test").config(config).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.config.vendor["deep"], deep_val);
}

// ---------------------------------------------------------------------------
// 5. Integer overflow values in numeric fields
// ---------------------------------------------------------------------------

#[test]
fn max_u64_duration() {
    let mut receipt = minimal_receipt();
    receipt.meta.duration_ms = u64::MAX;
    let json = serde_json::to_string(&receipt).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.meta.duration_ms, u64::MAX);
    let hash = receipt_hash(&rt).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn max_u32_turns_and_extreme_budget() {
    let wo = WorkOrderBuilder::new("test")
        .max_turns(u32::MAX)
        .max_budget_usd(f64::MAX)
        .build();
    assert_eq!(wo.config.max_turns, Some(u32::MAX));
    assert_eq!(wo.config.max_budget_usd, Some(f64::MAX));

    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.config.max_turns, Some(u32::MAX));
}

#[test]
fn max_u64_token_counts() {
    let mut receipt = minimal_receipt();
    receipt.usage.input_tokens = Some(u64::MAX);
    receipt.usage.output_tokens = Some(u64::MAX);
    receipt.usage.cache_read_tokens = Some(u64::MAX);
    receipt.usage.cache_write_tokens = Some(u64::MAX);
    receipt.usage.request_units = Some(u64::MAX);
    let json = serde_json::to_string(&receipt).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.usage.input_tokens, Some(u64::MAX));
}

// ---------------------------------------------------------------------------
// 6. Special characters in IDs
// ---------------------------------------------------------------------------

#[test]
fn special_chars_in_backend_id() {
    let ids = [
        "back/end",
        "back\\end",
        "back\tend",
        "back\nend",
        "a\"b",
        "<script>alert(1)</script>",
        "'; DROP TABLE receipts; --",
        "../../../etc/passwd",
    ];
    for id in &ids {
        let receipt = ReceiptBuilder::new(*id).outcome(Outcome::Complete).build();
        let json = serde_json::to_string(&receipt).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.backend.id, *id);
        let hash = receipt_hash(&rt).unwrap();
        assert_eq!(hash.len(), 64);
    }
}

#[test]
fn special_chars_in_tool_names() {
    let event = make_event(AgentEventKind::ToolCall {
        tool_name: "../../bin/sh".into(),
        tool_use_id: Some("\0null\0".into()),
        parent_tool_use_id: Some("\u{FFFF}".into()),
        input: serde_json::json!({"key": "val\u{0000}ue"}),
    });
    let json = serde_json::to_string(&event).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall { tool_name, .. } = &rt.kind {
        assert_eq!(tool_name, "../../bin/sh");
    } else {
        panic!("wrong variant");
    }
}

// ---------------------------------------------------------------------------
// 7. Filter edge cases
// ---------------------------------------------------------------------------

#[test]
fn filter_with_empty_kinds_list() {
    let event = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    // Include with empty list → nothing passes.
    let inc = filter::EventFilter::include_kinds(&[]);
    assert!(!inc.matches(&event));
    // Exclude with empty list → everything passes.
    let exc = filter::EventFilter::exclude_kinds(&[]);
    assert!(exc.matches(&event));
}

#[test]
fn filter_case_insensitivity() {
    let event = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let f = filter::EventFilter::include_kinds(&["RUN_STARTED"]);
    assert!(f.matches(&event));
    let f = filter::EventFilter::include_kinds(&["Run_Started"]);
    assert!(f.matches(&event));
}

// ---------------------------------------------------------------------------
// 8. Validation edge cases
// ---------------------------------------------------------------------------

#[test]
fn receipt_with_wrong_contract_version() {
    let mut receipt = minimal_receipt();
    receipt.meta.contract_version = "abp/v99.99".into();
    let errs = validate::validate_receipt(&receipt).unwrap_err();
    assert!(errs.iter().any(|e| {
        matches!(e, validate::ValidationError::InvalidOutcome { reason }
            if reason.contains("contract_version"))
    }));
}

#[test]
fn receipt_with_tampered_hash() {
    let receipt = minimal_receipt().with_hash().unwrap();
    let mut tampered = receipt;
    tampered.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let errs = validate::validate_receipt(&tampered).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, validate::ValidationError::InvalidHash { .. }))
    );
}

#[test]
fn receipt_hash_determinism_with_unicode() {
    let receipt = ReceiptBuilder::new("バックエンド🔥")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

// ---------------------------------------------------------------------------
// 9. Ext field edge cases
// ---------------------------------------------------------------------------

#[test]
fn event_with_large_ext_map() {
    let mut ext = BTreeMap::new();
    for i in 0..100 {
        ext.insert(format!("key_{i}"), serde_json::json!(i));
    }
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "x".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(rt.ext.is_some());
    assert_eq!(rt.ext.unwrap().len(), 100);
}

// ---------------------------------------------------------------------------
// 10. WorkOrder JSON edge cases (malformed inputs)
// ---------------------------------------------------------------------------

#[test]
fn malformed_json_does_not_panic() {
    let bad_inputs = [
        "",
        "null",
        "true",
        "42",
        "[]",
        "{}",
        r#"{"task": null}"#,
        r#"{"id": "not-a-uuid"}"#,
        r#"{"id": "00000000-0000-0000-0000-000000000000", "task": 123}"#,
        "{\"task\": \"x\", \"lane\": \"invalid_lane\"}",
        &format!("{{\"task\": \"{}\"}}", "x".repeat(1_000_000)),
    ];
    for input in &bad_inputs {
        // Must not panic — errors are fine.
        let _ = serde_json::from_str::<WorkOrder>(input);
        let _ = serde_json::from_str::<Receipt>(input);
    }
}

#[test]
fn receipt_json_with_nan_like_floats() {
    // Ensure that explicit null in float fields round-trips.
    let json = r#"{
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000000",
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": "2024-01-01T00:00:00Z",
            "finished_at": "2024-01-01T00:00:01Z",
            "duration_ms": 1000
        },
        "backend": {"id": "mock", "backend_version": null, "adapter_version": null},
        "capabilities": {},
        "mode": "mapped",
        "usage_raw": {},
        "usage": {
            "input_tokens": null,
            "output_tokens": null,
            "cache_read_tokens": null,
            "cache_write_tokens": null,
            "request_units": null,
            "estimated_cost_usd": null
        },
        "trace": [],
        "artifacts": [],
        "verification": {"git_diff": null, "git_status": null, "harness_ok": false},
        "outcome": "complete",
        "receipt_sha256": null
    }"#;
    let receipt: Receipt = serde_json::from_str(json).unwrap();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
    validate::validate_receipt(&receipt).unwrap();
}
