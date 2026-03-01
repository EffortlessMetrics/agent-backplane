// SPDX-License-Identifier: MIT OR Apache-2.0
//! Consistency tests for every error type in the workspace.
//!
//! Verifies Display, Debug, non-empty messages, no leading/trailing whitespace,
//! source chains, and distinct Display output across variants.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Debug, Display};

/// Assert that a value implements Display and Debug, the Display output is
/// non-empty, and it has no leading/trailing whitespace.
fn assert_display_debug<T: Display + Debug>(val: &T) {
    let display = val.to_string();
    assert!(!display.is_empty(), "Display is empty for {:?}", val);
    assert_eq!(
        display,
        display.trim(),
        "Display has leading/trailing whitespace for {:?}: {:?}",
        val,
        display
    );
}

/// Assert that every item in a slice produces distinct Display output.
fn assert_distinct_display<T: Display + Debug>(items: &[T]) {
    let mut seen = HashSet::new();
    for item in items {
        let s = item.to_string();
        assert!(
            seen.insert(s.clone()),
            "Duplicate Display output: {:?} for {:?}",
            s,
            item
        );
    }
}

// ───────────────────────────────── ContractError ─────────────────────────────

#[test]
fn contract_error_display_debug() {
    let json_err: Result<serde_json::Value, _> = serde_json::from_str("{bad");
    let err = abp_core::ContractError::Json(json_err.unwrap_err());
    assert_display_debug(&err);
    assert!(
        err.source().is_some(),
        "ContractError::Json should have source"
    );
}

// ───────────────────────────────── ErrorCode ─────────────────────────────────

#[test]
fn error_code_display_debug() {
    let codes = abp_core::error::ErrorCatalog::all();
    assert!(!codes.is_empty());
    for code in &codes {
        assert_display_debug(code);
    }
}

#[test]
fn error_code_implements_std_error() {
    use abp_core::error::ErrorCode;
    let code = ErrorCode::IoError;
    let _: &dyn Error = &code;
}

#[test]
fn error_code_distinct_display() {
    let codes = abp_core::error::ErrorCatalog::all();
    assert_distinct_display(&codes);
}

// ───────────────────────────────── ErrorInfo ─────────────────────────────────

#[test]
fn error_info_display_debug_source() {
    use abp_core::error::{ErrorCode, ErrorInfo};

    let plain = ErrorInfo::new(ErrorCode::IoError, "disk full");
    assert_display_debug(&plain);
    assert!(plain.source().is_none());

    let with_ctx =
        ErrorInfo::new(ErrorCode::ReadDenied, "forbidden").with_context("path", "/etc/shadow");
    assert_display_debug(&with_ctx);
    assert!(with_ctx.to_string().contains("path=/etc/shadow"));

    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
    let with_src = ErrorInfo::new(ErrorCode::IoError, "read failed").with_source(inner);
    assert_display_debug(&with_src);
    assert!(
        with_src.source().is_some(),
        "ErrorInfo with_source should chain"
    );
}

// ───────────────────────────────── ValidationError ──────────────────────────

#[test]
fn validation_error_display_debug() {
    use abp_core::validate::ValidationError;

    let variants: Vec<ValidationError> = vec![
        ValidationError::MissingField { field: "meta" },
        ValidationError::InvalidHash {
            expected: "abc".into(),
            actual: "xyz".into(),
        },
        ValidationError::EmptyBackendId,
        ValidationError::InvalidOutcome {
            reason: "bad".into(),
        },
    ];

    for v in &variants {
        assert_display_debug(v);
        assert!(v.source().is_none());
    }
    assert_distinct_display(&variants);
}

// ───────────────────────────────── ChainError ────────────────────────────────

#[test]
fn chain_error_display_debug() {
    use abp_core::chain::ChainError;

    let variants: Vec<ChainError> = vec![
        ChainError::InvalidHash { index: 0 },
        ChainError::EmptyChain,
        ChainError::DuplicateId {
            id: uuid::Uuid::nil(),
        },
    ];

    for v in &variants {
        assert_display_debug(v);
        assert!(v.source().is_none());
    }
    assert_distinct_display(&variants);
}

// ───────────────────────────────── ProtocolError ─────────────────────────────

#[test]
fn protocol_error_display_debug() {
    use abp_protocol::ProtocolError;

    let json_err: Result<serde_json::Value, _> = serde_json::from_str("{bad");
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");

    let variants: Vec<ProtocolError> = vec![
        ProtocolError::Json(json_err.unwrap_err()),
        ProtocolError::Io(io_err),
        ProtocolError::Violation("test violation".into()),
        ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        },
    ];

    for v in &variants {
        assert_display_debug(v);
    }
    assert_distinct_display(&variants);
}

#[test]
fn protocol_error_source_chains() {
    use abp_protocol::ProtocolError;

    let json_err: Result<serde_json::Value, _> = serde_json::from_str("{bad");
    let err = ProtocolError::Json(json_err.unwrap_err());
    assert!(
        err.source().is_some(),
        "ProtocolError::Json should chain source"
    );

    let err = ProtocolError::Io(std::io::Error::other("x"));
    assert!(
        err.source().is_some(),
        "ProtocolError::Io should chain source"
    );

    let err = ProtocolError::Violation("v".into());
    assert!(err.source().is_none());
}

// ───────────────────────────────── VersionError ──────────────────────────────

#[test]
fn version_error_display_debug() {
    use abp_protocol::version::{ProtocolVersion, VersionError};

    let variants: Vec<VersionError> = vec![
        VersionError::InvalidFormat,
        VersionError::InvalidMajor,
        VersionError::InvalidMinor,
        VersionError::Incompatible {
            local: ProtocolVersion { major: 0, minor: 1 },
            remote: ProtocolVersion { major: 1, minor: 0 },
        },
    ];

    for v in &variants {
        assert_display_debug(v);
        assert!(v.source().is_none());
    }
    assert_distinct_display(&variants);
}

// ───────────────────────────────── RuntimeError ──────────────────────────────

#[test]
fn runtime_error_display_debug() {
    use abp_runtime::RuntimeError;

    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "foo".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("temp dir")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob")),
        RuntimeError::BackendFailed(anyhow::anyhow!("timeout")),
        RuntimeError::CapabilityCheckFailed("missing streaming".into()),
    ];

    for v in &variants {
        assert_display_debug(v);
    }
    assert_distinct_display(&variants);
}

#[test]
fn runtime_error_source_chains() {
    use abp_runtime::RuntimeError;

    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(err.source().is_none());

    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("boom"));
    assert!(
        err.source().is_some(),
        "WorkspaceFailed should chain source"
    );

    let err = RuntimeError::BackendFailed(anyhow::anyhow!("fail"));
    assert!(err.source().is_some(), "BackendFailed should chain source");
}

// ───────────────────────────────── MultiplexError ────────────────────────────

#[test]
fn multiplex_error_display_debug() {
    use abp_runtime::multiplex::MultiplexError;

    let variants: Vec<MultiplexError> = vec![
        MultiplexError::NoSubscribers,
        MultiplexError::Lagged { missed: 42 },
        MultiplexError::Closed,
    ];

    for v in &variants {
        assert_display_debug(v);
        assert!(v.source().is_none());
    }
    assert_distinct_display(&variants);
}

// ───────────────────────────────── HostError ─────────────────────────────────

#[test]
fn host_error_display_debug() {
    use abp_host::HostError;

    let variants: Vec<HostError> = vec![
        HostError::Spawn(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        )),
        HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe")),
        HostError::Stdin(std::io::Error::new(std::io::ErrorKind::WriteZero, "zero")),
        HostError::Protocol(abp_protocol::ProtocolError::Violation("test".into())),
        HostError::Violation("bad state".into()),
        HostError::Fatal("out of memory".into()),
        HostError::Exited { code: Some(1) },
        HostError::SidecarCrashed {
            exit_code: Some(137),
            stderr: "killed".into(),
        },
        HostError::Timeout {
            duration: std::time::Duration::from_secs(30),
        },
    ];

    for v in &variants {
        assert_display_debug(v);
    }
    assert_distinct_display(&variants);
}

#[test]
fn host_error_source_chains() {
    use abp_host::HostError;

    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "x"));
    assert!(err.source().is_some(), "Spawn should chain source");

    let err = HostError::Protocol(abp_protocol::ProtocolError::Violation("x".into()));
    assert!(err.source().is_some(), "Protocol should chain source");

    let err = HostError::Violation("v".into());
    assert!(err.source().is_none());

    let err = HostError::Fatal("f".into());
    assert!(err.source().is_none());
}

// ───────────────────────────────── PipelineError ─────────────────────────────

#[test]
fn pipeline_error_display_debug() {
    use sidecar_kit::PipelineError;

    let variants: Vec<PipelineError> = vec![
        PipelineError::StageError {
            stage: "redact".into(),
            message: "field missing".into(),
        },
        PipelineError::InvalidEvent,
    ];

    for v in &variants {
        assert_display_debug(v);
        assert!(v.source().is_none());
    }
    assert_distinct_display(&variants);
}

// ───────────────────────────────── SidecarError ──────────────────────────────

#[test]
fn sidecar_error_display_debug() {
    use sidecar_kit::SidecarError;

    let variants: Vec<SidecarError> = vec![
        SidecarError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")),
        SidecarError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe")),
        SidecarError::Stdin(std::io::Error::new(std::io::ErrorKind::WriteZero, "zero")),
        SidecarError::Protocol("bad frame".into()),
        SidecarError::Serialize(serde_json::from_str::<serde_json::Value>("{bad").unwrap_err()),
        SidecarError::Deserialize(serde_json::from_str::<serde_json::Value>("[bad").unwrap_err()),
        SidecarError::Fatal("crash".into()),
        SidecarError::Exited(Some(127)),
        SidecarError::Timeout,
    ];

    for v in &variants {
        assert_display_debug(v);
    }
    assert_distinct_display(&variants);
}

#[test]
fn sidecar_error_source_chains() {
    use sidecar_kit::SidecarError;

    let err = SidecarError::Spawn(std::io::Error::other("x"));
    assert!(err.source().is_some(), "Spawn should chain source");

    let err = SidecarError::Serialize(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
    assert!(err.source().is_some(), "Serialize should chain source");

    let err = SidecarError::Protocol("p".into());
    assert!(err.source().is_none());

    let err = SidecarError::Timeout;
    assert!(err.source().is_none());
}

// ───────────────────────────────── ApiError ──────────────────────────────────

#[test]
fn api_error_display_debug() {
    use axum::http::StatusCode;

    let variants: Vec<abp_daemon::ApiError> = vec![
        abp_daemon::ApiError::new(StatusCode::BAD_REQUEST, "invalid input"),
        abp_daemon::ApiError::new(StatusCode::NOT_FOUND, "run not found"),
        abp_daemon::ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "boom"),
    ];

    for v in &variants {
        assert_display_debug(v);
        assert!(v.source().is_none());
    }
    assert_distinct_display(&variants);
}

// ───────────────────────────────── Cross-cutting ─────────────────────────────

/// Ensure the error types are object-safe / can be used as `dyn Error`.
#[test]
fn all_errors_are_dyn_error_compatible() {
    let errors: Vec<Box<dyn Error>> = vec![
        Box::new(abp_core::ContractError::Json(
            serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
        )),
        Box::new(abp_core::error::ErrorCode::IoError),
        Box::new(abp_core::error::ErrorInfo::new(
            abp_core::error::ErrorCode::InternalError,
            "test",
        )),
        Box::new(abp_core::validate::ValidationError::EmptyBackendId),
        Box::new(abp_core::chain::ChainError::EmptyChain),
        Box::new(abp_protocol::ProtocolError::Violation("v".into())),
        Box::new(abp_protocol::version::VersionError::InvalidFormat),
        Box::new(abp_runtime::RuntimeError::UnknownBackend { name: "x".into() }),
        Box::new(abp_runtime::multiplex::MultiplexError::Closed),
        Box::new(abp_host::HostError::Fatal("f".into())),
        Box::new(sidecar_kit::PipelineError::InvalidEvent),
        Box::new(sidecar_kit::SidecarError::Timeout),
        Box::new(abp_daemon::ApiError::new(
            axum::http::StatusCode::IM_A_TEAPOT,
            "teapot",
        )),
    ];

    for e in &errors {
        let display = e.to_string();
        assert!(!display.is_empty());
        let _debug = format!("{:?}", e);
    }
}
