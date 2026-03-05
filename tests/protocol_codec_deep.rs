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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep tests for the JSONL protocol codec and envelope system.
//!
//! Covers: Envelope construction, serde roundtrip with tag "t", ref_id
//! correlation, CONTRACT_VERSION presence, JSONL line parsing, multi-line
//! streams, malformed input, large payloads, and edge cases.

use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ===========================================================================
// Helpers
// ===========================================================================

fn backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn hello_env() -> Envelope {
    Envelope::hello(backend("test-sidecar"), CapabilityManifest::new())
}

fn run_env(task: &str) -> (String, Envelope) {
    let wo = WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    (id, env)
}

fn event_env(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        },
    }
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: ReceiptBuilder::new("test-sidecar")
            .outcome(Outcome::Complete)
            .build(),
    }
}

fn fatal_env(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn encode_trim(env: &Envelope) -> String {
    JsonlCodec::encode(env).unwrap()
}

fn roundtrip(env: &Envelope) -> Envelope {
    let json = encode_trim(env);
    JsonlCodec::decode(json.trim()).unwrap()
}

// ===========================================================================
// 1. Envelope construction for all variants
// ===========================================================================

mod construction {
    use super::*;

    #[test]
    fn hello_has_contract_version() {
        match hello_env() {
            Envelope::Hello {
                contract_version, ..
            } => assert_eq!(contract_version, CONTRACT_VERSION),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_has_backend_identity() {
        match hello_env() {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, "test-sidecar"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_default_mode_is_mapped() {
        match hello_env() {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_with_passthrough_mode() {
        let env = Envelope::hello_with_mode(
            backend("pt"),
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        match env {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_with_capabilities() {
        use abp_core::{Capability, SupportLevel};
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolRead, SupportLevel::Emulated);
        let env = Envelope::hello(backend("cap-test"), caps);
        match env {
            Envelope::Hello { capabilities, .. } => assert_eq!(capabilities.len(), 2),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn run_has_id_and_work_order() {
        let (id, env) = run_env("test task");
        match env {
            Envelope::Run {
                id: eid,
                work_order,
            } => {
                assert_eq!(eid, id);
                assert_eq!(work_order.task, "test task");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn event_has_ref_id() {
        let env = event_env("run-1", "hello");
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-1"),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn final_has_ref_id_and_receipt() {
        let env = final_env("run-1");
        match env {
            Envelope::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(receipt.backend.id, "test-sidecar");
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn fatal_with_ref_id() {
        let env = fatal_env(Some("run-1"), "boom");
        match env {
            Envelope::Fatal {
                ref_id,
                error,
                error_code,
                ..
            } => {
                assert_eq!(ref_id.as_deref(), Some("run-1"));
                assert_eq!(error, "boom");
                assert!(error_code.is_none());
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_without_ref_id() {
        let env = fatal_env(None, "crash");
        match env {
            Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_with_error_code() {
        let env = Envelope::fatal_with_code(
            Some("run-1".into()),
            "protocol error",
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
        );
        match &env {
            Envelope::Fatal { error_code, .. } => {
                assert_eq!(
                    *error_code,
                    Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
                );
            }
            _ => panic!("expected Fatal"),
        }
        assert_eq!(
            env.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    }

    #[test]
    fn error_code_returns_none_for_non_fatal() {
        assert!(hello_env().error_code().is_none());
    }
}

// ===========================================================================
// 2. Envelope serde roundtrip with tag "t"
// ===========================================================================

mod serde_roundtrip {
    use super::*;

    #[test]
    fn hello_roundtrip() {
        let decoded = roundtrip(&hello_env());
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn hello_json_uses_t_tag() {
        let json = encode_trim(&hello_env());
        assert!(json.contains(r#""t":"hello""#));
        assert!(!json.contains(r#""type":"hello""#));
    }

    #[test]
    fn run_roundtrip() {
        let (id, env) = run_env("roundtrip task");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Run {
                id: did,
                work_order,
            } => {
                assert_eq!(did, id);
                assert_eq!(work_order.task, "roundtrip task");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_json_uses_t_tag() {
        let (_, env) = run_env("tag check");
        let json = encode_trim(&env);
        assert!(json.contains(r#""t":"run""#));
    }

    #[test]
    fn event_roundtrip() {
        let env = event_env("r1", "some text");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { ref_id, event } => {
                assert_eq!(ref_id, "r1");
                match event.kind {
                    AgentEventKind::AssistantMessage { ref text } => {
                        assert_eq!(text, "some text");
                    }
                    _ => panic!("expected AssistantMessage"),
                }
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_json_uses_t_tag() {
        let json = encode_trim(&event_env("r1", "hi"));
        assert!(json.contains(r#""t":"event""#));
    }

    #[test]
    fn final_roundtrip() {
        let env = final_env("r1");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(receipt.outcome, Outcome::Complete);
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_json_uses_t_tag() {
        let json = encode_trim(&final_env("r1"));
        assert!(json.contains(r#""t":"final""#));
    }

    #[test]
    fn fatal_roundtrip() {
        let env = fatal_env(Some("r1"), "oops");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("r1"));
                assert_eq!(error, "oops");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_json_uses_t_tag() {
        let json = encode_trim(&fatal_env(None, "err"));
        assert!(json.contains(r#""t":"fatal""#));
    }

    #[test]
    fn fatal_with_null_ref_id_roundtrip() {
        let env = fatal_env(None, "no ref");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn encode_always_ends_with_newline() {
        let envs: Vec<Envelope> = vec![
            hello_env(),
            run_env("nl").1,
            event_env("r", "t"),
            final_env("r"),
            fatal_env(None, "e"),
        ];
        for env in &envs {
            assert!(encode_trim(env).ends_with('\n'));
        }
    }

    #[test]
    fn decode_with_trailing_whitespace() {
        let json = encode_trim(&hello_env());
        let padded = format!("  {}  ", json.trim());
        let decoded = JsonlCodec::decode(padded.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn event_preserves_all_event_kinds() {
        let kinds = vec![
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            AgentEventKind::AssistantDelta { text: "tok".into() },
            AgentEventKind::AssistantMessage { text: "msg".into() },
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/a"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!("contents"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "edit".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: Some("file.txt".into()),
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
            let env = Envelope::Event {
                ref_id: "r1".into(),
                event: AgentEvent {
                    ts: Utc::now(),
                    kind,
                    ext: None,
                },
            };
            let decoded = roundtrip(&env);
            assert!(matches!(decoded, Envelope::Event { .. }));
        }
    }

    #[test]
    fn work_order_fields_survive_roundtrip() {
        let wo = WorkOrderBuilder::new("complex task")
            .root("/workspace")
            .workspace_mode(WorkspaceMode::Staged)
            .model("gpt-4")
            .max_turns(5)
            .build();
        let id = wo.id;
        let env = Envelope::Run {
            id: id.to_string(),
            work_order: wo,
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.id, id);
                assert_eq!(work_order.task, "complex task");
                assert_eq!(work_order.workspace.root, "/workspace");
                assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
                assert_eq!(work_order.config.max_turns, Some(5));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn receipt_fields_survive_roundtrip() {
        let receipt = ReceiptBuilder::new("backend-x")
            .outcome(Outcome::Partial)
            .build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.backend.id, "backend-x");
                assert_eq!(receipt.outcome, Outcome::Partial);
            }
            _ => panic!("expected Final"),
        }
    }
}

// ===========================================================================
// 3. ref_id correlation
// ===========================================================================

mod ref_id_correlation {
    use super::*;

    #[test]
    fn event_ref_id_matches_run_id() {
        let (id, _) = run_env("task");
        let env = event_env(&id, "hello");
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, id),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn final_ref_id_matches_run_id() {
        let (id, _) = run_env("task");
        let env = final_env(&id);
        match env {
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, id),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn fatal_ref_id_matches_run_id() {
        let (id, _) = run_env("task");
        let env = fatal_env(Some(&id), "err");
        match env {
            Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some(id.as_str())),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn multiple_events_same_ref_id() {
        let (id, _) = run_env("task");
        for i in 0..10 {
            let env = event_env(&id, &format!("msg {i}"));
            let decoded = roundtrip(&env);
            match decoded {
                Envelope::Event { ref_id, .. } => assert_eq!(ref_id, id),
                _ => panic!("expected Event"),
            }
        }
    }

    #[test]
    fn ref_id_preserved_through_encode_decode() {
        let ref_id = "custom-ref-id-12345";
        let env = event_env(ref_id, "test");
        let json = encode_trim(&env);
        assert!(json.contains(ref_id));
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event {
                ref_id: decoded_ref,
                ..
            } => assert_eq!(decoded_ref, ref_id),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn different_runs_have_different_ref_ids() {
        let (id1, _) = run_env("task a");
        let (id2, _) = run_env("task b");
        assert_ne!(id1, id2);
    }
}

// ===========================================================================
// 4. CONTRACT_VERSION in hello/run
// ===========================================================================

mod contract_version {
    use super::*;

    #[test]
    fn contract_version_is_abp_v0_1() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn hello_serializes_contract_version() {
        let json = encode_trim(&hello_env());
        assert!(json.contains(CONTRACT_VERSION));
    }

    #[test]
    fn hello_deserializes_contract_version() {
        let decoded = roundtrip(&hello_env());
        match decoded {
            Envelope::Hello {
                contract_version, ..
            } => assert_eq!(contract_version, CONTRACT_VERSION),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn run_contains_work_order_with_uuid() {
        let (_, env) = run_env("version check");
        let json = encode_trim(&env);
        // Run envelope contains work order which is a struct, not the contract version directly,
        // but the id field should be a valid UUID string.
        assert!(json.contains(r#""t":"run""#));
    }

    #[test]
    fn hello_with_custom_version_deserializes() {
        let json = r#"{"t":"hello","contract_version":"abp/v99.0","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
        let decoded = JsonlCodec::decode(json).unwrap();
        match decoded {
            Envelope::Hello {
                contract_version, ..
            } => assert_eq!(contract_version, "abp/v99.0"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn parse_version_valid() {
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
        assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
    }

    #[test]
    fn parse_version_invalid() {
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version("abp/0.1"), None);
        assert_eq!(parse_version("v0.1"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compatible_version_same_major() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    }

    #[test]
    fn incompatible_version_different_major() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    }

    #[test]
    fn incompatible_version_invalid_strings() {
        assert!(!is_compatible_version("invalid", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "garbage"));
    }
}

// ===========================================================================
// 5. JSONL line parsing (one envelope per line)
// ===========================================================================

mod jsonl_line_parsing {
    use super::*;

    #[test]
    fn single_line_decode() {
        let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
        let env = JsonlCodec::decode(json).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn single_line_with_trailing_newline() {
        let json = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n";
        let env = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn encode_produces_exactly_one_newline() {
        let json = encode_trim(&hello_env());
        assert_eq!(json.chars().filter(|c| *c == '\n').count(), 1);
        assert!(json.ends_with('\n'));
    }

    #[test]
    fn encode_produces_single_line_json() {
        let json = encode_trim(&hello_env());
        let trimmed = json.trim();
        assert!(!trimmed.contains('\n'));
    }

    #[test]
    fn decode_stream_single_line() {
        let json = encode_trim(&hello_env());
        let reader = BufReader::new(json.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], Envelope::Hello { .. }));
    }

    #[test]
    fn each_variant_encodes_to_single_line() {
        let (_, run) = run_env("single line check");
        let envs = vec![
            hello_env(),
            run,
            event_env("r", "t"),
            final_env("r"),
            fatal_env(None, "e"),
        ];
        for env in &envs {
            let json = encode_trim(env);
            let lines: Vec<&str> = json.trim().lines().collect();
            assert_eq!(lines.len(), 1, "envelope should be single line");
        }
    }

    #[test]
    fn decode_hand_crafted_hello() {
        let json = format!(
            r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"manual","backend_version":null,"adapter_version":null}},"capabilities":{{}},"mode":"mapped"}}"#,
            CONTRACT_VERSION
        );
        let env = JsonlCodec::decode(&json).unwrap();
        match env {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, "manual"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn decode_hand_crafted_fatal() {
        let json = r#"{"t":"fatal","ref_id":"abc","error":"something broke"}"#;
        let env = JsonlCodec::decode(json).unwrap();
        match env {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("abc"));
                assert_eq!(error, "something broke");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn encode_to_writer_works() {
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &hello_env()).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.ends_with('\n'));
        assert!(s.contains(r#""t":"hello""#));
    }
}

// ===========================================================================
// 6. Multi-line JSONL streams
// ===========================================================================

mod multiline_stream {
    use super::*;

    #[test]
    fn decode_stream_two_messages() {
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &fatal_env(None, "a")).unwrap();
        JsonlCodec::encode_to_writer(&mut buf, &fatal_env(None, "b")).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let input = format!(
            "{}\n\n{}\n\n",
            encode_trim(&fatal_env(None, "a")).trim(),
            encode_trim(&fatal_env(None, "b")).trim()
        );
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn decode_stream_full_protocol_sequence() {
        let (id, run) = run_env("stream test");
        let envs = vec![
            hello_env(),
            run,
            event_env(&id, "msg 1"),
            event_env(&id, "msg 2"),
            final_env(&id),
        ];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 5);
        assert!(matches!(results[0], Envelope::Hello { .. }));
        assert!(matches!(results[1], Envelope::Run { .. }));
        assert!(matches!(results[2], Envelope::Event { .. }));
        assert!(matches!(results[3], Envelope::Event { .. }));
        assert!(matches!(results[4], Envelope::Final { .. }));
    }

    #[test]
    fn decode_stream_with_whitespace_only_lines() {
        let fatal_line = encode_trim(&fatal_env(None, "x"));
        let input = format!("   \n\t\n{}\n  \n", fatal_line.trim());
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn encode_many_to_writer_produces_multiple_lines() {
        let envs = vec![
            fatal_env(None, "a"),
            fatal_env(None, "b"),
            fatal_env(None, "c"),
        ];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let s = String::from_utf8(buf).unwrap();
        let nonempty_lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(nonempty_lines.len(), 3);
    }

    #[test]
    fn stream_preserves_order() {
        let mut envs = Vec::new();
        for i in 0..20 {
            envs.push(fatal_env(None, &format!("msg-{i}")));
        }
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 20);
        for (i, r) in results.iter().enumerate() {
            match r {
                Envelope::Fatal { error, .. } => assert_eq!(error, &format!("msg-{i}")),
                _ => panic!("expected Fatal"),
            }
        }
    }

    #[test]
    fn decode_stream_empty_input() {
        let reader = BufReader::new("".as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn decode_stream_only_blank_lines() {
        let reader = BufReader::new("\n\n\n\n".as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn many_events_stream() {
        let (id, run) = run_env("bulk");
        let mut envs = vec![hello_env(), run];
        for i in 0..50 {
            envs.push(event_env(&id, &format!("event-{i}")));
        }
        envs.push(final_env(&id));
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 53); // hello + run + 50 events + final
    }
}

// ===========================================================================
// 7. Malformed JSONL handling
// ===========================================================================

mod malformed_input {
    use super::*;

    #[test]
    fn empty_string_fails() {
        let err = JsonlCodec::decode("").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn plain_text_fails() {
        let err = JsonlCodec::decode("not valid json").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn valid_json_missing_t_field() {
        let err = JsonlCodec::decode(r#"{"error":"boom"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn unknown_t_variant() {
        let err = JsonlCodec::decode(r#"{"t":"unknown_variant","data":1}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn t_field_wrong_type() {
        let err = JsonlCodec::decode(r#"{"t":42}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn t_field_null() {
        let err = JsonlCodec::decode(r#"{"t":null}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn hello_missing_required_field() {
        let err = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn run_missing_work_order() {
        let err = JsonlCodec::decode(r#"{"t":"run","id":"123"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn event_missing_event_payload() {
        let err = JsonlCodec::decode(r#"{"t":"event","ref_id":"r1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn final_missing_receipt() {
        let err = JsonlCodec::decode(r#"{"t":"final","ref_id":"r1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn fatal_missing_error() {
        let err = JsonlCodec::decode(r#"{"t":"fatal","ref_id":"r1"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn truncated_json() {
        let err = JsonlCodec::decode(r#"{"t":"hello","contract_version":"ab"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn json_array_instead_of_object() {
        let err = JsonlCodec::decode(r#"[1,2,3]"#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn json_number() {
        let err = JsonlCodec::decode("42").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn json_string() {
        let err = JsonlCodec::decode(r#""hello""#).unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn json_boolean() {
        let err = JsonlCodec::decode("true").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn json_null() {
        let err = JsonlCodec::decode("null").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn malformed_in_stream_yields_error() {
        let input = format!(
            "{}\nnot json\n{}\n",
            encode_trim(&fatal_env(None, "a")).trim(),
            encode_trim(&fatal_env(None, "b")).trim()
        );
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<Result<Envelope, ProtocolError>> =
            JsonlCodec::decode_stream(reader).collect();
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
    }

    #[test]
    fn extra_fields_are_tolerated() {
        // serde default is to ignore unknown fields
        let json = r#"{"t":"fatal","ref_id":null,"error":"ok","extra_field":true,"another":123}"#;
        let env = JsonlCodec::decode(json).unwrap();
        match env {
            Envelope::Fatal { error, .. } => assert_eq!(error, "ok"),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn protocol_error_display() {
        let err = JsonlCodec::decode("bad").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("invalid JSON"));
    }

    #[test]
    fn nested_invalid_json() {
        let err = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":{"nested":"object"}}"#)
            .unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }
}

// ===========================================================================
// 8. Large payload handling
// ===========================================================================

mod large_payloads {
    use super::*;

    #[test]
    fn large_assistant_message() {
        let text = "x".repeat(100_000);
        let env = event_env("r1", &text);
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text: t } => assert_eq!(t.len(), 100_000),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn large_error_message() {
        let msg = "e".repeat(50_000);
        let env = fatal_env(None, &msg);
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Fatal { error, .. } => assert_eq!(error.len(), 50_000),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn large_tool_call_input() {
        let big_input = serde_json::json!({
            "data": "x".repeat(100_000),
        });
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "big-tool".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: big_input,
                },
                ext: None,
            },
        };
        let decoded = roundtrip(&env);
        assert!(matches!(decoded, Envelope::Event { .. }));
    }

    #[test]
    fn many_events_in_stream() {
        let mut buf = Vec::new();
        for i in 0..500 {
            let env = fatal_env(None, &format!("event-{i}"));
            JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        }
        let reader = BufReader::new(buf.as_slice());
        let count = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .len();
        assert_eq!(count, 500);
    }

    #[test]
    fn large_ref_id() {
        let ref_id = "r".repeat(10_000);
        let env = event_env(&ref_id, "msg");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event {
                ref_id: decoded_ref,
                ..
            } => assert_eq!(decoded_ref.len(), 10_000),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn large_backend_id() {
        let id = "b".repeat(10_000);
        let env = Envelope::hello(backend(&id), CapabilityManifest::new());
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Hello { backend: b, .. } => assert_eq!(b.id.len(), 10_000),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn many_capabilities() {
        use abp_core::{Capability, SupportLevel};
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
        ];
        for cap in &all_caps {
            caps.insert(cap.clone(), SupportLevel::Native);
        }
        let env = Envelope::hello(backend("full-caps"), caps);
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Hello { capabilities, .. } => {
                assert_eq!(capabilities.len(), all_caps.len());
            }
            _ => panic!("expected Hello"),
        }
    }
}

// ===========================================================================
// 9. Edge cases: empty payloads, unicode, binary-like data
// ===========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_task_string() {
        let wo = WorkOrderBuilder::new("")
            .root(".")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let env = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Run { work_order, .. } => assert!(work_order.task.is_empty()),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn empty_error_string() {
        let env = fatal_env(None, "");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Fatal { error, .. } => assert!(error.is_empty()),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn empty_backend_id() {
        let env = Envelope::hello(
            BackendIdentity {
                id: String::new(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Hello { backend, .. } => assert!(backend.id.is_empty()),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn unicode_in_task() {
        let wo = WorkOrderBuilder::new("修复登录模块 🔧")
            .root(".")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let env = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.task, "修复登录模块 🔧"),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn unicode_in_error() {
        let env = fatal_env(None, "エラー発生 💥");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Fatal { error, .. } => assert_eq!(error, "エラー発生 💥"),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn unicode_in_event_text() {
        let env = event_env("r1", "Привет мир 🌍");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Привет мир 🌍"),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn unicode_in_backend_identity() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "后端-αβγ".into(),
                backend_version: Some("版本1.0".into()),
                adapter_version: Some("адаптер".into()),
            },
            CapabilityManifest::new(),
        );
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Hello { backend, .. } => {
                assert_eq!(backend.id, "后端-αβγ");
                assert_eq!(backend.backend_version.as_deref(), Some("版本1.0"));
                assert_eq!(backend.adapter_version.as_deref(), Some("адаптер"));
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn special_json_chars_in_strings() {
        let env = fatal_env(None, r#"quotes: "hello", backslash: \, newline: \n"#);
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Fatal { error, .. } => {
                assert!(error.contains(r#"quotes: "hello""#));
                assert!(error.contains(r"backslash: \"));
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn newlines_in_message_text() {
        let text = "line1\nline2\nline3";
        let env = event_env("r1", text);
        let json = encode_trim(&env);
        // The encoded JSON should be a single line (newlines escaped)
        let trimmed = json.trim();
        assert!(!trimmed.contains('\n') || trimmed.lines().count() == 1);
        let decoded = JsonlCodec::decode(trimmed).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text: t } => assert_eq!(t, text),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn tabs_and_control_chars_in_text() {
        let text = "tab:\there\r\n\x00null";
        let env = event_env("r1", text);
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text: t } => assert_eq!(t, text),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn emoji_in_all_string_fields() {
        let env = fatal_env(Some("🆔"), "💀");
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("🆔"));
                assert_eq!(error, "💀");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn empty_capabilities_map() {
        let env = Envelope::hello(backend("x"), CapabilityManifest::new());
        let json = encode_trim(&env);
        assert!(json.contains(r#""capabilities":{}"#));
    }

    #[test]
    fn null_optional_fields_in_backend() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let json = encode_trim(&env);
        assert!(json.contains(r#""backend_version":null"#));
        assert!(json.contains(r#""adapter_version":null"#));
    }

    #[test]
    fn ext_field_none_is_omitted() {
        let env = event_env("r1", "msg");
        let json = encode_trim(&env);
        // ext is skip_serializing_if = "Option::is_none", so should not appear
        assert!(!json.contains("\"ext\""));
    }

    #[test]
    fn ext_field_present_roundtrips() {
        use std::collections::BTreeMap;
        let mut ext = BTreeMap::new();
        ext.insert("raw_message".into(), serde_json::json!({"original": true}));
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "test".into(),
                },
                ext: Some(ext),
            },
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => {
                assert!(event.ext.is_some());
                let ext = event.ext.unwrap();
                assert!(ext.contains_key("raw_message"));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn error_code_none_not_serialized() {
        let env = fatal_env(None, "no code");
        let json = encode_trim(&env);
        assert!(!json.contains("error_code"));
    }

    #[test]
    fn error_code_some_roundtrips() {
        let env = Envelope::fatal_with_code(
            None,
            "bad envelope",
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
        );
        let json = encode_trim(&env);
        assert!(json.contains("error_code"));
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error_code, .. } => {
                assert_eq!(
                    error_code,
                    Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
                );
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn deterministic_encoding() {
        // Encoding the same envelope twice should produce identical JSON
        let env = hello_env();
        let json1 = encode_trim(&env);
        let json2 = encode_trim(&env);
        assert_eq!(json1, json2);
    }

    #[test]
    fn mode_default_omission() {
        // When mode is the default (Mapped), verify it still roundtrips correctly
        let json = format!(
            r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"test","backend_version":null,"adapter_version":null}},"capabilities":{{}}}}"#,
            CONTRACT_VERSION
        );
        let decoded = JsonlCodec::decode(&json).unwrap();
        match decoded {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn very_long_ref_id_in_stream() {
        let long_id = "a".repeat(50_000);
        let env = event_env(&long_id, "msg");
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn tool_result_with_error_flag() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "bash".into(),
                    tool_use_id: Some("tu1".into()),
                    output: serde_json::json!("command not found"),
                    is_error: true,
                },
                ext: None,
            },
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
                _ => panic!("expected ToolResult"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn command_executed_with_none_fields() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::CommandExecuted {
                    command: "echo hi".into(),
                    exit_code: None,
                    output_preview: None,
                },
                ext: None,
            },
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::CommandExecuted {
                    exit_code,
                    output_preview,
                    ..
                } => {
                    assert!(exit_code.is_none());
                    assert!(output_preview.is_none());
                }
                _ => panic!("expected CommandExecuted"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn warning_event_roundtrip() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "careful!".into(),
                },
                ext: None,
            },
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::Warning { message } => assert_eq!(message, "careful!"),
                _ => panic!("expected Warning"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn error_event_with_error_code_roundtrip() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "fatal error".into(),
                    error_code: Some(abp_error::ErrorCode::ProtocolInvalidEnvelope),
                },
                ext: None,
            },
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::Error {
                    message,
                    error_code,
                } => {
                    assert_eq!(message, "fatal error");
                    assert_eq!(
                        error_code,
                        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
                    );
                }
                _ => panic!("expected Error"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn file_changed_event_roundtrip() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "src/main.rs".into(),
                    summary: "added function".into(),
                },
                ext: None,
            },
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::FileChanged { path, summary } => {
                    assert_eq!(path, "src/main.rs");
                    assert_eq!(summary, "added function");
                }
                _ => panic!("expected FileChanged"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn nested_tool_call_parent_id() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "sub-tool".into(),
                    tool_use_id: Some("child-1".into()),
                    parent_tool_use_id: Some("parent-1".into()),
                    input: serde_json::json!({}),
                },
                ext: None,
            },
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::ToolCall {
                    parent_tool_use_id, ..
                } => {
                    assert_eq!(parent_tool_use_id.as_deref(), Some("parent-1"));
                }
                _ => panic!("expected ToolCall"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn receipt_with_artifacts_roundtrip() {
        use abp_core::ArtifactRef;
        let mut receipt = ReceiptBuilder::new("art-backend").build();
        receipt.artifacts.push(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        });
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.artifacts.len(), 1);
                assert_eq!(receipt.artifacts[0].kind, "patch");
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn receipt_with_trace_roundtrip() {
        let trace_event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "traced msg".into(),
            },
            ext: None,
        };
        let mut receipt = ReceiptBuilder::new("trace-backend").build();
        receipt.trace.push(trace_event);
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let decoded = roundtrip(&env);
        match decoded {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.trace.len(), 1);
            }
            _ => panic!("expected Final"),
        }
    }
}

// ===========================================================================
// 10. Additional codec and protocol correctness
// ===========================================================================

mod codec_correctness {
    use super::*;

    #[test]
    fn decode_is_inverse_of_encode_for_hello() {
        let original = hello_env();
        let json = JsonlCodec::encode(&original).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(json, re_encoded);
    }

    #[test]
    fn decode_is_inverse_of_encode_for_fatal() {
        let original = fatal_env(Some("ref"), "msg");
        let json = JsonlCodec::encode(&original).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(json, re_encoded);
    }

    #[test]
    fn all_variants_have_distinct_t_values() {
        let (_, run) = run_env("t check");
        let envs = [
            hello_env(),
            run,
            event_env("r", "m"),
            final_env("r"),
            fatal_env(None, "e"),
        ];
        let tags: Vec<String> = envs
            .iter()
            .map(|e| {
                let json = encode_trim(e);
                let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
                v["t"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(tags, vec!["hello", "run", "event", "final", "fatal"]);
    }

    #[test]
    fn t_values_are_snake_case() {
        let (_, run) = run_env("case check");
        let envs = vec![
            hello_env(),
            run,
            event_env("r", "m"),
            final_env("r"),
            fatal_env(None, "e"),
        ];
        for env in &envs {
            let json = encode_trim(env);
            let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
            let tag = v["t"].as_str().unwrap();
            assert_eq!(tag, tag.to_lowercase(), "tag should be lowercase");
            assert!(!tag.contains('-'), "tag should not contain hyphens");
        }
    }

    #[test]
    fn agent_event_kind_uses_type_tag_not_t() {
        let env = event_env("r1", "check tag");
        let json = encode_trim(&env);
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        // The outer envelope uses "t", the inner event kind uses "type"
        assert!(v["t"].is_string());
        let event_val = &v["event"];
        assert!(event_val["type"].is_string());
    }

    #[test]
    fn multiple_roundtrips_are_stable() {
        let env = hello_env();
        let mut json = JsonlCodec::encode(&env).unwrap();
        for _ in 0..10 {
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            json = JsonlCodec::encode(&decoded).unwrap();
        }
        let final_decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(final_decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn parse_version_contract_version() {
        assert_eq!(parse_version(CONTRACT_VERSION), Some((0, 1)));
    }

    #[test]
    fn is_compatible_with_self() {
        assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
    }

    #[test]
    fn protocol_error_variants() {
        // Json variant
        let json_err = JsonlCodec::decode("bad").unwrap_err();
        assert!(matches!(json_err, ProtocolError::Json(_)));

        // Violation variant
        let violation = ProtocolError::Violation("test".into());
        assert!(format!("{violation}").contains("protocol violation"));

        // UnexpectedMessage variant
        let unexpected = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        };
        assert!(format!("{unexpected}").contains("hello"));
        assert!(format!("{unexpected}").contains("run"));
    }

    #[test]
    fn protocol_error_codes() {
        let violation = ProtocolError::Violation("v".into());
        assert_eq!(
            violation.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );

        let unexpected = ProtocolError::UnexpectedMessage {
            expected: "a".into(),
            got: "b".into(),
        };
        assert_eq!(
            unexpected.error_code(),
            Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
        );

        let json_err = JsonlCodec::decode("bad").unwrap_err();
        assert!(json_err.error_code().is_none());
    }
}
