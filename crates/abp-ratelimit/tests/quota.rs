//! Integration tests for `QuotaManager`.

use std::sync::Arc;
use std::time::Duration;

use abp_ratelimit::{QuotaLimit, QuotaManager, QuotaResult};

// ---------------------------------------------------------------------------
// Basic quota behaviour
// ---------------------------------------------------------------------------

#[test]
fn no_limits_means_unlimited() {
    let mgr = QuotaManager::new();
    assert!(matches!(
        mgr.try_consume("any", 100),
        QuotaResult::Allowed {
            remaining: u64::MAX
        }
    ));
}

#[test]
fn single_limit_allows_then_exhausts() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 5,
            period: Duration::from_secs(60),
        }],
    );
    for i in (0..5).rev() {
        match mgr.try_consume("key", 1) {
            QuotaResult::Allowed { remaining } => assert_eq!(remaining, i as u64),
            other => panic!("expected Allowed, got {other:?}"),
        }
    }
    assert!(matches!(
        mgr.try_consume("key", 1),
        QuotaResult::Exhausted { .. }
    ));
}

#[test]
fn batch_consume() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 100,
            period: Duration::from_secs(60),
        }],
    );
    assert!(matches!(
        mgr.try_consume("key", 60),
        QuotaResult::Allowed { remaining: 40 }
    ));
    assert!(matches!(
        mgr.try_consume("key", 41),
        QuotaResult::Exhausted { .. }
    ));
}

// ---------------------------------------------------------------------------
// Multi-bucket most-constrained
// ---------------------------------------------------------------------------

#[test]
fn most_constrained_bucket_wins() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![
            QuotaLimit {
                limit: 1000,
                period: Duration::from_secs(3600),
            },
            QuotaLimit {
                limit: 3,
                period: Duration::from_secs(60),
            },
        ],
    );
    for _ in 0..3 {
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Allowed { .. }
        ));
    }
    assert!(matches!(
        mgr.try_consume("key", 1),
        QuotaResult::Exhausted { .. }
    ));
}

// ---------------------------------------------------------------------------
// Default limits
// ---------------------------------------------------------------------------

#[test]
fn default_limits_for_unknown_keys() {
    let mgr = QuotaManager::new();
    mgr.set_default_limits(vec![QuotaLimit {
        limit: 2,
        period: Duration::from_secs(60),
    }]);
    assert!(matches!(
        mgr.try_consume("alpha", 1),
        QuotaResult::Allowed { .. }
    ));
    assert!(matches!(
        mgr.try_consume("alpha", 1),
        QuotaResult::Allowed { .. }
    ));
    assert!(matches!(
        mgr.try_consume("alpha", 1),
        QuotaResult::Exhausted { .. }
    ));
}

#[test]
fn explicit_overrides_defaults() {
    let mgr = QuotaManager::new();
    mgr.set_default_limits(vec![QuotaLimit {
        limit: 1,
        period: Duration::from_secs(60),
    }]);
    mgr.set_limits(
        "special",
        vec![QuotaLimit {
            limit: 100,
            period: Duration::from_secs(60),
        }],
    );
    for _ in 0..50 {
        assert!(matches!(
            mgr.try_consume("special", 1),
            QuotaResult::Allowed { .. }
        ));
    }
}

// ---------------------------------------------------------------------------
// Remaining
// ---------------------------------------------------------------------------

#[test]
fn remaining_tracks_consumption() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 10,
            period: Duration::from_secs(60),
        }],
    );
    mgr.try_consume("key", 0); // force bucket creation
    assert_eq!(mgr.remaining("key"), 10);
    mgr.try_consume("key", 7);
    assert_eq!(mgr.remaining("key"), 3);
}

#[test]
fn remaining_unknown_key_is_max() {
    let mgr = QuotaManager::new();
    assert_eq!(mgr.remaining("nope"), u64::MAX);
}

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

#[test]
fn reset_clears_single_key() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 1,
            period: Duration::from_secs(60),
        }],
    );
    mgr.try_consume("key", 1);
    mgr.reset("key");
    // After reset, key is gone → unlimited
    assert!(matches!(
        mgr.try_consume("key", 1),
        QuotaResult::Allowed {
            remaining: u64::MAX
        }
    ));
}

#[test]
fn reset_all_clears_everything() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "a",
        vec![QuotaLimit {
            limit: 1,
            period: Duration::from_secs(60),
        }],
    );
    mgr.set_limits(
        "b",
        vec![QuotaLimit {
            limit: 1,
            period: Duration::from_secs(60),
        }],
    );
    mgr.try_consume("a", 1);
    mgr.try_consume("b", 1);
    mgr.reset_all();
    assert_eq!(mgr.remaining("a"), u64::MAX);
    assert_eq!(mgr.remaining("b"), u64::MAX);
}

// ---------------------------------------------------------------------------
// Period reset
// ---------------------------------------------------------------------------

#[test]
fn period_expiry_restores_quota() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 1,
            period: Duration::from_millis(30),
        }],
    );
    mgr.try_consume("key", 1);
    assert!(matches!(
        mgr.try_consume("key", 1),
        QuotaResult::Exhausted { .. }
    ));
    std::thread::sleep(Duration::from_millis(50));
    assert!(matches!(
        mgr.try_consume("key", 1),
        QuotaResult::Allowed { .. }
    ));
}

// ---------------------------------------------------------------------------
// has_limits
// ---------------------------------------------------------------------------

#[test]
fn has_limits_explicit_and_defaults() {
    let mgr = QuotaManager::new();
    assert!(!mgr.has_limits("any"));
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 10,
            period: Duration::from_secs(60),
        }],
    );
    assert!(mgr.has_limits("key"));
    mgr.set_default_limits(vec![QuotaLimit {
        limit: 5,
        period: Duration::from_secs(60),
    }]);
    assert!(mgr.has_limits("unknown-key"));
}

// ---------------------------------------------------------------------------
// Exhausted retry_after
// ---------------------------------------------------------------------------

#[test]
fn exhausted_retry_after_is_positive() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 1,
            period: Duration::from_secs(60),
        }],
    );
    mgr.try_consume("key", 1);
    if let QuotaResult::Exhausted { retry_after } = mgr.try_consume("key", 1) {
        assert!(retry_after > Duration::ZERO);
    } else {
        panic!("expected Exhausted");
    }
}

// ---------------------------------------------------------------------------
// Zero-unit consume
// ---------------------------------------------------------------------------

#[test]
fn zero_consume_with_zero_limit() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 0,
            period: Duration::from_secs(60),
        }],
    );
    assert!(matches!(
        mgr.try_consume("key", 0),
        QuotaResult::Allowed { remaining: 0 }
    ));
}

// ---------------------------------------------------------------------------
// Independent keys
// ---------------------------------------------------------------------------

#[test]
fn separate_keys_have_independent_quotas() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "a",
        vec![QuotaLimit {
            limit: 5,
            period: Duration::from_secs(60),
        }],
    );
    mgr.set_limits(
        "b",
        vec![QuotaLimit {
            limit: 5,
            period: Duration::from_secs(60),
        }],
    );
    mgr.try_consume("a", 5);
    assert!(matches!(
        mgr.try_consume("b", 1),
        QuotaResult::Allowed { .. }
    ));
}

// ---------------------------------------------------------------------------
// Clone / Default
// ---------------------------------------------------------------------------

#[test]
fn clone_shares_state() {
    let mgr = QuotaManager::new();
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 3,
            period: Duration::from_secs(60),
        }],
    );
    let clone = mgr.clone();
    mgr.try_consume("key", 2);
    assert_eq!(clone.remaining("key"), 1);
}

#[test]
fn default_impl() {
    let mgr = QuotaManager::default();
    assert!(!mgr.has_limits("x"));
}

// ---------------------------------------------------------------------------
// Concurrent access
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_consume_respects_limits() {
    let mgr = Arc::new(QuotaManager::new());
    mgr.set_limits(
        "key",
        vec![QuotaLimit {
            limit: 50,
            period: Duration::from_secs(60),
        }],
    );
    let mut handles = Vec::new();
    for _ in 0..10 {
        let m = Arc::clone(&mgr);
        handles.push(tokio::spawn(async move {
            let mut allowed = 0u64;
            for _ in 0..10 {
                if matches!(m.try_consume("key", 1), QuotaResult::Allowed { .. }) {
                    allowed += 1;
                }
            }
            allowed
        }));
    }
    let mut total = 0u64;
    for h in handles {
        total += h.await.unwrap();
    }
    assert_eq!(total, 50);
}
