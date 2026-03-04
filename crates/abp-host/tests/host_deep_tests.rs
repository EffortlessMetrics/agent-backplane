// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the abp-host crate covering sidecar process management.

use std::io::{BufReader, Cursor};
use std::path::PathBuf;
use std::time::Duration;

use abp_core::{
    BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest, ExecutionMode, SupportLevel,
};
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, PoolStats, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{RetryConfig, RetryMetadata, compute_delay, is_retryable};
use abp_host::{HostError, SidecarHello, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};

// ═══════════════════════════════════════════════════════════════════════
// 1. SidecarSpec construction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_spec_default_fields() {
    let spec = SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_custom_fields() {
    let mut spec = SidecarSpec::new("python3");
    spec.args = vec!["host.py".into(), "--verbose".into()];
    spec.env.insert("PYTHONPATH".into(), "/usr/lib".into());
    spec.cwd = Some("/tmp/workdir".into());
    assert_eq!(spec.command, "python3");
    assert_eq!(spec.args.len(), 2);
    assert_eq!(spec.env["PYTHONPATH"], "/usr/lib");
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/workdir"));
}

#[test]
fn sidecar_spec_serde_roundtrip() {
    let mut spec = SidecarSpec::new("bash");
    spec.args = vec!["-c".into(), "echo hi".into()];
    spec.env.insert("FOO".into(), "bar".into());
    let json = serde_json::to_string(&spec).unwrap();
    let deser: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.command, "bash");
    assert_eq!(deser.args, spec.args);
    assert_eq!(deser.env, spec.env);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. SidecarConfig validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_config_valid() {
    let cfg = SidecarConfig::new("my-sidecar", "node");
    assert!(cfg.validate().is_ok());
}

#[test]
fn sidecar_config_empty_name_invalid() {
    let cfg = SidecarConfig::new("", "node");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_empty_command_invalid() {
    let cfg = SidecarConfig::new("my-sidecar", "");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_both_empty_invalid() {
    let cfg = SidecarConfig::new("", "");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_with_args_and_env() {
    let mut cfg = SidecarConfig::new("test", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("NODE_ENV".into(), "production".into());
    cfg.working_dir = Some(PathBuf::from("/opt/sidecar"));
    assert!(cfg.validate().is_ok());
    assert_eq!(cfg.args.len(), 1);
    assert_eq!(cfg.env["NODE_ENV"], "production");
}

#[test]
fn sidecar_config_to_spec_conversion() {
    let mut cfg = SidecarConfig::new("test", "python3");
    cfg.args = vec!["host.py".into()];
    cfg.env.insert("KEY".into(), "VALUE".into());
    cfg.working_dir = Some(PathBuf::from("/home/agent"));
    let spec = cfg.to_spec();
    assert_eq!(spec.command, "python3");
    assert_eq!(spec.args, vec!["host.py"]);
    assert_eq!(spec.env["KEY"], "VALUE");
    assert!(spec.cwd.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Registry: register, duplicate, unregister
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    let cfg = SidecarConfig::new("alpha", "node");
    reg.register(cfg).unwrap();
    assert!(reg.get("alpha").is_some());
    assert_eq!(reg.get("alpha").unwrap().command, "node");
}

#[test]
fn registry_duplicate_registration_fails() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("alpha", "node")).unwrap();
    let result = reg.register(SidecarConfig::new("alpha", "python"));
    assert!(result.is_err());
}

#[test]
fn registry_remove_existing() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("beta", "bash")).unwrap();
    assert!(reg.remove("beta"));
    assert!(reg.get("beta").is_none());
}

#[test]
fn registry_remove_nonexistent() {
    let mut reg = SidecarRegistry::default();
    assert!(!reg.remove("ghost"));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Backend registry: list, lookup
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("charlie", "node")).unwrap();
    reg.register(SidecarConfig::new("alpha", "python")).unwrap();
    reg.register(SidecarConfig::new("bravo", "bash")).unwrap();
    let names = reg.list();
    assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
}

#[test]
fn registry_list_empty() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_lookup_missing_returns_none() {
    let reg = SidecarRegistry::default();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn registry_discover_from_dir() {
    let dir = tempfile::tempdir().unwrap();
    let node_dir = dir.path().join("my-node");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// node sidecar").unwrap();

    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.get("my-node").is_some());
    let cfg = reg.get("my-node").unwrap();
    assert_eq!(cfg.command, "node");
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Protocol state machine (lifecycle)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_initial_state_is_uninitialized() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_valid_full_sequence() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
}

#[test]
fn lifecycle_invalid_transition_detected() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Running, None);
    assert!(matches!(
        result,
        Err(LifecycleError::InvalidTransition { .. })
    ));
}

#[test]
fn lifecycle_already_in_state() {
    let mgr = LifecycleManager::new();
    // can't transition to same state
    let mut mgr = mgr;
    let result = mgr.transition(LifecycleState::Uninitialized, None);
    assert!(matches!(result, Err(LifecycleError::AlreadyInState(_))));
}

#[test]
fn lifecycle_failed_from_any_state() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Failed, Some("crashed".into()))
        .unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Failed);
}

#[test]
fn lifecycle_history_is_recorded() {
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
fn lifecycle_can_transition_checks() {
    let mgr = LifecycleManager::new();
    assert!(mgr.can_transition(&LifecycleState::Starting));
    assert!(!mgr.can_transition(&LifecycleState::Ready));
    assert!(!mgr.can_transition(&LifecycleState::Running));
    // Failed is always allowed
    assert!(mgr.can_transition(&LifecycleState::Failed));
}

#[test]
fn lifecycle_uptime_none_before_ready() {
    let mgr = LifecycleManager::new();
    assert!(mgr.uptime().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Process management concepts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_config_defaults() {
    let cfg = ProcessConfig::default();
    assert!(cfg.working_dir.is_none());
    assert!(cfg.env_vars.is_empty());
    assert!(cfg.timeout.is_none());
    assert!(cfg.inherit_env);
}

#[test]
fn process_info_initial_state() {
    let spec = SidecarSpec::new("node");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(!info.is_running());
    assert!(!info.is_terminated());
    assert!(info.started_at.is_none());
    assert!(info.ended_at.is_none());
}

#[test]
fn process_status_running_check() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 12345 };
    assert!(info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_status_exited_check() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn process_status_killed_check() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_status_timed_out_check() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

#[test]
fn process_config_custom_env_and_timeout() {
    let mut cfg = ProcessConfig::default();
    cfg.env_vars.insert("MY_VAR".into(), "my_val".into());
    cfg.timeout = Some(Duration::from_secs(30));
    cfg.inherit_env = false;
    assert_eq!(cfg.env_vars["MY_VAR"], "my_val");
    assert_eq!(cfg.timeout, Some(Duration::from_secs(30)));
    assert!(!cfg.inherit_env);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. JSONL codec: encode/decode envelopes, partial lines, large lines
// ═══════════════════════════════════════════════════════════════════════

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0".into()),
        adapter_version: None,
    }
}

#[test]
fn jsonl_encode_hello_ends_with_newline() {
    let env = Envelope::hello(test_backend(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"hello\""));
}

#[test]
fn jsonl_roundtrip_hello() {
    let env = Envelope::hello(test_backend(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn jsonl_roundtrip_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-42"));
            assert_eq!(error, "out of memory");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn jsonl_decode_invalid_json() {
    let result = JsonlCodec::decode("not valid json {{{");
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn jsonl_decode_stream_skips_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\n\n";
    let reader = BufReader::new(Cursor::new(input));
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn jsonl_decode_stream_empty_input() {
    let reader = BufReader::new(Cursor::new(""));
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn jsonl_large_payload_roundtrip() {
    let big_error = "x".repeat(100_000);
    let env = Envelope::Fatal {
        ref_id: None,
        error: big_error.clone(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100_000),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn jsonl_encode_to_writer() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("boom"));
}

#[test]
fn jsonl_encode_many_to_writer() {
    let envs = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "e1".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e2".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.lines().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Error handling: HostError & ProtocolError variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "nope"));
    let msg = err.to_string();
    assert!(msg.contains("spawn"));
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(msg_contains(&err, "stdout"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(msg_contains(&err, "stdin"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("bad message".into());
    assert!(msg_contains(&err, "violation"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("kaboom".into());
    assert!(msg_contains(&err, "kaboom"));
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(137) };
    let msg = err.to_string();
    assert!(msg.contains("137"));
}

#[test]
fn host_error_sidecar_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "segfault".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("segfault"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(5),
    };
    let msg = err.to_string();
    assert!(msg.contains("5"));
}

#[test]
fn protocol_error_violation_display() {
    let err = ProtocolError::Violation("bad state".into());
    assert!(err.to_string().contains("bad state"));
}

#[test]
fn protocol_error_unexpected_message_display() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("hello"));
    assert!(msg.contains("run"));
}

fn msg_contains(err: &HostError, needle: &str) -> bool {
    err.to_string().to_lowercase().contains(needle)
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Timeout / retry configuration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_config_defaults() {
    let cfg = RetryConfig::default();
    assert_eq!(cfg.max_retries, 3);
    assert_eq!(cfg.base_delay, Duration::from_millis(100));
    assert_eq!(cfg.max_delay, Duration::from_secs(10));
    assert_eq!(cfg.overall_timeout, Duration::from_secs(60));
    assert!((cfg.jitter_factor - 0.5).abs() < f64::EPSILON);
}

#[test]
fn retry_config_custom() {
    let cfg = RetryConfig {
        max_retries: 0,
        base_delay: Duration::from_millis(50),
        max_delay: Duration::from_secs(1),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    assert_eq!(cfg.max_retries, 0);
    assert_eq!(cfg.overall_timeout, Duration::from_secs(5));
}

#[test]
fn retry_config_zero_jitter_deterministic() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        ..RetryConfig::default()
    };
    let d0 = compute_delay(&cfg, 0);
    assert_eq!(d0, Duration::from_millis(100));
    let d1 = compute_delay(&cfg, 1);
    assert_eq!(d1, Duration::from_millis(200));
    let d2 = compute_delay(&cfg, 2);
    assert_eq!(d2, Duration::from_millis(400));
}

#[test]
fn retry_delay_capped_at_max() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_millis(2000),
        ..RetryConfig::default()
    };
    let d10 = compute_delay(&cfg, 10);
    assert!(d10 <= Duration::from_millis(2000));
}

#[test]
fn retry_metadata_to_receipt() {
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(500),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(3));
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(500u64));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Environment variable passthrough
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_config_env_passthrough() {
    let mut cfg = SidecarConfig::new("envtest", "node");
    cfg.env.insert("API_KEY".into(), "secret-123".into());
    cfg.env.insert("DEBUG".into(), "true".into());
    let spec = cfg.to_spec();
    assert_eq!(spec.env.len(), 2);
    assert_eq!(spec.env["API_KEY"], "secret-123");
    assert_eq!(spec.env["DEBUG"], "true");
}

#[test]
fn process_config_env_vars_passthrough() {
    let mut cfg = ProcessConfig::default();
    cfg.env_vars.insert("MY_VAR".into(), "hello".into());
    cfg.env_vars.insert("ANOTHER_VAR".into(), "world".into());
    assert_eq!(cfg.env_vars.len(), 2);
}

#[test]
fn sidecar_spec_env_btreemap_is_sorted() {
    let mut spec = SidecarSpec::new("test");
    spec.env.insert("ZZZ".into(), "last".into());
    spec.env.insert("AAA".into(), "first".into());
    spec.env.insert("MMM".into(), "middle".into());
    let keys: Vec<&String> = spec.env.keys().collect();
    assert_eq!(keys, vec!["AAA", "MMM", "ZZZ"]);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Capability negotiation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: test_backend(),
        capabilities: caps,
    };
    assert_eq!(hello.capabilities.len(), 2);
    assert!(matches!(
        hello.capabilities[&Capability::Streaming],
        SupportLevel::Native
    ));
    assert!(matches!(
        hello.capabilities[&Capability::ToolBash],
        SupportLevel::Emulated
    ));
}

#[test]
fn envelope_hello_includes_contract_version() {
    let env = Envelope::hello(test_backend(), CapabilityManifest::new());
    match env {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn envelope_hello_default_mode_is_mapped() {
    let env = Envelope::hello(test_backend(), CapabilityManifest::new());
    match env {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn envelope_hello_with_explicit_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        test_backend(),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    match env {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Contract version checking
// ═══════════════════════════════════════════════════════════════════════

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
    assert_eq!(parse_version(""), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/vx.y"), None);
}

#[test]
fn compatible_version_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.99"));
}

#[test]
fn incompatible_version_different_major() {
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
}

#[test]
fn incompatible_version_invalid_input() {
    assert!(!is_compatible_version("garbage", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
    assert!(!is_compatible_version("", ""));
}

#[test]
fn contract_version_is_parseable() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert!(parsed.is_some());
    let (major, minor) = parsed.unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Bonus: Pool management
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pool_add_acquire_release() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.state, PoolEntryState::Busy);
    pool.release(&entry.id);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_max_size_enforced() {
    let cfg = PoolConfig {
        max_size: 2,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(cfg);
    assert!(pool.add("a"));
    assert!(pool.add("b"));
    assert!(!pool.add("c"));
    assert_eq!(pool.total_count(), 2);
}

#[test]
fn pool_stats_utilization() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("x");
    pool.add("y");
    let _entry = pool.acquire().unwrap();
    let stats = pool.stats();
    assert_eq!(stats.total, 2);
    assert_eq!(stats.busy, 1);
    assert_eq!(stats.idle, 1);
    assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn pool_stats_empty_utilization_is_zero() {
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
fn pool_mark_failed_and_drain() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("f1");
    pool.add("d1");
    pool.mark_failed("f1");
    pool.drain("d1");
    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.draining, 1);
    assert_eq!(stats.idle, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// Bonus: Health monitoring
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn health_monitor_record_and_check() {
    let mut mon = HealthMonitor::new();
    mon.record_check("s1", HealthStatus::Healthy, Some(Duration::from_millis(10)));
    assert!(mon.all_healthy());
    assert_eq!(mon.total_checks(), 1);
    let check = mon.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_consecutive_failures() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    mon.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    let check = mon.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 2);
}

#[test]
fn health_monitor_report_overall() {
    let mut mon = HealthMonitor::new();
    mon.record_check("s1", HealthStatus::Healthy, None);
    mon.record_check(
        "s2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = mon.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
    assert_eq!(report.checks.len(), 2);
}

#[test]
fn health_uptime_percentage() {
    let mut mon = HealthMonitor::new();
    mon.record_check("s1", HealthStatus::Healthy, None);
    mon.record_check("s1", HealthStatus::Healthy, None);
    mon.record_check("s1", HealthStatus::Unhealthy { reason: "x".into() }, None);
    let pct = mon.uptime_percentage("s1");
    // 2 healthy out of 3
    assert!((pct - 66.66666666666667).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════════════════
// Bonus: Retryability classification
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retryable_errors_classified_correctly() {
    assert!(is_retryable(&HostError::Spawn(std::io::Error::other("e"))));
    assert!(is_retryable(&HostError::Stdout(std::io::Error::other("e"))));
    assert!(is_retryable(&HostError::Exited { code: Some(1) }));
    assert!(is_retryable(&HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: String::new(),
    }));
    assert!(is_retryable(&HostError::Timeout {
        duration: Duration::from_secs(1),
    }));
}

#[test]
fn non_retryable_errors_classified_correctly() {
    assert!(!is_retryable(&HostError::Violation("bad".into())));
    assert!(!is_retryable(&HostError::Fatal("boom".into())));
}

#[test]
fn protocol_error_from_host_error() {
    let pe = ProtocolError::Violation("test".into());
    let he = HostError::Protocol(pe);
    assert!(he.to_string().contains("protocol"));
}
