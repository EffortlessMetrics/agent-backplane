// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive fuzz-style tests for protocol parsing, JSON handling, and security
//! boundaries.  These are deterministic regression tests that exercise edge-case
//! inputs which could cause panics, data corruption, or security issues.
#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use abp_capability::{
    CapabilityRegistry, CompatibilityReport, NegotiationResult, check_capability, generate_report,
    negotiate_capabilities,
};
use abp_config::{BackendEntry, BackplaneConfig, parse_toml, validate_config};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, ReceiptBuilder, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_ir::lower::{
    lower_to_claude, lower_to_codex, lower_to_copilot, lower_to_gemini, lower_to_kimi,
    lower_to_openai,
};
use abp_ir::normalize;
use abp_policy::PolicyEngine;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
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

// ===========================================================================
// 7. Fuzz: JSONL envelope parsing — invalid JSON, truncated, missing fields
// ===========================================================================

#[test]
fn fuzz_jsonl_control_characters_no_panic() {
    // ASCII control chars 0x01–0x1F (except \t, \n, \r which are whitespace)
    for byte in 1u8..=31 {
        let input = format!("{{\"t\":\"fatal\",\"error\":\"x{}y\"}}", byte as char);
        let _ = JsonlCodec::decode(&input);
    }
}

#[test]
fn fuzz_jsonl_trailing_comma_variants() {
    let inputs = &[
        r#"{"t":"fatal","error":"x",}"#,
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x",},"capabilities":{}}"#,
        r#"{"t":"fatal","error":"x","error_code":"sidecar_crashed",}"#,
    ];
    for input in inputs {
        let result = JsonlCodec::decode(input);
        assert!(result.is_err(), "trailing comma must fail: {}", input);
    }
}

#[test]
fn fuzz_jsonl_number_edge_values_in_fields() {
    let inputs = &[
        r#"{"t":"fatal","error":"NaN"}"#,
        r#"{"t":"fatal","error":"Infinity"}"#,
        r#"{"t":"fatal","error":""}"#,
        // Integer overflow in unexpected places
        r#"{"t":"run","id":"x","work_order":99999999999999999999}"#,
    ];
    for input in inputs {
        let _ = JsonlCodec::decode(input);
    }
}

#[test]
fn fuzz_jsonl_bom_prefix() {
    // UTF-8 BOM before valid JSON
    let bom = "\u{FEFF}";
    let input = format!("{}{{\"t\":\"fatal\",\"error\":\"boom\"}}", bom);
    let _ = JsonlCodec::decode(&input);
    // Must not panic; may or may not parse depending on BOM handling
}

#[test]
fn fuzz_jsonl_multiple_json_objects_on_one_line() {
    let input = r#"{"t":"fatal","error":"a"}{"t":"fatal","error":"b"}"#;
    let result = JsonlCodec::decode(input);
    // Should fail or only parse the first object — must not panic
    let _ = result;
}

#[test]
fn fuzz_jsonl_extremely_long_string_key() {
    let long_key = "k".repeat(1_000_000);
    let input = format!(r#"{{"t":"fatal","{}":"value","error":"x"}}"#, long_key);
    let _ = JsonlCodec::decode(&input);
}

// ===========================================================================
// 8. Fuzz: WorkOrder construction — invalid UUIDs, empty fields, oversized
// ===========================================================================

#[test]
fn fuzz_workorder_invalid_uuid_deserialization() {
    let invalid_uuids = &[
        "not-a-uuid",
        "",
        "00000000-0000-0000-0000",               // truncated
        "00000000-0000-0000-0000-0000000000000", // too long
        "ZZZZZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZZZZZZZZZ",  // invalid hex
        "00000000000000000000000000000000",      // missing dashes
        "null",
    ];
    for bad_uuid in invalid_uuids {
        let json = format!(
            r#"{{"id":"{}","task":"test","lane":"patch_first","workspace":{{"root":".","mode":"staged","include":[],"exclude":[]}},"context":{{"files":[],"snippets":[]}},"policy":{{}},"requirements":{{"required":[]}},"config":{{"vendor":{{}},"env":{{}}}}}}"#,
            bad_uuid
        );
        let result = serde_json::from_str::<WorkOrder>(&json);
        assert!(
            result.is_err(),
            "invalid UUID '{}' must fail to deserialize",
            bad_uuid
        );
    }
}

#[test]
fn fuzz_workorder_all_fields_null_json() {
    let json = r#"{"id":null,"task":null,"lane":null,"workspace":null,"context":null,"policy":null,"requirements":null,"config":null}"#;
    let result = serde_json::from_str::<WorkOrder>(json);
    assert!(result.is_err(), "all-null WorkOrder must fail");
}

#[test]
fn fuzz_workorder_extra_nested_vendor_config() {
    // Deeply nested vendor config should not panic
    let mut depth = serde_json::Value::String("leaf".into());
    for _ in 0..200 {
        let mut map = serde_json::Map::new();
        map.insert("nested".into(), depth);
        depth = serde_json::Value::Object(map);
    }
    let wo = WorkOrderBuilder::new("test").build();
    let mut json_val = serde_json::to_value(&wo).unwrap();
    json_val["config"]["vendor"]["deep"] = depth;
    let result = serde_json::from_value::<WorkOrder>(json_val);
    // Should succeed — vendor config accepts arbitrary JSON
    assert!(result.is_ok(), "deep vendor config should be tolerated");
}

#[test]
fn fuzz_workorder_negative_budget_and_turns() {
    // JSON with negative numbers for unsigned/float fields
    let wo = WorkOrderBuilder::new("test").build();
    let mut json_val = serde_json::to_value(&wo).unwrap();
    json_val["config"]["max_budget_usd"] = json!(-1.0);
    let _ = serde_json::from_value::<WorkOrder>(json_val.clone());

    json_val["config"]["max_turns"] = json!(-1);
    let result = serde_json::from_value::<WorkOrder>(json_val);
    // Negative u32 should fail deserialization
    assert!(
        result.is_err(),
        "negative max_turns must fail for u32 field"
    );
}

#[test]
fn fuzz_workorder_nan_infinity_budget() {
    let wo = WorkOrderBuilder::new("test").build();
    let full_json = serde_json::to_string(&wo).unwrap();
    // Replace budget with NaN (invalid JSON)
    let nan_json = full_json.replace("null", "NaN");
    let _ = serde_json::from_str::<WorkOrder>(&nan_json);

    // Replace with Infinity
    let inf_json = full_json.replace("null", "Infinity");
    let _ = serde_json::from_str::<WorkOrder>(&inf_json);
    // Must not panic
}

// ===========================================================================
// 9. Fuzz: Receipt manipulation — hash tampering, field removal, type confusion
// ===========================================================================

#[test]
fn fuzz_receipt_outcome_type_confusion() {
    let r = make_hashed_receipt("test");
    let mut json_val = serde_json::to_value(&r).unwrap();
    // Replace outcome string with integer
    json_val["outcome"] = json!(42);
    let result = serde_json::from_value::<Receipt>(json_val.clone());
    assert!(result.is_err(), "integer outcome must fail");

    // Replace with object
    json_val["outcome"] = json!({"status": "complete"});
    let result = serde_json::from_value::<Receipt>(json_val);
    assert!(result.is_err(), "object outcome must fail");
}

#[test]
fn fuzz_receipt_remove_required_fields_one_by_one() {
    let r = make_hashed_receipt("test");
    let json_val = serde_json::to_value(&r).unwrap();
    let required_fields = ["meta", "backend", "outcome", "capabilities"];
    for field in &required_fields {
        let mut modified = json_val.clone();
        modified.as_object_mut().unwrap().remove(*field);
        let result = serde_json::from_value::<Receipt>(modified);
        assert!(
            result.is_err(),
            "receipt with '{}' removed must fail",
            field
        );
    }
}

#[test]
fn fuzz_receipt_hash_with_empty_trace() {
    let r1 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    // Different run_ids mean different hashes even with empty traces
    assert_ne!(h1, h2, "distinct run_ids must produce different hashes");
}

#[test]
fn fuzz_receipt_enormous_trace() {
    let mut builder = ReceiptBuilder::new("test").outcome(Outcome::Complete);
    for i in 0..1000 {
        let event = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("chunk-{}", i),
            },
            ext: None,
        };
        builder = builder.add_trace_event(event);
    }
    let r = builder.build();
    let hash = receipt_hash(&r);
    assert!(hash.is_ok(), "hashing 1000-event receipt must succeed");
}

#[test]
fn fuzz_receipt_all_outcome_variants_roundtrip() {
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    for outcome in &outcomes {
        let r = make_receipt("test", outcome.clone());
        let json = serde_json::to_string(&r).unwrap();
        let roundtrip: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(&r.outcome).unwrap(),
            serde_json::to_value(&roundtrip.outcome).unwrap()
        );
    }
}

#[test]
fn fuzz_receipt_meta_fields_replaced_with_wrong_types() {
    let r = make_hashed_receipt("test");
    let mut json_val = serde_json::to_value(&r).unwrap();
    // Replace meta.run_id with an integer
    json_val["meta"]["run_id"] = json!(12345);
    let result = serde_json::from_value::<Receipt>(json_val);
    assert!(result.is_err(), "integer run_id must fail");
}

// ===========================================================================
// 10. Fuzz: Policy evaluation — conflicting rules, deep nested patterns
// ===========================================================================

#[test]
fn fuzz_policy_many_overlapping_patterns() {
    // 100 overlapping glob patterns
    let mut deny_write: Vec<String> = Vec::new();
    for depth in 0..100 {
        deny_write.push(format!("{}/**/*.rs", "a/".repeat(depth)));
    }
    let policy = PolicyProfile {
        deny_write,
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy);
    if let Ok(engine) = engine {
        let _ = engine.can_write_path(Path::new("a/a/a/a/deep/file.rs"));
    }
}

#[test]
fn fuzz_policy_simultaneous_allow_deny_read_write() {
    let policy = PolicyProfile {
        deny_read: vec!["**/*.secret".into()],
        deny_write: vec!["**/*.config".into()],
        allowed_tools: vec!["bash".into(), "python".into()],
        disallowed_tools: vec!["python".into(), "node".into()],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        // bash is allowed-only
        assert!(engine.can_use_tool("bash").allowed);
        // python is in both allow and deny — deny wins
        assert!(!engine.can_use_tool("python").allowed);
        // node is denied
        assert!(!engine.can_use_tool("node").allowed);
        // read checks
        assert!(!engine.can_read_path(Path::new("keys.secret")).allowed);
        assert!(engine.can_read_path(Path::new("app.config")).allowed);
        // write checks
        assert!(!engine.can_write_path(Path::new("app.config")).allowed);
    }
}

#[test]
fn fuzz_policy_deeply_nested_directory_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/secret/**".into()],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        let deep_path = format!("{}/secret/file.txt", "a/b/c/".repeat(50));
        let _ = engine.can_write_path(Path::new(&deep_path));
    }
}

#[test]
fn fuzz_policy_symlink_like_paths() {
    let tricky_paths = &[
        "src/./main.rs",
        "src//double//slash.rs",
        "src/sub/../main.rs",
        "src/.hidden/file.rs",
        "src/...triple.rs",
    ];
    let policy = PolicyProfile {
        deny_write: vec!["src/**".into()],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        for path in tricky_paths {
            let _ = engine.can_write_path(Path::new(path));
            // Must not panic
        }
    }
}

#[test]
fn fuzz_policy_all_empty_lists() {
    let policy = PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec![],
        ..Default::default()
    };
    if let Ok(engine) = PolicyEngine::new(&policy) {
        // Everything should be allowed
        assert!(engine.can_use_tool("anything").allowed);
        assert!(engine.can_read_path(Path::new("any/file.rs")).allowed);
        assert!(engine.can_write_path(Path::new("any/file.rs")).allowed);
    }
}

// ===========================================================================
// 11. Fuzz: Capability negotiation — impossible features, version mismatches
// ===========================================================================

#[test]
fn fuzz_capability_negotiate_empty_manifest_all_required() {
    let manifest: CapabilityManifest = BTreeMap::new();
    let required = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolBash,
        Capability::Vision,
        Capability::ExtendedThinking,
    ];
    let result = negotiate_capabilities(&required, &manifest);
    assert!(
        !result.unsupported.is_empty(),
        "empty manifest cannot satisfy any requirements"
    );
}

#[test]
fn fuzz_capability_negotiate_all_native() {
    let mut manifest: CapabilityManifest = BTreeMap::new();
    let caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ];
    for cap in &caps {
        manifest.insert(cap.clone(), SupportLevel::Native);
    }
    let result = negotiate_capabilities(&caps, &manifest);
    assert!(result.is_viable(), "fully native manifest should be viable");
    assert_eq!(result.native.len(), 3);
}

#[test]
fn fuzz_capability_negotiate_duplicate_requirements() {
    let mut manifest: CapabilityManifest = BTreeMap::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    // Duplicate the same requirement
    let required = vec![
        Capability::Streaming,
        Capability::Streaming,
        Capability::Streaming,
    ];
    let result = negotiate_capabilities(&required, &manifest);
    // Must not panic; native count might be 1 or 3 depending on dedup
    let _ = result;
}

#[test]
fn fuzz_capability_version_parsing_edge_cases() {
    let cases: &[(&str, Option<(u32, u32)>)] = &[
        ("abp/v0.1", Some((0, 1))),
        ("abp/v1.0", Some((1, 0))),
        ("abp/v999.999", Some((999, 999))),
        ("abp/v0.0", Some((0, 0))),
        ("", None),
        ("abp/v", None),
        ("abp/v1", None),
        ("abp/v1.", None),
        ("abp/v.1", None),
        ("abp/v-1.0", None),
        ("xyz/v0.1", None),
        ("abp/v0.1.0", None),
        ("ABP/V0.1", None),
    ];
    for (input, expected) in cases {
        let result = parse_version(input);
        assert_eq!(
            result, *expected,
            "parse_version({:?}) expected {:?}, got {:?}",
            input, expected, result
        );
    }
}

#[test]
fn fuzz_capability_version_compatibility() {
    // Same major version -> compatible
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.9"));
    // Different major version -> incompatible
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    // Invalid versions -> incompatible
    assert!(!is_compatible_version("garbage", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
    assert!(!is_compatible_version("", ""));
}

#[test]
fn fuzz_capability_registry_unknown_backend() {
    let registry = CapabilityRegistry::with_defaults();
    let result = registry.negotiate_by_name("nonexistent_backend_xyz", &[Capability::Streaming]);
    assert!(
        result.is_none(),
        "unknown backend must return None from registry"
    );
}

// ===========================================================================
// 12. Fuzz: IR translation — invalid content blocks, missing roles, empty msgs
// ===========================================================================

#[test]
fn fuzz_ir_empty_conversation_lowering() {
    let conv = IrConversation::new();
    let tools: Vec<IrToolDefinition> = vec![];
    // All lowering targets must handle empty conversations without panic
    let _ = lower_to_openai(&conv, &tools);
    let _ = lower_to_claude(&conv, &tools);
    let _ = lower_to_gemini(&conv, &tools);
    let _ = lower_to_kimi(&conv, &tools);
    let _ = lower_to_codex(&conv, &tools);
    let _ = lower_to_copilot(&conv, &tools);
}

#[test]
fn fuzz_ir_empty_text_blocks() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, ""),
        IrMessage::text(IrRole::Assistant, ""),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: String::new(),
            }],
        ),
    ]);
    let tools: Vec<IrToolDefinition> = vec![];
    let _ = lower_to_openai(&conv, &tools);
    let _ = lower_to_claude(&conv, &tools);
}

#[test]
fn fuzz_ir_tool_use_with_invalid_input() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "do something"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "".into(),
                name: "".into(),
                input: json!(null),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "".into(),
                content: vec![],
                is_error: true,
            }],
        ),
    ]);
    let _ = lower_to_openai(&conv, &[]);
    let _ = lower_to_claude(&conv, &[]);
}

#[test]
fn fuzz_ir_deeply_nested_tool_results() {
    // ToolResult containing another ToolResult (unusual but should not panic)
    let inner = IrContentBlock::ToolResult {
        tool_use_id: "inner".into(),
        content: vec![IrContentBlock::Text {
            text: "deep".into(),
        }],
        is_error: false,
    };
    let outer = IrContentBlock::ToolResult {
        tool_use_id: "outer".into(),
        content: vec![inner],
        is_error: false,
    };
    let conv = IrConversation::from_messages(vec![IrMessage::new(IrRole::Tool, vec![outer])]);
    let _ = lower_to_openai(&conv, &[]);
    let _ = lower_to_claude(&conv, &[]);
}

#[test]
fn fuzz_ir_message_with_all_block_types_mixed() {
    let blocks = vec![
        IrContentBlock::Text {
            text: "hello".into(),
        },
        IrContentBlock::Thinking {
            text: "reasoning...".into(),
        },
        IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "bash".into(),
            input: json!({"cmd": "ls"}),
        },
        IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        },
    ];
    let conv = IrConversation::from_messages(vec![IrMessage::new(IrRole::Assistant, blocks)]);
    // All lowering functions must handle mixed block types
    let _ = lower_to_openai(&conv, &[]);
    let _ = lower_to_claude(&conv, &[]);
    let _ = lower_to_gemini(&conv, &[]);
}

#[test]
fn fuzz_ir_normalize_empty_and_whitespace() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "   "),
        IrMessage::text(IrRole::User, "\t\n"),
        IrMessage::text(IrRole::Assistant, ""),
        IrMessage::new(IrRole::User, vec![]),
    ]);
    let normalized = normalize::normalize(&conv);
    // Must not panic; empty messages should be stripped
    let _ = normalized;
}

#[test]
fn fuzz_ir_normalize_dedup_multiple_system() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "system 1"),
        IrMessage::text(IrRole::System, "system 2"),
        IrMessage::text(IrRole::System, "system 3"),
        IrMessage::text(IrRole::User, "hello"),
    ]);
    let deduped = normalize::dedup_system(&conv);
    let system_count = deduped.messages_by_role(IrRole::System).len();
    assert!(
        system_count <= 1,
        "dedup_system should leave at most 1 system message, got {}",
        system_count
    );
}

#[test]
fn fuzz_ir_conversation_role_accessors() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "q1"),
        IrMessage::text(IrRole::Assistant, "a1"),
        IrMessage::text(IrRole::User, "q2"),
    ]);
    assert!(conv.system_message().is_none());
    assert_eq!(conv.last_assistant().unwrap().text_content(), "a1");
    assert_eq!(conv.last_message().unwrap().text_content(), "q2");
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert!(conv.tool_calls().is_empty());
}

#[test]
fn fuzz_ir_tool_definition_empty_parameters() {
    let tools = vec![
        IrToolDefinition {
            name: "".into(),
            description: "".into(),
            parameters: json!(null),
        },
        IrToolDefinition {
            name: "tool".into(),
            description: "desc".into(),
            parameters: json!({}),
        },
        IrToolDefinition {
            name: "evil<script>".into(),
            description: "'; DROP TABLE;--".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
    ];
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "test")]);
    let _ = lower_to_openai(&conv, &tools);
    let _ = lower_to_claude(&conv, &tools);
}

// ===========================================================================
// 13. Fuzz: Config parsing — malformed TOML, type mismatches, boundary values
// ===========================================================================

#[test]
fn fuzz_config_empty_string() {
    let result = parse_toml("");
    // Empty TOML is valid; should produce default config
    assert!(result.is_ok(), "empty TOML should parse to defaults");
}

#[test]
fn fuzz_config_completely_invalid_toml() {
    let invalid_inputs = &[
        "{{{{",
        "not toml at all!!!",
        "[[[triple bracket]]]",
        "key = {unclosed",
        "= value_without_key",
        "\x00\x01\x02",
    ];
    for input in invalid_inputs {
        let result = parse_toml(input);
        assert!(result.is_err(), "invalid TOML must fail: {:?}", input);
    }
}

#[test]
fn fuzz_config_type_mismatches() {
    let inputs = &[
        // port as string instead of integer
        r#"port = "not_a_number""#,
        // log_level as integer instead of string
        r#"log_level = 42"#,
        // backends as string instead of table
        r#"backends = "invalid""#,
        // policy_profiles as integer instead of array
        r#"policy_profiles = 99"#,
    ];
    for input in inputs {
        let result = parse_toml(input);
        assert!(result.is_err(), "type mismatch must fail: {:?}", input);
    }
}

#[test]
fn fuzz_config_boundary_port_values() {
    // Valid port
    let result = parse_toml("port = 8080");
    assert!(result.is_ok());

    // Port 0 — allowed by TOML, validation may warn
    let result = parse_toml("port = 0");
    if let Ok(cfg) = &result {
        let _ = validate_config(cfg);
    }

    // Max u16
    let result = parse_toml("port = 65535");
    assert!(result.is_ok());

    // Overflow u16
    let result = parse_toml("port = 70000");
    assert!(result.is_err(), "port > 65535 must fail for u16");
}

#[test]
fn fuzz_config_backend_sidecar_missing_command() {
    let toml = r#"
[backends.test]
type = "sidecar"
args = ["--flag"]
"#;
    let result = parse_toml(toml);
    // Missing 'command' in sidecar — should error or produce incomplete config
    let _ = result;
}

#[test]
fn fuzz_config_validate_invalid_log_levels() {
    let levels = &["INVALID", "verbose", "TRACE", "Debug", "", "   "];
    for level in levels {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..Default::default()
        };
        let result = validate_config(&cfg);
        // Invalid log levels should produce a validation error
        assert!(
            result.is_err(),
            "invalid log_level '{}' must fail validation",
            level
        );
    }
}

#[test]
fn fuzz_config_sidecar_extreme_timeout() {
    let toml = r#"
[backends.slow]
type = "sidecar"
command = "slow-agent"
timeout_secs = 999999
"#;
    let result = parse_toml(toml);
    if let Ok(cfg) = &result {
        let warnings = validate_config(cfg);
        if let Ok(ws) = warnings {
            // Should have a large-timeout warning
            let has_timeout_warning = ws
                .iter()
                .any(|w| matches!(w, abp_config::ConfigWarning::LargeTimeout { .. }));
            assert!(
                has_timeout_warning,
                "extreme timeout should produce warning"
            );
        }
    }
}

// ===========================================================================
// 14. Fuzz: Stream event sequences — out-of-order, duplicates, gaps
// ===========================================================================

#[test]
fn fuzz_stream_events_out_of_order() {
    use chrono::Utc;
    // Events that logically should be in order but aren't
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        },
    ];
    // Build a receipt with out-of-order events — should not panic
    let mut builder = ReceiptBuilder::new("test").outcome(Outcome::Complete);
    for e in events {
        builder = builder.add_trace_event(e);
    }
    let receipt = builder.build();
    let _ = receipt.with_hash();
}

#[test]
fn fuzz_stream_duplicate_events() {
    use chrono::Utc;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        ext: None,
    };
    let mut builder = ReceiptBuilder::new("test").outcome(Outcome::Complete);
    for _ in 0..100 {
        builder = builder.add_trace_event(event.clone());
    }
    let receipt = builder.build();
    assert_eq!(receipt.trace.len(), 100);
    let _ = receipt.with_hash();
}

#[test]
fn fuzz_stream_tool_result_without_tool_call() {
    use chrono::Utc;
    // ToolResult event without preceding ToolCall — should still serialize
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("orphan_id".into()),
                output: json!("result without call"),
                is_error: false,
            },
            ext: None,
        },
    ];
    let mut builder = ReceiptBuilder::new("test").outcome(Outcome::Complete);
    for e in events {
        builder = builder.add_trace_event(e);
    }
    let receipt = builder.build();
    let json = serde_json::to_string(&receipt).unwrap();
    let _: Receipt = serde_json::from_str(&json).unwrap();
}

#[test]
fn fuzz_stream_events_as_envelope_batch() {
    use chrono::Utc;
    let ref_id = "run-123";
    let envelopes: Vec<Envelope> = (0..20)
        .map(|i| Envelope::Event {
            ref_id: ref_id.into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{}", i),
                },
                ext: None,
            },
        })
        .collect();
    let batch = StreamingCodec::encode_batch(&envelopes);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 20);
    for (i, r) in results.iter().enumerate() {
        assert!(r.is_ok(), "event {} must decode successfully", i);
    }
}

#[test]
fn fuzz_stream_interleaved_valid_invalid_lines() {
    let valid_fatal = r#"{"t":"fatal","error":"ok"}"#;
    let invalid = r#"{"t":"unknown_garbage"}"#;
    let bad_json = "not json at all";
    let batch = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        valid_fatal, invalid, bad_json, valid_fatal, invalid
    );
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 5);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_err());
    assert!(results[3].is_ok());
    assert!(results[4].is_err());
}

#[test]
fn fuzz_stream_final_envelope_with_minimal_receipt() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(&encoded).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));
}

// ===========================================================================
// 15. Fuzz: Cross-concern edge cases
// ===========================================================================

#[test]
fn fuzz_envelope_run_with_policy_that_denies_everything() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        deny_write: vec!["**".into()],
        disallowed_tools: vec!["*".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("test").policy(policy).build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(&encoded).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        if let Ok(engine) = PolicyEngine::new(&work_order.policy) {
            assert!(!engine.can_use_tool("bash").allowed);
        }
    }
}

#[test]
fn fuzz_receipt_with_special_backend_ids() {
    let long_id = "a".repeat(10000);
    let special_ids: &[&str] = &[
        "",
        " ",
        "🤖",
        "backend\nwith\nnewlines",
        &long_id,
        "<script>alert(1)</script>",
    ];
    for id in special_ids {
        let r = ReceiptBuilder::new(*id).outcome(Outcome::Complete).build();
        let hash = receipt_hash(&r);
        assert!(hash.is_ok(), "special backend id {:?} must hash", id);
        let json = serde_json::to_string(&r).unwrap();
        let roundtrip: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.backend.id, *id);
    }
}

#[test]
fn fuzz_workorder_roundtrip_through_envelope() {
    // Build a maximally-populated work order and roundtrip through envelope
    let wo = WorkOrderBuilder::new("complex task 🎯")
        .root("/tmp/workspace")
        .model("gpt-4o")
        .max_turns(100)
        .max_budget_usd(50.0)
        .lane(ExecutionLane::WorkspaceFirst)
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["**/*.rs".into(), "**/*.toml".into()])
        .exclude(vec!["target/**".into()])
        .build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo.clone(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(&encoded).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.task, wo.task);
        assert_eq!(work_order.id, wo.id);
    } else {
        panic!("expected Run envelope");
    }
}

#[test]
fn fuzz_ir_roundtrip_through_json() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello!"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking...".into(),
                },
                IrContentBlock::Text {
                    text: "Hi there!".into(),
                },
            ],
        ),
    ]);
    let json = serde_json::to_string(&conv).unwrap();
    let roundtrip: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, roundtrip);
}

#[test]
fn fuzz_ir_image_block_with_huge_data() {
    // Simulate a large base64-encoded image
    let huge_data = "A".repeat(1_000_000);
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: huge_data,
        }],
    )]);
    let _ = lower_to_openai(&conv, &[]);
    let _ = lower_to_claude(&conv, &[]);
}

#[test]
fn fuzz_config_roundtrip_mock_backend() {
    let toml_str = r#"
default_backend = "mock"
log_level = "debug"

[backends.mock]
type = "mock"
"#;
    let cfg = parse_toml(toml_str).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert!(cfg.backends.contains_key("mock"));
    let warnings = validate_config(&cfg);
    assert!(warnings.is_ok());
}

#[test]
fn fuzz_capability_report_generation() {
    let mut manifest: CapabilityManifest = BTreeMap::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Unsupported);
    manifest.insert(Capability::Vision, SupportLevel::Emulated);

    let required = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::Vision,
    ];
    let result = negotiate_capabilities(&required, &manifest);
    let report = generate_report(&result);

    assert!(report.native_count >= 1);
    assert!(report.summary.len() > 0, "report summary must not be empty");
}

#[test]
fn fuzz_glob_pattern_catastrophic_backtracking() {
    // Patterns known to cause exponential matching in naive implementations
    let pattern = "a]".repeat(100);
    let _ = IncludeExcludeGlobs::new(&[pattern], &[]);

    let long_stars = "**/*".repeat(20);
    let result = IncludeExcludeGlobs::new(&[long_stars], &[]);
    if let Ok(g) = result {
        // Should complete quickly, not hang
        let _ = g.decide_path(Path::new("a/b/c/d/e/f/g/h/i/j/k/l/m.txt"));
    }
}

#[test]
fn fuzz_streaming_codec_line_count_edge_cases() {
    assert_eq!(StreamingCodec::line_count(""), 0);
    assert_eq!(StreamingCodec::line_count("\n"), 0);
    assert_eq!(StreamingCodec::line_count("\n\n\n"), 0);
    assert_eq!(StreamingCodec::line_count("a"), 1);
    assert_eq!(StreamingCodec::line_count("a\nb"), 2);
    assert_eq!(StreamingCodec::line_count("a\nb\n"), 2);
    assert_eq!(StreamingCodec::line_count("a\n\nb"), 2);
}
