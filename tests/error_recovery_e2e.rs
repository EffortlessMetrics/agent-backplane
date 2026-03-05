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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Integration tests exercising error recovery and retry paths across the
//! runtime pipeline.

use std::time::Duration;

use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_runtime::retry::{FallbackChain, RetryPolicy};
use abp_runtime::RuntimeError;

// ═══════════════════════════════════════════════════════════════════════════
// (a) Retry policy correctness — 10 tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn retry_backoff_increases_exponentially() {
    let policy = RetryPolicy::builder()
        .initial_backoff(Duration::from_millis(100))
        .backoff_multiplier(2.0)
        .max_backoff(Duration::from_secs(60))
        .max_retries(5)
        .build();

    let d0 = policy.compute_delay(0);
    let d1 = policy.compute_delay(1);
    let d2 = policy.compute_delay(2);

    // Even with jitter (±25%), exponential growth should be clearly visible.
    // Raw: 100, 200, 400 ms — lower bounds (×0.75): 75, 150, 300.
    assert!(d1 > d0, "delay should increase: d1={d1:?} > d0={d0:?}");
    assert!(d2 > d1, "delay should increase: d2={d2:?} > d1={d1:?}");
}

#[test]
fn retry_max_delay_caps_correctly() {
    let policy = RetryPolicy::builder()
        .initial_backoff(Duration::from_secs(1))
        .backoff_multiplier(10.0)
        .max_backoff(Duration::from_secs(5))
        .max_retries(10)
        .build();

    // Attempt 5: raw = 1 * 10^5 = 100_000 s → capped at 5 s.
    let delay = policy.compute_delay(5);
    assert!(
        delay <= Duration::from_secs(5),
        "delay must be capped: got {delay:?}"
    );
}

#[test]
fn retry_max_retries_exhausts_correctly() {
    let policy = RetryPolicy::builder().max_retries(3).build();

    assert!(policy.should_retry(0));
    assert!(policy.should_retry(1));
    assert!(policy.should_retry(2));
    assert!(!policy.should_retry(3), "attempt 3 should NOT be retried");
    assert!(!policy.should_retry(4));
}

#[test]
fn retryable_errors_trigger_retry() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("connection reset"));
    assert!(err.is_retryable(), "BackendFailed should be retryable");

    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.is_retryable(), "WorkspaceFailed should be retryable");
}

#[test]
fn non_retryable_errors_skip_retry() {
    let err = RuntimeError::UnknownBackend {
        name: "nope".into(),
    };
    assert!(!err.is_retryable(), "UnknownBackend is not retryable");

    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert!(!err.is_retryable(), "PolicyFailed is not retryable");

    let err = RuntimeError::CapabilityCheckFailed("missing tool_read".into());
    assert!(
        !err.is_retryable(),
        "CapabilityCheckFailed is not retryable"
    );

    let err = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert!(!err.is_retryable(), "NoProjectionMatch is not retryable");
}

#[test]
fn zero_retry_policy_never_retries() {
    let policy = RetryPolicy::no_retry();
    assert_eq!(policy.max_retries, 0);
    assert!(!policy.should_retry(0), "attempt 0 should NOT be retried");
}

#[test]
fn jitter_stays_within_bounds() {
    let policy = RetryPolicy::builder()
        .initial_backoff(Duration::from_secs(1))
        .backoff_multiplier(1.0) // constant base, only jitter varies
        .max_backoff(Duration::from_secs(10))
        .max_retries(100)
        .build();

    for attempt in 0..100 {
        let delay = policy.compute_delay(attempt);
        // Base is 1 s, jitter [0.75, 1.25] → delay in [750 ms, 1250 ms].
        assert!(
            delay >= Duration::from_millis(750) && delay <= Duration::from_millis(1250),
            "attempt {attempt}: delay {delay:?} out of jitter bounds"
        );
    }
}

#[test]
fn retry_policy_serialization_roundtrip() {
    let policy = RetryPolicy::builder()
        .max_retries(5)
        .initial_backoff(Duration::from_millis(200))
        .max_backoff(Duration::from_secs(10))
        .backoff_multiplier(3.0)
        .build();

    let json = serde_json::to_string(&policy).expect("serialize");
    let restored: RetryPolicy = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(policy, restored);
}

#[test]
fn retry_delay_for_is_alias_for_compute_delay() {
    let policy = RetryPolicy::default();
    for attempt in 0..5 {
        assert_eq!(policy.delay_for(attempt), policy.compute_delay(attempt));
    }
}

#[test]
fn retry_default_values_are_sensible() {
    let policy = RetryPolicy::default();
    assert_eq!(policy.max_retries, 3);
    assert_eq!(policy.initial_backoff, Duration::from_millis(100));
    assert_eq!(policy.max_backoff, Duration::from_secs(5));
    assert!((policy.backoff_multiplier - 2.0).abs() < f64::EPSILON);
}

// ═══════════════════════════════════════════════════════════════════════════
// (b) Fallback chain — 10 tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fallback_chain_iterates_in_order() {
    let mut chain = FallbackChain::new(vec!["a".into(), "b".into(), "c".into()]);
    assert_eq!(chain.next_backend(), Some("a"));
    assert_eq!(chain.next_backend(), Some("b"));
    assert_eq!(chain.next_backend(), Some("c"));
}

#[test]
fn fallback_chain_exhausted_returns_none() {
    let mut chain = FallbackChain::new(vec!["x".into()]);
    assert_eq!(chain.next_backend(), Some("x"));
    assert_eq!(chain.next_backend(), None);
    assert_eq!(chain.next_backend(), None);
}

#[test]
fn fallback_chain_reset_allows_reiteration() {
    let mut chain = FallbackChain::new(vec!["a".into(), "b".into()]);
    assert_eq!(chain.next_backend(), Some("a"));
    assert_eq!(chain.next_backend(), Some("b"));
    assert_eq!(chain.next_backend(), None);

    chain.reset();
    assert_eq!(chain.next_backend(), Some("a"));
    assert_eq!(chain.next_backend(), Some("b"));
}

#[test]
fn fallback_chain_single_backend() {
    let mut chain = FallbackChain::new(vec!["only".into()]);
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
    assert_eq!(chain.remaining(), 1);
    assert_eq!(chain.next_backend(), Some("only"));
    assert_eq!(chain.remaining(), 0);
    assert_eq!(chain.next_backend(), None);
}

#[test]
fn fallback_chain_empty_returns_none_immediately() {
    let mut chain = FallbackChain::new(vec![]);
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert_eq!(chain.remaining(), 0);
    assert_eq!(chain.next_backend(), None);
}

#[test]
fn fallback_chain_remaining_decreases() {
    let mut chain = FallbackChain::new(vec!["a".into(), "b".into(), "c".into()]);
    assert_eq!(chain.remaining(), 3);
    chain.next_backend();
    assert_eq!(chain.remaining(), 2);
    chain.next_backend();
    assert_eq!(chain.remaining(), 1);
    chain.next_backend();
    assert_eq!(chain.remaining(), 0);
}

#[test]
fn fallback_chain_len_is_total_count() {
    let chain = FallbackChain::new(vec!["a".into(), "b".into()]);
    assert_eq!(chain.len(), 2);
}

#[test]
fn fallback_chain_reset_after_partial_iteration() {
    let mut chain = FallbackChain::new(vec!["a".into(), "b".into(), "c".into()]);
    chain.next_backend(); // consume "a"
    chain.reset();
    assert_eq!(chain.remaining(), 3);
    assert_eq!(chain.next_backend(), Some("a"));
}

#[test]
fn fallback_chain_multiple_resets() {
    let mut chain = FallbackChain::new(vec!["x".into()]);
    for _ in 0..5 {
        assert_eq!(chain.next_backend(), Some("x"));
        assert_eq!(chain.next_backend(), None);
        chain.reset();
    }
}

#[test]
fn fallback_chain_is_empty_only_when_no_backends() {
    let empty = FallbackChain::new(vec![]);
    assert!(empty.is_empty());

    let non_empty = FallbackChain::new(vec!["a".into()]);
    assert!(!non_empty.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// (c) Error taxonomy integration — 10 tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_error_unknown_backend_maps_to_backend_not_found() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_workspace_failed_maps_to_workspace_init_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_policy_failed_maps_to_policy_invalid() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_backend_failed_maps_to_backend_crashed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_capability_check_failed_maps_to_capability_unsupported() {
    let err = RuntimeError::CapabilityCheckFailed("tool_read".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_classified_preserves_inner_code() {
    let abp_err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let err = RuntimeError::Classified(abp_err);
    assert_eq!(err.error_code(), ErrorCode::BackendTimeout);
}

#[test]
fn runtime_error_no_projection_match_maps_to_backend_not_found() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_display_messages_are_descriptive() {
    let err = RuntimeError::UnknownBackend {
        name: "my-backend".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("my-backend"),
        "display should include backend name: {msg}"
    );

    let err = RuntimeError::CapabilityCheckFailed("tool_bash missing".into());
    let msg = err.to_string();
    assert!(
        msg.contains("tool_bash missing"),
        "display should include detail: {msg}"
    );
}

#[test]
fn runtime_errors_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeError>();
}

#[test]
fn runtime_error_into_abp_error_roundtrip() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
    assert!(abp.message.contains("unknown backend"));
}

// ═══════════════════════════════════════════════════════════════════════════
// (d) Pipeline error scenarios — 5 tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_timeout_error_code_is_retryable() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    assert!(err.is_retryable(), "BackendTimeout should be retryable");

    let classified = RuntimeError::Classified(err);
    assert!(
        classified.is_retryable(),
        "Classified(BackendTimeout) should be retryable"
    );
}

#[test]
fn policy_violation_is_non_retryable() {
    let err_denied = AbpError::new(ErrorCode::PolicyDenied, "write to /etc denied");
    assert!(
        !err_denied.is_retryable(),
        "PolicyDenied should not be retryable"
    );

    let err_invalid = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob syntax"));
    assert!(
        !err_invalid.is_retryable(),
        "PolicyFailed should not be retryable"
    );
}

#[test]
fn unknown_backend_error_message_includes_backend_name() {
    let name = "sidecar:nonexistent";
    let err = RuntimeError::UnknownBackend {
        name: name.to_string(),
    };
    let display = format!("{err}");
    assert!(
        display.contains(name),
        "error display should include backend name '{name}': {display}"
    );
}

#[test]
fn workspace_failure_wraps_source_error() {
    let inner = anyhow::anyhow!("permission denied: /tmp/workspace");
    let err = RuntimeError::WorkspaceFailed(inner);
    let display = err.to_string();
    assert!(
        display.contains("workspace preparation failed"),
        "should mention workspace: {display}"
    );
    // The source chain should be accessible.
    let source = std::error::Error::source(&err);
    assert!(source.is_some(), "should have a source error");
    let source_msg = source.unwrap().to_string();
    assert!(
        source_msg.contains("/tmp/workspace"),
        "source should include path info: {source_msg}"
    );
}

#[test]
fn capability_check_failure_lists_missing_capabilities() {
    let missing = "backend 'mock': missing required capabilities: [tool_bash, tool_write]";
    let err = RuntimeError::CapabilityCheckFailed(missing.to_string());
    let display = err.to_string();
    assert!(
        display.contains("tool_bash"),
        "should list missing capability: {display}"
    );
    assert!(
        display.contains("tool_write"),
        "should list missing capability: {display}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional error taxonomy tests (to exceed 35 total)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_category_mapping_is_correct() {
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn error_code_as_str_is_snake_case() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.as_str(),
        "workspace_init_failed"
    );
}

#[test]
fn error_info_retryable_inferred_from_code() {
    let retryable = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
    assert!(retryable.is_retryable);

    let permanent = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    assert!(!permanent.is_retryable);
}

#[test]
fn error_info_with_detail_attaches_metadata() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30_000);
    assert_eq!(info.details.len(), 2);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
    assert_eq!(info.details["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_source_chain_is_accessible() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err =
        AbpError::new(ErrorCode::WorkspaceInitFailed, "workspace setup failed").with_source(io_err);
    let source = std::error::Error::source(&err);
    assert!(source.is_some());
    assert!(source.unwrap().to_string().contains("file not found"));
}

#[test]
fn abp_error_display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "process exited with code 137");
    let display = err.to_string();
    assert!(display.contains("backend_crashed"), "display: {display}");
    assert!(display.contains("process exited"), "display: {display}");
}
