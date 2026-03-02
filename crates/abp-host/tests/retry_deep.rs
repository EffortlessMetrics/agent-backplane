// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the retry / recovery layer in `abp_host::retry`.

use abp_host::retry::{
    RetryAttempt, RetryConfig, RetryMetadata, compute_delay, is_retryable, retry_async,
    spawn_with_retry,
};
use abp_host::{HostError, SidecarSpec};
use abp_protocol::ProtocolError;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

// ───────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────

fn fast_cfg(max_retries: u32) -> RetryConfig {
    RetryConfig {
        max_retries,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(50),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    }
}

fn no_jitter_cfg(base_ms: u64, max_ms: u64) -> RetryConfig {
    RetryConfig {
        max_retries: 10,
        base_delay: Duration::from_millis(base_ms),
        max_delay: Duration::from_millis(max_ms),
        overall_timeout: Duration::from_secs(60),
        jitter_factor: 0.0,
    }
}

fn spawn_err() -> HostError {
    HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ))
}

// ───────────────────────────────────────────────────────────────────
// 1. Default RetryConfig values
// ───────────────────────────────────────────────────────────────────

#[test]
fn default_max_retries_is_three() {
    assert_eq!(RetryConfig::default().max_retries, 3);
}

#[test]
fn default_base_delay_is_100ms() {
    assert_eq!(
        RetryConfig::default().base_delay,
        Duration::from_millis(100)
    );
}

#[test]
fn default_max_delay_is_10s() {
    assert_eq!(RetryConfig::default().max_delay, Duration::from_secs(10));
}

#[test]
fn default_overall_timeout_is_60s() {
    assert_eq!(
        RetryConfig::default().overall_timeout,
        Duration::from_secs(60)
    );
}

#[test]
fn default_jitter_factor_is_half() {
    let cfg = RetryConfig::default();
    assert!((cfg.jitter_factor - 0.5).abs() < f64::EPSILON);
}

// ───────────────────────────────────────────────────────────────────
// 2. Exponential backoff calculation at each attempt
// ───────────────────────────────────────────────────────────────────

#[test]
fn backoff_attempt_0_returns_base_delay() {
    let cfg = no_jitter_cfg(100, 100_000);
    assert_eq!(compute_delay(&cfg, 0), Duration::from_millis(100));
}

#[test]
fn backoff_attempt_1_doubles_base() {
    let cfg = no_jitter_cfg(100, 100_000);
    assert_eq!(compute_delay(&cfg, 1), Duration::from_millis(200));
}

#[test]
fn backoff_attempt_2_quadruples_base() {
    let cfg = no_jitter_cfg(100, 100_000);
    assert_eq!(compute_delay(&cfg, 2), Duration::from_millis(400));
}

#[test]
fn backoff_attempt_3_is_800ms() {
    let cfg = no_jitter_cfg(100, 100_000);
    assert_eq!(compute_delay(&cfg, 3), Duration::from_millis(800));
}

#[test]
fn backoff_attempt_4_is_1600ms() {
    let cfg = no_jitter_cfg(100, 100_000);
    assert_eq!(compute_delay(&cfg, 4), Duration::from_millis(1600));
}

#[test]
fn backoff_1s_base_standard_progression() {
    let cfg = no_jitter_cfg(1000, 1_000_000);
    assert_eq!(compute_delay(&cfg, 0), Duration::from_secs(1));
    assert_eq!(compute_delay(&cfg, 1), Duration::from_secs(2));
    assert_eq!(compute_delay(&cfg, 2), Duration::from_secs(4));
    assert_eq!(compute_delay(&cfg, 3), Duration::from_secs(8));
    assert_eq!(compute_delay(&cfg, 4), Duration::from_secs(16));
}

#[test]
fn backoff_powers_of_two_sequence() {
    let cfg = no_jitter_cfg(1, 1_000_000);
    for attempt in 0..16u32 {
        let expected = Duration::from_millis(2u64.pow(attempt));
        assert_eq!(compute_delay(&cfg, attempt), expected, "attempt {attempt}");
    }
}

#[test]
fn backoff_large_attempt_saturates_no_overflow() {
    let cfg = no_jitter_cfg(1000, u64::MAX / 2);
    // 2^63 would overflow, saturating_pow should handle it
    let delay = compute_delay(&cfg, 63);
    assert!(delay.as_millis() > 0);
}

// ───────────────────────────────────────────────────────────────────
// 3. Backoff capped at max_delay
// ───────────────────────────────────────────────────────────────────

#[test]
fn backoff_caps_at_max_delay() {
    let cfg = no_jitter_cfg(100, 500);
    // 100 * 2^3 = 800 > 500, should be capped
    assert_eq!(compute_delay(&cfg, 3), Duration::from_millis(500));
}

#[test]
fn backoff_all_high_attempts_return_max_delay() {
    let cfg = no_jitter_cfg(100, 500);
    for attempt in 3..20u32 {
        assert_eq!(
            compute_delay(&cfg, attempt),
            Duration::from_millis(500),
            "attempt {attempt}"
        );
    }
}

#[test]
fn backoff_just_below_cap_not_capped() {
    let cfg = no_jitter_cfg(100, 500);
    // 100 * 2^2 = 400 < 500
    assert_eq!(compute_delay(&cfg, 2), Duration::from_millis(400));
}

#[test]
fn backoff_exactly_at_cap() {
    let cfg = no_jitter_cfg(100, 800);
    // 100 * 2^3 = 800 == 800
    assert_eq!(compute_delay(&cfg, 3), Duration::from_millis(800));
}

// ───────────────────────────────────────────────────────────────────
// 4. Jitter adds randomness within expected range
// ───────────────────────────────────────────────────────────────────

#[test]
fn zero_jitter_returns_exact_delay() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(500),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    // Should be deterministic
    for attempt in 0..5 {
        let d1 = compute_delay(&cfg, attempt);
        let d2 = compute_delay(&cfg, attempt);
        assert_eq!(
            d1, d2,
            "zero jitter must be deterministic at attempt {attempt}"
        );
    }
}

#[test]
fn full_jitter_stays_at_or_below_nominal() {
    let cfg = RetryConfig {
        jitter_factor: 1.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    for attempt in 0..8 {
        let nominal_ms = (100u64 * 2u64.saturating_pow(attempt)).min(60_000);
        for _ in 0..20 {
            let delay = compute_delay(&cfg, attempt);
            assert!(
                delay <= Duration::from_millis(nominal_ms),
                "delay {delay:?} > nominal {nominal_ms}ms at attempt {attempt}"
            );
        }
    }
}

#[test]
fn half_jitter_stays_within_half_to_nominal() {
    let cfg = RetryConfig {
        jitter_factor: 0.5,
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    for attempt in 0..4 {
        let nominal_ms = (1000u64 * 2u64.saturating_pow(attempt)).min(60_000);
        let min_expected = nominal_ms - (nominal_ms / 2); // half jitter subtracts at most 50%
        for _ in 0..20 {
            let delay = compute_delay(&cfg, attempt);
            assert!(
                delay >= Duration::from_millis(min_expected),
                "delay {delay:?} < min {min_expected}ms at attempt {attempt}"
            );
            assert!(
                delay <= Duration::from_millis(nominal_ms),
                "delay {delay:?} > nominal {nominal_ms}ms at attempt {attempt}"
            );
        }
    }
}

#[test]
fn jitter_factor_clamped_below_zero() {
    let cfg = RetryConfig {
        jitter_factor: -1.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    // Negative jitter is clamped to 0.0, so should behave like no jitter
    assert_eq!(compute_delay(&cfg, 0), Duration::from_millis(100));
    assert_eq!(compute_delay(&cfg, 1), Duration::from_millis(200));
}

#[test]
fn jitter_factor_clamped_above_one() {
    let cfg = RetryConfig {
        jitter_factor: 5.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    // Clamped to 1.0, so full jitter — delay should be <= nominal
    let nominal = Duration::from_millis(100);
    for _ in 0..20 {
        assert!(compute_delay(&cfg, 0) <= nominal);
    }
}

#[test]
fn jitter_with_zero_base_delay_returns_zero() {
    let cfg = RetryConfig {
        jitter_factor: 1.0,
        base_delay: Duration::ZERO,
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    assert_eq!(compute_delay(&cfg, 0), Duration::ZERO);
    assert_eq!(compute_delay(&cfg, 5), Duration::ZERO);
}

// ───────────────────────────────────────────────────────────────────
// 5. RetryMetadata tracks attempt count correctly
// ───────────────────────────────────────────────────────────────────

#[test]
fn metadata_default_is_zeroed() {
    let meta = RetryMetadata::default();
    assert_eq!(meta.total_attempts, 0);
    assert!(meta.failed_attempts.is_empty());
    assert_eq!(meta.total_duration, Duration::ZERO);
}

#[test]
fn metadata_tracks_total_attempts() {
    let meta = RetryMetadata {
        total_attempts: 5,
        failed_attempts: vec![],
        total_duration: Duration::from_secs(10),
    };
    assert_eq!(meta.total_attempts, 5);
}

#[test]
fn metadata_receipt_no_failures_excludes_key() {
    let meta = RetryMetadata {
        total_attempts: 1,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(10),
    };
    let map = meta.to_receipt_metadata();
    assert!(!map.contains_key("retry_failed_attempts"));
    assert_eq!(map["retry_total_attempts"], serde_json::json!(1));
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(10));
}

#[test]
fn metadata_receipt_with_failures_includes_details() {
    let meta = RetryMetadata {
        total_attempts: 4,
        failed_attempts: vec![
            RetryAttempt {
                attempt: 0,
                error: "err0".into(),
                delay: Duration::from_millis(10),
            },
            RetryAttempt {
                attempt: 1,
                error: "err1".into(),
                delay: Duration::from_millis(20),
            },
            RetryAttempt {
                attempt: 2,
                error: "err2".into(),
                delay: Duration::from_millis(40),
            },
        ],
        total_duration: Duration::from_millis(100),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(4));
    let arr = map["retry_failed_attempts"].as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["error"], "err0");
    assert_eq!(arr[1]["delay_ms"], 20);
    assert_eq!(arr[2]["attempt"], 2);
}

#[test]
fn metadata_receipt_duration_is_u64_millis() {
    let meta = RetryMetadata {
        total_attempts: 1,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(12345),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(12345u64));
}

#[test]
fn metadata_receipt_returns_btreemap() {
    let meta = RetryMetadata::default();
    let map: BTreeMap<String, serde_json::Value> = meta.to_receipt_metadata();
    // BTreeMap keys are sorted
    let keys: Vec<_> = map.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

// ───────────────────────────────────────────────────────────────────
// 6. retry_async retries on failure up to max_attempts
// ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn retry_async_retries_exact_max_attempts() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cfg = fast_cfg(4); // 1 initial + 4 retries = 5 total

    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(spawn_err())
            }
        },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(counter.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn retry_async_failed_attempts_recorded_correctly() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cfg = fast_cfg(3);

    let result = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 3 {
                    Err::<i32, _>(spawn_err())
                } else {
                    Ok(42)
                }
            }
        },
        is_retryable,
    )
    .await;

    let outcome = result.unwrap();
    assert_eq!(outcome.value, 42);
    assert_eq!(outcome.metadata.total_attempts, 4);
    assert_eq!(outcome.metadata.failed_attempts.len(), 3);
    // Verify attempt indices are sequential
    for (i, a) in outcome.metadata.failed_attempts.iter().enumerate() {
        assert_eq!(a.attempt, i as u32);
    }
}

// ───────────────────────────────────────────────────────────────────
// 7. retry_async succeeds immediately if first attempt works
// ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn retry_async_immediate_success() {
    let cfg = fast_cfg(5);
    let outcome = retry_async(&cfg, || async { Ok::<_, HostError>(100) }, is_retryable)
        .await
        .unwrap();

    assert_eq!(outcome.value, 100);
    assert_eq!(outcome.metadata.total_attempts, 1);
    assert!(outcome.metadata.failed_attempts.is_empty());
}

#[tokio::test]
async fn retry_async_immediate_success_duration_is_short() {
    let cfg = fast_cfg(5);
    let outcome = retry_async(&cfg, || async { Ok::<_, HostError>(()) }, is_retryable)
        .await
        .unwrap();

    assert!(outcome.metadata.total_duration < Duration::from_secs(1));
}

// ───────────────────────────────────────────────────────────────────
// 8. retry_async respects timeout
// ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn retry_async_timeout_limits_attempts() {
    let cfg = RetryConfig {
        max_retries: 1000,
        base_delay: Duration::from_millis(30),
        max_delay: Duration::from_millis(30),
        overall_timeout: Duration::from_millis(100),
        jitter_factor: 0.0,
    };
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();

    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(spawn_err())
            }
        },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    // With 30ms backoff and 100ms timeout, should get at most a handful of attempts
    let attempts = counter.load(Ordering::SeqCst);
    assert!(attempts < 10, "too many attempts: {attempts}");
    assert!(
        attempts >= 2,
        "should have at least 2 attempts, got {attempts}"
    );
}

#[tokio::test]
async fn retry_async_timeout_returns_timeout_or_last_error() {
    let cfg = RetryConfig {
        max_retries: 1000,
        base_delay: Duration::from_millis(40),
        max_delay: Duration::from_millis(40),
        overall_timeout: Duration::from_millis(80),
        jitter_factor: 0.0,
    };

    let result: Result<_, HostError> =
        retry_async(&cfg, || async { Err::<(), _>(spawn_err()) }, is_retryable).await;

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Timeout { .. } | HostError::Spawn(_)),
        "expected Timeout or Spawn, got: {err}"
    );
}

// ───────────────────────────────────────────────────────────────────
// 9. Zero retries means one attempt only
// ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn zero_retries_single_attempt_failure() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cfg = fast_cfg(0);

    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(spawn_err())
            }
        },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn zero_retries_single_attempt_success() {
    let cfg = fast_cfg(0);
    let outcome = retry_async(&cfg, || async { Ok::<_, HostError>("ok") }, is_retryable)
        .await
        .unwrap();
    assert_eq!(outcome.value, "ok");
    assert_eq!(outcome.metadata.total_attempts, 1);
}

// ───────────────────────────────────────────────────────────────────
// 10. Custom RetryConfig values
// ───────────────────────────────────────────────────────────────────

#[test]
fn custom_config_all_fields() {
    let cfg = RetryConfig {
        max_retries: 10,
        base_delay: Duration::from_millis(250),
        max_delay: Duration::from_secs(30),
        overall_timeout: Duration::from_secs(120),
        jitter_factor: 0.75,
    };
    assert_eq!(cfg.max_retries, 10);
    assert_eq!(cfg.base_delay, Duration::from_millis(250));
    assert_eq!(cfg.max_delay, Duration::from_secs(30));
    assert_eq!(cfg.overall_timeout, Duration::from_secs(120));
    assert!((cfg.jitter_factor - 0.75).abs() < f64::EPSILON);
}

#[test]
fn custom_config_with_defaults_override() {
    let cfg = RetryConfig {
        max_retries: 7,
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    assert_eq!(cfg.max_retries, 7);
    assert!((cfg.jitter_factor - 0.0).abs() < f64::EPSILON);
    // Other fields from default
    assert_eq!(cfg.base_delay, Duration::from_millis(100));
    assert_eq!(cfg.max_delay, Duration::from_secs(10));
    assert_eq!(cfg.overall_timeout, Duration::from_secs(60));
}

#[test]
fn custom_config_aggressive_backoff() {
    let cfg = RetryConfig {
        max_retries: 20,
        base_delay: Duration::from_secs(1),
        max_delay: Duration::from_secs(300),
        overall_timeout: Duration::from_secs(600),
        jitter_factor: 1.0,
    };
    assert_eq!(cfg.max_retries, 20);
    assert_eq!(cfg.base_delay, Duration::from_secs(1));
}

// ───────────────────────────────────────────────────────────────────
// 11. Serde roundtrip for RetryConfig
// ───────────────────────────────────────────────────────────────────

#[test]
fn retry_config_serde_roundtrip_defaults() {
    let cfg = RetryConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.max_retries, cfg.max_retries);
    assert_eq!(parsed.base_delay, cfg.base_delay);
    assert_eq!(parsed.max_delay, cfg.max_delay);
    assert_eq!(parsed.overall_timeout, cfg.overall_timeout);
    assert!((parsed.jitter_factor - cfg.jitter_factor).abs() < f64::EPSILON);
}

#[test]
fn retry_config_serde_roundtrip_custom() {
    let cfg = RetryConfig {
        max_retries: 7,
        base_delay: Duration::from_millis(250),
        max_delay: Duration::from_secs(30),
        overall_timeout: Duration::from_secs(120),
        jitter_factor: 0.75,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.max_retries, 7);
    assert_eq!(parsed.base_delay, Duration::from_millis(250));
    assert_eq!(parsed.max_delay, Duration::from_secs(30));
    assert_eq!(parsed.overall_timeout, Duration::from_secs(120));
    assert!((parsed.jitter_factor - 0.75).abs() < f64::EPSILON);
}

#[test]
fn retry_config_serializes_durations_as_millis() {
    let cfg = RetryConfig {
        base_delay: Duration::from_secs(2),
        max_delay: Duration::from_secs(30),
        overall_timeout: Duration::from_secs(120),
        ..RetryConfig::default()
    };
    let val: serde_json::Value = serde_json::to_value(&cfg).unwrap();
    assert_eq!(val["base_delay"], serde_json::json!(2000));
    assert_eq!(val["max_delay"], serde_json::json!(30000));
    assert_eq!(val["overall_timeout"], serde_json::json!(120000));
}

#[test]
fn retry_config_deserializes_from_millis() {
    let json = r#"{"max_retries":5,"base_delay":500,"max_delay":5000,"overall_timeout":30000,"jitter_factor":0.25}"#;
    let cfg: RetryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.max_retries, 5);
    assert_eq!(cfg.base_delay, Duration::from_millis(500));
    assert_eq!(cfg.max_delay, Duration::from_secs(5));
    assert_eq!(cfg.overall_timeout, Duration::from_secs(30));
    assert!((cfg.jitter_factor - 0.25).abs() < f64::EPSILON);
}

// ───────────────────────────────────────────────────────────────────
// 12. RetryMetadata serde roundtrip
// ───────────────────────────────────────────────────────────────────

#[test]
fn retry_metadata_serde_roundtrip_empty() {
    let meta = RetryMetadata {
        total_attempts: 1,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(5),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: RetryMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.total_attempts, 1);
    assert!(parsed.failed_attempts.is_empty());
    assert_eq!(parsed.total_duration, Duration::from_millis(5));
}

#[test]
fn retry_metadata_serde_roundtrip_with_attempts() {
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![
            RetryAttempt {
                attempt: 0,
                error: "err a".into(),
                delay: Duration::from_millis(100),
            },
            RetryAttempt {
                attempt: 1,
                error: "err b".into(),
                delay: Duration::from_millis(200),
            },
        ],
        total_duration: Duration::from_millis(350),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: RetryMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.total_attempts, 3);
    assert_eq!(parsed.failed_attempts.len(), 2);
    assert_eq!(parsed.failed_attempts[0].attempt, 0);
    assert_eq!(parsed.failed_attempts[0].error, "err a");
    assert_eq!(parsed.failed_attempts[0].delay, Duration::from_millis(100));
    assert_eq!(parsed.failed_attempts[1].attempt, 1);
    assert_eq!(parsed.failed_attempts[1].error, "err b");
    assert_eq!(parsed.failed_attempts[1].delay, Duration::from_millis(200));
    assert_eq!(parsed.total_duration, Duration::from_millis(350));
}

#[test]
fn retry_attempt_serde_roundtrip() {
    let attempt = RetryAttempt {
        attempt: 5,
        error: "connection refused".into(),
        delay: Duration::from_millis(1600),
    };
    let json = serde_json::to_string(&attempt).unwrap();
    let parsed: RetryAttempt = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.attempt, 5);
    assert_eq!(parsed.error, "connection refused");
    assert_eq!(parsed.delay, Duration::from_millis(1600));
}

// ───────────────────────────────────────────────────────────────────
// 13. Edge cases
// ───────────────────────────────────────────────────────────────────

#[test]
fn edge_max_delay_zero_returns_zero() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::ZERO,
        ..RetryConfig::default()
    };
    assert_eq!(compute_delay(&cfg, 0), Duration::ZERO);
    assert_eq!(compute_delay(&cfg, 5), Duration::ZERO);
}

#[test]
fn edge_base_delay_zero() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::ZERO,
        max_delay: Duration::from_secs(10),
        ..RetryConfig::default()
    };
    for attempt in 0..10 {
        assert_eq!(compute_delay(&cfg, attempt), Duration::ZERO);
    }
}

#[test]
fn edge_very_large_base_delay_capped_by_max() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_secs(3600),
        max_delay: Duration::from_secs(10),
        ..RetryConfig::default()
    };
    assert_eq!(compute_delay(&cfg, 0), Duration::from_secs(10));
}

#[test]
fn edge_very_large_max_retries_config() {
    let cfg = RetryConfig {
        max_retries: u32::MAX,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        overall_timeout: Duration::from_secs(1),
        jitter_factor: 0.0,
    };
    assert_eq!(cfg.max_retries, u32::MAX);
}

#[tokio::test]
async fn edge_one_retry_means_two_total_attempts() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cfg = fast_cfg(1);

    let _: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(spawn_err())
            }
        },
        is_retryable,
    )
    .await;

    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn edge_success_on_last_retry() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cfg = fast_cfg(3); // 4 total attempts

    let result = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 3 {
                    Err::<&str, _>(spawn_err())
                } else {
                    Ok("last chance")
                }
            }
        },
        is_retryable,
    )
    .await;

    let outcome = result.unwrap();
    assert_eq!(outcome.value, "last chance");
    assert_eq!(outcome.metadata.total_attempts, 4);
    assert_eq!(outcome.metadata.failed_attempts.len(), 3);
}

#[tokio::test]
async fn edge_non_retryable_error_returns_original() {
    let cfg = fast_cfg(5);

    let result: Result<_, HostError> = retry_async(
        &cfg,
        || async { Err::<(), _>(HostError::Fatal("critical".into())) },
        is_retryable,
    )
    .await;

    match result.unwrap_err() {
        HostError::Fatal(msg) => assert_eq!(msg, "critical"),
        other => panic!("expected Fatal, got: {other}"),
    }
}

// ───────────────────────────────────────────────────────────────────
// 14. Concurrent retry operations don't interfere
// ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn concurrent_retries_independent() {
    let cfg = fast_cfg(2);

    let counter_a = Arc::new(AtomicU32::new(0));
    let counter_b = Arc::new(AtomicU32::new(0));

    let ca = counter_a.clone();
    let cb = counter_b.clone();

    let cfg_a = cfg.clone();
    let cfg_b = cfg.clone();

    let handle_a = tokio::spawn(async move {
        retry_async(
            &cfg_a,
            move || {
                let c = ca.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n < 1 {
                        Err::<i32, _>(spawn_err())
                    } else {
                        Ok(1)
                    }
                }
            },
            is_retryable,
        )
        .await
    });

    let handle_b = tokio::spawn(async move {
        retry_async(
            &cfg_b,
            move || {
                let c = cb.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        Err::<i32, _>(spawn_err())
                    } else {
                        Ok(2)
                    }
                }
            },
            is_retryable,
        )
        .await
    });

    let (ra, rb) = tokio::join!(handle_a, handle_b);
    let oa = ra.unwrap().unwrap();
    let ob = rb.unwrap().unwrap();

    assert_eq!(oa.value, 1);
    assert_eq!(ob.value, 2);
    assert_eq!(counter_a.load(Ordering::SeqCst), 2);
    assert_eq!(counter_b.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn concurrent_many_retries_all_succeed() {
    let cfg = fast_cfg(2);
    let mut handles = Vec::new();

    for i in 0..10u32 {
        let cfg = cfg.clone();
        handles.push(tokio::spawn(async move {
            let counter = Arc::new(AtomicU32::new(0));
            let c = counter.clone();
            let outcome = retry_async(
                &cfg,
                move || {
                    let c = c.clone();
                    async move {
                        let n = c.fetch_add(1, Ordering::SeqCst);
                        if n < 1 {
                            Err::<u32, _>(spawn_err())
                        } else {
                            Ok(i)
                        }
                    }
                },
                is_retryable,
            )
            .await
            .unwrap();
            outcome.value
        }));
    }

    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }
    results.sort();
    assert_eq!(results, (0..10).collect::<Vec<_>>());
}

// ───────────────────────────────────────────────────────────────────
// 15. Builder pattern (struct update syntax) for RetryConfig
// ───────────────────────────────────────────────────────────────────

#[test]
fn builder_pattern_partial_override() {
    let cfg = RetryConfig {
        max_retries: 5,
        ..RetryConfig::default()
    };
    assert_eq!(cfg.max_retries, 5);
    assert_eq!(cfg.base_delay, Duration::from_millis(100));
    assert_eq!(cfg.max_delay, Duration::from_secs(10));
}

#[test]
fn builder_pattern_override_delays_only() {
    let cfg = RetryConfig {
        base_delay: Duration::from_secs(1),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    assert_eq!(cfg.max_retries, 3);
    assert_eq!(cfg.base_delay, Duration::from_secs(1));
    assert_eq!(cfg.max_delay, Duration::from_secs(60));
    assert_eq!(cfg.overall_timeout, Duration::from_secs(60));
    assert!((cfg.jitter_factor - 0.5).abs() < f64::EPSILON);
}

#[test]
fn builder_pattern_override_jitter_only() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    assert!((cfg.jitter_factor - 0.0).abs() < f64::EPSILON);
    assert_eq!(cfg.max_retries, 3);
}

#[test]
fn clone_and_modify() {
    let base = RetryConfig::default();
    let mut modified = base.clone();
    modified.max_retries = 10;
    modified.jitter_factor = 0.0;
    assert_eq!(modified.max_retries, 10);
    assert_eq!(base.max_retries, 3); // original unchanged
}

// ───────────────────────────────────────────────────────────────────
// Additional: is_retryable coverage
// ───────────────────────────────────────────────────────────────────

#[test]
fn is_retryable_spawn() {
    assert!(is_retryable(&HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found"
    ))));
}

#[test]
fn is_retryable_stdout() {
    assert!(is_retryable(&HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken pipe"
    ))));
}

#[test]
fn is_retryable_exited() {
    assert!(is_retryable(&HostError::Exited { code: Some(1) }));
    assert!(is_retryable(&HostError::Exited { code: None }));
}

#[test]
fn is_retryable_sidecar_crashed() {
    assert!(is_retryable(&HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed".into(),
    }));
}

#[test]
fn is_retryable_timeout() {
    assert!(is_retryable(&HostError::Timeout {
        duration: Duration::from_secs(30),
    }));
}

#[test]
fn not_retryable_protocol_violation() {
    assert!(!is_retryable(&HostError::Protocol(
        ProtocolError::Violation("bad".into())
    )));
}

#[test]
fn not_retryable_violation() {
    assert!(!is_retryable(&HostError::Violation(
        "unexpected message".into()
    )));
}

#[test]
fn not_retryable_fatal() {
    assert!(!is_retryable(&HostError::Fatal("fatal".into())));
}

#[test]
fn not_retryable_stdin() {
    assert!(!is_retryable(&HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pipe"
    ))));
}

// ───────────────────────────────────────────────────────────────────
// Additional: retry_async edge cases with custom retryable fn
// ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn retry_async_custom_retryable_fn() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cfg = fast_cfg(3);

    // Only retry Timeout errors, not Spawn
    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Err::<(), _>(HostError::Timeout {
                        duration: Duration::from_secs(1),
                    })
                } else {
                    Err(spawn_err())
                }
            }
        },
        |err| matches!(err, HostError::Timeout { .. }),
    )
    .await;

    assert!(result.is_err());
    // First attempt: Timeout (retryable), second attempt: Spawn (not retryable) -> stops
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn retry_async_metadata_error_strings_preserved() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cfg = fast_cfg(2);

    let result = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err::<(), _>(HostError::Spawn(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("attempt {n}"),
                    )))
                } else {
                    Ok(())
                }
            }
        },
        is_retryable,
    )
    .await
    .unwrap();

    assert_eq!(result.metadata.failed_attempts.len(), 2);
    assert!(
        result.metadata.failed_attempts[0]
            .error
            .contains("attempt 0")
    );
    assert!(
        result.metadata.failed_attempts[1]
            .error
            .contains("attempt 1")
    );
}

#[tokio::test]
async fn retry_async_total_duration_increases_with_backoff() {
    let cfg = RetryConfig {
        max_retries: 2,
        base_delay: Duration::from_millis(20),
        max_delay: Duration::from_millis(100),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();

    let result = retry_async(
        &cfg,
        move || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err::<(), _>(spawn_err())
                } else {
                    Ok(())
                }
            }
        },
        is_retryable,
    )
    .await
    .unwrap();

    // Base delay: 20ms * 2^0 = 20ms, 20ms * 2^1 = 40ms → total backoff ≈ 60ms
    assert!(
        result.metadata.total_duration >= Duration::from_millis(40),
        "duration should reflect backoff waits: {:?}",
        result.metadata.total_duration
    );
}

// ───────────────────────────────────────────────────────────────────
// spawn_with_retry
// ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn spawn_with_retry_invalid_binary_is_spawn_error() {
    let cfg = fast_cfg(1);
    let spec = SidecarSpec::new("nonexistent-binary-deep-test");
    let result = spawn_with_retry(spec, &cfg).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Spawn(_)));
}

#[tokio::test]
async fn spawn_with_retry_invalid_binary_retries() {
    let cfg = RetryConfig {
        max_retries: 2,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let spec = SidecarSpec::new("nonexistent-binary-deep-test-2");

    // We can't directly count attempts in spawn_with_retry, but
    // we can verify it returns a Spawn error (it tried and all failed).
    let err = spawn_with_retry(spec, &cfg).await.unwrap_err();
    assert!(matches!(err, HostError::Spawn(_)));
}
