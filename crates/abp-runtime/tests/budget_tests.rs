// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the budget enforcement module.

use abp_runtime::budget::*;
use std::time::Duration;

// ─── BudgetLimit construction ───────────────────────────────────────

#[test]
fn default_limit_is_unlimited() {
    let lim = BudgetLimit::default();
    assert!(lim.max_tokens.is_none());
    assert!(lim.max_cost_usd.is_none());
    assert!(lim.max_turns.is_none());
    assert!(lim.max_duration.is_none());
}

#[test]
fn limit_round_trips_through_json() {
    let lim = BudgetLimit {
        max_tokens: Some(4096),
        max_cost_usd: Some(0.25),
        max_turns: Some(10),
        max_duration: Some(Duration::from_secs(60)),
    };
    let json = serde_json::to_string(&lim).unwrap();
    let back: BudgetLimit = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_tokens, Some(4096));
    assert_eq!(back.max_cost_usd, Some(0.25));
    assert_eq!(back.max_turns, Some(10));
    assert_eq!(back.max_duration, Some(Duration::from_secs(60)));
}

// ─── WithinLimits ───────────────────────────────────────────────────

#[test]
fn within_limits_no_caps_set() {
    let t = BudgetTracker::new(BudgetLimit::default());
    t.record_tokens(1_000_000);
    t.record_cost(999.0);
    for _ in 0..100 {
        t.record_turn();
    }
    assert_eq!(t.check(), BudgetStatus::WithinLimits);
}

#[test]
fn within_limits_below_all_caps() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        max_cost_usd: Some(1.0),
        max_turns: Some(10),
        max_duration: Some(Duration::from_secs(60)),
    });
    t.start_timer();
    t.record_tokens(100);
    t.record_cost(0.1);
    t.record_turn();
    assert_eq!(t.check(), BudgetStatus::WithinLimits);
}

// ─── Exceeded ───────────────────────────────────────────────────────

#[test]
fn tokens_exceeded_exact_boundary() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        ..Default::default()
    });
    // At the boundary: 100 out of 100 → not exceeded (Warning at 80%+)
    t.record_tokens(100);
    assert!(!matches!(t.check(), BudgetStatus::Exceeded(_)));
    // One more → exceeded
    t.record_tokens(1);
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::TokensExceeded {
            used: 101,
            limit: 100
        })
    ));
}

#[test]
fn cost_exceeded_from_increments() {
    let t = BudgetTracker::new(BudgetLimit {
        max_cost_usd: Some(0.50),
        ..Default::default()
    });
    t.record_cost(0.3);
    t.record_cost(0.25);
    match t.check() {
        BudgetStatus::Exceeded(BudgetViolation::CostExceeded { used, limit }) => {
            assert!(used > 0.50);
            assert!((limit - 0.50).abs() < f64::EPSILON);
        }
        other => panic!("expected CostExceeded, got {other:?}"),
    }
}

#[test]
fn turns_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_turns: Some(5),
        ..Default::default()
    });
    for _ in 0..6 {
        t.record_turn();
    }
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::TurnsExceeded { used: 6, limit: 5 })
    ));
}

#[test]
fn duration_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_duration: Some(Duration::from_millis(1)),
        ..Default::default()
    });
    t.start_timer();
    std::thread::sleep(Duration::from_millis(15));
    match t.check() {
        BudgetStatus::Exceeded(BudgetViolation::DurationExceeded { elapsed, limit }) => {
            assert!(elapsed > Duration::from_millis(1));
            assert_eq!(limit, Duration::from_millis(1));
        }
        other => panic!("expected DurationExceeded, got {other:?}"),
    }
}

// ─── Warning ────────────────────────────────────────────────────────

#[test]
fn warning_at_80_percent_tokens() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        ..Default::default()
    });
    t.record_tokens(80);
    match t.check() {
        BudgetStatus::Warning { usage_pct } => {
            assert!(usage_pct >= 80.0, "expected >=80, got {usage_pct}");
        }
        other => panic!("expected Warning, got {other:?}"),
    }
}

#[test]
fn warning_at_90_percent_cost() {
    let t = BudgetTracker::new(BudgetLimit {
        max_cost_usd: Some(1.0),
        ..Default::default()
    });
    t.record_cost(0.9);
    match t.check() {
        BudgetStatus::Warning { usage_pct } => {
            assert!(usage_pct >= 89.0, "expected >=89, got {usage_pct}");
        }
        other => panic!("expected Warning, got {other:?}"),
    }
}

#[test]
fn no_warning_below_threshold() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        max_cost_usd: Some(1.0),
        max_turns: Some(10),
        ..Default::default()
    });
    t.record_tokens(79);
    t.record_cost(0.79);
    for _ in 0..7 {
        t.record_turn();
    }
    assert_eq!(t.check(), BudgetStatus::WithinLimits);
}

// ─── Remaining ──────────────────────────────────────────────────────

#[test]
fn remaining_decreases_as_usage_grows() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        max_cost_usd: Some(2.0),
        max_turns: Some(20),
        ..Default::default()
    });
    t.record_tokens(250);
    t.record_cost(0.5);
    for _ in 0..5 {
        t.record_turn();
    }
    let r = t.remaining();
    assert_eq!(r.tokens, Some(750));
    assert!((r.cost_usd.unwrap() - 1.5).abs() < 0.01);
    assert_eq!(r.turns, Some(15));
}

#[test]
fn remaining_clamps_to_zero_when_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        max_turns: Some(3),
        ..Default::default()
    });
    t.record_tokens(200);
    for _ in 0..5 {
        t.record_turn();
    }
    let r = t.remaining();
    assert_eq!(r.tokens, Some(0));
    assert_eq!(r.turns, Some(0));
}

#[test]
fn remaining_is_none_for_unlimited() {
    let t = BudgetTracker::new(BudgetLimit::default());
    t.record_tokens(500);
    let r = t.remaining();
    assert!(r.tokens.is_none());
    assert!(r.cost_usd.is_none());
    assert!(r.turns.is_none());
    assert!(r.duration.is_none());
}

#[test]
fn remaining_duration_shrinks() {
    let t = BudgetTracker::new(BudgetLimit {
        max_duration: Some(Duration::from_secs(10)),
        ..Default::default()
    });
    t.start_timer();
    std::thread::sleep(Duration::from_millis(50));
    let r = t.remaining();
    let left = r.duration.unwrap();
    assert!(left < Duration::from_secs(10));
}

// ─── Display ────────────────────────────────────────────────────────

#[test]
fn violation_display_tokens() {
    let v = BudgetViolation::TokensExceeded {
        used: 200,
        limit: 100,
    };
    let s = v.to_string();
    assert!(s.contains("200"), "display should contain used value");
    assert!(s.contains("100"), "display should contain limit value");
}

#[test]
fn violation_display_cost() {
    let v = BudgetViolation::CostExceeded {
        used: 1.2345,
        limit: 1.0,
    };
    let s = v.to_string();
    assert!(s.contains("$1.2345"), "display: {s}");
    assert!(s.contains("$1.0000"), "display: {s}");
}

#[test]
fn violation_display_turns() {
    let v = BudgetViolation::TurnsExceeded { used: 6, limit: 5 };
    assert!(v.to_string().contains("6"));
    assert!(v.to_string().contains("5"));
}

#[test]
fn violation_display_duration() {
    let v = BudgetViolation::DurationExceeded {
        elapsed: Duration::from_secs(120),
        limit: Duration::from_secs(60),
    };
    let s = v.to_string();
    assert!(s.contains("120.0"), "display: {s}");
    assert!(s.contains("60.0"), "display: {s}");
}

// ─── Thread safety ──────────────────────────────────────────────────

#[test]
fn concurrent_recording_is_safe() {
    use std::sync::Arc;
    let t = Arc::new(BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100_000),
        max_cost_usd: Some(100.0),
        max_turns: Some(10_000),
        ..Default::default()
    }));
    t.start_timer();

    let mut handles = vec![];
    for _ in 0..8 {
        let t2 = Arc::clone(&t);
        handles.push(std::thread::spawn(move || {
            for _ in 0..1000 {
                t2.record_tokens(1);
                t2.record_cost(0.001);
                t2.record_turn();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let r = t.remaining();
    assert_eq!(r.tokens, Some(100_000 - 8_000));
    assert_eq!(r.turns, Some(10_000 - 8_000));
}

// ─── Edge cases ─────────────────────────────────────────────────────

#[test]
fn check_before_start_timer_does_not_panic() {
    let t = BudgetTracker::new(BudgetLimit {
        max_duration: Some(Duration::from_secs(5)),
        ..Default::default()
    });
    // Duration not started → should still be within limits, not panic.
    assert_eq!(t.check(), BudgetStatus::WithinLimits);
}

#[test]
fn zero_limit_immediately_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(0),
        ..Default::default()
    });
    t.record_tokens(1);
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::TokensExceeded { used: 1, limit: 0 })
    ));
}
