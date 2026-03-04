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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive JSONL protocol codec tests covering abp-protocol and
//! abp-sidecar-proto: envelope serialization, tag discriminator, JSONL line
//! parsing, invalid input, ref_id correlation, round-trip fidelity, edge cases,
//! large payloads, unicode, stream parser, builder API, validation, batch,
//! compression, routing, and version negotiation.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, ReceiptBuilder, SupportLevel, UsageNormalized, WorkOrderBuilder,
    WorkspaceMode,
};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use serde_json::json;

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

fn encode(env: &Envelope) -> String {
    JsonlCodec::encode(env).unwrap()
}

fn roundtrip(env: &Envelope) -> Envelope {
    let json = encode(env);
    JsonlCodec::decode(json.trim()).unwrap()
}

// ===========================================================================
// 1. Envelope tag discriminator ("t", not "type")
// ===========================================================================

mod tag_discriminator {
    use super::*;

    #[test]
    fn hello_uses_t_tag() {
        let json = encode(&hello_env());
        assert!(json.contains(r#""t":"hello""#));
        assert!(!json.contains(r#""type":"hello""#));
    }

    #[test]
    fn run_uses_t_tag() {
        let (_, env) = run_env("task");
        let json = encode(&env);
        assert!(json.contains(r#""t":"run""#));
    }

    #[test]
    fn event_uses_t_tag() {
        let json = encode(&event_env("r1", "hi"));
        assert!(json.contains(r#""t":"event""#));
    }

    #[test]
    fn final_uses_t_tag() {
        let json = encode(&final_env("r1"));
        assert!(json.contains(r#""t":"final""#));
    }

    #[test]
    fn fatal_uses_t_tag() {
        let json = encode(&fatal_env(None, "err"));
        assert!(json.contains(r#""t":"fatal""#));
    }

    #[test]
    fn agent_event_kind_uses_type_tag() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"assistant_message""#));
    }

    #[test]
    fn envelope_with_type_key_does_not_parse() {
        let bad = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":{}}"#;
        assert!(JsonlCodec::decode(bad).is_err());
    }

    #[test]
    fn t_tag_value_is_snake_case() {
        let variants: Vec<Envelope> = vec![
            hello_env(),
            run_env("x").1,
            event_env("r", "t"),
            final_env("r"),
            fatal_env(None, "e"),
        ];
        let names = ["hello", "run", "event", "final", "fatal"];
        for (env, name) in variants.iter().zip(names.iter()) {
            let json = encode(env);
            assert!(
                json.contains(&format!(r#""t":"{}""#, name)),
                "expected t={name}"
            );
        }
    }
}

// ===========================================================================
// 2. Hello envelope construction and parsing
// ===========================================================================

mod hello_envelope {
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
            Envelope::Hello { backend, .. } => {
                assert_eq!(backend.id, "test-sidecar");
                assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
            }
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
    fn hello_passthrough_mode() {
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
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolRead, SupportLevel::Emulated);
        let env = Envelope::hello(backend("cap"), caps);
        match env {
            Envelope::Hello { capabilities, .. } => assert_eq!(capabilities.len(), 2),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_roundtrip() {
        let env = hello_env();
        let rt = roundtrip(&env);
        match rt {
            Envelope::Hello {
                contract_version,
                backend,
                ..
            } => {
                assert_eq!(contract_version, CONTRACT_VERSION);
                assert_eq!(backend.id, "test-sidecar");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_json_contains_version_string() {
        let json = encode(&hello_env());
        assert!(json.contains(CONTRACT_VERSION));
    }

    #[test]
    fn hello_empty_capabilities_serializes() {
        let env = Envelope::hello(backend("x"), CapabilityManifest::new());
        let json = encode(&env);
        assert!(json.contains(r#""capabilities":{}"#));
    }

    #[test]
    fn hello_mode_absent_defaults_to_mapped() {
        let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let env = JsonlCodec::decode(json).unwrap();
        match env {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
            _ => panic!("expected Hello"),
        }
    }
}

// ===========================================================================
// 3. Run envelope with WorkOrder payload
// ===========================================================================

mod run_envelope {
    use super::*;

    #[test]
    fn run_has_id_and_task() {
        let (id, env) = run_env("do something");
        match env {
            Envelope::Run {
                id: eid,
                work_order,
            } => {
                assert_eq!(eid, id);
                assert_eq!(work_order.task, "do something");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_roundtrip_preserves_task() {
        let (_, env) = run_env("roundtrip task");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.task, "roundtrip task");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_id_matches_work_order_uuid() {
        let wo = WorkOrderBuilder::new("test").root(".").build();
        let id = wo.id.to_string();
        let env = Envelope::Run {
            id: id.clone(),
            work_order: wo,
        };
        match env {
            Envelope::Run {
                id: eid,
                work_order,
            } => {
                assert_eq!(eid, work_order.id.to_string());
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_with_model_config() {
        let wo = WorkOrderBuilder::new("model test")
            .root(".")
            .model("gpt-4")
            .build();
        let env = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_with_workspace_spec() {
        let wo = WorkOrderBuilder::new("ws test")
            .root("/project")
            .workspace_mode(WorkspaceMode::Staged)
            .build();
        let env = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.workspace.root, "/project");
                assert!(matches!(work_order.workspace.mode, WorkspaceMode::Staged));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_with_max_turns() {
        let wo = WorkOrderBuilder::new("turns test")
            .root(".")
            .max_turns(5)
            .build();
        let env = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.config.max_turns, Some(5));
            }
            _ => panic!("expected Run"),
        }
    }
}

// ===========================================================================
// 4. Event envelope with AgentEvent payload
// ===========================================================================

mod event_envelope {
    use super::*;

    #[test]
    fn event_has_ref_id() {
        match event_env("run-1", "hello") {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-1"),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_roundtrip_preserves_text() {
        let env = event_env("r1", "my message");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, "my message"),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_assistant_delta() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "token".into(),
                },
                ext: None,
            },
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => {
                assert!(matches!(event.kind, AgentEventKind::AssistantDelta { .. }));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_tool_call() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "bash".into(),
                    tool_use_id: Some("tu-1".into()),
                    parent_tool_use_id: None,
                    input: json!({"command": "ls"}),
                },
                ext: None,
            },
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::ToolCall {
                    tool_name, input, ..
                } => {
                    assert_eq!(tool_name, "bash");
                    assert_eq!(input["command"], "ls");
                }
                _ => panic!("expected ToolCall"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_tool_result() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "bash".into(),
                    tool_use_id: Some("tu-1".into()),
                    output: json!("file.txt"),
                    is_error: false,
                },
                ext: None,
            },
        };
        let json = encode(&env);
        assert!(json.contains("tool_result"));
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::ToolResult { is_error, .. } => assert!(!is_error),
                _ => panic!("expected ToolResult"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_file_changed() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "src/main.rs".into(),
                    summary: "added main".into(),
                },
                ext: None,
            },
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::FileChanged { path, summary } => {
                    assert_eq!(path, "src/main.rs");
                    assert_eq!(summary, "added main");
                }
                _ => panic!("expected FileChanged"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_command_executed() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::CommandExecuted {
                    command: "cargo build".into(),
                    exit_code: Some(0),
                    output_preview: Some("Compiling...".into()),
                },
                ext: None,
            },
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::CommandExecuted { exit_code, .. } => {
                    assert_eq!(exit_code, Some(0));
                }
                _ => panic!("expected CommandExecuted"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_warning() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "low budget".into(),
                },
                ext: None,
            },
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => {
                assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_error_kind() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "something broke".into(),
                    error_code: None,
                },
                ext: None,
            },
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::Error { message, .. } => {
                    assert_eq!(message, "something broke");
                }
                _ => panic!("expected Error"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_run_started() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "begin".into(),
                },
                ext: None,
            },
        };
        let json = encode(&env);
        assert!(json.contains("run_started"));
    }

    #[test]
    fn event_run_completed() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        };
        let json = encode(&env);
        assert!(json.contains("run_completed"));
    }

    #[test]
    fn event_with_extension_data() {
        let mut ext = BTreeMap::new();
        ext.insert("custom_key".into(), json!("custom_value"));
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "ext".into() },
                ext: Some(ext),
            },
        };
        let json = encode(&env);
        assert!(json.contains("custom_key"));
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => {
                assert!(event.ext.is_some());
                assert_eq!(event.ext.unwrap()["custom_key"], "custom_value");
            }
            _ => panic!("expected Event"),
        }
    }
}

// ===========================================================================
// 5. Final envelope with Receipt payload
// ===========================================================================

mod final_envelope {
    use super::*;

    #[test]
    fn final_has_ref_id_and_receipt() {
        match final_env("run-1") {
            Envelope::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(receipt.backend.id, "test-sidecar");
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_roundtrip_preserves_outcome() {
        let rt = roundtrip(&final_env("r1"));
        match rt {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.outcome, Outcome::Complete);
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_with_partial_outcome() {
        let receipt = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Partial)
            .build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_with_failed_outcome() {
        let receipt = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Failed)
            .build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_receipt_backend_identity_preserved() {
        let receipt = ReceiptBuilder::new("my-backend")
            .backend_version("2.0.0")
            .adapter_version("1.0.0")
            .build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.backend.id, "my-backend");
                assert_eq!(receipt.backend.backend_version.as_deref(), Some("2.0.0"));
                assert_eq!(receipt.backend.adapter_version.as_deref(), Some("1.0.0"));
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_receipt_with_usage() {
        let receipt = ReceiptBuilder::new("sidecar")
            .usage(UsageNormalized {
                input_tokens: Some(100),
                output_tokens: Some(200),
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: Some(0.01),
            })
            .build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.usage.input_tokens, Some(100));
                assert_eq!(receipt.usage.output_tokens, Some(200));
            }
            _ => panic!("expected Final"),
        }
    }
}

// ===========================================================================
// 6. Fatal envelope with error message
// ===========================================================================

mod fatal_envelope {
    use super::*;

    #[test]
    fn fatal_with_ref_id() {
        match fatal_env(Some("run-1"), "boom") {
            Envelope::Fatal {
                ref_id,
                error,
                error_code,
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
        match fatal_env(None, "early crash") {
            Envelope::Fatal { ref_id, error, .. } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "early crash");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_roundtrip() {
        let env = fatal_env(Some("r1"), "test error");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("r1"));
                assert_eq!(error, "test error");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_with_error_code() {
        let env = Envelope::fatal_with_code(
            Some("r1".into()),
            "timeout",
            abp_error::ErrorCode::BackendTimeout,
        );
        let json = encode(&env);
        assert!(json.contains("error_code"));
        let rt = roundtrip(&env);
        assert_eq!(rt.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
    }

    #[test]
    fn fatal_from_abp_error() {
        let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::Internal, "internal fail");
        let env = Envelope::fatal_from_abp_error(Some("r1".into()), &abp_err);
        match env {
            Envelope::Fatal {
                error, error_code, ..
            } => {
                assert_eq!(error, "internal fail");
                assert_eq!(error_code, Some(abp_error::ErrorCode::Internal));
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_error_code_none_for_non_fatal() {
        assert!(hello_env().error_code().is_none());
    }

    #[test]
    fn fatal_null_ref_id_serializes() {
        let json = encode(&fatal_env(None, "err"));
        assert!(json.contains(r#""ref_id":null"#));
    }

    #[test]
    fn fatal_error_code_omitted_when_none() {
        let json = encode(&fatal_env(Some("r1"), "err"));
        assert!(!json.contains("error_code"));
    }
}

// ===========================================================================
// 7. JSONL line-delimited parsing
// ===========================================================================

mod jsonl_parsing {
    use super::*;

    #[test]
    fn encode_ends_with_newline() {
        let json = encode(&hello_env());
        assert!(json.ends_with('\n'));
    }

    #[test]
    fn encode_is_single_line() {
        let json = encode(&hello_env());
        assert_eq!(json.trim().lines().count(), 1);
    }

    #[test]
    fn decode_stream_multiple_lines() {
        let mut input = String::new();
        input.push_str(&encode(&hello_env()));
        input.push_str(&encode(&fatal_env(None, "err")));
        let reader = BufReader::new(input.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envs.len(), 2);
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let mut input = String::new();
        input.push_str(&encode(&hello_env()));
        input.push('\n');
        input.push_str("   \n");
        input.push_str(&encode(&fatal_env(None, "err")));
        let reader = BufReader::new(input.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envs.len(), 2);
    }

    #[test]
    fn encode_to_writer() {
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &hello_env()).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains(r#""t":"hello""#));
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn encode_many_to_writer() {
        let envs = vec![hello_env(), fatal_env(None, "err")];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s.lines().count(), 2);
    }

    #[test]
    fn decode_stream_full_protocol_sequence() {
        let (id, run) = run_env("task");
        let mut input = String::new();
        input.push_str(&encode(&hello_env()));
        input.push_str(&encode(&run));
        input.push_str(&encode(&event_env(&id, "working")));
        input.push_str(&encode(&final_env(&id)));
        let reader = BufReader::new(input.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envs.len(), 4);
        assert!(matches!(envs[0], Envelope::Hello { .. }));
        assert!(matches!(envs[1], Envelope::Run { .. }));
        assert!(matches!(envs[2], Envelope::Event { .. }));
        assert!(matches!(envs[3], Envelope::Final { .. }));
    }
}

// ===========================================================================
// 8. Invalid JSON handling
// ===========================================================================

mod invalid_json {
    use super::*;

    #[test]
    fn malformed_json() {
        let result = JsonlCodec::decode("{not valid json}");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
    }

    #[test]
    fn incomplete_json() {
        let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"#);
        assert!(result.is_err());
    }

    #[test]
    fn empty_string() {
        assert!(JsonlCodec::decode("").is_err());
    }

    #[test]
    fn null_json() {
        assert!(JsonlCodec::decode("null").is_err());
    }

    #[test]
    fn json_array() {
        assert!(JsonlCodec::decode("[1,2,3]").is_err());
    }

    #[test]
    fn json_number() {
        assert!(JsonlCodec::decode("42").is_err());
    }

    #[test]
    fn json_string() {
        assert!(JsonlCodec::decode(r#""hello""#).is_err());
    }

    #[test]
    fn json_boolean() {
        assert!(JsonlCodec::decode("true").is_err());
    }

    #[test]
    fn missing_t_field() {
        let result = JsonlCodec::decode(r#"{"error":"boom"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_t_value() {
        let result = JsonlCodec::decode(r#"{"t":"unknown_variant","data":123}"#);
        assert!(result.is_err());
    }

    #[test]
    fn extra_fields_are_tolerated() {
        // serde default behavior: unknown fields are ignored
        let json = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"ignored"}"#;
        let env = JsonlCodec::decode(json).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn missing_required_field_in_fatal() {
        // error field is required for Fatal
        let json = r#"{"t":"fatal","ref_id":null}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn missing_required_field_in_run() {
        let json = r#"{"t":"run","id":"123"}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn wrong_type_for_field() {
        let json = r#"{"t":"fatal","ref_id":42,"error":"boom"}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn protocol_error_display_json() {
        let err = JsonlCodec::decode("bad").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("invalid JSON"));
    }
}

// ===========================================================================
// 9. Ref_id correlation between envelopes
// ===========================================================================

mod ref_id_correlation {
    use super::*;

    #[test]
    fn event_ref_id_matches_run_id() {
        let (id, _) = run_env("task");
        let ev = event_env(&id, "work");
        match ev {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, id),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn final_ref_id_matches_run_id() {
        let (id, _) = run_env("task");
        let fin = final_env(&id);
        match fin {
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, id),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn fatal_ref_id_matches_run_id() {
        let (id, _) = run_env("task");
        let fat = fatal_env(Some(&id), "oops");
        match fat {
            Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some(id.as_str())),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn multiple_events_same_ref_id() {
        let id = "run-multi";
        let events: Vec<_> = (0..5).map(|i| event_env(id, &format!("msg-{i}"))).collect();
        for ev in &events {
            match ev {
                Envelope::Event { ref_id, .. } => assert_eq!(ref_id, id),
                _ => panic!("expected Event"),
            }
        }
    }

    #[test]
    fn ref_id_preserved_through_roundtrip() {
        let env = event_env("unique-ref-id-123", "test");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "unique-ref-id-123"),
            _ => panic!("expected Event"),
        }
    }
}

// ===========================================================================
// 10. Unknown envelope types → error
// ===========================================================================

mod unknown_envelope_types {
    use super::*;

    #[test]
    fn unknown_t_value_errors() {
        let json = r#"{"t":"subscribe","channel":"updates"}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn misspelled_hello() {
        let json =
            r#"{"t":"helo","contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":{}}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn camel_case_type_name() {
        let json =
            r#"{"t":"Hello","contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":{}}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn uppercase_type_name() {
        let json = r#"{"t":"FATAL","ref_id":null,"error":"boom"}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn empty_t_value() {
        let json = r#"{"t":"","error":"boom"}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }

    #[test]
    fn numeric_t_value() {
        let json = r#"{"t":1,"error":"boom"}"#;
        assert!(JsonlCodec::decode(json).is_err());
    }
}

// ===========================================================================
// 11. Large payload handling
// ===========================================================================

mod large_payloads {
    use super::*;

    #[test]
    fn large_assistant_message() {
        let text = "x".repeat(100_000);
        let env = event_env("r1", &text);
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text: t } => assert_eq!(t.len(), 100_000),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn large_tool_output() {
        let output = "y".repeat(500_000);
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read".into(),
                    tool_use_id: None,
                    output: json!(output),
                    is_error: false,
                },
                ext: None,
            },
        };
        let json = encode(&env);
        let rt = JsonlCodec::decode(json.trim()).unwrap();
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::ToolResult { output, .. } => {
                    assert_eq!(output.as_str().unwrap().len(), 500_000);
                }
                _ => panic!("expected ToolResult"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn large_error_message() {
        let msg = "e".repeat(50_000);
        let env = fatal_env(Some("r1"), &msg);
        let rt = roundtrip(&env);
        match rt {
            Envelope::Fatal { error, .. } => assert_eq!(error.len(), 50_000),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn many_events_in_stream() {
        let mut input = String::new();
        for i in 0..200 {
            input.push_str(&encode(&event_env("r1", &format!("msg-{i}"))));
        }
        let reader = BufReader::new(input.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envs.len(), 200);
    }
}

// ===========================================================================
// 12. Unicode in envelope payloads
// ===========================================================================

mod unicode_payloads {
    use super::*;

    #[test]
    fn unicode_in_task() {
        let wo = WorkOrderBuilder::new("修复 bug 🐛").root(".").build();
        let env = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.task, "修复 bug 🐛"),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn unicode_in_assistant_message() {
        let env = event_env("r1", "こんにちは世界 🌍");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text, "こんにちは世界 🌍");
                }
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn unicode_in_error() {
        let env = fatal_env(Some("r1"), "Ошибка: файл не найден 📁");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Fatal { error, .. } => {
                assert_eq!(error, "Ошибка: файл не найден 📁");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn unicode_in_backend_id() {
        let env = Envelope::hello(backend("バックエンド"), CapabilityManifest::new());
        let rt = roundtrip(&env);
        match rt {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, "バックエンド"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn emoji_in_file_path() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "src/🔥/hot.rs".into(),
                    summary: "🆕 new file".into(),
                },
                ext: None,
            },
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::FileChanged { path, .. } => {
                    assert_eq!(path, "src/🔥/hot.rs");
                }
                _ => panic!("expected FileChanged"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn mixed_scripts() {
        let text = "Hello مرحبا 你好 Привет 🎉";
        let env = event_env("r1", text);
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text: t } => assert_eq!(t, text),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }
}

// ===========================================================================
// 13. Empty payload edge cases
// ===========================================================================

mod empty_payloads {
    use super::*;

    #[test]
    fn empty_error_message_roundtrips() {
        let env = fatal_env(Some("r1"), "");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Fatal { error, .. } => assert_eq!(error, ""),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn empty_assistant_message() {
        let env = event_env("r1", "");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, ""),
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn empty_task_roundtrips() {
        let wo = WorkOrderBuilder::new("").root(".").build();
        let env = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.task, ""),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn empty_capabilities() {
        let env = Envelope::hello(backend("x"), CapabilityManifest::new());
        let rt = roundtrip(&env);
        match rt {
            Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn empty_ref_id_string() {
        let env = event_env("", "msg");
        let rt = roundtrip(&env);
        match rt {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, ""),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn empty_backend_id() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let rt = roundtrip(&env);
        match rt {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, ""),
            _ => panic!("expected Hello"),
        }
    }
}

// ===========================================================================
// 14. Envelope round-trip fidelity
// ===========================================================================

mod roundtrip_fidelity {
    use super::*;

    #[test]
    fn hello_roundtrip_fidelity() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolBash, SupportLevel::Emulated);
        let env = Envelope::hello_with_mode(
            BackendIdentity {
                id: "fidelity-test".into(),
                backend_version: Some("2.0".into()),
                adapter_version: Some("1.0".into()),
            },
            caps,
            ExecutionMode::Passthrough,
        );
        let rt = roundtrip(&env);
        match rt {
            Envelope::Hello {
                contract_version,
                backend,
                capabilities,
                mode,
            } => {
                assert_eq!(contract_version, CONTRACT_VERSION);
                assert_eq!(backend.id, "fidelity-test");
                assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
                assert_eq!(backend.adapter_version.as_deref(), Some("1.0"));
                assert_eq!(capabilities.len(), 2);
                assert_eq!(mode, ExecutionMode::Passthrough);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn run_roundtrip_fidelity() {
        let wo = WorkOrderBuilder::new("fidelity")
            .root("/src")
            .workspace_mode(WorkspaceMode::Staged)
            .model("claude-3")
            .max_turns(10)
            .max_budget_usd(5.0)
            .build();
        let id = wo.id;
        let env = Envelope::Run {
            id: id.to_string(),
            work_order: wo,
        };
        let rt = roundtrip(&env);
        match rt {
            Envelope::Run {
                id: eid,
                work_order,
            } => {
                assert_eq!(eid, id.to_string());
                assert_eq!(work_order.task, "fidelity");
                assert_eq!(work_order.workspace.root, "/src");
                assert!(matches!(work_order.workspace.mode, WorkspaceMode::Staged));
                assert_eq!(work_order.config.model.as_deref(), Some("claude-3"));
                assert_eq!(work_order.config.max_turns, Some(10));
                assert_eq!(work_order.config.max_budget_usd, Some(5.0));
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn fatal_roundtrip_with_error_code() {
        let env = Envelope::fatal_with_code(
            Some("r-fidelity".into()),
            "version mismatch",
            abp_error::ErrorCode::ProtocolVersionMismatch,
        );
        let rt = roundtrip(&env);
        match rt {
            Envelope::Fatal {
                ref_id,
                error,
                error_code,
            } => {
                assert_eq!(ref_id.as_deref(), Some("r-fidelity"));
                assert_eq!(error, "version mismatch");
                assert_eq!(
                    error_code,
                    Some(abp_error::ErrorCode::ProtocolVersionMismatch)
                );
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn double_roundtrip_stable() {
        let env = event_env("stable-ref", "stable message");
        let first = encode(&env);
        let rt1 = JsonlCodec::decode(first.trim()).unwrap();
        let second = encode(&rt1);
        let rt2 = JsonlCodec::decode(second.trim()).unwrap();
        // Check both produce same JSON (modulo timestamps already fixed)
        let json1 = encode(&rt1);
        let json2 = encode(&rt2);
        assert_eq!(json1, json2);
    }

    #[test]
    fn all_variant_roundtrips() {
        let envs: Vec<Envelope> = vec![
            hello_env(),
            run_env("test").1,
            event_env("r1", "msg"),
            final_env("r1"),
            fatal_env(Some("r1"), "err"),
            fatal_env(None, "err"),
        ];
        for env in &envs {
            let json = encode(env);
            let rt = JsonlCodec::decode(json.trim()).unwrap();
            // Verify same variant
            assert_eq!(std::mem::discriminant(env), std::mem::discriminant(&rt));
        }
    }
}

// ===========================================================================
// 15. StreamParser tests
// ===========================================================================

mod stream_parser {
    use super::*;

    #[test]
    fn empty_push() {
        let mut parser = StreamParser::new();
        assert!(parser.push(b"").is_empty());
        assert!(parser.is_empty());
    }

    #[test]
    fn partial_then_complete() {
        let mut parser = StreamParser::new();
        let line = encode(&fatal_env(None, "boom"));
        let bytes = line.as_bytes();
        let (first, second) = bytes.split_at(10);
        assert!(parser.push(first).is_empty());
        assert!(!parser.is_empty());
        assert_eq!(parser.buffered_len(), 10);
        let results = parser.push(second);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn multiple_lines_in_single_chunk() {
        let mut parser = StreamParser::new();
        let mut input = String::new();
        input.push_str(&encode(&hello_env()));
        input.push_str(&encode(&fatal_env(None, "err")));
        let results = parser.push(input.as_bytes());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn finish_flushes_partial() {
        let mut parser = StreamParser::new();
        let json = serde_json::to_string(&fatal_env(None, "x")).unwrap();
        // Push without trailing newline
        parser.push(json.as_bytes());
        assert!(parser.push(b"").is_empty());
        let results = parser.finish();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn reset_clears_buffer() {
        let mut parser = StreamParser::new();
        parser.push(b"partial");
        assert!(!parser.is_empty());
        parser.reset();
        assert!(parser.is_empty());
        assert_eq!(parser.buffered_len(), 0);
    }

    #[test]
    fn blank_lines_skipped() {
        let mut parser = StreamParser::new();
        let line = encode(&fatal_env(None, "ok"));
        let mut input = String::new();
        input.push('\n');
        input.push_str("  \n");
        input.push_str(&line);
        let results = parser.push(input.as_bytes());
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn max_line_len_enforced() {
        let mut parser = StreamParser::with_max_line_len(50);
        let long = "a".repeat(100) + "\n";
        let results = parser.push(long.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn default_trait() {
        let parser = StreamParser::default();
        assert!(parser.is_empty());
    }

    #[test]
    fn byte_by_byte_feeding() {
        let mut parser = StreamParser::new();
        let line = encode(&fatal_env(None, "byte"));
        let mut total = Vec::new();
        for &b in line.as_bytes() {
            let results = parser.push(&[b]);
            total.extend(results);
        }
        assert_eq!(total.len(), 1);
        assert!(total[0].is_ok());
    }
}

// ===========================================================================
// 16. Builder API tests
// ===========================================================================

mod builder_api {
    use super::*;
    use abp_protocol::builder::EnvelopeBuilder;

    #[test]
    fn hello_builder_minimal() {
        let env = EnvelopeBuilder::hello().backend("test").build().unwrap();
        match env {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, "test"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_builder_missing_backend() {
        let err = EnvelopeBuilder::hello().build().unwrap_err();
        assert_eq!(err.to_string(), "missing required field: backend");
    }

    #[test]
    fn hello_builder_full() {
        let env = EnvelopeBuilder::hello()
            .backend("my-sc")
            .version("3.0")
            .adapter_version("2.0")
            .mode(ExecutionMode::Passthrough)
            .capabilities(CapabilityManifest::new())
            .build()
            .unwrap();
        match env {
            Envelope::Hello { backend, mode, .. } => {
                assert_eq!(backend.id, "my-sc");
                assert_eq!(backend.backend_version.as_deref(), Some("3.0"));
                assert_eq!(mode, ExecutionMode::Passthrough);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn run_builder() {
        let wo = WorkOrderBuilder::new("builder task").root(".").build();
        let env = EnvelopeBuilder::run(wo)
            .ref_id("custom-id")
            .build()
            .unwrap();
        match env {
            Envelope::Run { id, .. } => assert_eq!(id, "custom-id"),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_builder_default_id() {
        let wo = WorkOrderBuilder::new("task").root(".").build();
        let expected_id = wo.id.to_string();
        let env = EnvelopeBuilder::run(wo).build().unwrap();
        match env {
            Envelope::Run { id, .. } => assert_eq!(id, expected_id),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn event_builder() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        };
        let env = EnvelopeBuilder::event(event)
            .ref_id("r-builder")
            .build()
            .unwrap();
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "r-builder"),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_builder_missing_ref_id() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "x".into() },
            ext: None,
        };
        assert!(EnvelopeBuilder::event(event).build().is_err());
    }

    #[test]
    fn final_builder() {
        let receipt = ReceiptBuilder::new("x").build();
        let env = EnvelopeBuilder::final_receipt(receipt)
            .ref_id("r-fin")
            .build()
            .unwrap();
        match env {
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "r-fin"),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_builder_missing_ref_id() {
        let receipt = ReceiptBuilder::new("x").build();
        assert!(EnvelopeBuilder::final_receipt(receipt).build().is_err());
    }

    #[test]
    fn fatal_builder() {
        let env = EnvelopeBuilder::fatal("oops")
            .ref_id("r-fat")
            .build()
            .unwrap();
        match env {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("r-fat"));
                assert_eq!(error, "oops");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_builder_no_ref_id() {
        let env = EnvelopeBuilder::fatal("early").build().unwrap();
        match env {
            Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
            _ => panic!("expected Fatal"),
        }
    }
}

// ===========================================================================
// 17. Validation tests
// ===========================================================================

mod validation {
    use super::*;

    #[test]
    fn valid_hello_passes() {
        let v = EnvelopeValidator::new();
        let result = v.validate(&hello_env());
        assert!(result.valid);
    }

    #[test]
    fn empty_backend_id_fails_validation() {
        let v = EnvelopeValidator::new();
        let env = Envelope::hello(
            BackendIdentity {
                id: "".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let result = v.validate(&env);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| matches!(
            e,
            ValidationError::EmptyField { field } if field == "backend.id"
        )));
    }

    #[test]
    fn invalid_version_fails_validation() {
        let env = Envelope::Hello {
            contract_version: "invalid".into(),
            backend: backend("x"),
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::Mapped,
        };
        let v = EnvelopeValidator::new();
        let result = v.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn empty_run_id_fails_validation() {
        let wo = WorkOrderBuilder::new("task").root(".").build();
        let env = Envelope::Run {
            id: "".into(),
            work_order: wo,
        };
        let v = EnvelopeValidator::new();
        let result = v.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn empty_error_in_fatal_fails_validation() {
        let env = fatal_env(Some("r1"), "");
        let v = EnvelopeValidator::new();
        let result = v.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn empty_ref_id_in_event_fails_validation() {
        let env = event_env("", "msg");
        let v = EnvelopeValidator::new();
        let result = v.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn missing_optional_fields_produce_warnings() {
        let v = EnvelopeValidator::new();
        let env = Envelope::hello(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let result = v.validate(&env);
        assert!(result.valid);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn valid_sequence() {
        let v = EnvelopeValidator::new();
        let (id, run) = run_env("task");
        let seq = vec![hello_env(), run, event_env(&id, "msg"), final_env(&id)];
        let errors = v.validate_sequence(&seq);
        assert!(errors.is_empty());
    }

    #[test]
    fn sequence_missing_hello() {
        let v = EnvelopeValidator::new();
        let (id, run) = run_env("task");
        let seq = vec![run, final_env(&id)];
        let errors = v.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingHello))
        );
    }

    #[test]
    fn sequence_missing_terminal() {
        let v = EnvelopeValidator::new();
        let (_, run) = run_env("task");
        let seq = vec![hello_env(), run];
        let errors = v.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingTerminal))
        );
    }

    #[test]
    fn sequence_ref_id_mismatch() {
        let v = EnvelopeValidator::new();
        let (id, run) = run_env("task");
        let seq = vec![
            hello_env(),
            run,
            event_env("wrong-id", "msg"),
            final_env(&id),
        ];
        let errors = v.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
        );
    }

    #[test]
    fn sequence_hello_not_first() {
        let v = EnvelopeValidator::new();
        let (id, run) = run_env("task");
        let seq = vec![run, hello_env(), final_env(&id)];
        let errors = v.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
        );
    }

    #[test]
    fn empty_sequence() {
        let v = EnvelopeValidator::new();
        let errors = v.validate_sequence(&[]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingHello))
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingTerminal))
        );
    }
}

// ===========================================================================
// 18. Version parsing and negotiation
// ===========================================================================

mod version_negotiation {
    use super::*;
    use abp_protocol::version::{ProtocolVersion, negotiate_version};

    #[test]
    fn parse_valid_version() {
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    }

    #[test]
    fn parse_invalid_version() {
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version("v0.1"), None);
        assert_eq!(parse_version("abp/0.1"), None);
    }

    #[test]
    fn compatible_versions() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    }

    #[test]
    fn incompatible_versions() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "invalid"));
    }

    #[test]
    fn protocol_version_current() {
        let current = ProtocolVersion::current();
        assert_eq!(current.to_string(), CONTRACT_VERSION);
    }

    #[test]
    fn protocol_version_parse() {
        let v = ProtocolVersion::parse("abp/v0.1").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
    }

    #[test]
    fn negotiate_compatible() {
        let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
        let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
        let result = negotiate_version(&v01, &v02).unwrap();
        assert_eq!(result.minor, 1); // min
    }

    #[test]
    fn negotiate_incompatible() {
        let v0 = ProtocolVersion::parse("abp/v0.1").unwrap();
        let v1 = ProtocolVersion::parse("abp/v1.0").unwrap();
        assert!(negotiate_version(&v0, &v1).is_err());
    }
}

// ===========================================================================
// 19. Streaming codec (batch encode/decode)
// ===========================================================================

mod streaming_codec {
    use super::*;
    use abp_protocol::codec::StreamingCodec;

    #[test]
    fn encode_batch() {
        let envs = vec![hello_env(), fatal_env(None, "err")];
        let batch = StreamingCodec::encode_batch(&envs);
        assert_eq!(batch.lines().count(), 2);
    }

    #[test]
    fn decode_batch() {
        let envs = vec![hello_env(), fatal_env(None, "err")];
        let batch = StreamingCodec::encode_batch(&envs);
        let results = StreamingCodec::decode_batch(&batch);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn line_count() {
        let envs = vec![hello_env(), fatal_env(None, "a"), fatal_env(None, "b")];
        let batch = StreamingCodec::encode_batch(&envs);
        assert_eq!(StreamingCodec::line_count(&batch), 3);
    }

    #[test]
    fn validate_jsonl_all_valid() {
        let envs = vec![hello_env()];
        let batch = StreamingCodec::encode_batch(&envs);
        let errors = StreamingCodec::validate_jsonl(&batch);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_jsonl_with_errors() {
        let mut batch = StreamingCodec::encode_batch(&[hello_env()]);
        batch.push_str("not json\n");
        let errors = StreamingCodec::validate_jsonl(&batch);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, 2); // 1-based line number
    }
}

// ===========================================================================
// 20. Protocol error types
// ===========================================================================

mod protocol_errors {
    use super::*;

    #[test]
    fn json_error_has_code() {
        let err = JsonlCodec::decode("bad").unwrap_err();
        // JSON parse errors don't carry an ErrorCode
        assert!(err.error_code().is_none());
    }

    #[test]
    fn violation_error_has_code() {
        let err = ProtocolError::Violation("test".into());
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    }

    #[test]
    fn unexpected_message_has_code() {
        let err = ProtocolError::UnexpectedMessage {
            expected: "run".into(),
            got: "hello".into(),
        };
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
        );
    }

    #[test]
    fn abp_error_conversion() {
        let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendNotFound, "not found");
        let proto_err: ProtocolError = abp_err.into();
        assert_eq!(
            proto_err.error_code(),
            Some(abp_error::ErrorCode::BackendNotFound)
        );
    }

    #[test]
    fn io_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let proto_err: ProtocolError = io_err.into();
        assert!(proto_err.to_string().contains("I/O error"));
    }
}

// ===========================================================================
// 21. Sidecar proto - EventSender and helpers
// ===========================================================================

mod sidecar_proto {
    use super::*;
    use abp_sidecar_proto::EventSender;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn event_sender_ref_id() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "my-run");
        assert_eq!(sender.ref_id(), "my-run");
    }

    #[tokio::test]
    async fn event_sender_send_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "r1");
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        };
        sender.send_event(event).await.unwrap();
        let env = rx.try_recv().unwrap();
        assert!(matches!(env, Envelope::Event { ref_id, .. } if ref_id == "r1"));
    }

    #[tokio::test]
    async fn event_sender_send_final() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "r1");
        let receipt = ReceiptBuilder::new("x").build();
        sender.send_final(receipt).await.unwrap();
        let env = rx.try_recv().unwrap();
        assert!(matches!(env, Envelope::Final { ref_id, .. } if ref_id == "r1"));
    }

    #[tokio::test]
    async fn event_sender_send_fatal() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "r1");
        sender.send_fatal("boom").await.unwrap();
        let env = rx.try_recv().unwrap();
        match env {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("r1"));
                assert_eq!(error, "boom");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[tokio::test]
    async fn event_sender_closed_channel() {
        let (tx, rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "r1");
        drop(rx);
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "x".into() },
            ext: None,
        };
        let result = sender.send_event(event).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn event_sender_clone() {
        let (tx, rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "r1");
        let clone = sender.clone();
        assert_eq!(clone.ref_id(), "r1");
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "a".into() },
            ext: None,
        };
        sender.send_event(event).await.unwrap();
        let event2 = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "b".into() },
            ext: None,
        };
        clone.send_event(event2).await.unwrap();
        assert_eq!(rx.len(), 2);
    }
}

// ===========================================================================
// 22. Sidecar proto - async write helpers
// ===========================================================================

mod sidecar_proto_write {
    use super::*;
    use tokio::io::AsyncReadExt;

    async fn drain(mut r: tokio::io::DuplexStream) -> String {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn send_hello_writes_hello() {
        let (mut w, r) = tokio::io::duplex(4096);
        abp_sidecar_proto::send_hello(&mut w, backend("proto-test"), CapabilityManifest::new())
            .await
            .unwrap();
        drop(w);
        let text = drain(r).await;
        assert!(text.contains(r#""t":"hello""#));
        assert!(text.contains("proto-test"));
    }

    #[tokio::test]
    async fn send_event_writes_event() {
        let (mut w, r) = tokio::io::duplex(4096);
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        };
        abp_sidecar_proto::send_event(&mut w, "run-1", event)
            .await
            .unwrap();
        drop(w);
        let text = drain(r).await;
        assert!(text.contains(r#""t":"event""#));
        assert!(text.contains(r#""ref_id":"run-1""#));
    }

    #[tokio::test]
    async fn send_final_writes_final() {
        let (mut w, r) = tokio::io::duplex(8192);
        let receipt = ReceiptBuilder::new("x").build();
        abp_sidecar_proto::send_final(&mut w, "run-1", receipt)
            .await
            .unwrap();
        drop(w);
        let text = drain(r).await;
        assert!(text.contains(r#""t":"final""#));
    }

    #[tokio::test]
    async fn send_fatal_writes_fatal() {
        let (mut w, r) = tokio::io::duplex(4096);
        abp_sidecar_proto::send_fatal(&mut w, Some("r1".into()), "error msg")
            .await
            .unwrap();
        drop(w);
        let text = drain(r).await;
        assert!(text.contains(r#""t":"fatal""#));
        assert!(text.contains("error msg"));
    }

    #[tokio::test]
    async fn send_fatal_null_ref_id() {
        let (mut w, r) = tokio::io::duplex(4096);
        abp_sidecar_proto::send_fatal(&mut w, None, "early")
            .await
            .unwrap();
        drop(w);
        let text = drain(r).await;
        assert!(text.contains(r#""ref_id":null"#));
    }
}

// ===========================================================================
// 23. Router tests
// ===========================================================================

mod router {
    use super::*;
    use abp_protocol::router::{MessageRoute, MessageRouter, RouteTable};

    #[test]
    fn route_by_envelope_type() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "hello".into(),
            destination: "init-handler".into(),
            priority: 1,
        });
        let matched = router.route(&hello_env());
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().destination, "init-handler");
    }

    #[test]
    fn route_no_match() {
        let router = MessageRouter::new();
        assert!(router.route(&hello_env()).is_none());
    }

    #[test]
    fn route_by_ref_id_prefix() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "run-".into(),
            destination: "run-handler".into(),
            priority: 1,
        });
        let env = event_env("run-123", "msg");
        assert!(router.route(&env).is_some());
    }

    #[test]
    fn route_priority() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "fatal".into(),
            destination: "low".into(),
            priority: 1,
        });
        router.add_route(MessageRoute {
            pattern: "fatal".into(),
            destination: "high".into(),
            priority: 10,
        });
        let matched = router.route(&fatal_env(None, "err"));
        assert_eq!(matched.unwrap().destination, "high");
    }

    #[test]
    fn route_all() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "event".into(),
            destination: "ev".into(),
            priority: 1,
        });
        let envs = vec![hello_env(), event_env("r1", "a"), event_env("r1", "b")];
        let matches = router.route_all(&envs);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn remove_route() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "hello".into(),
            destination: "h".into(),
            priority: 1,
        });
        assert_eq!(router.route_count(), 1);
        router.remove_route("h");
        assert_eq!(router.route_count(), 0);
    }

    #[test]
    fn route_table_insert_lookup() {
        let mut table = RouteTable::new();
        table.insert("hello", "init");
        assert_eq!(table.lookup("hello"), Some("init"));
        assert_eq!(table.lookup("run"), None);
    }
}

// ===========================================================================
// 24. Batch processing tests
// ===========================================================================

mod batch {
    use super::*;
    use abp_protocol::batch::{BatchProcessor, BatchRequest, BatchValidationError, MAX_BATCH_SIZE};

    #[test]
    fn process_batch_all_valid() {
        let proc = BatchProcessor::new();
        let req = BatchRequest {
            id: "b1".into(),
            envelopes: vec![hello_env(), fatal_env(None, "err")],
            created_at: "2024-01-01T00:00:00Z".into(),
        };
        let resp = proc.process(req);
        assert_eq!(resp.request_id, "b1");
        assert_eq!(resp.results.len(), 2);
    }

    #[test]
    fn validate_empty_batch() {
        let proc = BatchProcessor::new();
        let req = BatchRequest {
            id: "b2".into(),
            envelopes: vec![],
            created_at: "2024-01-01T00:00:00Z".into(),
        };
        let errors = proc.validate_batch(&req);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, BatchValidationError::EmptyBatch))
        );
    }

    #[test]
    fn max_batch_size_constant() {
        assert_eq!(MAX_BATCH_SIZE, 1000);
    }
}

// ===========================================================================
// 25. Compression tests
// ===========================================================================

mod compression {
    use abp_protocol::compress::{CompressionAlgorithm, MessageCompressor};

    #[test]
    fn none_roundtrip() {
        let c = MessageCompressor::new(CompressionAlgorithm::None);
        let data = b"hello world";
        assert_eq!(c.decompress(&c.compress(data).unwrap()).unwrap(), data);
    }

    #[test]
    fn gzip_roundtrip() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let data = b"gzip test data";
        assert_eq!(c.decompress(&c.compress(data).unwrap()).unwrap(), data);
    }

    #[test]
    fn compress_message_roundtrip() {
        let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
        let data = b"message test";
        let msg = c.compress_message(data).unwrap();
        assert_eq!(msg.original_size, data.len());
        let decompressed = c.decompress_message(&msg).unwrap();
        assert_eq!(decompressed, data);
    }
}

// ===========================================================================
// 26. JSON structure verification
// ===========================================================================

mod json_structure {
    use super::*;

    #[test]
    fn hello_json_structure() {
        let json = encode(&hello_env());
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(v["t"], "hello");
        assert!(v["contract_version"].is_string());
        assert!(v["backend"].is_object());
        assert!(v["capabilities"].is_object());
    }

    #[test]
    fn run_json_structure() {
        let (_, env) = run_env("task");
        let json = encode(&env);
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(v["t"], "run");
        assert!(v["id"].is_string());
        assert!(v["work_order"].is_object());
        assert!(v["work_order"]["task"].is_string());
    }

    #[test]
    fn event_json_structure() {
        let json = encode(&event_env("r1", "msg"));
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(v["t"], "event");
        assert_eq!(v["ref_id"], "r1");
        assert!(v["event"].is_object());
    }

    #[test]
    fn fatal_json_structure() {
        let json = encode(&fatal_env(Some("r1"), "err"));
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(v["t"], "fatal");
        assert_eq!(v["ref_id"], "r1");
        assert_eq!(v["error"], "err");
    }

    #[test]
    fn final_json_structure() {
        let json = encode(&final_env("r1"));
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(v["t"], "final");
        assert_eq!(v["ref_id"], "r1");
        assert!(v["receipt"].is_object());
    }

    #[test]
    fn event_kind_type_field_present() {
        let json = encode(&event_env("r1", "hello"));
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        // AgentEvent flattens kind, so the "type" field appears inside "event"
        assert_eq!(v["event"]["type"], "assistant_message");
        assert_eq!(v["event"]["text"], "hello");
    }

    #[test]
    fn no_type_field_at_envelope_level() {
        let json = encode(&hello_env());
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert!(v.get("type").is_none());
    }

    #[test]
    fn execution_mode_serialization() {
        let env = Envelope::hello_with_mode(
            backend("x"),
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        let json = encode(&env);
        assert!(json.contains("pass_through") || json.contains("passthrough"));
    }
}
