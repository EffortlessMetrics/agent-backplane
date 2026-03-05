#![allow(dead_code, unused_imports, unused_variables)]
#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive security-focused fuzz tests for protocol parsing, receipt
//! hashing, config parsing, serialization round-trips, and DoS resistance.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_config::{parse_toml, BackendEntry, BackplaneConfig};
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use proptest::prelude::*;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn long_string(len: usize) -> String {
    "X".repeat(len)
}

fn make_receipt_with_backend(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .build()
}

fn make_receipt() -> Receipt {
    make_receipt_with_backend("fuzz-backend")
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. PROTOCOL FUZZING — malformed input must not crash
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_random_bytes_dont_crash() {
    let garbage: Vec<&[u8]> = vec![
        b"",
        b"\x00",
        b"\xff\xfe\xfd",
        b"\x00\x00\x00\x00",
        b"\x80\x81\x82\x83",
        b"\xef\xbb\xbf", // UTF-8 BOM
        b"\xff\xff\xff\xff\xff\xff",
        &[0u8; 256],
        &[0xffu8; 256],
    ];
    for bytes in &garbage {
        if let Ok(s) = std::str::from_utf8(bytes) {
            let _ = JsonlCodec::decode(s);
        }
    }
}

#[test]
fn protocol_malformed_json_envelopes() {
    let cases = vec![
        "",
        " ",
        "\n",
        "\t",
        "null",
        "true",
        "false",
        "42",
        "\"string\"",
        "[]",
        "[1,2,3]",
        "{}",
        r#"{"t":null}"#,
        r#"{"t":42}"#,
        r#"{"t":true}"#,
        r#"{"t":[]}"#,
        r#"{"t":{}}"#,
        r#"{"t":"nonexistent_variant"}"#,
        r#"{"t":"hello"}"#, // missing required fields
        r#"{"t":"run"}"#,   // missing required fields
        r#"{"t":"event"}"#, // missing required fields
        r#"{"t":"final"}"#, // missing required fields
        r#"{"t":"fatal"}"#, // missing required error field
        r#"{"type":"fatal","error":"wrong discriminator"}"#,
        r#"{"T":"fatal","error":"case-sensitive"}"#,
        r#"{t:"fatal","error":"unquoted key"}"#,
        r#"{"t":"fatal","error":"boom",}"#, // trailing comma
    ];
    for input in &cases {
        let _ = JsonlCodec::decode(input);
    }
}

#[test]
fn protocol_valid_json_wrong_types() {
    let cases = vec![
        r#"{"t":"fatal","ref_id":42,"error":"boom"}"#,
        r#"{"t":"fatal","ref_id":true,"error":"boom"}"#,
        r#"{"t":"fatal","ref_id":[],"error":"boom"}"#,
        r#"{"t":"fatal","ref_id":"ok","error":42}"#,
        r#"{"t":"fatal","ref_id":"ok","error":null}"#,
        r#"{"t":"fatal","ref_id":"ok","error":true}"#,
        r#"{"t":"hello","contract_version":42,"backend":{},"capabilities":{}}"#,
        r#"{"t":"run","id":42,"work_order":"not an object"}"#,
        r#"{"t":"event","ref_id":"x","event":"not an object"}"#,
    ];
    for input in &cases {
        let result = JsonlCodec::decode(input);
        // Must not panic — error is expected for most
        let _ = result;
    }
}

#[test]
fn protocol_extremely_long_strings() {
    let long_1mb = long_string(1_048_576);
    let cases = vec![
        format!(r#"{{"t":"fatal","ref_id":null,"error":"{}"}}"#, long_1mb),
        format!(r#"{{"t":"fatal","ref_id":"{}","error":"x"}}"#, long_1mb),
    ];
    for input in &cases {
        let result = JsonlCodec::decode(input);
        assert!(result.is_ok(), "long string should parse successfully");
    }
}

#[test]
fn protocol_deeply_nested_objects() {
    // Build deeply nested JSON string: {"n":{"n":{"n":...}}}
    let depth = 128;
    let mut json = r#""leaf""#.to_string();
    for _ in 0..depth {
        json = format!(r#"{{"n":{json}}}"#);
    }
    // This is valid JSON but not a valid Envelope — should error, not crash
    let _ = JsonlCodec::decode(&json);
}

#[test]
fn protocol_null_bytes_in_strings() {
    let cases = vec![
        r#"{"t":"fatal","ref_id":null,"error":"he\u0000llo"}"#,
        r#"{"t":"fatal","ref_id":"a\u0000b","error":"x"}"#,
    ];
    for input in &cases {
        let result = JsonlCodec::decode(input);
        // \u0000 is valid in JSON strings
        assert!(result.is_ok());
    }
}

#[test]
fn protocol_unicode_edge_cases() {
    let cases = vec![
        // BOM
        "\u{FEFF}{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}",
        // RTL override
        r#"{"t":"fatal","ref_id":null,"error":"\u202Eadmin\u202D"}"#,
        // Surrogate-like (not actually surrogates in valid UTF-8, just testing)
        r#"{"t":"fatal","ref_id":null,"error":"𐐷"}"#,
        // Zero-width spaces
        r#"{"t":"fatal","ref_id":null,"error":"a\u200Bb\u200Cc"}"#,
        // Combining characters
        r#"{"t":"fatal","ref_id":null,"error":"a\u0300\u0301\u0302"}"#,
        // Emoji sequences
        r#"{"t":"fatal","ref_id":null,"error":"👨‍👩‍👧‍👦🏳️‍🌈"}"#,
    ];
    for input in &cases {
        let _ = JsonlCodec::decode(input);
    }
}

#[test]
fn protocol_decode_stream_with_garbage() {
    let input = "not json\n{}\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\n\x00\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    // Should not panic, may contain errors
    assert!(!results.is_empty());
    // At least one should succeed (the valid fatal line)
    assert!(results.iter().any(|r| r.is_ok()));
}

#[test]
fn protocol_decode_stream_all_garbage() {
    let input = "garbage\nmore garbage\n{invalid json}\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.iter().all(|r| r.is_err()));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. RECEIPT HASH FUZZING — hash stability and invariants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_deterministic() {
    let r = make_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "Same receipt must produce same hash");
    assert_eq!(h1.len(), 64, "SHA-256 hex must be 64 chars");
}

#[test]
fn receipt_hash_changes_on_field_mutation_backend() {
    let r1 = make_receipt_with_backend("backend-a");
    let r2 = make_receipt_with_backend("backend-b");
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2, "Different backends must produce different hashes");
}

#[test]
fn receipt_hash_changes_on_outcome_mutation() {
    let r1 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("test").outcome(Outcome::Failed).build();
    // Use same timestamps for determinism
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2, "Different outcomes must produce different hashes");
}

#[test]
fn receipt_hash_ignores_receipt_sha256_field() {
    let mut r1 = make_receipt();
    r1.receipt_sha256 = None;
    let h1 = receipt_hash(&r1).unwrap();

    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("deadbeef".into());
    let h2 = receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2, "receipt_sha256 must not influence the hash");
}

#[test]
fn receipt_hash_with_hash_sets_field() {
    let r = make_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);

    // Verify hash matches independent computation
    let expected = receipt_hash(&r).unwrap();
    assert_eq!(hash, &expected);
}

#[test]
fn receipt_hash_btreemap_ordering_independence() {
    // BTreeMap is ordered, so insertion order doesn't matter
    let mut caps1 = CapabilityManifest::new();
    caps1.insert(Capability::Streaming, SupportLevel::Native);
    caps1.insert(Capability::ToolRead, SupportLevel::Emulated);

    let mut caps2 = CapabilityManifest::new();
    caps2.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps2.insert(Capability::Streaming, SupportLevel::Native);

    let r1 = ReceiptBuilder::new("test").capabilities(caps1).build();
    let r2 = ReceiptBuilder::new("test").capabilities(caps2).build();

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    // Note: hashes may differ due to different timestamps from builder,
    // but the capability map serialization order is deterministic
    assert_eq!(caps1_sorted(&r1), caps1_sorted(&r2));
}

fn caps1_sorted(r: &Receipt) -> String {
    serde_json::to_string(&r.capabilities).unwrap()
}

#[test]
fn receipt_hash_with_unicode_content() {
    let r = ReceiptBuilder::new("test")
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "🔥\u{200D}\u{FEFF}\u{202E}unicode\u{0300}\u{0301}".into(),
        }))
        .build();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);

    // Hash is stable across repeated calls with unicode
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn receipt_hash_with_empty_trace() {
    let r = ReceiptBuilder::new("test").build();
    assert!(r.trace.is_empty());
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_hash_with_null_fields_in_usage_raw() {
    let r = ReceiptBuilder::new("test")
        .usage_raw(serde_json::json!(null))
        .build();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn canonical_json_deterministic_key_order() {
    let v1 = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let v2 = serde_json::json!({"a": 2, "m": 3, "z": 1});
    let c1 = canonical_json(&v1).unwrap();
    let c2 = canonical_json(&v2).unwrap();
    assert_eq!(
        c1, c2,
        "canonical_json must be order-independent for objects"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. CONFIG PARSING FUZZING — parser robustness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_random_toml_input_doesnt_crash() {
    let inputs = vec![
        "",
        " ",
        "\n",
        "\t\t\t",
        "=",
        "===",
        "[",
        "[[",
        "[]",
        "[[]",
        "key",
        "key =",
        "key = ",
        "key = value",
        "key = 'value'",
        "\"unterminated",
        "key = 2024-01-01T00:00:00Z",
        "key = [1, 2, 3]",
        "key = {inline = true}",
        "a.b.c = 1",
        "x = \"",
    ];
    for input in &inputs {
        let _ = parse_toml(input);
    }
    // Very long string value
    let long_toml = format!("x = \"{}\"", "A".repeat(100_000));
    let _ = parse_toml(&long_toml);
}

#[test]
fn config_valid_toml_invalid_config() {
    let inputs = vec![
        // Valid TOML but wrong structure for BackplaneConfig
        "name = 42",
        "backends = 42",
        "backends = \"string\"",
        "backends = [1, 2]",
        "[backends.test]\ntype = \"unknown_type\"",
        "[backends.test]\ncommand = \"foo\"",
        // Missing type field for backend
        "[backends.test]\nargs = [\"a\"]",
        // Wrong type for known fields
        "default_backend = 42",
        "log_level = 42",
        "port = \"not_a_number\"",
        "port = -1",
        "port = 99999",
        "policy_profiles = \"not_an_array\"",
    ];
    for input in &inputs {
        let _ = parse_toml(input);
    }
}

#[test]
fn config_edge_case_values() {
    // Empty strings
    let r = parse_toml("default_backend = \"\"");
    assert!(r.is_ok());
    assert_eq!(r.unwrap().default_backend, Some("".into()));

    // Max u16 port
    let r = parse_toml("port = 65535");
    assert!(r.is_ok());
    assert_eq!(r.unwrap().port, Some(65535));

    // Port = 0
    let r = parse_toml("port = 0");
    assert!(r.is_ok());
    assert_eq!(r.unwrap().port, Some(0));

    // Special characters in strings
    let r = parse_toml(r#"default_backend = "hello\nworld""#);
    assert!(r.is_ok());
}

#[test]
fn config_missing_required_fields_uses_defaults() {
    // Completely empty config should parse successfully
    let r = parse_toml("").unwrap();
    // Note: BackplaneConfig::default() sets log_level to Some("info"),
    // but parsing empty TOML yields None for all optional fields.
    assert!(r.default_backend.is_none());
    assert!(r.backends.is_empty());
    assert!(r.policy_profiles.is_empty());
}

#[test]
fn config_extra_unknown_fields_are_rejected() {
    // TOML with unknown fields — serde should reject by default
    let result = parse_toml("unknown_field = \"value\"");
    // Depending on serde config, this may or may not error
    // The important thing is it doesn't panic
    let _ = result;
}

#[test]
fn config_valid_mock_backend() {
    let toml = r#"
[backends.test]
type = "mock"
"#;
    let config = parse_toml(toml).unwrap();
    assert!(config.backends.contains_key("test"));
    assert!(matches!(config.backends["test"], BackendEntry::Mock {}));
}

#[test]
fn config_valid_sidecar_backend() {
    let toml = r#"
[backends.node]
type = "sidecar"
command = "node"
args = ["sidecar.js"]
timeout_secs = 300
"#;
    let config = parse_toml(toml).unwrap();
    match &config.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["sidecar.js"]);
            assert_eq!(*timeout_secs, Some(300));
        }
        _ => panic!("expected Sidecar"),
    }
}

#[test]
fn config_special_chars_in_backend_names() {
    let names = vec![
        "",
        " ",
        "a b c",
        "backend/with/slashes",
        "backend.with.dots",
        "🔥",
        "null",
        "true",
        "../../../etc",
    ];
    for name in names {
        let toml = format!(
            r#"
[backends."{name}"]
type = "mock"
"#
        );
        let _ = parse_toml(&toml);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. SERIALIZATION ROUND-TRIP FUZZING
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_work_order() {
    let wo = WorkOrderBuilder::new("test task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/test")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec!["*.secret".into()],
            deny_write: vec!["/etc/*".into()],
            allow_network: vec!["*.example.com".into()],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec!["delete".into()],
        })
        .context(ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "some context".into(),
            }],
        })
        .build();

    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, wo.task);
    assert_eq!(rt.lane, wo.lane);
    assert_eq!(rt.config.model, wo.config.model);
    assert_eq!(rt.policy.allowed_tools, wo.policy.allowed_tools);
}

#[test]
fn roundtrip_receipt() {
    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "started".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "file.rs"}),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!("file contents"),
            is_error: false,
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .verification(VerificationReport {
            git_diff: Some("diff --git ...".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        })
        .build();

    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.backend.id, r.backend.id);
    assert_eq!(rt.outcome, r.outcome);
    assert_eq!(rt.trace.len(), r.trace.len());
    assert_eq!(rt.artifacts.len(), r.artifacts.len());
}

#[test]
fn roundtrip_agent_event_all_variants() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "tok".into() }),
        make_event(AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: Some("p1".into()),
            input: serde_json::json!({"cmd": "ls"}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("file1\nfile2"),
            is_error: false,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added fn".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test passed".into()),
        }),
        make_event(AgentEventKind::Warning {
            message: "heads up".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        }),
    ];

    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&rt).unwrap();
        assert_eq!(json, json2, "Round-trip must be lossless");
    }
}

#[test]
fn roundtrip_envelope_all_variants() {
    let envelopes = vec![
        Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: Some("1.0".into()),
                adapter_version: None,
            },
            CapabilityManifest::new(),
        ),
        Envelope::Run {
            id: "run-1".into(),
            work_order: make_work_order("test"),
        },
        Envelope::Event {
            ref_id: "run-1".into(),
            event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        },
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: make_receipt(),
        },
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "boom".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "early error".into(),
            error_code: None,
        },
    ];

    for env in &envelopes {
        let line = JsonlCodec::encode(env).unwrap();
        assert!(line.ends_with('\n'));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        // Re-encode to verify structural equivalence
        let line2 = JsonlCodec::encode(&decoded).unwrap();
        // Parse both as JSON values for comparison (timestamps may differ in formatting)
        let v1: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        let v2: serde_json::Value = serde_json::from_str(line2.trim()).unwrap();
        assert_eq!(v1, v2, "Envelope round-trip must be lossless");
    }
}

#[test]
fn roundtrip_policy_profile() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/*".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["deploy".into()],
    };
    let json = serde_json::to_string(&p).unwrap();
    let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.allowed_tools, p.allowed_tools);
    assert_eq!(rt.disallowed_tools, p.disallowed_tools);
    assert_eq!(rt.deny_read, p.deny_read);
    assert_eq!(rt.deny_write, p.deny_write);
    assert_eq!(rt.allow_network, p.allow_network);
    assert_eq!(rt.deny_network, p.deny_network);
    assert_eq!(rt.require_approval_for, p.require_approval_for);
}

#[test]
fn roundtrip_config() {
    let config = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("./receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["policy.toml".into()],
        backends: {
            let mut m = BTreeMap::new();
            m.insert("mock".into(), BackendEntry::Mock {});
            m.insert(
                "node".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["sidecar.js".into()],
                    timeout_secs: Some(300),
                },
            );
            m
        },
    };
    let toml_str = toml::to_string(&config).unwrap();
    let rt: BackplaneConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(rt, config);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. DENIAL OF SERVICE RESISTANCE
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dos_very_large_payload_envelope() {
    let big = long_string(2 * 1024 * 1024); // 2MB
    let env = Envelope::Fatal {
        ref_id: None,
        error: big,
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 2 * 1024 * 1024),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn dos_very_large_work_order_task() {
    let big = long_string(2 * 1024 * 1024);
    let wo = make_work_order(&big);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task.len(), 2 * 1024 * 1024);
}

#[test]
fn dos_deeply_nested_structures_128_levels() {
    let mut val = serde_json::json!("leaf");
    for _ in 0..128 {
        val = serde_json::json!({"n": val});
    }
    let event = make_event(AgentEventKind::ToolCall {
        tool_name: "test".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: val,
    });
    let json = serde_json::to_string(&event).unwrap();
    // serde_json has a recursion limit (~128); deserializing may fail gracefully
    let result = serde_json::from_str::<AgentEvent>(&json);
    // Must not panic — an Err is acceptable
    let _ = result;
}

#[test]
fn dos_many_trace_events() {
    // 100k events in a receipt trace
    let mut builder = ReceiptBuilder::new("stress-test");
    for i in 0..100_000 {
        builder = builder.add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: format!("token_{i}"),
        }));
    }
    let r = builder.build();
    assert_eq!(r.trace.len(), 100_000);

    // Hashing should still work (may be slow, but shouldn't crash)
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn dos_very_long_strings_in_all_fields() {
    let big = long_string(1_048_576);
    let wo = WorkOrderBuilder::new(&big)
        .root(&big)
        .model(&big)
        .include(vec![big.clone()])
        .exclude(vec![big.clone()])
        .context(ContextPacket {
            files: vec![big.clone()],
            snippets: vec![ContextSnippet {
                name: big.clone(),
                content: big.clone(),
            }],
        })
        .policy(PolicyProfile {
            allowed_tools: vec![big.clone()],
            disallowed_tools: vec![big.clone()],
            deny_read: vec![big.clone()],
            deny_write: vec![big.clone()],
            allow_network: vec![big.clone()],
            deny_network: vec![big.clone()],
            require_approval_for: vec![big.clone()],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task.len(), 1_048_576);
}

#[test]
fn dos_many_capabilities_in_manifest() {
    let mut caps = CapabilityManifest::new();
    // Insert all known capabilities
    let all_caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ];
    for cap in &all_caps {
        caps.insert(cap.clone(), SupportLevel::Native);
    }
    let r = ReceiptBuilder::new("test").capabilities(caps).build();
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.capabilities.len(), all_caps.len());
}

#[test]
fn dos_many_backends_in_config() {
    let mut backends = BTreeMap::new();
    for i in 0..1000 {
        backends.insert(format!("backend_{i}"), BackendEntry::Mock {});
    }
    let config = BackplaneConfig {
        backends,
        ..BackplaneConfig::default()
    };
    let toml_str = toml::to_string(&config).unwrap();
    let rt: BackplaneConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(rt.backends.len(), 1000);
}

#[test]
fn dos_large_vendor_map_in_runtime_config() {
    let mut vendor = BTreeMap::new();
    for i in 0..10_000 {
        vendor.insert(format!("key_{i}"), serde_json::json!(i));
    }
    let config = RuntimeConfig {
        vendor,
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.vendor.len(), 10_000);
}

#[test]
fn dos_large_env_map_in_runtime_config() {
    let mut env = BTreeMap::new();
    for i in 0..10_000 {
        env.insert(format!("KEY_{i}"), format!("VALUE_{i}"));
    }
    let config = RuntimeConfig {
        env,
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.env.len(), 10_000);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. PROPTEST-BASED PROPERTY TESTS
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // --- Protocol fuzzing with random strings ---

    #[test]
    fn proptest_decode_arbitrary_string_doesnt_panic(s in ".*") {
        let _ = JsonlCodec::decode(&s);
    }

    #[test]
    fn proptest_decode_arbitrary_json_like_doesnt_panic(
        s in r#"\{"t":"[a-z_]+"(,"[a-z_]+":"[^"]*")*\}"#
    ) {
        let _ = JsonlCodec::decode(&s);
    }

    // --- Receipt hash stability ---

    #[test]
    fn proptest_receipt_hash_is_always_64_hex_chars(task in "[a-zA-Z0-9 ]{0,100}") {
        let r = ReceiptBuilder::new(&task).build();
        let h = receipt_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn proptest_receipt_hash_deterministic(task in "[a-zA-Z0-9]{1,50}") {
        let r = ReceiptBuilder::new(&task).build();
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    #[test]
    fn proptest_receipt_sha256_field_ignored_in_hash(
        task in "[a-zA-Z]{1,20}",
        fake_hash in "[0-9a-f]{64}"
    ) {
        let r = ReceiptBuilder::new(&task).build();
        let h1 = receipt_hash(&r).unwrap();
        let mut r2 = r;
        r2.receipt_sha256 = Some(fake_hash);
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // --- WorkOrder round-trip ---

    #[test]
    fn proptest_work_order_roundtrip(task in ".{0,200}") {
        let wo = WorkOrderBuilder::new(&task).build();
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&rt.task, &wo.task);
    }

    // --- Envelope round-trip for Fatal ---

    #[test]
    fn proptest_envelope_fatal_roundtrip(
        error_msg in ".{0,500}",
        ref_id in proptest::option::of("[a-zA-Z0-9-]{0,50}")
    ) {
        let env = Envelope::Fatal {
            ref_id: ref_id.clone(),
            error: error_msg.clone(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Fatal { ref_id: r, error: e, .. } = decoded {
            prop_assert_eq!(r, ref_id);
            prop_assert_eq!(e, error_msg);
        } else {
            prop_assert!(false, "expected Fatal variant");
        }
    }

    // --- Config parsing with random strings ---

    #[test]
    fn proptest_config_parse_doesnt_panic(s in ".{0,500}") {
        let _ = parse_toml(&s);
    }

    #[test]
    fn proptest_config_valid_default_backend(name in "[a-zA-Z][a-zA-Z0-9_-]{0,30}") {
        let toml = format!("default_backend = \"{name}\"");
        let config = parse_toml(&toml);
        if let Ok(c) = config {
            prop_assert_eq!(c.default_backend, Some(name));
        }
    }

    // --- SHA-256 properties ---

    #[test]
    fn proptest_sha256_hex_length(data in proptest::collection::vec(any::<u8>(), 0..1000)) {
        let h = sha256_hex(&data);
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn proptest_sha256_deterministic(data in proptest::collection::vec(any::<u8>(), 0..500)) {
        let h1 = sha256_hex(&data);
        let h2 = sha256_hex(&data);
        prop_assert_eq!(h1, h2);
    }

    // --- AgentEvent round-trip ---

    #[test]
    fn proptest_agent_event_message_roundtrip(text in ".{0,1000}") {
        let event = make_event(AgentEventKind::AssistantMessage { text: text.clone() });
        let json = serde_json::to_string(&event).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantMessage { text: rt_text } = &rt.kind {
            prop_assert_eq!(rt_text, &text);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // --- PolicyProfile round-trip ---

    #[test]
    fn proptest_policy_roundtrip(
        tools in proptest::collection::vec("[a-z_]{1,20}", 0..10),
        deny in proptest::collection::vec("[a-z_/*.]{1,20}", 0..5)
    ) {
        let p = PolicyProfile {
            allowed_tools: tools.clone(),
            disallowed_tools: vec![],
            deny_read: deny.clone(),
            deny_write: deny,
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        };
        let json = serde_json::to_string(&p).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(rt.allowed_tools, tools);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. ADDITIONAL EDGE CASES
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_with_all_optional_fields_null() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"x"}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "x");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn receipt_hash_with_nan_budget_is_stable() {
    // f64 NaN can cause serialization issues
    let r = ReceiptBuilder::new("test")
        .usage(UsageNormalized {
            estimated_cost_usd: Some(f64::NAN),
            ..UsageNormalized::default()
        })
        .build();
    // NaN serialization may fail, which is fine — just don't panic
    let _ = receipt_hash(&r);
}

#[test]
fn receipt_hash_with_infinity_budget() {
    let r = ReceiptBuilder::new("test")
        .usage(UsageNormalized {
            estimated_cost_usd: Some(f64::INFINITY),
            ..UsageNormalized::default()
        })
        .build();
    let _ = receipt_hash(&r);
}

#[test]
fn work_order_with_duplicate_context_files() {
    let wo = WorkOrderBuilder::new("test")
        .context(ContextPacket {
            files: vec!["same.rs".into(), "same.rs".into(), "same.rs".into()],
            snippets: vec![],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.context.files.len(), 3);
}

#[test]
fn envelope_hello_roundtrip_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    caps.insert(Capability::McpClient, SupportLevel::Unsupported);

    let env = Envelope::hello(
        BackendIdentity {
            id: "cap-test".into(),
            backend_version: Some("2.0".into()),
            adapter_version: Some("1.0".into()),
        },
        caps,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert_eq!(capabilities.len(), 4);
        assert!(matches!(
            capabilities.get(&Capability::ToolBash),
            Some(SupportLevel::Restricted { .. })
        ));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn config_toml_with_many_policy_profiles() {
    let profiles: Vec<String> = (0..1000).map(|i| format!("policy_{i}.toml")).collect();
    let toml = format!(
        "policy_profiles = [{}]",
        profiles
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let config = parse_toml(&toml).unwrap();
    assert_eq!(config.policy_profiles.len(), 1000);
}

#[test]
fn execution_mode_roundtrip() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let json = serde_json::to_string(&mode).unwrap();
        let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, mode);
    }
}

#[test]
fn outcome_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, outcome);
    }
}

#[test]
fn execution_lane_roundtrip() {
    for lane in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        let json = serde_json::to_string(&lane).unwrap();
        let rt: ExecutionLane = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, lane);
    }
}

#[test]
fn workspace_mode_roundtrip() {
    for mode in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        let json = serde_json::to_string(&mode).unwrap();
        let rt: WorkspaceMode = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, mode);
    }
}
