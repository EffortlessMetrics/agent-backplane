// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the retry / recovery layer in `abp_host::retry`.

use abp_host::retry::{
    RetryConfig, RetryMetadata, compute_delay, is_retryable, retry_async, spawn_with_retry,
};
use abp_host::{HostError, SidecarSpec};
use abp_protocol::ProtocolError;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fast_config(max_retries: u32) -> RetryConfig {
    RetryConfig {
        max_retries,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(50),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    }
}

fn python_cmd() -> Option<String> {
    for cmd in &["python3", "python"] {
        if std::process::Command::new(cmd)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(cmd.to_string());
        }
    }
    None
}

fn mock_script_path() -> String {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("tests")
        .join("mock_sidecar.py")
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------------------
// RetryConfig defaults
// ---------------------------------------------------------------------------

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
fn retry_config_serde_roundtrip() {
    let cfg = RetryConfig::default();
    let json = serde_json::to_string(&cfg).expect("serialize");
    let parsed: RetryConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.max_retries, cfg.max_retries);
    assert_eq!(parsed.base_delay, cfg.base_delay);
    assert_eq!(parsed.max_delay, cfg.max_delay);
}

// ---------------------------------------------------------------------------
// compute_delay
// ---------------------------------------------------------------------------

#[test]
fn compute_delay_exponential_without_jitter() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        ..RetryConfig::default()
    };

    assert_eq!(compute_delay(&cfg, 0), Duration::from_millis(100)); // 100 * 2^0
    assert_eq!(compute_delay(&cfg, 1), Duration::from_millis(200)); // 100 * 2^1
    assert_eq!(compute_delay(&cfg, 2), Duration::from_millis(400)); // 100 * 2^2
    assert_eq!(compute_delay(&cfg, 3), Duration::from_millis(800)); // 100 * 2^3
}

#[test]
fn compute_delay_caps_at_max_delay() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(500),
        ..RetryConfig::default()
    };

    // 100 * 2^3 = 800 > 500 → capped at 500
    assert_eq!(compute_delay(&cfg, 3), Duration::from_millis(500));
    assert_eq!(compute_delay(&cfg, 10), Duration::from_millis(500));
}

#[test]
fn compute_delay_with_jitter_stays_within_bounds() {
    let cfg = RetryConfig {
        jitter_factor: 1.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        ..RetryConfig::default()
    };

    for attempt in 0..10 {
        let delay = compute_delay(&cfg, attempt);
        let nominal = Duration::from_millis(
            (100u64 * 2u64.saturating_pow(attempt)).min(cfg.max_delay.as_millis() as u64),
        );
        // With full jitter, delay should be in [0, nominal].
        assert!(
            delay <= nominal,
            "delay {delay:?} should be <= nominal {nominal:?} at attempt {attempt}"
        );
    }
}

// ---------------------------------------------------------------------------
// is_retryable
// ---------------------------------------------------------------------------

#[test]
fn retryable_errors() {
    assert!(is_retryable(&HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found"
    ))));
    assert!(is_retryable(&HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pipe"
    ))));
    assert!(is_retryable(&HostError::Exited { code: Some(1) }));
    assert!(is_retryable(&HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "boom".into(),
    }));
    assert!(is_retryable(&HostError::Timeout {
        duration: Duration::from_secs(1),
    }));
}

#[test]
fn non_retryable_errors() {
    assert!(!is_retryable(&HostError::Protocol(
        ProtocolError::Violation("bad".into())
    )));
    assert!(!is_retryable(&HostError::Violation(
        "unexpected message".into()
    )));
    assert!(!is_retryable(&HostError::Fatal("fatal".into())));
}

// ---------------------------------------------------------------------------
// RetryMetadata
// ---------------------------------------------------------------------------

#[test]
fn retry_metadata_empty_to_receipt_metadata() {
    let meta = RetryMetadata {
        total_attempts: 1,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(42),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(1));
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(42));
    assert!(!map.contains_key("retry_failed_attempts"));
}

#[test]
fn retry_metadata_with_attempts_to_receipt_metadata() {
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
                error: "spawn failed again".into(),
                delay: Duration::from_millis(200),
            },
        ],
        total_duration: Duration::from_millis(350),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(3));
    let failed = map["retry_failed_attempts"].as_array().unwrap();
    assert_eq!(failed.len(), 2);
    assert_eq!(failed[0]["attempt"], 0);
    assert_eq!(failed[1]["delay_ms"], 200);
}

// ---------------------------------------------------------------------------
// retry_async — success on first attempt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_async_success_first_attempt() {
    let cfg = fast_config(3);
    let result = retry_async(&cfg, || async { Ok::<_, HostError>(42) }, is_retryable).await;

    let outcome = result.expect("should succeed");
    assert_eq!(outcome.value, 42);
    assert_eq!(outcome.metadata.total_attempts, 1);
    assert!(outcome.metadata.failed_attempts.is_empty());
}

// ---------------------------------------------------------------------------
// retry_async — success after retries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_async_success_after_retries() {
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();

    let cfg = fast_config(3);
    let result = retry_async(
        &cfg,
        move || {
            let c = counter_clone.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(HostError::Spawn(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "not found",
                    )))
                } else {
                    Ok(99)
                }
            }
        },
        is_retryable,
    )
    .await;

    let outcome = result.expect("should succeed on third attempt");
    assert_eq!(outcome.value, 99);
    assert_eq!(outcome.metadata.total_attempts, 3);
    assert_eq!(outcome.metadata.failed_attempts.len(), 2);
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

// ---------------------------------------------------------------------------
// retry_async — max retries exhausted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_async_max_retries_exhausted() {
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();

    let cfg = fast_config(2); // initial + 2 retries = 3 total
    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>(HostError::Exited { code: Some(1) })
            }
        },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), HostError::Exited { code: Some(1) }),
        "should return the last error"
    );
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

// ---------------------------------------------------------------------------
// retry_async — non-retryable error stops immediately
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_async_non_retryable_stops_immediately() {
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();

    let cfg = fast_config(5);
    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>(HostError::Fatal("non-retryable".into()))
            }
        },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Fatal(_)));
    assert_eq!(counter.load(Ordering::SeqCst), 1, "should only try once");
}

// ---------------------------------------------------------------------------
// retry_async — overall timeout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_async_overall_timeout() {
    let cfg = RetryConfig {
        max_retries: 100,
        base_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(50),
        overall_timeout: Duration::from_millis(120),
        jitter_factor: 0.0,
    };

    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();

    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>(HostError::Spawn(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                )))
            }
        },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    // Should be either the last Spawn error or a Timeout.
    assert!(
        matches!(err, HostError::Timeout { .. } | HostError::Spawn(_)),
        "expected Timeout or Spawn error, got: {err}"
    );
    // With 50ms delay, 120ms timeout → should manage at most a few attempts.
    let attempts = counter.load(Ordering::SeqCst);
    assert!(
        attempts <= 5,
        "should not attempt too many times, got {attempts}"
    );
}

// ---------------------------------------------------------------------------
// retry_async — zero retries means single attempt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_async_zero_retries() {
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();

    let cfg = fast_config(0);
    let result: Result<_, HostError> = retry_async(
        &cfg,
        move || {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>(HostError::Spawn(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                )))
            }
        },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(counter.load(Ordering::SeqCst), 1, "only one attempt");
}

// ---------------------------------------------------------------------------
// spawn_with_retry — invalid binary exhausts retries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn spawn_with_retry_invalid_binary_exhausts_retries() {
    let cfg = RetryConfig {
        max_retries: 2,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let spec = SidecarSpec::new("nonexistent-binary-retry-test-xyz");
    let result = spawn_with_retry(spec, &cfg).await;

    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), HostError::Spawn(_)),
        "should be a Spawn error"
    );
}

// ---------------------------------------------------------------------------
// spawn_with_retry — valid sidecar succeeds on first try
// ---------------------------------------------------------------------------

#[tokio::test]
async fn spawn_with_retry_success_first_try() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path()];

    let cfg = fast_config(3);
    let outcome = spawn_with_retry(spec, &cfg).await.expect("should succeed");

    assert_eq!(outcome.value.hello.backend.id, "mock-test");
    assert_eq!(outcome.metadata.total_attempts, 1);
    assert!(outcome.metadata.failed_attempts.is_empty());
}

// ---------------------------------------------------------------------------
// Metadata serde roundtrip
// ---------------------------------------------------------------------------

#[test]
fn retry_metadata_serde_roundtrip() {
    use abp_host::retry::RetryAttempt;

    let meta = RetryMetadata {
        total_attempts: 2,
        failed_attempts: vec![RetryAttempt {
            attempt: 0,
            error: "spawn failed".into(),
            delay: Duration::from_millis(100),
        }],
        total_duration: Duration::from_millis(150),
    };
    let json = serde_json::to_string(&meta).expect("serialize");
    let parsed: RetryMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.total_attempts, 2);
    assert_eq!(parsed.failed_attempts.len(), 1);
    assert_eq!(parsed.total_duration, Duration::from_millis(150));
}
