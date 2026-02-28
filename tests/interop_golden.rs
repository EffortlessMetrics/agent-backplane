// SPDX-License-Identifier: MIT OR Apache-2.0
//! Golden-file interop tests.
//!
//! Validates that JSON produced by the Rust `abp-protocol` crate matches
//! the exact wire format expected by the JavaScript / Python sidecar hosts
//! (hosts/node/host.js, hosts/python/host.py, hosts/claude/host.js).
//!
//! Each test compares against hardcoded "golden" JSON strings that mirror
//! what the sidecar hosts emit and parse.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ── Deterministic fixtures ──────────────────────────────────────────────

const FIXED_UUID: &str = "00000000-0000-0000-0000-000000000001";

fn fixed_uuid() -> Uuid {
    FIXED_UUID.parse().unwrap()
}

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "example_node_sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1".into()),
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: fixed_uuid(),
        task: "Fix the login bug".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/workspace".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn test_receipt() -> Receipt {
    let ts = fixed_ts();
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({"note": "example_node_sidecar"}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Parse a JSON string to a generic Value so we can compare structurally.
fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

/// Serialize an Envelope to a Value (without the trailing newline).
fn envelope_to_value(env: &Envelope) -> serde_json::Value {
    let s = JsonlCodec::encode(env).unwrap();
    json(s.trim())
}

// ═══════════════════════════════════════════════════════════════════════
//  1. Hello envelope matches expected JSON format
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_envelope_matches_golden_json() {
    let hello = Envelope::hello(test_backend(), test_capabilities());
    let v = envelope_to_value(&hello);

    // Mirrors what hosts/node/host.js writes at startup:
    //   { t: "hello", contract_version: "abp/v0.1", backend, capabilities }
    assert_eq!(v["t"], "hello");
    assert_eq!(v["contract_version"], CONTRACT_VERSION);
    assert_eq!(v["backend"]["id"], "example_node_sidecar");
    assert_eq!(v["backend"]["backend_version"], "1.0.0");
    assert_eq!(v["backend"]["adapter_version"], "0.1");
    assert_eq!(v["capabilities"]["streaming"], "native");
    assert_eq!(v["capabilities"]["tool_read"], "emulated");
    // mode defaults to "mapped" (hosts/python/host.py sends mode: "mapped")
    assert_eq!(v["mode"], "mapped");
}

// ═══════════════════════════════════════════════════════════════════════
//  2. Run envelope with WorkOrder matches expected format
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_envelope_matches_golden_json() {
    let wo = test_work_order();
    let run = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let v = envelope_to_value(&run);

    // JS hosts parse: msg.t === "run", msg.id, msg.work_order
    assert_eq!(v["t"], "run");
    assert_eq!(v["id"], "run-001");
    assert_eq!(v["work_order"]["task"], "Fix the login bug");
    assert_eq!(v["work_order"]["workspace"]["root"], "/tmp/workspace");
    assert_eq!(v["work_order"]["lane"], "patch_first");
    assert_eq!(v["work_order"]["workspace"]["mode"], "staged");
    assert_eq!(
        v["work_order"]["id"],
        FIXED_UUID
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  3. Event envelope matches expected format
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_envelope_matches_golden_json() {
    let event = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello from the sidecar.".into(),
            },
            ext: None,
        },
    };
    let v = envelope_to_value(&event);

    // JS hosts emit: { t: "event", ref_id: runId, event: { ts, type: "assistant_message", text } }
    assert_eq!(v["t"], "event");
    assert_eq!(v["ref_id"], "run-001");
    assert_eq!(v["event"]["type"], "assistant_message");
    assert_eq!(v["event"]["text"], "Hello from the sidecar.");
    assert!(v["event"]["ts"].as_str().unwrap().contains("2025"));
}

// ═══════════════════════════════════════════════════════════════════════
//  4. Final envelope with Receipt matches expected format
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn final_envelope_matches_golden_json() {
    let final_env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: test_receipt(),
    };
    let v = envelope_to_value(&final_env);

    // JS hosts emit: { t: "final", ref_id: runId, receipt: { meta, backend, ... } }
    assert_eq!(v["t"], "final");
    assert_eq!(v["ref_id"], "run-001");
    assert_eq!(v["receipt"]["meta"]["contract_version"], CONTRACT_VERSION);
    assert_eq!(v["receipt"]["outcome"], "complete");
    assert_eq!(v["receipt"]["backend"]["id"], "example_node_sidecar");
    assert_eq!(v["receipt"]["verification"]["harness_ok"], true);
    assert!(v["receipt"]["receipt_sha256"].is_null());
    assert_eq!(v["receipt"]["mode"], "mapped");
}

// ═══════════════════════════════════════════════════════════════════════
//  5. Fatal envelope matches expected format
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fatal_envelope_matches_golden_json() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "out of memory".into(),
    };
    let v = envelope_to_value(&fatal);

    // JS hosts emit: { t: "fatal", ref_id: null|id, error: "..." }
    assert_eq!(v["t"], "fatal");
    assert_eq!(v["ref_id"], "run-001");
    assert_eq!(v["error"], "out of memory");

    // null ref_id variant (used by Python host on startup errors)
    let fatal_null = Envelope::Fatal {
        ref_id: None,
        error: "invalid json".into(),
    };
    let v2 = envelope_to_value(&fatal_null);
    assert!(v2["ref_id"].is_null());
    assert_eq!(v2["error"], "invalid json");
}

// ═══════════════════════════════════════════════════════════════════════
//  6. Contract version in hello matches CONTRACT_VERSION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_contract_version_matches_constant() {
    let hello = Envelope::hello(test_backend(), test_capabilities());
    let v = envelope_to_value(&hello);

    // Both JS and Python sidecars hardcode CONTRACT_VERSION = "abp/v0.1"
    assert_eq!(v["contract_version"].as_str().unwrap(), "abp/v0.1");
    assert_eq!(v["contract_version"].as_str().unwrap(), CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════════
//  7. Envelope discriminator field is "t" not "type"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_discriminator_is_t_not_type() {
    let envelopes: Vec<Envelope> = vec![
        Envelope::hello(test_backend(), test_capabilities()),
        Envelope::Run {
            id: "r1".into(),
            work_order: test_work_order(),
        },
        Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: test_receipt(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
        },
    ];

    for env in &envelopes {
        let s = JsonlCodec::encode(env).unwrap();
        let v: serde_json::Value = serde_json::from_str(s.trim()).unwrap();
        // Protocol uses "t" as the tag field (serde(tag = "t"))
        assert!(
            v.get("t").is_some(),
            "envelope missing \"t\" field: {}",
            s.trim()
        );
        // "type" at the top level must NOT be present (it's used inside AgentEventKind)
        assert!(
            v.get("type").is_none(),
            "envelope must not have top-level \"type\" field: {}",
            s.trim()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  8. AgentEvent kinds match expected string values
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn agent_event_kinds_match_js_host_strings() {
    // JS hosts emit events with { type: "run_started" }, { type: "assistant_message" }, etc.
    // Verify every Rust variant serializes to the same snake_case string.
    let cases: Vec<(AgentEventKind, &str)> = vec![
        (
            AgentEventKind::RunStarted {
                message: "".into(),
            },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted {
                message: "".into(),
            },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta {
                text: "".into(),
            },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage {
                text: "".into(),
            },
            "assistant_message",
        ),
        (
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::Warning {
                message: "".into(),
            },
            "warning",
        ),
        (
            AgentEventKind::Error {
                message: "".into(),
            },
            "error",
        ),
    ];

    for (kind, expected_type) in cases {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind,
            ext: None,
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(
            v["type"].as_str().unwrap(),
            expected_type,
            "AgentEventKind mismatch for {}",
            expected_type,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  9. Capability names match expected string values
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_names_match_js_host_strings() {
    // JS hosts use these exact keys in their capabilities object:
    //   streaming, tool_read, tool_write, tool_edit,
    //   structured_output_json_schema, hooks_pre_tool_use, hooks_post_tool_use,
    //   session_resume
    let cases: Vec<(Capability, &str)> = vec![
        (Capability::Streaming, "streaming"),
        (Capability::ToolRead, "tool_read"),
        (Capability::ToolWrite, "tool_write"),
        (Capability::ToolEdit, "tool_edit"),
        (Capability::ToolBash, "tool_bash"),
        (Capability::ToolGlob, "tool_glob"),
        (Capability::ToolGrep, "tool_grep"),
        (Capability::ToolWebSearch, "tool_web_search"),
        (Capability::ToolWebFetch, "tool_web_fetch"),
        (Capability::ToolAskUser, "tool_ask_user"),
        (Capability::HooksPreToolUse, "hooks_pre_tool_use"),
        (Capability::HooksPostToolUse, "hooks_post_tool_use"),
        (Capability::SessionResume, "session_resume"),
        (Capability::SessionFork, "session_fork"),
        (Capability::Checkpointing, "checkpointing"),
        (Capability::StructuredOutputJsonSchema, "structured_output_json_schema"),
        (Capability::McpClient, "mcp_client"),
        (Capability::McpServer, "mcp_server"),
    ];

    for (cap, expected_name) in cases {
        let serialized = serde_json::to_value(&cap).unwrap();
        assert_eq!(
            serialized.as_str().unwrap(),
            expected_name,
            "Capability mismatch for {:?}",
            cap,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  10. Full protocol session produces valid JSONL
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn full_session_hello_run_events_final_produces_valid_jsonl() {
    // Simulate the full sidecar protocol sequence as the JS/Python hosts do:
    //   1. hello
    //   2. (control plane sends run)
    //   3. event (run_started)
    //   4. event (assistant_message)
    //   5. event (run_completed)
    //   6. final

    let run_id = "run-session-001";
    let wo = test_work_order();

    let envelopes = vec![
        Envelope::hello(test_backend(), test_capabilities()),
        Envelope::Run {
            id: run_id.into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "node sidecar starting".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello from the Node sidecar.".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunCompleted {
                    message: "node sidecar complete".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(),
        },
    ];

    // Encode to a JSONL blob.
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let blob = String::from_utf8(buf).unwrap();

    // Each line must be valid JSON with a "t" field.
    let lines: Vec<&str> = blob.lines().collect();
    assert_eq!(lines.len(), 6);

    let expected_types = ["hello", "run", "event", "event", "event", "final"];
    for (line, &expected_t) in lines.iter().zip(&expected_types) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["t"].as_str().unwrap(), expected_t);
    }

    // Round-trip: decode the blob back via the stream decoder.
    let reader = BufReader::new(blob.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 6);

    // First must be hello, last must be final.
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[5], Envelope::Final { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
//  11. JS hosts can decode Rust-encoded hello (exact golden string)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rust_hello_decodes_to_exact_golden_structure() {
    // Golden JSON that a JS host would produce for the same data.
    let golden = r#"{
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {
            "id": "example_node_sidecar",
            "backend_version": "1.0.0",
            "adapter_version": "0.1"
        },
        "capabilities": {
            "streaming": "native",
            "tool_read": "emulated"
        },
        "mode": "mapped"
    }"#;

    let hello = Envelope::hello(test_backend(), test_capabilities());
    let rust_val = envelope_to_value(&hello);
    let golden_val = json(golden);

    assert_eq!(rust_val, golden_val);
}

// ═══════════════════════════════════════════════════════════════════════
//  12. Rust can decode golden JSON produced by JS hosts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rust_decodes_js_golden_hello() {
    // Exact string a JS host writes (hosts/node/host.js line 33-38).
    // Note: JS hosts may omit "mode" — the Rust side defaults to mapped.
    let js_hello = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"example_node_sidecar","backend_version":"v20.0.0","adapter_version":"0.1"},"capabilities":{"streaming":"native","tool_read":"emulated","tool_write":"emulated","tool_edit":"emulated","structured_output_json_schema":"emulated"}}"#;

    let env = JsonlCodec::decode(js_hello).unwrap();
    match env {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend.id, "example_node_sidecar");
            assert_eq!(mode, ExecutionMode::Mapped); // default
            assert!(capabilities.contains_key(&Capability::Streaming));
            assert!(capabilities.contains_key(&Capability::ToolRead));
            assert!(capabilities.contains_key(&Capability::ToolWrite));
            assert!(capabilities.contains_key(&Capability::ToolEdit));
            assert!(capabilities.contains_key(&Capability::StructuredOutputJsonSchema));
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  13. Rust can decode golden fatal from JS host
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rust_decodes_js_golden_fatal() {
    // JS hosts: write({ t: "fatal", ref_id: null, error: `invalid json: ${e}` });
    let js_fatal = r#"{"t":"fatal","ref_id":null,"error":"invalid json: SyntaxError"}"#;

    let env = JsonlCodec::decode(js_fatal).unwrap();
    match env {
        Envelope::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "invalid json: SyntaxError");
        }
        other => panic!("expected Fatal, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  14. Rust can decode golden event from Python host
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rust_decodes_python_golden_event() {
    // Python host emits: { t: "event", ref_id: ..., event: { ts: ..., type: "tool_call", ... } }
    let py_event = r#"{"t":"event","ref_id":"run-abc","event":{"ts":"2025-01-15T12:00:00Z","type":"tool_call","tool_name":"read","tool_use_id":"tu-1","parent_tool_use_id":null,"input":{"path":"/tmp/foo.txt"}}}"#;

    let env = JsonlCodec::decode(py_event).unwrap();
    match env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-abc");
            match event.kind {
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    input,
                    ..
                } => {
                    assert_eq!(tool_name, "read");
                    assert_eq!(tool_use_id.unwrap(), "tu-1");
                    assert_eq!(input["path"], "/tmp/foo.txt");
                }
                other => panic!("expected ToolCall, got {:?}", other),
            }
        }
        other => panic!("expected Event, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  15. SupportLevel serializes to JS-compatible strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn support_level_serializes_to_js_compatible_strings() {
    // JS hosts use string values: "native", "emulated", "unsupported"
    assert_eq!(
        serde_json::to_value(SupportLevel::Native).unwrap(),
        json(r#""native""#)
    );
    assert_eq!(
        serde_json::to_value(SupportLevel::Emulated).unwrap(),
        json(r#""emulated""#)
    );
    assert_eq!(
        serde_json::to_value(SupportLevel::Unsupported).unwrap(),
        json(r#""unsupported""#)
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  16. Outcome serializes to JS-compatible strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn outcome_serializes_to_js_compatible_strings() {
    // JS hosts set receipt.outcome to "complete", "partial", "failed"
    assert_eq!(
        serde_json::to_value(Outcome::Complete).unwrap(),
        json(r#""complete""#)
    );
    assert_eq!(
        serde_json::to_value(Outcome::Partial).unwrap(),
        json(r#""partial""#)
    );
    assert_eq!(
        serde_json::to_value(Outcome::Failed).unwrap(),
        json(r#""failed""#)
    );
}
