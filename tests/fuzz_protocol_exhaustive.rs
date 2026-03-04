// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive fuzz-style tests for protocol parsing, JSON handling, and security
//! boundaries.  These are deterministic regression tests that exercise edge-case
//! inputs which could cause panics, data corruption, or security issues.
#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, ReceiptBuilder, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::{Envelope, JsonlCodec};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend).outcome(outcome).build()
}

fn make_hashed_receipt(backend: &str) -> Receipt {
    let r = ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .build();
    r.with_hash().expect("hashing should not fail")
}

fn make_hello_json() -> String {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    JsonlCodec::encode(&env).unwrap()
}

// ===========================================================================
// 1. JSONL parsing robustness (10 tests)
// ===========================================================================

#[test]
fn jsonl_random_bytes_no_panic() {
    let random_inputs: &[&[u8]] = &[
        b"\xff\xfe\xfd\xfc\xfb",
        b"\x00\x01\x02\x03\x04",
        b"\x80\x81\x82\x83\x84",
        &[0xDE, 0xAD, 0xBE, 0xEF],
        &[255; 64],
    ];
    for input in random_inputs {
        if let Ok(s) = std::str::from_utf8(input) {
            let _ = JsonlCodec::decode(s);
        }
        // Non-UTF8 should never reach the decoder; verify no panic on the
        // conversion itself.
    }
}

#[test]
fn jsonl_truncated_json_no_panic() {
    let truncated = &[
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"#,
        r#"{"t":"hello"#,
        r#"{"t":"he"#,
        r#"{"#,
        r#""#,
        r#"{"t":"run","id":"abc","work_order":{"id":"00000000-0000-0000-0000-000000000000","task":"x"#,
    ];
    for input in truncated {
        let result = JsonlCodec::decode(input);
        assert!(result.is_err(), "truncated JSON must not parse: {}", input);
    }
}

#[test]
fn jsonl_oversized_payload_no_panic() {
    // 10 MB of nested braces
    let big = "{".repeat(1_000_000);
    let _ = JsonlCodec::decode(&big);

    // 10 MB string value
    let huge_val = format!(r#"{{"t":"hello","x":"{}"}}"#, "A".repeat(10_000_000));
    let _ = JsonlCodec::decode(&huge_val);
}

#[test]
fn jsonl_unicode_edge_cases() {
    let unicode_inputs = &[
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"🤖","backend_version":null,"adapter_version":null},"capabilities":{}}"#,
        // Zero-width joiner
        r#"{"t":"fatal","error":"error\u200B\u200Cmsg"}"#,
        // RTL override
        r#"{"t":"fatal","error":"error\u202Emsg"}"#,
        // Surrogate-pair boundary in JSON escapes
        r#"{"t":"fatal","error":"\uD83D\uDE00"}"#,
        // Combining diacriticals
        r#"{"t":"fatal","error":"e\u0301\u0301\u0301"}"#,
    ];
    for input in unicode_inputs {
        let _ = JsonlCodec::decode(input);
        // Must not panic regardless of result
    }
}

#[test]
fn jsonl_deeply_nested_objects() {
    // 128 levels deep
    let mut nested = String::new();
    for _ in 0..128 {
        nested.push_str(r#"{"a":"#);
    }
    nested.push_str("1");
    for _ in 0..128 {
        nested.push('}');
    }
    let _ = JsonlCodec::decode(&nested);
    let _ = serde_json::from_str::<Envelope>(&nested);
}

#[test]
fn jsonl_duplicate_keys() {
    // JSON with duplicate "t" key — serde_json uses last-value-wins
    let input = r#"{"t":"hello","t":"fatal","error":"dup"}"#;
    let result = JsonlCodec::decode(input);
    // Must not panic; may succeed or fail depending on parse strategy
    let _ = result;
}

#[test]
fn jsonl_null_bytes_in_string() {
    let input = "{\x00\"t\":\"hello\"}";
    let _ = JsonlCodec::decode(input);

    let input2 = "{\"t\":\"hello\",\"error\":\"a\x00b\"}";
    let _ = JsonlCodec::decode(input2);
}

#[test]
fn jsonl_mixed_whitespace_and_newlines() {
    let inputs = &[
        "\t\n\r {\"t\":\"fatal\",\"error\":\"x\"}\n\r\t",
        "   {\"t\":\"fatal\",\"error\":\"x\"}   ",
        "\n\n{\"t\":\"fatal\",\"error\":\"x\"}\n\n",
    ];
    for input in inputs {
        // The codec should trim; must not panic
        let _ = JsonlCodec::decode(input);
    }
}

#[test]
fn jsonl_empty_and_blank_lines() {
    let inputs = &[
        "", " ", "\n", "\r\n", "\t", "null", "true", "false", "42", "[]",
    ];
    for input in inputs {
        let result = JsonlCodec::decode(input);
        assert!(
            result.is_err(),
            "non-object JSON must not parse as Envelope: {}",
            input
        );
    }
}

#[test]
fn jsonl_streaming_codec_malformed_batch() {
    let batch = "not json\n{invalid}\n{\"t\":\"fatal\",\"error\":\"ok\"}\n";
    let results = StreamingCodec::decode_batch(batch);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_err());
    assert!(results[1].is_err());
    // Third line may or may not parse depending on field requirements
    let errors = StreamingCodec::validate_jsonl(batch);
    assert!(errors.len() >= 2, "should report at least 2 errors");
}

// ===========================================================================
// 2. Envelope parsing (10 tests)
// ===========================================================================

#[test]
fn envelope_missing_t_tag() {
    let input = r#"{"contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":{}}"#;
    let result = JsonlCodec::decode(input);
    assert!(result.is_err(), "missing 't' tag must fail");
}

#[test]
fn envelope_unknown_t_tag() {
    let inputs = &[
        r#"{"t":"unknown_type","data":"x"}"#,
        r#"{"t":"HELLO","contract_version":"abp/v0.1"}"#,
        r#"{"t":"","error":"empty tag"}"#,
        r#"{"t":null}"#,
        r#"{"t":42}"#,
        r#"{"t":true}"#,
    ];
    for input in inputs {
        let result = JsonlCodec::decode(input);
        assert!(result.is_err(), "unknown/invalid 't' must fail: {}", input);
    }
}

#[test]
fn envelope_hello_missing_required_fields() {
    let inputs = &[
        // Missing backend
        r#"{"t":"hello","contract_version":"abp/v0.1","capabilities":{}}"#,
        // Missing contract_version
        r#"{"t":"hello","backend":{"id":"x"},"capabilities":{}}"#,
        // Missing capabilities
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x"}}"#,
    ];
    for input in inputs {
        let result = JsonlCodec::decode(input);
        assert!(
            result.is_err(),
            "hello with missing field must fail: {}",
            input
        );
    }
}

#[test]
fn envelope_wrong_type_for_fields() {
    let inputs = &[
        // contract_version as number
        r#"{"t":"hello","contract_version":42,"backend":{"id":"x"},"capabilities":{}}"#,
        // capabilities as string
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":"none"}"#,
        // backend as string instead of object
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":"x","capabilities":{}}"#,
        // error as array in fatal
        r#"{"t":"fatal","error":["a","b"]}"#,
    ];
    for input in inputs {
        let result = JsonlCodec::decode(input);
        assert!(
            result.is_err(),
            "wrong field types must produce error: {}",
            input
        );
    }
}

#[test]
fn envelope_extra_fields_tolerated() {
    // serde should skip unknown fields by default
    let input = r#"{"t":"fatal","error":"boom","extra_field":"ignored","nested":{"a":1}}"#;
    let result = JsonlCodec::decode(input);
    // Should succeed — extra fields are just ignored
    assert!(result.is_ok(), "extra fields should be tolerated");
}

#[test]
fn envelope_run_with_minimal_work_order() {
    // A run envelope with a work order containing only required fields
    let wo = WorkOrderBuilder::new("test task").build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(&encoded).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn envelope_roundtrip_all_variants() {
    let backend = BackendIdentity {
        id: "test".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.1".into()),
    };
    let caps: CapabilityManifest = BTreeMap::new();

    let hello = Envelope::hello(backend.clone(), caps.clone());
    let fatal = Envelope::Fatal {
        ref_id: Some("ref-1".into()),
        error: "something broke".into(),
        error_code: None,
    };
    let wo = WorkOrderBuilder::new("do thing").build();
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };

    for env in &[hello, fatal, run] {
        let encoded = JsonlCodec::encode(env).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        // Re-encode for determinism check
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(encoded, re_encoded, "roundtrip must be deterministic");
    }
}

#[test]
fn envelope_event_with_all_event_kinds() {
    use chrono::Utc;
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "hello".into(),
        },
        AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: json!("file.txt"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "modified".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        },
    ];
    for kind in kinds {
        let event = AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(&encoded);
        assert!(decoded.is_ok(), "event roundtrip must succeed");
    }
}

#[test]
fn envelope_fatal_minimal_and_maximal() {
    // Minimal fatal — just error
    let min = r#"{"t":"fatal","error":"boom"}"#;
    let result = JsonlCodec::decode(min);
    assert!(result.is_ok(), "minimal fatal should parse");

    // Maximal fatal — all optional fields
    let max = r#"{"t":"fatal","ref_id":"abc-123","error":"boom","error_code":"sidecar_crashed"}"#;
    let _ = JsonlCodec::decode(max);
}

#[test]
fn envelope_decode_batch_preserves_order() {
    let hello = make_hello_json();
    let fatal_line = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    })
    .unwrap();
    let batch = format!("{}{}", hello, fatal_line);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 2);
    assert!(matches!(
        results[0].as_ref().unwrap(),
        Envelope::Hello { .. }
    ));
    assert!(matches!(
        results[1].as_ref().unwrap(),
        Envelope::Fatal { .. }
    ));
}

// ===========================================================================
// 3. Work order validation (10 tests)
// ===========================================================================

#[test]
fn workorder_empty_task() {
    let wo = WorkOrderBuilder::new("").build();
    assert!(wo.task.is_empty());
    // Should still serialize and roundtrip
    let json = serde_json::to_string(&wo).unwrap();
    let _: WorkOrder = serde_json::from_str(&json).unwrap();
}

#[test]
fn workorder_oversized_task() {
    let big_task = "A".repeat(10_000_000);
    let wo = WorkOrderBuilder::new(big_task.clone()).build();
    assert_eq!(wo.task.len(), 10_000_000);
    let json = serde_json::to_string(&wo).unwrap();
    let roundtrip: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.task.len(), 10_000_000);
}

#[test]
fn workorder_special_characters_in_task() {
    let special_tasks = &[
        "task with \"quotes\" and \\backslashes\\",
        "task\nwith\nnewlines",
        "task\twith\ttabs",
        "task with null: \0",
        "task with emoji: 🔥🚀💯",
        "<script>alert('xss')</script>",
        "'; DROP TABLE work_orders; --",
        "../../../etc/passwd",
    ];
    for task in special_tasks {
        let wo = WorkOrderBuilder::new(*task).build();
        let json = serde_json::to_string(&wo).unwrap();
        let roundtrip: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.task, *task);
    }
}

#[test]
fn workorder_id_is_valid_uuid() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(!wo.id.is_nil());
    // Confirm the serialized form contains a valid UUID string
    let json = serde_json::to_value(&wo).unwrap();
    let id_str = json["id"].as_str().unwrap();
    let parsed: uuid::Uuid = id_str.parse().unwrap();
    assert_eq!(wo.id, parsed);
}

#[test]
fn workorder_duplicate_include_exclude_patterns() {
    let wo = WorkOrderBuilder::new("test")
        .include(vec!["*.rs".into(), "*.rs".into(), "*.rs".into()])
        .exclude(vec!["target/*".into(), "target/*".into()])
        .build();
    // Should build without panic
    let json = serde_json::to_string(&wo).unwrap();
    let _: WorkOrder = serde_json::from_str(&json).unwrap();
}

#[test]
fn workorder_null_and_empty_config_values() {
    let wo = WorkOrderBuilder::new("test")
        .model("")
        .max_turns(0)
        .max_budget_usd(0.0)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let _: WorkOrder = serde_json::from_str(&json).unwrap();
}

#[test]
fn workorder_all_execution_lanes() {
    let lanes = [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst];
    for lane in &lanes {
        let wo = WorkOrderBuilder::new("test").lane(lane.clone()).build();
        let json = serde_json::to_string(&wo).unwrap();
        let roundtrip: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(&wo.lane).unwrap(),
            serde_json::to_value(&roundtrip.lane).unwrap()
        );
    }
}

#[test]
fn workorder_all_workspace_modes() {
    let modes = [WorkspaceMode::Staged];
    for mode in &modes {
        let wo = WorkOrderBuilder::new("test")
            .workspace_mode(mode.clone())
            .build();
        let json = serde_json::to_string(&wo).unwrap();
        let roundtrip: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(&wo.workspace.mode).unwrap(),
            serde_json::to_value(&roundtrip.workspace.mode).unwrap()
        );
    }
}

#[test]
fn workorder_unicode_in_all_string_fields() {
    let wo = WorkOrderBuilder::new("タスク 🎯")
        .root("./путь/к/проекту")
        .model("模型-v1")
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let roundtrip: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.task, "タスク 🎯");
}

#[test]
fn workorder_extreme_max_turns_and_budget() {
    let wo = WorkOrderBuilder::new("test")
        .max_turns(u32::MAX)
        .max_budget_usd(f64::MAX)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let _roundtrip: WorkOrder = serde_json::from_str(&json).unwrap();
}

// ===========================================================================
// 4. Receipt tampering detection (10 tests)
// ===========================================================================

#[test]
fn receipt_hash_deterministic() {
    let r1 = make_receipt("test", Outcome::Complete);
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2, "receipt hash must be deterministic");
}

#[test]
fn receipt_hash_is_valid_hex() {
    let r = make_hashed_receipt("test");
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex must be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be valid hex"
    );
}

#[test]
fn receipt_hash_changes_on_field_modification() {
    let r1 = make_hashed_receipt("backend-a");
    let r2 = make_hashed_receipt("backend-b");
    assert_ne!(
        r1.receipt_sha256, r2.receipt_sha256,
        "different backends must produce different hashes"
    );
}

#[test]
fn receipt_hash_changes_on_outcome_modification() {
    let r_complete = make_receipt("test", Outcome::Complete);
    let r_failed = make_receipt("test", Outcome::Failed);
    let h1 = receipt_hash(&r_complete).unwrap();
    let h2 = receipt_hash(&r_failed).unwrap();
    assert_ne!(h1, h2, "different outcomes must produce different hashes");
}

#[test]
fn receipt_with_hash_embeds_correctly() {
    let r = make_receipt("test", Outcome::Complete);
    assert!(r.receipt_sha256.is_none(), "builder receipt has no hash");
    let hashed = r.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some(), "with_hash must embed hash");
}

#[test]
fn receipt_rehash_after_embedding_is_consistent() {
    let hashed = make_hashed_receipt("test");
    let embedded_hash = hashed.receipt_sha256.clone().unwrap();
    // Re-hashing should produce same result (receipt_hash nulls the field first)
    let recomputed = receipt_hash(&hashed).unwrap();
    assert_eq!(
        embedded_hash, recomputed,
        "re-hashing must be consistent with embedded hash"
    );
}

#[test]
fn receipt_tampered_hash_detected() {
    let mut hashed = make_hashed_receipt("test");
    let original_hash = hashed.receipt_sha256.clone().unwrap();
    // Tamper with the hash
    hashed.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let recomputed = receipt_hash(&hashed).unwrap();
    assert_eq!(
        recomputed, original_hash,
        "recomputed hash should match original, not tampered value"
    );
    assert_ne!(
        hashed.receipt_sha256.as_ref().unwrap(),
        &recomputed,
        "tampered hash differs from recomputed"
    );
}

#[test]
fn receipt_truncated_json_no_panic() {
    let r = make_hashed_receipt("test");
    let full_json = serde_json::to_string(&r).unwrap();
    // Try deserializing various truncations
    for cut in &[10, 50, 100, full_json.len() / 2, full_json.len() - 1] {
        if *cut < full_json.len() {
            let truncated = &full_json[..*cut];
            let result = serde_json::from_str::<Receipt>(truncated);
            assert!(result.is_err(), "truncated receipt must fail to parse");
        }
    }
}

#[test]
fn receipt_invalid_hex_in_hash_field() {
    let r = make_hashed_receipt("test");
    let mut json_val = serde_json::to_value(&r).unwrap();
    // Set hash to invalid hex
    json_val["receipt_sha256"] = json!("ZZZZ_NOT_HEX_AT_ALL");
    let roundtrip: Receipt = serde_json::from_value(json_val).unwrap();
    // Deserialization succeeds (it's just a string), but recomputed hash differs
    let recomputed = receipt_hash(&roundtrip).unwrap();
    assert_ne!(
        roundtrip.receipt_sha256.as_deref(),
        Some(recomputed.as_str())
    );
}

#[test]
fn receipt_swapped_fields_changes_hash() {
    use chrono::Utc;
    let event_a = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let event_b = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "warn".into(),
        },
        ext: None,
    };
    let r1 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .add_trace_event(event_a.clone())
        .add_trace_event(event_b.clone())
        .build();
    let r2 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .add_trace_event(event_b)
        .add_trace_event(event_a)
        .build();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2, "swapped event order must produce different hashes");
}

// ===========================================================================
// 5. Policy boundary testing (10 tests)
// ===========================================================================

#[test]
fn policy_path_traversal_in_deny_write() {
    let policy = PolicyProfile {
        deny_write: vec!["**/../../../etc/passwd".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy);
    // May fail to compile (invalid glob) or succeed — must not panic
    if let Ok(engine) = engine {
        let decision = engine.can_write_path(Path::new("../../../etc/passwd"));
        // Should either deny or the glob just doesn't match the literal path
        let _ = decision;
    }
}

#[test]
fn policy_path_traversal_variants() {
    let traversal_paths = &[
        "../../../etc/shadow",
        "..\\..\\..\\windows\\system32\\config\\sam",
        "foo/../../bar",
        "./../../../../root/.ssh/id_rsa",
        "normal/../../../escape",
    ];
    let policy = PolicyProfile {
        deny_write: vec!["**".into()],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        for path in traversal_paths {
            let decision = engine.can_write_path(Path::new(path));
            assert!(
                !decision.allowed,
                "deny-all policy must deny path: {}",
                path
            );
        }
    }
}

#[test]
fn policy_regex_injection_in_patterns() {
    let evil_patterns = &[
        "(.*)",
        "[a-z]+",
        "(?:exploit)*",
        "a{1000000}",
        ".*",
        r"\d+",
        "(?i)secret",
    ];
    for pattern in evil_patterns {
        let policy = PolicyProfile {
            deny_write: vec![(*pattern).into()],
            ..Default::default()
        };
        // Must not panic — may fail to compile as a glob
        let _ = PolicyEngine::new(&policy);
    }
}

#[test]
fn policy_oversized_patterns() {
    let huge_pattern = "*".repeat(100_000);
    let policy = PolicyProfile {
        deny_write: vec![huge_pattern],
        ..Default::default()
    };
    // Must not panic
    let _ = PolicyEngine::new(&policy);
}

#[test]
fn policy_conflicting_allow_deny_tools() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        let decision = engine.can_use_tool("bash");
        // Deny should take precedence over allow
        assert!(
            !decision.allowed,
            "deny must take precedence over allow for tools"
        );
    }
}

#[test]
fn policy_unicode_paths() {
    let unicode_paths = &[
        "проект/src/main.rs",
        "プロジェクト/設定.toml",
        "مشروع/ملف.txt",
        "项目/源代码/主.rs",
        "dossier/café.txt",
    ];
    let policy = PolicyProfile {
        deny_write: vec!["**/*.rs".into()],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        for path in unicode_paths {
            let _ = engine.can_write_path(Path::new(path));
            let _ = engine.can_read_path(Path::new(path));
            // Must not panic regardless of unicode content
        }
    }
}

#[test]
fn policy_empty_and_whitespace_patterns() {
    let patterns = vec!["".into(), " ".into(), "\t".into(), "\n".into()];
    let policy = PolicyProfile {
        deny_write: patterns.clone(),
        deny_read: patterns,
        ..Default::default()
    };
    // Must not panic
    let _ = PolicyEngine::new(&policy);
}

#[test]
fn policy_glob_special_characters() {
    let special_patterns = &[
        "[!abc]*.rs",
        "{src,lib}/**/*.rs",
        "file[0-9].txt",
        "**/?single.rs",
        "path/to/[[]bracket.txt",
    ];
    for pattern in special_patterns {
        let policy = PolicyProfile {
            deny_write: vec![(*pattern).into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy);
        if let Ok(engine) = engine {
            let _ = engine.can_write_path(Path::new("src/test.rs"));
        }
    }
}

#[test]
fn policy_tool_names_with_special_chars() {
    let long_name = "a".repeat(10000);
    let tool_names: &[&str] = &[
        "",
        " ",
        "bash; rm -rf /",
        "tool\x00name",
        "../escape",
        "🔧",
        "tool\nname",
        &long_name,
    ];
    let policy = PolicyProfile {
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        for name in tool_names {
            let _ = engine.can_use_tool(name);
        }
    }
}

#[test]
fn policy_glob_include_exclude_edge_cases() {
    // Empty include + empty exclude
    let g1 = IncludeExcludeGlobs::new(&[], &[]);
    assert!(g1.is_ok());
    if let Ok(g) = g1 {
        assert!(g.decide_path(Path::new("anything.txt")).is_allowed());
    }

    // Exclude everything
    let g2 = IncludeExcludeGlobs::new(&[], &["**".into()]);
    if let Ok(g) = g2 {
        assert!(!g.decide_path(Path::new("file.rs")).is_allowed());
    }

    // Include nothing (empty include means include all)
    let g3 = IncludeExcludeGlobs::new(&["*.rs".into()], &[]);
    if let Ok(g) = g3 {
        assert!(g.decide_path(Path::new("main.rs")).is_allowed());
        assert!(!g.decide_path(Path::new("main.txt")).is_allowed());
    }

    // Conflicting: include *.rs but exclude *.rs
    let g4 = IncludeExcludeGlobs::new(&["*.rs".into()], &["*.rs".into()]);
    if let Ok(g) = g4 {
        // Exclude should take precedence
        assert!(!g.decide_path(Path::new("main.rs")).is_allowed());
    }
}

// ===========================================================================
// 6. Additional edge-case tests (bonus coverage)
// ===========================================================================

#[test]
fn streaming_codec_empty_input() {
    let results = StreamingCodec::decode_batch("");
    assert!(results.is_empty());
    assert_eq!(StreamingCodec::line_count(""), 0);
    let errors = StreamingCodec::validate_jsonl("");
    assert!(errors.is_empty());
}

#[test]
fn receipt_hash_excludes_hash_field() {
    // Core invariant: receipt_hash nulls the hash field before hashing.
    // Verify by hashing a receipt with and without a pre-existing hash.
    let r1 = make_receipt("test", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("decafbad".repeat(8));

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(
        h1, h2,
        "hash must be identical regardless of pre-existing receipt_sha256 value"
    );
}

#[test]
fn envelope_encode_never_contains_raw_newlines() {
    let events_with_newlines = vec![
        AgentEventKind::AssistantMessage {
            text: "line1\nline2\nline3".into(),
        },
        AgentEventKind::Error {
            message: "error\n\n\nmultiline".into(),
            error_code: None,
        },
    ];
    for kind in events_with_newlines {
        let event = AgentEvent {
            ts: chrono::Utc::now(),
            kind,
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event,
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        // JSONL: each envelope must be exactly one line (trailing \n allowed)
        let line_count = encoded.trim_end().matches('\n').count();
        assert_eq!(
            line_count, 0,
            "encoded envelope must not contain internal newlines"
        );
    }
}
