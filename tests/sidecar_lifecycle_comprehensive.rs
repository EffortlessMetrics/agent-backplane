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
//! Comprehensive tests for sidecar process lifecycle: spawn configuration,
//! hello handshake, contract version handling, event streaming, final/fatal
//! envelopes, termination, stderr capture, multiple sidecars, registration,
//! and error recovery.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::*;
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, PoolStats, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{RetryConfig, RetryMetadata, compute_delay, is_retryable};
use abp_host::{HostError, SidecarHello, SidecarSpec};
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn test_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("0.1.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "hello world".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/test".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn test_receipt(run_id: Uuid) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 42,
        },
        backend: test_identity(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::Value::Null,
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn test_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "started".into(),
        },
        ext: None,
    }
}

fn make_hello() -> Envelope {
    Envelope::hello(test_identity(), test_capabilities())
}

fn make_run(id: &str) -> Envelope {
    Envelope::Run {
        id: id.into(),
        work_order: test_work_order(),
    }
}

fn make_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: test_event(),
    }
}

fn make_final(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: test_receipt(Uuid::nil()),
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

// ===========================================================================
// 1. Sidecar Spawn Configuration
// ===========================================================================

#[test]
fn sidecar_spec_new_defaults() {
    let spec = SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_with_args() {
    let spec = SidecarSpec {
        command: "python".into(),
        args: vec!["host.py".into(), "--verbose".into()],
        env: BTreeMap::new(),
        cwd: None,
    };
    assert_eq!(spec.args.len(), 2);
    assert_eq!(spec.args[0], "host.py");
}

#[test]
fn sidecar_spec_with_env() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".into(), "secret".into());
    env.insert("LOG_LEVEL".into(), "debug".into());
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec![],
        env,
        cwd: None,
    };
    assert_eq!(spec.env.len(), 2);
    assert_eq!(spec.env["API_KEY"], "secret");
}

#[test]
fn sidecar_spec_with_cwd() {
    let spec = SidecarSpec {
        command: "bash".into(),
        args: vec!["host.sh".into()],
        env: BTreeMap::new(),
        cwd: Some("/workspace".into()),
    };
    assert_eq!(spec.cwd.as_deref(), Some("/workspace"));
}

#[test]
fn sidecar_spec_serde_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "VAL".into());
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["host.js".into()],
        env,
        cwd: Some("/tmp".into()),
    };
    let json = serde_json::to_string(&spec).unwrap();
    let decoded: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, "node");
    assert_eq!(decoded.args, vec!["host.js"]);
    assert_eq!(decoded.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn sidecar_spec_clone() {
    let spec = SidecarSpec::new("node");
    let cloned = spec.clone();
    assert_eq!(cloned.command, spec.command);
}

#[test]
fn sidecar_spec_env_deterministic_ordering() {
    let mut env = BTreeMap::new();
    env.insert("Z_KEY".into(), "z".into());
    env.insert("A_KEY".into(), "a".into());
    env.insert("M_KEY".into(), "m".into());
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec![],
        env,
        cwd: None,
    };
    let keys: Vec<&String> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A_KEY", "M_KEY", "Z_KEY"]);
}

#[test]
fn sidecar_spec_empty_command() {
    let spec = SidecarSpec::new("");
    assert!(spec.command.is_empty());
}

// ===========================================================================
// 2. Hello Handshake Protocol Validation
// ===========================================================================

#[test]
fn hello_envelope_roundtrip() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_envelope_contains_contract_version() {
    let hello = make_hello();
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_contains_backend_identity() {
    let hello = make_hello();
    match &hello {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("0.1.0"));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_contains_capabilities() {
    let hello = make_hello();
    match &hello {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_default_mode_is_mapped() {
    let hello = make_hello();
    match &hello {
        Envelope::Hello { mode, .. } => {
            assert_eq!(*mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        test_identity(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    match &hello {
        Envelope::Hello { mode, .. } => {
            assert_eq!(*mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_json_has_t_discriminator() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn hello_hello_data_parses_to_sidecar_hello_struct() {
    let hello = make_hello();
    match hello {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            ..
        } => {
            let sh = SidecarHello {
                contract_version,
                backend,
                capabilities,
            };
            assert_eq!(sh.contract_version, CONTRACT_VERSION);
            assert_eq!(sh.backend.id, "test-sidecar");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_must_be_first_in_sequence() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_run("r1"), make_hello(), make_final("r1")];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
    );
}

#[test]
fn hello_empty_backend_id_is_validation_error() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

// ===========================================================================
// 3. Contract Version Mismatch Handling
// ===========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/v1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn compatible_versions_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn incompatible_with_garbage() {
    assert!(!is_compatible_version("garbage", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
}

#[test]
fn protocol_version_parse_and_display() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    assert_eq!(format!("{v}"), "abp/v0.1");
}

#[test]
fn protocol_version_current() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn negotiate_version_same() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result.minor, 1); // min of the two
}

#[test]
fn negotiate_version_incompatible() {
    let v0 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v1 = ProtocolVersion::parse("abp/v1.0").unwrap();
    let result = negotiate_version(&v0, &v1);
    assert!(result.is_err());
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
}

#[test]
fn version_range_compatible() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

#[test]
fn hello_with_invalid_version_fails_validation() {
    let hello = Envelope::Hello {
        contract_version: "invalid".into(),
        backend: test_identity(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn hello_with_empty_version_fails_validation() {
    let hello = Envelope::Hello {
        contract_version: "".into(),
        backend: test_identity(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
}

// ===========================================================================
// 4. Event Streaming from Sidecar
// ===========================================================================

#[test]
fn event_envelope_roundtrip() {
    let event = make_event("run-1");
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-1"),
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_assistant_delta() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "hello".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("assistant_delta"));
}

#[test]
fn event_with_tool_call() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall { tool_name, .. } => {
                assert_eq!(tool_name, "read_file");
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_tool_result() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"content": "hello"}),
            is_error: false,
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("tool_result"));
}

#[test]
fn event_with_file_changed() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added function".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("file_changed"));
}

#[test]
fn event_with_command_executed() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("All tests passed".into()),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("command_executed"));
}

#[test]
fn event_with_warning() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "deprecated API".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("warning"));
}

#[test]
fn event_with_error() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "something failed".into(),
            error_code: None,
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("error"));
}

#[test]
fn event_with_extensions() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"vendor": "test"}));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("raw_message"));
}

#[test]
fn event_empty_ref_id_fails_validation() {
    let event = Envelope::Event {
        ref_id: "".into(),
        event: test_event(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&event);
    assert!(!result.valid);
}

#[test]
fn multiple_events_in_sequence_valid() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_event("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

// ===========================================================================
// 5. Final/Fatal Envelope Handling
// ===========================================================================

#[test]
fn final_envelope_roundtrip() {
    let f = make_final("run-1");
    let encoded = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn fatal_envelope_roundtrip() {
    let f = make_fatal(Some("run-1"), "out of memory");
    let encoded = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_without_ref_id() {
    let f = make_fatal(None, "startup failure");
    let encoded = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "startup failure");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn final_empty_ref_id_fails_validation() {
    let env = Envelope::Final {
        ref_id: "".into(),
        receipt: test_receipt(Uuid::nil()),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn fatal_empty_error_fails_validation() {
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "".into(),
        error_code: None,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn sequence_missing_terminal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_hello(), make_run("r1"), make_event("r1")];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
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
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MultipleTerminals))
    );
}

#[test]
fn sequence_ref_id_mismatch() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r2"), // wrong ref_id
        make_final("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn valid_full_sequence() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn valid_sequence_with_fatal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(Some("r1"), "crash"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

// ===========================================================================
// 6. Sidecar Process Termination (Lifecycle State Machine)
// ===========================================================================

#[test]
fn lifecycle_starts_uninitialized() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_valid_transition_uninitialized_to_starting() {
    let mut mgr = LifecycleManager::new();
    assert!(mgr.transition(LifecycleState::Starting, None).is_ok());
    assert_eq!(*mgr.state(), LifecycleState::Starting);
}

#[test]
fn lifecycle_valid_transition_starting_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_valid_transition_ready_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Running);
}

#[test]
fn lifecycle_valid_transition_running_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_valid_transition_running_to_stopping() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopping);
}

#[test]
fn lifecycle_valid_transition_stopping_to_stopped() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
}

#[test]
fn lifecycle_failed_from_any_state() {
    for start_state in [
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
    ] {
        let mut mgr = LifecycleManager::new();
        // Walk to the target state
        let path = match &start_state {
            LifecycleState::Uninitialized => vec![],
            LifecycleState::Starting => vec![LifecycleState::Starting],
            LifecycleState::Ready => vec![LifecycleState::Starting, LifecycleState::Ready],
            LifecycleState::Running => vec![
                LifecycleState::Starting,
                LifecycleState::Ready,
                LifecycleState::Running,
            ],
            LifecycleState::Stopping => vec![
                LifecycleState::Starting,
                LifecycleState::Ready,
                LifecycleState::Stopping,
            ],
            _ => vec![],
        };
        for s in path {
            mgr.transition(s, None).unwrap();
        }
        assert!(
            mgr.transition(LifecycleState::Failed, Some("crash".into()))
                .is_ok()
        );
        assert_eq!(*mgr.state(), LifecycleState::Failed);
    }
}

#[test]
fn lifecycle_invalid_transition_uninitialized_to_ready() {
    let mut mgr = LifecycleManager::new();
    let err = mgr.transition(LifecycleState::Ready, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn lifecycle_already_in_state() {
    let mut mgr = LifecycleManager::new();
    let err = mgr
        .transition(LifecycleState::Uninitialized, None)
        .unwrap_err();
    assert!(matches!(err, LifecycleError::AlreadyInState(_)));
}

#[test]
fn lifecycle_history_records_transitions() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let history = mgr.history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].from, LifecycleState::Uninitialized);
    assert_eq!(history[0].to, LifecycleState::Starting);
    assert_eq!(history[0].reason.as_deref(), Some("boot"));
}

#[test]
fn lifecycle_uptime_none_before_ready() {
    let mgr = LifecycleManager::new();
    assert!(mgr.uptime().is_none());
}

#[test]
fn lifecycle_uptime_some_after_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert!(mgr.uptime().is_some());
}

#[test]
fn lifecycle_display_states() {
    assert_eq!(
        format!("{}", LifecycleState::Uninitialized),
        "uninitialized"
    );
    assert_eq!(format!("{}", LifecycleState::Starting), "starting");
    assert_eq!(format!("{}", LifecycleState::Ready), "ready");
    assert_eq!(format!("{}", LifecycleState::Running), "running");
    assert_eq!(format!("{}", LifecycleState::Stopping), "stopping");
    assert_eq!(format!("{}", LifecycleState::Stopped), "stopped");
    assert_eq!(format!("{}", LifecycleState::Failed), "failed");
}

#[test]
fn lifecycle_can_transition_checks() {
    let mgr = LifecycleManager::new();
    assert!(mgr.can_transition(&LifecycleState::Starting));
    assert!(!mgr.can_transition(&LifecycleState::Ready));
    assert!(mgr.can_transition(&LifecycleState::Failed));
}

// ===========================================================================
// 7. Stderr Capture and Logging (ProcessInfo, ProcessStatus)
// ===========================================================================

#[test]
fn process_info_initial_state() {
    let spec = SidecarSpec::new("node");
    let config = ProcessConfig::default();
    let info = ProcessInfo::new(spec, config);
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(!info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_status_running() {
    let status = ProcessStatus::Running { pid: 1234 };
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = status;
    assert!(info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_status_exited() {
    let status = ProcessStatus::Exited { code: 0 };
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = status;
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn process_status_killed() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_status_timed_out() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

#[test]
fn process_config_defaults() {
    let config = ProcessConfig::default();
    assert!(config.working_dir.is_none());
    assert!(config.env_vars.is_empty());
    assert!(config.timeout.is_none());
    assert!(config.inherit_env);
}

#[test]
fn process_config_serde_roundtrip() {
    let config = ProcessConfig {
        working_dir: Some("/tmp".into()),
        env_vars: BTreeMap::new(),
        timeout: Some(Duration::from_secs(30)),
        inherit_env: false,
    };
    let json = serde_json::to_string(&config).unwrap();
    let decoded: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.timeout, Some(Duration::from_millis(30000)));
    assert!(!decoded.inherit_env);
}

#[test]
fn process_info_serde_roundtrip() {
    let spec = SidecarSpec::new("python");
    let config = ProcessConfig::default();
    let info = ProcessInfo::new(spec, config);
    let json = serde_json::to_string(&info).unwrap();
    let decoded: ProcessInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.spec.command, "python");
    assert_eq!(decoded.status, ProcessStatus::NotStarted);
}

// ===========================================================================
// 8. Multiple Sidecars (Pool Management)
// ===========================================================================

#[test]
fn pool_default_config() {
    let config = PoolConfig::default();
    assert_eq!(config.min_size, 1);
    assert_eq!(config.max_size, 4);
}

#[test]
fn pool_add_and_acquire() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.state, PoolEntryState::Busy);
    assert_eq!(entry.id, "s1");
}

#[test]
fn pool_acquire_empty_returns_none() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.acquire().is_none());
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
        max_size: 2,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3")); // exceeds max
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
fn pool_mark_failed() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.mark_failed("s1");
    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
}

#[test]
fn pool_drain_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.drain("s1");
    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
}

#[test]
fn pool_stats_comprehensive() {
    let config = PoolConfig {
        max_size: 10,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.acquire(); // s1 becomes busy
    pool.mark_failed("s3");
    let stats = pool.stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.busy, 1);
    assert_eq!(stats.idle, 1);
    assert_eq!(stats.failed, 1);
}

#[test]
fn pool_stats_utilization() {
    let stats = PoolStats {
        total: 4,
        idle: 2,
        busy: 2,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn pool_stats_utilization_empty() {
    let stats = PoolStats {
        total: 0,
        idle: 0,
        busy: 0,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn pool_active_count() {
    let config = PoolConfig {
        max_size: 10,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    pool.add("s1");
    pool.add("s2");
    pool.acquire(); // one becomes busy
    assert_eq!(pool.active_count(), 2); // idle + busy
}

// ===========================================================================
// 9. Sidecar Registration
// ===========================================================================

#[test]
fn registry_register_and_lookup() {
    let mut reg = SidecarRegistry::default();
    let config = SidecarConfig::new("node-sidecar", "node");
    reg.register(config).unwrap();
    assert!(reg.get("node-sidecar").is_some());
}

#[test]
fn registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("z-sidecar", "node"))
        .unwrap();
    reg.register(SidecarConfig::new("a-sidecar", "python"))
        .unwrap();
    let list = reg.list();
    assert_eq!(list, vec!["a-sidecar", "z-sidecar"]);
}

#[test]
fn registry_duplicate_name_fails() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("test", "node")).unwrap();
    let err = reg.register(SidecarConfig::new("test", "python"));
    assert!(err.is_err());
}

#[test]
fn registry_remove() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("test", "node")).unwrap();
    assert!(reg.remove("test"));
    assert!(reg.get("test").is_none());
}

#[test]
fn registry_remove_nonexistent() {
    let mut reg = SidecarRegistry::default();
    assert!(!reg.remove("ghost"));
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
    config.args = vec!["host.js".into()];
    config.env.insert("KEY".into(), "VALUE".into());
    config.working_dir = Some("/tmp".into());
    let spec = config.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env["KEY"], "VALUE");
    assert_eq!(spec.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn sidecar_config_serde_roundtrip() {
    let config = SidecarConfig::new("test-sc", "python");
    let json = serde_json::to_string(&config).unwrap();
    let decoded: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.name, "test-sc");
    assert_eq!(decoded.command, "python");
}

// ===========================================================================
// 10. Error Recovery on Sidecar Crash
// ===========================================================================

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    let msg = format!("{err}");
    assert!(msg.contains("spawn"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("unexpected state".into());
    let msg = format!("{err}");
    assert!(msg.contains("unexpected state"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("out of memory".into());
    let msg = format!("{err}");
    assert!(msg.contains("out of memory"));
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(1) };
    let msg = format!("{err}");
    assert!(msg.contains("exited"));
}

#[test]
fn host_error_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by OOM".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("crashed"));
    assert!(msg.contains("137"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = format!("{err}");
    assert!(msg.contains("timed out"));
}

#[test]
fn host_error_protocol_display() {
    let err = HostError::Protocol(ProtocolError::Violation("bad data".into()));
    let msg = format!("{err}");
    assert!(msg.contains("protocol"));
}

#[test]
fn is_retryable_spawn_error() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "nf"));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_exited_error() {
    let err = HostError::Exited { code: Some(1) };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_timeout_error() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(5),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_crashed_error() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "crash".into(),
    };
    assert!(is_retryable(&err));
}

#[test]
fn not_retryable_violation() {
    let err = HostError::Violation("bad protocol".into());
    assert!(!is_retryable(&err));
}

#[test]
fn not_retryable_fatal() {
    let err = HostError::Fatal("permanent error".into());
    assert!(!is_retryable(&err));
}

#[test]
fn not_retryable_protocol_error() {
    let err = HostError::Protocol(ProtocolError::Violation("bad".into()));
    assert!(!is_retryable(&err));
}

#[test]
fn retry_config_defaults() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.base_delay, Duration::from_millis(100));
    assert_eq!(config.max_delay, Duration::from_secs(10));
    assert_eq!(config.overall_timeout, Duration::from_secs(60));
}

#[test]
fn compute_delay_increases_exponentially() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        jitter_factor: 0.0, // no jitter for deterministic test
        ..RetryConfig::default()
    };
    let d0 = compute_delay(&config, 0);
    let d1 = compute_delay(&config, 1);
    let d2 = compute_delay(&config, 2);
    assert_eq!(d0, Duration::from_millis(100));
    assert_eq!(d1, Duration::from_millis(200));
    assert_eq!(d2, Duration::from_millis(400));
}

#[test]
fn compute_delay_capped_at_max() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(300),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let d5 = compute_delay(&config, 5);
    assert_eq!(d5, Duration::from_millis(300));
}

#[test]
fn compute_delay_with_jitter_within_bounds() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(60),
        jitter_factor: 0.5,
        ..RetryConfig::default()
    };
    for attempt in 0..5 {
        let d = compute_delay(&config, attempt);
        let nominal = 1000u64 * 2u64.pow(attempt);
        // With 50% jitter, delay is in [nominal*0.5, nominal]
        assert!(d.as_millis() as u64 <= nominal);
    }
}

#[test]
fn retry_metadata_to_receipt_metadata() {
    let meta = RetryMetadata {
        total_attempts: 2,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(150),
    };
    let receipt_meta = meta.to_receipt_metadata();
    assert_eq!(receipt_meta["retry_total_attempts"], serde_json::json!(2));
    assert!(receipt_meta.contains_key("retry_total_duration_ms"));
}

#[test]
fn retry_config_serde_roundtrip() {
    let config = RetryConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let decoded: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.max_retries, config.max_retries);
}

// ===========================================================================
// Health monitoring
// ===========================================================================

#[test]
fn health_monitor_record_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, Some(Duration::from_millis(5)));
    assert!(monitor.all_healthy());
    assert_eq!(monitor.total_checks(), 1);
}

#[test]
fn health_monitor_record_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "timeout".into(),
        },
        None,
    );
    assert!(!monitor.all_healthy());
    assert_eq!(monitor.unhealthy_sidecars().len(), 1);
}

#[test]
fn health_monitor_consecutive_failures() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "still down".into(),
        },
        None,
    );
    let check = monitor.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 2);
}

#[test]
fn health_monitor_recovery_resets_failures() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    monitor.record_check("s1", HealthStatus::Healthy, None);
    let check = monitor.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_uptime_percentage() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check("s1", HealthStatus::Unhealthy { reason: "x".into() }, None);
    monitor.record_check("s1", HealthStatus::Healthy, None);
    // 3 out of 4 healthy = 75%
    let pct = monitor.uptime_percentage("s1");
    assert!((pct - 75.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_report_overall_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check("s2", HealthStatus::Healthy, None);
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Healthy));
}

#[test]
fn health_monitor_report_overall_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check(
        "s2",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn health_monitor_report_overall_degraded() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check(
        "s2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

#[test]
fn health_monitor_empty_report_unknown() {
    let monitor = HealthMonitor::new();
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unknown));
}

#[test]
fn health_monitor_uptime_unknown_sidecar() {
    let monitor = HealthMonitor::new();
    assert!((monitor.uptime_percentage("nonexistent") - 0.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 11. JSONL Envelope Types — Extended
// ===========================================================================

#[test]
fn run_envelope_roundtrip() {
    let run = make_run("run-42");
    let encoded = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-42");
            assert_eq!(work_order.task, "hello world");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_envelope_t_tag() {
    let run = make_run("r1");
    let json = JsonlCodec::encode(&run).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn event_envelope_t_tag() {
    let event = make_event("r1");
    let json = JsonlCodec::encode(&event).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn final_envelope_t_tag() {
    let f = make_final("r1");
    let json = JsonlCodec::encode(&f).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn fatal_envelope_t_tag() {
    let f = make_fatal(Some("r1"), "boom");
    let json = JsonlCodec::encode(&f).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn envelope_encode_ends_with_newline() {
    for env in [
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("r1"),
    ] {
        let encoded = JsonlCodec::encode(&env).unwrap();
        assert!(encoded.ends_with('\n'), "envelope must end with newline");
    }
}

#[test]
fn unknown_envelope_type_fails_decode() {
    let line = r#"{"t":"unknown_type","data":"test"}"#;
    let result = JsonlCodec::decode(line);
    assert!(result.is_err());
}

#[test]
fn invalid_json_fails_decode() {
    let result = JsonlCodec::decode("not valid json at all");
    assert!(result.is_err());
}

#[test]
fn empty_json_object_fails_decode() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn json_missing_t_field_fails_decode() {
    let result = JsonlCodec::decode(r#"{"ref_id":"r1","error":"boom"}"#);
    assert!(result.is_err());
}

#[test]
fn fatal_with_error_code_roundtrip() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "rate limited",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(decoded.error_code().is_some());
}

#[test]
fn envelope_error_code_on_non_fatal_is_none() {
    let hello = make_hello();
    assert!(hello.error_code().is_none());
    let run = make_run("r1");
    assert!(run.error_code().is_none());
    let event = make_event("r1");
    assert!(event.error_code().is_none());
}

#[test]
fn fatal_without_error_code_returns_none() {
    let f = make_fatal(Some("r1"), "boom");
    assert!(f.error_code().is_none());
}

#[test]
fn decode_stream_multiple_envelopes() {
    let mut buf = String::new();
    buf.push_str(&JsonlCodec::encode(&make_fatal(None, "err1")).unwrap());
    buf.push_str(&JsonlCodec::encode(&make_fatal(None, "err2")).unwrap());
    let reader = std::io::BufReader::new(buf.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_skips_blank_lines() {
    let line = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();
    let input = format!("\n\n{}\n\n", line.trim());
    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn encode_to_writer_works() {
    let mut buf = Vec::new();
    let env = make_fatal(None, "test");
    abp_protocol::JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("fatal"));
}

#[test]
fn encode_many_to_writer_works() {
    let mut buf = Vec::new();
    let envs = [make_fatal(None, "a"), make_fatal(None, "b")];
    abp_protocol::JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<_> = s.lines().collect();
    assert_eq!(lines.len(), 2);
}

// ===========================================================================
// 12. Protocol Handshake — Extended
// ===========================================================================

#[test]
fn hello_with_empty_capabilities() {
    let hello = Envelope::hello(test_identity(), CapabilityManifest::new());
    match &hello {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.is_empty());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_multiple_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);
    let hello = Envelope::hello(test_identity(), caps);
    match &hello {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 4);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_missing_backend_version_warns() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(result.valid); // warnings don't invalidate
    assert!(!result.warnings.is_empty());
}

#[test]
fn hello_sequence_missing_hello_error() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_run("r1"), make_final("r1")];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
}

#[test]
fn hello_empty_sequence_errors() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
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

#[test]
fn hello_contract_version_matches_constant() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn hello_with_mode_passthrough_serde_roundtrip() {
    let hello = Envelope::hello_with_mode(
        test_identity(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(v01 < v02);
    assert!(v02 < v10);
}

#[test]
fn protocol_version_serde_roundtrip() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let json = serde_json::to_string(&v).unwrap();
    let decoded: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, v);
}

#[test]
fn version_error_display_messages() {
    use abp_protocol::version::VersionError;
    let invalid_format = VersionError::InvalidFormat;
    assert!(format!("{invalid_format}").contains("invalid version format"));
    let invalid_major = VersionError::InvalidMajor;
    assert!(format!("{invalid_major}").contains("major"));
}

// ===========================================================================
// 13. Run Lifecycle — Extended
// ===========================================================================

#[test]
fn run_preserves_work_order_fields() {
    let wo = test_work_order();
    let id = wo.id;
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.id, id);
            assert_eq!(work_order.task, "hello world");
            assert!(matches!(work_order.lane, ExecutionLane::PatchFirst));
            assert!(matches!(
                work_order.workspace.mode,
                WorkspaceMode::PassThrough
            ));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_complex_work_order() {
    let wo = WorkOrder {
        id: Uuid::new_v4(),
        task: "Refactor authentication module with multi-factor support".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/user/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**/*.rs".into(), "tests/**/*.rs".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/auth.rs".into(), "src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "requirements".into(),
                content: "Must support TOTP and WebAuthn".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read_file".into(), "write_file".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/secrets/**".into()],
            allow_network: vec![],
            deny_network: vec!["*.internal.corp".into()],
            require_approval_for: vec!["write_file".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        },
        config: RuntimeConfig {
            model: Some("claude-sonnet-4-20250514".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: Some(1.0),
            max_turns: Some(10),
        },
    };

    let run = Envelope::Run {
        id: "complex-run".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "complex-run");
            assert_eq!(work_order.context.files.len(), 2);
            assert_eq!(work_order.context.snippets.len(), 1);
            assert_eq!(work_order.policy.allowed_tools.len(), 2);
            assert_eq!(work_order.config.max_turns, Some(10));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_empty_task_fails_validation() {
    let mut wo = test_work_order();
    wo.task = String::new();
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&run);
    assert!(!result.valid);
}

#[test]
fn run_empty_id_fails_validation() {
    let run = Envelope::Run {
        id: String::new(),
        work_order: test_work_order(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&run);
    assert!(!result.valid);
}

#[test]
fn ref_id_correlation_across_events_and_final() {
    let run_id = "correlated-run-123";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_event(run_id),
        make_event(run_id),
        make_final(run_id),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn ref_id_mismatch_in_final_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_event("run-1"),
        make_final("run-2"), // wrong ref_id
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
        make_event("r1"), // before Run
        make_run("r1"),
        make_final("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::OutOfOrderEvents))
    );
}

#[test]
fn receipt_in_final_preserves_outcome() {
    let receipt = test_receipt(Uuid::nil());
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt: receipt.clone(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn receipt_partial_outcome_roundtrip() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.outcome = Outcome::Partial;
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
        _ => panic!("expected Final"),
    }
}

#[test]
fn receipt_failed_outcome_roundtrip() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.outcome = Outcome::Failed;
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
        _ => panic!("expected Final"),
    }
}

// ===========================================================================
// 14. Error Handling — Extended
// ===========================================================================

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("{{bad json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    assert!(format!("{err}").contains("JSON"));
}

#[test]
fn protocol_error_violation_variant() {
    let err = ProtocolError::Violation("test violation".into());
    assert!(format!("{err}").contains("test violation"));
}

#[test]
fn protocol_error_unexpected_message_variant() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("hello"));
    assert!(msg.contains("run"));
}

#[test]
fn protocol_error_code_for_violation() {
    let err = ProtocolError::Violation("bad".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_code_for_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn protocol_error_code_for_json_is_none() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    assert!(err.error_code().is_none());
}

#[test]
fn host_error_from_protocol_error() {
    let proto_err = ProtocolError::Violation("bad data".into());
    let host_err = HostError::Protocol(proto_err);
    let msg = format!("{host_err}");
    assert!(msg.contains("protocol"));
    assert!(msg.contains("bad data"));
}

#[test]
fn host_error_exited_without_code() {
    let err = HostError::Exited { code: None };
    let msg = format!("{err}");
    assert!(msg.contains("exited"));
    assert!(msg.contains("None"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pipe broken",
    ));
    let msg = format!("{err}");
    assert!(msg.contains("stdin"));
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "eof",
    ));
    let msg = format!("{err}");
    assert!(msg.contains("stdout"));
}

#[test]
fn is_retryable_stdout_error() {
    let err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "eof",
    ));
    assert!(is_retryable(&err));
}

#[test]
fn not_retryable_stdin_error() {
    let err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pipe broken",
    ));
    assert!(!is_retryable(&err));
}

#[test]
fn fatal_envelope_terminates_valid_sequence() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_fatal(Some("r1"), "OOM"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn fatal_with_none_ref_id_warns_validation() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup crash".into(),
        error_code: None,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(result.valid); // ref_id is optional on fatal
    assert!(!result.warnings.is_empty());
}

// ===========================================================================
// 15. SidecarConfig — Extended
// ===========================================================================

#[test]
fn sidecar_config_with_env_vars() {
    let mut config = SidecarConfig::new("my-sidecar", "node");
    config.env.insert("NODE_ENV".into(), "production".into());
    config.env.insert("PORT".into(), "3000".into());
    let spec = config.to_spec();
    assert_eq!(spec.env.len(), 2);
    assert_eq!(spec.env["NODE_ENV"], "production");
}

#[test]
fn sidecar_config_with_working_dir() {
    let mut config = SidecarConfig::new("test", "python");
    config.working_dir = Some("/opt/sidecar".into());
    let spec = config.to_spec();
    assert_eq!(spec.cwd.as_deref(), Some("/opt/sidecar"));
}

#[test]
fn sidecar_config_with_args() {
    let mut config = SidecarConfig::new("test", "node");
    config.args = vec!["host.js".into(), "--port".into(), "8080".into()];
    let spec = config.to_spec();
    assert_eq!(spec.args.len(), 3);
    assert_eq!(spec.args[2], "8080");
}

#[test]
fn sidecar_config_validate_valid() {
    let config = SidecarConfig::new("ok", "python");
    assert!(config.validate().is_ok());
}

#[test]
fn sidecar_config_full_serde_roundtrip() {
    let mut config = SidecarConfig::new("full-test", "node");
    config.args = vec!["host.js".into()];
    config.env.insert("K".into(), "V".into());
    config.working_dir = Some("/tmp/work".into());
    let json = serde_json::to_string(&config).unwrap();
    let decoded: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.name, "full-test");
    assert_eq!(decoded.args, vec!["host.js"]);
    assert_eq!(decoded.env["K"], "V");
    assert_eq!(decoded.working_dir, Some("/tmp/work".into()));
}

#[test]
fn sidecar_config_default_fields() {
    let json = r#"{"name":"test","command":"node"}"#;
    let config: SidecarConfig = serde_json::from_str(json).unwrap();
    assert!(config.args.is_empty());
    assert!(config.env.is_empty());
    assert!(config.working_dir.is_none());
}

// ===========================================================================
// 16. Multi-Sidecar Scenarios — Extended
// ===========================================================================

#[test]
fn registry_multiple_sidecars_lookup() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node-sc", "node")).unwrap();
    reg.register(SidecarConfig::new("python-sc", "python"))
        .unwrap();
    reg.register(SidecarConfig::new("bash-sc", "bash")).unwrap();
    assert_eq!(reg.list().len(), 3);
    assert!(reg.get("node-sc").is_some());
    assert!(reg.get("python-sc").is_some());
    assert!(reg.get("bash-sc").is_some());
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn registry_sidecar_selection_by_name() {
    let mut reg = SidecarRegistry::default();
    let mut node_config = SidecarConfig::new("node-sc", "node");
    node_config.args = vec!["host.js".into()];
    let mut python_config = SidecarConfig::new("python-sc", "python");
    python_config.args = vec!["host.py".into()];
    reg.register(node_config).unwrap();
    reg.register(python_config).unwrap();

    let selected = reg.get("python-sc").unwrap();
    assert_eq!(selected.command, "python");
    assert_eq!(selected.args, vec!["host.py"]);
}

#[test]
fn registry_remove_then_re_register() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("test", "node")).unwrap();
    reg.remove("test");
    reg.register(SidecarConfig::new("test", "python")).unwrap();
    let config = reg.get("test").unwrap();
    assert_eq!(config.command, "python");
}

#[test]
fn pool_multiple_entries_lifecycle() {
    let config = PoolConfig {
        max_size: 10,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");

    // Acquire all
    let e1 = pool.acquire().unwrap();
    let e2 = pool.acquire().unwrap();
    let e3 = pool.acquire().unwrap();
    assert!(pool.acquire().is_none()); // all busy

    // Release one
    pool.release(&e1.id);
    assert_eq!(pool.idle_count(), 1);

    // Mark one failed
    pool.mark_failed(&e2.id);
    let stats = pool.stats();
    assert_eq!(stats.busy, 1);
    assert_eq!(stats.idle, 1);
    assert_eq!(stats.failed, 1);

    // Acquire the released one
    let e4 = pool.acquire().unwrap();
    assert_eq!(e4.id, e1.id);

    // Drain the last busy one
    pool.drain(&e3.id);
    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
}

#[test]
fn pool_config_serde_roundtrip() {
    let config = PoolConfig {
        min_size: 2,
        max_size: 8,
        idle_timeout: Duration::from_secs(120),
        health_check_interval: Duration::from_secs(15),
    };
    let json = serde_json::to_string(&config).unwrap();
    let decoded: PoolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.min_size, 2);
    assert_eq!(decoded.max_size, 8);
    assert_eq!(decoded.idle_timeout, Duration::from_secs(120));
}

#[test]
fn pool_stats_serde_roundtrip() {
    let stats = PoolStats {
        total: 5,
        idle: 2,
        busy: 2,
        draining: 1,
        failed: 0,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let decoded: PoolStats = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, stats);
}

#[test]
fn pool_remove_nonexistent_returns_none() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.remove("ghost").is_none());
}

// ===========================================================================
// 17. Lifecycle State Machine — Extended
// ===========================================================================

#[test]
fn lifecycle_state_serde_roundtrip() {
    let states = vec![
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
        LifecycleState::Stopped,
        LifecycleState::Failed,
    ];
    for state in states {
        let json = serde_json::to_string(&state).unwrap();
        let decoded: LifecycleState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }
}

#[test]
fn lifecycle_invalid_transition_starting_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    let err = mgr.transition(LifecycleState::Running, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn lifecycle_invalid_transition_stopped_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    let err = mgr.transition(LifecycleState::Ready, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn lifecycle_full_happy_path() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, Some("handshake done".into()))
        .unwrap();
    mgr.transition(LifecycleState::Running, Some("work order received".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, Some("run complete".into()))
        .unwrap();
    mgr.transition(LifecycleState::Running, Some("second run".into()))
        .unwrap();
    mgr.transition(LifecycleState::Stopping, Some("shutdown".into()))
        .unwrap();
    mgr.transition(LifecycleState::Stopped, Some("clean exit".into()))
        .unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
    assert_eq!(mgr.history().len(), 7);
}

#[test]
fn lifecycle_error_display() {
    let err = LifecycleError::InvalidTransition {
        from: LifecycleState::Uninitialized,
        to: LifecycleState::Running,
    };
    let msg = format!("{err}");
    assert!(msg.contains("invalid lifecycle transition"));
    assert!(msg.contains("uninitialized"));
    assert!(msg.contains("running"));

    let err2 = LifecycleError::AlreadyInState(LifecycleState::Ready);
    let msg2 = format!("{err2}");
    assert!(msg2.contains("already in state"));
    assert!(msg2.contains("ready"));
}

// ===========================================================================
// 18. Health Monitoring — Extended
// ===========================================================================

#[test]
fn health_status_serde_roundtrip() {
    let statuses = vec![
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        HealthStatus::Unknown,
    ];
    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let decoded: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, status);
    }
}

#[test]
fn health_monitor_multiple_sidecars() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, Some(Duration::from_millis(5)));
    monitor.record_check("s2", HealthStatus::Healthy, Some(Duration::from_millis(10)));
    monitor.record_check(
        "s3",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    assert_eq!(monitor.total_checks(), 3);
    assert!(!monitor.all_healthy());
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

#[test]
fn health_report_serde_roundtrip() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    let report = monitor.generate_report();
    let json = serde_json::to_string(&report).unwrap();
    let decoded: abp_host::health::HealthReport = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.checks.len(), 1);
}

// ===========================================================================
// 19. Retry — Extended
// ===========================================================================

#[test]
fn retry_config_serde_custom_values() {
    let config = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(30),
        overall_timeout: Duration::from_secs(120),
        jitter_factor: 0.3,
    };
    let json = serde_json::to_string(&config).unwrap();
    let decoded: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.max_retries, 5);
    assert_eq!(decoded.base_delay, Duration::from_millis(200));
    assert!((decoded.jitter_factor - 0.3).abs() < f64::EPSILON);
}

#[test]
fn retry_metadata_with_failed_attempts() {
    use abp_host::retry::RetryAttempt;
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![
            RetryAttempt {
                attempt: 0,
                error: "spawn failed".into(),
                delay: Duration::from_millis(100),
            },
            RetryAttempt {
                attempt: 1,
                error: "timeout".into(),
                delay: Duration::from_millis(200),
            },
        ],
        total_duration: Duration::from_millis(500),
    };
    let receipt_meta = meta.to_receipt_metadata();
    assert_eq!(receipt_meta["retry_total_attempts"], serde_json::json!(3));
    assert!(receipt_meta.contains_key("retry_failed_attempts"));
}

#[test]
fn compute_delay_zero_jitter_is_deterministic() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(50),
        max_delay: Duration::from_secs(60),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let d0a = compute_delay(&config, 0);
    let d0b = compute_delay(&config, 0);
    assert_eq!(d0a, d0b);
    assert_eq!(d0a, Duration::from_millis(50));
}

// ===========================================================================
// 20. Process Status — Extended
// ===========================================================================

#[test]
fn process_status_serde_roundtrip() {
    let statuses = vec![
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 42 },
        ProcessStatus::Exited { code: 0 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ];
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let decoded: ProcessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, status);
    }
}

#[test]
fn process_info_running_then_exited() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    assert!(!info.is_running());
    assert!(!info.is_terminated());

    info.status = ProcessStatus::Running { pid: 999 };
    info.started_at = Some(Utc::now());
    assert!(info.is_running());
    assert!(!info.is_terminated());

    info.status = ProcessStatus::Exited { code: 0 };
    info.ended_at = Some(Utc::now());
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn process_config_with_all_fields() {
    let mut env_vars = BTreeMap::new();
    env_vars.insert("LOG".into(), "debug".into());
    let config = ProcessConfig {
        working_dir: Some("/opt/sidecar".into()),
        env_vars,
        timeout: Some(Duration::from_secs(60)),
        inherit_env: false,
    };
    let json = serde_json::to_string(&config).unwrap();
    let decoded: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.working_dir, Some("/opt/sidecar".into()));
    assert_eq!(decoded.env_vars["LOG"], "debug");
    assert_eq!(decoded.timeout, Some(Duration::from_secs(60)));
    assert!(!decoded.inherit_env);
}

// ===========================================================================
// 21. SidecarHello — Extended
// ===========================================================================

#[test]
fn sidecar_hello_serde_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_identity(),
        capabilities: test_capabilities(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.contract_version, CONTRACT_VERSION);
    assert_eq!(decoded.backend.id, "test-sidecar");
    assert!(decoded.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn sidecar_hello_empty_capabilities_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "minimal".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: SidecarHello = serde_json::from_str(&json).unwrap();
    assert!(decoded.capabilities.is_empty());
}
