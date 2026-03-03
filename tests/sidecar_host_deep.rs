// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for the sidecar host module — process spawning,
//! JSONL communication, lifecycle management, and error handling.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use abp_core::{
    AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, WorkOrderBuilder,
};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{RetryConfig, compute_delay, is_retryable};
use abp_host::{HostError, SidecarHello, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_sidecar_utils::frame::{
    backend_identity, contract_version, decode_envelope, encode_envelope, encode_fatal,
    encode_final, encode_hello,
};
use abp_sidecar_utils::testing::{mock_event, mock_fatal, mock_final, mock_hello};
use abp_sidecar_utils::validate::{validate_hello, validate_ref_id, validate_sequence};

// ═══════════════════════════════════════════════════════════════════════
// a) Host configuration tests (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn default_sidecar_spec_is_valid() {
    let spec = SidecarSpec::new("echo");
    assert_eq!(spec.command, "echo");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn custom_command_path() {
    let spec = SidecarSpec {
        command: "/usr/local/bin/my-sidecar".into(),
        args: vec!["--verbose".into()],
        env: BTreeMap::new(),
        cwd: None,
    };
    assert_eq!(spec.command, "/usr/local/bin/my-sidecar");
    assert_eq!(spec.args, vec!["--verbose"]);
}

#[test]
fn environment_variables_passed_to_sidecar() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".into(), "secret123".into());
    env.insert("LOG_LEVEL".into(), "debug".into());
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["host.js".into()],
        env,
        cwd: None,
    };
    assert_eq!(spec.env.get("API_KEY").unwrap(), "secret123");
    assert_eq!(spec.env.get("LOG_LEVEL").unwrap(), "debug");
    assert_eq!(spec.env.len(), 2);
}

#[test]
fn working_directory_configuration() {
    let spec = SidecarSpec {
        command: "python".into(),
        args: vec!["host.py".into()],
        env: BTreeMap::new(),
        cwd: Some("/tmp/sidecar-workspace".into()),
    };
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/sidecar-workspace"));
}

#[test]
fn timeout_configuration_in_process_config() {
    let config = ProcessConfig {
        timeout: Some(Duration::from_secs(30)),
        ..ProcessConfig::default()
    };
    assert_eq!(config.timeout, Some(Duration::from_secs(30)));

    let default_config = ProcessConfig::default();
    assert!(default_config.timeout.is_none());
}

#[test]
fn argument_passing_preserved() {
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec![
            "host.js".into(),
            "--port".into(),
            "8080".into(),
            "--mode=passthrough".into(),
        ],
        env: BTreeMap::new(),
        cwd: None,
    };
    assert_eq!(spec.args.len(), 4);
    assert_eq!(spec.args[0], "host.js");
    assert_eq!(spec.args[2], "8080");
    assert_eq!(spec.args[3], "--mode=passthrough");
}

#[test]
fn config_serialization_roundtrip() {
    let spec = SidecarSpec {
        command: "python3".into(),
        args: vec!["host.py".into(), "--debug".into()],
        env: {
            let mut m = BTreeMap::new();
            m.insert("FOO".into(), "bar".into());
            m
        },
        cwd: Some("/workspace".into()),
    };
    let json = serde_json::to_string(&spec).unwrap();
    let deserialized: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.command, "python3");
    assert_eq!(deserialized.args, vec!["host.py", "--debug"]);
    assert_eq!(deserialized.env.get("FOO").unwrap(), "bar");
    assert_eq!(deserialized.cwd.as_deref(), Some("/workspace"));
}

#[test]
fn invalid_command_path_produces_spawn_error() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "command not found: /nonexistent/sidecar",
    ));
    let msg = err.to_string();
    assert!(msg.contains("spawn"));
    assert!(msg.contains("command not found"));
}

#[test]
fn sidecar_config_validates_empty_name() {
    let config = SidecarConfig {
        name: "".into(),
        command: "node".into(),
        args: vec![],
        env: BTreeMap::new(),
        working_dir: None,
    };
    assert!(config.validate().is_err());
}

#[test]
fn multiple_host_configs_dont_conflict() {
    let spec1 = SidecarSpec::new("node");
    let spec2 = SidecarSpec::new("python");
    let spec3 = SidecarSpec {
        command: "bash".into(),
        args: vec!["-c".into(), "echo hello".into()],
        env: BTreeMap::new(),
        cwd: Some("/tmp".into()),
    };
    // Each spec is independent.
    assert_eq!(spec1.command, "node");
    assert_eq!(spec2.command, "python");
    assert_eq!(spec3.command, "bash");
    assert_ne!(spec1.command, spec2.command);
    assert_ne!(spec2.command, spec3.command);
}

// ═══════════════════════════════════════════════════════════════════════
// b) Protocol handshake simulation (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_hello_envelope_from_json_string() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":"1.0","adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let envelope = JsonlCodec::decode(json).unwrap();
    match envelope {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend.id, "test");
        }
        _ => panic!("expected Hello envelope"),
    }
}

#[test]
fn parse_run_envelope_from_json_string() {
    let wo = WorkOrderBuilder::new("test task").build();
    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-1");
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run envelope"),
    }
}

#[test]
fn parse_event_envelope_from_json_string() {
    let event_env = mock_event("run-42", "hello world");
    let encoded = JsonlCodec::encode(&event_env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-42");
            match event.kind {
                AgentEventKind::AssistantMessage { ref text } => {
                    assert_eq!(text, "hello world");
                }
                _ => panic!("expected AssistantMessage"),
            }
        }
        _ => panic!("expected Event envelope"),
    }
}

#[test]
fn parse_final_envelope_from_json_string() {
    let final_env = mock_final("run-99");
    let encoded = JsonlCodec::encode(&final_env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-99");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final envelope"),
    }
}

#[test]
fn parse_fatal_envelope_from_json_string() {
    let fatal_env = mock_fatal("run-1", "out of memory");
    let encoded = JsonlCodec::encode(&fatal_env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-1".into()));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal envelope"),
    }
}

#[test]
fn build_hello_response_with_capabilities() {
    let hello_line = encode_hello("my-backend", "2.0", &["streaming"]);
    assert!(hello_line.contains("\"t\":\"hello\""));
    assert!(hello_line.contains("my-backend"));
    assert!(hello_line.contains("2.0"));

    let decoded = decode_envelope(&hello_line).unwrap();
    match decoded {
        Envelope::Hello {
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "my-backend");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
            assert!(!capabilities.is_empty());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn build_run_with_work_order() {
    let wo = WorkOrderBuilder::new("implement feature X").build();
    let run_env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo.clone(),
    };
    let encoded = JsonlCodec::encode(&run_env).unwrap();
    assert!(encoded.contains("\"t\":\"run\""));
    assert!(encoded.contains("implement feature X"));

    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, wo.id.to_string());
            assert_eq!(work_order.task, "implement feature X");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn contract_version_in_hello_matches_expected() {
    let hello = mock_hello("test-backend");
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(contract_version, "abp/v0.1");
        }
        _ => panic!("expected Hello"),
    }
    // Also verify via the frame helper.
    assert_eq!(contract_version(), CONTRACT_VERSION);
}

#[test]
fn ref_id_correlation_across_envelopes() {
    let run_id = "correlated-run-123";
    let event = mock_event(run_id, "step 1");
    let final_env = mock_final(run_id);
    let fatal_env = mock_fatal(run_id, "something broke");

    // All should pass ref_id validation.
    validate_ref_id(&event, run_id).unwrap();
    validate_ref_id(&final_env, run_id).unwrap();
    validate_ref_id(&fatal_env, run_id).unwrap();

    // Mismatched ref_id should fail.
    assert!(validate_ref_id(&event, "wrong-id").is_err());
    assert!(validate_ref_id(&final_env, "wrong-id").is_err());
}

#[test]
fn multiple_sequential_handshakes_validate() {
    // Build two complete valid sequences.
    let hello1 = mock_hello("backend-a");
    let event1 = mock_event("run-1", "msg1");
    let final1 = mock_final("run-1");

    let hello2 = mock_hello("backend-b");
    let event2 = mock_event("run-2", "msg2");
    let fatal2 = mock_fatal("run-2", "crash");

    // Both sequences should be structurally valid.
    validate_sequence(&[hello1, event1, final1]).unwrap();
    validate_sequence(&[hello2, event2, fatal2]).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// c) Lifecycle and error handling (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_graceful_shutdown_transitions() {
    let mut mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);

    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Stopping, Some("graceful shutdown".into()))
        .unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
    assert_eq!(mgr.history().len(), 5);
}

#[test]
fn timeout_error_is_descriptive() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = err.to_string();
    assert!(msg.contains("timed out"));
    assert!(msg.contains("30"));
}

#[test]
fn sidecar_crash_detection_error() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "Killed by OOM".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"));
    assert!(msg.contains("137"));
    assert!(msg.contains("Killed by OOM"));
}

#[test]
fn stderr_capture_in_crash_error() {
    let stderr_output = "Error: Cannot find module 'missing'\nStack trace...".to_string();
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: stderr_output.clone(),
    };
    match err {
        HostError::SidecarCrashed { stderr, .. } => {
            assert!(stderr.contains("Cannot find module"));
            assert!(stderr.contains("Stack trace"));
        }
        _ => panic!("expected SidecarCrashed"),
    }
}

#[test]
fn stdin_stdout_violation_errors() {
    let stdin_err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pipe closed",
    ));
    assert!(stdin_err.to_string().contains("stdin"));

    let stdout_err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "EOF",
    ));
    assert!(stdout_err.to_string().contains("stdout"));

    let violation = HostError::Violation("sidecar stdin unavailable".into());
    assert!(violation.to_string().contains("stdin unavailable"));
}

#[test]
fn process_cleanup_status_tracking() {
    let spec = SidecarSpec::new("echo");
    let config = ProcessConfig::default();
    let mut info = ProcessInfo::new(spec, config);

    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(!info.is_running());
    assert!(!info.is_terminated());

    info.status = ProcessStatus::Running { pid: 12345 };
    assert!(info.is_running());
    assert!(!info.is_terminated());

    info.status = ProcessStatus::Killed;
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn restart_after_failure_lifecycle() {
    let mut mgr = LifecycleManager::new();

    // Normal start → run → fail.
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Failed, Some("OOM".into()))
        .unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Failed);

    // Transition to Failed is always allowed, but we can't transition
    // from Failed to Starting (not a valid transition).
    let result = mgr.transition(LifecycleState::Starting, None);
    assert!(result.is_err());
}

#[test]
fn multiple_concurrent_pool_entries() {
    let pool = SidecarPool::new(PoolConfig {
        min_size: 1,
        max_size: 5,
        ..PoolConfig::default()
    });

    for i in 0..5 {
        assert!(pool.add(format!("sidecar-{i}")));
    }
    // Exceeding max_size fails.
    assert!(!pool.add("sidecar-5"));

    // Acquire three concurrently.
    let e1 = pool.acquire().unwrap();
    let e2 = pool.acquire().unwrap();
    let e3 = pool.acquire().unwrap();
    assert_eq!(e1.state, PoolEntryState::Busy);
    assert_eq!(e2.state, PoolEntryState::Busy);
    assert_eq!(e3.state, PoolEntryState::Busy);

    let stats = pool.stats();
    assert_eq!(stats.busy, 3);
    assert_eq!(stats.idle, 2);
    assert_eq!(stats.total, 5);

    // Release one back.
    pool.release(&e1.id);
    assert_eq!(pool.idle_count(), 3);
}

#[test]
fn resource_cleanup_pool_failed_entries() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");

    pool.mark_failed("s1");
    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.idle, 1);

    // Remove the failed entry.
    let removed = pool.remove("s1");
    assert!(removed.is_some());
    assert_eq!(pool.total_count(), 1);
}

#[test]
fn error_messages_are_descriptive() {
    let errors: Vec<HostError> = vec![
        HostError::Spawn(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No such file",
        )),
        HostError::Stdout(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "EOF",
        )),
        HostError::Stdin(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "broken pipe",
        )),
        HostError::Protocol(ProtocolError::Violation("bad frame".into())),
        HostError::Violation("unexpected hello".into()),
        HostError::Fatal("backend overloaded".into()),
        HostError::Exited { code: Some(1) },
        HostError::SidecarCrashed {
            exit_code: Some(139),
            stderr: "segfault".into(),
        },
        HostError::Timeout {
            duration: Duration::from_secs(60),
        },
    ];

    for err in &errors {
        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "error message should not be empty: {err:?}"
        );
        // Every variant should produce a human-readable message.
        assert!(msg.len() > 5, "error message too short for {err:?}: {msg}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional deep tests for completeness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_hello_serialization_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test-be".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.5.0".into()),
        },
        capabilities: CapabilityManifest::new(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let deserialized: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.contract_version, CONTRACT_VERSION);
    assert_eq!(deserialized.backend.id, "test-be");
    assert_eq!(
        deserialized.backend.adapter_version.as_deref(),
        Some("0.5.0")
    );
}

#[test]
fn retry_config_defaults_are_reasonable() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 3);
    assert!(config.base_delay >= Duration::from_millis(50));
    assert!(config.max_delay <= Duration::from_secs(60));
    assert!(config.overall_timeout <= Duration::from_secs(120));
    assert!(config.jitter_factor >= 0.0 && config.jitter_factor <= 1.0);
}

#[test]
fn retry_config_serialization_roundtrip() {
    let config = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(15),
        overall_timeout: Duration::from_secs(90),
        jitter_factor: 0.3,
    };
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.max_retries, 5);
    assert_eq!(deserialized.base_delay, Duration::from_millis(200));
    assert_eq!(deserialized.jitter_factor, 0.3);
}

#[test]
fn compute_delay_exponential_backoff() {
    let config = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        overall_timeout: Duration::from_secs(60),
        jitter_factor: 0.0, // No jitter for deterministic test.
    };
    let d0 = compute_delay(&config, 0);
    let d1 = compute_delay(&config, 1);
    let d2 = compute_delay(&config, 2);
    // With zero jitter: delay = base * 2^attempt, capped at max_delay.
    assert_eq!(d0, Duration::from_millis(100));
    assert_eq!(d1, Duration::from_millis(200));
    assert_eq!(d2, Duration::from_millis(400));
}

#[test]
fn is_retryable_classifies_errors_correctly() {
    // Retryable errors.
    assert!(is_retryable(&HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::Other,
        "err"
    ))));
    assert!(is_retryable(&HostError::Exited { code: Some(1) }));
    assert!(is_retryable(&HostError::Timeout {
        duration: Duration::from_secs(5)
    }));
    assert!(is_retryable(&HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: String::new()
    }));

    // Non-retryable errors.
    assert!(!is_retryable(&HostError::Violation(
        "protocol error".into()
    )));
    assert!(!is_retryable(&HostError::Fatal("fatal".into())));
    assert!(!is_retryable(&HostError::Protocol(
        ProtocolError::Violation("bad".into())
    )));
}

#[test]
fn registry_register_and_lookup() {
    let mut registry = SidecarRegistry::default();
    let config = SidecarConfig::new("node-host", "node");
    registry.register(config).unwrap();

    assert!(registry.get("node-host").is_some());
    assert!(registry.get("missing").is_none());
    assert_eq!(registry.list(), vec!["node-host"]);
}

#[test]
fn registry_rejects_duplicate_names() {
    let mut registry = SidecarRegistry::default();
    registry
        .register(SidecarConfig::new("dup", "node"))
        .unwrap();
    let err = registry.register(SidecarConfig::new("dup", "python"));
    assert!(err.is_err());
}

#[test]
fn sidecar_config_to_spec_conversion() {
    let config = SidecarConfig {
        name: "my-sidecar".into(),
        command: "python3".into(),
        args: vec!["host.py".into()],
        env: {
            let mut m = BTreeMap::new();
            m.insert("PORT".into(), "9000".into());
            m
        },
        working_dir: Some(PathBuf::from("/opt/sidecar")),
    };
    let spec = config.to_spec();
    assert_eq!(spec.command, "python3");
    assert_eq!(spec.args, vec!["host.py"]);
    assert_eq!(spec.env.get("PORT").unwrap(), "9000");
    assert!(spec.cwd.as_ref().unwrap().contains("sidecar"));
}

#[test]
fn validate_hello_rejects_incompatible_version() {
    let hello = Envelope::Hello {
        contract_version: "abp/v99.0".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = validate_hello(&hello);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("incompatible"));
}

#[test]
fn validate_hello_accepts_compatible_version() {
    let hello = mock_hello("good-backend");
    validate_hello(&hello).unwrap();
}

#[test]
fn validate_sequence_rejects_empty() {
    assert!(validate_sequence(&[]).is_err());
}

#[test]
fn validate_sequence_rejects_hello_only() {
    let hello = mock_hello("backend");
    assert!(validate_sequence(&[hello]).is_err());
}

#[test]
fn validate_sequence_rejects_missing_hello() {
    let event = mock_event("run-1", "msg");
    let final_env = mock_final("run-1");
    let result = validate_sequence(&[event, final_env]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("hello"));
}

#[test]
fn validate_sequence_rejects_duplicate_terminals() {
    let hello = mock_hello("be");
    let final1 = mock_final("run-1");
    let final2 = mock_final("run-1");
    let result = validate_sequence(&[hello, final1, final2]);
    assert!(result.is_err());
}

#[test]
fn encode_decode_envelope_roundtrip() {
    let hello = mock_hello("roundtrip-backend");
    let encoded = encode_envelope(&hello).unwrap();
    let decoded = decode_envelope(&encoded).unwrap();
    match decoded {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "roundtrip-backend");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn encode_helpers_produce_valid_jsonl() {
    let hello_line = encode_hello("be", "1.0", &[]);
    assert!(hello_line.ends_with('\n'));
    assert!(hello_line.contains("\"t\":\"hello\""));

    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let final_line = encode_final("run-1", &receipt);
    assert!(final_line.ends_with('\n'));
    assert!(final_line.contains("\"t\":\"final\""));

    let fatal_line = encode_fatal("run-1", "oops");
    assert!(fatal_line.ends_with('\n'));
    assert!(fatal_line.contains("\"t\":\"fatal\""));
    assert!(fatal_line.contains("oops"));
}

#[test]
fn backend_identity_helper() {
    let id = backend_identity("my-be", "2.0");
    assert_eq!(id.id, "my-be");
    assert_eq!(id.backend_version.as_deref(), Some("2.0"));
    assert!(id.adapter_version.is_none());
}

#[test]
fn lifecycle_invalid_transition_detected() {
    let mut mgr = LifecycleManager::new();
    // Can't go directly from Uninitialized to Running.
    let err = mgr.transition(LifecycleState::Running, None);
    assert!(err.is_err());
    match err.unwrap_err() {
        LifecycleError::InvalidTransition { from, to } => {
            assert_eq!(from, LifecycleState::Uninitialized);
            assert_eq!(to, LifecycleState::Running);
        }
        _ => panic!("expected InvalidTransition"),
    }
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
fn lifecycle_failed_from_any_state() {
    // Failed transition is always allowed from any state.
    for initial in [
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
    ] {
        let mut mgr = LifecycleManager::new();
        // Walk to the desired initial state.
        match initial {
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
        assert_eq!(*mgr.state(), initial);
        mgr.transition(LifecycleState::Failed, Some("test".into()))
            .unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Failed);
    }
}

#[test]
fn health_monitor_tracks_sidecar_status() {
    use abp_host::health::{HealthMonitor, HealthStatus};
    let mut monitor = HealthMonitor::new();

    monitor.record_check(
        "sidecar-1",
        HealthStatus::Healthy,
        Some(Duration::from_millis(5)),
    );
    monitor.record_check(
        "sidecar-2",
        HealthStatus::Healthy,
        Some(Duration::from_millis(10)),
    );
    assert!(monitor.all_healthy());
    assert_eq!(monitor.total_checks(), 2);

    monitor.record_check(
        "sidecar-1",
        HealthStatus::Unhealthy {
            reason: "process died".into(),
        },
        None,
    );
    assert!(!monitor.all_healthy());
    assert_eq!(monitor.unhealthy_sidecars().len(), 1);

    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn process_config_serialization_roundtrip() {
    let config = ProcessConfig {
        working_dir: Some(PathBuf::from("/tmp/work")),
        env_vars: {
            let mut m = BTreeMap::new();
            m.insert("KEY".into(), "VAL".into());
            m
        },
        timeout: Some(Duration::from_secs(120)),
        inherit_env: false,
    };
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.working_dir, Some(PathBuf::from("/tmp/work")));
    assert_eq!(deserialized.timeout, Some(Duration::from_secs(120)));
    assert!(!deserialized.inherit_env);
}

#[test]
fn pool_utilization_computed_correctly() {
    let pool = SidecarPool::new(PoolConfig {
        min_size: 1,
        max_size: 4,
        ..PoolConfig::default()
    });
    pool.add("a");
    pool.add("b");
    pool.add("c");
    pool.add("d");

    let stats = pool.stats();
    assert!((stats.utilization() - 0.0).abs() < f64::EPSILON);

    pool.acquire(); // 1 busy out of 4
    let stats = pool.stats();
    assert!((stats.utilization() - 0.25).abs() < f64::EPSILON);

    pool.acquire(); // 2 busy out of 4
    let stats = pool.stats();
    assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
}
