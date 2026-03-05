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
//! Comprehensive end-to-end tests for the `abp-host` crate's sidecar process
//! management, covering configuration, JSONL codec, handshake validation,
//! lifecycle state machine, health monitoring, pool management, registry,
//! retry logic, and edge cases.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, Outcome, Receipt,
    ReceiptBuilder, SupportLevel, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, PoolStats, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{self, RetryConfig, RetryMetadata};
use abp_host::{HostError, SidecarClient, SidecarHello, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ===========================================================================
// 1. SidecarSpec builder/configuration
// ===========================================================================

#[test]
fn sidecar_spec_new_sets_command() {
    let spec = SidecarSpec::new("python3");
    assert_eq!(spec.command, "python3");
}

#[test]
fn sidecar_spec_new_has_empty_args() {
    let spec = SidecarSpec::new("node");
    assert!(spec.args.is_empty());
}

#[test]
fn sidecar_spec_new_has_empty_env() {
    let spec = SidecarSpec::new("node");
    assert!(spec.env.is_empty());
}

#[test]
fn sidecar_spec_new_has_no_cwd() {
    let spec = SidecarSpec::new("node");
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_with_args() {
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["host.js".into(), "--verbose".into()],
        env: BTreeMap::new(),
        cwd: None,
    };
    assert_eq!(spec.args.len(), 2);
    assert_eq!(spec.args[0], "host.js");
}

#[test]
fn sidecar_spec_with_env() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".into(), "secret".into());
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec![],
        env,
        cwd: None,
    };
    assert_eq!(spec.env.get("API_KEY").unwrap(), "secret");
}

#[test]
fn sidecar_spec_with_cwd() {
    let spec = SidecarSpec {
        command: "python3".into(),
        args: vec![],
        env: BTreeMap::new(),
        cwd: Some("/tmp/workspace".into()),
    };
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/workspace"));
}

#[test]
fn sidecar_spec_serialization_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "val".into());
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["host.js".into()],
        env,
        cwd: Some("/workspace".into()),
    };
    let json = serde_json::to_string(&spec).unwrap();
    let decoded: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, "node");
    assert_eq!(decoded.args, vec!["host.js"]);
    assert_eq!(decoded.cwd.as_deref(), Some("/workspace"));
}

#[test]
fn sidecar_spec_clone() {
    let spec = SidecarSpec::new("python3");
    let cloned = spec.clone();
    assert_eq!(cloned.command, "python3");
}

#[test]
fn sidecar_spec_debug() {
    let spec = SidecarSpec::new("node");
    let debug = format!("{spec:?}");
    assert!(debug.contains("node"));
}

// ===========================================================================
// 2. Process specification (ProcessConfig, ProcessInfo, ProcessStatus)
// ===========================================================================

#[test]
fn process_config_default_has_no_timeout() {
    let config = ProcessConfig::default();
    assert!(config.timeout.is_none());
}

#[test]
fn process_config_default_has_no_working_dir() {
    let config = ProcessConfig::default();
    assert!(config.working_dir.is_none());
}

#[test]
fn process_config_default_inherits_env() {
    let config = ProcessConfig::default();
    assert!(config.inherit_env);
}

#[test]
fn process_config_default_has_empty_env_vars() {
    let config = ProcessConfig::default();
    assert!(config.env_vars.is_empty());
}

#[test]
fn process_config_with_timeout() {
    let config = ProcessConfig {
        timeout: Some(Duration::from_secs(30)),
        ..ProcessConfig::default()
    };
    assert_eq!(config.timeout, Some(Duration::from_secs(30)));
}

#[test]
fn process_config_with_working_dir() {
    let config = ProcessConfig {
        working_dir: Some(PathBuf::from("/tmp")),
        ..ProcessConfig::default()
    };
    assert_eq!(
        config.working_dir.as_deref(),
        Some(std::path::Path::new("/tmp"))
    );
}

#[test]
fn process_config_serialization_roundtrip() {
    let config = ProcessConfig {
        timeout: Some(Duration::from_millis(5000)),
        working_dir: Some(PathBuf::from("/work")),
        inherit_env: false,
        env_vars: BTreeMap::new(),
    };
    let json = serde_json::to_string(&config).unwrap();
    let decoded: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.timeout, Some(Duration::from_millis(5000)));
    assert!(!decoded.inherit_env);
}

#[test]
fn process_status_not_started() {
    let status = ProcessStatus::NotStarted;
    assert_eq!(status, ProcessStatus::NotStarted);
}

#[test]
fn process_status_running() {
    let status = ProcessStatus::Running { pid: 1234 };
    assert_eq!(status, ProcessStatus::Running { pid: 1234 });
}

#[test]
fn process_status_exited() {
    let status = ProcessStatus::Exited { code: 0 };
    assert_eq!(status, ProcessStatus::Exited { code: 0 });
}

#[test]
fn process_status_killed() {
    assert_eq!(ProcessStatus::Killed, ProcessStatus::Killed);
}

#[test]
fn process_status_timed_out() {
    assert_eq!(ProcessStatus::TimedOut, ProcessStatus::TimedOut);
}

#[test]
fn process_status_serde_roundtrip() {
    let statuses = vec![
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 42 },
        ProcessStatus::Exited { code: 1 },
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
fn process_info_new_not_started() {
    let spec = SidecarSpec::new("node");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(info.started_at.is_none());
    assert!(info.ended_at.is_none());
}

#[test]
fn process_info_is_running_false_when_not_started() {
    let spec = SidecarSpec::new("node");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert!(!info.is_running());
}

#[test]
fn process_info_is_terminated_false_when_not_started() {
    let spec = SidecarSpec::new("node");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_is_running_true_when_running() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 100 };
    assert!(info.is_running());
}

#[test]
fn process_info_is_terminated_when_exited() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(info.is_terminated());
}

#[test]
fn process_info_is_terminated_when_killed() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_info_is_terminated_when_timed_out() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

// ===========================================================================
// 3. JSONL codec for handshake messages
// ===========================================================================

fn make_hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build()
}

#[test]
fn jsonl_encode_hello_contains_tag_t() {
    let hello = make_hello_envelope();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains(r#""t":"hello""#));
}

#[test]
fn jsonl_encode_ends_with_newline() {
    let hello = make_hello_envelope();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn jsonl_decode_hello_roundtrip() {
    let hello = make_hello_envelope();
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn jsonl_encode_run_contains_tag_t() {
    let wo = WorkOrderBuilder::new("test task").build();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    assert!(line.contains(r#""t":"run""#));
}

#[test]
fn jsonl_encode_event_contains_tag_t() {
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    assert!(line.contains(r#""t":"event""#));
}

#[test]
fn jsonl_encode_final_contains_tag_t() {
    let receipt = make_receipt();
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&fin).unwrap();
    assert!(line.contains(r#""t":"final""#));
}

#[test]
fn jsonl_encode_fatal_contains_tag_t() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something broke".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    assert!(line.contains(r#""t":"fatal""#));
}

#[test]
fn jsonl_decode_fatal_without_ref_id() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "boom");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn jsonl_decode_invalid_json_is_error() {
    let result = JsonlCodec::decode("not valid json");
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_empty_string_is_error() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_unknown_tag_is_error() {
    let line = r#"{"t":"unknown_type","data":123}"#;
    let result = JsonlCodec::decode(line);
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_stream_multiple_lines() {
    let hello = make_hello_envelope();
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "oops".into(),
        error_code: None,
    };
    let mut buf = String::new();
    buf.push_str(&JsonlCodec::encode(&hello).unwrap());
    buf.push_str(&JsonlCodec::encode(&fatal).unwrap());

    let reader = BufReader::new(buf.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn jsonl_decode_stream_skips_blank_lines() {
    let hello = make_hello_envelope();
    let mut buf = String::new();
    buf.push('\n');
    buf.push_str(&JsonlCodec::encode(&hello).unwrap());
    buf.push('\n');
    buf.push('\n');

    let reader = BufReader::new(buf.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn jsonl_hello_includes_contract_version() {
    let hello = make_hello_envelope();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains(CONTRACT_VERSION));
}

// ===========================================================================
// 4. Handshake validation: hello → run → event* → final|fatal
// ===========================================================================

#[test]
fn sidecar_hello_struct_fields() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
    };
    assert_eq!(hello.contract_version, CONTRACT_VERSION);
    assert_eq!(hello.backend.id, "test");
}

#[test]
fn sidecar_hello_serde_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.backend.id, "sidecar:node");
}

#[test]
fn sidecar_hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: caps,
    };
    assert_eq!(hello.capabilities.len(), 2);
}

#[test]
fn envelope_hello_constructor() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sc".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    match env {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "sc");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn protocol_handshake_sequence_encode_decode() {
    // Simulate the protocol: hello → run → event → final
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );

    let wo = WorkOrderBuilder::new("do something").build();
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };

    let event = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
    };

    let receipt = make_receipt();
    let fin = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };

    // All should encode/decode successfully
    for env in &[hello, run, event, fin] {
        let line = JsonlCodec::encode(env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        // Verify the tag type matches
        match (env, &decoded) {
            (Envelope::Hello { .. }, Envelope::Hello { .. }) => {}
            (Envelope::Run { .. }, Envelope::Run { .. }) => {}
            (Envelope::Event { .. }, Envelope::Event { .. }) => {}
            (Envelope::Final { .. }, Envelope::Final { .. }) => {}
            _ => panic!("envelope type mismatch"),
        }
    }
}

// ===========================================================================
// 5. Error handling
// ===========================================================================

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    let msg = err.to_string();
    assert!(msg.contains("spawn"));
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken pipe",
    ));
    let msg = err.to_string();
    assert!(msg.contains("stdout"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken pipe",
    ));
    let msg = err.to_string();
    assert!(msg.contains("stdin"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("unexpected hello".into());
    let msg = err.to_string();
    assert!(msg.contains("unexpected hello"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("something broke".into());
    let msg = err.to_string();
    assert!(msg.contains("something broke"));
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(1) };
    let msg = err.to_string();
    assert!(msg.contains("1"));
}

#[test]
fn host_error_exited_no_code() {
    let err = HostError::Exited { code: None };
    let msg = err.to_string();
    assert!(msg.contains("None"));
}

#[test]
fn host_error_sidecar_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(139),
        stderr: "segfault".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("139"));
    assert!(msg.contains("segfault"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = err.to_string();
    assert!(msg.contains("30"));
}

#[test]
fn host_error_protocol_from_protocol_error() {
    let proto_err = ProtocolError::Violation("bad message".into());
    let host_err = HostError::Protocol(proto_err);
    let msg = host_err.to_string();
    assert!(msg.contains("bad message"));
}

#[tokio::test]
async fn spawn_nonexistent_command_returns_spawn_error() {
    let spec = SidecarSpec::new("__nonexistent_command_xyz__");
    let result: Result<SidecarClient, HostError> = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, HostError::Spawn(_)));
}

#[tokio::test]
async fn spawn_command_that_exits_immediately_returns_error() {
    // A command that exits with 0 without printing anything
    let spec = SidecarSpec {
        command: if cfg!(windows) {
            "cmd".into()
        } else {
            "true".into()
        },
        args: if cfg!(windows) {
            vec!["/C".into(), "exit".into()]
        } else {
            vec![]
        },
        env: BTreeMap::new(),
        cwd: None,
    };
    let result: Result<SidecarClient, HostError> = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
}

// ===========================================================================
// 6. Sidecar registry
// ===========================================================================

#[test]
fn registry_default_is_empty() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    let config = SidecarConfig::new("test-sidecar", "node");
    reg.register(config).unwrap();
    assert!(reg.get("test-sidecar").is_some());
}

#[test]
fn registry_register_duplicate_fails() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "node")).unwrap();
    let result = reg.register(SidecarConfig::new("sc", "python3"));
    assert!(result.is_err());
}

#[test]
fn registry_list_returns_sorted_names() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("zeta", "node")).unwrap();
    reg.register(SidecarConfig::new("alpha", "python3"))
        .unwrap();
    reg.register(SidecarConfig::new("middle", "bash")).unwrap();
    let names = reg.list();
    assert_eq!(names, vec!["alpha", "middle", "zeta"]);
}

#[test]
fn registry_remove_existing() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "node")).unwrap();
    assert!(reg.remove("sc"));
    assert!(reg.get("sc").is_none());
}

#[test]
fn registry_remove_nonexistent() {
    let mut reg = SidecarRegistry::default();
    assert!(!reg.remove("nope"));
}

#[test]
fn registry_get_nonexistent() {
    let reg = SidecarRegistry::default();
    assert!(reg.get("nope").is_none());
}

#[test]
fn sidecar_config_new() {
    let config = SidecarConfig::new("my-sidecar", "python3");
    assert_eq!(config.name, "my-sidecar");
    assert_eq!(config.command, "python3");
    assert!(config.args.is_empty());
    assert!(config.env.is_empty());
    assert!(config.working_dir.is_none());
}

#[test]
fn sidecar_config_validate_success() {
    let config = SidecarConfig::new("test", "node");
    assert!(config.validate().is_ok());
}

#[test]
fn sidecar_config_validate_empty_name_fails() {
    let config = SidecarConfig::new("", "node");
    assert!(config.validate().is_err());
}

#[test]
fn sidecar_config_validate_empty_command_fails() {
    let config = SidecarConfig::new("test", "");
    assert!(config.validate().is_err());
}

#[test]
fn sidecar_config_to_spec() {
    let mut config = SidecarConfig::new("test", "node");
    config.args = vec!["host.js".into()];
    config.working_dir = Some(PathBuf::from("/work"));
    let spec = config.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert!(spec.cwd.is_some());
}

#[test]
fn sidecar_config_to_spec_no_cwd() {
    let config = SidecarConfig::new("test", "python3");
    let spec = config.to_spec();
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_config_serde_roundtrip() {
    let mut config = SidecarConfig::new("my-sc", "node");
    config.args = vec!["a".into(), "b".into()];
    let json = serde_json::to_string(&config).unwrap();
    let decoded: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.name, "my-sc");
    assert_eq!(decoded.args.len(), 2);
}

#[test]
fn registry_from_config_dir_with_tempdir() {
    let dir = tempfile::tempdir().unwrap();

    // Create a sidecar directory with a host.js file
    let sc_dir = dir.path().join("my-sidecar");
    std::fs::create_dir(&sc_dir).unwrap();
    std::fs::write(sc_dir.join("host.js"), "// mock").unwrap();

    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert_eq!(reg.list(), vec!["my-sidecar"]);
    let config = reg.get("my-sidecar").unwrap();
    assert_eq!(config.command, "node");
}

#[test]
fn registry_from_config_dir_multiple_sidecars() {
    let dir = tempfile::tempdir().unwrap();

    let js_dir = dir.path().join("node-sc");
    std::fs::create_dir(&js_dir).unwrap();
    std::fs::write(js_dir.join("host.js"), "").unwrap();

    let py_dir = dir.path().join("python-sc");
    std::fs::create_dir(&py_dir).unwrap();
    std::fs::write(py_dir.join("host.py"), "").unwrap();

    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    let names = reg.list();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"node-sc"));
    assert!(names.contains(&"python-sc"));
}

#[test]
fn registry_from_config_dir_ignores_files() {
    let dir = tempfile::tempdir().unwrap();
    // Create a file (not a directory) — should be ignored
    std::fs::write(dir.path().join("not-a-dir.txt"), "").unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_from_config_dir_ignores_dirs_without_host_script() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("empty-sidecar");
    std::fs::create_dir(&sub).unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_discover_from_dir_alias() {
    let dir = tempfile::tempdir().unwrap();
    let sc_dir = dir.path().join("sc");
    std::fs::create_dir(&sc_dir).unwrap();
    std::fs::write(sc_dir.join("host.py"), "").unwrap();

    let reg = SidecarRegistry::discover_from_dir(dir.path()).unwrap();
    assert_eq!(reg.list().len(), 1);
}

#[test]
fn registry_from_config_dir_nonexistent_path_errors() {
    let result = SidecarRegistry::from_config_dir(std::path::Path::new("/nonexistent/path/xyz"));
    assert!(result.is_err());
}

// ===========================================================================
// 7. Lifecycle state machine
// ===========================================================================

#[test]
fn lifecycle_new_is_uninitialized() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_default_is_uninitialized() {
    let mgr = LifecycleManager::default();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_transition_uninitialized_to_starting() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Starting);
}

#[test]
fn lifecycle_transition_starting_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_transition_ready_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Running);
}

#[test]
fn lifecycle_transition_running_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_transition_running_to_stopping() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopping);
}

#[test]
fn lifecycle_transition_stopping_to_stopped() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
}

#[test]
fn lifecycle_transition_to_failed_always_allowed() {
    // From every state, Failed should be reachable
    for initial in [
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
    ] {
        let mut mgr = LifecycleManager::new();
        // Walk to the desired initial state
        match &initial {
            LifecycleState::Uninitialized => {}
            LifecycleState::Starting => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
            }
            LifecycleState::Ready => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
            }
            LifecycleState::Running => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
                mgr.transition(LifecycleState::Running, None).unwrap();
            }
            LifecycleState::Stopping => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
                mgr.transition(LifecycleState::Stopping, None).unwrap();
            }
            _ => {}
        }
        assert!(
            mgr.transition(LifecycleState::Failed, Some("test".into()))
                .is_ok(),
            "Failed transition should succeed from {initial}"
        );
    }
}

#[test]
fn lifecycle_invalid_transition_returns_error() {
    let mut mgr = LifecycleManager::new();
    // Cannot go directly from Uninitialized to Ready
    let result = mgr.transition(LifecycleState::Ready, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        LifecycleError::InvalidTransition { from, to } => {
            assert_eq!(from, LifecycleState::Uninitialized);
            assert_eq!(to, LifecycleState::Ready);
        }
        _ => panic!("expected InvalidTransition"),
    }
}

#[test]
fn lifecycle_same_state_returns_already_in_state() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Uninitialized, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        LifecycleError::AlreadyInState(s) => {
            assert_eq!(s, LifecycleState::Uninitialized);
        }
        _ => panic!("expected AlreadyInState"),
    }
}

#[test]
fn lifecycle_history_tracks_transitions() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();

    let hist = mgr.history();
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0].from, LifecycleState::Uninitialized);
    assert_eq!(hist[0].to, LifecycleState::Starting);
    assert_eq!(hist[0].reason.as_deref(), Some("boot"));
    assert_eq!(hist[1].from, LifecycleState::Starting);
    assert_eq!(hist[1].to, LifecycleState::Ready);
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
fn lifecycle_can_transition_checks() {
    let mgr = LifecycleManager::new();
    assert!(mgr.can_transition(&LifecycleState::Starting));
    assert!(!mgr.can_transition(&LifecycleState::Ready));
    assert!(!mgr.can_transition(&LifecycleState::Running));
    assert!(mgr.can_transition(&LifecycleState::Failed));
}

#[test]
fn lifecycle_state_display() {
    assert_eq!(LifecycleState::Uninitialized.to_string(), "uninitialized");
    assert_eq!(LifecycleState::Starting.to_string(), "starting");
    assert_eq!(LifecycleState::Ready.to_string(), "ready");
    assert_eq!(LifecycleState::Running.to_string(), "running");
    assert_eq!(LifecycleState::Stopping.to_string(), "stopping");
    assert_eq!(LifecycleState::Stopped.to_string(), "stopped");
    assert_eq!(LifecycleState::Failed.to_string(), "failed");
}

#[test]
fn lifecycle_error_display() {
    let err = LifecycleError::InvalidTransition {
        from: LifecycleState::Uninitialized,
        to: LifecycleState::Running,
    };
    assert!(err.to_string().contains("invalid lifecycle transition"));

    let err = LifecycleError::AlreadyInState(LifecycleState::Ready);
    assert!(err.to_string().contains("already in state"));
}

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
    for state in &states {
        let json = serde_json::to_string(state).unwrap();
        let decoded: LifecycleState = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, state);
    }
}

// ===========================================================================
// 8. Health monitoring
// ===========================================================================

#[test]
fn health_monitor_new_is_empty() {
    let monitor = HealthMonitor::new();
    assert_eq!(monitor.total_checks(), 0);
}

#[test]
fn health_monitor_record_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc1",
        HealthStatus::Healthy,
        Some(Duration::from_millis(50)),
    );
    assert_eq!(monitor.total_checks(), 1);
    let check = monitor.get_status("sc1").unwrap();
    assert!(matches!(check.status, HealthStatus::Healthy));
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_record_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "crash".into(),
        },
        None,
    );
    let check = monitor.get_status("sc1").unwrap();
    assert!(matches!(check.status, HealthStatus::Unhealthy { .. }));
    assert_eq!(check.consecutive_failures, 1);
}

#[test]
fn health_monitor_consecutive_failures_increment() {
    let mut monitor = HealthMonitor::new();
    for i in 1..=3 {
        monitor.record_check(
            "sc1",
            HealthStatus::Unhealthy {
                reason: "fail".into(),
            },
            None,
        );
        assert_eq!(monitor.get_status("sc1").unwrap().consecutive_failures, i);
    }
}

#[test]
fn health_monitor_consecutive_failures_reset_on_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    assert_eq!(monitor.get_status("sc1").unwrap().consecutive_failures, 0);
}

#[test]
fn health_monitor_all_healthy_true() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check("sc2", HealthStatus::Healthy, None);
    assert!(monitor.all_healthy());
}

#[test]
fn health_monitor_all_healthy_false_when_one_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc2",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    assert!(!monitor.all_healthy());
}

#[test]
fn health_monitor_all_healthy_false_when_empty() {
    let monitor = HealthMonitor::new();
    assert!(!monitor.all_healthy());
}

#[test]
fn health_monitor_unhealthy_sidecars() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc2",
        HealthStatus::Unhealthy {
            reason: "oops".into(),
        },
        None,
    );
    let unhealthy = monitor.unhealthy_sidecars();
    assert_eq!(unhealthy.len(), 1);
    assert_eq!(unhealthy[0].name, "sc2");
}

#[test]
fn health_monitor_get_status_nonexistent() {
    let monitor = HealthMonitor::new();
    assert!(monitor.get_status("nope").is_none());
}

#[test]
fn health_monitor_uptime_percentage() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    // 3 out of 4 healthy = 75%
    let pct = monitor.uptime_percentage("sc1");
    assert!((pct - 75.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_uptime_percentage_unknown() {
    let monitor = HealthMonitor::new();
    assert!((monitor.uptime_percentage("nope")).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_generate_report_empty() {
    let monitor = HealthMonitor::new();
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unknown));
    assert!(report.checks.is_empty());
}

#[test]
fn health_monitor_generate_report_all_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Healthy));
}

#[test]
fn health_monitor_generate_report_with_degraded() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

#[test]
fn health_monitor_generate_report_with_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc2",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

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
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let decoded: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, status);
    }
}

// ===========================================================================
// 9. Pool management
// ===========================================================================

#[test]
fn pool_config_default() {
    let config = PoolConfig::default();
    assert_eq!(config.min_size, 1);
    assert_eq!(config.max_size, 4);
}

#[test]
fn pool_new_is_empty() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.idle_count(), 0);
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn pool_add_and_count() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    assert_eq!(pool.total_count(), 1);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_add_respects_max_size() {
    let config = PoolConfig {
        max_size: 2,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3")); // Should be rejected
    assert_eq!(pool.total_count(), 2);
}

#[test]
fn pool_acquire_marks_busy() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.state, PoolEntryState::Busy);
    assert_eq!(entry.id, "s1");
    assert_eq!(pool.idle_count(), 0);
}

#[test]
fn pool_acquire_returns_none_when_empty() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_acquire_returns_none_when_all_busy() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.acquire().unwrap();
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_release_marks_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.acquire().unwrap();
    pool.release("s1");
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_mark_failed() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.mark_failed("s1");
    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.idle, 0);
}

#[test]
fn pool_drain() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.drain("s1");
    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
}

#[test]
fn pool_remove() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let removed = pool.remove("s1");
    assert!(removed.is_some());
    assert_eq!(pool.total_count(), 0);
}

#[test]
fn pool_remove_nonexistent() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.remove("nope").is_none());
}

#[test]
fn pool_stats() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.acquire().unwrap(); // s1 becomes busy
    pool.mark_failed("s3");

    let stats = pool.stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.busy, 1);
    assert_eq!(stats.idle, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.draining, 0);
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
    assert!((stats.utilization()).abs() < f64::EPSILON);
}

#[test]
fn pool_active_count_excludes_failed_and_draining() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.mark_failed("s2");
    pool.drain("s3");
    assert_eq!(pool.active_count(), 1); // Only s1 is idle/active
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
}

// ===========================================================================
// 10. Retry logic
// ===========================================================================

#[test]
fn retry_config_default() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 3);
    assert!(config.base_delay > Duration::ZERO);
    assert!(config.max_delay > config.base_delay);
    assert!(config.overall_timeout > Duration::ZERO);
}

#[test]
fn retry_config_serde_roundtrip() {
    let config = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(5),
        overall_timeout: Duration::from_secs(30),
        jitter_factor: 0.3,
    };
    let json = serde_json::to_string(&config).unwrap();
    let decoded: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.max_retries, 5);
}

#[test]
fn compute_delay_first_attempt() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let delay = retry::compute_delay(&config, 0);
    assert_eq!(delay, Duration::from_millis(100));
}

#[test]
fn compute_delay_exponential_backoff() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let d0 = retry::compute_delay(&config, 0);
    let d1 = retry::compute_delay(&config, 1);
    let d2 = retry::compute_delay(&config, 2);
    assert_eq!(d0, Duration::from_millis(100));
    assert_eq!(d1, Duration::from_millis(200));
    assert_eq!(d2, Duration::from_millis(400));
}

#[test]
fn compute_delay_capped_at_max() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(500),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let delay = retry::compute_delay(&config, 10);
    assert!(delay <= Duration::from_millis(500));
}

#[test]
fn compute_delay_with_jitter_within_bounds() {
    let config = RetryConfig {
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(10),
        jitter_factor: 0.5,
        ..RetryConfig::default()
    };
    // With jitter, delay should be between 500ms and 1000ms for attempt 0
    for _ in 0..10 {
        let delay = retry::compute_delay(&config, 0);
        assert!(delay <= Duration::from_millis(1000));
        assert!(delay >= Duration::from_millis(500));
    }
}

#[test]
fn is_retryable_spawn_error() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, ""));
    assert!(retry::is_retryable(&err));
}

#[test]
fn is_retryable_stdout_error() {
    let err = HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, ""));
    assert!(retry::is_retryable(&err));
}

#[test]
fn is_retryable_exited() {
    let err = HostError::Exited { code: Some(1) };
    assert!(retry::is_retryable(&err));
}

#[test]
fn is_retryable_crashed() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(139),
        stderr: "segfault".into(),
    };
    assert!(retry::is_retryable(&err));
}

#[test]
fn is_retryable_timeout() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(10),
    };
    assert!(retry::is_retryable(&err));
}

#[test]
fn is_not_retryable_violation() {
    let err = HostError::Violation("bad protocol".into());
    assert!(!retry::is_retryable(&err));
}

#[test]
fn is_not_retryable_fatal() {
    let err = HostError::Fatal("fatal error".into());
    assert!(!retry::is_retryable(&err));
}

#[test]
fn is_not_retryable_protocol_error() {
    let proto = ProtocolError::Violation("bad".into());
    let err = HostError::Protocol(proto);
    assert!(!retry::is_retryable(&err));
}

#[test]
fn retry_metadata_default() {
    let meta = RetryMetadata::default();
    assert_eq!(meta.total_attempts, 0);
    assert!(meta.failed_attempts.is_empty());
}

#[test]
fn retry_metadata_to_receipt_metadata_no_failures() {
    let meta = RetryMetadata {
        total_attempts: 1,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(100),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map.get("retry_total_attempts"), Some(&serde_json::json!(1)));
    assert!(!map.contains_key("retry_failed_attempts"));
}

#[test]
fn retry_metadata_to_receipt_metadata_with_failures() {
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![retry::RetryAttempt {
            attempt: 0,
            error: "spawn error".into(),
            delay: Duration::from_millis(100),
        }],
        total_duration: Duration::from_millis(500),
    };
    let map = meta.to_receipt_metadata();
    assert!(map.contains_key("retry_failed_attempts"));
}

#[tokio::test]
async fn retry_async_succeeds_first_attempt() {
    let config = RetryConfig {
        max_retries: 3,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_secs(1),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let result = retry::retry_async(
        &config,
        || async { Ok::<_, HostError>(42) },
        retry::is_retryable,
    )
    .await;
    let outcome = result.unwrap();
    assert_eq!(outcome.value, 42);
    assert_eq!(outcome.metadata.total_attempts, 1);
    assert!(outcome.metadata.failed_attempts.is_empty());
}

#[tokio::test]
async fn retry_async_non_retryable_fails_immediately() {
    let config = RetryConfig {
        max_retries: 3,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_secs(1),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let result = retry::retry_async(
        &config,
        || async { Err::<i32, _>(HostError::Violation("bad".into())) },
        retry::is_retryable,
    )
    .await;
    assert!(result.is_err());
}

// ===========================================================================
// 11. Edge cases
// ===========================================================================

#[test]
fn jsonl_decode_only_whitespace_is_error() {
    let result = JsonlCodec::decode("   ");
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_valid_json_but_wrong_schema() {
    let result = JsonlCodec::decode(r#"{"key": "value"}"#);
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_partial_hello_missing_fields() {
    let result = JsonlCodec::decode(r#"{"t":"hello"}"#);
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_run_without_work_order_is_error() {
    let result = JsonlCodec::decode(r#"{"t":"run","id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn jsonl_large_message_roundtrip() {
    let big_text = "x".repeat(100_000);
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: big_text.clone(),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text.len(), 100_000);
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn jsonl_encode_special_characters() {
    let event = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "line1\nline2\ttab\"quote\\backslash".into(),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains('\n'));
                assert!(text.contains('\t'));
                assert!(text.contains('"'));
                assert!(text.contains('\\'));
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn jsonl_encode_unicode() {
    let event = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "こんにちは 🌍 مرحبا".into(),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains("こんにちは"));
                assert!(text.contains("🌍"));
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn pool_concurrent_add_acquire_release() {
    let pool = SidecarPool::new(PoolConfig {
        max_size: 100,
        ..PoolConfig::default()
    });
    // Add many entries
    for i in 0..50 {
        assert!(pool.add(format!("s{i}")));
    }
    assert_eq!(pool.total_count(), 50);

    // Acquire all
    let mut acquired = Vec::new();
    while let Some(entry) = pool.acquire() {
        acquired.push(entry.id.clone());
    }
    assert_eq!(acquired.len(), 50);
    assert_eq!(pool.idle_count(), 0);

    // Release all
    for id in &acquired {
        pool.release(id);
    }
    assert_eq!(pool.idle_count(), 50);
}

#[test]
fn sidecar_spec_env_is_deterministic_btreemap() {
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
    // BTreeMap iterates in sorted key order
    let keys: Vec<_> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A_KEY", "M_KEY", "Z_KEY"]);
}

#[test]
fn work_order_builder_creates_valid_work_order() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
}

#[test]
fn receipt_builder_creates_valid_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn protocol_error_display_json() {
    let err: ProtocolError = serde_json::from_str::<Envelope>("bad json")
        .unwrap_err()
        .into();
    let msg = err.to_string();
    assert!(msg.contains("JSON"));
}

#[test]
fn protocol_error_display_violation() {
    let err = ProtocolError::Violation("test violation".into());
    assert!(err.to_string().contains("test violation"));
}

#[test]
fn protocol_error_display_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("hello"));
    assert!(msg.contains("run"));
}

#[test]
fn lifecycle_full_happy_path() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("booting".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, Some("handshake done".into()))
        .unwrap();
    mgr.transition(
        LifecycleState::Running,
        Some("processing work order".into()),
    )
    .unwrap();
    mgr.transition(LifecycleState::Ready, Some("work complete".into()))
        .unwrap();
    mgr.transition(LifecycleState::Stopping, Some("shutting down".into()))
        .unwrap();
    mgr.transition(LifecycleState::Stopped, Some("clean exit".into()))
        .unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
    assert_eq!(mgr.history().len(), 6);
}

#[test]
fn lifecycle_invalid_stopped_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    let result = mgr.transition(LifecycleState::Running, None);
    assert!(result.is_err());
}

#[test]
fn health_monitor_default_trait() {
    let monitor = HealthMonitor::default();
    assert_eq!(monitor.total_checks(), 0);
}

#[test]
fn pool_expired_idle_entries_empty_when_fresh() {
    let pool = SidecarPool::new(PoolConfig {
        idle_timeout: Duration::from_secs(300),
        ..PoolConfig::default()
    });
    pool.add("s1");
    // Just added, so not expired
    assert!(pool.expired_idle_entries().is_empty());
}

#[test]
fn pool_config_accessor() {
    let config = PoolConfig {
        min_size: 3,
        max_size: 10,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    assert_eq!(pool.config().min_size, 3);
    assert_eq!(pool.config().max_size, 10);
}

#[test]
fn sidecar_config_with_env_and_args() {
    let mut config = SidecarConfig::new("advanced", "node");
    config.args = vec!["--port".into(), "8080".into()];
    config.env.insert("NODE_ENV".into(), "production".into());
    config.working_dir = Some(PathBuf::from("/app"));

    assert!(config.validate().is_ok());
    let spec = config.to_spec();
    assert_eq!(spec.args.len(), 2);
    assert_eq!(spec.env.get("NODE_ENV").unwrap(), "production");
    assert!(spec.cwd.is_some());
}

#[test]
fn envelope_fatal_with_ref_id() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-42".into()));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn host_error_is_debug() {
    let err = HostError::Violation("test".into());
    let debug = format!("{err:?}");
    assert!(debug.contains("Violation"));
}
