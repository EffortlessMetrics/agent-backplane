#![allow(clippy::all)]
#![allow(dead_code)]
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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive error recovery and resilience tests.
//!
//! Covers: backend failure recovery, sidecar crash recovery, protocol error
//! recovery, timeout recovery, partial stream recovery, circuit breaker
//! patterns, error classification, error code propagation, error aggregation,
//! graceful degradation, resource cleanup on error, and error event generation.

use std::error::Error;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use abp_core::aggregate::EventAggregator;
use abp_core::{AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, WorkOrderBuilder};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_error_taxonomy::classification::{
    ClassificationCategory, ErrorClassifier, ErrorSeverity, RecoveryAction,
};
use abp_host::HostError;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_retry::{
    retry_with_policy, CircuitBreaker, CircuitBreakerError, CircuitState, RetryPolicy,
};
use abp_runtime::RuntimeError;
use chrono::Utc;

// =========================================================================
// Helpers
// =========================================================================

/// All retryable ErrorCode variants from abp-error.
const RETRYABLE_CODES: &[ErrorCode] = &[
    ErrorCode::BackendUnavailable,
    ErrorCode::BackendTimeout,
    ErrorCode::BackendRateLimited,
    ErrorCode::BackendCrashed,
];

/// Representative permanent (non-retryable) ErrorCode variants.
const PERMANENT_CODES: &[ErrorCode] = &[
    ErrorCode::BackendNotFound,
    ErrorCode::BackendAuthFailed,
    ErrorCode::BackendModelNotFound,
    ErrorCode::PolicyDenied,
    ErrorCode::CapabilityUnsupported,
    ErrorCode::ContractVersionMismatch,
    ErrorCode::ProtocolInvalidEnvelope,
];

fn make_error_event(msg: &str, code: Option<ErrorCode>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: code,
        },
        ext: None,
    }
}

fn make_delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn make_run_started_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "started".to_string(),
        },
        ext: None,
    }
}

fn make_failed_receipt(error_events: Vec<AgentEvent>) -> Receipt {
    let mut builder = ReceiptBuilder::new("test-backend").outcome(Outcome::Failed);
    for evt in error_events {
        builder = builder.add_trace_event(evt);
    }
    builder.build()
}

// =========================================================================
// 1. Backend failure recovery — retry on transient, fail on permanent
// =========================================================================

#[tokio::test]
async fn retry_succeeds_on_first_attempt() {
    let policy = RetryPolicy::no_retry();
    let result: Result<&str, String> =
        retry_with_policy(&policy, || async { Ok::<_, String>("ok") }).await;
    assert_eq!(result.unwrap(), "ok");
}

#[tokio::test]
async fn retry_recovers_after_transient_failures() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let policy = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_millis(10),
        1.0,
        false,
    );
    let result: Result<&str, &str> = retry_with_policy(&policy, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err("transient")
            } else {
                Ok("recovered")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "recovered");
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_exhaustion_returns_last_error() {
    let policy = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_millis(5),
        1.0,
        false,
    );
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result: Result<(), &str> = retry_with_policy(&policy, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("still failing")
        }
    })
    .await;
    assert_eq!(result.unwrap_err(), "still failing");
    assert_eq!(counter.load(Ordering::SeqCst), 3); // initial + 2 retries
}

#[tokio::test]
async fn no_retry_policy_calls_once() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let policy = RetryPolicy::no_retry();
    let _: Result<(), &str> = retry_with_policy(&policy, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn transient_error_codes_are_retryable() {
    for code in RETRYABLE_CODES {
        assert!(code.is_retryable(), "{code:?} should be retryable");
    }
}

#[test]
fn permanent_error_codes_are_not_retryable() {
    for code in PERMANENT_CODES {
        assert!(!code.is_retryable(), "{code:?} should NOT be retryable");
    }
}

#[test]
fn abp_error_retryable_delegation() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    assert!(err.is_retryable());

    let err2 = AbpError::new(ErrorCode::BackendAuthFailed, "bad creds");
    assert!(!err2.is_retryable());
}

#[test]
fn error_info_retryable_from_code() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "slow down");
    assert!(info.is_retryable);

    let info2 = ErrorInfo::new(ErrorCode::PolicyDenied, "nope");
    assert!(!info2.is_retryable);
}

// =========================================================================
// 2. Sidecar crash recovery
// =========================================================================

#[test]
fn host_error_sidecar_crashed_captures_stderr() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by OOM".into(),
    };
    let display = err.to_string();
    assert!(display.contains("137"), "should contain exit code");
    assert!(display.contains("killed by OOM"), "should contain stderr");
}

#[test]
fn host_error_exited_none_code() {
    let err = HostError::Exited { code: None };
    let display = err.to_string();
    assert!(display.contains("None"), "no exit code shown");
}

#[test]
fn host_error_exited_with_code() {
    let err = HostError::Exited { code: Some(1) };
    assert!(err.to_string().contains('1'));
}

#[test]
fn host_error_fatal_message() {
    let err = HostError::Fatal("sidecar panic".into());
    assert!(err.to_string().contains("sidecar panic"));
}

#[test]
fn host_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HostError>();
}

#[test]
fn runtime_backend_failed_maps_to_crashed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("exit 1"));
    assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_into_abp_preserves_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("boom"));
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendCrashed);
}

// =========================================================================
// 3. Protocol error recovery — malformed JSONL, invalid envelope
// =========================================================================

#[test]
fn protocol_error_from_invalid_json() {
    let result = JsonlCodec::decode("not valid json");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_error_json_has_no_error_code() {
    let err = JsonlCodec::decode("{bad").unwrap_err();
    assert!(err.error_code().is_none());
}

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("bad state".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_error_unexpected_message_has_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn protocol_error_abp_variant_carries_code() {
    let inner = AbpError::new(ErrorCode::ProtocolHandshakeFailed, "no hello");
    let err = ProtocolError::Abp(inner);
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolHandshakeFailed));
}

#[test]
fn protocol_error_io_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err = ProtocolError::from(io_err);
    assert!(err.to_string().contains("pipe broke"));
}

#[test]
fn protocol_error_from_abp_error() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "v0.2 vs v0.1");
    let proto: ProtocolError = abp.into();
    assert!(matches!(proto, ProtocolError::Abp(_)));
}

#[test]
fn malformed_envelope_missing_tag() {
    let line = r#"{"ref_id":"abc","event":{}}"#;
    let result = JsonlCodec::decode(line);
    assert!(result.is_err());
}

#[test]
fn empty_line_is_protocol_error() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn truncated_json_is_protocol_error() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version"#);
    assert!(result.is_err());
}

// =========================================================================
// 4. Timeout recovery
// =========================================================================

#[test]
fn host_error_timeout_includes_duration() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let display = err.to_string();
    assert!(display.contains("30"));
}

#[test]
fn backend_timeout_is_retriable() {
    assert!(ErrorCode::BackendTimeout.is_retryable());
}

#[test]
fn backend_timeout_category_is_backend() {
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
}

#[tokio::test]
async fn retry_after_simulated_timeout() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let policy = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_millis(5),
        1.0,
        false,
    );
    let result: Result<&str, AbpError> = retry_with_policy(&policy, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(AbpError::new(ErrorCode::BackendTimeout, "timed out"))
            } else {
                Ok("ok")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "ok");
}

#[test]
fn timeout_classifier_suggests_retry() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::TimeoutError);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.is_some());
}

// =========================================================================
// 5. Partial stream recovery — events received before failure
// =========================================================================

#[test]
fn partial_receipt_preserves_events_before_failure() {
    let events = vec![
        make_run_started_event(),
        make_delta_event("partial output"),
        make_error_event("backend died", Some(ErrorCode::BackendCrashed)),
    ];
    let receipt = make_failed_receipt(events);
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.trace.len(), 3);
    assert!(matches!(
        &receipt.trace[1].kind,
        AgentEventKind::AssistantDelta { text } if text == "partial output"
    ));
}

#[test]
fn aggregator_counts_partial_events_before_error() {
    let mut agg = EventAggregator::new();
    agg.add(&make_run_started_event());
    agg.add(&make_delta_event("chunk1"));
    agg.add(&make_delta_event("chunk2"));
    agg.add(&make_error_event("crash", None));

    assert_eq!(agg.event_count(), 4);
    assert!(agg.has_errors());
    assert_eq!(agg.error_messages().len(), 1);
    assert_eq!(agg.text_length(), "chunk1".len() + "chunk2".len());
}

#[test]
fn partial_receipt_hash_is_stable() {
    let events = vec![
        make_run_started_event(),
        make_error_event("fail", Some(ErrorCode::BackendCrashed)),
    ];
    let receipt = make_failed_receipt(events);
    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
}

#[test]
fn partial_stream_summary_shows_error_count() {
    let mut agg = EventAggregator::new();
    agg.add(&make_delta_event("some text"));
    agg.add(&make_error_event("err1", None));
    agg.add(&make_error_event("err2", None));
    let summary = agg.summary();
    assert_eq!(summary.errors, 2);
    assert_eq!(summary.total_text_chars, "some text".len());
}

// =========================================================================
// 6. Circuit breaker patterns
// =========================================================================

#[tokio::test]
async fn circuit_breaker_starts_closed() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn circuit_breaker_success_stays_closed() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    let result: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<String, String>("ok".into()) })
        .await;
    assert!(result.is_ok());
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn circuit_breaker_opens_after_threshold() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(60));
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);
    assert_eq!(cb.consecutive_failures(), 2);
}

#[tokio::test]
async fn circuit_breaker_open_rejects_calls() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(60));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);

    let result: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok("should not run".into()) }).await;
    assert!(matches!(result, Err(CircuitBreakerError::Open)));
}

#[tokio::test]
async fn circuit_breaker_half_open_after_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Next call transitions to half-open; success closes it
    let result: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok("recovered".into()) }).await;
    assert_eq!(result.unwrap(), "recovered");
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn circuit_breaker_half_open_failure_reopens() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;

    tokio::time::sleep(Duration::from_millis(20)).await;

    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("still down") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[test]
fn circuit_breaker_accessors() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    assert_eq!(cb.failure_threshold(), 5);
    assert_eq!(cb.recovery_timeout(), Duration::from_secs(30));
}

#[test]
fn circuit_state_serde_roundtrip() {
    let states = [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ];
    for state in &states {
        let json = serde_json::to_string(state).unwrap();
        let back: CircuitState = serde_json::from_str(&json).unwrap();
        assert_eq!(*state, back);
    }
}

// =========================================================================
// 7. Error classification — transient vs permanent, severity
// =========================================================================

#[test]
fn classify_all_retryable_codes_as_retriable_severity() {
    let classifier = ErrorClassifier::new();
    for code in RETRYABLE_CODES {
        let cl = classifier.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Retriable,
            "{code:?} should be Retriable"
        );
    }
}

#[test]
fn classify_permanent_backend_codes_as_fatal() {
    let classifier = ErrorClassifier::new();
    let fatal_codes = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
    ];
    for code in &fatal_codes {
        let cl = classifier.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "{code:?} should be Fatal"
        );
    }
}

#[test]
fn classify_protocol_errors_as_fatal() {
    let classifier = ErrorClassifier::new();
    let proto_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::ProtocolUnexpectedMessage,
    ];
    for code in &proto_codes {
        let cl = classifier.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "{code:?} should be Fatal"
        );
    }
}

#[test]
fn classify_lossy_conversion_as_degraded() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
}

#[test]
fn classify_capability_emulation_as_degraded() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::CapabilityEmulationFailed);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
}

#[test]
fn classifier_recovery_for_rate_limit_has_delay() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.unwrap() > 0);
}

#[test]
fn classifier_recovery_for_auth_is_contact_admin() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn classifier_recovery_for_model_not_found_is_change_model() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
}

#[test]
fn classifier_recovery_for_capability_unsupported_is_fallback() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn classification_preserves_input_code() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::Internal);
    assert_eq!(cl.code, ErrorCode::Internal);
}

#[test]
fn suggest_recovery_from_classification() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendTimeout);
    let suggestion = classifier.suggest_recovery(&cl);
    assert_eq!(suggestion.action, RecoveryAction::Retry);
}

// =========================================================================
// 8. Error code propagation — from backend to receipt
// =========================================================================

#[test]
fn runtime_error_unknown_backend_code() {
    let err = RuntimeError::UnknownBackend {
        name: "nonexistent".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_workspace_failed_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("io error"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_policy_failed_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("glob error"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_capability_check_code() {
    let err = RuntimeError::CapabilityCheckFailed("streaming".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_classified_carries_original_code() {
    let abp = AbpError::new(ErrorCode::MappingDialectMismatch, "mismatch");
    let err = RuntimeError::Classified(abp);
    assert_eq!(err.error_code(), ErrorCode::MappingDialectMismatch);
}

#[test]
fn runtime_error_no_projection_match_code() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no backend".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn error_code_propagated_to_receipt_trace() {
    let events = vec![make_error_event("fail", Some(ErrorCode::BackendCrashed))];
    let receipt = make_failed_receipt(events);
    let error_event = &receipt.trace[0];
    if let AgentEventKind::Error { error_code, .. } = &error_event.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Error event");
    }
}

#[test]
fn error_code_none_in_generic_error_event() {
    let events = vec![make_error_event("unknown error", None)];
    let receipt = make_failed_receipt(events);
    if let AgentEventKind::Error { error_code, .. } = &receipt.trace[0].kind {
        assert!(error_code.is_none());
    } else {
        panic!("expected Error event");
    }
}

#[test]
fn abp_error_to_info_propagates_code_and_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert!(info.is_retryable);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
    assert_eq!(info.details["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_dto_roundtrip() {
    let err = AbpError::new(ErrorCode::Internal, "oops").with_context("phase", "init");
    let dto = AbpErrorDto::from(&err);
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, ErrorCode::Internal);
    assert_eq!(back.message, "oops");
    assert_eq!(back.context["phase"], serde_json::json!("init"));
}

// =========================================================================
// 9. Error aggregation — multiple errors in a single run
// =========================================================================

#[test]
fn aggregator_collects_multiple_errors() {
    let mut agg = EventAggregator::new();
    agg.add(&make_error_event("err1", Some(ErrorCode::BackendTimeout)));
    agg.add(&make_error_event("err2", Some(ErrorCode::BackendCrashed)));
    agg.add(&make_error_event("err3", None));

    assert_eq!(agg.error_messages().len(), 3);
    assert!(agg.has_errors());
}

#[test]
fn aggregator_no_errors_returns_false() {
    let mut agg = EventAggregator::new();
    agg.add(&make_run_started_event());
    agg.add(&make_delta_event("text"));
    assert!(!agg.has_errors());
    assert!(agg.error_messages().is_empty());
}

#[test]
fn receipt_trace_can_hold_mixed_events_including_errors() {
    let events = vec![
        make_run_started_event(),
        make_delta_event("hello"),
        make_error_event("warning-like", None),
        make_delta_event("more text"),
        make_error_event("fatal", Some(ErrorCode::Internal)),
    ];
    let receipt = make_failed_receipt(events);
    assert_eq!(receipt.trace.len(), 5);

    let error_count = receipt
        .trace
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Error { .. }))
        .count();
    assert_eq!(error_count, 2);
}

#[test]
fn aggregation_summary_counts_errors_and_text() {
    let mut agg = EventAggregator::new();
    agg.add(&make_delta_event("abc"));
    agg.add(&make_error_event("e1", None));
    agg.add(&make_delta_event("defgh"));
    agg.add(&make_error_event("e2", None));

    let summary = agg.summary();
    assert_eq!(summary.total_events, 4);
    assert_eq!(summary.errors, 2);
    assert_eq!(summary.total_text_chars, 8); // abc + defgh
}

// =========================================================================
// 10. Graceful degradation — fallback backends
// =========================================================================

#[test]
fn classifier_suggests_fallback_for_mapping_failure() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::MappingDialectMismatch);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn classifier_suggests_fallback_for_unmappable_tool() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::MappingUnmappableTool);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn degraded_capability_emulation_suggests_fallback() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::CapabilityEmulationFailed);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    // CapabilityUnsupported category → Fallback, regardless of Degraded severity
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn recovery_suggestion_description_is_nonempty() {
    let classifier = ErrorClassifier::new();
    for code in RETRYABLE_CODES.iter().chain(PERMANENT_CODES.iter()) {
        let cl = classifier.classify(code);
        assert!(
            !cl.recovery.description.is_empty(),
            "{code:?} recovery should have a description"
        );
    }
}

#[tokio::test]
async fn fallback_simulation_with_retry() {
    // Simulates trying primary backend then falling back
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let policy = RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_millis(5),
        1.0,
        false,
    );

    let result: Result<&str, &str> = retry_with_policy(&policy, || {
        let c = c.clone();
        async move {
            let attempt = c.fetch_add(1, Ordering::SeqCst);
            match attempt {
                0 => Err("primary failed"),
                _ => Ok("fallback succeeded"),
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "fallback succeeded");
}

// =========================================================================
// 11. Resource cleanup on error — temp dirs, channels, handles
// =========================================================================

#[test]
fn work_order_builder_does_not_leak_on_error_path() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
    // WorkOrder is just data; this confirms no panic/leak on drop
    drop(wo);
}

#[test]
fn receipt_builder_dropped_without_build() {
    let builder = ReceiptBuilder::new("test");
    // Builder is dropped without calling build — no resources to clean
    drop(builder);
}

#[tokio::test]
async fn mpsc_channel_drops_cleanly_on_error() {
    let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(10);
    tx.send(make_run_started_event()).await.unwrap();
    tx.send(make_error_event("fail", Some(ErrorCode::Internal)))
        .await
        .unwrap();
    drop(tx); // Sender dropped (simulating error cleanup)
    drop(rx); // Receiver dropped — no dangling resources
}

#[test]
fn temp_dir_cleaned_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    assert!(path.exists());
    drop(dir);
    assert!(!path.exists());
}

#[tokio::test]
async fn circuit_breaker_usable_after_error_sequence() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(10));
    // Drive to open
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Recover
    let result: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok("back".into()) }).await;
    assert!(result.is_ok());
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

// =========================================================================
// 12. Error event generation — AgentEvent with error details
// =========================================================================

#[test]
fn error_event_without_code() {
    let event = make_error_event("something broke", None);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &event.kind
    {
        assert_eq!(message, "something broke");
        assert!(error_code.is_none());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn error_event_with_code() {
    let event = make_error_event("timed out", Some(ErrorCode::BackendTimeout));
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &event.kind
    {
        assert_eq!(message, "timed out");
        assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn error_event_serializes_with_type_tag() {
    let event = make_error_event("fail", Some(ErrorCode::Internal));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["message"], "fail");
    assert_eq!(json["error_code"], "internal");
}

#[test]
fn error_event_without_code_omits_field() {
    let event = make_error_event("fail", None);
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("error_code"), "None code should be skipped");
}

#[test]
fn warning_event_is_distinct_from_error() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "degraded".into(),
        },
        ext: None,
    };
    assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "warning");
}

#[test]
fn error_event_roundtrip_serde() {
    let event = make_error_event("round trip", Some(ErrorCode::BackendCrashed));
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &back.kind
    {
        assert_eq!(message, "round trip");
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("wrong variant");
    }
}

// =========================================================================
// Cross-cutting: Error category and code exhaustiveness
// =========================================================================

#[test]
fn all_error_codes_have_category() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    for code in &codes {
        let _ = code.category(); // Must not panic
        let _ = code.as_str();
        let _ = code.message();
    }
}

#[test]
fn all_error_codes_as_str_are_unique() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
    ];
    let strs: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
    let mut deduped = strs.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(strs.len(), deduped.len());
}

#[test]
fn error_category_display_coverage() {
    let categories = [
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Mapping,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Internal,
    ];
    for cat in &categories {
        let display = cat.to_string();
        assert!(!display.is_empty());
    }
}

#[test]
fn error_category_serde_roundtrip() {
    let cat = ErrorCategory::Backend;
    let json = serde_json::to_string(&cat).unwrap();
    let back: ErrorCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(cat, back);
}

#[test]
fn error_code_serde_roundtrip() {
    let code = ErrorCode::BackendTimeout;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(code, back);
}

// =========================================================================
// Cross-cutting: Error chain and cause propagation
// =========================================================================

#[test]
fn abp_error_source_chain() {
    let inner = std::io::Error::new(std::io::ErrorKind::TimedOut, "deadline exceeded");
    let err = AbpError::new(ErrorCode::BackendTimeout, "backend timed out").with_source(inner);
    assert!(err.source().is_some());
    assert!(err
        .source()
        .unwrap()
        .to_string()
        .contains("deadline exceeded"));
}

#[test]
fn abp_error_no_source_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    assert!(err.source().is_none());
}

#[test]
fn abp_error_display_includes_code() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    let display = err.to_string();
    assert!(display.contains("backend_not_found"));
    assert!(display.contains("no such backend"));
}

#[test]
fn abp_error_display_with_context_includes_json() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    let display = err.to_string();
    assert!(display.contains("backend_timeout"));
    assert!(display.contains("timed out"));
    assert!(display.contains("openai"));
}

#[test]
fn abp_error_debug_shows_struct() {
    let err = AbpError::new(ErrorCode::Internal, "bug");
    let debug = format!("{err:?}");
    assert!(debug.contains("AbpError"));
    assert!(debug.contains("Internal"));
}

#[test]
fn abp_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AbpError>();
}

#[test]
fn protocol_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProtocolError>();
}

#[test]
fn runtime_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeError>();
}

// =========================================================================
// Retry policy configuration
// =========================================================================

#[test]
fn retry_policy_default_values() {
    let p = RetryPolicy::default();
    assert_eq!(p.max_retries, 3);
    assert_eq!(p.base_delay, Duration::from_millis(100));
    assert_eq!(p.max_delay, Duration::from_secs(5));
    assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    assert!(p.jitter);
}

#[test]
fn retry_policy_no_retry_values() {
    let p = RetryPolicy::no_retry();
    assert_eq!(p.max_retries, 0);
    assert_eq!(p.base_delay, Duration::ZERO);
    assert!(!p.jitter);
}

#[test]
fn retry_policy_delay_respects_max() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(100),
        Duration::from_millis(200),
        10.0,
        false,
    );
    let delay = p.delay_for_attempt(5);
    assert!(delay <= Duration::from_millis(200));
}

#[test]
fn retry_policy_exponential_backoff() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    let d0 = p.delay_for_attempt(0);
    let d1 = p.delay_for_attempt(1);
    let d2 = p.delay_for_attempt(2);
    assert!(d1 > d0, "delay should increase");
    assert!(d2 > d1, "delay should keep increasing");
}

#[test]
fn retry_policy_serde_roundtrip() {
    let p = RetryPolicy::default();
    let json = serde_json::to_string(&p).unwrap();
    let back: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn retry_policy_clone_eq() {
    let p = RetryPolicy::default();
    let cloned = p.clone();
    assert_eq!(p, cloned);
}

// =========================================================================
// Envelope Fatal variant for error propagation
// =========================================================================

#[test]
fn fatal_envelope_carries_error_code() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "backend crashed".into(),
        error_code: Some(ErrorCode::BackendCrashed),
    };
    assert_eq!(env.error_code(), Some(ErrorCode::BackendCrashed));
}

#[test]
fn fatal_envelope_without_code() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "unknown".into(),
        error_code: None,
    };
    assert_eq!(env.error_code(), None);
}

#[test]
fn fatal_envelope_serde_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "crash".into(),
        error_code: Some(ErrorCode::Internal),
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let back = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = back
    {
        assert_eq!(ref_id, Some("run-1".into()));
        assert_eq!(error, "crash");
        assert_eq!(error_code, Some(ErrorCode::Internal));
    } else {
        panic!("expected Fatal");
    }
}

// =========================================================================
// Host error → Protocol error chain
// =========================================================================

#[test]
fn host_error_protocol_wraps_inner() {
    let proto = ProtocolError::Violation("bad envelope".into());
    let host = HostError::Protocol(proto);
    let display = host.to_string();
    assert!(display.contains("bad envelope"));
}

#[test]
fn host_error_spawn_wraps_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such binary");
    let err = HostError::Spawn(io_err);
    assert!(err.to_string().contains("no such binary"));
    assert!(err.source().is_some());
}

#[test]
fn host_error_violation_message() {
    let err = HostError::Violation("unexpected state".into());
    assert!(err.to_string().contains("unexpected state"));
}
