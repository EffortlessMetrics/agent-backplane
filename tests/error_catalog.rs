// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive error taxonomy tests for every error type in the workspace.
//!
//! Verifies Display, Debug, Error trait, Send + Sync + 'static bounds,
//! source chains, From conversions, and anyhow interop.

use std::error::Error;
use std::io;

// ── Helpers ──────────────────────────────────────────────────────────────

fn assert_send_sync_static<T: Send + Sync + 'static>() {}

fn assert_std_error<T: std::error::Error>() {}

/// Verify Display is non-empty and Debug is non-empty for a given error value.
fn check_display_debug(err: &dyn Error) {
    let display = err.to_string();
    assert!(!display.is_empty(), "Display must be non-empty");
    let debug = format!("{err:?}");
    assert!(!debug.is_empty(), "Debug must be non-empty");
}

/// Round-trip through anyhow::Error and back via downcast.
fn check_anyhow_roundtrip<E: Error + Send + Sync + 'static + Clone>(err: E) {
    let anyhow_err: anyhow::Error = anyhow::Error::new(err.clone());
    let display_before = err.to_string();
    let display_after = anyhow_err.to_string();
    assert_eq!(display_before, display_after);
    // Downcast back
    let downcasted = anyhow_err
        .downcast_ref::<E>()
        .expect("downcast should succeed");
    assert_eq!(downcasted.to_string(), display_before);
}

// =========================================================================
// 1. ContractError (abp-core)
// =========================================================================
mod contract_error {
    use super::*;
    use abp_core::ContractError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<ContractError>();
        assert_std_error::<ContractError>();
    }

    #[test]
    fn json_variant_display_contains_context() {
        let bad_json = serde_json::from_str::<serde_json::Value>("not json");
        let serde_err = bad_json.unwrap_err();
        let err = ContractError::Json(serde_err);
        let msg = err.to_string();
        assert!(
            msg.contains("serialize") || msg.contains("JSON") || msg.contains("json"),
            "ContractError::Json Display should mention JSON: {msg}"
        );
        check_display_debug(&err);
    }

    #[test]
    fn json_variant_source_is_serde_error() {
        let serde_err = serde_json::from_str::<serde_json::Value>("!").unwrap_err();
        let err = ContractError::Json(serde_err);
        assert!(err.source().is_some(), "Json variant should have a source");
        assert!(
            err.source()
                .unwrap()
                .downcast_ref::<serde_json::Error>()
                .is_some()
        );
    }

    #[test]
    fn from_serde_json_error() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
        let err: ContractError = serde_err.into();
        assert!(matches!(err, ContractError::Json(_)));
    }

    #[test]
    fn anyhow_interop() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
        let err = ContractError::Json(serde_err);
        let anyhow_err: anyhow::Error = err.into();
        assert!(
            anyhow_err.to_string().contains("JSON")
                || anyhow_err.to_string().contains("json")
                || anyhow_err.to_string().contains("serialize")
        );
        assert!(anyhow_err.downcast_ref::<ContractError>().is_some());
    }
}

// =========================================================================
// 2. ValidationError (abp-core::validate)
// =========================================================================
mod validation_error {
    use super::*;
    use abp_core::validate::ValidationError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<ValidationError>();
        assert_std_error::<ValidationError>();
    }

    #[test]
    fn missing_field_display() {
        let err = ValidationError::MissingField {
            field: "backend_id",
        };
        let msg = err.to_string();
        assert!(
            msg.contains("backend_id"),
            "should mention the field name: {msg}"
        );
        assert!(msg.contains("missing"), "should mention 'missing': {msg}");
        check_display_debug(&err);
    }

    #[test]
    fn invalid_hash_display() {
        let err = ValidationError::InvalidHash {
            expected: "abc123".into(),
            actual: "def456".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("abc123"),
            "should include expected hash: {msg}"
        );
        assert!(msg.contains("def456"), "should include actual hash: {msg}");
        check_display_debug(&err);
    }

    #[test]
    fn empty_backend_id_display() {
        let err = ValidationError::EmptyBackendId;
        let msg = err.to_string();
        assert!(msg.contains("backend"), "should mention backend: {msg}");
        check_display_debug(&err);
    }

    #[test]
    fn invalid_outcome_display() {
        let err = ValidationError::InvalidOutcome {
            reason: "timestamps are wrong".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("timestamps are wrong"),
            "should include reason: {msg}"
        );
        check_display_debug(&err);
    }

    #[test]
    fn no_source_for_validation_errors() {
        // ValidationError is a leaf error; no underlying cause.
        let variants: Vec<ValidationError> = vec![
            ValidationError::MissingField { field: "x" },
            ValidationError::InvalidHash {
                expected: "a".into(),
                actual: "b".into(),
            },
            ValidationError::EmptyBackendId,
            ValidationError::InvalidOutcome { reason: "r".into() },
        ];
        for v in &variants {
            assert!(
                v.source().is_none(),
                "ValidationError should have no source: {v}"
            );
        }
    }

    #[test]
    fn exhaustive_variants() {
        // Compile-time exhaustiveness: pattern-match all variants.
        let variants: Vec<ValidationError> = vec![
            ValidationError::MissingField { field: "f" },
            ValidationError::InvalidHash {
                expected: "e".into(),
                actual: "a".into(),
            },
            ValidationError::EmptyBackendId,
            ValidationError::InvalidOutcome { reason: "r".into() },
        ];
        for v in &variants {
            match v {
                ValidationError::MissingField { .. } => {}
                ValidationError::InvalidHash { .. } => {}
                ValidationError::EmptyBackendId => {}
                ValidationError::InvalidOutcome { .. } => {}
            }
        }
    }

    #[test]
    fn anyhow_roundtrip() {
        check_anyhow_roundtrip(ValidationError::EmptyBackendId);
        check_anyhow_roundtrip(ValidationError::MissingField { field: "x" });
        check_anyhow_roundtrip(ValidationError::InvalidHash {
            expected: "a".into(),
            actual: "b".into(),
        });
        check_anyhow_roundtrip(ValidationError::InvalidOutcome {
            reason: "bad".into(),
        });
    }
}

// =========================================================================
// 3. ProtocolError (abp-protocol)
// =========================================================================
mod protocol_error {
    use super::*;
    use abp_protocol::ProtocolError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<ProtocolError>();
        assert_std_error::<ProtocolError>();
    }

    #[test]
    fn json_variant() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
        let err = ProtocolError::Json(serde_err);
        assert!(
            err.to_string().contains("JSON")
                || err.to_string().contains("json")
                || err.to_string().contains("invalid")
        );
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn io_variant() {
        let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
        let err = ProtocolError::Io(io_err);
        assert!(err.to_string().contains("pipe broke") || err.to_string().contains("I/O"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn violation_variant() {
        let err = ProtocolError::Violation("bad framing".into());
        assert!(err.to_string().contains("bad framing"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn unexpected_message_variant() {
        let err = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("hello"), "should mention expected: {msg}");
        assert!(msg.contains("run"), "should mention got: {msg}");
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn from_serde_json_error() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let err: ProtocolError = serde_err.into();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err: ProtocolError = io_err.into();
        assert!(matches!(err, ProtocolError::Io(_)));
    }

    #[test]
    fn exhaustive_variants() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let variants: Vec<ProtocolError> = vec![
            ProtocolError::Json(serde_err),
            ProtocolError::Io(io::Error::other("x")),
            ProtocolError::Violation("v".into()),
            ProtocolError::UnexpectedMessage {
                expected: "a".into(),
                got: "b".into(),
            },
        ];
        for v in &variants {
            match v {
                ProtocolError::Json(_) => {}
                ProtocolError::Io(_) => {}
                ProtocolError::Violation(_) => {}
                ProtocolError::UnexpectedMessage { .. } => {}
            }
            check_display_debug(v);
        }
    }

    #[test]
    fn anyhow_interop() {
        let err = ProtocolError::Violation("test".into());
        let anyhow_err: anyhow::Error = err.into();
        assert!(anyhow_err.to_string().contains("test"));
        assert!(anyhow_err.downcast_ref::<ProtocolError>().is_some());
    }
}

// =========================================================================
// 4. HostError (abp-host)
// =========================================================================
mod host_error {
    use super::*;
    use abp_host::HostError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<HostError>();
        assert_std_error::<HostError>();
    }

    #[test]
    fn spawn_variant() {
        let err = HostError::Spawn(io::Error::new(io::ErrorKind::NotFound, "not found"));
        assert!(err.to_string().contains("spawn"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn stdout_variant() {
        let err = HostError::Stdout(io::Error::new(io::ErrorKind::BrokenPipe, "broken"));
        assert!(err.to_string().contains("stdout"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn stdin_variant() {
        let err = HostError::Stdin(io::Error::new(io::ErrorKind::WriteZero, "write zero"));
        assert!(err.to_string().contains("stdin"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn protocol_variant_and_from_conversion() {
        let proto = abp_protocol::ProtocolError::Violation("bad".into());
        let err: HostError = proto.into();
        assert!(matches!(err, HostError::Protocol(_)));
        assert!(err.to_string().contains("protocol"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn violation_variant() {
        let err = HostError::Violation("out of order".into());
        assert!(err.to_string().contains("out of order"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn fatal_variant() {
        let err = HostError::Fatal("OOM".into());
        assert!(err.to_string().contains("OOM"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn exited_variant() {
        let err = HostError::Exited { code: Some(1) };
        let msg = err.to_string();
        assert!(msg.contains('1'), "should include exit code: {msg}");
        check_display_debug(&err);

        let err_none = HostError::Exited { code: None };
        let msg_none = err_none.to_string();
        assert!(
            msg_none.contains("None") || msg_none.contains("exited"),
            "should handle None code: {msg_none}"
        );
        check_display_debug(&err_none);
    }

    #[test]
    fn exhaustive_variants() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let variants: Vec<HostError> = vec![
            HostError::Spawn(io::Error::other("x")),
            HostError::Stdout(io::Error::other("x")),
            HostError::Stdin(io::Error::other("x")),
            HostError::Protocol(abp_protocol::ProtocolError::Json(serde_err)),
            HostError::Violation("v".into()),
            HostError::Fatal("f".into()),
            HostError::Exited { code: Some(0) },
            HostError::SidecarCrashed {
                exit_code: Some(1),
                stderr: "segfault".into(),
            },
            HostError::Timeout {
                duration: std::time::Duration::from_secs(30),
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
            check_display_debug(v);
        }
    }

    #[test]
    fn anyhow_interop() {
        let err = HostError::Fatal("test fatal".into());
        let anyhow_err: anyhow::Error = err.into();
        assert!(anyhow_err.to_string().contains("test fatal"));
        assert!(anyhow_err.downcast_ref::<HostError>().is_some());
    }
}

// =========================================================================
// 5. RuntimeError (abp-runtime)
// =========================================================================
mod runtime_error {
    use super::*;
    use abp_runtime::RuntimeError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<RuntimeError>();
        assert_std_error::<RuntimeError>();
    }

    #[test]
    fn unknown_backend_variant() {
        let err = RuntimeError::UnknownBackend {
            name: "nonexistent".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent"),
            "should include backend name: {msg}"
        );
        assert!(
            msg.contains("unknown") || msg.contains("backend"),
            "should mention unknown backend: {msg}"
        );
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn workspace_failed_variant() {
        let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
        let msg = err.to_string();
        assert!(msg.contains("workspace"), "should mention workspace: {msg}");
        assert!(
            err.source().is_some(),
            "should chain the inner anyhow error"
        );
        check_display_debug(&err);
    }

    #[test]
    fn policy_failed_variant() {
        let err = RuntimeError::PolicyFailed(anyhow::anyhow!("invalid glob"));
        assert!(err.to_string().contains("policy"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn backend_failed_variant() {
        let err = RuntimeError::BackendFailed(anyhow::anyhow!("timeout"));
        assert!(err.to_string().contains("backend"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn capability_check_failed_variant() {
        let err = RuntimeError::CapabilityCheckFailed("streaming not supported".into());
        let msg = err.to_string();
        assert!(
            msg.contains("streaming not supported"),
            "should include details: {msg}"
        );
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn exhaustive_variants() {
        let variants: Vec<RuntimeError> = vec![
            RuntimeError::UnknownBackend { name: "x".into() },
            RuntimeError::WorkspaceFailed(anyhow::anyhow!("w")),
            RuntimeError::PolicyFailed(anyhow::anyhow!("p")),
            RuntimeError::BackendFailed(anyhow::anyhow!("b")),
            RuntimeError::CapabilityCheckFailed("c".into()),
        ];
        for v in &variants {
            match v {
                RuntimeError::UnknownBackend { .. } => {}
                RuntimeError::WorkspaceFailed(_) => {}
                RuntimeError::PolicyFailed(_) => {}
                RuntimeError::BackendFailed(_) => {}
                RuntimeError::CapabilityCheckFailed(_) => {}
            }
            check_display_debug(v);
        }
    }

    #[test]
    fn anyhow_interop() {
        let err = RuntimeError::UnknownBackend { name: "foo".into() };
        let anyhow_err: anyhow::Error = err.into();
        assert!(anyhow_err.to_string().contains("foo"));
        assert!(anyhow_err.downcast_ref::<RuntimeError>().is_some());
    }
}

// =========================================================================
// 6. ConfigError (abp-cli::config)
// =========================================================================
mod config_error {
    use super::*;
    use abp_cli::config::ConfigError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<ConfigError>();
        assert_std_error::<ConfigError>();
    }

    #[test]
    fn invalid_backend_display() {
        let err = ConfigError::InvalidBackend {
            name: "my-sidecar".into(),
            reason: "command is empty".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("my-sidecar"), "should include name: {msg}");
        assert!(
            msg.contains("command is empty"),
            "should include reason: {msg}"
        );
        check_display_debug(&err);
    }

    #[test]
    fn invalid_timeout_display() {
        let err = ConfigError::InvalidTimeout { value: 0 };
        let msg = err.to_string();
        assert!(msg.contains('0'), "should include the value: {msg}");
        assert!(msg.contains("timeout"), "should mention timeout: {msg}");
        check_display_debug(&err);
    }

    #[test]
    fn missing_required_field_display() {
        let err = ConfigError::MissingRequiredField {
            field: "backend name".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("backend name"), "should include field: {msg}");
        assert!(msg.contains("missing"), "should mention missing: {msg}");
        check_display_debug(&err);
    }

    #[test]
    fn no_source_for_config_errors() {
        let variants: Vec<ConfigError> = vec![
            ConfigError::InvalidBackend {
                name: "x".into(),
                reason: "r".into(),
            },
            ConfigError::InvalidTimeout { value: 99999 },
            ConfigError::MissingRequiredField { field: "f".into() },
        ];
        for v in &variants {
            assert!(
                v.source().is_none(),
                "ConfigError should have no source: {v}"
            );
        }
    }

    #[test]
    fn exhaustive_variants() {
        let variants: Vec<ConfigError> = vec![
            ConfigError::InvalidBackend {
                name: "n".into(),
                reason: "r".into(),
            },
            ConfigError::InvalidTimeout { value: 100 },
            ConfigError::MissingRequiredField { field: "f".into() },
        ];
        for v in &variants {
            match v {
                ConfigError::InvalidBackend { .. } => {}
                ConfigError::InvalidTimeout { .. } => {}
                ConfigError::MissingRequiredField { .. } => {}
            }
        }
    }

    #[test]
    fn anyhow_roundtrip() {
        check_anyhow_roundtrip(ConfigError::InvalidTimeout { value: 42 });
        check_anyhow_roundtrip(ConfigError::InvalidBackend {
            name: "x".into(),
            reason: "bad".into(),
        });
        check_anyhow_roundtrip(ConfigError::MissingRequiredField { field: "f".into() });
    }
}

// =========================================================================
// 7. SidecarError (sidecar-kit)
// =========================================================================
mod sidecar_error {
    use super::*;
    use sidecar_kit::SidecarError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<SidecarError>();
        assert_std_error::<SidecarError>();
    }

    #[test]
    fn spawn_variant() {
        let err = SidecarError::Spawn(io::Error::new(io::ErrorKind::NotFound, "no such file"));
        assert!(err.to_string().contains("spawn"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn stdout_variant() {
        let err = SidecarError::Stdout(io::Error::new(io::ErrorKind::BrokenPipe, "broken"));
        assert!(err.to_string().contains("stdout"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn stdin_variant() {
        let err = SidecarError::Stdin(io::Error::other("closed"));
        assert!(err.to_string().contains("stdin"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn protocol_variant() {
        let err = SidecarError::Protocol("bad frame".into());
        assert!(err.to_string().contains("bad frame"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn serialize_variant() {
        let serde_err = serde_json::from_str::<serde_json::Value>("not-json").unwrap_err();
        let err = SidecarError::Serialize(serde_err);
        assert!(err.to_string().contains("serialization"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn deserialize_variant() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
        let err = SidecarError::Deserialize(serde_err);
        assert!(err.to_string().contains("deserialization"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn fatal_variant() {
        let err = SidecarError::Fatal("out of memory".into());
        assert!(err.to_string().contains("out of memory"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn exited_variant() {
        let err = SidecarError::Exited(Some(137));
        assert!(err.to_string().contains("137"));
        check_display_debug(&err);

        let err_none = SidecarError::Exited(None);
        assert!(err_none.to_string().contains("None") || err_none.to_string().contains("exited"));
        check_display_debug(&err_none);
    }

    #[test]
    fn timeout_variant() {
        let err = SidecarError::Timeout;
        assert!(err.to_string().contains("timed out") || err.to_string().contains("timeout"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn exhaustive_variants() {
        let serde_err1 = serde_json::from_str::<serde_json::Value>("not-json").unwrap_err();
        let serde_err2 = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let variants: Vec<SidecarError> = vec![
            SidecarError::Spawn(io::Error::other("x")),
            SidecarError::Stdout(io::Error::other("x")),
            SidecarError::Stdin(io::Error::other("x")),
            SidecarError::Protocol("p".into()),
            SidecarError::Serialize(serde_err1),
            SidecarError::Deserialize(serde_err2),
            SidecarError::Fatal("f".into()),
            SidecarError::Exited(Some(0)),
            SidecarError::Timeout,
        ];
        for v in &variants {
            match v {
                SidecarError::Spawn(_) => {}
                SidecarError::Stdout(_) => {}
                SidecarError::Stdin(_) => {}
                SidecarError::Protocol(_) => {}
                SidecarError::Serialize(_) => {}
                SidecarError::Deserialize(_) => {}
                SidecarError::Fatal(_) => {}
                SidecarError::Exited(_) => {}
                SidecarError::Timeout => {}
            }
            check_display_debug(v);
        }
    }

    #[test]
    fn anyhow_interop() {
        let err = SidecarError::Timeout;
        let anyhow_err: anyhow::Error = err.into();
        assert!(anyhow_err.downcast_ref::<SidecarError>().is_some());
    }
}

// =========================================================================
// 8. BridgeError (claude-bridge)
// =========================================================================
mod bridge_error {
    use super::*;
    use claude_bridge::BridgeError;

    #[test]
    fn trait_bounds() {
        assert_send_sync_static::<BridgeError>();
        assert_std_error::<BridgeError>();
    }

    #[test]
    fn node_not_found_variant() {
        let err = BridgeError::NodeNotFound("/usr/bin/node".into());
        assert!(err.to_string().contains("/usr/bin/node"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn host_script_not_found_variant() {
        let err = BridgeError::HostScriptNotFound("hosts/claude/index.js".into());
        assert!(err.to_string().contains("hosts/claude/index.js"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn sidecar_variant_and_from_conversion() {
        let sidecar_err = sidecar_kit::SidecarError::Timeout;
        let err: BridgeError = sidecar_err.into();
        assert!(matches!(err, BridgeError::Sidecar(_)));
        assert!(err.to_string().contains("sidecar"));
        assert!(err.source().is_some());
        check_display_debug(&err);
    }

    #[test]
    fn config_variant() {
        let err = BridgeError::Config("missing API key".into());
        assert!(err.to_string().contains("missing API key"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn run_variant() {
        let err = BridgeError::Run("agent crashed".into());
        assert!(err.to_string().contains("agent crashed"));
        assert!(err.source().is_none());
        check_display_debug(&err);
    }

    #[test]
    fn exhaustive_variants() {
        let variants: Vec<BridgeError> = vec![
            BridgeError::NodeNotFound("node".into()),
            BridgeError::HostScriptNotFound("script.js".into()),
            BridgeError::Sidecar(sidecar_kit::SidecarError::Timeout),
            BridgeError::Config("c".into()),
            BridgeError::Run("r".into()),
        ];
        for v in &variants {
            match v {
                BridgeError::NodeNotFound(_) => {}
                BridgeError::HostScriptNotFound(_) => {}
                BridgeError::Sidecar(_) => {}
                BridgeError::Config(_) => {}
                BridgeError::Run(_) => {}
            }
            check_display_debug(v);
        }
    }

    #[test]
    fn anyhow_interop() {
        let err = BridgeError::Config("test".into());
        let anyhow_err: anyhow::Error = err.into();
        assert!(anyhow_err.downcast_ref::<BridgeError>().is_some());
    }
}

// =========================================================================
// 9. Cross-error conversion paths
// =========================================================================
mod error_conversion_paths {
    use super::*;

    #[test]
    fn serde_json_to_contract_error() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let contract: abp_core::ContractError = serde_err.into();
        assert!(matches!(contract, abp_core::ContractError::Json(_)));
    }

    #[test]
    fn serde_json_to_protocol_error() {
        let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let proto: abp_protocol::ProtocolError = serde_err.into();
        assert!(matches!(proto, abp_protocol::ProtocolError::Json(_)));
    }

    #[test]
    fn io_error_to_protocol_error() {
        let io_err = io::Error::new(io::ErrorKind::TimedOut, "timeout");
        let proto: abp_protocol::ProtocolError = io_err.into();
        assert!(matches!(proto, abp_protocol::ProtocolError::Io(_)));
    }

    #[test]
    fn protocol_error_to_host_error() {
        let proto = abp_protocol::ProtocolError::Violation("v".into());
        let host: abp_host::HostError = proto.into();
        assert!(matches!(host, abp_host::HostError::Protocol(_)));
    }

    #[test]
    fn sidecar_error_to_bridge_error() {
        let sidecar = sidecar_kit::SidecarError::Fatal("boom".into());
        let bridge: claude_bridge::BridgeError = sidecar.into();
        assert!(matches!(bridge, claude_bridge::BridgeError::Sidecar(_)));
    }

    #[test]
    fn error_chain_host_protocol_json() {
        // HostError::Protocol -> ProtocolError::Json -> serde_json::Error
        let serde_err = serde_json::from_str::<serde_json::Value>("!").unwrap_err();
        let proto = abp_protocol::ProtocolError::Json(serde_err);
        let host: abp_host::HostError = proto.into();

        // Walk the error chain
        let src1 = host.source().expect("HostError should have source");
        assert!(src1.downcast_ref::<abp_protocol::ProtocolError>().is_some());
        let src2 = src1.source().expect("ProtocolError should have source");
        assert!(src2.downcast_ref::<serde_json::Error>().is_some());
    }

    #[test]
    fn error_chain_bridge_sidecar() {
        // BridgeError::Sidecar -> SidecarError::Spawn -> io::Error
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "no perms");
        let sidecar = sidecar_kit::SidecarError::Spawn(io_err);
        let bridge: claude_bridge::BridgeError = sidecar.into();

        let src1 = bridge.source().expect("BridgeError should have source");
        assert!(src1.downcast_ref::<sidecar_kit::SidecarError>().is_some());
        let src2 = src1
            .source()
            .expect("SidecarError::Spawn should have source");
        assert!(src2.downcast_ref::<io::Error>().is_some());
    }
}

// =========================================================================
// 10. Error messages contain helpful information
// =========================================================================
mod error_messages_quality {

    #[test]
    fn runtime_error_unknown_backend_is_actionable() {
        let err = abp_runtime::RuntimeError::UnknownBackend {
            name: "openai-gpt4".into(),
        };
        let msg = err.to_string();
        // Should tell the user which backend was not found
        assert!(
            msg.contains("openai-gpt4"),
            "Error should include the attempted backend name for debugging: {msg}"
        );
    }

    #[test]
    fn validation_error_hash_mismatch_shows_both_hashes() {
        let err = abp_core::validate::ValidationError::InvalidHash {
            expected: "aaaa1111".into(),
            actual: "bbbb2222".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("aaaa1111") && msg.contains("bbbb2222"),
            "Hash mismatch error should show both values for debugging: {msg}"
        );
    }

    #[test]
    fn config_error_invalid_backend_names_the_backend() {
        let err = abp_cli::config::ConfigError::InvalidBackend {
            name: "my-broken-sidecar".into(),
            reason: "command not found".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("my-broken-sidecar"),
            "Should name the problematic backend: {msg}"
        );
        assert!(
            msg.contains("command not found"),
            "Should include the reason: {msg}"
        );
    }

    #[test]
    fn protocol_unexpected_message_shows_expected_and_actual() {
        let err = abp_protocol::ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "event".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("hello") && msg.contains("event"),
            "Should show both expected and actual message types: {msg}"
        );
    }

    #[test]
    fn host_error_exit_code_is_visible() {
        let err = abp_host::HostError::Exited { code: Some(137) };
        let msg = err.to_string();
        assert!(
            msg.contains("137"),
            "Should show the exit code for signal debugging: {msg}"
        );
    }

    #[test]
    fn sidecar_error_exit_code_is_visible() {
        let err = sidecar_kit::SidecarError::Exited(Some(2));
        let msg = err.to_string();
        assert!(msg.contains('2'), "Should show the exit code: {msg}");
    }
}
