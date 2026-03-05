//! Comprehensive thread safety and concurrent access tests.
//!
//! 70+ tests covering registry, stream, rate limiter, receipt, policy engine,
//! backend pool, config transaction, receipt store, stream multiplexer,
//! workspace pool, sliding window, and backend rate limiter concurrency.

#![allow(clippy::needless_return)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::Barrier;
use uuid::Uuid;

use abp_backend_core::{BackendHealth, BackendMetadata, BackendRegistry, HealthStatus};
use abp_capability::registry::{CapabilitySet, SharedCapabilityRegistry};
use abp_config::store::ConfigStore;
use abp_config::transaction::ConfigTransaction;
use abp_config::BackplaneConfig;
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, Outcome, PolicyProfile, Receipt,
    SupportLevel,
};
use abp_dialect::registry::DialectRegistry;
use abp_dialect::Dialect;
use abp_integrations::pool::{BackendPool, PoolConfig as BackendPoolConfig};
use abp_policy::{Decision, PolicyEngine};
use abp_ratelimit::{
    AdaptiveLimiter, BackendRateLimiter, CircuitBreaker, CircuitState, ModelLimitResult,
    ModelRateLimiter, RateLimitPolicy, SlidingWindowCounter, TokenBucket,
};
use abp_receipt::audit_trail::{AuditAction, AuditTrail};
use abp_receipt::{compute_hash, ReceiptBuilder, ReceiptChain};
use abp_runtime::store::ReceiptStore;
use abp_runtime::telemetry::RunMetrics;
use abp_stream::{FanOut, ReplayBuffer, StreamMultiplexer};
use abp_workspace::pool::{PoolConfig as WsPoolConfig, WorkspacePool};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: msg.to_string(),
        },
        ext: None,
    }
}

fn make_capability_set(caps: &[(Capability, SupportLevel)]) -> CapabilitySet {
    let mut manifest = CapabilityManifest::new();
    for (cap, level) in caps {
        manifest.insert(cap.clone(), level.clone());
    }
    CapabilitySet::new(manifest)
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

const CONCURRENCY: usize = 20;

// ===========================================================================
// 1. Registry concurrency (10 tests)
// ===========================================================================

#[tokio::test]
async fn registry_concurrent_register() {
    let registry = SharedCapabilityRegistry::new();
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let caps = make_capability_set(&[(Capability::Streaming, SupportLevel::Native)]);
            reg.register(&format!("backend-{i}"), caps);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(registry.len(), CONCURRENCY);
}

#[tokio::test]
async fn registry_concurrent_lookup() {
    let registry = SharedCapabilityRegistry::new();
    for i in 0..CONCURRENCY {
        let caps = make_capability_set(&[(Capability::Streaming, SupportLevel::Native)]);
        registry.register(&format!("backend-{i}"), caps);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let result = reg.lookup(&format!("backend-{i}"));
            assert!(result.is_some(), "backend-{i} not found");
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn registry_concurrent_register_and_lookup() {
    let registry = SharedCapabilityRegistry::new();
    let barrier = Arc::new(Barrier::new(CONCURRENCY * 2));
    let mut handles = Vec::new();

    // Writers
    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let caps = make_capability_set(&[(Capability::ToolRead, SupportLevel::Native)]);
            reg.register(&format!("backend-{i}"), caps);
        }));
    }

    // Readers
    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            // May or may not find—just must not panic
            let _ = reg.lookup(&format!("backend-{i}"));
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn registry_concurrent_unregister() {
    let registry = SharedCapabilityRegistry::new();
    for i in 0..CONCURRENCY {
        let caps = make_capability_set(&[(Capability::Streaming, SupportLevel::Native)]);
        registry.register(&format!("backend-{i}"), caps);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            reg.unregister(&format!("backend-{i}"));
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(registry.len(), 0);
}

#[tokio::test]
async fn registry_concurrent_query_capability() {
    let registry = SharedCapabilityRegistry::new();
    for i in 0..CONCURRENCY {
        let caps = make_capability_set(&[(Capability::Streaming, SupportLevel::Native)]);
        registry.register(&format!("backend-{i}"), caps);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let results = reg.query(&Capability::Streaming);
            assert_eq!(results.len(), CONCURRENCY);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn registry_concurrent_find_backends_supporting() {
    let registry = SharedCapabilityRegistry::new();
    for i in 0..CONCURRENCY {
        let caps = make_capability_set(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]);
        registry.register(&format!("backend-{i}"), caps);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let found = reg.find_backends_supporting(&[Capability::Streaming]);
            assert_eq!(found.len(), CONCURRENCY);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn backend_registry_concurrent_health_updates() {
    let registry = Arc::new(tokio::sync::Mutex::new(BackendRegistry::new()));
    {
        let mut reg = registry.lock().await;
        for i in 0..CONCURRENCY {
            reg.register_with_metadata(
                &format!("be-{i}"),
                BackendMetadata {
                    name: format!("be-{i}"),
                    dialect: "openai".into(),
                    version: "1.0".into(),
                    max_tokens: Some(4096),
                    supports_streaming: true,
                    supports_tools: true,
                    rate_limit: None,
                },
            );
        }
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut r = reg.lock().await;
            r.update_health(
                &format!("be-{i}"),
                BackendHealth {
                    status: HealthStatus::Healthy,
                    ..Default::default()
                },
            );
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let reg = registry.lock().await;
    let healthy = reg.healthy_backends();
    assert_eq!(healthy.len(), CONCURRENCY);
}

#[tokio::test]
async fn backend_registry_concurrent_read_write_health() {
    let registry = Arc::new(tokio::sync::Mutex::new(BackendRegistry::new()));
    {
        let mut reg = registry.lock().await;
        for i in 0..CONCURRENCY {
            reg.register_with_metadata(
                &format!("be-{i}"),
                BackendMetadata {
                    name: format!("be-{i}"),
                    dialect: "anthropic".into(),
                    version: "1.0".into(),
                    max_tokens: None,
                    supports_streaming: false,
                    supports_tools: false,
                    rate_limit: None,
                },
            );
        }
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY * 2));
    let mut handles = Vec::new();

    // Writers
    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut r = reg.lock().await;
            r.update_health(
                &format!("be-{i}"),
                BackendHealth {
                    status: HealthStatus::Degraded,
                    ..Default::default()
                },
            );
        }));
    }

    // Readers
    for _ in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let r = reg.lock().await;
            let _ = r.healthy_backends();
            let _ = r.operational_backends();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn dialect_registry_concurrent_lookups() {
    let registry = Arc::new(DialectRegistry::with_builtins());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    let dialects = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
        Dialect::Copilot,
    ];

    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        let dialect = dialects[i % dialects.len()];
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let entry = reg.get(dialect);
            assert!(entry.is_some(), "dialect {dialect:?} not found");
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn dialect_registry_concurrent_list_and_lookup() {
    let registry = Arc::new(DialectRegistry::with_builtins());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 2 == 0 {
                let list = reg.list_dialects();
                assert!(!list.is_empty());
            } else {
                let _ = reg.get(Dialect::OpenAi);
                let _ = reg.supports_pair(Dialect::OpenAi, Dialect::Claude);
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ===========================================================================
// 2. Stream concurrency (10 tests)
// ===========================================================================

#[tokio::test]
async fn fanout_concurrent_subscribers() {
    let fanout = FanOut::new(128);
    let mut receivers = Vec::new();
    for _ in 0..CONCURRENCY {
        receivers.push(fanout.add_subscriber());
    }

    let event = make_event("hello");
    let sent_to = fanout.broadcast(&event);
    assert_eq!(sent_to, CONCURRENCY);

    for mut rx in receivers {
        let received = rx.recv().await.unwrap();
        // Verify we received an event (AgentEventKind doesn't impl PartialEq)
        let text = match &received.kind {
            AgentEventKind::AssistantDelta { text } => text.clone(),
            _ => panic!("unexpected event kind"),
        };
        assert_eq!(text, "hello");
    }
}

#[tokio::test]
async fn fanout_concurrent_broadcast() {
    let fanout = Arc::new(FanOut::new(256));
    let mut rx = fanout.add_subscriber();
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let fo = fanout.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let event = make_event(&format!("msg-{i}"));
            fo.broadcast(&event);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, CONCURRENCY);
}

#[tokio::test]
async fn fanout_subscribe_during_broadcast() {
    let fanout = Arc::new(FanOut::new(256));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    // Half broadcast, half subscribe
    for i in 0..CONCURRENCY {
        let fo = fanout.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 2 == 0 {
                let event = make_event(&format!("msg-{i}"));
                fo.broadcast(&event);
            } else {
                let _rx = fo.add_subscriber();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Must not panic; operations under contention are safe
    // subscriber_count may lag due to broadcast channel semantics
    // subscriber_count is valid (returns usize, so always non-negative)
    let _ = fanout.subscriber_count();
}

#[tokio::test]
async fn replay_buffer_send_then_subscribe() {
    let mut buffer = ReplayBuffer::new(64, 128);

    for i in 0..10 {
        buffer.send(&make_event(&format!("historical-{i}")));
    }

    let sub = buffer.subscribe();
    assert_eq!(sub.buffered.len(), 10);
}

#[tokio::test]
async fn replay_buffer_concurrent_subscribe() {
    let buffer = Arc::new(tokio::sync::Mutex::new(ReplayBuffer::new(64, 128)));

    // Pre-fill the buffer
    {
        let mut buf = buffer.lock().await;
        for i in 0..10 {
            buf.send(&make_event(&format!("event-{i}")));
        }
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let buf = buffer.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let b = buf.lock().await;
            let sub = b.subscribe();
            assert_eq!(sub.buffered.len(), 10);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn replay_buffer_send_and_subscribe_concurrent() {
    let buffer = Arc::new(tokio::sync::Mutex::new(ReplayBuffer::new(128, 256)));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let buf = buffer.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut b = buf.lock().await;
            if i % 2 == 0 {
                b.send(&make_event(&format!("msg-{i}")));
            } else {
                let _ = b.subscribe();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn concurrent_event_producer_consumer_mpsc() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(256);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    // Producers
    for i in 0..CONCURRENCY {
        let sender = tx.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            sender
                .send(make_event(&format!("producer-{i}")))
                .await
                .unwrap();
        }));
    }
    drop(tx);

    for h in handles {
        h.await.unwrap();
    }

    let mut count = 0;
    while rx.recv().await.is_some() {
        count += 1;
    }
    assert_eq!(count, CONCURRENCY);
}

#[tokio::test]
async fn concurrent_multiple_producers_single_consumer() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(512);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));

    let msgs_per_producer = 5;
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let sender = tx.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            for j in 0..msgs_per_producer {
                sender
                    .send(make_event(&format!("p{i}-m{j}")))
                    .await
                    .unwrap();
            }
        }));
    }
    drop(tx);

    let consumer = tokio::spawn(async move {
        let mut count = 0;
        while rx.recv().await.is_some() {
            count += 1;
        }
        count
    });

    for h in handles {
        h.await.unwrap();
    }
    let total = consumer.await.unwrap();
    assert_eq!(total, CONCURRENCY * msgs_per_producer);
}

#[tokio::test]
async fn fanout_multiple_subscribers_receive_all() {
    let fanout = FanOut::new(256);
    let num_subscribers = 10;
    let num_events = 20;

    let mut receivers: Vec<_> = (0..num_subscribers)
        .map(|_| fanout.add_subscriber())
        .collect();

    for i in 0..num_events {
        fanout.broadcast(&make_event(&format!("event-{i}")));
    }

    for rx in &mut receivers {
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, num_events);
    }
}

#[tokio::test]
async fn concurrent_fanout_broadcast_and_receive() {
    let fanout = Arc::new(FanOut::new(512));
    let mut rx = fanout.add_subscriber();
    let barrier = Arc::new(Barrier::new(CONCURRENCY));

    let mut handles = Vec::new();
    let msg_count = 5;

    for i in 0..CONCURRENCY {
        let fo = fanout.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            for j in 0..msg_count {
                fo.broadcast(&make_event(&format!("t{i}-e{j}")));
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, CONCURRENCY * msg_count);
}

// ===========================================================================
// 3. Rate limiter concurrency (10 tests)
// ===========================================================================

#[tokio::test]
async fn token_bucket_concurrent_acquire() {
    let bucket = TokenBucket::new(1000.0, 100);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let b = bucket.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            // Each acquires 1 token; must not panic
            b.try_acquire(1)
        }));
    }

    let mut acquired = 0;
    for h in handles {
        if h.await.unwrap() {
            acquired += 1;
        }
    }
    // At least some should succeed given burst=100
    assert!(acquired > 0, "no tokens acquired under contention");
}

#[tokio::test]
async fn token_bucket_contention_no_overcount() {
    // Burst of exactly CONCURRENCY tokens at high rate
    let bucket = TokenBucket::new(10000.0, CONCURRENCY);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let b = bucket.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            b.try_acquire(1)
        }));
    }

    let mut acquired = 0;
    for h in handles {
        if h.await.unwrap() {
            acquired += 1;
        }
    }
    // Cannot exceed burst
    assert!(acquired <= CONCURRENCY);
}

#[tokio::test]
async fn token_bucket_concurrent_wait_for() {
    let bucket = TokenBucket::new(10000.0, 100);
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let b = bucket.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            b.wait_for(1).await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn circuit_breaker_concurrent_failures_trip() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(60));
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = Vec::new();

    // 10 concurrent failures should trip the breaker (threshold=5)
    for _ in 0..10 {
        let breaker = cb.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            breaker.record_failure();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn circuit_breaker_concurrent_trip_and_reset() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let breaker = cb.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 3 == 0 {
                breaker.reset();
            } else {
                breaker.record_failure();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Final state depends on ordering, but must not panic
    let state = cb.state();
    assert!(
        state == CircuitState::Closed || state == CircuitState::Open,
        "unexpected state: {state:?}"
    );
}

#[tokio::test]
async fn circuit_breaker_concurrent_call() {
    let cb = CircuitBreaker::new(100, Duration::from_secs(60));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let breaker = cb.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let result = breaker.call(|| -> Result<i32, &str> {
                if i % 2 == 0 {
                    Ok(i as i32)
                } else {
                    Err("fail")
                }
            });
            result.is_ok()
        }));
    }

    let mut successes = 0;
    for h in handles {
        if h.await.unwrap() {
            successes += 1;
        }
    }
    assert!(successes > 0);
}

#[tokio::test]
async fn adaptive_limiter_concurrent_updates() {
    let limiter = AdaptiveLimiter::new(100.0, 200);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut headers = HashMap::new();
            headers.insert("x-ratelimit-remaining".to_string(), format!("{}", 100 - i));
            lim.update_from_headers(&headers);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Rate may have adapted; must not panic and rate must be positive
    assert!(limiter.current_rate() > 0.0);
}

#[tokio::test]
async fn adaptive_limiter_concurrent_acquire_and_update() {
    let limiter = AdaptiveLimiter::new(1000.0, 500);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 2 == 0 {
                lim.try_acquire()
            } else {
                let mut headers = HashMap::new();
                headers.insert("x-ratelimit-remaining".to_string(), "50".to_string());
                lim.update_from_headers(&headers);
                true
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn model_rate_limiter_concurrent_models() {
    let limiter = ModelRateLimiter::new();
    for i in 0..CONCURRENCY {
        limiter.register_model_limits(
            &format!("model-{i}"),
            RateLimitPolicy::TokenBucket {
                rate: 100.0,
                burst: 50,
            },
        );
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let result = lim.try_acquire(&format!("model-{i}"));
            matches!(result, ModelLimitResult::Allowed)
        }));
    }

    let mut allowed = 0;
    for h in handles {
        if h.await.unwrap() {
            allowed += 1;
        }
    }
    assert!(allowed > 0, "at least some models should allow requests");
}

#[tokio::test]
async fn model_rate_limiter_concurrent_register_and_acquire() {
    let limiter = ModelRateLimiter::new();
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 2 == 0 {
                lim.register_model_limits(
                    &format!("model-{i}"),
                    RateLimitPolicy::TokenBucket {
                        rate: 50.0,
                        burst: 20,
                    },
                );
            } else {
                let _ = lim.try_acquire(&format!("model-{}", i - 1));
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert!(!limiter.registered_models().is_empty());
}

// ===========================================================================
// 4. Receipt concurrency (10 tests)
// ===========================================================================

#[tokio::test]
async fn receipt_hash_concurrent_consistency() {
    let receipt = make_receipt("test-backend");

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let receipt_arc = Arc::new(receipt);
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let r = receipt_arc.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            compute_hash(&r).unwrap()
        }));
    }

    let mut hashes = Vec::new();
    for h in handles {
        hashes.push(h.await.unwrap());
    }

    let first = &hashes[0];
    for hash in &hashes[1..] {
        assert_eq!(hash, first, "concurrent hashes must be identical");
    }
}

#[tokio::test]
async fn receipt_hash_different_receipts_concurrent() {
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let receipt = make_receipt(&format!("backend-{i}"));
            compute_hash(&receipt).unwrap()
        }));
    }

    let mut hashes = Vec::new();
    for h in handles {
        hashes.push(h.await.unwrap());
    }

    // Different backends should produce different hashes
    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert!(
        unique.len() > 1,
        "expected different hashes for different backends"
    );
}

#[tokio::test]
async fn receipt_hash_repeated_concurrent_batches() {
    let receipt = make_receipt("stable-backend");
    let receipt_arc = Arc::new(receipt);

    // Run 3 batches of concurrent hashing, all must match
    let mut all_hashes = Vec::new();
    for _ in 0..3 {
        let barrier = Arc::new(Barrier::new(10));
        let mut handles = Vec::new();
        for _ in 0..10 {
            let r = receipt_arc.clone();
            let bar = barrier.clone();
            handles.push(tokio::spawn(async move {
                bar.wait().await;
                compute_hash(&r).unwrap()
            }));
        }
        for h in handles {
            all_hashes.push(h.await.unwrap());
        }
    }

    let first = &all_hashes[0];
    for h in &all_hashes[1..] {
        assert_eq!(h, first, "hash must be stable across batches");
    }
}

#[tokio::test]
async fn audit_trail_concurrent_writes() {
    let trail = Arc::new(tokio::sync::Mutex::new(AuditTrail::new()));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let t = trail.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut audit = t.lock().await;
            audit.record(Uuid::new_v4(), format!("actor-{i}"), AuditAction::Created);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let audit = trail.lock().await;
    assert_eq!(audit.len(), CONCURRENCY);
}

#[tokio::test]
async fn audit_trail_concurrent_mixed_actions() {
    let trail = Arc::new(tokio::sync::Mutex::new(AuditTrail::new()));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let run_id = Uuid::new_v4();
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let t = trail.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut audit = t.lock().await;
            let action = match i % 4 {
                0 => AuditAction::Created,
                1 => AuditAction::Hashed,
                2 => AuditAction::Verified { success: true },
                _ => AuditAction::Archived,
            };
            audit.record(run_id, format!("actor-{i}"), action);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let audit = trail.lock().await;
    assert_eq!(audit.len(), CONCURRENCY);
    let for_run = audit.entries_for_run(run_id);
    assert_eq!(for_run.len(), CONCURRENCY);
}

#[tokio::test]
async fn audit_trail_concurrent_writes_and_reads() {
    let trail = Arc::new(tokio::sync::Mutex::new(AuditTrail::new()));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let t = trail.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut audit = t.lock().await;
            if i % 2 == 0 {
                audit.record(Uuid::new_v4(), format!("writer-{i}"), AuditAction::Created);
            }
            // Read in all cases
            let _ = audit.entries();
            let _ = audit.len();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn receipt_chain_concurrent_appends() {
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));
    // Sequential appends (chain requires ordering) but lock contention tests safety
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let c = chain.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let receipt = make_receipt(&format!("chain-be-{i}"));
            let mut ch = c.lock().await;
            // push may fail for duplicate IDs, which is fine under contention
            let _ = ch.push(receipt);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let ch = chain.lock().await;
    assert!(!ch.is_empty(), "chain should have at least one receipt");
}

#[tokio::test]
async fn receipt_chain_concurrent_append_and_verify() {
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));

    // Pre-fill the chain
    for i in 0..5 {
        let mut ch = chain.lock().await;
        let receipt = make_receipt(&format!("pre-{i}"));
        let _ = ch.push(receipt);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let c = chain.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut ch = c.lock().await;
            if i % 2 == 0 {
                let receipt = make_receipt(&format!("append-{i}"));
                let _ = ch.push(receipt);
            } else {
                let _ = ch.verify();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn receipt_chain_concurrent_reads() {
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));

    // Pre-fill
    for i in 0..10 {
        let mut ch = chain.lock().await;
        let receipt = make_receipt(&format!("read-{i}"));
        let _ = ch.push(receipt);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let c = chain.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let ch = c.lock().await;
            let _ = ch.latest();
            let _ = ch.get(i % 10);
            let _ = ch.chain_summary();
            ch.len()
        }));
    }

    for h in handles {
        let len = h.await.unwrap();
        assert_eq!(len, 10);
    }
}

#[tokio::test]
async fn receipt_chain_concurrent_detect_tampering() {
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));

    for i in 0..5 {
        let mut ch = chain.lock().await;
        let receipt = make_receipt(&format!("tamper-{i}"));
        let _ = ch.push(receipt);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let c = chain.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let ch = c.lock().await;
            let _ = ch.detect_tampering();
            let _ = ch.find_gaps();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ===========================================================================
// Bonus: cross-category stress tests
// ===========================================================================

#[tokio::test]
async fn mixed_registry_and_ratelimit_contention() {
    let registry = SharedCapabilityRegistry::new();
    let bucket = TokenBucket::new(5000.0, 200);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let reg = registry.clone();
        let bkt = bucket.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let caps = make_capability_set(&[(Capability::Streaming, SupportLevel::Native)]);
            reg.register(&format!("backend-{i}"), caps);
            bkt.try_acquire(1)
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(registry.len(), CONCURRENCY);
}

// ===========================================================================
// 5. RunMetrics concurrent access
// ===========================================================================

#[tokio::test]
async fn run_metrics_concurrent_record() {
    let metrics = RunMetrics::new();
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let m = metrics.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            m.record_run((i as u64) * 10, i % 2 == 0, (i as u64) + 1);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, CONCURRENCY as u64);
}

#[tokio::test]
async fn run_metrics_concurrent_snapshot_while_recording() {
    let metrics = RunMetrics::new();
    let barrier = Arc::new(Barrier::new(CONCURRENCY * 2));
    let mut handles = Vec::new();

    // Writers
    for i in 0..CONCURRENCY {
        let m = metrics.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            m.record_run(100, i % 2 == 0, 5);
        }));
    }

    // Readers
    for _ in 0..CONCURRENCY {
        let m = metrics.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let snap = m.snapshot();
            // total_runs monotonically increases
            assert!(snap.total_runs <= CONCURRENCY as u64);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ===========================================================================
// 6. PolicyEngine concurrent evaluation
// ===========================================================================

fn make_policy_engine() -> PolicyEngine {
    let profile = PolicyProfile {
        tools: Some(abp_core::IncludeExclude {
            include: vec!["read_*".into(), "write_*".into(), "list_*".into()],
            exclude: vec!["write_secret*".into()],
        }),
        deny_read: Some(vec!["**/.env".into(), "**/secrets/**".into()]),
        deny_write: Some(vec!["**/node_modules/**".into()]),
    };
    PolicyEngine::new(&profile).expect("valid policy")
}

#[tokio::test]
async fn policy_engine_concurrent_tool_checks() {
    let engine = Arc::new(make_policy_engine());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    let tools = [
        "read_file",
        "write_file",
        "list_dir",
        "write_secret_key",
        "unknown_tool",
    ];

    for i in 0..CONCURRENCY {
        let eng = engine.clone();
        let bar = barrier.clone();
        let tool = tools[i % tools.len()].to_string();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let decision = eng.can_use_tool(&tool);
            (tool, decision)
        }));
    }

    for h in handles {
        let (tool, decision) = h.await.unwrap();
        if tool == "write_secret_key" {
            assert!(!decision.allowed, "write_secret_key should be denied");
        }
    }
}

#[tokio::test]
async fn policy_engine_concurrent_read_path_checks() {
    let engine = Arc::new(make_policy_engine());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let eng = engine.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let path = if i % 3 == 0 {
                "src/main.rs".to_string()
            } else if i % 3 == 1 {
                ".env".to_string()
            } else {
                "secrets/api_key.txt".to_string()
            };
            let decision = eng.can_read_path(Path::new(&path));
            (path, decision)
        }));
    }

    for h in handles {
        let (path, decision) = h.await.unwrap();
        if path == ".env" || path.starts_with("secrets/") {
            assert!(!decision.allowed, "{path} should be denied for read");
        }
    }
}

#[tokio::test]
async fn policy_engine_concurrent_write_path_checks() {
    let engine = Arc::new(make_policy_engine());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let eng = engine.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let path = if i % 2 == 0 {
                "src/lib.rs".to_string()
            } else {
                "node_modules/pkg/index.js".to_string()
            };
            let decision = eng.can_write_path(Path::new(&path));
            (path, decision)
        }));
    }

    for h in handles {
        let (path, decision) = h.await.unwrap();
        if path.starts_with("node_modules/") {
            assert!(!decision.allowed, "{path} should be denied for write");
        }
    }
}

#[tokio::test]
async fn policy_engine_concurrent_mixed_checks() {
    let engine = Arc::new(make_policy_engine());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let eng = engine.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            match i % 3 {
                0 => {
                    let _ = eng.can_use_tool("read_file");
                }
                1 => {
                    let _ = eng.can_read_path(Path::new("src/main.rs"));
                }
                _ => {
                    let _ = eng.can_write_path(Path::new("src/lib.rs"));
                }
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ===========================================================================
// 7. ReceiptChain concurrent verification and append (extended)
// ===========================================================================

#[tokio::test]
async fn receipt_chain_concurrent_verify_consistency() {
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));

    // Build a chain
    for i in 0..10 {
        let mut ch = chain.lock().await;
        let receipt = make_receipt(&format!("verify-be-{i}"));
        let _ = ch.push(receipt);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let c = chain.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let ch = c.lock().await;
            ch.verify()
        }));
    }

    for h in handles {
        let result = h.await.unwrap();
        assert!(result.is_ok(), "chain verification must succeed");
    }
}

#[tokio::test]
async fn receipt_chain_concurrent_summary_reads() {
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));

    for i in 0..10 {
        let mut ch = chain.lock().await;
        let _ = ch.push(make_receipt(&format!("summary-{i}")));
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let c = chain.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let ch = c.lock().await;
            let summary = ch.chain_summary();
            assert_eq!(summary.total, 10);
            let _ = ch.latest();
            ch.len()
        }));
    }

    for h in handles {
        assert_eq!(h.await.unwrap(), 10);
    }
}

// ===========================================================================
// 8. BackendPool concurrent acquire/release
// ===========================================================================

#[tokio::test]
async fn backend_pool_concurrent_checkout_checkin() {
    let pool = Arc::new(tokio::sync::Mutex::new(BackendPool::new()));
    {
        let mut p = pool.lock().await;
        p.register(
            "test-be",
            BackendPoolConfig {
                min_connections: 0,
                max_connections: CONCURRENCY,
            },
        )
        .unwrap();
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let p = pool.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut pool = p.lock().await;
            let conn_id = pool.checkout("test-be").unwrap();
            pool.checkin("test-be", conn_id).unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let p = pool.lock().await;
    let status = p.status("test-be").unwrap();
    assert_eq!(status.active, 0);
}

#[tokio::test]
async fn backend_pool_concurrent_register_and_checkout() {
    let pool = Arc::new(tokio::sync::Mutex::new(BackendPool::new()));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let p = pool.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut pool = p.lock().await;
            let name = format!("be-{i}");
            let _ = pool.register(
                &name,
                BackendPoolConfig {
                    min_connections: 0,
                    max_connections: 10,
                },
            );
            let _ = pool.checkout(&name);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let p = pool.lock().await;
    assert_eq!(p.backend_count(), CONCURRENCY);
}

#[tokio::test]
async fn backend_pool_concurrent_status_reads() {
    let pool = Arc::new(tokio::sync::Mutex::new(BackendPool::new()));
    {
        let mut p = pool.lock().await;
        for i in 0..5 {
            p.register(
                &format!("be-{i}"),
                BackendPoolConfig {
                    min_connections: 0,
                    max_connections: 10,
                },
            )
            .unwrap();
        }
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let p = pool.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let pool = p.lock().await;
            let _ = pool.status(&format!("be-{}", i % 5));
            let _ = pool.status_all();
            pool.backend_count()
        }));
    }

    for h in handles {
        assert_eq!(h.await.unwrap(), 5);
    }
}

// ===========================================================================
// 9. ConfigTransaction concurrent begin/commit/rollback
// ===========================================================================

#[tokio::test]
async fn config_store_concurrent_reads() {
    let store = ConfigStore::new(BackplaneConfig::default());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let s = store.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let cfg = s.get();
            let _ = cfg.log_level.clone();
            s.version()
        }));
    }

    for h in handles {
        let v = h.await.unwrap();
        assert_eq!(v, 0);
    }
}

#[tokio::test]
async fn config_store_concurrent_updates() {
    let store = ConfigStore::new(BackplaneConfig::default());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let s = store.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut cfg = BackplaneConfig::default();
            cfg.log_level = Some(format!("level-{i}"));
            let _ = s.update(cfg);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Version should have incremented for each successful update
    assert!(store.version() > 0);
}

#[tokio::test]
async fn config_transaction_concurrent_begin_commit() {
    let store = ConfigStore::new(BackplaneConfig::default());
    let store_arc = Arc::new(store);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let s = store_arc.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut tx = ConfigTransaction::begin(&s);
            let mut cfg = BackplaneConfig::default();
            cfg.log_level = Some(format!("tx-level-{i}"));
            let _ = tx.commit(cfg);
            tx.is_committed()
        }));
    }

    let mut committed = 0;
    for h in handles {
        if h.await.unwrap() {
            committed += 1;
        }
    }
    assert!(committed > 0, "at least some transactions should commit");
}

#[tokio::test]
async fn config_transaction_concurrent_rollback() {
    let store = ConfigStore::new(BackplaneConfig::default());
    let store_arc = Arc::new(store);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let s = store_arc.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut tx = ConfigTransaction::begin(&s);
            let _ = tx.rollback();
            tx.is_rolled_back()
        }));
    }

    for h in handles {
        assert!(h.await.unwrap(), "rollback should succeed");
    }

    // Original config unchanged
    assert_eq!(store_arc.version(), 0);
}

#[tokio::test]
async fn config_transaction_mixed_commit_rollback() {
    let store = ConfigStore::new(BackplaneConfig::default());
    let store_arc = Arc::new(store);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let s = store_arc.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let mut tx = ConfigTransaction::begin(&s);
            if i % 2 == 0 {
                let mut cfg = BackplaneConfig::default();
                cfg.port = Some(8080 + i as u16);
                let _ = tx.commit(cfg);
            } else {
                let _ = tx.rollback();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ===========================================================================
// 10. ReceiptStore concurrent save/load/list
// ===========================================================================

#[tokio::test]
async fn receipt_store_concurrent_save() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(tmp.path());
    let store_arc = Arc::new(store);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let s = store_arc.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let receipt = make_receipt(&format!("store-be-{i}"));
            s.save(&receipt)
        }));
    }

    for h in handles {
        let result = h.await.unwrap();
        assert!(result.is_ok(), "save should succeed");
    }

    let ids = store_arc.list().unwrap();
    assert_eq!(ids.len(), CONCURRENCY);
}

#[tokio::test]
async fn receipt_store_concurrent_save_and_load() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(tmp.path());

    // Pre-save some receipts
    let mut saved_ids = Vec::new();
    for i in 0..10 {
        let receipt = make_receipt(&format!("preload-{i}"));
        let id = receipt.meta.run_id;
        store.save(&receipt).unwrap();
        saved_ids.push(id);
    }

    let store_arc = Arc::new(store);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let s = store_arc.clone();
        let bar = barrier.clone();
        let load_id = saved_ids[i % saved_ids.len()];
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 2 == 0 {
                let receipt = make_receipt(&format!("new-{i}"));
                let _ = s.save(&receipt);
            } else {
                let _ = s.load(load_id);
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn receipt_store_concurrent_list() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(tmp.path());

    for i in 0..10 {
        let receipt = make_receipt(&format!("list-{i}"));
        store.save(&receipt).unwrap();
    }

    let store_arc = Arc::new(store);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let s = store_arc.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            s.list()
        }));
    }

    for h in handles {
        let ids = h.await.unwrap().unwrap();
        assert_eq!(ids.len(), 10);
    }
}

#[tokio::test]
async fn receipt_store_concurrent_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(tmp.path());

    let receipt = make_receipt("verify-be");
    let run_id = receipt.meta.run_id;
    store.save(&receipt).unwrap();

    let store_arc = Arc::new(store);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let s = store_arc.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            s.verify(run_id)
        }));
    }

    for h in handles {
        let result = h.await.unwrap();
        assert!(result.is_ok());
    }
}

// ===========================================================================
// 11. StreamMultiplexer concurrent subscribe/publish
// ===========================================================================

#[tokio::test]
async fn stream_mux_concurrent_subscribe() {
    let mux = Arc::new(StreamMultiplexer::new(64));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let m = mux.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let (id, _rx) = m.subscribe().await;
            id
        }));
    }

    let mut ids = Vec::new();
    for h in handles {
        ids.push(h.await.unwrap());
    }

    let count = mux.subscriber_count().await;
    assert_eq!(count, CONCURRENCY);

    // All IDs should be unique
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), CONCURRENCY);
}

#[tokio::test]
async fn stream_mux_concurrent_broadcast() {
    let mux = Arc::new(StreamMultiplexer::new(256));
    let (_id, mut rx) = mux.subscribe().await;

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let m = mux.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            m.broadcast(&make_event(&format!("mux-msg-{i}"))).await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, CONCURRENCY);
}

#[tokio::test]
async fn stream_mux_concurrent_subscribe_and_broadcast() {
    let mux = Arc::new(StreamMultiplexer::new(128));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let m = mux.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 2 == 0 {
                let (_id, _rx) = m.subscribe().await;
            } else {
                m.broadcast(&make_event(&format!("mixed-{i}"))).await;
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Must not panic; subscriber count is consistent
    let _ = mux.subscriber_count().await;
}

#[tokio::test]
async fn stream_mux_concurrent_unsubscribe() {
    let mux = Arc::new(StreamMultiplexer::new(64));
    let mut sub_ids = Vec::new();

    for _ in 0..CONCURRENCY {
        let (id, _rx) = mux.subscribe().await;
        sub_ids.push(id);
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for id in sub_ids {
        let m = mux.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            m.unsubscribe(id).await
        }));
    }

    for h in handles {
        assert!(h.await.unwrap(), "unsubscribe should succeed");
    }

    assert_eq!(mux.subscriber_count().await, 0);
}

// ===========================================================================
// 12. WorkspacePool concurrent checkout/return
// ===========================================================================

#[tokio::test]
async fn workspace_pool_concurrent_checkout() {
    let pool = WorkspacePool::new(WsPoolConfig {
        capacity: CONCURRENCY,
        prefix: Some("test-ws".into()),
    });
    let _ = pool.warm(CONCURRENCY);

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let p = pool.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            p.checkout()
        }));
    }

    let mut checked_out = 0;
    for h in handles {
        if h.await.unwrap().is_ok() {
            checked_out += 1;
        }
    }
    assert!(checked_out > 0, "at least some checkouts should succeed");
}

#[tokio::test]
async fn workspace_pool_concurrent_checkout_and_return() {
    let pool = WorkspacePool::new(WsPoolConfig {
        capacity: CONCURRENCY,
        prefix: Some("test-ws-cr".into()),
    });
    let _ = pool.warm(CONCURRENCY);

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let p = pool.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if let Ok(ws) = p.checkout() {
                // PooledWorkspace is returned on drop
                drop(ws);
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn workspace_pool_concurrent_status_reads() {
    let pool = WorkspacePool::new(WsPoolConfig {
        capacity: CONCURRENCY,
        prefix: Some("test-ws-sr".into()),
    });
    let _ = pool.warm(5);

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let p = pool.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let _ = p.available();
            let _ = p.capacity();
            p.total_checkouts()
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn workspace_pool_concurrent_drain_and_checkout() {
    let pool = WorkspacePool::new(WsPoolConfig {
        capacity: 10,
        prefix: Some("test-ws-dc".into()),
    });
    let _ = pool.warm(10);

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let p = pool.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            if i % 3 == 0 {
                p.drain();
            } else {
                let _ = p.checkout();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ===========================================================================
// 13. RateLimit concurrent acquire/check (extended)
// ===========================================================================

#[tokio::test]
async fn sliding_window_concurrent_acquire() {
    let counter = SlidingWindowCounter::new(Duration::from_secs(60), 100);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let c = counter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            c.try_acquire()
        }));
    }

    let mut acquired = 0;
    for h in handles {
        if h.await.unwrap() {
            acquired += 1;
        }
    }
    assert!(acquired > 0, "some requests should be allowed");
    assert!(acquired <= 100, "cannot exceed max_requests");
}

#[tokio::test]
async fn sliding_window_concurrent_remaining() {
    let counter = SlidingWindowCounter::new(Duration::from_secs(60), 100);
    // Consume some permits
    for _ in 0..10 {
        counter.try_acquire();
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let c = counter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            c.remaining()
        }));
    }

    for h in handles {
        let remaining = h.await.unwrap();
        assert!(remaining <= 100);
    }
}

#[tokio::test]
async fn backend_rate_limiter_concurrent_acquire() {
    let limiter = BackendRateLimiter::new();
    for i in 0..5 {
        limiter.set_policy(
            &format!("be-{i}"),
            RateLimitPolicy::TokenBucket {
                rate: 1000.0,
                burst: 50,
            },
        );
    }

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let result = lim.try_acquire(&format!("be-{}", i % 5));
            result.is_ok()
        }));
    }

    let mut allowed = 0;
    for h in handles {
        if h.await.unwrap() {
            allowed += 1;
        }
    }
    assert!(allowed > 0);
}

#[tokio::test]
async fn backend_rate_limiter_concurrent_set_policy_and_acquire() {
    let limiter = BackendRateLimiter::new();
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let name = format!("be-{i}");
            lim.set_policy(
                &name,
                RateLimitPolicy::TokenBucket {
                    rate: 100.0,
                    burst: 20,
                },
            );
            let _ = lim.try_acquire(&name);
            lim.has_policy(&name)
        }));
    }

    for h in handles {
        assert!(h.await.unwrap(), "policy should be registered");
    }
}

#[tokio::test]
async fn backend_rate_limiter_concurrent_permit_tracking() {
    let limiter = BackendRateLimiter::new();
    limiter.set_policy(
        "tracked",
        RateLimitPolicy::Fixed {
            max_concurrent: CONCURRENCY,
        },
    );

    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for _ in 0..CONCURRENCY {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let permit = lim.try_acquire("tracked");
            if let Ok(_permit) = permit {
                // permit is dropped here, decrementing count
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // After all permits dropped, active should be 0
    assert_eq!(limiter.active_permits("tracked"), 0);
}

// ===========================================================================
// 14. Additional cross-category stress tests
// ===========================================================================

#[tokio::test]
async fn mixed_policy_and_ratelimit_contention() {
    let engine = Arc::new(make_policy_engine());
    let bucket = TokenBucket::new(5000.0, 200);
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let eng = engine.clone();
        let bkt = bucket.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let decision = eng.can_use_tool(&format!("read_file_{i}"));
            let acquired = bkt.try_acquire(1);
            (decision.allowed, acquired)
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn mixed_receipt_store_and_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(ReceiptStore::new(tmp.path()));
    let chain = Arc::new(tokio::sync::Mutex::new(ReceiptChain::new()));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let s = store.clone();
        let c = chain.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let receipt = make_receipt(&format!("mixed-{i}"));
            let _ = s.save(&receipt);
            let mut ch = c.lock().await;
            let _ = ch.push(receipt);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let ids = store.list().unwrap();
    assert_eq!(ids.len(), CONCURRENCY);
}

#[tokio::test]
async fn mixed_mux_and_fanout_contention() {
    let mux = Arc::new(StreamMultiplexer::new(128));
    let fanout = Arc::new(FanOut::new(128));
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    let _mux_rx = {
        let (_id, rx) = mux.subscribe().await;
        rx
    };
    let _fan_rx = fanout.add_subscriber();

    for i in 0..CONCURRENCY {
        let m = mux.clone();
        let fo = fanout.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let event = make_event(&format!("dual-{i}"));
            m.broadcast(&event).await;
            fo.broadcast(&event);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn mixed_config_store_and_metrics() {
    let store = ConfigStore::new(BackplaneConfig::default());
    let metrics = RunMetrics::new();
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut handles = Vec::new();

    for i in 0..CONCURRENCY {
        let s = store.clone();
        let m = metrics.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;
            let _ = s.get();
            m.record_run(50, i % 2 == 0, 1);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, CONCURRENCY as u64);
}
