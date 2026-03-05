#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, ReceiptBuilder, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_protocol::{
    Envelope, JsonlCodec, ProtocolError,
    validate::{EnvelopeValidator, SequenceError, ValidationError, ValidationWarning},
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn make_caps() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps
}

fn make_hello() -> Envelope {
    Envelope::hello(make_backend("test-sidecar"), make_caps())
}

fn make_hello_with_mode(mode: ExecutionMode) -> Envelope {
    Envelope::hello_with_mode(make_backend("test-sidecar"), make_caps(), mode)
}

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Fix the bug").build()
}

fn make_run(id: &str) -> Envelope {
    Envelope::Run {
        id: id.into(),
        work_order: make_work_order(),
    }
}

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build()
}

fn make_final(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: make_receipt(),
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn decode_stream(input: &str) -> Vec<Result<Envelope, ProtocolError>> {
    let reader = BufReader::new(input.as_bytes());
    JsonlCodec::decode_stream(reader).collect()
}

// ===================================================================
// 1. Envelope tag field: "t" not "type"
// ===================================================================

#[test]
fn t_field_hello_uses_t_discriminator() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(
        json.contains(r#""t":"hello""#),
        "expected t:hello in {json}"
    );
    assert!(!json.contains(r#""type":"hello""#));
}

#[test]
fn t_field_run_uses_t_discriminator() {
    let env = make_run("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn t_field_event_uses_t_discriminator() {
    let env = make_event(
        "run-1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn t_field_final_uses_t_discriminator() {
    let env = make_final("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn t_field_fatal_uses_t_discriminator() {
    let env = make_fatal(Some("run-1"), "boom");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn t_field_decode_rejects_type_discriminator() {
    let json = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn t_field_decode_accepts_t_discriminator() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

// ===================================================================
// 2. Hello envelope fields
// ===================================================================

#[test]
fn hello_contract_version_matches_constant() {
    let env = make_hello();
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_contract_version_roundtrips() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello {
        contract_version, ..
    } = decoded
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_backend_id_preserved() {
    let env = Envelope::hello(make_backend("my-cool-sidecar"), BTreeMap::new());
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { backend, .. } = decoded {
        assert_eq!(backend.id, "my-cool-sidecar");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_backend_version_preserved() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { backend, .. } = decoded {
        assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
        assert_eq!(backend.adapter_version.as_deref(), Some("0.1.0"));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_empty_capabilities() {
    let env = Envelope::hello(make_backend("x"), BTreeMap::new());
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_multiple_capabilities() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert_eq!(capabilities.len(), 2);
        assert!(capabilities.contains_key(&Capability::Streaming));
        assert!(capabilities.contains_key(&Capability::ToolRead));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_default_mode_is_mapped() {
    let env = make_hello();
    if let Envelope::Hello { mode, .. } = &env {
        assert_eq!(*mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_passthrough_mode() {
    let env = make_hello_with_mode(ExecutionMode::Passthrough);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_mode_absent_defaults_to_mapped() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_all_capability_variants_serialize() {
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
    let mut manifest = CapabilityManifest::new();
    for cap in &all_caps {
        manifest.insert(cap.clone(), SupportLevel::Native);
    }
    let env = Envelope::hello(make_backend("full"), manifest.clone());
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert_eq!(capabilities.len(), all_caps.len());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_support_level_variants() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Unsupported);
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let env = Envelope::hello(make_backend("sl"), caps);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(matches!(
            capabilities.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
        assert!(matches!(
            capabilities.get(&Capability::ToolRead),
            Some(SupportLevel::Emulated)
        ));
        assert!(matches!(
            capabilities.get(&Capability::ToolWrite),
            Some(SupportLevel::Unsupported)
        ));
        assert!(matches!(
            capabilities.get(&Capability::ToolBash),
            Some(SupportLevel::Restricted { .. })
        ));
    } else {
        panic!("expected Hello");
    }
}

// ===================================================================
// 3. Run envelope
// ===================================================================

#[test]
fn run_envelope_roundtrip() {
    let env = make_run("run-abc");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "run-abc");
        assert_eq!(work_order.task, "Fix the bug");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_envelope_preserves_work_order_task() {
    let wo = WorkOrderBuilder::new("Refactor auth module").build();
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.task, "Refactor auth module");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_envelope_work_order_has_uuid() {
    let env = make_run("r2");
    if let Envelope::Run { work_order, .. } = &env {
        assert_ne!(work_order.id, Uuid::nil());
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_envelope_work_order_fields_roundtrip() {
    let wo = WorkOrderBuilder::new("Test task")
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(10.0)
        .build();
    let env = Envelope::Run {
        id: "r3".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
        assert_eq!(work_order.config.max_turns, Some(5));
        assert_eq!(work_order.config.max_budget_usd, Some(10.0));
    } else {
        panic!("expected Run");
    }
}

// ===================================================================
// 4. Event envelope – all AgentEventKind variants
// ===================================================================

#[test]
fn event_run_started() {
    let env = make_event(
        "r1",
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"run_started""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, ref_id, .. } = decoded {
        assert_eq!(ref_id, "r1");
        assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_run_completed() {
    let env = make_event(
        "r1",
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"run_completed""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_assistant_delta() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"assistant_delta""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantDelta { text } = &event.kind {
            assert_eq!(text, "Hello");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_assistant_message() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: "Full message".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"assistant_message""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "Full message");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_call() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "main.rs"}),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"tool_call""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input,
        } = &event.kind
        {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
            assert!(parent_tool_use_id.is_none());
            assert_eq!(input["path"], "main.rs");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_call_with_parent() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "grep".into(),
            tool_use_id: Some("tu-2".into()),
            parent_tool_use_id: Some("tu-1".into()),
            input: json!({"pattern": "fn main"}),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall {
            parent_tool_use_id, ..
        } = &event.kind
        {
            assert_eq!(parent_tool_use_id.as_deref(), Some("tu-1"));
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_success() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"tool_result""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolResult {
            is_error, output, ..
        } = &event.kind
        {
            assert!(!is_error);
            assert_eq!(output["content"], "fn main() {}");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_error() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: json!("permission denied"),
            is_error: true,
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolResult { is_error, .. } = &event.kind {
            assert!(is_error);
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_file_changed() {
    let env = make_event(
        "r1",
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "Added error handling".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"file_changed""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::FileChanged { path, summary } = &event.kind {
            assert_eq!(path, "src/main.rs");
            assert_eq!(summary, "Added error handling");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_command_executed() {
    let env = make_event(
        "r1",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"command_executed""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } = &event.kind
        {
            assert_eq!(command, "cargo test");
            assert_eq!(*exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("test result: ok"));
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_command_executed_no_exit_code() {
    let env = make_event(
        "r1",
        AgentEventKind::CommandExecuted {
            command: "kill -9".into(),
            exit_code: None,
            output_preview: None,
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::CommandExecuted { exit_code, .. } = &event.kind {
            assert!(exit_code.is_none());
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_warning() {
    let env = make_event(
        "r1",
        AgentEventKind::Warning {
            message: "budget at 90%".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"warning""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::Warning { message } = &event.kind {
            assert_eq!(message, "budget at 90%");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_error_without_code() {
    let env = make_event(
        "r1",
        AgentEventKind::Error {
            message: "unexpected crash".into(),
            error_code: None,
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""type":"error""#));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::Error {
            message,
            error_code,
        } = &event.kind
        {
            assert_eq!(message, "unexpected crash");
            assert!(error_code.is_none());
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_with_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        json!({"sdk": "anthropic", "data": 42}),
    );
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("raw_message"));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(event.ext.is_some());
        let ext = event.ext.unwrap();
        assert!(ext.contains_key("raw_message"));
    } else {
        panic!("expected Event");
    }
}

// ===================================================================
// 5. Final envelope
// ===================================================================

#[test]
fn final_envelope_roundtrip() {
    let env = make_final("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { ref_id, receipt } = decoded {
        assert_eq!(ref_id, "run-1");
        assert_eq!(receipt.outcome, Outcome::Complete);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_backend_preserved() {
    let env = make_final("run-2");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.backend.id, "test-sidecar");
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_outcome_partial() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Partial)
        .build();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.outcome, Outcome::Partial);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_outcome_failed() {
    let receipt = ReceiptBuilder::new("test").outcome(Outcome::Failed).build();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.outcome, Outcome::Failed);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_with_trace_events() {
    let receipt = ReceiptBuilder::new("test")
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.trace.len(), 2);
    } else {
        panic!("expected Final");
    }
}

// ===================================================================
// 6. Fatal envelope
// ===================================================================

#[test]
fn fatal_with_ref_id() {
    let env = make_fatal(Some("run-1"), "out of memory");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = decoded
    {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
        assert_eq!(error, "out of memory");
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_without_ref_id() {
    let env = make_fatal(None, "startup failure");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = decoded {
        assert!(ref_id.is_none());
        assert_eq!(error, "startup failure");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "handshake failed",
        abp_error::ErrorCode::ProtocolHandshakeFailed,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { error_code, .. } = decoded {
        assert!(error_code.is_some());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_error_code_method() {
    let env = Envelope::fatal_with_code(
        None,
        "bad envelope",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    assert!(env.error_code().is_some());
}

#[test]
fn non_fatal_error_code_is_none() {
    let env = make_hello();
    assert!(env.error_code().is_none());
}

// ===================================================================
// 7. JSONL codec: encode/decode basics
// ===================================================================

#[test]
fn encode_ends_with_newline() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.ends_with('\n'));
}

#[test]
fn encode_is_single_line() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let trimmed = json.trim_end_matches('\n');
    assert!(!trimmed.contains('\n'));
}

#[test]
fn decode_valid_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn decode_invalid_json() {
    assert!(JsonlCodec::decode("not json at all").is_err());
}

#[test]
fn decode_empty_string() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn decode_empty_object() {
    assert!(JsonlCodec::decode("{}").is_err());
}

#[test]
fn decode_unknown_t_value() {
    let json = r#"{"t":"unknown_type","data":"test"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn decode_missing_required_fields() {
    let json = r#"{"t":"hello"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

// ===================================================================
// 8. JSONL stream: decode_stream
// ===================================================================

#[test]
fn stream_multiple_envelopes() {
    let e1 = JsonlCodec::encode(&make_hello()).unwrap();
    let e2 = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();
    let input = format!("{e1}{e2}");
    let results = decode_stream(&input);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());
}

#[test]
fn stream_skips_empty_lines() {
    let e1 = JsonlCodec::encode(&make_hello()).unwrap();
    let e2 = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();
    let input = format!("{e1}\n\n\n{e2}");
    let results = decode_stream(&input);
    assert_eq!(results.len(), 2);
}

#[test]
fn stream_skips_whitespace_only_lines() {
    let e1 = JsonlCodec::encode(&make_hello()).unwrap();
    let input = format!("{e1}   \n  \n");
    let results = decode_stream(&input);
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_empty_input() {
    let results = decode_stream("");
    assert!(results.is_empty());
}

#[test]
fn stream_only_newlines() {
    let results = decode_stream("\n\n\n");
    assert!(results.is_empty());
}

#[test]
fn stream_mixed_valid_and_invalid() {
    let e1 = JsonlCodec::encode(&make_hello()).unwrap();
    let input = format!("{e1}not valid json\n");
    let results = decode_stream(&input);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

// ===================================================================
// 9. encode_to_writer / encode_many_to_writer
// ===================================================================

#[test]
fn encode_to_writer_works() {
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &make_hello()).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains(r#""t":"hello""#));
}

#[test]
fn encode_many_to_writer_works() {
    let envs = vec![make_hello(), make_fatal(None, "err")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
}

// ===================================================================
// 10. Malformed JSONL handling
// ===================================================================

#[test]
fn malformed_missing_closing_brace() {
    assert!(JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1""#).is_err());
}

#[test]
fn malformed_trailing_comma() {
    assert!(JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":"boom",}"#).is_err());
}

#[test]
fn malformed_double_colon() {
    assert!(JsonlCodec::decode(r#"{"t"::"fatal"}"#).is_err());
}

#[test]
fn malformed_null_bytes() {
    assert!(JsonlCodec::decode("{\x00}").is_err());
}

#[test]
fn malformed_binary_data() {
    let bytes = [0xFF, 0xFE, 0xFD];
    let s = String::from_utf8_lossy(&bytes);
    assert!(JsonlCodec::decode(&s).is_err());
}

#[test]
fn malformed_array_instead_of_object() {
    assert!(JsonlCodec::decode(r#"[{"t":"hello"}]"#).is_err());
}

#[test]
fn malformed_number_as_input() {
    assert!(JsonlCodec::decode("42").is_err());
}

#[test]
fn malformed_string_as_input() {
    assert!(JsonlCodec::decode(r#""hello""#).is_err());
}

#[test]
fn malformed_boolean_as_input() {
    assert!(JsonlCodec::decode("true").is_err());
}

#[test]
fn malformed_null_as_input() {
    assert!(JsonlCodec::decode("null").is_err());
}

// ===================================================================
// 11. Unicode in envelope content
// ===================================================================

#[test]
fn unicode_in_task() {
    let wo = WorkOrderBuilder::new("修复登录模块的bug").build();
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.task, "修复登录模块的bug");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn unicode_in_error_message() {
    let env = make_fatal(None, "エラーが発生しました");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, "エラーが発生しました");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn unicode_emoji_in_message() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: "✅ All tests passed! 🎉🚀".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "✅ All tests passed! 🎉🚀");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn unicode_rtl_text() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: "مرحبا بالعالم".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "مرحبا بالعالم");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn unicode_in_backend_id() {
    let env = Envelope::hello(make_backend("сайдкар-тест"), BTreeMap::new());
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { backend, .. } = decoded {
        assert_eq!(backend.id, "сайдкар-тест");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn unicode_escaped_in_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"\u0048\u0065\u006c\u006c\u006f"}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, "Hello");
    } else {
        panic!("expected Fatal");
    }
}

// ===================================================================
// 12. Oversized line handling
// ===================================================================

#[test]
fn oversized_line_still_parses() {
    let long_msg = "x".repeat(1_000_000);
    let env = make_fatal(None, &long_msg);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error.len(), 1_000_000);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn validator_warns_on_large_payload() {
    let long_text = "x".repeat(11 * 1024 * 1024);
    let env = make_event("r1", AgentEventKind::AssistantMessage { text: long_text });
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    let has_large_warning = result
        .warnings
        .iter()
        .any(|w| matches!(w, ValidationWarning::LargePayload { .. }));
    assert!(has_large_warning);
}

// ===================================================================
// 13. Handshake protocol validation (hello must be first)
// ===================================================================

#[test]
fn sequence_valid_hello_run_event_final() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_final("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn sequence_missing_hello() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_run("r1"), make_final("r1")];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn sequence_hello_not_first() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_run("r1"), make_hello(), make_final("r1")];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { position: 1 }))
    );
}

#[test]
fn sequence_missing_terminal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_hello(), make_run("r1")];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_multiple_terminals() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_final("r1"),
        make_fatal(Some("r1"), "extra"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[test]
fn sequence_empty() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_hello_run_fatal_is_valid() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(Some("r1"), "crash"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

// ===================================================================
// 14. ref_id correlation
// ===================================================================

#[test]
fn ref_id_match_accepted() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("run-abc"),
        make_event(
            "run-abc",
            AgentEventKind::AssistantDelta { text: "hi".into() },
        ),
        make_final("run-abc"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn ref_id_mismatch_on_event() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("run-abc"),
        make_event(
            "wrong-id",
            AgentEventKind::AssistantDelta { text: "hi".into() },
        ),
        make_final("run-abc"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn ref_id_mismatch_on_final() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_hello(), make_run("run-abc"), make_final("wrong-id")];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn ref_id_mismatch_on_fatal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("run-abc"),
        make_fatal(Some("wrong-id"), "err"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn event_before_run_is_out_of_order() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_event("r1", AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_run("r1"),
        make_final("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

// ===================================================================
// 15. Envelope validation
// ===================================================================

#[test]
fn validate_hello_valid() {
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&make_hello());
    assert!(result.valid);
}

#[test]
fn validate_hello_empty_contract_version() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: String::new(),
        backend: make_backend("x"),
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(
        |e| matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
    ));
}

#[test]
fn validate_hello_invalid_version() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "invalid".into(),
        backend: make_backend("x"),
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn validate_hello_empty_backend_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "backend.id"))
    );
}

#[test]
fn validate_hello_warns_missing_optional_versions() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version")));
    assert!(result.warnings.iter().any(|w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "backend.adapter_version")));
}

#[test]
fn validate_run_empty_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Run {
        id: String::new(),
        work_order: make_work_order(),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_run_empty_task() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_event_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = make_event(
        "",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_final_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Final {
        ref_id: String::new(),
        receipt: make_receipt(),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_empty_error() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: String::new(),
        error_code: None,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_warns_missing_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = make_fatal(None, "boom");
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(
        |w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "ref_id")
    ));
}

// ===================================================================
// 16. Version parsing and compatibility
// ===================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(abp_protocol::parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(abp_protocol::parse_version("invalid"), None);
    assert_eq!(abp_protocol::parse_version("abp/v"), None);
    assert_eq!(abp_protocol::parse_version("abp/v1"), None);
    assert_eq!(abp_protocol::parse_version(""), None);
}

#[test]
fn compatible_versions_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(abp_protocol::is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn incompatible_versions_invalid_format() {
    assert!(!abp_protocol::is_compatible_version("garbage", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "garbage"));
}

// ===================================================================
// 17. Full protocol sequence roundtrip
// ===================================================================

#[test]
fn full_protocol_sequence_via_writer_and_stream() {
    let run_id = "run-full-test";
    let envelopes = vec![
        make_hello(),
        make_run(run_id),
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "starting".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "Hello ".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "World".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::ToolCall {
                tool_name: "write_file".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "out.txt", "content": "hello"}),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::ToolResult {
                tool_name: "write_file".into(),
                tool_use_id: Some("tc-1".into()),
                output: json!({"success": true}),
                is_error: false,
            },
        ),
        make_event(
            run_id,
            AgentEventKind::FileChanged {
                path: "out.txt".into(),
                summary: "Created file".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        make_final(run_id),
    ];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let input = String::from_utf8(buf).unwrap();

    let results: Vec<Envelope> = decode_stream(&input)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), envelopes.len());

    assert!(matches!(results[0], Envelope::Hello { .. }));
    assert!(matches!(results[1], Envelope::Run { .. }));
    for i in 2..9 {
        assert!(matches!(results[i], Envelope::Event { .. }));
    }
    assert!(matches!(results[9], Envelope::Final { .. }));

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&results);
    assert!(errors.is_empty(), "sequence errors: {errors:?}");
}

// ===================================================================
// 18. AgentEventKind uses "type" (not "t") as inner tag
// ===================================================================

#[test]
fn agent_event_kind_uses_type_tag() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "low budget".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"warning""#));
    // The agent event kind uses "type", not "t"
    assert!(!json.contains(r#""t":"warning""#));
}

#[test]
fn agent_event_kind_nested_in_envelope_has_both_tags() {
    let env = make_event(
        "r1",
        AgentEventKind::Warning {
            message: "test".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    // Envelope uses "t"
    assert!(json.contains(r#""t":"event""#));
    // Inner AgentEventKind uses "type"
    assert!(json.contains(r#""type":"warning""#));
}

// ===================================================================
// 19. Edge cases in serde roundtrip
// ===================================================================

#[test]
fn roundtrip_preserves_uuid_format() {
    let env = make_run("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = &env {
        if let Envelope::Run {
            work_order: wo2, ..
        } = decoded
        {
            assert_eq!(work_order.id, wo2.id);
        }
    }
}

#[test]
fn roundtrip_preserves_timestamps() {
    let now = Utc::now();
    let event = AgentEvent {
        ts: now,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event.ts, decoded.ts);
}

#[test]
fn null_ref_id_serialized_as_null() {
    let env = make_fatal(None, "err");
    let json = JsonlCodec::encode(&env).unwrap();
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v["ref_id"].is_null());
}

#[test]
fn empty_trace_serialized_as_empty_array() {
    let receipt = ReceiptBuilder::new("test").build();
    let json = serde_json::to_string(&receipt).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v["trace"].is_array());
    assert_eq!(v["trace"].as_array().unwrap().len(), 0);
}

#[test]
fn receipt_sha256_absent_is_null() {
    let receipt = ReceiptBuilder::new("test").build();
    let json = serde_json::to_string(&receipt).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn receipt_with_hash_fills_sha256() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ===================================================================
// 20. Protocol error types
// ===================================================================

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_error_display() {
    let err = JsonlCodec::decode("{bad").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"));
}

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("test violation".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_has_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

// ===================================================================
// 21. JSON field ordering / determinism (BTreeMap)
// ===================================================================

#[test]
fn capabilities_serialized_deterministically() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Audio, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let env = Envelope::hello(make_backend("sorted"), caps);
    let json1 = JsonlCodec::encode(&env).unwrap();
    let json2 = JsonlCodec::encode(&env).unwrap();
    // BTreeMap ensures deterministic key ordering across serializations
    assert_eq!(json1, json2);
}

// ===================================================================
// 22. Special characters in strings
// ===================================================================

#[test]
fn special_chars_newline_in_error() {
    let env = make_fatal(None, "line1\nline2\nline3");
    let json = JsonlCodec::encode(&env).unwrap();
    // The encoded newlines should be escaped, keeping it single-line JSONL
    let line_count = json.trim_end_matches('\n').matches('\n').count();
    assert_eq!(line_count, 0, "encoded JSON must be single line");
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, "line1\nline2\nline3");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn special_chars_tab_in_message() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: "col1\tcol2\tcol3".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "col1\tcol2\tcol3");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn special_chars_quotes_in_tool_input() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"cmd": "echo \"hello world\""}),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall { input, .. } = &event.kind {
            assert_eq!(input["cmd"], "echo \"hello world\"");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn special_chars_backslash_in_path() {
    let env = make_event(
        "r1",
        AgentEventKind::FileChanged {
            path: r"C:\Users\test\file.rs".into(),
            summary: "modified".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::FileChanged { path, .. } = &event.kind {
            assert_eq!(path, r"C:\Users\test\file.rs");
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

// ===================================================================
// 23. Decode from raw JSON strings (hand-crafted)
// ===================================================================

#[test]
fn decode_raw_hello_json() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"node-sidecar","backend_version":"2.0","adapter_version":"0.5"},"capabilities":{"streaming":"native","tool_read":"emulated"}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello {
        backend,
        capabilities,
        ..
    } = env
    {
        assert_eq!(backend.id, "node-sidecar");
        assert_eq!(capabilities.len(), 2);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn decode_raw_fatal_json() {
    let json = r#"{"t":"fatal","ref_id":"run-42","error":"process crashed","error_code":"protocol_invalid_envelope"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = env
    {
        assert_eq!(ref_id.as_deref(), Some("run-42"));
        assert_eq!(error, "process crashed");
        assert!(error_code.is_some());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn decode_raw_fatal_no_error_code_field() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal { error_code, .. } = env {
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

// ===================================================================
// 24. Envelope builder method (Envelope::hello)
// ===================================================================

#[test]
fn envelope_hello_builder_sets_contract_version() {
    let env = Envelope::hello(make_backend("x"), BTreeMap::new());
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn envelope_hello_with_mode_builder() {
    let env = Envelope::hello_with_mode(
        make_backend("x"),
        BTreeMap::new(),
        ExecutionMode::Passthrough,
    );
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn envelope_fatal_with_code_builder() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "test error",
        abp_error::ErrorCode::ProtocolHandshakeFailed,
    );
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = env
    {
        assert_eq!(ref_id.as_deref(), Some("r1"));
        assert_eq!(error, "test error");
        assert!(error_code.is_some());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn envelope_fatal_from_abp_error_builder() {
    let abp_err = abp_error::AbpError {
        code: abp_error::ErrorCode::ProtocolInvalidEnvelope,
        message: "bad envelope".into(),
        source: None,
        context: BTreeMap::new(),
        location: None,
    };
    let env = Envelope::fatal_from_abp_error(Some("r1".into()), &abp_err);
    if let Envelope::Fatal {
        error, error_code, ..
    } = env
    {
        assert_eq!(error, "bad envelope");
        assert_eq!(
            error_code,
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    } else {
        panic!("expected Fatal");
    }
}

// ===================================================================
// 25. Edge: multiple events in stream with interleaved empty lines
// ===================================================================

#[test]
fn stream_interleaved_empty_lines_between_all_envelopes() {
    let h = JsonlCodec::encode(&make_hello()).unwrap();
    let r = JsonlCodec::encode(&make_run("r1")).unwrap();
    let e = JsonlCodec::encode(&make_event(
        "r1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    ))
    .unwrap();
    let f = JsonlCodec::encode(&make_final("r1")).unwrap();
    let input = format!("\n{h}\n\n{r}\n\n\n{e}\n{f}\n\n");
    let results: Vec<Envelope> = decode_stream(&input)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 4);
}

// ===================================================================
// 26. Decode trimming behavior
// ===================================================================

#[test]
fn decode_with_leading_whitespace() {
    let json = r#"  {"t":"fatal","ref_id":null,"error":"boom"}  "#;
    // decode_stream trims, but direct decode may not
    let reader = BufReader::new(json.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn decode_with_trailing_whitespace() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}   "#;
    let reader = BufReader::new(json.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

// ===================================================================
// 27. Contract version constant
// ===================================================================

#[test]
fn contract_version_is_abp_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_parseable() {
    let parsed = abp_protocol::parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}

#[test]
fn contract_version_compatible_with_self() {
    assert!(abp_protocol::is_compatible_version(
        CONTRACT_VERSION,
        CONTRACT_VERSION
    ));
}
