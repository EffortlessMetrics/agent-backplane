// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive error handling tests for all workspace error types.
//!
//! Validates Display messages, Debug output, Send/Sync bounds, error chains,
//! downcast behavior, exhaustive variant coverage, and stable error codes.

use std::error::Error;
use std::time::Duration;

// ---------------------------------------------------------------------------
// 1. HostError: each variant has a unique Display message
// ---------------------------------------------------------------------------

#[test]
fn host_error_variants_have_unique_display() {
    use abp_host::HostError;
    use abp_protocol::ProtocolError;

    let variants: Vec<HostError> = vec![
        HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "cmd")),
        HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe")),
        HostError::Stdin(std::io::Error::new(std::io::ErrorKind::WriteZero, "zero")),
        HostError::Protocol(ProtocolError::Violation("v".into())),
        HostError::Violation("bad state".into()),
        HostError::Fatal("oom".into()),
        HostError::Exited { code: Some(1) },
        HostError::SidecarCrashed {
            exit_code: Some(137),
            stderr: "killed by signal".into(),
        },
        HostError::Timeout {
            duration: Duration::from_secs(30),
        },
    ];

    let messages: Vec<String> = variants.iter().map(|e| e.to_string()).collect();
    let unique: std::collections::HashSet<&String> = messages.iter().collect();
    assert_eq!(
        unique.len(),
        messages.len(),
        "duplicate Display messages found: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// 2. HostError: Display messages contain useful context
// ---------------------------------------------------------------------------

#[test]
fn host_error_display_contains_context() {
    use abp_host::HostError;

    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by signal".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("137"), "should contain exit code: {msg}");
    assert!(
        msg.contains("killed by signal"),
        "should contain stderr: {msg}"
    );

    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = err.to_string();
    assert!(msg.contains("30"), "should contain duration: {msg}");

    let err = HostError::Exited { code: Some(42) };
    let msg = err.to_string();
    assert!(msg.contains("42"), "should contain exit code: {msg}");
}

// ---------------------------------------------------------------------------
// 3. HostError: implements std::error::Error
// ---------------------------------------------------------------------------

#[test]
fn host_error_implements_std_error() {
    use abp_host::HostError;

    let err = HostError::Fatal("test".into());
    let _: &dyn Error = &err;
}

// ---------------------------------------------------------------------------
// 4. HostError: Send + Sync
// ---------------------------------------------------------------------------

#[test]
fn host_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<abp_host::HostError>();
}

// ---------------------------------------------------------------------------
// 5. HostError: source() chains for wrapping variants
// ---------------------------------------------------------------------------

#[test]
fn host_error_source_chains() {
    use abp_host::HostError;

    let err = HostError::Spawn(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Spawn should have a source");

    let err = HostError::Stdout(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Stdout should have a source");

    let err = HostError::Stdin(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Stdin should have a source");

    let err = HostError::Protocol(abp_protocol::ProtocolError::Violation("v".into()));
    assert!(err.source().is_some(), "Protocol should have a source");

    // Plain-string variants have no source.
    let err = HostError::Violation("v".into());
    assert!(err.source().is_none());

    let err = HostError::Fatal("f".into());
    assert!(err.source().is_none());

    let err = HostError::Exited { code: Some(1) };
    assert!(err.source().is_none());

    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "err".into(),
    };
    assert!(err.source().is_none());

    let err = HostError::Timeout {
        duration: Duration::from_secs(5),
    };
    assert!(err.source().is_none());
}

// ---------------------------------------------------------------------------
// 6. HostError: exhaustive variant coverage
// ---------------------------------------------------------------------------

#[test]
fn host_error_exhaustive_variants() {
    use abp_host::HostError;
    use abp_protocol::ProtocolError;

    let variants: Vec<HostError> = vec![
        HostError::Spawn(std::io::Error::other("x")),
        HostError::Stdout(std::io::Error::other("x")),
        HostError::Stdin(std::io::Error::other("x")),
        HostError::Protocol(ProtocolError::Violation("x".into())),
        HostError::Violation("x".into()),
        HostError::Fatal("x".into()),
        HostError::Exited { code: None },
        HostError::SidecarCrashed {
            exit_code: None,
            stderr: String::new(),
        },
        HostError::Timeout {
            duration: Duration::from_secs(1),
        },
    ];

    for v in &variants {
        match v {
            HostError::Spawn(_) => {}
            HostError::Stdout(_) => {}
            HostError::Stdin(_) => {}
            HostError::Protocol(_) => {}
            HostError::Violation(_) => {}
            HostError::Fatal(_) => {}
            HostError::Exited { .. } => {}
            HostError::SidecarCrashed { .. } => {}
            HostError::Timeout { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// 7. HostError: Debug output is useful
// ---------------------------------------------------------------------------

#[test]
fn host_error_debug_output() {
    use abp_host::HostError;

    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "segfault".into(),
    };
    let debug = format!("{err:?}");
    assert!(
        debug.contains("SidecarCrashed"),
        "Debug should name variant: {debug}"
    );
    assert!(
        debug.contains("137"),
        "Debug should contain exit_code: {debug}"
    );

    let err = HostError::Timeout {
        duration: Duration::from_millis(500),
    };
    let debug = format!("{err:?}");
    assert!(
        debug.contains("Timeout"),
        "Debug should name variant: {debug}"
    );
}

// ---------------------------------------------------------------------------
// 8. HostError: downcast works
// ---------------------------------------------------------------------------

#[test]
fn host_error_downcast() {
    use abp_host::HostError;

    let err: Box<dyn Error> = Box::new(HostError::Fatal("test".into()));
    let downcasted = err.downcast_ref::<HostError>();
    assert!(downcasted.is_some(), "downcast to HostError should work");
}

// ---------------------------------------------------------------------------
// 9. ProtocolError: exhaustive variants
// ---------------------------------------------------------------------------

#[test]
fn protocol_error_exhaustive_variants() {
    use abp_protocol::ProtocolError;

    let json_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let variants: Vec<ProtocolError> = vec![
        ProtocolError::Json(json_err),
        ProtocolError::Io(std::io::Error::other("x")),
        ProtocolError::Violation("x".into()),
        ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        },
    ];

    for v in &variants {
        match v {
            ProtocolError::Json(_) => {}
            ProtocolError::Io(_) => {}
            ProtocolError::Violation(_) => {}
            ProtocolError::UnexpectedMessage { .. } => {}
            ProtocolError::Abp(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// 10. ProtocolError: Send + Sync
// ---------------------------------------------------------------------------

#[test]
fn protocol_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<abp_protocol::ProtocolError>();
}

// ---------------------------------------------------------------------------
// 11. ProtocolError: unique Display messages
// ---------------------------------------------------------------------------

#[test]
fn protocol_error_unique_display() {
    use abp_protocol::ProtocolError;

    let json_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let variants: Vec<ProtocolError> = vec![
        ProtocolError::Json(json_err),
        ProtocolError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe")),
        ProtocolError::Violation("test violation".into()),
        ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        },
    ];

    let messages: Vec<String> = variants.iter().map(|e| e.to_string()).collect();
    let unique: std::collections::HashSet<&String> = messages.iter().collect();
    assert_eq!(
        unique.len(),
        messages.len(),
        "duplicate Display: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// 12. ProtocolError: source chains
// ---------------------------------------------------------------------------

#[test]
fn protocol_error_source_chains() {
    use abp_protocol::ProtocolError;

    let err = ProtocolError::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
    assert!(err.source().is_some(), "Json should chain source");

    let err = ProtocolError::Io(std::io::Error::other("x"));
    assert!(err.source().is_some(), "Io should chain source");

    let err = ProtocolError::Violation("v".into());
    assert!(err.source().is_none());

    let err = ProtocolError::UnexpectedMessage {
        expected: "a".into(),
        got: "b".into(),
    };
    assert!(err.source().is_none());
}

// ---------------------------------------------------------------------------
// 13. RuntimeError: exhaustive variants
// ---------------------------------------------------------------------------

#[test]
fn runtime_error_exhaustive_variants() {
    use abp_runtime::RuntimeError;

    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend {
            name: "test".into(),
        },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("ws")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("pol")),
        RuntimeError::BackendFailed(anyhow::anyhow!("be")),
        RuntimeError::CapabilityCheckFailed("cap".into()),
    ];

    for v in &variants {
        match v {
            RuntimeError::UnknownBackend { .. } => {}
            RuntimeError::WorkspaceFailed(_) => {}
            RuntimeError::PolicyFailed(_) => {}
            RuntimeError::BackendFailed(_) => {}
            RuntimeError::CapabilityCheckFailed(_) => {}
            RuntimeError::Classified(_) => {}
            RuntimeError::NoProjectionMatch { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// 14. RuntimeError: unique Display
// ---------------------------------------------------------------------------

#[test]
fn runtime_error_unique_display() {
    use abp_runtime::RuntimeError;

    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "foo".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("temp dir")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob")),
        RuntimeError::BackendFailed(anyhow::anyhow!("timeout")),
        RuntimeError::CapabilityCheckFailed("missing streaming".into()),
    ];

    let messages: Vec<String> = variants.iter().map(|e| e.to_string()).collect();
    let unique: std::collections::HashSet<&String> = messages.iter().collect();
    assert_eq!(
        unique.len(),
        messages.len(),
        "duplicate Display: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// 15. RuntimeError: Display contains backend context
// ---------------------------------------------------------------------------

#[test]
fn runtime_error_display_contains_context() {
    use abp_runtime::RuntimeError;

    let err = RuntimeError::UnknownBackend {
        name: "my-backend".into(),
    };
    assert!(
        err.to_string().contains("my-backend"),
        "should contain backend name: {}",
        err
    );

    let err = RuntimeError::CapabilityCheckFailed("missing streaming for claude".into());
    assert!(
        err.to_string().contains("streaming"),
        "should contain capability: {}",
        err
    );
}

// ---------------------------------------------------------------------------
// 16. RuntimeError: Send + Sync
// ---------------------------------------------------------------------------

#[test]
fn runtime_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<abp_runtime::RuntimeError>();
}

// ---------------------------------------------------------------------------
// 17. RuntimeError: source chains
// ---------------------------------------------------------------------------

#[test]
fn runtime_error_source_chains() {
    use abp_runtime::RuntimeError;

    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(err.source().is_none());

    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("boom"));
    assert!(err.source().is_some(), "WorkspaceFailed should chain");

    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
    assert!(err.source().is_some(), "PolicyFailed should chain");

    let err = RuntimeError::BackendFailed(anyhow::anyhow!("fail"));
    assert!(err.source().is_some(), "BackendFailed should chain");

    let err = RuntimeError::CapabilityCheckFailed("x".into());
    assert!(err.source().is_none());
}

// ---------------------------------------------------------------------------
// 18. RuntimeError: downcast works
// ---------------------------------------------------------------------------

#[test]
fn runtime_error_downcast() {
    use abp_runtime::RuntimeError;

    let err: Box<dyn Error> = Box::new(RuntimeError::UnknownBackend {
        name: "test".into(),
    });
    assert!(err.downcast_ref::<RuntimeError>().is_some());
}

// ---------------------------------------------------------------------------
// 19. RuntimeError: Debug output is useful
// ---------------------------------------------------------------------------

#[test]
fn runtime_error_debug_is_useful() {
    use abp_runtime::RuntimeError;

    let err = RuntimeError::UnknownBackend {
        name: "sidecar:node".into(),
    };
    let debug = format!("{err:?}");
    assert!(
        debug.contains("UnknownBackend"),
        "should name variant: {debug}"
    );
    assert!(
        debug.contains("sidecar:node"),
        "should contain backend: {debug}"
    );
}

// ---------------------------------------------------------------------------
// 20. MappingError: exhaustive variants
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_exhaustive_variants() {
    use abp_core::error::MappingError;

    let variants: Vec<MappingError> = vec![
        MappingError::FidelityLoss {
            field: "max_tokens".into(),
            source_dialect: "claude".into(),
            target_dialect: "openai".into(),
            detail: "range differs".into(),
        },
        MappingError::UnsupportedCapability {
            capability: "mcp".into(),
            dialect: "openai".into(),
        },
        MappingError::EmulationRequired {
            feature: "tool_use".into(),
            detail: "synthetic".into(),
        },
        MappingError::IncompatibleModel {
            requested: "claude-4".into(),
            dialect: "openai".into(),
            suggestion: Some("gpt-4".into()),
        },
        MappingError::ParameterNotMappable {
            parameter: "temperature".into(),
            value: "2.0".into(),
            dialect: "gemini".into(),
        },
        MappingError::StreamingUnsupported {
            dialect: "batch-only".into(),
        },
    ];

    for v in &variants {
        match v {
            MappingError::FidelityLoss { .. } => {}
            MappingError::UnsupportedCapability { .. } => {}
            MappingError::EmulationRequired { .. } => {}
            MappingError::IncompatibleModel { .. } => {}
            MappingError::ParameterNotMappable { .. } => {}
            MappingError::StreamingUnsupported { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// 21. MappingError: unique Display
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_unique_display() {
    use abp_core::error::MappingError;

    let variants: Vec<MappingError> = vec![
        MappingError::FidelityLoss {
            field: "f".into(),
            source_dialect: "a".into(),
            target_dialect: "b".into(),
            detail: "d".into(),
        },
        MappingError::UnsupportedCapability {
            capability: "c".into(),
            dialect: "d".into(),
        },
        MappingError::EmulationRequired {
            feature: "e".into(),
            detail: "d".into(),
        },
        MappingError::IncompatibleModel {
            requested: "m".into(),
            dialect: "d".into(),
            suggestion: None,
        },
        MappingError::ParameterNotMappable {
            parameter: "p".into(),
            value: "v".into(),
            dialect: "d".into(),
        },
        MappingError::StreamingUnsupported {
            dialect: "s".into(),
        },
    ];

    let messages: Vec<String> = variants.iter().map(|e| e.to_string()).collect();
    let unique: std::collections::HashSet<&String> = messages.iter().collect();
    assert_eq!(
        unique.len(),
        messages.len(),
        "duplicate Display: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// 22. MappingError: stable error codes
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_stable_codes() {
    use abp_core::error::MappingError;

    assert_eq!(MappingError::FIDELITY_LOSS_CODE, "ABP_E_FIDELITY_LOSS");
    assert_eq!(MappingError::UNSUPPORTED_CAP_CODE, "ABP_E_UNSUPPORTED_CAP");
    assert_eq!(
        MappingError::EMULATION_REQUIRED_CODE,
        "ABP_E_EMULATION_REQUIRED"
    );
    assert_eq!(
        MappingError::INCOMPATIBLE_MODEL_CODE,
        "ABP_E_INCOMPATIBLE_MODEL"
    );
    assert_eq!(
        MappingError::PARAM_NOT_MAPPABLE_CODE,
        "ABP_E_PARAM_NOT_MAPPABLE"
    );
    assert_eq!(
        MappingError::STREAMING_UNSUPPORTED_CODE,
        "ABP_E_STREAMING_UNSUPPORTED"
    );

    // Verify code() returns matching constant.
    let err = MappingError::FidelityLoss {
        field: "f".into(),
        source_dialect: "a".into(),
        target_dialect: "b".into(),
        detail: "d".into(),
    };
    assert_eq!(err.code(), MappingError::FIDELITY_LOSS_CODE);

    let err = MappingError::StreamingUnsupported {
        dialect: "x".into(),
    };
    assert_eq!(err.code(), MappingError::STREAMING_UNSUPPORTED_CODE);
}

// ---------------------------------------------------------------------------
// 23. MappingError: Display contains context fields
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_display_contains_context() {
    use abp_core::error::MappingError;

    let err = MappingError::FidelityLoss {
        field: "max_tokens".into(),
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        detail: "range mismatch".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("max_tokens"), "should contain field: {msg}");
    assert!(msg.contains("claude"), "should contain source: {msg}");
    assert!(msg.contains("openai"), "should contain target: {msg}");
    assert!(
        msg.contains(MappingError::FIDELITY_LOSS_CODE),
        "should contain code: {msg}"
    );

    let err = MappingError::UnsupportedCapability {
        capability: "mcp_client".into(),
        dialect: "gemini".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("mcp_client"),
        "should contain capability: {msg}"
    );
    assert!(msg.contains("gemini"), "should contain dialect: {msg}");

    let err = MappingError::IncompatibleModel {
        requested: "claude-opus".into(),
        dialect: "openai".into(),
        suggestion: Some("gpt-4o".into()),
    };
    let msg = err.to_string();
    assert!(msg.contains("claude-opus"), "should contain model: {msg}");
    assert!(msg.contains("gpt-4o"), "should contain suggestion: {msg}");
}

// ---------------------------------------------------------------------------
// 24. MappingError: Send + Sync
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<abp_core::error::MappingError>();
}

// ---------------------------------------------------------------------------
// 25. MappingError: implements std::error::Error
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_implements_std_error() {
    use abp_core::error::MappingError;

    let err = MappingError::StreamingUnsupported {
        dialect: "x".into(),
    };
    let _: &dyn Error = &err;
}

// ---------------------------------------------------------------------------
// 26. Cross-type: all error types are dyn Error + Send + Sync
// ---------------------------------------------------------------------------

#[test]
fn all_errors_box_dyn_error_send_sync() {
    let errors: Vec<Box<dyn Error + Send + Sync>> = vec![
        Box::new(abp_host::HostError::Fatal("test".into())),
        Box::new(abp_host::HostError::SidecarCrashed {
            exit_code: Some(1),
            stderr: "err".into(),
        }),
        Box::new(abp_host::HostError::Timeout {
            duration: Duration::from_secs(1),
        }),
        Box::new(abp_protocol::ProtocolError::Violation("v".into())),
        Box::new(abp_runtime::RuntimeError::UnknownBackend { name: "x".into() }),
        Box::new(abp_core::error::MappingError::StreamingUnsupported {
            dialect: "x".into(),
        }),
    ];

    for e in &errors {
        assert!(!e.to_string().is_empty());
    }
}

// ---------------------------------------------------------------------------
// 27. HostError: SidecarCrashed with None exit_code
// ---------------------------------------------------------------------------

#[test]
fn host_error_sidecar_crashed_none_exit_code() {
    use abp_host::HostError;

    let err = HostError::SidecarCrashed {
        exit_code: None,
        stderr: "unknown crash".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("None"),
        "should show None for exit_code: {msg}"
    );
    assert!(
        msg.contains("unknown crash"),
        "should contain stderr: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 28. ProtocolError: UnexpectedMessage display contains expected and got
// ---------------------------------------------------------------------------

#[test]
fn protocol_error_unexpected_message_context() {
    use abp_protocol::ProtocolError;

    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "fatal".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("hello"), "should contain expected: {msg}");
    assert!(msg.contains("fatal"), "should contain got: {msg}");
}

// ---------------------------------------------------------------------------
// 29. MappingError: kind() classification
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_kind_classification() {
    use abp_core::error::{MappingError, MappingErrorKind};

    let fatal = MappingError::UnsupportedCapability {
        capability: "c".into(),
        dialect: "d".into(),
    };
    assert_eq!(fatal.kind(), MappingErrorKind::Fatal);
    assert!(fatal.is_fatal());
    assert!(!fatal.is_degraded());

    let degraded = MappingError::FidelityLoss {
        field: "f".into(),
        source_dialect: "a".into(),
        target_dialect: "b".into(),
        detail: "d".into(),
    };
    assert_eq!(degraded.kind(), MappingErrorKind::Degraded);
    assert!(degraded.is_degraded());
    assert!(!degraded.is_fatal());

    let emulated = MappingError::EmulationRequired {
        feature: "e".into(),
        detail: "d".into(),
    };
    assert_eq!(emulated.kind(), MappingErrorKind::Emulated);
    assert!(emulated.is_emulated());
}

// ---------------------------------------------------------------------------
// 30. Error chain: RuntimeError -> anyhow -> original preserves context
// ---------------------------------------------------------------------------

#[test]
fn error_chain_preserves_context() {
    use abp_runtime::RuntimeError;

    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "cannot write");
    let anyhow_err = anyhow::Error::new(io_err).context("staging workspace");
    let runtime_err = RuntimeError::WorkspaceFailed(anyhow_err);

    let msg = runtime_err.to_string();
    assert!(
        msg.contains("workspace"),
        "top-level should mention workspace: {msg}"
    );

    // Walk the source chain.
    let source = runtime_err.source().expect("should have source");
    let chain = format!("{source}");
    assert!(
        chain.contains("staging workspace"),
        "chain should contain anyhow context: {chain}"
    );
}
