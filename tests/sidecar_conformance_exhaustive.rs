#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive sidecar conformance tests validating JSONL protocol adherence
//! across all sidecar backends.
//!
//! Covers: hello envelope, run delivery, event streaming, final/fatal envelopes,
//! ref_id correlation, version negotiation, capability advertisement, timeout
//! handling, malformed input, concurrent sessions, SidecarSpec/registry, and more.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    Receipt, ReceiptBuilder, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{compute_delay, is_retryable, RetryConfig};
use abp_host::{HostError, SidecarSpec};
use abp_protocol::batch::{
    BatchItemStatus, BatchProcessor, BatchRequest, BatchValidationError, MAX_BATCH_SIZE,
};
use abp_protocol::builder::{BuilderError, EnvelopeBuilder};
use abp_protocol::codec::StreamingCodec;
use abp_protocol::compress::{CompressionAlgorithm, CompressionStats, MessageCompressor};
use abp_protocol::router::{MessageRoute, MessageRouter, RouteTable};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.to_string(),
        backend_version: Some("1.0.0".to_string()),
        adapter_version: Some("0.1.0".to_string()),
    }
}

fn make_backend_minimal(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.to_string(),
        backend_version: None,
        adapter_version: None,
    }
}

fn make_hello(backend_id: &str) -> Envelope {
    Envelope::hello(make_backend(backend_id), CapabilityManifest::default())
}

fn make_hello_with_version(backend_id: &str, version: &str) -> Envelope {
    Envelope::Hello {
        contract_version: version.to_string(),
        backend: make_backend(backend_id),
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::default(),
    }
}

fn make_run(id: &str) -> Envelope {
    Envelope::Run {
        id: id.to_string(),
        work_order: make_work_order("test task"),
    }
}

fn make_event(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.to_string(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.to_string(),
        receipt: make_receipt(),
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.to_string(),
        error_code: None,
    }
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build()
}

fn valid_protocol_sequence(run_id: &str) -> Vec<Envelope> {
    vec![
        make_hello("test-sidecar"),
        make_run(run_id),
        make_event(run_id, "thinking..."),
        make_event(run_id, "done"),
        make_final(run_id),
    ]
}

// =========================================================================
// 1. Hello Envelope Requirements
// =========================================================================

mod hello_envelope {
    use super::*;

    #[test]
    fn hello_must_contain_contract_version() {
        let env = make_hello("test");
        match &env {
            Envelope::Hello {
                contract_version, ..
            } => {
                assert_eq!(contract_version, CONTRACT_VERSION);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_must_contain_backend_id() {
        let env = make_hello("my-sidecar");
        match &env {
            Envelope::Hello { backend, .. } => {
                assert_eq!(backend.id, "my-sidecar");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_serializes_with_t_tag() {
        let env = make_hello("test");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"hello""#));
    }

    #[test]
    fn hello_round_trip() {
        let env = make_hello("roundtrip");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, "roundtrip"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_with_mode_passthrough() {
        let env = Envelope::hello_with_mode(
            make_backend("sidecar"),
            CapabilityManifest::default(),
            ExecutionMode::Passthrough,
        );
        match env {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_default_mode_is_mapped() {
        let env = make_hello("test");
        match env {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_missing_backend_version_warns() {
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: make_backend_minimal("test"),
            capabilities: CapabilityManifest::default(),
            mode: ExecutionMode::default(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(result.valid);
        let has_backend_version_warning = result.warnings.iter().any(|w| {
            matches!(w, ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version")
        });
        assert!(has_backend_version_warning);
    }

    #[test]
    fn hello_missing_adapter_version_warns() {
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: make_backend_minimal("test"),
            capabilities: CapabilityManifest::default(),
            mode: ExecutionMode::default(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        let has_adapter_warning = result.warnings.iter().any(|w| {
            matches!(w, ValidationWarning::MissingOptionalField { field } if field == "backend.adapter_version")
        });
        assert!(has_adapter_warning);
    }

    #[test]
    fn hello_empty_contract_version_invalid() {
        let env = Envelope::Hello {
            contract_version: String::new(),
            backend: make_backend("test"),
            capabilities: CapabilityManifest::default(),
            mode: ExecutionMode::default(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| {
            matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
        }));
    }

    #[test]
    fn hello_invalid_contract_version_format() {
        let env = Envelope::Hello {
            contract_version: "invalid-version".to_string(),
            backend: make_backend("test"),
            capabilities: CapabilityManifest::default(),
            mode: ExecutionMode::default(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
    }

    #[test]
    fn hello_empty_backend_id_invalid() {
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: BackendIdentity {
                id: String::new(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::default(),
            mode: ExecutionMode::default(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| {
            matches!(e, ValidationError::EmptyField { field } if field == "backend.id")
        }));
    }

    #[test]
    fn hello_builder_minimal() {
        let env = EnvelopeBuilder::hello()
            .backend("built-sidecar")
            .build()
            .unwrap();
        match env {
            Envelope::Hello {
                contract_version,
                backend,
                ..
            } => {
                assert_eq!(contract_version, CONTRACT_VERSION);
                assert_eq!(backend.id, "built-sidecar");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_builder_missing_backend_fails() {
        let err = EnvelopeBuilder::hello().build().unwrap_err();
        assert_eq!(err, BuilderError::MissingField("backend"));
    }

    #[test]
    fn hello_builder_all_fields() {
        let env = EnvelopeBuilder::hello()
            .backend("sidecar")
            .version("2.0")
            .adapter_version("1.0")
            .mode(ExecutionMode::Passthrough)
            .capabilities(CapabilityManifest::new())
            .build()
            .unwrap();
        match env {
            Envelope::Hello {
                backend,
                mode,
                contract_version,
                ..
            } => {
                assert_eq!(backend.id, "sidecar");
                assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
                assert_eq!(backend.adapter_version.as_deref(), Some("1.0"));
                assert_eq!(mode, ExecutionMode::Passthrough);
                assert_eq!(contract_version, CONTRACT_VERSION);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_must_be_first_in_sequence() {
        let validator = EnvelopeValidator::new();
        let sequence = vec![make_run("run-1"), make_hello("test"), make_final("run-1")];
        let errors = validator.validate_sequence(&sequence);
        assert!(errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { position: 1 })));
    }

    #[test]
    fn hello_missing_from_sequence() {
        let validator = EnvelopeValidator::new();
        let sequence = vec![make_run("run-1"), make_final("run-1")];
        let errors = validator.validate_sequence(&sequence);
        assert!(errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingHello)));
    }
}

// =========================================================================
// 2. Run Envelope Delivery
// =========================================================================

mod run_envelope {
    use super::*;

    #[test]
    fn run_serializes_with_t_tag() {
        let env = make_run("run-123");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"run""#));
    }

    #[test]
    fn run_round_trip() {
        let env = make_run("run-456");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Envelope::Run { id, work_order } => {
                assert_eq!(id, "run-456");
                assert!(!work_order.task.is_empty());
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_contains_work_order() {
        let env = make_run("test-run");
        match &env {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.task, "test task");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_empty_id_invalid() {
        let env = Envelope::Run {
            id: String::new(),
            work_order: make_work_order("task"),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "id")));
    }

    #[test]
    fn run_empty_task_invalid() {
        let env = Envelope::Run {
            id: "run-1".to_string(),
            work_order: make_work_order(""),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn run_builder_uses_work_order_id() {
        let wo = make_work_order("build test");
        let wo_id = wo.id.to_string();
        let env = EnvelopeBuilder::run(wo).build().unwrap();
        match env {
            Envelope::Run { id, .. } => assert_eq!(id, wo_id),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_builder_ref_id_override() {
        let wo = make_work_order("build test");
        let env = EnvelopeBuilder::run(wo)
            .ref_id("custom-id")
            .build()
            .unwrap();
        match env {
            Envelope::Run { id, .. } => assert_eq!(id, "custom-id"),
            _ => panic!("expected Run"),
        }
    }
}

// =========================================================================
// 3. Event Envelope Streaming
// =========================================================================

mod event_envelope {
    use super::*;

    #[test]
    fn event_serializes_with_t_tag() {
        let env = make_event("run-1", "hello");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"event""#));
    }

    #[test]
    fn event_round_trip() {
        let env = make_event("run-1", "test message");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Envelope::Event { ref_id, event } => {
                assert_eq!(ref_id, "run-1");
                match event.kind {
                    AgentEventKind::AssistantMessage { ref text } => {
                        assert_eq!(text, "test message")
                    }
                    _ => panic!("unexpected event kind"),
                }
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_empty_ref_id_invalid() {
        let env = Envelope::Event {
            ref_id: String::new(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "hi".to_string(),
                },
                ext: None,
            },
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| { matches!(e, ValidationError::EmptyField { field } if field == "ref_id") }));
    }

    #[test]
    fn event_builder_requires_ref_id() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".to_string(),
            },
            ext: None,
        };
        let err = EnvelopeBuilder::event(event).build().unwrap_err();
        assert_eq!(err, BuilderError::MissingField("ref_id"));
    }

    #[test]
    fn event_builder_with_ref_id() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".to_string(),
            },
            ext: None,
        };
        let env = EnvelopeBuilder::event(event)
            .ref_id("run-99")
            .build()
            .unwrap();
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-99"),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn multiple_events_in_sequence() {
        let run_id = "run-multi";
        let sequence = vec![
            make_hello("test"),
            make_run(run_id),
            make_event(run_id, "step 1"),
            make_event(run_id, "step 2"),
            make_event(run_id, "step 3"),
            make_final(run_id),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn event_with_tool_call_kind() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".to_string(),
                tool_use_id: Some("tc-1".to_string()),
                parent_tool_use_id: None,
                input: json!({"command": "ls"}),
            },
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "run-1".to_string(),
            event,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match event.kind {
                AgentEventKind::ToolCall { tool_name, .. } => {
                    assert_eq!(tool_name, "bash");
                }
                _ => panic!("expected ToolCall"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_tool_result_kind() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".to_string(),
                tool_use_id: Some("tc-1".to_string()),
                output: json!({"stdout": "hello"}),
                is_error: false,
            },
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "run-1".to_string(),
            event,
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Event { .. }));
    }

    #[test]
    fn event_with_file_changed_kind() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".to_string(),
                summary: "Added function".to_string(),
            },
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "run-1".to_string(),
            event,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains("file_changed"));
    }

    #[test]
    fn event_with_warning_kind() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "rate limited".to_string(),
            },
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "run-1".to_string(),
            event,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains("warning"));
    }

    #[test]
    fn event_with_error_kind() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something failed".to_string(),
                error_code: None,
            },
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "run-1".to_string(),
            event,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""type":"error""#));
    }

    #[test]
    fn event_with_command_executed_kind() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".to_string(),
                exit_code: Some(0),
                output_preview: Some("ok".to_string()),
            },
            ext: None,
        };
        let env = Envelope::Event {
            ref_id: "run-1".to_string(),
            event,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains("command_executed"));
    }

    #[test]
    fn event_with_extension_data() {
        let mut ext = BTreeMap::new();
        ext.insert("custom_field".to_string(), json!("custom_value"));
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hi".to_string(),
            },
            ext: Some(ext),
        };
        let env = Envelope::Event {
            ref_id: "run-1".to_string(),
            event,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains("custom_field"));
    }
}

// =========================================================================
// 4. Final Envelope (Receipt)
// =========================================================================

mod final_envelope {
    use super::*;

    #[test]
    fn final_serializes_with_t_tag() {
        let env = make_final("run-1");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"final""#));
    }

    #[test]
    fn final_round_trip() {
        let env = make_final("run-1");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "run-1"),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_contains_receipt() {
        let env = make_final("run-1");
        match &env {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.backend.id, "test-backend");
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_empty_ref_id_invalid() {
        let env = Envelope::Final {
            ref_id: String::new(),
            receipt: make_receipt(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| { matches!(e, ValidationError::EmptyField { field } if field == "ref_id") }));
    }

    #[test]
    fn final_builder_requires_ref_id() {
        let err = EnvelopeBuilder::final_receipt(make_receipt())
            .build()
            .unwrap_err();
        assert_eq!(err, BuilderError::MissingField("ref_id"));
    }

    #[test]
    fn final_builder_with_ref_id() {
        let env = EnvelopeBuilder::final_receipt(make_receipt())
            .ref_id("run-1")
            .build()
            .unwrap();
        match env {
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "run-1"),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn final_is_terminal_in_sequence() {
        let run_id = "run-terminal";
        let sequence = vec![
            make_hello("test"),
            make_run(run_id),
            make_event(run_id, "work"),
            make_final(run_id),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn multiple_terminals_rejected() {
        let run_id = "run-multi-term";
        let sequence = vec![
            make_hello("test"),
            make_run(run_id),
            make_final(run_id),
            make_final(run_id),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors
            .iter()
            .any(|e| matches!(e, SequenceError::MultipleTerminals)));
    }
}

// =========================================================================
// 5. Fatal Envelope Error Handling
// =========================================================================

mod fatal_envelope {
    use super::*;

    #[test]
    fn fatal_serializes_with_t_tag() {
        let env = make_fatal(Some("run-1"), "boom");
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(json.contains(r#""t":"fatal""#));
    }

    #[test]
    fn fatal_round_trip() {
        let env = make_fatal(Some("run-1"), "crash");
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id, Some("run-1".to_string()));
                assert_eq!(error, "crash");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_ref_id_optional() {
        let env = make_fatal(None, "unknown crash");
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id, None),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_missing_ref_id_warns() {
        let env = make_fatal(None, "error");
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(result.valid); // Fatal without ref_id is valid, but warned
        assert!(result.warnings.iter().any(|w| {
            matches!(w, ValidationWarning::MissingOptionalField { field } if field == "ref_id")
        }));
    }

    #[test]
    fn fatal_empty_error_invalid() {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".to_string()),
            error: String::new(),
            error_code: None,
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| { matches!(e, ValidationError::EmptyField { field } if field == "error") }));
    }

    #[test]
    fn fatal_with_error_code() {
        let env = Envelope::fatal_with_code(
            Some("run-1".to_string()),
            "handshake failed",
            abp_error::ErrorCode::ProtocolHandshakeFailed,
        );
        assert_eq!(
            env.error_code(),
            Some(abp_error::ErrorCode::ProtocolHandshakeFailed)
        );
    }

    #[test]
    fn fatal_is_terminal_in_sequence() {
        let run_id = "run-fatal";
        let sequence = vec![
            make_hello("test"),
            make_run(run_id),
            make_fatal(Some(run_id), "oops"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn fatal_builder_without_ref_id() {
        let env = EnvelopeBuilder::fatal("something broke").build().unwrap();
        match env {
            Envelope::Fatal {
                ref_id,
                error,
                error_code,
            } => {
                assert_eq!(ref_id, None);
                assert_eq!(error, "something broke");
                assert_eq!(error_code, None);
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_builder_with_ref_id() {
        let env = EnvelopeBuilder::fatal("broke")
            .ref_id("run-1")
            .build()
            .unwrap();
        match env {
            Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id, Some("run-1".to_string())),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_and_final_both_terminal() {
        let run_id = "run-double";
        let sequence = vec![
            make_hello("test"),
            make_run(run_id),
            make_final(run_id),
            make_fatal(Some(run_id), "late error"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors
            .iter()
            .any(|e| matches!(e, SequenceError::MultipleTerminals)));
    }
}

// =========================================================================
// 6. Ref-ID Correlation Rules
// =========================================================================

mod ref_id_correlation {
    use super::*;

    #[test]
    fn event_ref_id_must_match_run_id() {
        let sequence = vec![
            make_hello("test"),
            make_run("run-1"),
            make_event("run-WRONG", "data"),
            make_final("run-1"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.iter().any(|e| matches!(
            e,
            SequenceError::RefIdMismatch {
                expected,
                found
            } if expected == "run-1" && found == "run-WRONG"
        )));
    }

    #[test]
    fn final_ref_id_must_match_run_id() {
        let sequence = vec![
            make_hello("test"),
            make_run("run-1"),
            make_final("run-OTHER"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
    }

    #[test]
    fn fatal_ref_id_must_match_run_id() {
        let sequence = vec![
            make_hello("test"),
            make_run("run-1"),
            make_fatal(Some("run-BAD"), "error"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
    }

    #[test]
    fn correct_ref_ids_no_errors() {
        let run_id = "run-correct";
        let sequence = valid_protocol_sequence(run_id);
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.is_empty());
    }

    #[test]
    fn fatal_with_none_ref_id_no_mismatch() {
        let sequence = vec![
            make_hello("test"),
            make_run("run-1"),
            make_fatal(None, "unknown"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        // Fatal with None ref_id should not cause a RefIdMismatch
        assert!(!errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
    }

    #[test]
    fn event_before_run_is_out_of_order() {
        let sequence = vec![
            make_hello("test"),
            make_event("run-1", "early"),
            make_run("run-1"),
            make_final("run-1"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors
            .iter()
            .any(|e| matches!(e, SequenceError::OutOfOrderEvents)));
    }

    #[test]
    fn event_after_terminal_is_out_of_order() {
        let run_id = "run-late";
        let sequence = vec![
            make_hello("test"),
            make_run(run_id),
            make_final(run_id),
            make_event(run_id, "too late"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        // Multiple terminals or out of order
        assert!(!errors.is_empty());
    }
}

// =========================================================================
// 7. Protocol Version Negotiation
// =========================================================================

mod version_negotiation {
    use super::*;

    #[test]
    fn parse_valid_version() {
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
        assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    }

    #[test]
    fn parse_invalid_version() {
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version("v0.1"), None);
        assert_eq!(parse_version("abp/0.1"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compatible_versions_same_major() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
        assert!(is_compatible_version("abp/v1.0", "abp/v1.5"));
    }

    #[test]
    fn incompatible_versions_different_major() {
        assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
        assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
    }

    #[test]
    fn incompatible_if_unparseable() {
        assert!(!is_compatible_version("garbage", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "garbage"));
    }

    #[test]
    fn protocol_version_parse() {
        let v = ProtocolVersion::parse("abp/v0.1").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
    }

    #[test]
    fn protocol_version_parse_invalid() {
        assert!(matches!(
            ProtocolVersion::parse("bad"),
            Err(VersionError::InvalidFormat)
        ));
    }

    #[test]
    fn protocol_version_to_string() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        assert_eq!(v.to_string(), "abp/v0.1");
    }

    #[test]
    fn protocol_version_display() {
        let v = ProtocolVersion { major: 2, minor: 3 };
        assert_eq!(format!("{v}"), "abp/v2.3");
    }

    #[test]
    fn protocol_version_current_matches_contract() {
        let current = ProtocolVersion::current();
        assert_eq!(current.to_string(), CONTRACT_VERSION);
    }

    #[test]
    fn protocol_version_compatibility() {
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        // v01 is compatible with v02 if v02.minor >= v01.minor
        assert!(v01.is_compatible(&v02));
        // v02 is NOT compatible with v01 (v01.minor < v02.minor)
        assert!(!v02.is_compatible(&v01));
    }

    #[test]
    fn negotiate_compatible() {
        let local = ProtocolVersion { major: 0, minor: 1 };
        let remote = ProtocolVersion { major: 0, minor: 2 };
        let result = negotiate_version(&local, &remote).unwrap();
        assert_eq!(result, ProtocolVersion { major: 0, minor: 1 });
    }

    #[test]
    fn negotiate_incompatible() {
        let local = ProtocolVersion { major: 0, minor: 1 };
        let remote = ProtocolVersion { major: 1, minor: 0 };
        let err = negotiate_version(&local, &remote).unwrap_err();
        assert!(matches!(err, VersionError::Incompatible { .. }));
    }

    #[test]
    fn version_range_contains() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 3 },
        };
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    }

    #[test]
    fn version_range_is_compatible() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 3 },
        };
        assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
        assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
    }

    #[test]
    fn version_error_invalid_major() {
        let err = ProtocolVersion::parse("abp/vX.1").unwrap_err();
        assert!(matches!(err, VersionError::InvalidMajor));
    }

    #[test]
    fn version_error_invalid_minor() {
        let err = ProtocolVersion::parse("abp/v0.X").unwrap_err();
        assert!(matches!(err, VersionError::InvalidMinor));
    }
}

// =========================================================================
// 8. Capability Advertisement in Hello
// =========================================================================

mod capability_advertisement {
    use super::*;

    #[test]
    fn hello_with_empty_capabilities() {
        let env = Envelope::hello(make_backend("test"), CapabilityManifest::default());
        match env {
            Envelope::Hello { capabilities, .. } => {
                assert!(capabilities.is_empty());
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_capabilities_round_trip() {
        let caps = CapabilityManifest::new();
        let env = Envelope::hello(make_backend("test"), caps);
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Envelope::Hello { capabilities, .. } => {
                assert!(capabilities.is_empty());
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_capabilities_in_hello_builder() {
        let env = EnvelopeBuilder::hello()
            .backend("caps-sidecar")
            .capabilities(CapabilityManifest::new())
            .build()
            .unwrap();
        assert!(matches!(env, Envelope::Hello { .. }));
    }
}

// =========================================================================
// 9. Timeout Handling
// =========================================================================

mod timeout_handling {
    use super::*;

    #[test]
    fn host_error_timeout_has_duration() {
        let err = HostError::Timeout {
            duration: Duration::from_secs(30),
        };
        let msg = err.to_string();
        assert!(msg.contains("30"));
    }

    #[test]
    fn timeout_is_retryable() {
        let err = HostError::Timeout {
            duration: Duration::from_secs(10),
        };
        assert!(is_retryable(&err));
    }

    #[test]
    fn retry_config_default() {
        let cfg = RetryConfig::default();
        assert_eq!(cfg.max_retries, 3);
        assert_eq!(cfg.base_delay, Duration::from_millis(100));
        assert_eq!(cfg.max_delay, Duration::from_secs(10));
        assert_eq!(cfg.overall_timeout, Duration::from_secs(60));
    }

    #[test]
    fn compute_delay_exponential() {
        let cfg = RetryConfig {
            max_retries: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(60),
            overall_timeout: Duration::from_secs(300),
            jitter_factor: 0.0,
        };
        let d0 = compute_delay(&cfg, 0);
        let d1 = compute_delay(&cfg, 1);
        let d2 = compute_delay(&cfg, 2);
        assert_eq!(d0, Duration::from_millis(100));
        assert_eq!(d1, Duration::from_millis(200));
        assert_eq!(d2, Duration::from_millis(400));
    }

    #[test]
    fn compute_delay_capped_at_max() {
        let cfg = RetryConfig {
            max_retries: 10,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(500),
            overall_timeout: Duration::from_secs(60),
            jitter_factor: 0.0,
        };
        let d10 = compute_delay(&cfg, 10);
        assert_eq!(d10, Duration::from_millis(500));
    }

    #[test]
    fn compute_delay_with_jitter_less_than_no_jitter() {
        let cfg = RetryConfig {
            max_retries: 3,
            base_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(60),
            overall_timeout: Duration::from_secs(300),
            jitter_factor: 0.5,
        };
        let delay = compute_delay(&cfg, 2);
        // With jitter, delay should be <= 4000ms (nominal) and >= 2000ms (50% jitter)
        assert!(delay <= Duration::from_millis(4000));
        assert!(delay >= Duration::from_millis(2000));
    }

    #[test]
    fn retryable_errors() {
        assert!(is_retryable(&HostError::Spawn(std::io::Error::new(
            std::io::ErrorKind::Other,
            "spawn"
        ))));
        assert!(is_retryable(&HostError::Stdout(std::io::Error::new(
            std::io::ErrorKind::Other,
            "io"
        ))));
        assert!(is_retryable(&HostError::Exited { code: Some(1) }));
        assert!(is_retryable(&HostError::SidecarCrashed {
            exit_code: Some(1),
            stderr: "err".to_string(),
        }));
        assert!(is_retryable(&HostError::Timeout {
            duration: Duration::from_secs(1),
        }));
    }

    #[test]
    fn non_retryable_errors() {
        assert!(!is_retryable(&HostError::Violation("bad".to_string())));
        assert!(!is_retryable(&HostError::Fatal("fatal".to_string())));
        assert!(!is_retryable(&HostError::Protocol(
            ProtocolError::Violation("bad".to_string())
        )));
    }
}

// =========================================================================
// 10. Malformed Input Handling
// =========================================================================

mod malformed_input {
    use super::*;

    #[test]
    fn decode_empty_string() {
        let result = JsonlCodec::decode("");
        assert!(result.is_err());
    }

    #[test]
    fn decode_invalid_json() {
        let result = JsonlCodec::decode("not valid json at all");
        assert!(matches!(result, Err(ProtocolError::Json(_))));
    }

    #[test]
    fn decode_valid_json_wrong_schema() {
        let result = JsonlCodec::decode(r#"{"name":"test"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn decode_missing_t_field() {
        let result = JsonlCodec::decode(r#"{"ref_id":"run-1","error":"boom"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn decode_unknown_t_value() {
        let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":"test"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n\n";
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn stream_parser_partial_line() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
        let (first, second) = line.as_bytes().split_at(10);

        let r1 = parser.feed(first);
        assert!(r1.is_empty());

        let r2 = parser.feed(second);
        assert_eq!(r2.len(), 1);
        assert!(r2[0].is_ok());
    }

    #[test]
    fn stream_parser_multiple_lines_in_one_chunk() {
        let mut parser = StreamParser::new();
        let line1 = JsonlCodec::encode(&make_fatal(None, "err1")).unwrap();
        let line2 = JsonlCodec::encode(&make_fatal(None, "err2")).unwrap();
        let combined = format!("{line1}{line2}");

        let results = parser.feed(combined.as_bytes());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn stream_parser_max_line_len() {
        let mut parser = StreamParser::with_max_line_len(50);
        let long_line = format!(
            "{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{}\"}}\n",
            "x".repeat(100)
        );
        let results = parser.feed(long_line.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn stream_parser_finish_flushes() {
        let mut parser = StreamParser::new();
        let line = r#"{"t":"fatal","ref_id":null,"error":"unterminated"}"#;
        parser.feed(line.as_bytes());
        assert!(!parser.is_empty());

        let results = parser.finish();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        assert!(parser.is_empty());
    }

    #[test]
    fn stream_parser_reset() {
        let mut parser = StreamParser::new();
        parser.feed(b"partial data");
        assert!(!parser.is_empty());
        parser.reset();
        assert!(parser.is_empty());
    }

    #[test]
    fn stream_parser_invalid_utf8() {
        let mut parser = StreamParser::new();
        let invalid = b"\xff\xfe\n";
        let results = parser.feed(invalid);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn stream_parser_blank_lines_skipped() {
        let mut parser = StreamParser::new();
        let results = parser.feed(b"\n\n\n");
        assert!(results.is_empty());
    }

    #[test]
    fn streaming_codec_validate_jsonl() {
        let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\nnot json\n";
        let errors = StreamingCodec::validate_jsonl(input);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, 2); // line 2 is invalid
    }

    #[test]
    fn streaming_codec_line_count() {
        let input = "line1\n\nline2\nline3\n";
        assert_eq!(StreamingCodec::line_count(input), 3);
    }

    #[test]
    fn streaming_codec_encode_decode_batch() {
        let envelopes = vec![make_fatal(None, "err1"), make_fatal(Some("r1"), "err2")];
        let encoded = StreamingCodec::encode_batch(&envelopes);
        let decoded = StreamingCodec::decode_batch(&encoded);
        assert_eq!(decoded.len(), 2);
        assert!(decoded.iter().all(|r| r.is_ok()));
    }
}

// =========================================================================
// 11. Concurrent Sidecar Sessions (Theoretical)
// =========================================================================

mod concurrent_sessions {
    use super::*;

    #[test]
    fn pool_add_and_acquire() {
        let pool = SidecarPool::new(PoolConfig::default());
        assert!(pool.add("s1"));
        let entry = pool.acquire().unwrap();
        assert_eq!(entry.state, PoolEntryState::Busy);
        assert_eq!(entry.id, "s1");
    }

    #[test]
    fn pool_release_makes_idle() {
        let pool = SidecarPool::new(PoolConfig::default());
        pool.add("s1");
        let entry = pool.acquire().unwrap();
        pool.release(&entry.id);
        assert_eq!(pool.idle_count(), 1);
    }

    #[test]
    fn pool_max_size_enforced() {
        let config = PoolConfig {
            min_size: 1,
            max_size: 2,
            ..PoolConfig::default()
        };
        let pool = SidecarPool::new(config);
        assert!(pool.add("s1"));
        assert!(pool.add("s2"));
        assert!(!pool.add("s3"));
    }

    #[test]
    fn pool_acquire_none_when_empty() {
        let pool = SidecarPool::new(PoolConfig::default());
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn pool_acquire_none_when_all_busy() {
        let pool = SidecarPool::new(PoolConfig::default());
        pool.add("s1");
        pool.acquire().unwrap();
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn pool_mark_failed() {
        let pool = SidecarPool::new(PoolConfig::default());
        pool.add("s1");
        pool.mark_failed("s1");
        // Failed entry is not acquirable
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn pool_drain_entry() {
        let pool = SidecarPool::new(PoolConfig::default());
        pool.add("s1");
        pool.drain("s1");
        // Draining entry is not acquirable
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn pool_remove_entry() {
        let pool = SidecarPool::new(PoolConfig::default());
        pool.add("s1");
        let removed = pool.remove("s1");
        assert!(removed.is_some());
        assert_eq!(pool.total_count(), 0);
    }

    #[test]
    fn pool_stats_accurate() {
        let config = PoolConfig {
            min_size: 1,
            max_size: 10,
            ..PoolConfig::default()
        };
        let pool = SidecarPool::new(config);
        pool.add("s1");
        pool.add("s2");
        pool.add("s3");
        pool.acquire().unwrap(); // s1 becomes busy
        pool.mark_failed("s3");

        let stats = pool.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.idle, 1);
        assert_eq!(stats.busy, 1);
        assert_eq!(stats.failed, 1);
    }

    #[test]
    fn pool_utilization() {
        let config = PoolConfig {
            min_size: 1,
            max_size: 10,
            ..PoolConfig::default()
        };
        let pool = SidecarPool::new(config);
        pool.add("s1");
        pool.add("s2");
        pool.acquire().unwrap();

        let stats = pool.stats();
        assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn pool_multiple_acquire_release() {
        let config = PoolConfig {
            min_size: 1,
            max_size: 4,
            ..PoolConfig::default()
        };
        let pool = SidecarPool::new(config);
        pool.add("s1");
        pool.add("s2");

        let e1 = pool.acquire().unwrap();
        let e2 = pool.acquire().unwrap();
        assert!(pool.acquire().is_none());

        pool.release(&e1.id);
        let e3 = pool.acquire().unwrap();
        assert_eq!(e3.id, e1.id);

        pool.release(&e2.id);
        pool.release(&e3.id);
        assert_eq!(pool.idle_count(), 2);
    }
}

// =========================================================================
// 12. SidecarSpec Construction and Validation
// =========================================================================

mod sidecar_spec {
    use super::*;

    #[test]
    fn spec_new_defaults() {
        let spec = SidecarSpec::new("node");
        assert_eq!(spec.command, "node");
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn spec_with_args() {
        let mut spec = SidecarSpec::new("python");
        spec.args = vec!["host.py".to_string()];
        assert_eq!(spec.args.len(), 1);
    }

    #[test]
    fn spec_with_env() {
        let mut spec = SidecarSpec::new("node");
        spec.env
            .insert("NODE_ENV".to_string(), "production".to_string());
        assert_eq!(spec.env.get("NODE_ENV").unwrap(), "production");
    }

    #[test]
    fn spec_with_cwd() {
        let mut spec = SidecarSpec::new("node");
        spec.cwd = Some("/tmp/work".to_string());
        assert_eq!(spec.cwd.as_deref(), Some("/tmp/work"));
    }

    #[test]
    fn spec_serialization() {
        let spec = SidecarSpec::new("node");
        let json = serde_json::to_string(&spec).unwrap();
        let decoded: SidecarSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, "node");
    }

    #[test]
    fn sidecar_config_new() {
        let config = SidecarConfig::new("test-sidecar", "node");
        assert_eq!(config.name, "test-sidecar");
        assert_eq!(config.command, "node");
        assert!(config.args.is_empty());
    }

    #[test]
    fn sidecar_config_validate_ok() {
        let config = SidecarConfig::new("test", "node");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn sidecar_config_validate_empty_name() {
        let config = SidecarConfig::new("", "node");
        assert!(config.validate().is_err());
    }

    #[test]
    fn sidecar_config_validate_empty_command() {
        let config = SidecarConfig::new("test", "");
        assert!(config.validate().is_err());
    }

    #[test]
    fn sidecar_config_to_spec() {
        let mut config = SidecarConfig::new("test", "node");
        config.args = vec!["host.js".to_string()];
        config.env.insert("KEY".to_string(), "VALUE".to_string());
        let spec = config.to_spec();
        assert_eq!(spec.command, "node");
        assert_eq!(spec.args, vec!["host.js"]);
        assert_eq!(spec.env.get("KEY").unwrap(), "VALUE");
    }
}

// =========================================================================
// 13. Registered Sidecar Backends
// =========================================================================

mod registered_backends {
    use super::*;

    #[test]
    fn registry_register_and_get() {
        let mut registry = SidecarRegistry::default();
        let config = SidecarConfig::new("node-sidecar", "node");
        registry.register(config).unwrap();
        assert!(registry.get("node-sidecar").is_some());
    }

    #[test]
    fn registry_duplicate_name_rejected() {
        let mut registry = SidecarRegistry::default();
        registry
            .register(SidecarConfig::new("test", "node"))
            .unwrap();
        let err = registry.register(SidecarConfig::new("test", "python"));
        assert!(err.is_err());
    }

    #[test]
    fn registry_list() {
        let mut registry = SidecarRegistry::default();
        registry
            .register(SidecarConfig::new("alpha", "node"))
            .unwrap();
        registry
            .register(SidecarConfig::new("beta", "python"))
            .unwrap();
        let names = registry.list();
        assert_eq!(names, vec!["alpha", "beta"]); // sorted (BTreeMap)
    }

    #[test]
    fn registry_remove() {
        let mut registry = SidecarRegistry::default();
        registry
            .register(SidecarConfig::new("test", "node"))
            .unwrap();
        assert!(registry.remove("test"));
        assert!(registry.get("test").is_none());
    }

    #[test]
    fn registry_remove_nonexistent() {
        let mut registry = SidecarRegistry::default();
        assert!(!registry.remove("nonexistent"));
    }

    #[test]
    fn registry_get_nonexistent() {
        let registry = SidecarRegistry::default();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn registry_invalid_config_rejected() {
        let mut registry = SidecarRegistry::default();
        let config = SidecarConfig::new("", "node"); // empty name
        assert!(registry.register(config).is_err());
    }

    #[test]
    fn registry_from_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Create a subdirectory with a host.js file
        let sub = dir.path().join("my-sidecar");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("host.js"), "// sidecar").unwrap();

        let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
        assert!(registry.get("my-sidecar").is_some());
        let config = registry.get("my-sidecar").unwrap();
        assert_eq!(config.command, "node");
    }

    #[test]
    fn registry_discover_python_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("py-sidecar");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("host.py"), "# sidecar").unwrap();

        let registry = SidecarRegistry::discover_from_dir(dir.path()).unwrap();
        assert!(registry.get("py-sidecar").is_some());
        let config = registry.get("py-sidecar").unwrap();
        assert_eq!(config.command, "python");
    }

    #[test]
    fn registry_ignores_files_in_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("host.js"), "// not a sidecar dir").unwrap();

        let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn registry_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
        assert!(registry.list().is_empty());
    }
}

// =========================================================================
// 14. Lifecycle State Machine
// =========================================================================

mod lifecycle {
    use super::*;

    #[test]
    fn initial_state_uninitialized() {
        let mgr = LifecycleManager::new();
        assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
    }

    #[test]
    fn valid_transition_uninit_to_starting() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Starting, None).unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Starting);
    }

    #[test]
    fn valid_transition_starting_to_ready() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Starting, None).unwrap();
        mgr.transition(LifecycleState::Ready, None).unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Ready);
    }

    #[test]
    fn valid_transition_ready_to_running() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Starting, None).unwrap();
        mgr.transition(LifecycleState::Ready, None).unwrap();
        mgr.transition(LifecycleState::Running, None).unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Running);
    }

    #[test]
    fn running_back_to_ready() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Starting, None).unwrap();
        mgr.transition(LifecycleState::Ready, None).unwrap();
        mgr.transition(LifecycleState::Running, None).unwrap();
        mgr.transition(LifecycleState::Ready, None).unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Ready);
    }

    #[test]
    fn transition_to_failed_always_allowed() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Failed, Some("crash".to_string()))
            .unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Failed);
    }

    #[test]
    fn invalid_transition_rejected() {
        let mut mgr = LifecycleManager::new();
        let err = mgr.transition(LifecycleState::Running, None).unwrap_err();
        assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
    }

    #[test]
    fn already_in_state_error() {
        let mgr = LifecycleManager::new();
        // Cannot create the error directly through the public API since
        // Uninitialized->Uninitialized is caught by AlreadyInState
        let mut mgr = mgr;
        let err = mgr
            .transition(LifecycleState::Uninitialized, None)
            .unwrap_err();
        assert!(matches!(err, LifecycleError::AlreadyInState(_)));
    }

    #[test]
    fn history_records_transitions() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Starting, Some("boot".to_string()))
            .unwrap();
        mgr.transition(LifecycleState::Ready, None).unwrap();
        assert_eq!(mgr.history().len(), 2);
        assert_eq!(mgr.history()[0].from, LifecycleState::Uninitialized);
        assert_eq!(mgr.history()[0].to, LifecycleState::Starting);
    }

    #[test]
    fn uptime_none_before_ready() {
        let mgr = LifecycleManager::new();
        assert!(mgr.uptime().is_none());
    }

    #[test]
    fn uptime_some_after_ready() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Starting, None).unwrap();
        mgr.transition(LifecycleState::Ready, None).unwrap();
        assert!(mgr.uptime().is_some());
    }

    #[test]
    fn stopping_to_stopped() {
        let mut mgr = LifecycleManager::new();
        mgr.transition(LifecycleState::Starting, None).unwrap();
        mgr.transition(LifecycleState::Ready, None).unwrap();
        mgr.transition(LifecycleState::Stopping, None).unwrap();
        mgr.transition(LifecycleState::Stopped, None).unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Stopped);
    }
}

// =========================================================================
// 15. Health Monitoring
// =========================================================================

mod health_monitoring {
    use super::*;

    #[test]
    fn health_monitor_empty() {
        let monitor = HealthMonitor::new();
        assert!(!monitor.all_healthy());
        assert_eq!(monitor.total_checks(), 0);
    }

    #[test]
    fn record_healthy_check() {
        let mut monitor = HealthMonitor::new();
        monitor.record_check("s1", HealthStatus::Healthy, Some(Duration::from_millis(5)));
        assert!(monitor.all_healthy());
        let check = monitor.get_status("s1").unwrap();
        assert_eq!(check.consecutive_failures, 0);
    }

    #[test]
    fn record_unhealthy_check() {
        let mut monitor = HealthMonitor::new();
        monitor.record_check(
            "s1",
            HealthStatus::Unhealthy {
                reason: "timeout".to_string(),
            },
            None,
        );
        assert!(!monitor.all_healthy());
        let unhealthy = monitor.unhealthy_sidecars();
        assert_eq!(unhealthy.len(), 1);
    }

    #[test]
    fn consecutive_failures_tracked() {
        let mut monitor = HealthMonitor::new();
        monitor.record_check(
            "s1",
            HealthStatus::Unhealthy {
                reason: "err".to_string(),
            },
            None,
        );
        monitor.record_check(
            "s1",
            HealthStatus::Unhealthy {
                reason: "err".to_string(),
            },
            None,
        );
        let check = monitor.get_status("s1").unwrap();
        assert_eq!(check.consecutive_failures, 2);
    }

    #[test]
    fn consecutive_failures_reset_on_healthy() {
        let mut monitor = HealthMonitor::new();
        monitor.record_check(
            "s1",
            HealthStatus::Unhealthy {
                reason: "err".to_string(),
            },
            None,
        );
        monitor.record_check("s1", HealthStatus::Healthy, None);
        let check = monitor.get_status("s1").unwrap();
        assert_eq!(check.consecutive_failures, 0);
    }

    #[test]
    fn uptime_percentage() {
        let mut monitor = HealthMonitor::new();
        monitor.record_check("s1", HealthStatus::Healthy, None);
        monitor.record_check("s1", HealthStatus::Healthy, None);
        monitor.record_check(
            "s1",
            HealthStatus::Unhealthy {
                reason: "err".to_string(),
            },
            None,
        );
        monitor.record_check("s1", HealthStatus::Healthy, None);
        let pct = monitor.uptime_percentage("s1");
        assert!((pct - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn uptime_percentage_unknown_sidecar() {
        let monitor = HealthMonitor::new();
        assert!((monitor.uptime_percentage("nonexistent")).abs() < f64::EPSILON);
    }

    #[test]
    fn generate_report() {
        let mut monitor = HealthMonitor::new();
        monitor.record_check("s1", HealthStatus::Healthy, None);
        monitor.record_check(
            "s2",
            HealthStatus::Degraded {
                reason: "slow".to_string(),
            },
            None,
        );
        let report = monitor.generate_report();
        assert_eq!(report.checks.len(), 2);
        assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
    }
}

// =========================================================================
// 16. Process Management
// =========================================================================

mod process_management {
    use super::*;

    #[test]
    fn process_info_initial_state() {
        let info = ProcessInfo::new(SidecarSpec::new("node"), ProcessConfig::default());
        assert_eq!(info.status, ProcessStatus::NotStarted);
        assert!(!info.is_running());
        assert!(!info.is_terminated());
    }

    #[test]
    fn process_config_defaults() {
        let config = ProcessConfig::default();
        assert!(config.working_dir.is_none());
        assert!(config.env_vars.is_empty());
        assert!(config.timeout.is_none());
        assert!(config.inherit_env);
    }
}

// =========================================================================
// 17. Sequence Validation
// =========================================================================

mod sequence_validation {
    use super::*;

    #[test]
    fn empty_sequence_errors() {
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[]);
        assert!(errors.contains(&SequenceError::MissingHello));
        assert!(errors.contains(&SequenceError::MissingTerminal));
    }

    #[test]
    fn missing_terminal_error() {
        let sequence = vec![make_hello("test"), make_run("run-1")];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.contains(&SequenceError::MissingTerminal));
    }

    #[test]
    fn valid_hello_run_final() {
        let run_id = "run-simple";
        let sequence = vec![make_hello("test"), make_run(run_id), make_final(run_id)];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.is_empty());
    }

    #[test]
    fn valid_hello_run_fatal() {
        let run_id = "run-fail";
        let sequence = vec![
            make_hello("test"),
            make_run(run_id),
            make_fatal(Some(run_id), "error"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.is_empty());
    }

    #[test]
    fn valid_full_sequence_with_events() {
        let run_id = "run-full";
        let sequence = valid_protocol_sequence(run_id);
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&sequence);
        assert!(errors.is_empty());
    }
}

// =========================================================================
// 18. Message Routing
// =========================================================================

mod message_routing {
    use super::*;

    #[test]
    fn router_empty_no_match() {
        let router = MessageRouter::new();
        let env = make_hello("test");
        assert!(router.route(&env).is_none());
    }

    #[test]
    fn router_type_match() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "hello".to_string(),
            destination: "handler-hello".to_string(),
            priority: 1,
        });
        let env = make_hello("test");
        let route = router.route(&env).unwrap();
        assert_eq!(route.destination, "handler-hello");
    }

    #[test]
    fn router_ref_id_prefix_match() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "run-".to_string(),
            destination: "handler-run".to_string(),
            priority: 1,
        });
        let env = make_event("run-123", "test");
        let route = router.route(&env).unwrap();
        assert_eq!(route.destination, "handler-run");
    }

    #[test]
    fn router_priority_ordering() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "event".to_string(),
            destination: "low".to_string(),
            priority: 1,
        });
        router.add_route(MessageRoute {
            pattern: "event".to_string(),
            destination: "high".to_string(),
            priority: 10,
        });
        let env = make_event("run-1", "test");
        let route = router.route(&env).unwrap();
        assert_eq!(route.destination, "high");
    }

    #[test]
    fn router_remove_route() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "hello".to_string(),
            destination: "handler".to_string(),
            priority: 1,
        });
        assert_eq!(router.route_count(), 1);
        router.remove_route("handler");
        assert_eq!(router.route_count(), 0);
    }

    #[test]
    fn router_route_all() {
        let mut router = MessageRouter::new();
        router.add_route(MessageRoute {
            pattern: "fatal".to_string(),
            destination: "error-handler".to_string(),
            priority: 1,
        });
        let envelopes = vec![make_hello("test"), make_fatal(None, "err")];
        let matches = router.route_all(&envelopes);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].route.destination, "error-handler");
    }

    #[test]
    fn route_table_insert_lookup() {
        let mut table = RouteTable::new();
        table.insert("hello", "hello-handler");
        table.insert("fatal", "error-handler");
        assert_eq!(table.lookup("hello"), Some("hello-handler"));
        assert_eq!(table.lookup("fatal"), Some("error-handler"));
        assert_eq!(table.lookup("event"), None);
    }

    #[test]
    fn route_table_entries() {
        let mut table = RouteTable::new();
        table.insert("hello", "h");
        table.insert("run", "r");
        assert_eq!(table.entries().len(), 2);
    }
}

// =========================================================================
// 19. Batch Processing
// =========================================================================

mod batch_processing {
    use super::*;

    #[test]
    fn batch_process_success() {
        let processor = BatchProcessor::new();
        let request = BatchRequest {
            id: "batch-1".to_string(),
            envelopes: vec![make_fatal(None, "err1"), make_fatal(None, "err2")],
            created_at: Utc::now().to_rfc3339(),
        };
        let response = processor.process(request);
        assert_eq!(response.request_id, "batch-1");
        assert_eq!(response.results.len(), 2);
        assert!(response
            .results
            .iter()
            .all(|r| r.status == BatchItemStatus::Success));
    }

    #[test]
    fn batch_validate_empty() {
        let processor = BatchProcessor::new();
        let request = BatchRequest {
            id: "batch-empty".to_string(),
            envelopes: vec![],
            created_at: Utc::now().to_rfc3339(),
        };
        let errors = processor.validate_batch(&request);
        assert!(errors
            .iter()
            .any(|e| matches!(e, BatchValidationError::EmptyBatch)));
    }

    #[test]
    fn batch_max_size_constant() {
        assert_eq!(MAX_BATCH_SIZE, 1000);
    }
}

// =========================================================================
// 20. Compression
// =========================================================================

mod compression {
    use super::*;

    #[test]
    fn gzip_round_trip() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let data = b"test data for gzip compression";
        let compressed = c.compress(data).unwrap();
        let decompressed = c.decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn zstd_round_trip() {
        let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
        let data = b"test data for zstd compression";
        let compressed = c.compress(data).unwrap();
        let decompressed = c.decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn none_passthrough() {
        let c = MessageCompressor::new(CompressionAlgorithm::None);
        let data = b"plain data";
        assert_eq!(c.compress(data).unwrap(), data);
        assert_eq!(c.decompress(data).unwrap(), data);
    }

    #[test]
    fn compression_stats_tracking() {
        let mut stats = CompressionStats::new();
        stats.record(1000, 200);
        assert_eq!(stats.bytes_saved(), 800);
        assert!((stats.compression_ratio() - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn compressed_message_metadata() {
        let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
        let data = b"message payload";
        let msg = c.compress_message(data).unwrap();
        assert_eq!(msg.algorithm, CompressionAlgorithm::Gzip);
        assert_eq!(msg.original_size, data.len());
        let restored = c.decompress_message(&msg).unwrap();
        assert_eq!(restored, data);
    }
}

// =========================================================================
// 21. JSONL Codec Edge Cases
// =========================================================================

mod jsonl_codec_edge_cases {
    use super::*;

    #[test]
    fn encode_appends_newline() {
        let env = make_fatal(None, "test");
        let encoded = JsonlCodec::encode(&env).unwrap();
        assert!(encoded.ends_with('\n'));
    }

    #[test]
    fn encode_to_writer() {
        let env = make_fatal(None, "writer-test");
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.ends_with('\n'));
        assert!(s.contains("writer-test"));
    }

    #[test]
    fn encode_many_to_writer() {
        let envs = vec![make_fatal(None, "err1"), make_fatal(None, "err2")];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s.lines().count(), 2);
    }

    #[test]
    fn decode_stream_iterator() {
        let input = format!(
            "{}{}\n",
            JsonlCodec::encode(&make_fatal(None, "a")).unwrap(),
            JsonlCodec::encode(&make_fatal(None, "b")).unwrap().trim()
        );
        let reader = BufReader::new(input.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envs.len(), 2);
    }

    #[test]
    fn protocol_error_display() {
        let err = ProtocolError::Violation("test violation".to_string());
        assert_eq!(format!("{err}"), "protocol violation: test violation");
    }

    #[test]
    fn protocol_error_unexpected_message() {
        let err = ProtocolError::UnexpectedMessage {
            expected: "hello".to_string(),
            got: "run".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("hello"));
        assert!(msg.contains("run"));
    }

    #[test]
    fn protocol_error_codes() {
        let err = ProtocolError::Violation("v".to_string());
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );

        let err = ProtocolError::UnexpectedMessage {
            expected: "a".to_string(),
            got: "b".to_string(),
        };
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
        );
    }
}

// =========================================================================
// 22. Host Error Types
// =========================================================================

mod host_errors {
    use super::*;

    #[test]
    fn host_error_spawn_display() {
        let err = HostError::Spawn(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        assert!(err.to_string().contains("spawn"));
    }

    #[test]
    fn host_error_violation_display() {
        let err = HostError::Violation("bad message".to_string());
        assert!(err.to_string().contains("bad message"));
    }

    #[test]
    fn host_error_fatal_display() {
        let err = HostError::Fatal("sidecar crashed".to_string());
        assert!(err.to_string().contains("sidecar crashed"));
    }

    #[test]
    fn host_error_exited_display() {
        let err = HostError::Exited { code: Some(1) };
        assert!(err.to_string().contains("1"));
    }

    #[test]
    fn host_error_sidecar_crashed_display() {
        let err = HostError::SidecarCrashed {
            exit_code: Some(137),
            stderr: "killed".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("137"));
        assert!(msg.contains("killed"));
    }

    #[test]
    fn host_error_from_protocol_error() {
        let proto_err = ProtocolError::Violation("proto violation".to_string());
        let host_err: HostError = proto_err.into();
        assert!(matches!(host_err, HostError::Protocol(_)));
    }
}

// =========================================================================
// 23. Envelope Error Code Integration
// =========================================================================

mod error_code_integration {
    use super::*;

    #[test]
    fn fatal_with_protocol_error_code() {
        let env = Envelope::fatal_with_code(
            Some("run-1".to_string()),
            "invalid envelope",
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
        );
        assert_eq!(
            env.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    }

    #[test]
    fn fatal_without_error_code() {
        let env = make_fatal(Some("run-1"), "plain error");
        assert_eq!(env.error_code(), None);
    }

    #[test]
    fn non_fatal_has_no_error_code() {
        let env = make_hello("test");
        assert_eq!(env.error_code(), None);
    }

    #[test]
    fn fatal_error_code_round_trip() {
        let env = Envelope::fatal_with_code(
            Some("run-1".to_string()),
            "version mismatch",
            abp_error::ErrorCode::ProtocolVersionMismatch,
        );
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        assert_eq!(
            decoded.error_code(),
            Some(abp_error::ErrorCode::ProtocolVersionMismatch)
        );
    }
}

// =========================================================================
// 24. Wire Format Compliance
// =========================================================================

mod wire_format {
    use super::*;

    #[test]
    fn envelope_tag_field_is_t() {
        let env = make_hello("test");
        let json = serde_json::to_string(&env).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val.get("t").is_some(), "envelope must have 't' field");
    }

    #[test]
    fn hello_tag_is_hello() {
        let env = make_hello("test");
        let val: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(val["t"], "hello");
    }

    #[test]
    fn run_tag_is_run() {
        let env = make_run("r1");
        let val: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(val["t"], "run");
    }

    #[test]
    fn event_tag_is_event() {
        let env = make_event("r1", "msg");
        let val: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(val["t"], "event");
    }

    #[test]
    fn final_tag_is_final() {
        let env = make_final("r1");
        let val: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(val["t"], "final");
    }

    #[test]
    fn fatal_tag_is_fatal() {
        let env = make_fatal(None, "err");
        let val: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(val["t"], "fatal");
    }

    #[test]
    fn all_envelope_variants_encode() {
        let run_id = "run-wire";
        let envelopes = vec![
            make_hello("test"),
            make_run(run_id),
            make_event(run_id, "msg"),
            make_final(run_id),
            make_fatal(Some(run_id), "err"),
        ];
        for env in &envelopes {
            let encoded = JsonlCodec::encode(env);
            assert!(encoded.is_ok(), "failed to encode: {env:?}");
        }
    }

    #[test]
    fn agent_event_kind_tag_is_type() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".to_string(),
            },
            ext: None,
        };
        let val: serde_json::Value = serde_json::to_value(&event).unwrap();
        assert_eq!(val["type"], "run_started");
    }

    #[test]
    fn agent_event_kind_snake_case() {
        let kinds_and_expected = vec![
            (
                AgentEventKind::RunStarted {
                    message: "s".to_string(),
                },
                "run_started",
            ),
            (
                AgentEventKind::RunCompleted {
                    message: "c".to_string(),
                },
                "run_completed",
            ),
            (
                AgentEventKind::AssistantDelta {
                    text: "d".to_string(),
                },
                "assistant_delta",
            ),
            (
                AgentEventKind::AssistantMessage {
                    text: "m".to_string(),
                },
                "assistant_message",
            ),
            (
                AgentEventKind::Warning {
                    message: "w".to_string(),
                },
                "warning",
            ),
            (
                AgentEventKind::Error {
                    message: "e".to_string(),
                    error_code: None,
                },
                "error",
            ),
        ];
        for (kind, expected_type) in kinds_and_expected {
            let event = AgentEvent {
                ts: Utc::now(),
                kind,
                ext: None,
            };
            let val: serde_json::Value = serde_json::to_value(&event).unwrap();
            assert_eq!(
                val["type"].as_str().unwrap(),
                expected_type,
                "wrong type for {expected_type}"
            );
        }
    }

    #[test]
    fn contract_version_format() {
        let parsed = parse_version(CONTRACT_VERSION);
        assert!(parsed.is_some(), "CONTRACT_VERSION must be parseable");
        let (major, minor) = parsed.unwrap();
        assert_eq!(major, 0);
        assert_eq!(minor, 1);
    }

    #[test]
    fn execution_mode_default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }
}
