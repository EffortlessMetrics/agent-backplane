#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
//! Exhaustive JSONL protocol conformance tests.
//!
//! Validates wire-format correctness, envelope discriminator semantics,
//! handshake ordering, ref_id correlation, codec framing, sequence
//! legality, large/unicode payloads, and backward compatibility.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, Outcome,
    PolicyProfile, ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_error::ErrorCode;
use abp_protocol::validate::{EnvelopeValidator, SequenceError};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

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

fn caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m
}

fn hello_env() -> Envelope {
    Envelope::hello(backend("conformance-sidecar"), caps())
}

fn hello_passthrough() -> Envelope {
    Envelope::hello_with_mode(backend("pt-sidecar"), caps(), ExecutionMode::Passthrough)
}

fn run_env(task: &str) -> (String, Envelope) {
    let wo = WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let id = wo.id.to_string();
    (id.clone(), Envelope::Run { id, work_order: wo })
}

fn event_msg(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        },
    }
}

fn event_delta(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: text.into() },
            ext: None,
        },
    }
}

fn event_tool_call(ref_id: &str, name: &str, input: Value) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: name.into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input,
            },
            ext: None,
        },
    }
}

fn event_tool_result(ref_id: &str, name: &str, output: Value) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: name.into(),
                tool_use_id: Some("tu-1".into()),
                output,
                is_error: false,
            },
            ext: None,
        },
    }
}

fn event_file_changed(ref_id: &str, path: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: path.into(),
                summary: "modified".into(),
            },
            ext: None,
        },
    }
}

fn event_command(ref_id: &str, cmd: &str, exit_code: Option<i32>) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: cmd.into(),
                exit_code,
                output_preview: Some("ok".into()),
            },
            ext: None,
        },
    }
}

fn event_warning(ref_id: &str, msg: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: msg.into(),
            },
            ext: None,
        },
    }
}

fn event_error(ref_id: &str, msg: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: msg.into(),
                error_code: None,
            },
            ext: None,
        },
    }
}

fn event_run_started(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        },
    }
}

fn event_run_completed(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    }
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: ReceiptBuilder::new("conformance-sidecar")
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

fn to_json(env: &Envelope) -> Value {
    let s = JsonlCodec::encode(env).unwrap();
    serde_json::from_str(s.trim()).unwrap()
}

fn validator() -> EnvelopeValidator {
    EnvelopeValidator::new()
}

// ===========================================================================
// 1. Envelope Serialization – "t" discriminator (10 tests)
// ===========================================================================

mod serialization_discriminator {
    use super::*;

    #[test]
    fn hello_serializes_with_t_hello() {
        assert_eq!(to_json(&hello_env())["t"], "hello");
    }

    #[test]
    fn run_serializes_with_t_run() {
        let (_, env) = run_env("task");
        assert_eq!(to_json(&env)["t"], "run");
    }

    #[test]
    fn event_serializes_with_t_event() {
        assert_eq!(to_json(&event_msg("r", "hi"))["t"], "event");
    }

    #[test]
    fn final_serializes_with_t_final() {
        assert_eq!(to_json(&final_env("r"))["t"], "final");
    }

    #[test]
    fn fatal_serializes_with_t_fatal() {
        assert_eq!(to_json(&fatal_env(Some("r"), "boom"))["t"], "fatal");
    }

    #[test]
    fn discriminator_field_is_t_not_type() {
        let v = to_json(&hello_env());
        assert!(v.get("type").is_none(), "should not use 'type' as tag");
        assert!(v.get("t").is_some(), "must use 't' as tag");
    }

    #[test]
    fn all_discriminators_are_snake_case() {
        let variants: Vec<Value> = vec![
            to_json(&hello_env()),
            to_json(&run_env("x").1),
            to_json(&event_msg("r", "x")),
            to_json(&final_env("r")),
            to_json(&fatal_env(None, "x")),
        ];
        for v in &variants {
            let t = v["t"].as_str().unwrap();
            assert_eq!(t, t.to_lowercase(), "discriminator must be snake_case: {t}");
            assert!(!t.contains('-'), "no hyphens in discriminator");
        }
    }

    #[test]
    fn hello_passthrough_mode_serialized() {
        let v = to_json(&hello_passthrough());
        assert_eq!(v["mode"], "passthrough");
    }

    #[test]
    fn hello_mapped_mode_is_default() {
        let v = to_json(&hello_env());
        assert_eq!(v["mode"], "mapped");
    }

    #[test]
    fn fatal_error_code_omitted_when_none() {
        let v = to_json(&fatal_env(Some("r"), "err"));
        assert!(
            v.get("error_code").is_none(),
            "error_code should be skip_serialized when None"
        );
    }
}

// ===========================================================================
// 2. Envelope Deserialization – all variants parse (10 tests)
// ===========================================================================

mod deserialization {
    use super::*;

    #[test]
    fn hello_from_raw_json() {
        let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        assert!(matches!(env, Envelope::Hello { .. }));
    }

    #[test]
    fn hello_without_mode_defaults_to_mapped() {
        let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        if let Envelope::Hello { mode, .. } = env {
            assert_eq!(mode, ExecutionMode::Mapped);
        } else {
            panic!("expected Hello");
        }
    }

    #[test]
    fn run_roundtrip() {
        let (id, env) = run_env("deser test");
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Run {
            id: rid,
            work_order,
        } = back
        {
            assert_eq!(rid, id);
            assert_eq!(work_order.task, "deser test");
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn event_assistant_message_roundtrip() {
        let env = event_msg("r1", "hello world");
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Event { ref_id, event } = back {
            assert_eq!(ref_id, "r1");
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { .. }
            ));
        } else {
            panic!("expected Event");
        }
    }

    #[test]
    fn event_delta_roundtrip() {
        let env = event_delta("r1", "tok");
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Event { event, .. } = back {
            if let AgentEventKind::AssistantDelta { text } = &event.kind {
                assert_eq!(text, "tok");
            } else {
                panic!("expected AssistantDelta");
            }
        } else {
            panic!("expected Event");
        }
    }

    #[test]
    fn final_roundtrip() {
        let env = final_env("run-99");
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Final { ref_id, receipt } = back {
            assert_eq!(ref_id, "run-99");
            assert_eq!(receipt.outcome, Outcome::Complete);
        } else {
            panic!("expected Final");
        }
    }

    #[test]
    fn fatal_roundtrip() {
        let env = fatal_env(Some("r1"), "kaboom");
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } = back
        {
            assert_eq!(ref_id.as_deref(), Some("r1"));
            assert_eq!(error, "kaboom");
            assert!(error_code.is_none());
        } else {
            panic!("expected Fatal");
        }
    }

    #[test]
    fn fatal_with_error_code_roundtrip() {
        let env = Envelope::fatal_with_code(
            Some("r1".into()),
            "auth failed",
            ErrorCode::BackendAuthFailed,
        );
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Fatal { error_code, .. } = back {
            assert_eq!(error_code, Some(ErrorCode::BackendAuthFailed));
        } else {
            panic!("expected Fatal");
        }
    }

    #[test]
    fn unknown_t_value_is_rejected() {
        let json = r#"{"t":"unknown_variant","data":123}"#;
        let result = serde_json::from_str::<Envelope>(json);
        assert!(result.is_err(), "unknown discriminator must fail");
    }

    #[test]
    fn missing_t_field_is_rejected() {
        let json = r#"{"contract_version":"abp/v0.1","backend":{"id":"x"}}"#;
        let result = serde_json::from_str::<Envelope>(json);
        assert!(result.is_err(), "missing 't' must fail");
    }
}

// ===========================================================================
// 3. Hello Handshake (10 tests)
// ===========================================================================

mod hello_handshake {
    use super::*;

    #[test]
    fn hello_carries_contract_version() {
        let v = to_json(&hello_env());
        assert_eq!(v["contract_version"], CONTRACT_VERSION);
    }

    #[test]
    fn hello_contract_version_is_abp_v01() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn hello_carries_backend_identity() {
        let v = to_json(&hello_env());
        assert_eq!(v["backend"]["id"], "conformance-sidecar");
        assert_eq!(v["backend"]["backend_version"], "1.0.0");
    }

    #[test]
    fn hello_carries_capabilities() {
        let v = to_json(&hello_env());
        let cap_obj = v["capabilities"].as_object().unwrap();
        assert!(cap_obj.contains_key("streaming"));
        assert!(cap_obj.contains_key("tool_read"));
    }

    #[test]
    fn hello_with_empty_capabilities() {
        let env = Envelope::hello(backend("bare"), BTreeMap::new());
        let v = to_json(&env);
        assert_eq!(v["capabilities"], serde_json::json!({}));
    }

    #[test]
    fn hello_with_many_capabilities() {
        let mut m = BTreeMap::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m.insert(Capability::ToolRead, SupportLevel::Native);
        m.insert(Capability::ToolWrite, SupportLevel::Native);
        m.insert(Capability::ToolEdit, SupportLevel::Emulated);
        m.insert(Capability::ToolBash, SupportLevel::Unsupported);
        let env = Envelope::hello(backend("full"), m);
        let v = to_json(&env);
        let cap = v["capabilities"].as_object().unwrap();
        assert!(cap.len() >= 5);
    }

    #[test]
    fn hello_restricted_capability_preserves_reason() {
        let mut m = BTreeMap::new();
        m.insert(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        );
        let env = Envelope::hello(backend("restricted"), m);
        let v = to_json(&env);
        let bash_cap = &v["capabilities"]["tool_bash"];
        assert_eq!(bash_cap["restricted"]["reason"], "sandbox");
    }

    #[test]
    fn hello_must_be_first_in_sequence() {
        let (id, run) = run_env("t");
        let seq = vec![run, hello_env(), final_env(&id)];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
        );
    }

    #[test]
    fn hello_from_foreign_version_parses() {
        let json = r#"{"t":"hello","contract_version":"abp/v99.0","backend":{"id":"future","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        if let Envelope::Hello {
            contract_version, ..
        } = env
        {
            assert_eq!(contract_version, "abp/v99.0");
        } else {
            panic!("expected Hello");
        }
    }

    #[test]
    fn hello_mode_passthrough_serialized() {
        let env = hello_passthrough();
        let v = to_json(&env);
        assert_eq!(v["mode"], "passthrough");
    }
}

// ===========================================================================
// 4. Run Envelope (8 tests)
// ===========================================================================

mod run_envelope {
    use super::*;

    #[test]
    fn run_carries_work_order_task() {
        let (_, env) = run_env("my task");
        let v = to_json(&env);
        assert_eq!(v["work_order"]["task"], "my task");
    }

    #[test]
    fn run_has_id_matching_work_order_id() {
        let (id, env) = run_env("check id");
        let v = to_json(&env);
        let wo_id = v["work_order"]["id"].as_str().unwrap();
        assert_eq!(v["id"].as_str().unwrap(), id);
        assert_eq!(id, wo_id);
    }

    #[test]
    fn run_work_order_has_workspace_spec() {
        let (_, env) = run_env("ws check");
        let v = to_json(&env);
        assert!(v["work_order"]["workspace"].is_object());
        assert_eq!(v["work_order"]["workspace"]["root"], ".");
    }

    #[test]
    fn run_work_order_has_policy() {
        let (_, env) = run_env("policy check");
        let v = to_json(&env);
        assert!(v["work_order"]["policy"].is_object());
    }

    #[test]
    fn run_work_order_has_context() {
        let (_, env) = run_env("ctx check");
        let v = to_json(&env);
        assert!(v["work_order"]["context"].is_object());
    }

    #[test]
    fn run_work_order_has_config() {
        let (_, env) = run_env("cfg check");
        let v = to_json(&env);
        assert!(v["work_order"]["config"].is_object());
    }

    #[test]
    fn run_work_order_has_lane() {
        let (_, env) = run_env("lane check");
        let v = to_json(&env);
        assert!(v["work_order"]["lane"].is_string());
    }

    #[test]
    fn run_work_order_uuid_is_valid() {
        let (_, env) = run_env("uuid check");
        let v = to_json(&env);
        let id_str = v["work_order"]["id"].as_str().unwrap();
        Uuid::parse_str(id_str).expect("work_order.id must be valid UUID");
    }
}

// ===========================================================================
// 5. Event Envelope – all AgentEventKind variants (11 tests)
// ===========================================================================

mod event_envelope {
    use super::*;

    #[test]
    fn assistant_message_event() {
        let env = event_msg("r", "hello");
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "assistant_message");
        assert_eq!(v["event"]["text"], "hello");
    }

    #[test]
    fn assistant_delta_event() {
        let env = event_delta("r", "tok");
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "assistant_delta");
        assert_eq!(v["event"]["text"], "tok");
    }

    #[test]
    fn tool_call_event() {
        let env = event_tool_call("r", "read_file", serde_json::json!({"path": "a.txt"}));
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "tool_call");
        assert_eq!(v["event"]["tool_name"], "read_file");
        assert_eq!(v["event"]["input"]["path"], "a.txt");
    }

    #[test]
    fn tool_result_event() {
        let env = event_tool_result("r", "read_file", serde_json::json!("content"));
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "tool_result");
        assert_eq!(v["event"]["tool_name"], "read_file");
        assert_eq!(v["event"]["is_error"], false);
    }

    #[test]
    fn file_changed_event() {
        let env = event_file_changed("r", "src/main.rs");
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "file_changed");
        assert_eq!(v["event"]["path"], "src/main.rs");
    }

    #[test]
    fn command_executed_event() {
        let env = event_command("r", "cargo test", Some(0));
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "command_executed");
        assert_eq!(v["event"]["command"], "cargo test");
        assert_eq!(v["event"]["exit_code"], 0);
    }

    #[test]
    fn warning_event() {
        let env = event_warning("r", "rate limit approaching");
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "warning");
        assert_eq!(v["event"]["message"], "rate limit approaching");
    }

    #[test]
    fn error_event() {
        let env = event_error("r", "something broke");
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "error");
        assert_eq!(v["event"]["message"], "something broke");
    }

    #[test]
    fn run_started_event() {
        let env = event_run_started("r");
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "run_started");
    }

    #[test]
    fn run_completed_event() {
        let env = event_run_completed("r");
        let v = to_json(&env);
        assert_eq!(v["event"]["type"], "run_completed");
    }

    #[test]
    fn event_has_timestamp() {
        let env = event_msg("r", "ts check");
        let v = to_json(&env);
        assert!(v["event"]["ts"].is_string(), "event must have ts field");
    }
}

// ===========================================================================
// 6. Final Envelope – Receipt (8 tests)
// ===========================================================================

mod final_envelope {
    use super::*;

    #[test]
    fn final_carries_receipt() {
        let v = to_json(&final_env("r1"));
        assert!(v["receipt"].is_object());
    }

    #[test]
    fn receipt_has_outcome() {
        let v = to_json(&final_env("r1"));
        assert_eq!(v["receipt"]["outcome"], "complete");
    }

    #[test]
    fn receipt_has_meta() {
        let v = to_json(&final_env("r1"));
        assert!(v["receipt"]["meta"].is_object());
        assert!(v["receipt"]["meta"]["run_id"].is_string());
    }

    #[test]
    fn receipt_has_backend_identity() {
        let v = to_json(&final_env("r1"));
        assert_eq!(v["receipt"]["backend"]["id"], "conformance-sidecar");
    }

    #[test]
    fn receipt_has_usage() {
        let v = to_json(&final_env("r1"));
        assert!(v["receipt"]["usage"].is_object());
    }

    #[test]
    fn receipt_partial_outcome() {
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt: ReceiptBuilder::new("test")
                .outcome(Outcome::Partial)
                .build(),
        };
        let v = to_json(&env);
        assert_eq!(v["receipt"]["outcome"], "partial");
    }

    #[test]
    fn receipt_failed_outcome() {
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt: ReceiptBuilder::new("test").outcome(Outcome::Failed).build(),
        };
        let v = to_json(&env);
        assert_eq!(v["receipt"]["outcome"], "failed");
    }

    #[test]
    fn receipt_contract_version_in_meta() {
        let v = to_json(&final_env("r1"));
        assert_eq!(v["receipt"]["meta"]["contract_version"], CONTRACT_VERSION);
    }
}

// ===========================================================================
// 7. Fatal Envelope (8 tests)
// ===========================================================================

mod fatal_envelope {
    use super::*;

    #[test]
    fn fatal_preserves_error_message() {
        let env = fatal_env(Some("r"), "things went wrong");
        let v = to_json(&env);
        assert_eq!(v["error"], "things went wrong");
    }

    #[test]
    fn fatal_with_ref_id() {
        let v = to_json(&fatal_env(Some("run-42"), "err"));
        assert_eq!(v["ref_id"], "run-42");
    }

    #[test]
    fn fatal_without_ref_id() {
        let v = to_json(&fatal_env(None, "err"));
        assert!(v["ref_id"].is_null());
    }

    #[test]
    fn fatal_with_error_code() {
        let env =
            Envelope::fatal_with_code(Some("r1".into()), "timeout", ErrorCode::BackendTimeout);
        let v = to_json(&env);
        assert_eq!(v["error_code"], "backend_timeout");
    }

    #[test]
    fn fatal_error_code_roundtrip_all_protocol_codes() {
        let codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
        ];
        for code in codes {
            let env = Envelope::fatal_with_code(None, "test", code);
            let json = serde_json::to_string(&env).unwrap();
            let back: Envelope = serde_json::from_str(&json).unwrap();
            assert_eq!(back.error_code(), Some(code));
        }
    }

    #[test]
    fn fatal_error_method_returns_none_for_no_code() {
        let env = fatal_env(None, "no code");
        assert!(env.error_code().is_none());
    }

    #[test]
    fn fatal_error_method_returns_none_for_non_fatal() {
        let env = hello_env();
        assert!(env.error_code().is_none());
    }

    #[test]
    fn fatal_from_abp_error() {
        let abp_err = abp_error::AbpError {
            code: ErrorCode::BackendCrashed,
            message: "crash".into(),
            context: BTreeMap::new(),
            source: None,
            location: None,
        };
        let env = Envelope::fatal_from_abp_error(Some("r1".into()), &abp_err);
        if let Envelope::Fatal {
            error, error_code, ..
        } = &env
        {
            assert_eq!(error, "crash");
            assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
        } else {
            panic!("expected Fatal");
        }
    }
}

// ===========================================================================
// 8. ref_id Correlation (8 tests)
// ===========================================================================

mod ref_id_correlation {
    use super::*;

    #[test]
    fn event_ref_id_matches_run_id() {
        let (id, _) = run_env("ref test");
        let env = event_msg(&id, "msg");
        if let Envelope::Event { ref_id, .. } = &env {
            assert_eq!(ref_id, &id);
        }
    }

    #[test]
    fn final_ref_id_matches_run_id() {
        let (id, _) = run_env("ref test");
        let env = final_env(&id);
        if let Envelope::Final { ref_id, .. } = &env {
            assert_eq!(ref_id, &id);
        }
    }

    #[test]
    fn mismatched_ref_id_detected_by_validator() {
        let (id, run) = run_env("ref mismatch");
        let seq = vec![
            hello_env(),
            run,
            event_msg("wrong-ref", "oops"),
            final_env(&id),
        ];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
            "mismatched ref_id must be detected: {errors:?}"
        );
    }

    #[test]
    fn all_events_share_same_ref_id() {
        let (id, run) = run_env("shared ref");
        let seq = vec![
            hello_env(),
            run,
            event_msg(&id, "a"),
            event_delta(&id, "b"),
            event_file_changed(&id, "x.rs"),
            final_env(&id),
        ];
        let errors = validator().validate_sequence(&seq);
        assert!(errors.is_empty(), "all same ref_id: {errors:?}");
    }

    #[test]
    fn fatal_ref_id_is_optional() {
        let env = fatal_env(None, "no ref");
        let v = to_json(&env);
        assert!(v["ref_id"].is_null());
    }

    #[test]
    fn fatal_with_ref_id_matches_run() {
        let (id, run) = run_env("fatal ref");
        let seq = vec![hello_env(), run, fatal_env(Some(&id), "err")];
        let errors = validator().validate_sequence(&seq);
        // Fatal is a valid terminal – ref_id match is checked
        let has_mismatch = errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }));
        assert!(!has_mismatch, "matching ref_id should not mismatch");
    }

    #[test]
    fn ref_id_preserved_through_codec_roundtrip() {
        let env = event_msg("unique-ref-id-42", "msg");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { ref_id, .. } = decoded {
            assert_eq!(ref_id, "unique-ref-id-42");
        } else {
            panic!("expected Event");
        }
    }

    #[test]
    fn ref_id_with_special_characters() {
        let special_ref = "ref/with:special-chars_123";
        let env = event_msg(special_ref, "msg");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { ref_id, .. } = decoded {
            assert_eq!(ref_id, special_ref);
        } else {
            panic!("expected Event");
        }
    }
}

// ===========================================================================
// 9. JSONL Codec (10 tests)
// ===========================================================================

mod jsonl_codec {
    use super::*;

    #[test]
    fn encode_produces_trailing_newline() {
        let line = JsonlCodec::encode(&hello_env()).unwrap();
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn encode_is_single_line() {
        let line = JsonlCodec::encode(&hello_env()).unwrap();
        let trimmed = line.trim_end_matches('\n');
        assert!(!trimmed.contains('\n'));
    }

    #[test]
    fn decode_tolerates_trailing_whitespace() {
        let mut line = JsonlCodec::encode(&hello_env()).unwrap();
        line = line.trim().to_string() + "   ";
        let result = JsonlCodec::decode(&line);
        assert!(result.is_ok());
    }

    #[test]
    fn decode_rejects_empty_string() {
        let result = JsonlCodec::decode("");
        assert!(result.is_err());
    }

    #[test]
    fn decode_rejects_invalid_json() {
        let result = JsonlCodec::decode("{not valid json}");
        assert!(result.is_err());
    }

    #[test]
    fn decode_stream_multiple_lines() {
        let mut buf = String::new();
        buf.push_str(&JsonlCodec::encode(&hello_env()).unwrap());
        let (id, run) = run_env("stream");
        buf.push_str(&JsonlCodec::encode(&run).unwrap());
        buf.push_str(&JsonlCodec::encode(&event_msg(&id, "hi")).unwrap());
        buf.push_str(&JsonlCodec::encode(&final_env(&id)).unwrap());

        let reader = BufReader::new(buf.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(envs.len(), 4);
        assert!(matches!(envs[0], Envelope::Hello { .. }));
        assert!(matches!(envs[1], Envelope::Run { .. }));
        assert!(matches!(envs[2], Envelope::Event { .. }));
        assert!(matches!(envs[3], Envelope::Final { .. }));
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let hello = JsonlCodec::encode(&hello_env()).unwrap();
        let fatal = JsonlCodec::encode(&fatal_env(None, "x")).unwrap();
        let buf = format!("\n\n{hello}\n\n\n{fatal}\n\n");
        let reader = BufReader::new(buf.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(envs.len(), 2);
    }

    #[test]
    fn encode_to_writer_works() {
        let mut buf: Vec<u8> = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &hello_env()).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.ends_with('\n'));
        let decoded = JsonlCodec::decode(s.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn encode_many_to_writer() {
        let mut buf: Vec<u8> = Vec::new();
        let envs = [hello_env(), fatal_env(None, "done")];
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn codec_roundtrip_all_variants() {
        let (id, run) = run_env("codec roundtrip");
        let envelopes = vec![
            hello_env(),
            run,
            event_msg(&id, "msg"),
            final_env(&id),
            fatal_env(Some(&id), "err"),
        ];
        for env in &envelopes {
            let encoded = JsonlCodec::encode(env).unwrap();
            let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
            // Check discriminator survives
            let orig_t = to_json(env)["t"].as_str().unwrap().to_string();
            let decoded_t = to_json(&decoded)["t"].as_str().unwrap().to_string();
            assert_eq!(orig_t, decoded_t);
        }
    }
}

// ===========================================================================
// 10. Protocol Sequence Validation (10 tests)
// ===========================================================================

mod sequence_validation {
    use super::*;

    #[test]
    fn valid_hello_run_final() {
        let (id, run) = run_env("ok");
        let seq = vec![hello_env(), run, final_env(&id)];
        let errors = validator().validate_sequence(&seq);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn valid_hello_run_events_final() {
        let (id, run) = run_env("ok");
        let seq = vec![
            hello_env(),
            run,
            event_run_started(&id),
            event_msg(&id, "a"),
            event_delta(&id, "b"),
            event_tool_call(&id, "bash", serde_json::json!({"cmd": "ls"})),
            event_tool_result(&id, "bash", serde_json::json!("output")),
            event_file_changed(&id, "x.rs"),
            event_command(&id, "ls", Some(0)),
            event_run_completed(&id),
            final_env(&id),
        ];
        let errors = validator().validate_sequence(&seq);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn valid_hello_run_fatal() {
        let (id, run) = run_env("fatal");
        let seq = vec![hello_env(), run, fatal_env(Some(&id), "err")];
        let errors = validator().validate_sequence(&seq);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn invalid_no_hello() {
        let (id, run) = run_env("no hello");
        let seq = vec![run, final_env(&id)];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors.iter().any(|e| matches!(
                e,
                SequenceError::HelloNotFirst { .. } | SequenceError::MissingHello
            )),
            "{errors:?}"
        );
    }

    #[test]
    fn invalid_no_terminal() {
        let (_, run) = run_env("no terminal");
        let seq = vec![hello_env(), run];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingTerminal)),
            "{errors:?}"
        );
    }

    #[test]
    fn invalid_hello_not_first() {
        let (id, run) = run_env("late hello");
        let seq = vec![run, hello_env(), final_env(&id)];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
            "{errors:?}"
        );
    }

    #[test]
    fn invalid_events_before_run() {
        let seq = vec![hello_env(), event_msg("r", "early"), final_env("r")];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::OutOfOrderEvents)),
            "{errors:?}"
        );
    }

    #[test]
    fn empty_sequence_errors() {
        let seq: Vec<Envelope> = vec![];
        let errors = validator().validate_sequence(&seq);
        assert!(!errors.is_empty(), "empty seq must produce errors");
    }

    #[test]
    fn single_hello_only_errors() {
        let seq = vec![hello_env()];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingTerminal)),
            "{errors:?}"
        );
    }

    #[test]
    fn valid_hello_fatal_no_run() {
        // Fatal can appear without a prior Run (sidecar crashed at startup)
        let seq = vec![hello_env(), fatal_env(None, "startup crash")];
        let errors = validator().validate_sequence(&seq);
        // This is a valid early-abort sequence
        assert!(errors.is_empty(), "{errors:?}");
    }
}

// ===========================================================================
// 11. Large Payload Handling (5 tests)
// ===========================================================================

mod large_payloads {
    use super::*;

    #[test]
    fn large_text_payload_100k() {
        let big = "A".repeat(100_000);
        let env = event_msg("r", &big);
        let encoded = JsonlCodec::encode(&env).unwrap();
        let trimmed = encoded.trim_end_matches('\n');
        assert!(!trimmed.contains('\n'), "must stay single line");
        let decoded = JsonlCodec::decode(trimmed).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert_eq!(text.len(), 100_000);
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong envelope");
        }
    }

    #[test]
    fn large_tool_input_payload() {
        let big_val = serde_json::json!({
            "data": "x".repeat(50_000),
            "nested": {"arr": vec![1; 1000]},
        });
        let env = event_tool_call("r", "big_tool", big_val);
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Event { .. }));
    }

    #[test]
    fn many_events_stream_roundtrip() {
        let mut buf: Vec<u8> = Vec::new();
        let ref_id = "batch";
        let count = 500;
        for i in 0..count {
            let env = event_delta(ref_id, &format!("token-{i}"));
            JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        }
        let reader = BufReader::new(buf.as_slice());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(envs.len(), count);
    }

    #[test]
    fn large_receipt_with_trace() {
        let mut builder = ReceiptBuilder::new("test").outcome(Outcome::Complete);
        for i in 0..100 {
            builder = builder.add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{i}"),
                },
                ext: None,
            });
        }
        let receipt = builder.build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Final { receipt, .. } = decoded {
            assert_eq!(receipt.trace.len(), 100);
        } else {
            panic!("expected Final");
        }
    }

    #[test]
    fn large_text_payload_1m() {
        let big = "B".repeat(1_000_000);
        let env = event_msg("r", &big);
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert_eq!(text.len(), 1_000_000);
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong envelope");
        }
    }
}

// ===========================================================================
// 12. Unicode and Special Characters (10 tests)
// ===========================================================================

mod unicode_and_special_chars {
    use super::*;

    #[test]
    fn cjk_characters() {
        let env = event_msg("r", "你好世界 こんにちは 안녕하세요");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert!(text.contains("你好世界"));
                assert!(text.contains("こんにちは"));
                assert!(text.contains("안녕하세요"));
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong env");
        }
    }

    #[test]
    fn emoji() {
        let env = event_msg("r", "🚀🎉🌍💻🔥");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert!(text.contains("🚀"));
                assert!(text.contains("🌍"));
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong env");
        }
    }

    #[test]
    fn newlines_in_text_escaped() {
        let env = event_msg("r", "line1\nline2\r\nline3");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let trimmed = encoded.trim_end_matches('\n');
        assert!(!trimmed.contains('\n'), "newlines must be JSON-escaped");
    }

    #[test]
    fn tabs_and_control_chars() {
        let env = event_msg("r", "tab\there\x00null\x01soh");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert!(text.contains("tab\there"));
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong env");
        }
    }

    #[test]
    fn backslash_and_quotes() {
        let env = event_msg("r", r#"path\to\"file" and 'quoted'"#);
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert!(text.contains("path\\to\\\"file\""));
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong env");
        }
    }

    #[test]
    fn rtl_and_bidi_text() {
        let env = event_msg("r", "مرحبا بالعالم");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert!(text.contains("مرحبا"));
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong env");
        }
    }

    #[test]
    fn math_symbols() {
        let env = event_msg("r", "∀x∈ℝ: x² ≥ 0 ∧ √4 = 2");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert!(text.contains("∀x∈ℝ"));
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong env");
        }
    }

    #[test]
    fn unicode_in_backend_id() {
        let b = BackendIdentity {
            id: "бэкенд-тест".into(),
            backend_version: None,
            adapter_version: None,
        };
        let env = Envelope::hello(b, BTreeMap::new());
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Hello { backend, .. } = decoded {
            assert_eq!(backend.id, "бэкенд-тест");
        } else {
            panic!("expected Hello");
        }
    }

    #[test]
    fn unicode_in_error_message() {
        let env = fatal_env(None, "错误: 连接超时 ⚠️");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Fatal { error, .. } = decoded {
            assert!(error.contains("错误"));
            assert!(error.contains("⚠️"));
        } else {
            panic!("expected Fatal");
        }
    }

    #[test]
    fn zero_width_chars_preserved() {
        // zero-width space, zero-width joiner
        let text = "a\u{200B}b\u{200D}c";
        let env = event_msg("r", text);
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            if let AgentEventKind::AssistantMessage { text: t } = &event.kind {
                assert_eq!(t, text);
            } else {
                panic!("wrong kind");
            }
        } else {
            panic!("wrong env");
        }
    }
}

// ===========================================================================
// 13. Backward Compatibility / Wire Format Stability (6 tests)
// ===========================================================================

mod backward_compat {
    use super::*;

    #[test]
    fn hello_json_structure_is_stable() {
        let v = to_json(&hello_env());
        // Required top-level fields for Hello
        assert!(v.get("t").is_some());
        assert!(v.get("contract_version").is_some());
        assert!(v.get("backend").is_some());
        assert!(v.get("capabilities").is_some());
        assert!(v.get("mode").is_some());
    }

    #[test]
    fn run_json_structure_is_stable() {
        let (_, env) = run_env("stable");
        let v = to_json(&env);
        assert!(v.get("t").is_some());
        assert!(v.get("id").is_some());
        assert!(v.get("work_order").is_some());
    }

    #[test]
    fn event_json_structure_is_stable() {
        let v = to_json(&event_msg("r", "x"));
        assert!(v.get("t").is_some());
        assert!(v.get("ref_id").is_some());
        assert!(v.get("event").is_some());
    }

    #[test]
    fn final_json_structure_is_stable() {
        let v = to_json(&final_env("r"));
        assert!(v.get("t").is_some());
        assert!(v.get("ref_id").is_some());
        assert!(v.get("receipt").is_some());
    }

    #[test]
    fn fatal_json_structure_is_stable() {
        let v = to_json(&fatal_env(Some("r"), "err"));
        assert!(v.get("t").is_some());
        assert!(v.get("ref_id").is_some());
        assert!(v.get("error").is_some());
    }

    #[test]
    fn event_uses_type_not_t_for_kind() {
        let v = to_json(&event_msg("r", "x"));
        let event = &v["event"];
        assert!(event.get("type").is_some(), "AgentEventKind uses 'type'");
        assert!(event.get("t").is_none(), "AgentEventKind must NOT use 't'");
    }
}

// ===========================================================================
// 14. Extension field / Passthrough mode (4 tests)
// ===========================================================================

mod extension_field {
    use super::*;

    #[test]
    fn ext_field_omitted_when_none() {
        let v = to_json(&event_msg("r", "no ext"));
        assert!(v["event"].get("ext").is_none());
    }

    #[test]
    fn ext_field_roundtrip() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "raw_message".to_string(),
            serde_json::json!({"role": "assistant", "content": "hi"}),
        );
        let env = Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "hi".into() },
                ext: Some(ext),
            },
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            let ext = event.ext.unwrap();
            assert!(ext.contains_key("raw_message"));
        } else {
            panic!("expected Event");
        }
    }

    #[test]
    fn ext_field_with_nested_json() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "sdk_data".to_string(),
            serde_json::json!({
                "deep": {"nested": {"value": [1, 2, 3]}},
            }),
        );
        let env = Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "t".into() },
                ext: Some(ext),
            },
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            let ext = event.ext.unwrap();
            let deep = &ext["sdk_data"]["deep"]["nested"]["value"];
            assert_eq!(deep, &serde_json::json!([1, 2, 3]));
        } else {
            panic!("expected Event");
        }
    }

    #[test]
    fn ext_field_with_empty_map() {
        let env = Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "t".into() },
                ext: Some(BTreeMap::new()),
            },
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { event, .. } = decoded {
            assert!(event.ext.unwrap().is_empty());
        } else {
            panic!("expected Event");
        }
    }
}
